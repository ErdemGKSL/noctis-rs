# Macros and Removing Repetition

When and how to use `macro_rules!`, derives and procedural macros to eliminate repetitive code without making it unreadable.

## Contents

1. [The decision ladder](#1-the-decision-ladder)
2. [Patterns where `macro_rules!` shines](#2-patterns-where-macro_rules-shines)
3. [`macro_rules!` mechanics that matter](#3-macro_rules-mechanics-that-matter)
4. [Exporting and organizing macros](#4-exporting-and-organizing-macros)
5. [Ecosystem derives worth knowing](#5-ecosystem-derives-worth-knowing)
6. [Procedural macros: when they're worth it](#6-procedural-macros-when-theyre-worth-it)
7. [Debugging macros](#7-debugging-macros)
8. [Anti-patterns](#8-anti-patterns)

---

## 1. The decision ladder

Climb only as high as you need. Each rung is harder to read, debug and get good compiler
errors from than the one before.

1. **A function** — repetition of *behavior* with different values.
2. **A generic function / type** — the same behavior over different types.
3. **A trait + blanket impl or default methods** — the same capability for many types
   (`impl<T: Display> Describe for T`).
4. **A derive** (std or ecosystem) — boilerplate impls that follow the shape of a type.
5. **`macro_rules!`** — repetition that is *syntactic*: many near-identical `impl` blocks,
   tables that must stay in sync, items that can't be abstracted with generics.
6. **A procedural macro** — custom derives/attributes that need to inspect arbitrary type
   definitions. Costs a separate crate and compile time.

**Why:** generics and traits are type-checked once, at definition; macros are expanded
first and type-checked at every call site, so errors are worse and IDE support is weaker.
Use macros for what the type system can't express, not as a shortcut around designing a trait.

## 2. Patterns where `macro_rules!` shines

### 2.1 Many near-identical trait impls

```rust
// ❌ copy-paste: fix a bug in one, forget the other five
impl Zero for u8  { fn zero() -> Self { 0 } }
impl Zero for u16 { fn zero() -> Self { 0 } }
impl Zero for u32 { fn zero() -> Self { 0 } }
// ...
```

```rust
// ✅ one definition, a list of types
pub trait Zero {
    fn zero() -> Self;
}

macro_rules! impl_zero {
    ($($t:ty),+ $(,)?) => {
        $(
            impl Zero for $t {
                fn zero() -> Self { 0 as $t }
            }
        )+
    };
}

impl_zero!(u8, u16, u32, u64, i8, i16, i32, i64, usize, isize);
```

(Prefer a blanket impl when there's a trait that captures the capability — e.g. with
`num-traits` you'd write `impl<T: num_traits::Zero> …`. The macro is for when there isn't.)

### 2.2 Enum ↔ string tables that must stay in sync

```rust
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident { $($variant:ident => $text:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        $vis enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::core::str::FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    other => Err(format!("unknown {}: {other}", stringify!($name))),
                }
            }
        }
    };
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Region { UsEast => "us-east-1", EuWest => "eu-west-1", ApSouth => "ap-south-1" }
}
```

Adding a variant now updates `as_str`, `Display`, `FromStr` and `ALL` in one place. (For
this exact job `strum` derives exist — see §5. Hand-roll only when you need to avoid the
dependency or need a shape strum doesn't support.)

### 2.3 Newtype generation

```rust
macro_rules! id_newtype {
    ($($(#[$m:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(raw: u64) -> Self { Self(raw) }
            pub const fn get(self) -> u64 { self.0 }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }
    )+};
}

id_newtype! {
    /// Identifies a user.
    UserId,
    /// Identifies a guild.
    GuildId,
}
```

`UserId` and `GuildId` can no longer be mixed up, and the boilerplate lives in one place.

### 2.4 Test-case matrices

```rust
macro_rules! parse_tests {
    ($($name:ident: $input:expr => $expected:expr;)+) => {$(
        #[test]
        fn $name() {
            assert_eq!(crate::parse_duration($input), $expected);
        }
    )+};
}

#[cfg(test)]
mod tests {
    parse_tests! {
        seconds:   "5s"  => Some(5);
        minutes:   "2m"  => Some(120);
        empty:     ""    => None;
        garbage:   "abc" => None;
    }
}
```

Each case is a separately named test, so failures point at the exact case. (`rstest`'s
`#[case]` attribute is the dependency-based alternative.)

## 3. `macro_rules!` mechanics that matter

### Fragment specifiers

| Specifier | Matches | Typical use |
| --- | --- | --- |
| `ident` | identifier | names of generated items |
| `ty` | type | impl targets |
| `expr` | expression | values |
| `literal` | literal | strings/numbers in tables |
| `pat` | pattern (incl. `\|` in 2021+) | match arms |
| `path` | path like `a::b::C` | trait paths |
| `tt` | single token tree | anything, for forwarding |
| `block`, `stmt`, `item`, `meta`, `vis`, `lifetime` | as named | attributes (`meta`), visibility (`vis`) |

Prefer the most specific specifier: it gives better error messages and constrains what
callers can pass. Once captured as `expr`/`ty`, a fragment is opaque — you can't match into
it again (only `ident`, `lifetime`, `tt` stay re-matchable).

### Repetitions and trailing commas

- `$( ... ),*` zero or more, comma-separated; `+` one or more; `?` optional.
- Accept a trailing comma with `$(,)?` — callers (and rustfmt-formatted code) will write one.
- Every `$var` used inside `$( ... )*` in the expansion must have been captured under a
  repetition of the same depth.

### Internal rules

Use `@`-prefixed arms for helper steps; put them **before** the public arm so they match first:

```rust
macro_rules! count {
    (@one $_t:tt) => { 1usize };
    ($($t:tt)*) => { 0usize $(+ count!(@one $t))* };
}

const N: usize = count!(a b c);
```

### `$crate` and absolute paths

Inside exported macros, refer to your own crate's items as `$crate::path::Item` and to std
as `::core::…` / `::std::…`. The caller might have a local module named `fmt` or no `use`
of your types at all.

### Hygiene

Local variables introduced inside a macro don't clash with the caller's variables, but
**items** (fns, structs, impls) and identifiers passed in *by the caller* are unhygienic —
generated item names live in the caller's namespace. If a macro must create a name, take it
as an `ident` parameter instead of inventing one.

### Keep expansions small

Put the logic in a normal (generic) function and have the macro just call it. The function
is compiled once and type-checked properly; the macro is a thin, readable shim:

```rust
#[doc(hidden)]
pub fn __check_impl(cond: bool, msg: &str) -> Result<(), String> {
    if cond { Ok(()) } else { Err(msg.to_owned()) }
}

#[macro_export]
macro_rules! check {
    ($cond:expr, $($fmt:tt)+) => {
        $crate::__check_impl($cond, &::std::format!($($fmt)+))
    };
}
```

## 4. Exporting and organizing macros

- **Crate-internal macro** (preferred default): define it, then re-export with a path so it
  behaves like any other item:

  ```rust
  macro_rules! internal_helper { () => {} }
  pub(crate) use internal_helper; // now usable as crate::module::internal_helper!
  ```

  Textual order still matters for `macro_rules!` defined in the same file — define before use.

- **Public macro:** `#[macro_export]` places it at the **crate root** (`my_crate::my_macro!`)
  regardless of the module it's in. Document it like a function, with an example, and use
  `$crate::` for every path.
- Avoid the old `#[macro_use] extern crate`; import macros with `use` like other items.
- Document every macro with the syntax it accepts and an example — readers can't hover to
  see a signature.

## 5. Ecosystem derives worth knowing

Before writing a macro, check whether a well-maintained derive already does the job:

| Crate | Derives / what it removes |
| --- | --- |
| `serde` | `Serialize`, `Deserialize` — all (de)serialization code (see `serde.md`) |
| `thiserror` | `Error` — `Display`, `Error::source`, `From` for error enums |
| `strum` | `EnumString`, `Display`, `AsRefStr`, `EnumIter`, `IntoStaticStr`, `EnumCount` — enum ↔ string, iterate variants |
| `derive_more` | `From`, `Into`, `Deref`, `Display`, `Add`, `Constructor`, `AsRef` — newtype & arithmetic boilerplate |
| `bon` / `typed-builder` | compile-time-checked builders for structs and functions |
| `num_enum` | `TryFromPrimitive`, `IntoPrimitive` — enum ↔ integer |
| `bitflags` (macro) | type-safe bit flag sets |

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::Display, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum Color { Red, DarkBlue }

// "dark_blue".parse::<Color>() == Ok(Color::DarkBlue); Color::DarkBlue.to_string() == "dark_blue"
```

And don't forget std derives: `Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
Default` (including `#[default]` on an enum variant).

## 6. Procedural macros: when they're worth it

Write a proc macro when:

- the generated code depends on a type's **fields/variants** (a custom derive), or
- you need an attribute that rewrites a function, and
- the pattern appears often enough (many types, or used by many downstream users) to pay
  for a separate `proc-macro = true` crate and the compile-time cost of `syn`.

Minimal derive skeleton:

```rust
// my-derive/src/lib.rs   (Cargo.toml: [lib] proc-macro = true; deps: syn, quote, proc-macro2)
use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(Describe)]
pub fn derive_describe(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::my_crate::Describe for #name #ty_generics #where_clause {
            fn describe() -> &'static str { stringify!(#name) }
        }
    }
    .into()
}
```

Practices:

- Always handle generics via `split_for_impl()`.
- Report errors with `syn::Error::new_spanned(node, "msg").to_compile_error()` instead of
  panicking — the user gets a squiggle on the right token.
- Parse helper attributes (`#[my(rename = "x")]`) with `darling` instead of hand-walking
  `syn::Meta`.
- Re-export the derive from the main crate (`pub use my_derive::Describe;`) so users add one
  dependency.
- Test expansions with `trybuild` (compile-pass / compile-fail cases) and/or `macrotest`.

## 7. Debugging macros

- `cargo expand` (install `cargo-expand`) shows the fully expanded code — the first thing to
  run when a macro misbehaves. `cargo expand path::to::module` narrows it.
- `stringify!($x)` / `compile_error!("...")` inside arms helps locate which arm matched.
- When a call fails to match, the error points to the token that no arm accepts; check
  separators and trailing commas first.

## 8. Anti-patterns

- **Macro instead of a function.** `macro_rules! add { ($a:expr, $b:expr) => { $a + $b } }`
  gains nothing and loses type checking and IDE help.
- **DSLs nobody can read.** A macro with custom syntax is a mini-language your teammates
  must learn. Keep the syntax close to Rust (`name: Type`, `Variant => "text"`).
- **Evaluating an argument twice.** `($x:expr) => { $x + $x }` runs side effects twice;
  bind it first: `{ let v = $x; v + v }`.
- **Unqualified paths** in exported macros (`Vec`, `format!`, `Display`) break when the
  caller shadows them or is `no_std`; use `::std::vec::Vec`, `$crate::…`.
- **Huge expansions** at many call sites bloat compile times; move logic into functions.
- **Hiding control flow.** A macro that secretly `return`s or `?`s surprises readers; if it
  must, name it so (e.g. `try_parse!`) and document it.
- **Proc macro for a one-off.** If three types need an impl, `macro_rules!` or hand-written
  code is cheaper than a new crate.
