# Serde Data Modeling

Describe wire formats declaratively with serde attributes instead of hand-written
conversion code.

**Contents**
1. Setup and the basic derive
2. Field naming: `rename_all`, `rename`, `alias`
3. Optional and defaulted fields
4. `flatten`
5. Enum representations (and how to choose)
6. Validated types: `try_from` / `from` / `into` / `transparent`
7. Strictness: `deny_unknown_fields`
8. Custom field logic: `with`, `serialize_with`, `deserialize_with`
9. Zero-copy deserialization: `&'a str` and `Cow`
10. `serde_json::Value` only at the edges
11. Dates, UUIDs, and other ecosystem types
12. Testing round-trips
13. Pitfalls

---

## 1. Setup and the basic derive

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"          # or toml, serde_yaml, etc.
```

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub roles: Vec<Role>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role { Admin, Member }
```

**Why derive and not hand-write:** the derive generates code that's correct, fast, and
consistent across every format serde supports (JSON, TOML, YAML, MessagePack, bincode…).
A hand-written `match` over `serde_json::Value` handles one format and drifts out of sync
with the struct.

Derive only what you need. A type that's only read from config needs just
`Deserialize`; deriving `Serialize` too is harmless but adds compile time in large
crates.

## 2. Field naming: `rename_all`, `rename`, `alias`

Keep Rust names idiomatic (`snake_case` fields, `CamelCase` variants) and map to the wire
format with attributes.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]          // userId, createdAt, …
pub struct ApiUser {
    pub user_id: u64,
    pub created_at: String,
    #[serde(rename = "type")]                // `type` is a keyword in Rust
    pub kind: String,
    #[serde(alias = "display_name")]         // accept the old name when reading
    pub display_name_v2: String,
}
```

- `rename_all` options: `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`,
  `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`.
- On enums, `rename_all` renames *variants*; use `rename_all_fields` (serde ≥ 1.0.181)
  to rename fields inside struct variants.
- `alias` only affects deserialization. Use it to migrate a field name without breaking
  old payloads.
- `#[serde(rename(serialize = "a", deserialize = "b"))]` handles asymmetric APIs.

## 3. Optional and defaulted fields

```rust
use serde::{Deserialize, Serialize};

fn default_port() -> u16 { 8080 }

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    #[serde(default = "default_port")]          // missing → 8080
    pub port: u16,
    #[serde(default)]                           // missing → Vec::new()
    pub allowed_origins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_cert: Option<String>,               // omitted from output when None
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
}
```

- `Option<T>` fields are already treated as optional on input (missing → `None`) by
  serde_json. Add `#[serde(default)]` anyway: other formats and `deserialize_with`
  don't get that special case.
- `#[serde(default)]` on the **container** fills every missing field from the struct's
  `Default` impl, which is ideal for config files where every key is optional:

```rust
#[derive(Debug, serde::Deserialize)]
#[serde(default)]
pub struct Tuning { pub workers: usize, pub queue_len: usize }

impl Default for Tuning {
    fn default() -> Self { Self { workers: 4, queue_len: 1024 } }
}
```

- `skip_serializing_if` keeps output small and stable; pair it with `default` so the
  value round-trips.
- To distinguish "field absent" from "field explicitly null" (e.g. PATCH requests), use
  `Option<Option<T>>` with `#[serde(default, deserialize_with = …)]`, or the
  `serde_with::rust::double_option` helper. Better still, a small enum
  (`Patch::Missing | Patch::Null | Patch::Value(T)`).
- `#[serde(skip)]` excludes a field entirely (requires `Default` for deserialization),
  useful for caches or runtime-only state.

## 4. `flatten`

`flatten` inlines one struct's fields into another, which is good for shared groups of
fields and for capturing unknown keys.

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Pagination { pub page: u32, pub per_page: u32 }

#[derive(Debug, Serialize, Deserialize)]
pub struct ListUsers {
    pub query: String,
    #[serde(flatten)]
    pub pagination: Pagination,                  // {"query":…, "page":…, "per_page":…}
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>, // keeps unknown keys instead of dropping them
}
```

**Cost:** `flatten` makes serde buffer the input into an intermediate map, which is
slower, and it can't be combined with `deny_unknown_fields`. Some formats (bincode and
other non-self-describing formats) don't support it. Use it for readability, not inside
hot paths.

## 5. Enum representations

The same Rust enum can map to four JSON shapes. Pick the one that matches the protocol
you're talking to; for formats you control, **internally tagged** is usually the most
readable.

```rust
use serde::{Deserialize, Serialize};

