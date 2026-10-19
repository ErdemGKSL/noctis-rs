---
title: 'Story 4.1: Typed V2 message components with placement in the type system'
type: 'feature'
created: '2026-10-19'
status: 'in-progress'
baseline_revision: '1f94f48221a87cf928d7e5ad8226a2b0b1e2cc34'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/components-v2.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-ui` is an empty crate, so bot authors have no typed Components V2 model: nothing stops a Container inside a Container, a Thumbnail or Button in the wrong place, a hand-written `custom_id`, a component for a Route without a handler, or a dangling `attachment://` reference (FR-23, FR-58, FR-78, CV2-05/07/08/12/14).

**Approach:** Build the V2 message component model in `noctis-ui`: one type per component, placement as sealed marker traits whose `#[diagnostic::on_unimplemented]` cites the rule id, components built only from a Route value through a `Handled`-bounded extension trait, content-addressed `Attachment`/`AttachmentSet`/`AttachmentRef`, direct serde serialization to Discord JSON (unset fields omitted, numeric ids assigned in pre-order), re-exported through the facade, with trybuild fixtures, insta snapshots, the real skill-check component-entry check, and the Usage Skill/template reconciled.

## Boundaries & Constraints

**Always:**
- Layering (AD-1/deps-check): `noctis-ui` normal deps are exactly `noctis-codec`, `serde`, `bytes`, `blake3`, `twilight-model`, `thiserror`, `arrayvec` (all `workspace = true`); dev-deps `insta = { workspace = true, features = ["json"] }` (1.49.0, MSRV 1.66), `serde_json`. No `noctis-rules` dep is needed in 4.1; never a dep on ports/core/gateway.
- Placement traits are sealed (private `Sealed` supertrait plus a hidden conversion into the private node enum) so only Noctis types implement them. Messages, verbatim prefix: `TopLevel` `"[CV2-05] `{Self}` cannot be a top-level message component"`; `ContainerChild` `"[CV2-05] `{Self}` cannot be a Container child"`; `RowItem` `"[CV2-05] `{Self}` cannot be an Action Row button"`; `SectionAccessory` `"[CV2-07] `{Self}` is not a Section accessory: a Section needs exactly one accessory, a Button or a Thumbnail"`; `RowSelect` `"[CV2-06] `{Self}` is not a message select"`. Each has a `note`/`help` naming where the type may go (catalog §13 table).
- Every trait bound that must produce a rule-id error is on the method/function (E0277), never only on a blanket impl (that gives E0599 without the message).
- Every component builder takes text as `impl Into<Cow<'static, str>>`; setters are by-value and chainable; every public type is `Debug + Clone + Send + Sync`; public items are documented with doctests where runnable.
- No string `custom_id` constructor exists anywhere (AD-19); no API sets a numeric `id` (CV2-14); `File` accepts only `AttachmentRef` (CV2-12); `AttachmentRef` has no public constructor.
- Serialization omits unset optional fields and `false` booleans (`disabled`, `spoiler`, `default`, `animated`); media objects are exactly `{"url": …}` (never `content_type`).
- Every public API change updates `examples/project-template/` (code, `AGENTS.md` if a convention changes, the noctis skill) and `xtask/public-api.txt` + the skill's API hash in the same commit (AGENTS.md policy).
- Release-only cargo; trybuild relies on `.cargo/config.toml` (never edit it).

**Never:**
- No `V2Message`, flags, mentions, `LegacyMessage` (4.2); no runtime validator, `cost`, lenient truncation (4.3); no arity traits, `label!`/`placeholder!`/`rgb!`, `Rgb::try_from`, `components!`, `try_buttons`/`try_new`/`try_from_vec` (4.4); no modal types (4.5); no `ComponentRoute` defaults or `button_with` (9.3); no raw component (FR-31); no `noctis::twilight` re-export.
- No `const` assertions in generic code as an enforcement point; no edits to `.cargo/config.toml` or `_bmad/`.
- Do not add fixtures or test names citing ids still pending for later stories (e.g. `// [ATT-05]`, `cv2_06`, `[CID-02]`): a pending entry with evidence fails rules-catalog.
- `#[doc(hidden)]` items (`CustomId`, `Components::custom_ids_mut`, `Handled` re-export path) are not documented as public API in the skill.

## I/O & Edge-Case Matrix

