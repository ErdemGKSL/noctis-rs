# Epic 9 Context: Attachments, webhooks and advanced component surfaces

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Complete the Components V2 surface for bot authors: files attached to Views are named, tracked and diffed automatically on edit; raw component JSON and raw REST requests cover anything twilight does not model yet; components get declared Route defaults with per-render overrides; modals open pre-filled from the entity being edited; and Views can be sent and edited over webhooks with the same V2 safety as interaction replies. Noctis stays a Components V2 wrapper, not a widget framework.

## Stories

- Story 9.1: Content-addressed attachments with edit diffs
- Story 9.2: Raw escape hatch for components and REST
- Story 9.3: Route component defaults with per-render overrides
- Story 9.4: Modal pre-fill (merged into Story 9.3; not implemented separately)
- Story 9.5: Views over webhooks

## Requirements & Constraints

- Attachments: generated `attachment://` names are unique in a message; on edit, still-referenced attachments are kept by id without re-upload, unreferenced ones dropped, new ones uploaded as `files[n]` matching `attachments[].id`. Oversized file (Upload Budget = interaction `attachment_size_limit`, else 20 MiB), request over 25 MiB, or too many files is a typed error before any HTTP request. A rendered image in a Thumbnail/Media Gallery without alt text: error in Strict, warning in Lenient; other images keep the existing warning; alt over 1024 rejected.
- Raw escape hatch: raw component JSON counts toward the component total and serializes verbatim; unknown incoming component types still dispatch and are exposed as raw JSON; an edit never re-sends unparsed components (uses only the View's rendered tree); `ctx.http().raw(..)` uses the App's single HTTP client, rate limiting and configured proxy. Document stability caveats.
- Route defaults: overrides apply for one render only; the result is validated against the Limits (Strict/Lenient) after overrides.
- Modal pre-fill: Text Inputs get initial values; selects, Radio Groups and Checkbox Groups get default selections. A value over 4000, over `max_length`, or more defaults than `max_values` is a typed error before sending.
- Webhooks: always `with_components=true` on execute and edit; only flags valid for the endpoint; legacy fields nulled on edit; interactive components through a non-owned webhook are a compile error (plus typed error dynamically); forum/media channel without `thread_id`/`thread_name` is a typed error; webhook name must be 1–80.
- Every story's new public API updates the project template, its AGENTS.md and the Usage Skill (attachments/alt text, raw hatch, defaults/overrides, modal pre-fill recipe incl. the template's bio modal, webhooks); re-bless the API snapshot and the skill's API hash; `skill-check` passes.
- New or changed limits update `docs/discord-rules.md` and `noctis-rules` with the same rule id (ATT-01..07, CV2-03/08/09/12/16, MSG-07/08, MOD-05/07, WH-01..05).

## Technical Decisions

- Attachments are content-addressed (`noctis-ui`): `Attachment { name, bytes: Bytes, content_type, alt }`; `Attachment::new(bytes, ext)` names `<first 16 hex of blake3(bytes)>.<ext>`; `Attachment::with_digest` uses a cache-key digest; `Attachment::rendered(bytes, digest: [u8; 32], ext: &'static str)` takes primitives only (no `noctis-ui` -> `noctis-ports` edge) and marks origin `Rendered`, which is how the validator recognises rendered images. No other naming scheme.
- A rendered View is `(MessageTree, AttachmentSet)` keyed by name, so one image used twice uploads once. `noctis-ui` only checks every `attachment://` reference has a member; core's Responder alone enforces the Upload Budget, builds the multipart body and computes the edit diff. Views hold only ready attachments; rendering is awaited inside handlers so auto-defer covers it.
- The Responder owns every reply, including webhook sends that must never silently drop components. Flag sets are typed per endpoint.
- Noctis owns the V2 model: its types serialize directly to Discord JSON as raw twilight-http bodies; twilight-model is used only for ids, interactions, events and commands. Unmodelled incoming component data is kept as `serde_json::Value`, never dropped. Modal submissions decode from raw interaction bytes. Pre-fill values go through the same validator.
- One struct per interaction: kind derives (`Button`, `StringSelectMenu`, ..., `Modal`) with a mandatory `defaults(..)` block as an associated const on `ComponentRoute`. Required keys: Button `style` and `label` or `emoji`; selects `placeholder` (optional `channel_types`, `options = ChoicesEnum` for string selects); modal `title`. Missing block/key or unknown key is a compile error naming required keys; literals checked against limits at compile time; omitted keys default to `min_values = 1`, `max_values = 1`, `disabled = false`. Literal vs localized text is decided by syntax, never by a cargo feature: a string literal is hard-coded text checked against limits at compile time; a path (`keys::X`, a `MessageKey` from `i18n!`, Story 11.1) is a locale key; until 11.1 ships, a path value is a compile error saying locale keys are planned for Epic 11. Overrides take `impl Into<Text>` (`&str`/`String` or `MessageKey`).
- `.build()` emits the component from defaults plus the Payload as `custom_id`; chainable setters `.label/.emoji/.style/.disabled/.placeholder/.options/.default_values/.min_values/.max_values`, and for modals `.title(..)` and `.value(field, ..)`. A string select without `options` in defaults returns a typestate builder requiring `.options(..)` (trybuild fixture).
- Multi-field overrides: each kind has an overrides struct (`ButtonOverrides`, `StringSelectOverrides`, `UserSelectOverrides`, `RoleSelectOverrides`, `MentionableSelectOverrides`, `ChannelSelectOverrides`, per modal `<Modal>Overrides`) with all `Option` fields and `Default`, applied via `.overrides(..)` with `..Default::default()`; composes with chainable setters (later call wins), validated the same way.
- Modals: non-input fields are the Payload; input fields pre-fill from the struct value (`None` = empty); the derive also generates `X::blank(<payload fields>)`; pre-fill uses struct update syntax `BioForm { bio: Some(v), ..BioForm::blank(user) }`. The struct is submitted data, not layout: input components come from field attributes. `<Modal>Overrides { title, <input>: InputOverrides { label, description, placeholder } }` changes presentation only, never values.
- Non-owned webhooks accept only a non-interactive `StaticMessage` type (WH-02 enforced in types, trybuild `compile_fail`).
- Plugins attach only through core's `Plugin` seam; HTTP fakes drive the real paths in tests.

## Cross-Story Dependencies

- Builds on Story 4.1 (`Attachment`/`AttachmentSet`, V2 types), 4.3 (runtime validator and alt-text warning), 4.5/4.6 (modal model and `derive(Modal)`), 5.9 (single warm HTTP client, proxy), 6.11 (one struct per interaction, `defaults(..)`), Epic 8 (`ctx.respond`/`ctx.update`, views as plain functions).
- Story 9.1's `Attachment::rendered` and Responder diff are consumed by Epic 10 (`ctx.render`). Modal pre-fill (former 9.4) ships inside Story 9.3; the template's bio modal pre-fill is reconciled there. Key-looking literals in the template become `keys::` paths in Story 11.1.
