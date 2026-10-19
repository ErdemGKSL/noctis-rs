# Routes, Payloads and Refs

Covers: `#[derive(Route)]`, Payload field types, `#[handler]`, the `custom_id` wire
format and its compile-time length proof, `by_ref` and `overflow = spill`, Ref TTL and
one-shot, Route Guards, schema evolution, stale and expired handling, integrity tags.
Rules: CID-01..07, FR-15..19, FR-58..60. Never build a `custom_id` yourself.

## 1. Declaring a Route

```rust
use noctis::prelude::*;
use noctis::id::marker::{GuildMarker, UserMarker};

#[derive(Route, Debug, Clone, Copy)]
#[route("dash.mod", permissions = MANAGE_GUILD)]
pub struct ToggleModule {
    pub guild: Id<GuildMarker>,
    pub module: ModuleKey,       // #[derive(PayloadEnum)]
    pub enable: bool,
}

#[handler]
async fn toggle_module(ctx: ComponentCtx, t: ToggleModule) -> HandlerResult<impl IntoResponse> { … }
```

- The struct's fields **are** the Payload. The handler receives the decoded value.
- Exactly one `#[handler]` per Route (CID-04, App build). The handler macro implements
  `Handled` for the Route; components can only be built from `Handled` Routes, so a
  button pointing at a Route with no handler does not compile (FR-58). Remember
  `.add(toggle_module())` on the Module, or App build fails ("Route has no mounted
  handler").
- Generic Routes are rejected.

### Route id (CID-03)

`[a-z0-9._-]{1,24}`, unique per App, stable forever: **renaming a Route id breaks every
live component using it.** Ids starting with `_` are reserved. Use short dotted ids
(`setup.ch`, `dash.mod`) to leave room for the Payload.

### `#[route(...)]` arguments

Codec arguments (they change the `custom_id` and count toward the length proof):
`view`, `select`, `by_ref`, `overflow = spill`, `signed`, `schema = n`. Runtime
arguments (no effect on the length): `ttl`, `one_shot`, `opens_modal`, `owner`,
`permissions`, `guard`. Anything else is a compile error; rate limits go on
`#[handler]`, not here.

| Argument | Meaning |
| --- | --- |
| `"id"` | Route id (required, first) |
| `view` | this is a View's own Route: adds the 5-char view envelope (action, tab, page, state version) so Navbar/Wizard/Paginator controls work |
| `select` | used on select menus: adds a 2-char per-render nonce so clients show the server's defaults after a re-render (FR-25) |
| `by_ref` | always store the Payload in the RefStore; the id carries `~` + 11-char Ref |
| `overflow = spill` | inline when the actual value fits, else by Ref |
| `ttl = "2d"` | Ref TTL (by_ref/spill only); default `AppBuilder::ref_ttl` (24 h) |
| `one_shot` | Ref is deleted on first use (`take`); for confirmations |
| `signed` | inline body gets a 6-char integrity tag (keyed BLAKE3, `NOCTIS_PAYLOAD_KEY`); not with `by_ref` |
| `schema = n` | Payload schema version 1..=61 (see §5) |
| `opens_modal` | the handler answers with a modal and takes `ComponentResponder<OpensModal>`; no auto-defer (INT-05) |
| `owner = field` | Route Guard: only the user whose id is in `field` may click |
| `permissions = A \| B` | Route Guard: the clicking member needs these permissions |
| `guard = path::to::fn` | custom guard `async fn(&ComponentCtx, &Self) -> Result<(), MessageKey>` [planned] |

Guards run before the handler on every interaction; a failure sends a localized
ephemeral notice and the handler does not run. Still re-check authorization for
anything sensitive: Payloads are client input (CID-06).

There is no `overflow = error`: a plain inline Route that could exceed 100 characters
simply doesn't compile (`[CID-01]`), which is the "error" policy.

### `#[handler(...)]` arguments [planned]

| Argument | Meaning |
| --- | --- |
| `response = update` (default) / `ephemeral` / `public` | how a returned View is sent: edit in place, or a new ephemeral/public message (sub-panels) |
| `on_expired = fn` | this Route's Expired Handler |
| `on_stale = fn` | this Route's Stale Payload handler |
| `rate_limit(user, 5, per = "1m")` | see `modules-middleware-ratelimits.md` |

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
| a `#[derive(Payload)]` struct | sum of fields + separators |
| `String`, `Vec<T>` | unbounded (`None`) |

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

## 3. The `custom_id` format (normative, AD-5)

```text
custom_id = F route ":" [S] [E] [N] body [T]

F      1 char    base62 digit = 16 * generation + flags (1=S, 2=E, 4=N, 8=T)
route  1..24     [a-z0-9._-]
S      1 char    schema version (only if schema = n)
E      5 chars   view envelope: action, tab, page (2), state version (only with `view`)
N      2 chars   per-render select nonce (only with `select`)
body   inline    field ("," field)*      or      by ref: "~" + 11 base62 Ref
T      6 chars   integrity tag (only with `signed`, inline only)
```

All ASCII (CID-07), so "characters" are unambiguous. Envelope actions: `_` author,
`t` tab, `p` page, `j` jump, `n`/`b`/`c` wizard next/back/cancel, `i` inert indicator.

## 4. The length proof (CID-01: ≤ 100)

```text
MAX_LEN = 1 + route.len() + 1 + S + E + N + max(inline, by_ref)
inline  = Payload::MAX_ENCODED_LEN + T        by_ref = 1 + 11 = 12
S, E, N, T = 1, 5, 2, 6 when declared, else 0
```

The derive emits `const _: () = assert_fits::<T>();`, so `cargo check --release` fails on an
overflow, and a generated `#[test]` prints the width breakdown (`cargo test --release
-- --nocapture <route>`).

Worked examples from the template:

| Route | Calculation | Max |
| --- | --- | --- |
| `SetupState` `#[route("setup", view)]` {owner `Id`, log_channel `Option<Id>`, mod_role `Option<Id>`} | 1 + 5 + 1 + E5 + (11 + 12 + 12 + 2 sep) | 49 |
| `PickLogChannel` `#[route("setup.ch", select)]` {owner `Id`, mod_role `Option<Id>`} | 1 + 8 + 1 + N2 + (11 + 12 + 1) | 36 |
| `ToggleModule` `#[route("dash.mod")]` {guild `Id`, module enum, enable `bool`} | 1 + 8 + 1 + (11 + 1 + 1 + 2) | 25 |
| any `by_ref` Route, 24-char id, `view` + `select` + schema | 1 + 24 + 1 + 1 + 5 + 2 + 12 | 46 |

So `by_ref` and `spill` Routes always fit. An inline Route with a `String` field:

```text
error: [CID-01] route `report.reason` has unbounded fields; bound them (`BoundedStr<N>`) or add `by_ref`
```

Uniqueness within one message (CID-02) is checked at runtime: two components built from
the same Route and an equal Payload collide. Make Payloads differ (e.g. include the
item id), which is natural for "same handler, different data" (Prev/Next).

## 5. Payload evolution (FR-60)

Messages live for months; old components still get clicked after a deploy.

- **Compatible, no bump:** appending `Option<T>` fields at the end (old ids decode them
  as `None`, through `FieldDecode::absent`; `<Option<T>>::absent()` is `Some(None)`,
  every other field's is `None`, so a missing non-`Option` field stays an error).
- **Everything else bumps `schema`:** removing/reordering/retyping fields, adding a
  non-`Option` field, and changing `view`, `select`, `signed` or `by_ref`.
- A mismatched schema or shape decodes as **Stale**: it goes to the Route's `on_stale`
  handler, else the Expired Handler. Typical stale handler: re-open the View from
  scratch with a notice.
- Renaming the Route id = old components become unknown (logged, generic notice).

## 6. Refs

A Ref is 11 base62 chars from the OS CSPRNG (~65.5 bits), never sequential, namespaced
per App. It maps to a `RefRecord { route, schema, payload }` in the `RefStore`.

```rust
#[derive(Route, Debug, Clone)]
#[route("mod.ban.ok", by_ref, one_shot, ttl = "15m", permissions = BAN_MEMBERS)]
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
- Wizard Cancel deletes the Wizard's Refs automatically.

## 7. Expired, stale, unknown, forged

| Case | Goes to |
| --- | --- |
| Ref missing or expired | Route `on_expired`, else App `expired_handler`, else default localized ephemeral notice (`noctis.expired`) |
| Schema/shape mismatch | Route `on_stale`, else as expired |
| Unknown Route, malformed id, bad integrity tag | logged warning + generic notice; never a panic (FR-19) |

```rust
/// Re-open Step 1 of the setup Wizard when its Ref is gone (UJ-1).
async fn restart_setup(ctx: ComponentCtx, _expired: Expired) -> HandlerResult<impl IntoResponse> {
    Ok(SetupWizard::new(SetupState { owner: ctx.user_id(), log_channel: None, mod_role: None })
        .at(Nav::step(0))
        .with_notice(keys::NOCTIS_EXPIRED))
}

#[handler(on_expired = restart_setup)]
async fn open_setup(_ctx: ComponentCtx, state: SetupState) -> HandlerResult<SetupWizard> { … }
```

## 8. Building components from Routes

```rust
route.button()                 -> Button        // .label .style .emoji .disabled
route.string_select(options)   -> StringSelect  // requires `select` on the Route
route.user_select()            -> UserSelect
route.role_select()            -> RoleSelect
route.mentionable_select()     -> MentionableSelect
route.channel_select()         -> ChannelSelect
FormType::modal(route)         -> FormTypeModal // modals.md
```

Route component defaults (FR-59) [planned]: `impl ComponentRoute for ToggleModule { const
DEFAULTS: ComponentDefaults = ComponentDefaults::new().style(ButtonStyle::Secondary); }`;
per-render overrides win; the result is validated against the Limits.

Selected values: `ctx.values()` → `.strings()`, `.users()`, `.roles()`, `.channels()`,
`.mentionables()`; resolved entities: `ctx.resolved()`.

## 9. Interaction pipeline (fixed order)

1. decode `custom_id` / resolve command → 2. resolve Ref → 3. Boot Flag, guild access,
Module enabled ("disabled here") → 4. locale resolution → 5. rate limit → 6. arm
auto-defer (unless `opens_modal`) → 7. before-Middleware → 8. Route Guards → 9. your
handler → 10. after-Middleware, Error Sinks. Stages 1–5 run none of your code (except a
`LocaleResolver`).