// Externally tagged (default): {"Click":{"x":1,"y":2}}  /  "Close"
#[derive(Serialize, Deserialize)]
enum External { Click { x: i32, y: i32 }, Close }

// Internally tagged: {"type":"click","x":1,"y":2}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Internal { Click { x: i32, y: i32 }, Close }

// Adjacently tagged: {"t":"click","c":{"x":1,"y":2}}
#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
enum Adjacent { Click { x: i32, y: i32 }, Text(String), Close }

// Untagged: {"x":1,"y":2}  — serde tries each variant in order
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Untagged { Point { x: i32, y: i32 }, Label(String) }
```

| Representation | Good for | Watch out for |
| --- | --- | --- |
| External (default) | Rust-to-Rust, bincode, quick prototypes | Unusual shape for other languages |
| Internal `tag` | Event/message types, Discord/Slack-style payloads | Tuple variants with non-map content not allowed; buffers input |
| Adjacent `tag`+`content` | Variants holding primitives or sequences | Slightly more verbose JSON |
| `untagged` | Accepting "a string or an object" style inputs | Errors become "data did not match any variant"; order matters; slow on large inputs |

Tips:
- Unit-only enums (`enum Role { Admin, Member }`) serialize as plain strings with any
  of these; just add `rename_all`.
- `#[serde(other)]` on a unit variant catches unknown tags, so a new server-side event
  type doesn't break your client:

```rust
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Event {
    MessageCreate { content: String },
    Ready,
    #[serde(other)]
    Unknown,
}
```

- Keep `untagged` for small, clearly distinct shapes. When you need it, put the most
  specific variant first.
- Integer-coded enums (`0`, `1`, `2` on the wire) need `serde_repr`
  (`#[derive(Serialize_repr, Deserialize_repr)] #[repr(u8)]`).

## 6. Validated types: `try_from`, `from`, `into`, `transparent`

Route deserialization through your validating constructor so invalid data can't
produce a value (see newtypes in `type-design.md`).

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Username(String);

impl TryFrom<String> for Username {
    type Error = String; // must implement Display
    fn try_from(s: String) -> Result<Self, Self::Error> {
        let ok = (3..=32).contains(&s.len())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ok { Ok(Self(s)) } else { Err(format!("invalid username: {s:?}")) }
    }
}

impl From<Username> for String {
    fn from(u: Username) -> String { u.0 }
}

// serde_json::from_str::<Username>("\"a\"") → Err("invalid username: \"a\"")
```

- `try_from = "T"`: deserialize a `T`, then call `TryFrom<T>`; the error becomes a serde
  error with your message.
- `from = "T"`: same but infallible.
- `into = "T"`: serialize by converting to `T` (requires `Clone`).
- `#[serde(transparent)]`: serialize a single-field wrapper as its inner value, with
  **no** validation. Use it for unconstrained ID newtypes like `struct UserId(u64)`.

## 7. Strictness: `deny_unknown_fields`

```rust
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig { pub name: String, pub debug: bool }
```

Use it for **config files you own**, so a typo like `debgu = true` is an error instead
of a silently ignored key. Don't use it for **third-party API responses**: those APIs add
fields all the time, and your client would break on every addition. It's incompatible
with `flatten`.

## 8. Custom field logic: `with`, `serialize_with`, `deserialize_with`

When one field needs a non-default format, write a small module and reference it with
`with`. The struct keeps its plain Rust type.

```rust
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// Durations as integer milliseconds on the wire.
mod duration_ms {
    use super::*;
    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        u64::deserialize(d).map(Duration::from_millis)
    }
}

/// Accept both `"42"` and `42`.
fn u64_from_str_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw { Int(u64), Str(String) }
    match Raw::deserialize(d)? {
        Raw::Int(n) => Ok(n),
        Raw::Str(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    #[serde(with = "duration_ms")]
    pub timeout: Duration,
    #[serde(deserialize_with = "u64_from_str_or_int")]
    pub id: u64,
}
```

- Report bad data with `serde::de::Error::custom(msg)`, never by panicking.
- Before writing your own, check `serde_with`: it ships `DisplayFromStr`,
  `DurationSeconds`, `base64`, `StringWithSeparator`, `NoneAsEmptyString`, and many
  others via `#[serde_as]`.
- If a whole type needs custom logic, implement `Serialize`/`Deserialize` for the type
  itself (or use `try_from`) rather than repeating `with` on every field.

## 9. Zero-copy deserialization: `&'a str` and `Cow`

When you parse large inputs and only read them, borrow from the input buffer instead of
allocating a `String` per field:

