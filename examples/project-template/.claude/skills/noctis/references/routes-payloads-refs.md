# Routes, Payloads and Refs

Covers: the component kind derives (`#[derive(Button)]`, the select derives,
`#[derive(Modal)]`; this release `#[derive(Route)]`), `defaults(..)` and `.build()`,
`#[derive(Payload)]`, `#[derive(PayloadEnum)]`, the codec
traits (`Route`, `Payload`, `PayloadDecode`), Payload field types, `#[handler]`, the `custom_id` wire
format and its compile-time length proof, `by_ref` and `overflow = spill`, Ref TTL and
one-shot, Route Guards, schema evolution, stale and expired handling, integrity tags.
Rules: CID-01..07, FR-15..19, FR-58..60. Never build a `custom_id` yourself.

## 1. Declaring a component (a Route)

One struct = its id + its typed data + its handler [planned: Story 6.10]. The derive
names the component kind; a Route is any such struct.

```rust
use noctis::prelude::*;
use noctis::id::marker::{GuildMarker, UserMarker};

#[derive(Button, Debug, Clone, Copy)]
#[button(
    "dash.mod",                                     // the id, first
    permissions = MANAGE_GUILD,                     // routing arguments; `permissions` is [planned]
    defaults(style = Success, label = "common.enable"),  // mandatory
)]
pub struct ToggleModule {
    pub guild: Id<GuildMarker>,
    pub module: ModuleKey,       // #[derive(PayloadEnum)]
    pub enable: bool,
}

impl ToggleModule {
    #[handler]                   // takes no arguments
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        // decode (done: `self`) → authorize → write → ctx.update(render_dashboard(..))
    }
}

// mod.rs
Module::new("dashboard").add::<ToggleModule>()
```

| Derive | Attribute | Builds | Required in `defaults(..)` | Optional keys (default) |
| --- | --- | --- | --- | --- |
| `Button` | `#[button("id", ..)]` | `RouteButton<Self>` | `style`, and `label` or `emoji` | the other of `label`/`emoji`, `disabled` (`false`) |
| `StringSelectMenu` | `#[string_select("id", ..)]` | `StringSelect` | `placeholder` | `options = SomeChoicesEnum`, `min_values` (1), `max_values` (1), `disabled` (`false`) |
| `UserSelectMenu` / `RoleSelectMenu` / `MentionableSelectMenu` | `#[user_select(..)]` / `#[role_select(..)]` / `#[mentionable_select(..)]` | `UserSelect` / `RoleSelect` / `MentionableSelect` | `placeholder` | `min_values` (1), `max_values` (1), `disabled` (`false`) |
| `ChannelSelectMenu` | `#[channel_select("id", ..)]` | `ChannelSelect` | `placeholder` | the above plus `channel_types = [GuildText, …]` |
| `Modal` | `#[modal("id", ..)]` | `Modal` (`modals.md`) | `title` | — |

A missing `defaults(..)` block or required key is a compile error naming the kind's
required keys; an unknown key is a compile error; literal values are checked against
Discord's limits (CV2-08, CV2-09, MOD-01). Text values are locale keys with the `i18n`
feature (checked against the default-locale file) and literal text without it. Link and
Premium buttons have no handler and stay plain builders: `LinkButton::new(url)`.

- The struct's fields **are** the Payload. The handler receives the decoded value as
  `self`; a free `async fn` with `#[handler]` does not compile.
- Exactly one handler per component id (CID-04, App build). The `#[handler]` method
  marks the struct as handled; components can only be built from handled structs, so
  `.build()` on one with no handler does not compile (``[CID-04] Route `X` has no
  handler``, FR-58). Remember `.add::<ToggleModule>()` on the Module, or App build fails
  ("Route has no mounted handler").
- **This release** spells all of this `#[derive(Route)] #[route("dash.mod", ..)]` with a
  free `#[handler] async fn toggle_module(ctx: ComponentCtx, t: ToggleModule)` and
  `.add(toggle_module())`; the grammar and errors below are the shipped `#[route]` ones,
  which carry over unchanged to the kind attributes (minus `select`).
- Generic Routes are rejected (`[CID-01] a Route cannot be generic`), and so are enums
  and unions: a Route is a struct with named fields, tuple fields or none
  (`struct Ping;`).
