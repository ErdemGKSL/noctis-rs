# Type Design

Model the domain so the compiler rejects invalid states. Bugs you can't type can't ship.

**Contents**
1. Enums instead of strings
2. Enums with data
3. Converting enums to and from strings (`Display`, `FromStr`, `strum`)
4. Newtypes with private fields
5. `Option` instead of sentinel values
6. Enums instead of `bool` flags
7. Typestate: encode the lifecycle in the type
8. Builders
9. `#[non_exhaustive]`
10. `NonZero*` and other niche types
11. `PhantomData` markers and units
12. Primitive obsession: a checklist

---

## 1. Enums instead of strings

**Why:** a `String` field accepts every typo, and the compiler can't tell you when you
forgot to handle a case. An enum is exhaustive: add a variant and every `match` that
doesn't cover it stops compiling. It's also smaller (one byte rather than 24 plus a heap
allocation) and cheaper to compare.

```rust
// ❌ any string is "valid"; "Admin", "admin ", "amdin" all compile
struct User { role: String }
fn can_delete(u: &User) -> bool { u.role == "admin" || u.role == "moderator" }
```

```rust
// ✅ the set of roles is closed and checked
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role { Admin, Moderator, Member }

pub struct User { pub role: Role }

pub fn can_delete(u: &User) -> bool {
    matches!(u.role, Role::Admin | Role::Moderator)
}
```

Use a string only when the set really is open (user-chosen names, free-form tags). If the
set is *mostly* closed but must tolerate unknown values from outside, keep an escape
hatch variant instead of falling back to `String` everywhere:

```rust
pub enum Channel { Stable, Beta, Nightly, Other(String) }
```

**Pitfall:** don't write `_ => …` wildcard arms against your own enums unless you mean
"every future variant too". A wildcard silences the exhaustiveness check that made the
enum valuable.

## 2. Enums with data

**Why:** when different cases carry different data, a struct full of `Option`s lets you
build nonsense combinations. An enum with data makes each case carry exactly what it
needs.

```rust
// ❌ which fields are set for which kind? Nothing stops `kind: "card"` with an iban.
struct Payment { kind: String, card_number: Option<String>, iban: Option<String> }
```

```rust
// ✅ each variant is self-describing
pub enum Payment {
    Card { number: String, expiry: (u8, u16) },
    BankTransfer { iban: String },
    Cash,
}

pub fn describe(p: &Payment) -> String {
    match p {
        Payment::Card { number, .. } => format!("card ending {}", &number[number.len().saturating_sub(4)..]),
        Payment::BankTransfer { iban } => format!("transfer from {iban}"),
        Payment::Cash => "cash".to_owned(),
    }
}
```

