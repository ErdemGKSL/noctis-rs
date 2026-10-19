# Epic 4 Context: Components V2 and modal model with limit validation

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Give bot authors Noctis's own typed model for every Components V2 message and modal, where illegal structures do not compile, literals and literal trees are checked at `cargo check`, and every Discord Limit is validated at runtime before anything is sent. The epic is pure (`noctis-ui` over `noctis-codec` and `noctis-rules`, plus macros in `noctis-macros`) and verified with JSON snapshots, trybuild fixtures and unit tests. It removes dependence on twilight-model's incomplete component types and prevents Limit-related 400s.

## Stories

- Story 4.1: Typed V2 message components with placement in the type system
- Story 4.2: V2 messages, legacy messages, flags and mentions
- Story 4.3: Runtime Limit validator with path-precise errors
- Story 4.4: Compile-time checks for literals, arity and the `components!` tree macro
- Story 4.5: Modal component model and modal validation
- Story 4.6: `derive(Modal)` and typed modal submissions from raw JSON

## Requirements & Constraints

- Components V2 for anything interactive; plain `content`/embeds only via `LegacyMessage` (plain text, at most 5 Action Rows).
- V2 messages always set `IS_COMPONENTS_V2` and have no `content`, `embeds`, `poll`, `sticker_ids`, `shared_client_theme`. Legacy-to-V2 edits explicitly clear legacy fields; V2-to-legacy is a typed error.
- `allowed_mentions.parse` defaults to `[]`; opt-in via typed `Users::All | Users::Only(ids)` (and roles), so `parse` plus explicit ids is unbuildable.
- Runtime validator: 40 components at every depth (accessories included), 4000 Text Display chars via `discord_len`, per-field limits, duplicate `custom_id`/numeric ids rejected, every `attachment://` reference resolves. Errors name path (e.g. `container[0].section[2].accessory.label`), rule id, limit and actual value. `cost(&tree)` returns count, chars and warnings without sending.
- Lenient mode truncates text with `…` and warns, never for structural rules (CV2-03, CV2-05, CV2-06, CID-01, CID-02). Strict mode warns on missing alt text for non-`Rendered` images.
- Modals: top level only Label or Text Display (max 5); Labels wrap Text Input, modal selects, File Upload, Radio Group (2–10), Checkbox Group (1–10), Checkbox (no `required`); no message components; modal selects have no `disabled`; never emit Action-Row-wrapped Text Input or Text Input `label`; `custom_id` from a Route.
- Modal submissions decode from raw interaction JSON `&[u8]`, including values twilight drops; unknown data kept as `serde_json::Value`; missing/ill-typed fields are typed errors, never panics. File Upload attachment type has a download helper and documents the ephemeral CDN URL.
- Every compile-time rule has a compile-fail test: Noctis-diagnosed ones have `[<RULE-ID>]` in `.stderr`; by-construction ones carry `// [<RULE-ID>]` on the offending line. Boundary-pass tests (5 buttons, 40 components, 4000 chars, 80-char label). Remove matching `rules-pending.toml` entries.
- Each story updates the Usage Skill (one entry per component with placement, Limits, builder; `references/components-v2.md` for macros), the Project Template, `api-snapshot` and `skill-check`.

## Technical Decisions

- All limits live once in `noctis-rules` (sync, no I/O); stable rule ids from `docs/discord-rules.md`. Every measure/validate function takes `&Limits`; `discord_len` is the only length function. Macros call `noctis-rules` directly, never copying constants; they check against defaults.
- Enforcement ladder: compile time for literals/fixed shapes via proc-macro errors, trait bounds with `#[diagnostic::on_unimplemented(message = "[<RULE-ID>] …")]`, or construction. Never a `const` assert in generic code (must fail under `cargo check`). Runtime only for runtime data.
- Placement via marker traits `TopLevel`, `ContainerChild`, `SectionAccessory`, `RowItem`; arity via const-generic traits implemented only for legal `N` (rows 1–5, section texts 1–3, gallery 1–10). `label!`, `placeholder!`, `rgb!` measure literals; `try_from` is the runtime path using the same function.
- Types serialize directly to Discord JSON with unset fields omitted (no `content_type: null`), sent as raw twilight-http bodies; twilight-model only for ids, interactions, events, commands. Typed flag sets `CreateFlags`, `EditFlags`, `CallbackFlags` per endpoint.
- Components built only from a Route value (`BuyButton { .. }.button()`, selects require a Route declared `select`), bound `R: Handled`; no string `custom_id` constructor. Tests implement `Handled` via `#[doc(hidden)]` `::noctis::__private::ui::Handled` (not public API).
- `Attachment { name, bytes: Bytes, content_type, alt }` named `<first 16 hex of blake3(bytes)>.<ext>`, or `with_digest`; `Attachment::rendered(bytes, digest, ext)` marks origin `Rendered`. `AttachmentSet` keyed by name; `AttachmentRef` obtainable only by adding; `File` takes only `AttachmentRef`. Numeric ids assigned deterministically from the tree. Upload Budget is enforced by core's Responder, not here.
- `components!` (in `noctis-macros`, re-exported as `noctis::components!`) expands into typed builder calls under `::noctis::__private::ui`, yields `Components`, and emits span-precise `compile_error!`s with rule ids; holes count 1, spreads 0 (static lower bound only). Unrelated to the image macro `view!`.
- `noctis-ui` never names gateway types like `RawJson`; `derive(Modal)` output takes bytes.

## Cross-Story Dependencies

- Depends on Epic 1 (`noctis-rules`, rules catalog, xtask gates, Usage Skill) and Epic 2 (`Route`, `Payload`, codec in `noctis-codec`).
- 4.1 underlies all others; 4.3's validator is the runtime backstop for 4.4's static checks; 4.5 precedes 4.6.
- Later: `Handled` is implemented by the `#[handler]` macro (Epic 7); core passes `RawJson` bytes from Story 7.8; Views/widgets (Epic 8) consume `Components`/`V2Message`; rendered-image alt text becomes an error in Story 9.1.