- `#[derive(Route)]` implements the `Route` trait (`ID`, `SHAPE`, `SCHEMA`,
  `type Payload = Self`) and the `Payload`/`PayloadDecode` traits for the struct, and
  proves the length bound at compile time (§5). Today the codec traits and these
  derives exist; the component builders (§9, `components-v2.md`) encode a Route's
  `custom_id`; decoding happens in the **[planned]** dispatcher, and `#[handler]` and
  the rest of this file's runtime parts are **[planned]** too.

### Route id (CID-03)

`[a-z0-9._-]{1,24}`, unique per App, stable forever: **renaming a Route id breaks every
live component using it.** Ids starting with `_` are reserved. Use short dotted ids
(`setup.ch`, `dash.mod`) to leave room for the Payload.

### Arguments after the id (`#[button(..)]`, `#[string_select(..)]`, …; this release `#[route(...)]`)

Grammar: exactly one kind attribute (this release: `#[route("id", args…)]`). The first element is the id as a
string literal (`"shop.buy"`, `r"shop.buy"`); the arguments follow in any order,
separated by commas, and a trailing comma is fine.

Codec arguments (they change the `custom_id` and count toward the length proof):
`by_ref`, `overflow = spill`, `signed`, `schema = n`, and (this release only) `select`:
from Story 6.10 every select kind carries the nonce and the bare `select` is withdrawn.
These work today.
(`view` is **withdrawn**: never write it. It still parses in this release and is
removed by Story 6.10, after which it is a CID-03 error. Tabs, pages and steps are
ordinary Payload fields; see `views-and-recipes.md`.) Runtime arguments (no effect on the length): `response`, `auto_defer`, `ttl`, `one_shot`,
`owner`, `permissions`, `guard`, `rate_limit(..)` are **[planned]**: this release **rejects** them (and the
withdrawn `opens_modal`, which Story 6.10 makes an unknown argument) with
``[CID-03] `ttl` is a runtime argument of #[route] that this Noctis release does not
support yet`` (a silently ignored guard would be a security hole), and each one is
accepted from the release that implements it. Anything else is a compile error.
`#[handler]` itself takes no arguments: every setting is here.

| Argument | Meaning |
| --- | --- |
| `"id"` | Route id (required, first) |
| `select` (this release) | used on select menus: adds a 2-char per-render nonce so clients show the server's defaults after a re-render (FR-25); implied by every select kind from Story 6.10 |
| `response = update \| ephemeral \| public` [planned, Story 7.2] | how the click is answered: edit in place (default; auto-defer type 6), or a new ephemeral/public message (sub-panels; auto-defer type 5) |
| `auto_defer = true \| false` [planned, Story 7.2] | default `true`; `false` for a component that opens a modal (its handler takes `ComponentResponder<Manual>`; INT-05, `responder.md` §2b) |
| `by_ref` | always store the Payload in the RefStore; the id carries `~` + 11-char Ref |
| `overflow = spill` | inline when the actual value fits, else by Ref |
| `ttl = "2d"` [planned] | Ref TTL (by_ref/spill only); default `AppBuilder::ref_ttl` (24 h) |
| `one_shot` [planned] | Ref is deleted on first use (`take`); for confirmations |
| `signed` | inline body gets a 6-char integrity tag (keyed BLAKE3, `NOCTIS_PAYLOAD_KEY`); not with `by_ref` |
| `schema = n` | Payload schema version 1..=61 (see §6) |
| `owner = field` [planned] | Route Guard: only the user whose id is in `field` may click |
| `permissions = A \| B` [planned] | Route Guard: the clicking member needs these permissions |
| `guard = path::to::fn` [planned] | custom guard `async fn(&ComponentCtx, &Self) -> Result<(), MessageKey>` |
| `rate_limit(user, 5, per = "1m")` [planned, Story 13.4] | see `modules-middleware-ratelimits.md` |
| `defaults(..)` | the component defaults (table above); mandatory |

Guards run before the handler on every interaction; a failure sends a localized
ephemeral notice and the handler does not run. Still re-check authorization for
anything sensitive: Payloads are client input (CID-06).

There is no `overflow = error`: a plain inline Route that could exceed 100 characters
simply doesn't compile (`[CID-01]`), which is the "error" policy.