Serialization rows show `serde_json::to_value` output; `Components` assigns `id`s, a lone component has none. Routes below: `Plain` = inline, unsigned, no view/select (`#[route("shop.buy")] struct Buy { item: u64 }`, `Buy { item: 255 }` encodes `0shop.buy:47`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Text Display | `TextDisplay::new("## Hi 😀")` | `{"type":10,"content":"## Hi 😀"}` (unicode verbatim) | none |
| Empty text | `TextDisplay::new("")` | `{"type":10,"content":""}` (accepted; 4.3 validates) | none |
| Separator default | `Separator::new()` == `Separator::default()` == `Separator::small()` | `{"type":14,"divider":true,"spacing":1}` | none |
| Separator large, no divider | `Separator::large().divider(false)` | `{"type":14,"divider":false,"spacing":2}`; `.spacing(Spacing::Small)` switches back to 1 | none |
| Container minimal | `Container::new()` | `{"type":17,"components":[]}` (no `accent_color`, no `spoiler`) | none |
| Container full | `.accent(Rgb::new(0x58,0x65,0xF2)).spoiler(true).push(TextDisplay::new("a"))` | `{"type":17,"components":[{"type":10,"content":"a"}],"accent_color":5793266,"spoiler":true}` | none |
| Rgb bounds | `Rgb::new(0,0,0)`, `Rgb::new(255,255,255)` | `accent_color` `0` and `16777215`; `Rgb::value()` returns the `u32`; `Rgb::new` is `const` | none (out of range unrepresentable) |
| Route button default | `Buy{item:255}.button()` | `{"type":2,"style":2,"custom_id":"0shop.buy:47"}` (Secondary default, no label/emoji/disabled) | none |
| Route button full | `.style(ButtonStyle::Danger).label("Buy").emoji(Emoji::unicode("🛒")).disabled(true)` | `{"type":2,"style":4,"label":"Buy","emoji":{"name":"🛒"},"custom_id":"0shop.buy:47","disabled":true}`; styles Primary 1, Secondary 2, Success 3, Danger 4 only | none |
| Custom emoji | `Emoji::custom(Id::new(42), "pog", true)`; `animated=false` | `{"id":"42","name":"pog","animated":true}`; without `animated` key when false | none |
| Link button | `LinkButton::new("https://x.y").label("Docs")` | `{"type":2,"style":5,"label":"Docs","url":"https://x.y"}`; no `custom_id`; `.emoji`, `.disabled` available | none (URL length: 4.3) |
| Premium button | `PremiumButton::new(Id::new(7)).disabled(true)` | `{"type":2,"style":6,"sku_id":"7","disabled":true}`; the type has no `label`/`emoji`/`url` setter | none |
| Action Row of buttons | `ActionRow::buttons([b1, b2])` (same type) or `[Button::from(route_b), Button::from(link)]` | `{"type":1,"components":[…]}` in order | none |
| Action Row arity not yet bounded | `ActionRow::buttons::<Button, 0>([])`, 6 buttons | compiles and serializes in 4.1 (4.4 adds arity traits, 4.3 runtime check) | none |
| Action Row select | `ActionRow::select(sel)` | `{"type":1,"components":[<select>]}` | none |
| Section | `Section::new([TextDisplay::new("a")], Thumbnail::new(Media::url("https://i/x.png")))` | `{"type":9,"components":[{"type":10,…}],"accessory":{"type":11,"media":{"url":"https://i/x.png"}}}` | none |
| Section with button accessory | accessory = RouteButton / LinkButton / PremiumButton / `Button` | accessory is that button's JSON | none |
| Thumbnail full | `Thumbnail::new(media).alt("A").spoiler(true)` | `{"type":11,"media":{"url":…},"description":"A","spoiler":true}` | none |
| Media Gallery | `MediaGallery::new([MediaItem::new(Media::url(u)).alt("x")])` | `{"type":12,"items":[{"media":{"url":u},"description":"x"}]}`; items carry no `type`/`id` | none |
| File | `File::new(att_ref).spoiler(true)` | `{"type":13,"file":{"url":"attachment://<name>"},"spoiler":true}` | none |
| Media from attachment | `Media::from(att_ref.clone())` | `{"url":"attachment://<name>"}`; `Media` remembers the ref (origin readable by 4.3) | none |
| String select | `Pick{..}.string_select([SelectOption::new("A","a").description("d").emoji(e).default(true)])` then `.placeholder("p").min_values(1).max_values(2).disabled(true)`, resolved with nonce | `{"type":3,"custom_id":…,"options":[{"label":"A","value":"a","description":"d","emoji":{…},"default":true}],"placeholder":"p","min_values":1,"max_values":2,"disabled":true}`; unset fields omitted; 0 options accepted (4.3 checks) | none |
| Auto-populated selects | `user_select()` type 5, `role_select()` 6, `mentionable_select()` 7, `channel_select()` 8 | `default_values` only when non-empty: `[{"id":"1","type":"user"}]` (`role`, `channel`); `channel_types` only when non-empty, as numbers | none |
| Select defaults APIs | `UserSelect::default_users(ids)`, `RoleSelect::default_roles`, `MentionableSelect::default_users`/`default_roles` (appended in call order), `ChannelSelect::default_channels`, `ChannelSelect::channel_types(impl IntoIterator<Item = ChannelType>)` | as above; empty iterator leaves the key omitted | none |
| Components ids | `Components::new().push(Container(Section(text a, text b; accessory button)), ActionRow(btn))` | pre-order ids: container 1, section 2, a 3, b 4, button 5, row 6, row's button 7 (`"id"` right after `"type"`); select options and gallery items get none | none |
| Empty body | `Components::new()` | `[]`; `len() == 0`, `is_empty()` | none |
| Plain route eager | `Plain` route built | `CustomId::get() == Some("0shop.buy:47")` | none |
| Signed / view / select / by_ref route | built without parts | `get() == None` (unresolved) | serializing the tree is a serde error naming the route id and "unresolved" |
| Spill route that fits / overflows | `overflow = spill` route, short / long payload | fits: resolved eagerly; overflows: unresolved, `resolve(&Parts::new())` returns `Ok(Some(PendingRef))` | none |
| Resolve with right parts | select route, `resolve(&Parts::new().with_nonce(SelectNonce::new(1).unwrap()))` | `Ok(None)`, `get()` is the encoded id | none |
| Resolve with wrong parts | select route, `resolve(&Parts::new())` | `Err(EncodeError::MissingNonce)`; state unchanged | typed error, no panic |
| Finish a slot | `by_ref` route: `resolve` → `Some(slot)`; `finish(&slot, Ref::mint()?)` | `Ok(())`; `get() == Some(noctis_codec::finish(&slot, r))` | none |
| Finish mismatched slot | slot from another route | `Err(UiError::SlotMismatch { expected, actual })`; state unchanged | typed error |
| Attachment naming | `Attachment::new(Bytes::new(), "png")` | name `af1349b9f5f9a1a6.png` (first 16 lowercase hex of blake3 of empty input); origin `Plain` | none |
| Attachment ext bounds | ext `"a"`, `"jpeg"`, `"PNG"`, `"abcdefghij"` (10) | accepted, kept verbatim | none |
| Attachment bad ext | `""`, `"abcdefghijk"` (11), `".png"`, `"pn g"`, `"pñg"`, `"png/x"` | — | `Err(UiError::InvalidExtension { ext })` |
| With digest | `Attachment::with_digest(bytes, [0xAB; 32], "png")` | `abababababababab.png`, origin `Plain`; `[0; 32]` → `0000000000000000.png` | bad ext as above |
| Rendered | `Attachment::rendered(bytes, digest, "webp")` | named by digest like `with_digest`, origin `Rendered` | bad ext as above |
| Attachment fields | `.alt("x")`, `.content_type("image/png")` | getters `name()`, `bytes()`, `content_type()`, `alt()`, `origin()`; unset → `None` | none |
| Set add | `set.add(a)` | returns `AttachmentRef` (`name()`, `url()` = `attachment://<name>`, `origin()`) | none |
| Set duplicate name | add the same bytes twice (or same digest) | one member kept (the first), both refs equal; `len() == 1` | none |
| Set lookup | `get(name)`, `iter()` (name order), `len`, `is_empty` | as stated | none |
| Placement: Container in Container | `Container::new().push(Container::new())` | does not compile, `[CV2-05]` | compile error |
| Placement: Thumbnail outside accessory | `Container::new().push(Thumbnail::new(m))` / `Components::new().push(thumbnail)` | `[CV2-05]` | compile error |
| Placement: Button outside row/accessory | `Container::new().push(LinkButton::new(u))` | `[CV2-05]` | compile error |
| Section without accessory | `Section::new([TextDisplay::new("a")], ())` (there is no 1-argument constructor) | `[CV2-07]` | compile error |
| Route without handler | derived Route without `Handled`, `.button()` | `[CID-04]` with `#[handler]` note | compile error |
| Non-select Route select | `.string_select(..)` on a Route not declared `select` | E0277 naming `SelectRoute` and `#[route("…", select)]` | compile error |
| String custom_id / numeric id | `File::new("attachment://x.png")`; `TextDisplay::new("a").id(5)` | E0308 / E0599 (by construction) | compile error |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/lib.rs` -- empty skeleton; becomes the crate root with module docs (model, placement table, custom_id lifecycle, attachments) and flat re-exports.
- `crates/noctis-ui/Cargo.toml` -- add the deps listed in Boundaries.
- `crates/noctis-codec/src/route.rs` (trait `Route` ~L240, `RouteShape`) -- add `pub trait SelectRoute: Route {}` with `#[diagnostic::on_unimplemented(message = "route `{Self}` is not declared `select`", note = "declare it with #[route(\"…\", select)]: a message select carries a per-render nonce")]`; doc that hand-written select Routes implement it and that `#[derive(Route)]` does it for `select`. Export from `crates/noctis-codec/src/lib.rs`.
- `crates/noctis-macros/src/route.rs` `expand` (~L363-418) -- when `args.select`, also emit `#[automatically_derived] impl #codec::SelectRoute for #ident {}`; add a unit test asserting the impl appears only for `select`.
- `crates/noctis-codec/src/encoded.rs` -- reuse `encode_id::<R>(&payload, &Parts) -> Result<EncodedId, EncodeError>`, `finish(&PendingRef, Ref) -> ArrayString<100>`, `PendingRef::route()`; `layout.rs` `Parts::{new, with_envelope, with_nonce, with_signer}`; `parts.rs` `SelectNonce::new(u16) -> Option<_>`; `refs.rs` `Ref::mint()`.
- `crates/noctis/src/lib.rs` -- facade: new `pub mod ui` (re-exports + module docs + doctest building a tree), `prelude` adds `RouteComponents`, `codec` re-exports `SelectRoute`, `__private` adds `pub use noctis_ui as ui;`; Cargo.toml adds `noctis-ui.workspace = true`.
- `crates/noctis/tests/ui.rs` -- already runs `tests/ui/**/*.rs`; new fixture dirs need no registration change.
- `crates/noctis/tests/ui/msg/MSG-03_all_with_ids.rs` -- fixture style reference.
- `docs/discord-rules.md` rows CID-04 (L177), CV2-05 (L193), CV2-07 (L195) -- enforcement/how text to update (see Tasks).
- `xtask/rules-pending.toml` -- entries CV2-05 diagnosed, CV2-07 diagnosed, CV2-12 by-construction, CV2-14 by-construction (story 4.1) to remove.
- `xtask/src/skill_check/{mod.rs,pending.rs}`, `xtask/skill-pending.toml`, `xtask/tests/skill_check.rs` (L72, L300-331 pending tests) -- pending mechanism to replace; `pending.rs` `BUILDERS` (16 message + 6 modal names) and `due_builders` filter are reused by the new check.
- `xtask/src/rules_catalog/catalog.rs` `real_catalog_classification` -- by-construction list unaffected by the CID-04 change (CID-04 becomes diagnosed + unit); keep passing.
- `examples/project-template/src/modules/{dashboard,profile,setup}.rs` -- aspirational component code (uses `Text`, `ContainerChild` as a type, `.text()`, `ActionRow::try_buttons`, `Button::link`) to reconcile; the template does not compile before Story 14.2, so reconciliation is by inspection.
- `examples/project-template/.claude/skills/noctis/references/components-v2.md`, `views-and-widgets.md`, `SKILL.md` (API hash line) -- skill to update.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-codec/src/route.rs`, `lib.rs`; `crates/noctis-macros/src/route.rs` -- add `SelectRoute` and its derive emission (see Code Map) -- selects need a compile-time "declared `select`" bound.
- `crates/noctis-ui/Cargo.toml` -- deps per Boundaries.
- `crates/noctis-ui/src/` (layout owned by the code; suggested `route.rs`, `placement.rs`, `component/*.rs`, `attachment.rs`, `components.rs`, `error.rs`) -- implement:
  - `pub trait Handled: Route {}` with `#[diagnostic::on_unimplemented(message = "[CID-04] Route `{Self}` has no handler, so no component can be built for it", label = "no `#[handler]` for this Route", note = "the `#[handler]` macro implements `Handled`; add a handler for `{Self}` to a mounted Module")]`; rustdoc says it is implemented only by the handler macro through `::noctis::__private::ui::Handled` and is not public API.
  - `pub trait RouteComponents: Route + Sized` with default methods `button(self) -> RouteButton<Self>`, `string_select(self, options: impl IntoIterator<Item = SelectOption>) -> StringSelect`, `user_select(self) -> UserSelect`, `role_select`, `mentionable_select`, `channel_select`; every method `where Self: Route<Payload = Self> + Handled + Send + Sync + 'static` (selects also `+ SelectRoute`); `impl<R: Route> RouteComponents for R {}`.
  - `#[doc(hidden)] pub struct CustomId` (route value behind a private `Arc<dyn ErasedRoute>`, `Option<ArrayString<MAX_ID_LEN>>`): eager encode at construction iff the shape has no view, no select and is unsigned (`encode_id` with `Parts::new()`; `Inline` → resolved, slot or error → unresolved); `route() -> RouteId`, `get() -> Option<&str>`, `resolve(&mut self, &Parts<'_>) -> Result<Option<PendingRef>, EncodeError>` (Inline → stored, `Ok(None)`; slot → `Ok(Some)`; error → unchanged), `finish(&mut self, &PendingRef, Ref) -> Result<(), UiError>` (route mismatch → `UiError::SlotMismatch`). Debug prints the route id and the resolved id only.
  - Components: `TextDisplay`, `Separator` + `Spacing {Small, Large}` (+`Default`), `Rgb` (`const fn new(u8,u8,u8)`, `value() -> u32`), `Emoji` (`unicode`, `custom(Id<EmojiMarker>, name, animated)`), `ButtonStyle {Primary, Secondary, Success, Danger}`, `RouteButton<R>` (`PhantomData<fn() -> R>`; `style`, `label`, `emoji`, `disabled`), `LinkButton` (`new(url)`, `label`, `emoji`, `disabled`), `PremiumButton` (`new(Id<SkuMarker>)`, `disabled`), opaque `Button` (`From` each button), `Media` (`url(..)`, `From<AttachmentRef>`), `Thumbnail` (`new(impl Into<Media>)`, `alt`, `spoiler`), `MediaItem` (same setters), `MediaGallery::new<const N: usize>([MediaItem; N])`, `File::new(AttachmentRef)` + `spoiler`, `Section::new<A: SectionAccessory, const N: usize>([TextDisplay; N], A)`, `ActionRow::buttons<B: RowItem, const N: usize>([B; N])` and `ActionRow::select<S: RowSelect>(S)`, `Container` (`new`, `push<C: ContainerChild>`, `accent(Rgb)`, `spoiler`), opaque `ContainerComponent`/`TopLevelComponent` (`From` each allowed type; each implements its own trait), `SelectOption` (`new(label, value)`, `description`, `emoji`, `default`), `StringSelect`/`UserSelect`/`RoleSelect`/`MentionableSelect`/`ChannelSelect` (shared `placeholder`, `min_values(u8)`, `max_values(u8)`, `disabled`; defaults APIs per Matrix), `Components` (`new`, `push<T: TopLevel>`, `len`, `is_empty`, `Default`, `#[doc(hidden)] custom_ids_mut() -> impl Iterator<Item = &mut CustomId>` in pre-order).
  - Trait impls exactly per catalog §13: `TopLevel` = Container, Section, TextDisplay, MediaGallery, File, Separator, ActionRow, TopLevelComponent; `ContainerChild` = Section, TextDisplay, MediaGallery, File, Separator, ActionRow, ContainerComponent; `SectionAccessory` = RouteButton<R>, LinkButton, PremiumButton, Button, Thumbnail; `RowItem` = RouteButton<R>, LinkButton, PremiumButton, Button; `RowSelect` = the five selects.
  - `serde::Serialize` for every component and `Components` per Matrix (ids only via `Components`, pre-order, `u32` from 1).
  - `Attachment` (private fields; `new(impl Into<Bytes>, &str)`, `with_digest(impl Into<Bytes>, [u8; 32], &str)`, `rendered(Bytes, [u8; 32], &'static str)` all `-> Result<Self, UiError>`; `alt`, `content_type` setters; getters), `AttachmentOrigin {Plain, Rendered}`, `AttachmentSet` (BTreeMap by name; `new`, `add -> AttachmentRef`, `get`, `iter`, `len`, `is_empty`), `AttachmentRef` (`name`, `url`, `origin`; `Clone + Eq + Hash`).
  - `#[non_exhaustive] pub enum UiError` (thiserror): `InvalidExtension { ext: String }`, `SlotMismatch { expected: RouteId, actual: RouteId }`.
- `crates/noctis-ui/tests/components.rs` (+ committed `tests/snapshots/*.snap`) -- hand-implemented test Routes (`Payload` + `Route` + `Handled` (+`SelectRoute`)) covering every Matrix serialization/resolution/attachment row with `insta::assert_json_snapshot!` for each component type and the id-numbering tree, plain asserts elsewhere; test fn names may cite only ids with no pending entry left (e.g. `cv2_14_ids_are_preorder`, `cv2_12_file_url_is_attachment_ref`), never `cv2_03`/`cv2_06`/`cid_02`/`att_05` etc.; plus a compile-time `assert_send_sync::<Components>()`.
- `crates/noctis-ui/src/**` doctests -- a `compile_fail` doctest on `string_select` (non-select Route) and one on `File::new("attachment://x.png") // [CV2-12]`, one on `TextDisplay::new("a").id(5) // [CV2-14]` (hand-written Routes, no derive).
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` -- facade wiring (Code Map); `noctis::ui` docs list every item and the placement table.
- `crates/noctis/tests/components.rs` -- `#[derive(Route)]` Routes (one plain, one `select`) made handled with `impl ::noctis::__private::ui::Handled for R {}`; build a Container/Section/row tree through `noctis::ui` + `noctis::prelude`, resolve the select via `noctis::__private::ui` `custom_ids_mut` and `noctis::__private::codec::{Parts, SelectNonce}`, assert JSON with `serde_json::json!`.
- `crates/noctis/tests/ui/cv2/` -- `CV2-05_container_in_container.rs`, `CV2-05_thumbnail_outside_accessory.rs`, `CV2-05_button_outside_row.rs`, `CV2-07_section_without_accessory.rs` (`.stderr` contains the id), `CV2-12_file_from_string.rs`, `CV2-14_set_numeric_id.rs` (offending line ends `// [CV2-12]` / `// [CV2-14]`); `crates/noctis/tests/ui/cid/CID-04_route_without_handler.rs` (`.stderr` has `[CID-04]`). Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`.
- `docs/discord-rules.md` -- CID-04 Enforcement → `Compile-time (`R: Handled` bound on component constructors) / App-build`, How adds "`noctis-ui` component constructors require `R: Handled`, which only `#[handler]` implements (`[CID-04]`, FR-58)"; CV2-05 How: traits `TopLevel`, `ContainerChild`, `SectionAccessory`, `RowItem`; CV2-07 How: the `SectionAccessory` bound's message cites `[CV2-07]`, arity trait in Story 4.4.
- `xtask/rules-pending.toml` -- remove the four 4.1 entries.
- `xtask/src/skill_check/` -- delete `pending.rs` and `xtask/skill-pending.toml`; add `components.rs` implementing `component-entries`: for each snapshot `struct`/`enum` outside `noctis::rules::`/`noctis::codec::` whose name is in `BUILDERS`, the skill's `references/components-v2.md` or `references/modals.md` must have exactly one heading (`#`…`######`) naming `` `Name` `` as a code span, no heading may name two `BUILDERS` names, and the entry body (to the next heading of the same or higher level) must contain `**Where:**`, `**Limits:**` and `**Builder:**`. Problems: `skill-check: <SKILL_DIR>: component-entries: <Name> has no entry`, `skill-check: <file>:<line>: component-entries: <Name> has <n> entries`, `…: heading names more than one component builder`, `…: entry for <Name> lacks **Where:**` (per label). Summary becomes `<N> public items mentioned, <E> component entries checked, <F> template files linted`. Update module docs and `xtask/tests/skill_check.rs` (replace pending tests with: no builders → ok; builder with full entry → ok; missing entry; missing label; duplicate; two names in one heading; entry in modals.md accepted).
- `xtask/public-api.txt`, skill `SKILL.md` API hash -- `api-snapshot --bless`, then the hash.
- `examples/project-template/.claude/skills/noctis/references/components-v2.md` -- one heading per builder (16, e.g. "## 9a. Route button: `RouteButton`") each with `**Where:**`, `**Limits:**`, `**Builder:**` using the real 4.1 names; mark 4.2-4.4/9.3 items `[planned]`; document `RouteComponents` (prelude), `Handled`/`#[handler]` and the `[CID-04]` error, `SelectRoute`, attachments, `Components`, deterministic ids, unresolved ids are finalized by the Responder; fix `Text` → `TextDisplay`, `ContainerChild` enum → `ContainerComponent`. `views-and-widgets.md`: `Into<ContainerComponent>`.
- `examples/project-template/src/modules/{dashboard,profile,setup}.rs` -- reconcile to 4.1 names: `TextDisplay`, `ContainerComponent`, `Container::push`, `LinkButton`, fixed-size `ActionRow::buttons([..])` instead of `try_buttons` (branch on the optional button); leave `V2Message`, `rgb!`, `cx.*`, `Layout` as they are ([planned] in the skill). Update `AGENTS.md` only if a convention line names a changed item.

**Acceptance Criteria:**
- Given `noctis::ui`, when a bot crate builds every component in the Matrix, then each serializes exactly as the Matrix states and the insta snapshots in `crates/noctis-ui/tests/snapshots/` match.
- Given the `tests/ui/cv2` and `tests/ui/cid` fixtures, when `cargo test --release -p noctis --test ui` runs, then each fixture fails to compile and the `.stderr` of CV2-05, CV2-07 and CID-04 fixtures contains `[CV2-05]`, `[CV2-07]` and `[CID-04]`.
- Given a derived Route with `impl ::noctis::__private::ui::Handled`, when the facade test builds and resolves its components, then the JSON matches; without the impl `.button()` fails with `[CID-04]`.
- Given the snapshot exports the component builders, when `cargo run --release -p xtask -- skill-check` runs, then it passes with every builder entry present and fails (tested) for a missing/incomplete entry.
- Given the rules-pending edits and catalog changes, when `rules-catalog` runs, then it passes with CV2-05, CV2-07 (diagnosed), CV2-12, CV2-14 (by construction) and CID-04 (diagnosed) covered by evidence, not pending.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why `SectionAccessory` cites CV2-07:** CV2-07 defines a Section as texts plus exactly one Button-or-Thumbnail accessory; with a two-argument constructor the only way to "omit" an accessory is to pass a non-accessory (e.g. `()`), and that error names CV2-07. CV2-05 fixtures cover misplaced Thumbnails/Buttons through `ContainerChild`/`TopLevel`.
- **Why CID-04 for `Handled`:** FR-58's compile-time half needs a catalog id for its fixture; "exactly one handler per Route" already implies at least one, so CID-04 gains a Compile-time tier; its App-build unit evidence stays pending (7.1).
- **Bounds on methods:**
  ```rust
  pub trait RouteComponents: Route + Sized {
      fn button(self) -> RouteButton<Self>
      where Self: Route<Payload = Self> + Handled + Send + Sync + 'static { … }
  }
  impl<R: Route> RouteComponents for R {}
  ```
  A bound on the blanket impl instead would yield E0599 without `[CID-04]`.
- **Unresolved ids:** core (Epic 7) walks `custom_ids_mut`, calls `resolve` with the envelope/nonce/signer, mints Refs for returned slots and calls `finish` (AD-18); 4.1 only provides the hooks and a serde error if any id is still unresolved.
- Hand-written Routes whose Payload is not `Self` cannot build components (derived Routes always have `Payload = Self`); logged in the memlog.

## Verification

**Commands** (repo root; CI is paused, local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0, incl. `noctis-ui` snapshots, facade `components` and `ui` suites, doctests, xtask tests.
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0 (insta's deps licensed per `deny.toml`).
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- exit 0.
- `bash .github/scripts/check-build-config.sh` -- exit 0.
- `test ! -d target/debug` -- true.
