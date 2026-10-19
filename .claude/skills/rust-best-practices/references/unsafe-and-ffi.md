# Unsafe and FFI

How to keep `unsafe` rare, small, justified and tested — and how to cross the C boundary without undefined behavior.

**Contents**
1. Default to no `unsafe`
2. What `unsafe` actually means
3. Minimal blocks with `// SAFETY:` comments
4. Safe abstractions over unsafe code
5. Edition 2024 changes
6. Raw pointers and `MaybeUninit`
7. `unsafe impl Send`/`Sync`
8. Testing with Miri
9. FFI: layout, strings, ownership, panics
10. Tooling: `bindgen`, `cbindgen`, `cxx`
11. Pitfalls

---

## 1. Default to no `unsafe`

Almost all application code — services, CLIs, bots — needs zero `unsafe`. If you think
you need it for performance or to "get around" the borrow checker, first look for a
safe pattern (indices instead of references, `split_at_mut`, `Cell`, `std::mem::take`,
a crate that already encapsulates it).

Make the decision explicit at the crate root:

```rust
#![forbid(unsafe_code)] // lib.rs / main.rs: no unsafe anywhere in this crate
```

Or in `Cargo.toml` with `[lints.rust] unsafe_code = "forbid"`. `forbid` cannot be
overridden by inner `#[allow]`; use `deny` if one module legitimately needs an exception.

## 2. What `unsafe` actually means

`unsafe` does **not** turn off the borrow checker or type checking. It unlocks five
things: dereferencing raw pointers, calling `unsafe` functions (including FFI), accessing
or modifying `static mut`, implementing `unsafe` traits, and accessing union fields.

Inside an `unsafe` block *you* promise the compiler the invariants it can't check. If
the promise is wrong, the result is **undefined behavior** — not a crash you can catch,
but permission for the optimizer to miscompile anything. "It works in my test" proves
nothing about UB.

## 3. Minimal blocks with `// SAFETY:` comments

Keep each `unsafe` block as small as possible and precede it with a `// SAFETY:` comment
that explains *why each precondition holds here*. Unsafe functions document their
preconditions under a `# Safety` doc heading. Clippy enforces both with
`undocumented_unsafe_blocks` and `missing_safety_doc`.

```rust
// ❌ big block, no justification — reviewers can't tell what is being promised
unsafe {
    let len = v.len();
    let x = *v.get_unchecked(i);
    process(x, len);
}

// ✅ one operation, invariant stated next to it
assert!(i < v.len());
// SAFETY: `i < v.len()` was checked on the previous line.
let x = unsafe { *v.get_unchecked(i) };
process(x, v.len());
```

```rust
/// Returns the element at `index` without bounds checking.
///
/// # Safety
///
/// `index` must be less than `self.len()`.
pub unsafe fn get_unchecked(&self, index: usize) -> &T {
    // SAFETY: the caller guarantees `index < self.len()`.
    unsafe { self.data.get_unchecked(index) }
}
```

## 4. Safe abstractions over unsafe code

The goal is a **safe API whose implementation uses `unsafe` internally** such that *no*
sequence of safe calls can cause UB. That's how `Vec`, `String` and `split_at_mut` work.

- Put the unsafe code in a small private module; keep the fields that carry the
  invariant private so safe code in the crate can't break it.
- Validate in the constructor; then methods can rely on the invariant.
- Remember that safe code in the same module *can* break invariants (e.g. setting
  `len` past `capacity`). The module is the unit of trust.

```rust
/// A string guaranteed to contain only ASCII bytes.
pub struct AsciiString(Vec<u8>);

impl AsciiString {
    pub fn new(bytes: Vec<u8>) -> Option<Self> {
        bytes.is_ascii().then_some(Self(bytes))
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: the constructor only accepts ASCII, which is valid UTF-8, and the
        // field is private so it cannot be modified afterwards.
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}
```

Consider whether the unchecked variant is worth it at all: `std::str::from_utf8(&self.0)`
plus `expect` would cost one linear scan. Measure.

## 5. Edition 2024 changes