The derive turns the declaration into `Route::SHAPE`: `RouteShape::INLINE`,
`RouteShape::BY_REF` or `RouteShape::SPILL`, then `.select()` and `.signed()` when
declared; `Route::SCHEMA` is `n`, or 0 without `schema`.

#### `#[route]` errors (CID-03)

Each is a compile error spanned on the offending token, `[CID-03] …` plus a `help:` line:

- a missing `#[route]`, a second `#[route]`, `#[route]` or `#[route = "a"]` without a
  list, `#[route()]` without an id, an id that is not a string literal (`a`, `b"a"`);
- an id outside the grammar: wrong characters (`"Shop"`, `"a:b"`, `"é"`), empty or over
  24 characters, or starting with `_` (all applicable problems are reported);
- an unknown argument (`colour`, named in the error), a runtime argument (above), an
  argument given twice (`select, select`), a value on a flag (`select = true`, `by_ref()`);
- `overflow` without `= spill` (`overflow = error`, `overflow = inline`, bare
  `overflow`);
- `schema` that is not an integer literal 1..=61 (`schema = 0` says "leave it out for
  schema 0"; `schema = 62`, `schema = -1`, `schema = "3"`, bare `schema`);
- `by_ref` with `overflow = spill`, and `by_ref` with `signed`.

Accepted spellings include `r"a"`, `"shop\x2Ebuy"` (the value counts: `shop.buy`) and
`schema = 61u8`.

### Methods in the struct's `impl` [planned]

| Method attribute | Meaning |
| --- | --- |
| `#[handler] async fn handle(self, ctx, ..)` | the handler (exactly one; takes no arguments) |
| `#[on_expired] async fn …(ctx, expired: Expired)` | this component's Expired Handler (no `self`: the Payload is gone) [Story 7.3] |
| `#[on_stale] async fn …(ctx)` | this component's Stale Payload handler (no `self`) [Story 7.3] |

## 2. Field types and widths (AD-6)

Every field implements `noctis::codec::Field { const MAX_LEN: Option<usize> }`
(worst-case encoded ASCII width, escaping included) and `noctis::codec::FieldDecode`:

| Type | Width |
| --- | --- |
| `bool` | 1 |
| `u8`, `i8` | 2 |
| `u16`, `i16` | 3 |
| `u32`, `i32` | 6 |
| `u64`, `i64`, `Id<T>` | 11 |
| `#[derive(PayloadEnum)]` enum (≤ 62 unit variants) | 1 |
| `Option<T>` | `T + 1` |
| `BoundedStr<N>` (ASCII only, `!` escaping) | `2N` |
| `String`, `Vec<T>` | unbounded (`None`) |

A type that is not a field fails with `` [CID-01] `f32` is not a Payload field type ``.
A struct is never a field: a Payload is a flat field list.

Fields are separated by 1 char. Encodings (normative, AD-5):

- Integers: minimal-digit base62 (`0-9A-Za-z`, no padding, `0` is `"0"`, `255u8` is `"47"`);
  signed integers are zig-zag mapped first (`-1` is `"1"`, `1` is `"2"`).
- `bool`: `"0"`/`"1"`. `Option<T>`: `None` is `"0"`, `Some(v)` is `"1"` + `v`.
- `Id<T>`: its `u64`; `"0"` does not decode (`ZeroId`).
- String-like fields escape: `!` → `!0`, `,` → `!1`, `:` → `!2`, `~` → `!3`; other
  printable ASCII (`0x20..=0x7E`) is written as is. `String` and `Cow<str>` write every
  other char as `!u` + 6 lowercase hex digits (`é` → `!u0000e9`).
- `Vec<T>`: each element followed by `,`, the whole then escaped (`[1u8, 2]` → `"1!12!1"`).
  List elements must be owned (implement `FieldDecode` for every `'de`): `Vec<Cow<str>>`
  or a borrowing custom type encodes but does not decode.

### `BoundedStr<N>`

`noctis::codec::BoundedStr<N>`: printable ASCII (`0x20..=0x7E`) of at most `N` chars,
stored inline, `Copy`. `BoundedStr::new(&str)` returns
`Result<_, BoundedStrError>` (`BoundedStrError::InvalidChar { index, ch }` for a
non-printable or non-ASCII char, checked first, then
`BoundedStrError::TooLong { capacity, actual }`). `BoundedStr::from_static("abc")` is a
`const fn`: an invalid literal in a `const` is a compile error. Also: `as_str`, `len`,
`is_empty`, `BoundedStr::<N>::CAPACITY`, `Display`, `FromStr`, `TryFrom<&str>`,
`AsRef<str>` and `==` with `&str`.

### Custom field types

Your own type becomes a Payload field by implementing two traits from `noctis::codec`:

- `Field { const MAX_LEN: Option<usize>; fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result; }`
- `FieldDecode<'de>: Field + Sized { fn decode(raw: &'de str) -> Result<Self, FieldError>; }`
  (implement it for every `'de`, as `impl<'de> FieldDecode<'de> for T`, unless you
  borrow from the input; a borrowing type cannot be a `Vec` element).
  It also has a provided `fn absent() -> Option<Self>` (default `None`): the value used
  when the field is missing from the end of an older `custom_id`. Only `Option<T>`
  overrides it (`Some(None)`); leave the default on your own types.

Contract: every encoding is printable ASCII without `,`, `:` or `~`, at most `MAX_LEN`
chars when `MAX_LEN` is `Some`; `decode` never panics and is canonical (when it
returns `Ok(v)`, encoding `v` gives exactly the input). Delegating to a built-in
field is the easy way to meet it:

```rust
use noctis::codec::{Field, FieldDecode, FieldError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Percent(u8);

impl Field for Percent {
    const MAX_LEN: Option<usize> = <u8 as Field>::MAX_LEN;
    fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result {
        self.0.encode(out)
    }
}

impl<'de> FieldDecode<'de> for Percent {
    fn decode(raw: &'de str) -> Result<Self, FieldError> {
        let v = u8::decode(raw)?;
        if v <= 100 { Ok(Self(v)) } else { Err(FieldError::Overflow) }
    }
}
```

`FieldError` variants (`#[non_exhaustive]`): `Empty`, `InvalidChar { index }`,
`LeadingZero`, `Overflow`, `TooLong { max, actual }`, `OverCapacity { capacity, actual }`,
`ZeroId`, `BadEscape { index }`, `TrailingInput { index }`, `BadList`.

### `#[derive(PayloadEnum)]`

```rust
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKey { Logging, Moderation, Welcome }
```

- Unit variants only, 1 to 62 of them (`[CID-01]` for a 63rd variant, a tuple or struct
  variant, no variants, generics, or a struct). Explicit discriminants are allowed and
  ignored.
- It implements `Field` (`MAX_LEN = Some(1)`) and `FieldDecode`: a variant encodes as
  its **declaration index** in one base62 digit (`Logging` → `"0"`, the 62nd → `"z"`).
- An index the enum does not have decodes as `FieldError::Overflow` (no panic).
- **Appending** a variant is compatible; **reordering or removing** variants changes
  the meaning of old ids, so bump the Route's `schema`.

### Testing a field: `noctis::codec::check`

- `check::round_trip(&v)`: encodes, checks the contract, decodes and compares.
- `check::worst_case(&v)`: `round_trip` plus "the encoding is exactly `MAX_LEN`
  chars"; pass your worst-case value.
- `check::assert_round_trip(&v)` / `check::assert_worst_case(&v)`: the same, panicking
  with the error message (use these in tests).
- Failures are `check::CheckError`: `Encode` (encode returned an error),
  `NotPrintable { index, ch }`, `ReservedChar { index, ch }`, `TooLong { max, actual }`,
  `Decode { encoded, error }`, `Mismatch { encoded }`,
  `WidthMismatch { max, actual }`.

```rust
use noctis::codec::check::{assert_round_trip, assert_worst_case};

#[test]
fn percent_field() {
    assert_round_trip(&Percent(0));
    assert_round_trip(&Percent(42));
    assert_worst_case(&Percent(100));
}
```

With `proptest` in your `[dev-dependencies]`, check every value:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn percent_round_trips(v in 0u8..=100) {
        prop_assert_eq!(noctis::codec::check::round_trip(&Percent(v)), Ok(()));
    }
}
```

**Never put free text in a Payload.** `BoundedStr<N>` exists for tiny ASCII tokens
(a short code). User text, reasons, names, URLs: store them via a port (or use
`by_ref`) and carry an id.

## 3. The codec traits and `#[derive(Payload)]`