If a variant's payload grows large or is reused elsewhere, give it its own struct
(`Card(CardDetails)`), which you can pass around and implement methods on. If one variant
is much bigger than the others, `Box` it so the whole enum doesn't pay for it
(clippy's `large_enum_variant` will tell you).

## 3. Converting enums to and from strings

You still need strings at the edges (config files, CLI, DB columns, logs). Do the
conversion in exactly one place, with standard traits, so the rest of the code never
sees the string.

**Hand-written** (no dependency, fine for small enums):

```rust
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level { Debug, Info, Warn }

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self { Level::Debug => "debug", Level::Info => "info", Level::Warn => "warn" }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown level: {0:?}")]
pub struct ParseLevelError(String);

impl FromStr for Level {
    type Err = ParseLevelError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "debug" => Ok(Level::Debug),
            "info" => Ok(Level::Info),
            "warn" => Ok(Level::Warn),
            _ => Err(ParseLevelError(s.to_owned())),
        }
    }
}
```

**With `strum`** (recommended once you have several enums or need iteration):

```rust
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, EnumIter)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum Region { EuWest, UsEast, ApSouth }

// "eu_west".parse::<Region>()?, region.to_string(), <&'static str>::from(region),
// Region::iter() for listing all values in help text.
```

If the enum is also (de)serialized, use serde's `rename_all` (see `serde.md`) and keep
the spellings identical to strum's, or derive `Display`/`FromStr` from one and
`#[serde(into/try_from)]` from the other so there's a single source of truth.

**Pitfall:** don't use `{:?}` (`Debug`) as the user-facing or persisted spelling. `Debug`
output is not a stable format, and renaming a variant silently changes stored data.

## 4. Newtypes with private fields

**Why:** a `String` called `email` is still just a `String`; nothing stops you passing a
username where an email was expected, or storing an unvalidated one. A newtype with a
private field and a validating constructor guarantees every instance is valid, and the
check runs once, at the boundary ("parse, don't validate").

```rust
// ❌ validation is a convention; anyone can skip it, and arguments are swappable
fn invite(email: String, username: String) { /* … */ }
```

```rust
// ✅
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Email(String); // field is private: the only way in is `parse`/`TryFrom`

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("email is empty")]
    Empty,
    #[error("email {0:?} has no '@'")]
    MissingAt(String),
}

impl Email {
    pub fn parse(s: impl Into<String>) -> Result<Self, EmailError> {
        let s = s.into();
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(EmailError::Empty);
        }
        if !trimmed.contains('@') {
            return Err(EmailError::MissingAt(s));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl TryFrom<String> for Email {
    type Error = EmailError;
    fn try_from(s: String) -> Result<Self, Self::Error> { Email::parse(s) }
}

impl std::str::FromStr for Email {
    type Err = EmailError;
    fn from_str(s: &str) -> Result<Self, Self::Err> { Email::parse(s) }
}

impl AsRef<str> for Email {
    fn as_ref(&self) -> &str { &self.0 }
}

impl std::fmt::Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}
```

Guidelines:
- Give read access with `as_str()`/`AsRef`/`Display`, and an `into_inner()` if callers
  need the owned value back. Don't implement `DerefMut` or expose `&mut` to the inside;
  that would let callers break the invariant.
- Plain ID newtypes with no invariant (`struct UserId(u64)`) can have a `pub` field or a
  `From<u64>`: their job is only to stop you mixing up `UserId` and `OrderId`.
- With serde, `#[serde(try_from = "String", into = "String")]` makes deserialization go
  through the validating constructor (see `serde.md`).

## 5. `Option` instead of sentinel values

**Why:** `-1`, `0`, `""`, and `u64::MAX` as "no value" are invisible to the type system.
Someone will eventually do arithmetic on them. `Option<T>` forces every reader to handle
absence, and for many types it costs no extra space (`Option<Box<T>>`, `Option<&T>`,
`Option<NonZeroU32>` are pointer/integer sized).

```rust
// ❌
struct Config { timeout_secs: i64 } // -1 means "no timeout"
```

```rust
// ✅
use std::time::Duration;
struct Config { timeout: Option<Duration> }
```

Prefer `Option` combinators and `let … else` over `is_some()` + `unwrap()`:

```rust
fn first_word_len(s: Option<&str>) -> usize {
    let Some(s) = s else { return 0 };
    s.split_whitespace().next().map_or(0, str::len)
}
```

When "absent" has more than one meaning (not loaded yet vs. explicitly empty vs. failed),
`Option` is too weak: use an enum that names each state.

## 6. Enums instead of `bool` flags

**Why:** `render(true, false, true)` is unreadable at the call site and trivially
swappable. Two-variant enums cost nothing at runtime and document themselves.

```rust
// ❌
fn export(path: &str, overwrite: bool, compress: bool) { /* … */ }
// export("out.csv", true, false) — which is which?
```

```rust
// ✅
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite { Yes, No }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression { #[default] None, Gzip, Zstd }

pub fn export(path: &str, overwrite: Overwrite, compression: Compression) { /* … */ }
// export("out.csv", Overwrite::Yes, Compression::None)
```

When a function grows more than ~3 options, pass an options struct with `Default` so
callers can write `ExportOptions { compression: Compression::Gzip, ..Default::default() }`.

A single `bool` with an obvious name (`set_visible(bool)`) is fine; the problem is
positional bools that need a comment to decode.

## 7. Typestate: encode the lifecycle in the type

**Why:** if an object must go through steps in order (unconnected → connected →
authenticated), runtime checks like `if !self.connected { return Err(…) }` are easy to
forget. With typestate, calling a method in the wrong state doesn't compile.

```rust
use std::marker::PhantomData;

pub struct Disconnected;
pub struct Connected;
pub struct Authenticated;

pub struct Client<State> {
    addr: String,
    _state: PhantomData<State>,
}

impl Client<Disconnected> {
    pub fn new(addr: impl Into<String>) -> Self {
        Client { addr: addr.into(), _state: PhantomData }
    }
    pub fn connect(self) -> std::io::Result<Client<Connected>> {
        // … open socket …
        Ok(Client { addr: self.addr, _state: PhantomData })
    }
}

impl Client<Connected> {
    pub fn login(self, _token: &str) -> Result<Client<Authenticated>, String> {
        Ok(Client { addr: self.addr, _state: PhantomData })
    }
}

impl Client<Authenticated> {
    pub fn send(&self, msg: &str) { println!("{} <- {msg}", self.addr); }
}

// Client::new("db:5432").send("hi");  // ❌ does not compile: no `send` on Disconnected
```

Methods that take `self` by value consume the old state, so a stale handle can't be
reused. Use typestate for protocols and builders with required steps; skip it when the
state is only known at runtime (e.g., it comes from a server reply). A plain enum
`state` field is the right tool there.

## 8. Builders

**Why:** Rust has no default or named arguments. For a type with many optional settings,
a builder keeps construction readable and lets you validate once in `build()`.

```rust
use std::time::Duration;

#[derive(Debug)]
pub struct HttpClient { base_url: String, timeout: Duration, retries: u32, user_agent: String }

#[derive(Debug, Default)]
#[must_use]
pub struct HttpClientBuilder { base_url: Option<String>, timeout: Option<Duration>, retries: u32, user_agent: Option<String> }

impl HttpClient {
    pub fn builder() -> HttpClientBuilder { HttpClientBuilder::default() }
}

impl HttpClientBuilder {
    pub fn base_url(mut self, url: impl Into<String>) -> Self { self.base_url = Some(url.into()); self }
    pub fn timeout(mut self, t: Duration) -> Self { self.timeout = Some(t); self }
    pub fn retries(mut self, n: u32) -> Self { self.retries = n; self }

    pub fn build(self) -> Result<HttpClient, &'static str> {
        Ok(HttpClient {
            base_url: self.base_url.ok_or("base_url is required")?,
            timeout: self.timeout.unwrap_or(Duration::from_secs(30)),
            retries: self.retries,
            user_agent: self.user_agent.unwrap_or_else(|| "my-app/1.0".into()),
        })
    }
}

// let c = HttpClient::builder().base_url("https://api").retries(3).build()?;
```

Notes:
- By-value `self` builders chain nicely and work for one-shot construction; use
  `&mut self` builders when callers build conditionally in a loop.
- `#[must_use]` on the builder catches `builder.retries(3);` whose result is dropped.
- For a struct whose fields are all optional, `Default` + struct-update syntax is often
  enough; no builder needed.
- `bon` or `derive_builder` generate builders (bon can even enforce required fields at
  compile time). Reach for them when you have many builders.
- If a field is mandatory, make it a parameter of `builder(base_url)` instead of a
  runtime error in `build()`.

## 9. `#[non_exhaustive]`

**Why:** in a library, adding an enum variant or a struct field is normally a breaking
change, because downstream code matches exhaustively or uses struct literals.
`#[non_exhaustive]` forces downstream crates to add a wildcard arm / use a constructor,
so you can extend the type in a minor release.

```rust
#[non_exhaustive]
#[derive(Debug)]
pub enum Event { Created, Deleted }

#[non_exhaustive]
pub struct Limits { pub max_conns: u32 }
impl Limits {
    pub fn new(max_conns: u32) -> Self { Self { max_conns } }
}
```

Apply it to public error enums and config structs that are likely to grow. Don't apply
it inside a binary or to internal types: within the defining crate it has no effect, and
it only takes away exhaustiveness checking from your users.

## 10. `NonZero*` and other niche types

`std::num::NonZero<u32>` (alias `NonZeroU32`) states "never zero" in the type and gives
`Option<NonZero<u32>>` the same size as `u32`. Good for counts, IDs, page sizes, worker
counts:

```rust
use std::num::NonZeroUsize;

pub fn chunk(data: &[u8], size: NonZeroUsize) -> impl Iterator<Item = &[u8]> {
    data.chunks(size.get()) // `chunks(0)` would panic; the type rules that out
}
```

Related standard types that carry meaning: `Duration` instead of `u64` seconds,
`PathBuf`/`&Path` instead of `String` for paths, `IpAddr`/`SocketAddr` instead of
strings, `std::time::Instant` for elapsed time.

## 11. `PhantomData` markers and units

`PhantomData<T>` lets a type carry a compile-time tag without storing a `T`. Typestate
(section 7) is one use; typed IDs and units are others:

```rust
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id<T> { raw: u64, _marker: PhantomData<fn() -> T> }

impl<T> Id<T> {
    pub const fn new(raw: u64) -> Self { Self { raw, _marker: PhantomData } }
    pub const fn get(self) -> u64 { self.raw }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct User;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Order;

pub fn load_order(_id: Id<Order>) {}
// load_order(Id::<User>::new(1)); // ❌ mismatched types
```

`PhantomData<fn() -> T>` keeps `Id<T>` `Send + Sync` and covariant regardless of `T`.
`PhantomData<T>` would make it inherit `T`'s auto traits, which is wrong for a pure tag.

**Pitfall:** `#[derive(Clone, Copy, PartialEq…)]` on a generic struct adds `T: Clone`
etc. bounds even when `T` is only phantom. That's why the marker types above derive the
same traits; without that, `Id<User>` wouldn't be `Copy`. For unit structs it's free.
The alternative is implementing the traits manually without the bound.

For physical units, the same pattern (`Quantity<Meters>`) works, but `uom` already
exists if you need real dimensional analysis.

## 12. Primitive obsession: a checklist

When reviewing a struct or function signature, ask for each `String`, integer, and
`bool`:

- Is the set of values closed? → enum.
- Does it have a validity rule (non-empty, range, format)? → newtype with a fallible
  constructor.
- Could it be confused with another parameter of the same primitive type? → newtype ID.
- Does it represent "maybe absent"? → `Option`, not a sentinel.
- Is it a duration, path, address, or timestamp? → the standard type for it.
- Is it a flag at a call site? → two-variant enum or options struct.
- Do several fields only make sense together, or only in certain cases? → group them in
  a struct, or split the type into an enum with data.

You don't need a newtype for every integer; the cost is boilerplate. Introduce one where
a mix-up or an invalid value is plausible and would be expensive.