- **`unsafe_op_in_unsafe_fn` warns by default.** The body of an `unsafe fn` is no longer
  implicitly an unsafe block; wrap each unsafe operation in its own `unsafe {}` with a
  `SAFETY` comment (as in the `get_unchecked` example above). This keeps "this function
  has preconditions" separate from "this line relies on them".
- **`extern` blocks must be written `unsafe extern`**, and individual items can be marked
  `safe` when calling them can't cause UB:

```rust
unsafe extern "C" {
    pub safe fn abs(x: i32) -> i32;           // callable without `unsafe`
    pub unsafe fn strlen(s: *const std::ffi::c_char) -> usize;
}
```

- **Unsafe attributes** must be marked: `#[unsafe(no_mangle)]`,
  `#[unsafe(export_name = "...")]`, `#[unsafe(link_section = "...")]` — because a symbol
  clash can cause UB.
- **`std::env::set_var` / `remove_var` are `unsafe`** (they race with other threads
  reading the environment). Set env before spawning threads, or better, pass config
  explicitly.
- **References to `static mut` are denied** (`static_mut_refs`). Use atomics, `Mutex`,
  `OnceLock`/`LazyLock`, or `&raw const` / `&raw mut` for raw pointers.

## 6. Raw pointers and `MaybeUninit`

- Creating raw pointers is safe; dereferencing requires that the pointer is non-null,
  aligned, points to a live, initialized value of the right type, and that aliasing
  rules hold (no `&mut` coexisting with any other access).
- Use `&raw const x` / `&raw mut x` to create pointers without creating an intermediate
  reference (important for packed fields and uninitialized memory).
- Prefer `NonNull<T>` in data structures: it's non-null, covariant, and gives
  `Option<NonNull<T>>` the niche optimization.
- Never create a reference (`&T` / `&mut T`) to uninitialized memory — use
  `MaybeUninit<T>`. `std::mem::uninitialized()` and `mem::zeroed()` for types where zero
  isn't valid (references, `NonNull`, enums, `bool` patterns) are UB.

```rust
use std::mem::MaybeUninit;

fn squares() -> [u64; 4] {
    let mut arr = [MaybeUninit::<u64>::uninit(); 4];
    for (i, slot) in arr.iter_mut().enumerate() {
        slot.write((i as u64) * (i as u64));
    }
    // SAFETY: every element was initialized in the loop above.
    arr.map(|x| unsafe { x.assume_init() })
}
```

(Of course `std::array::from_fn(|i| (i * i) as u64)` does this safely — prefer it.)

## 7. `unsafe impl Send`/`Sync`

Types containing raw pointers are automatically `!Send` and `!Sync`. Implementing them
manually is a claim about thread safety the compiler takes on faith. Justify it:

```rust
pub struct Handle { ptr: std::ptr::NonNull<ffi::Ctx> }

// SAFETY: the C library documents that a `Ctx` may be used from any thread as long as
// calls are not concurrent; `Handle` is not `Clone` and all methods take `&mut self`,
// so there is never concurrent access. It is therefore `Send` but deliberately not `Sync`.
unsafe impl Send for Handle {}
```

If you can't write that comment with confidence, wrap the value in a `Mutex` or keep it
on one thread instead.

## 8. Testing with Miri

Miri is an interpreter that detects many kinds of UB: out-of-bounds access,
use-after-free, invalid values, data races, and aliasing violations (Stacked/Tree
Borrows). Run it on every crate that contains `unsafe`.

```sh
rustup +nightly component add miri
cargo +nightly miri test
```

Miri can't run FFI calls or most I/O; isolate the pure-Rust unsafe logic so it can be
tested. Complement with sanitizers (`-Zsanitizer=address`) and fuzzing for parsers.

## 9. FFI

**Layout.** Rust struct layout is unspecified. Anything passed to C must be
`#[repr(C)]` (or `#[repr(transparent)]` for single-field wrappers). C-like enums crossing
the boundary need `#[repr(C)]`/`#[repr(u32)]` — and since C can send *any* integer,
receive them as integers and convert with `TryFrom`, because an invalid enum value in
Rust is UB.