All in `noctis::codec`; `use noctis::prelude::*;` brings the `Route` and `Payload`
traits and the `Route`, `Payload` and `PayloadEnum` derives.

- `trait Route { const ID: RouteId; const SHAPE: RouteShape; const SCHEMA: u8; type Payload: Payload; }`
  — what `#[derive(Route)]` implements.
- `RouteId`: `RouteId::new("shop.buy")` is a `const fn` (a bad literal in a `const`
  does not compile), `as_str()`, `is_reserved()` (starts with `_`), `Display`.
- `RouteShape`: `INLINE`, `BY_REF`, `SPILL`, the `const` builders `select()` and
  `signed()` (`BY_REF.signed()` is a compile error in a `const`), and the getters
  `body() -> BodyKind`, `has_select()`, `is_signed()`. The builder `view()` and the
  getter `has_view()` belong to the withdrawn view envelope; don't use them (removed
  by Story 6.10).
- `BodyKind`: `Inline`, `ByRef`, `Spill`.
- `trait Payload { const MAX_ENCODED_LEN: Option<usize>; fn encode_fields(&self, out: &mut FieldWriter<'_>) -> core::fmt::Result; }`
  — `MAX_ENCODED_LEN` is the worst-case body width: the fields' `MAX_LEN`s plus one
  `,` between each two, `None` when any field is unbounded, `Some(0)` for no fields.