```rust
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Debug, Deserialize)]
pub struct LogLine<'a> {
    pub level: &'a str,          // fails if the JSON string contains escapes
    #[serde(borrow)]
    pub message: Cow<'a, str>,   // borrows when possible, allocates only for escapes
}

// let line: LogLine = serde_json::from_str(&buf)?;  // `line` can't outlive `buf`
```

- `&str` fields fail on strings with escape sequences (`\n`, `\"`), because the
  unescaped text doesn't exist in the input. `Cow<'a, str>` with `#[serde(borrow)]`
  handles both cases.
- Borrowing only works with `from_str`/`from_slice`, not `from_reader`.
- Use owned types (`String`) for anything you store long-term; borrowing pays off for
  short-lived processing of big inputs.

## 10. `serde_json::Value` only at the edges

**Why:** `Value` throws away the type information you just described. Code that does
`v["user"]["id"].as_u64().unwrap()` panics on schema changes and is hard to refactor.

```rust
// ❌
fn user_id(v: &serde_json::Value) -> u64 { v["user"]["id"].as_u64().unwrap() }
```

```rust
// ✅ describe only the part you need; unknown fields are ignored by default
#[derive(serde::Deserialize)]
struct Envelope { user: UserRef }
#[derive(serde::Deserialize)]
struct UserRef { id: u64 }

fn user_id(json: &str) -> serde_json::Result<u64> {
    Ok(serde_json::from_str::<Envelope>(json)?.user.id)
}
```

Legitimate uses of `Value`: proxying or storing payloads you don't interpret, truly
dynamic documents, the `extra` bag in a `flatten`, and the `json!` macro in tests.
`Box<serde_json::value::RawValue>` defers parsing of a sub-document without
interpreting it.

## 11. Dates, UUIDs, and other ecosystem types

Most popular crates implement serde behind a feature flag; enable it instead of
converting to strings by hand.

```toml
chrono  = { version = "0.4", features = ["serde"] }   # DateTime<Utc> ↔ RFC 3339
time    = { version = "0.3", features = ["serde", "formatting", "parsing"] }
uuid    = { version = "1", features = ["serde", "v4"] }
url     = { version = "2", features = ["serde"] }
rust_decimal = { version = "1", features = ["serde-with-str"] } # money: never f64
indexmap = { version = "2", features = ["serde"] }    # preserve key order
```

- `chrono::serde::ts_seconds` / `time::serde::timestamp` give Unix-timestamp formats
  via `#[serde(with = …)]`.
- Use `BTreeMap` or `IndexMap` instead of `HashMap` when you want deterministic output
  (snapshots, signatures, diffs).
- In a library, expose serde support behind your own optional `serde` feature so users
  who don't need it don't pay the compile time:

```toml
[features]
serde = ["dep:serde", "uuid/serde"]
[dependencies]
serde = { version = "1", features = ["derive"], optional = true }
```

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point { pub x: f64, pub y: f64 }
```

## 12. Testing round-trips

Every wire type deserves a test that pins the exact format. Renaming a Rust field
silently changes your API otherwise.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_wire_format_is_stable() {
        let e = Internal::Click { x: 1, y: 2 };
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({"type": "click", "x": 1, "y": 2}));
    }

    #[test]
    fn username_round_trips_and_rejects_bad_input() {
        let u: Username = serde_json::from_str(r#""ferris_1""#).unwrap();
        assert_eq!(serde_json::to_string(&u).unwrap(), r#""ferris_1""#);
        assert!(serde_json::from_str::<Username>(r#""x""#).is_err());
    }
}
```

For larger payloads, snapshot tests with `insta` (`assert_json_snapshot!`) make format
changes show up in review as a diff.

## 13. Pitfalls

- **`f64` for money.** Use integer minor units (`i64` cents) or `rust_decimal`.
- **`u64` in JSON for JavaScript consumers.** JS loses precision above 2^53; Discord and
  Twitter send snowflake IDs as strings for this reason. Use `serde_with::DisplayFromStr`
  or a string newtype.
- **`#[serde(default)]` hiding bugs.** A required field that silently defaults to `0` or
  `""` is worse than an error. Default only fields that have a genuine default.
- **Renaming a field breaks stored data.** Add `alias` for the old name when you rename.
- **Using `Debug` output as the wire format.** Use serde or `Display`, never `{:?}`.
- **`untagged` error messages.** "data did not match any variant" hides the real cause;
  prefer a tag field when you control the format.
- **Large `flatten`/`untagged` structures in hot paths.** Both buffer input; measure.