```rust
#[repr(C)]
pub struct Point { pub x: f64, pub y: f64 }
```

**Strings.** C strings are NUL-terminated byte arrays, not UTF-8 `&str`.

```rust
use std::ffi::{c_char, CStr, CString};

// Rust → C: CString adds the NUL and rejects interior NULs.
let name = CString::new("vesper")?;      // Err if the input contains '\0'
unsafe_c_fn(name.as_ptr());               // `name` must outlive the call

/// C → Rust: borrow, then convert (may not be valid UTF-8).
///
/// # Safety
///
/// `p` must be null or point to a valid NUL-terminated string that stays alive
/// and unmodified for the duration of this call.
unsafe fn from_c(p: *const c_char) -> Option<String> {
    if p.is_null() { return None; }
    // SAFETY: non-null was checked above; validity is guaranteed by the caller.
    let s = unsafe { CStr::from_ptr(p) };
    Some(s.to_string_lossy().into_owned())
}
```

Note that `from_c` must itself be `unsafe fn`: a *safe* function that dereferences a
raw pointer it was handed is unsound, because safe callers could pass any address.
Clippy's `not_unsafe_ptr_arg_deref` (deny by default) catches exactly this mistake.

Classic bug: `CString::new(s)?.as_ptr()` in one expression — the `CString` is dropped at
the end of the statement and the pointer dangles. Bind it to a variable first.

`c"literal"` gives a `&'static CStr` directly (Rust 1.77+).

**Ownership across the boundary.** Memory must be freed by the allocator that
allocated it. If Rust allocates and hands a pointer to C, export a matching free
function:

```rust
pub struct Parser { /* ... */ }

#[unsafe(no_mangle)]
pub extern "C" fn parser_new() -> *mut Parser {
    Box::into_raw(Box::new(Parser { /* ... */ }))
}

/// # Safety
/// `p` must come from `parser_new` and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn parser_free(p: *mut Parser) {
    if !p.is_null() {
        // SAFETY: guaranteed by the caller contract above.
        drop(unsafe { Box::from_raw(p) });
    }
}
```

**Panics.** Unwinding out of an `extern "C"` function aborts the process (since Rust
1.81). Catch panics at the boundary and convert them into an error code, so a bug
becomes a reported error rather than a crash of the host program:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn do_work(input: i32) -> i32 {
    match std::panic::catch_unwind(|| work(input)) {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => -1,  // domain error
        Err(_) => -2,      // panic
    }
}
```

(`catch_unwind` does nothing with `panic = "abort"`.) Use `extern "C-unwind"` only when
unwinding across the boundary is intended and supported by both sides.

**Wrap the raw API.** Expose a safe Rust module (`Handle` with `Drop`, methods returning
`Result`) on top of the `-sys` bindings and keep `unsafe` out of the rest of the code.

## 10. Tooling

- **`bindgen`** generates Rust declarations from C headers (usually in a `foo-sys`
  crate's `build.rs`). Review the output; pin the allowlist to what you use.
- **`cbindgen`** generates C headers from your `extern "C"` Rust API.
- **`cxx`** for C++: a safe, checked bridge — much less hand-written `unsafe` than raw
  FFI. **`pyo3`**, **`napi-rs`**, **`uniffi`** for Python, Node and mobile bindings.

## 11. Pitfalls

- Using `unsafe` to bypass a borrow error — the error was pointing at a real aliasing
  problem.
- `std::mem::transmute` between types without guaranteed layout; prefer `from_ne_bytes`,
  `bytemuck`/`zerocopy` for byte casts, `as` for numbers, `.cast()` for pointers.
- Assuming `&mut T` from two raw pointers to the same place is fine — it's UB even if
  never used concurrently.
- Lifetimes of pointers obtained from temporaries (`CString`, `Vec::as_ptr` on a
  temporary).
- Forgetting `Drop` for FFI handles (leaks) or double-free when a handle is `Clone`.
- Skipping Miri because "it's only a few lines of unsafe".