- `trait PayloadDecode<'de>: Payload + Sized { fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError>; }`.
- `FieldWriter::field(&mut self, &impl Field)` writes the next field (separators are
  automatic); `FieldReader::field::<F>()` reads the next one.
- `CodecError` (`#[non_exhaustive]`): why an id or inline body does not decode:
  `Empty`, `TooLong { len }`, `NotAscii { index }`, `InvalidChar { index }`,
  `Generation { generation }`, `MissingSeparator`, `EmptyRoute`,
  `RouteTooLong { len }`, `Truncated { section }`, `ZeroSchema`, `TagOnRef`,
  `RefLength { len }`, `MissingField { field }`, `ExtraField { field }`,
  `Field { field, error }` (a `FieldError`), `UnexpectedSection { section }`.
- `Section` (`#[non_exhaustive]`): the fixed-width layout sections `Schema`,
  `Nonce`, `Tag` (and `Envelope`, of the withdrawn view envelope; removed by Story
  6.10).

`#[derive(Payload)]` implements `Payload` and `PayloadDecode` for a struct, exactly as
`#[derive(Route)]` does, for the Payload of a hand-written `Route`. It is not a field
type, and deriving both `Route` and `Payload` on one struct is a conflicting-impl error
(`Route` already includes it).

```rust
use noctis::codec::{Route, RouteId, RouteShape};
use noctis::prelude::*;

#[derive(Payload, Debug, PartialEq)]
pub struct Pick { pub module: ModuleKey, pub page: Option<u16> }

pub struct PickRoute;
impl Route for PickRoute {
    const ID: RouteId = RouteId::new("dash.pick");
    const SHAPE: RouteShape = RouteShape::INLINE;
    const SCHEMA: u8 = 0;
    type Payload = Pick;
}
```

A hand-written `Route` gets no compile-time length proof (only `#[derive(Route)]` emits
it), so an overlong id fails only at encode time; prefer `#[derive(Route)]`.

## 4. The `custom_id` format (normative, AD-5)

```text
custom_id = F route ":" [S] [N] body [T]

F      1 char    base62 digit = 16 * generation + flags (1=S, 4=N, 8=T; 2 is reserved)
route  1..24     [a-z0-9._-]
S      1 char    schema version (only if schema = n)
N      2 chars   per-render select nonce (only with `select`)
body   inline    field ("," field)*      or      by ref: "~" + 11 base62 Ref
T      6 chars   integrity tag (only with `signed`, inline only)
```

All ASCII (CID-07), so "characters" are unambiguous. (The withdrawn 5-character view
envelope `E` still exists in this release's codec and is removed by Story 6.10; no
Route of yours should declare it.)

## 5. The length proof (CID-01: ≤ 100)

```text
MAX_LEN = 1 + route.len() + 1 + S + N + max(inline, by_ref)
inline  = Payload::MAX_ENCODED_LEN + T        by_ref = 1 + 11 = 12
S, N, T = 1, 2, 6 when declared, else 0
```

Only an **inline** Route must fit with its worst-case body: `by_ref` needs only
`prefix + 12 ≤ 100`, and a `spill` id is inline only when the value fits, so its maximum
is capped at 100.

The derive emits a constant assertion, so `cargo check --release` fails on an overflow
with the route id and the worst case. Each derived Route also gets a generated
`#[test] fn __noctis_route_width_<StructName>` that prints the width breakdown and
asserts it fits; run all of them with
`cargo test --release -- --nocapture noctis_route_width`. The width test runs only
for module-level Routes: a Route declared inside a function body gets the compile-time
proof but no runnable width test, and rustc warns `cannot test inner items` unless that
function carries `#[allow(unnameable_test_items)]`. The breakdown line:

```text
shop.buy: F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)
setup.big: F 1 + route 9 + sep 1 + S 0 + N 0 + ref 12 = 23 (limit 100)
s: F 1 + route 1 + sep 1 + S 0 + N 0 + body unbounded + T 0 inline, else ref 12 = 100 (limit 100)
```

(inline, `by_ref` and `overflow = spill` Routes; `body` is `Payload::MAX_ENCODED_LEN`).
This release still prints an `E 0` term between `S` and `N`; it disappears with the
withdrawn envelope (Story 6.10).
Copy the line into the Route's doc comment so reviewers see the budget.

Worked examples from the template:

| Route | Calculation | Max |
| --- | --- | --- |
| `SetupState` `#[button("setup")]` {owner `Id`, step enum, log_channel `Option<Id>`, mod_role `Option<Id>`} | 1 + 5 + 1 + (11 + 1 + 12 + 12 + 3 sep) | 46 |
| `PickLogChannel` `#[channel_select("setup.ch")]` (nonce N) {owner `Id`, mod_role `Option<Id>`} | 1 + 8 + 1 + N2 + (11 + 12 + 1) | 36 |
| `ToggleModule` `#[button("dash.mod")]` {guild `Id`, module enum, enable `bool`} | 1 + 8 + 1 + (11 + 1 + 1 + 2) | 25 |
| any `by_ref` Route, 24-char id, `select` + schema | 1 + 24 + 1 + 1 + 2 + 12 | 41 |

So `by_ref` and `spill` Routes always fit. An inline Route with a `String` field:

```text
error[E0080]: evaluation panicked: [CID-01] route `report.reason` has unbounded fields
                = help: bound them (`BoundedStr<N>`), or add `by_ref` or `overflow = spill` to #[route]
```

An inline Route one character over:

```text
error[E0080]: evaluation panicked: [CID-01] route `a` can exceed 100 characters: worst case 101
                = help: shorten the route id or the fields (`PayloadEnum`, smaller integers), or add `by_ref` or `overflow = spill`; run the generated noctis_route_width test for the breakdown
```

Uniqueness within one message (CID-02) is checked at runtime: two components built from
the same Route and an equal Payload collide. Make Payloads differ (e.g. include the
item id), which is natural for "same handler, different data" (Prev/Next).

## 6. Payload evolution (FR-60)

Messages live for months; old components still get clicked after a deploy.

- **Compatible, no bump:** appending `Option<T>` fields at the end (old ids decode them
  as `None`, through `FieldDecode::absent`; `<Option<T>>::absent()` is `Some(None)`,
  every other field's is `None`, so a missing non-`Option` field stays an error).
- **Everything else bumps `schema`:** removing/reordering/retyping fields, adding a
  non-`Option` field, and changing `select`, `signed` or `by_ref`.
- A mismatched schema or shape decodes as **Stale**: it goes to the Route's `on_stale`
  handler, else the Expired Handler. Typical stale handler: call the screen's render
  function from scratch and add a `ctx.reply_text` notice.
- Renaming the Route id = old components become unknown (logged, generic notice).

## 7. Refs

A Ref is 11 base62 chars from the OS CSPRNG (~65.5 bits), never sequential, namespaced
per App. It maps to a `RefRecord { route, schema, payload }` in the `RefStore`.

```rust
#[derive(Button, Debug, Clone)]
#[button("mod.ban.ok", by_ref, one_shot, ttl = "15m", permissions = BAN_MEMBERS,   // runtime args [planned]
         defaults(style = Danger, label = "mod.ban.confirm"))]
pub struct ConfirmBan {
    pub target: Id<UserMarker>,
    pub reason: String,               // fine: by_ref has no inline length limit
}
```

- Minted in two phases: building a component makes a `RefSlot`; the Responder stores it
  (`put_if_absent`) only when the message is actually sent. Identical slots in one
  message share one Ref.
- Resolution: `one_shot` → `take`, else `get`. Missing/expired → **Expired Handler**.
  A record whose route differs from the id's route → forged (unknown-Route path).
  Schema mismatch → stale.
- A step flow's Cancel handler can delete the flow's Refs: return the deletions as
  `RefOps` with the response; core runs them after a successful send.

## 8. Expired, stale, unknown, forged

| Case | Goes to |
| --- | --- |
| Ref missing or expired | Route `on_expired`, else App `expired_handler`, else default localized ephemeral notice (`noctis.expired`) |
| Schema/shape mismatch | Route `on_stale`, else as expired |
| Unknown Route, malformed id, bad integrity tag | logged warning + generic notice; never a panic (FR-19) |

```rust
impl SetupState {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> { … }

    /// Re-open step 1 of the setup flow when its Ref is gone (UJ-1): the same render
    /// function, plus a one-line notice.
    #[on_expired]
    async fn restart(ctx: ComponentCtx, _expired: Expired) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let first = SetupState { owner: ctx.user_id(), step: SetupStep::Channel, log_channel: None, mod_role: None };
        ctx.update(render_setup(first, &s)).await?;
        ctx.reply_text(keys::NOCTIS_EXPIRED).await                // ephemeral follow-up
    }
}
```

## 9. Building components: `.build()` [planned: Story 6.10, defaults Story 9.3]

```rust
ToggleModule { guild, module, enable }.build()      // the declared kind, from `defaults(..)`
    .label(s.common.disable())                      // per-render overrides replace one field each
    .style(ButtonStyle::Danger)
    .disabled(false);

TicketSystem.build()                                // a string select whose defaults have no `options`:
    .options(systems.iter().map(|t| SelectOption::new(&t.name, t.id.to_string())))  // required before use (typestate)
    .default_values([current]);
```

Overrides: `.label(..)`, `.emoji(..)`, `.style(..)`, `.disabled(..)`, `.placeholder(..)`,
`.options(..)`, `.default_values(..)`, `.min_values(..)`, `.max_values(..)`; for modals
`.title(..)` and `.value(field, ..)`. Anything not overridden comes from the defaults;
the result is validated against the Limits (Strict or Lenient) when sent. `.build()`
requires a handler (`[CID-04]`); on a struct without a declared kind it is a compile
error whose help lists the kind derives.

**This release** builds with the `RouteComponents` methods instead (prelude;
`components-v2.md` §0), each requiring a handler (`[CID-04]`) and the selects a Route
declared `select`:

```rust
route.button()                 -> RouteButton<R> // .label .style .emoji .disabled
route.string_select(options)   -> StringSelect  // requires `select` on the Route (SelectRoute)
route.user_select()            -> UserSelect
route.role_select()            -> RoleSelect
route.mentionable_select()     -> MentionableSelect
route.channel_select()         -> ChannelSelect
FormType::modal(route)         -> FormTypeModal // modals.md
```

Selected values: `ctx.values()` → `.strings()`, `.users()`, `.roles()`, `.channels()`,
`.mentionables()`; resolved entities: `ctx.resolved()`.

## 10. Interaction pipeline (fixed order)

1. decode `custom_id` / resolve command → 2. resolve Ref → 3. Boot Flag, guild access,
Module enabled ("disabled here") → 4. locale resolution → 5. rate limit → 6. arm
auto-defer (unless `auto_defer = false`; shaped by `response`) → 7. before-Middleware → 8. Route Guards → 9. your
handler → 10. after-Middleware, Error Sinks. Stages 1–5 run none of your code (except a
`LocaleResolver`).
