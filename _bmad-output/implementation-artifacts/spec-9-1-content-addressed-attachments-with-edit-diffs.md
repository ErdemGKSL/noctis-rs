---
title: 'Story 9.1: Content-addressed attachments with edit diffs'
type: 'feature'
created: '2026-10-22'
status: 'done'
baseline_revision: '7b3e9ebcd014c0f463fbf433126b04145aeb0beb'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Template AGENTS.md and skill references still point modal pre-fill at "[planned, Story 9.4]" although Story 9.4 was merged into Story 9.3.
    evidence: |-
      examples/project-template/AGENTS.md:55, references/modals.md:51/168 and references/views-and-recipes.md:90/877/1427 name Story 9.4; maintainer-decisions.md section 9 and epics.md (commit d9efbb4, a maintainer-decision commit, not Story 9.1 work) moved it to 9.3. Story 9.3 should retag these lines. Separately, Story 9.1 retagged testing.md's File Upload submit as a bare "[planned]" because no story owns it.
    location: >-
      examples/project-template/AGENTS.md:55
    severity: low
  - summary: >-
      maintainer-decisions.md section 9 overrides one sentence of section 7 without amending section 7 in place.
    evidence: |-
      Section 9 says it "replaces section 7's a literal without the i18n feature; with it, a locale key", but section 7 still carries the old text. It came from maintainer-decision commit c14f940, not from Story 9.1; the page's own header asks for the other text to be fixed in the same change.
    location: >-
      _bmad-output/planning-artifacts/maintainer-decisions.md section 7
    severity: low
---

<intent-contract>

## Intent

**Problem:** A View can hold an `AttachmentSet`, but the Responder refuses any message with attachments (`ResponderError::AttachmentsUnsupported`), every edit sends `"attachments": []` (dropping files), the Upload Budget (ATT-01..03) is never applied, and a rendered image without alt text is silently accepted (FR-30, UX-DR2).

**Approach:** The Responder uploads the attachments the tree references as multipart (`payload_json` + `files[n]`), remembers each message's attachment ids, and on every edit sends a keep-list (known names by id, new names uploaded, the rest dropped). One budget check runs right before any request. `noctis-ui` turns a rendered image without alt text into a Strict error / Lenient warning and gains an `alt!` literal macro (ATT-06).

## Boundaries & Constraints

**Always:**
- **Used attachments.** `V2Message::used_attachments(&self) -> impl Iterator<Item = &Attachment>` (new, public, `noctis-ui/src/message/mod.rs`): every set member whose `attachment://<name>` URL appears at a File, Thumbnail or MediaItem site of the tree (built with an `AttachmentRef` or with a hand-written `Media::url("attachment://<name>")`), each once, in name order. The Responder uploads or keeps only these. An unreferenced member is never sent: one `tracing::debug!` (target `noctis::responder`, field `name`) per member.
- **Rendered alt (FR-30, UX-DR2).** A Thumbnail or MediaItem site is *rendered* when its media's `AttachmentRef` has origin `Rendered`, or its URL names a set member with origin `Rendered`. Only the site's own alt counts (`Thumbnail::alt` / `MediaItem::alt`), never `Attachment::alt`. If that alt is `None`, empty or whitespace-only:
  - in Strict mode, `validate` returns a violation `RuleId::ATT_06`, path `<site path>.description` (for example `section[0].accessory.description` or `gallery[1].item[0].description`), `Bound::Describe("alt text on a rendered image (FR-30)")`, `Actual::Text(<alt or "">)`, help ``add `.alt(..)` to the Thumbnail or MediaItem (UX-DR2)``;
  - in Lenient mode it is `LimitWarning::MissingAlt { path: <site path> }`;
  - `cost()` always reports it as `MissingAlt`.

  The check is `noctis_rules::attachment::validate_rendered_image_alt(alt: Option<&str>, limits: &Limits) -> Vec<Violation>` (new, empty path, `limits` unused like `validate_attachment_ref`); `noctis-ui` sets the path and help. Violation order: …, ATT-05, ATT-06 (attachment alts), then rendered-alt ATT-06 in pre-order, then MSG-01/MSG-09/MSG-04 as today. A non-rendered image without alt keeps Story 4.3's `MissingAlt` warning in both modes. A File site has no alt requirement.
- **`alt!` / `AltText`.** `noctis_ui::AltText` comes from `literal_newtype!` in `noctis-ui/src/literal.rs`, checked by `validate_attachment_description` (ATT-06, at most `attachment_description_len` = 1024 by `discord_len`, empty allowed). It has `try_new`, `as_str`, `AsRef<str>`, `Into<Cow<'static, str>>`, `TryFrom<&'static str | String>` and `__from_checked`, like `Label`. `noctis_macros::alt` reuses `literal::expand_text` with a new `ALT` `TextMacro` (rule ATT-06, ty `AltText`). It is re-exported as `noctis::alt!` (doc line beside `label!`) and `noctis::ui::AltText`.
- **Known files.** New module `noctis-core/src/responder/files.rs`. `KnownFiles` is an ordered list of `(filename, Id<AttachmentMarker>)`, built from a message JSON's `attachments[]`. An entry whose `id` is not a non-zero snowflake string, or whose `filename` is not a string, is skipped. On a duplicate filename the first entry wins. It is tracked for:
  - the source message: `SourceMessage` gains `files: KnownFiles`, read from the raw interaction's `message.attachments`. It drops `Copy` and keeps `Clone`; `DETACHED_SOURCE` stays a `const`, using an empty `Vec`.
  - the original: `Original` gains `files`. It is set from the type-4 `with_response` resource message, from the source on `update`/`defer_update`, from every edit response (`PATCH @original` or a bot-token channel edit), and from the Dispatcher's message reply. It is empty after a type-5 defer.
  - each follow-up: `FollowupHandle` gains `files: Mutex<KnownFiles>`, set from the follow-up response (`wait=true`) and from each edit response.

  When a response is missing or does not parse (204, detached), the known list becomes the kept entries only. New uploads stay unknown, so a later edit re-uploads them, which is safe.
- **Attachment entries (ATT-04, MSG-08).** For one send, walk `used_attachments()` in name order:
  - on an edit, a name found in the target's `KnownFiles` becomes the kept entry `{"id": "<snowflake>"}`;
  - every other name becomes the new entry `{"id": n, "filename": name}`, plus `"description": alt` when `Attachment::alt` is set. `n` is a JSON integer counting `0, 1, …` over the new entries only, and it is uploaded as the part `files[n]` with filename `name`.

  Placement of the array:
  - Create bodies (type-4 `data`, follow-up, Dispatcher reply, notice) carry `attachments` only when at least one file is used, so the no-file body stays byte-identical to today.
  - Edit bodies (type-7 `data`, `PATCH @original`, follow-up edit, bot-token channel edit) always carry `attachments`. Names known but not used are omitted, so Discord drops them.
  - A legacy message has no attachments and always sends `[]` on an edit.
- **Upload Budget (ATT-01..03), one check before any request.** It runs inside body building, after the entries and the payload JSON exist and before the transport call (also when Detached). It calls `noctis_rules::attachment::validate_attachments` with:
  - one `AttachmentDecl` per used attachment in name order, `size` = byte length for new entries and `0` for kept ones;
  - `json_len` = byte length of the serialized `payload_json`, which for callbacks is the whole `{"type":…,"data":…}`;
  - `file_limit` = `InteractionTarget::file_limit`, else `None`, which means `Limits::attachment_bytes`.

  Any violation is `ResponderError::Invalid(UiError::Limits { violations })` in both modes. No request is sent and the responder state is unchanged.
- **`attachment_size_limit`.** `InteractionTarget` gains `file_limit: Option<usize>`, parsed from the raw interaction's top-level `attachment_size_limit` in `pipeline.rs` and `pipeline/component.rs`. Only a positive JSON integer counts; one above `usize::MAX` saturates. Anything else is `None`.
- **Wire.** With no new upload the body is JSON exactly as today. With at least one upload it is a `twilight_http::request::Form`: `json_part(b"payload_json", json)` first, then `file_part(b"files[n]", name, bytes)` in ascending `n`.
  - The type-4/7 callback sends it over the edge as `CallbackBody::Raw { content_type: form.content_type(), body: form.build() }`.
  - Every other route uses `Request::builder(route).form(form)`, with the same `use_authorization_token` as today, so interaction-token routes never carry the bot token (INT-13).
  - A type-7 update with at least one upload is sent `with_response=true`, so the new ids are learned.
  - `Attachment::mime` is not sent: Discord takes the type from the extension.
- **Removals.** `ResponderError::AttachmentsUnsupported` is deleted, together with every mention of it in code, tests and the skill.
- **Testkit.**
  - `RecordedRequest::json()` returns the `payload_json` part for a `multipart/form-data` request.
  - New `RecordedRequest::files() -> Vec<RecordedFile>`. `RecordedFile` is `#[non_exhaustive]` with pub `name` (`files[n]`), `filename` and `bytes: Bytes`, in body order, and is empty for non-multipart requests.
  - TestApp's wire echoes `attachments` on every message it answers: a callback resource, a follow-up, `@original` and a follow-up edit, and the message of a later click. A kept entry `{"id":"<sf>"}` copies that message's previous object, and an unknown id is dropped. A new entry `{"id":n,…}` gets a fresh snowflake, `filename`, `size` (the `files[n]` part length), `url` and `proxy_url` `https://cdn.discordapp.com/attachments/<channel>/<id>/<filename>`. The result parses as a twilight `Message`.
- Every public API change (`used_attachments`, `AltText`, `alt!`, `validate_rendered_image_alt`, `RecordedRequest::files`/`RecordedFile`, the removed variant) updates the Usage Skill, the template `AGENTS.md`, `xtask/public-api.txt` (re-blessed) and the skill's `API hash` in the same change.

**Never:**
- No second naming scheme and no change to `Attachment`/`AttachmentSet` naming, dedup or `rendered(bytes, digest, ext)`. There is no `noctis-ui` → `noctis-ports` edge.
- The Upload Budget is not checked in `noctis-ui` or `prepare`, and Lenient mode never truncates or drops a file to fit.
- No new crate dependency: use twilight `Form` and the `bytes` already in the workspace.
- No webhook (9.5), raw hatch (9.2) or `ctx.render` (Epic 10) work.
- Do not edit `_bmad/`, and do not weaken `.cargo/config.toml`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error |
|---|---|---|---|---|
| 1 | No files, reply | V2 text only | JSON body byte-identical to today, no `attachments` key | — |
| 2 | One file, type-4 reply | gallery item → `a.png` | multipart; `payload_json` `data.attachments=[{"id":0,"filename":"<h>.png"}]`, part `files[0]` filename `<h>.png` holds the bytes | — |
| 3 | Same image twice | 2 items, same ref | one entry, one `files[0]` | — |
| 4 | Unreferenced member | set has `a`,`b`; tree uses `a` | only `a` sent; debug log names `b` | — |
| 5 | File alt | `Attachment::alt("x")` on new | entry has `"description":"x"` | — |
| 6 | Hand-written URL | `Media::url("attachment://<name>")` of a member | counted as used and uploaded | — |
| 7 | Edit, all kept | known `{a→11}`, view uses `a` | JSON (no multipart) `attachments=[{"id":"11"}]` | — |
| 8 | Edit, mixed | known `{a→11,c→12}`, view `a`,`b` | `[{"id":"11"},{"id":0,"filename":b}]`, `files[0]`=b; `c` absent (MSG-08) | — |
| 9 | Edit, all dropped | known `{a→11}`, no files | `attachments: []` | — |
| 10 | Update keeps source files | component raw `message.attachments=[{id:"11",filename:a}]`, view `a` | type-7 `data.attachments=[{"id":"11"}]`, JSON, no `with_response` | — |
| 11 | Update uploads | view `b` new | multipart type-7, `?with_response=true`; ids learned from `resource.message` | — |
| 12 | Edit after type-5 defer | defer then `edit_reply` with `a` | `a` new (`{"id":0,…}`) | — |
| 13 | Edit after type-6 defer | `defer_update`, `edit_reply` with source's `a` | kept by id | — |
| 14 | Second edit | reply uploaded `a` (resource id 21), then edit with `a` | `[{"id":"21"}]`, no upload | — |
| 15 | Unknown ids | response without message (204/detached), then edit with `a` | `a` re-uploaded as new | — |
| 16 | Follow-up + edit | follow-up uploads `a` (response id 31), `handle.edit` with `a` | edit keeps `{"id":"31"}` | — |
| 17 | Expired token edit | public, known id, token expired | bot-token `PATCH /channels/…` with the same keep-list/multipart | — |
| 18 | Legacy edit | `LegacyMessage` | `attachments: []` | — |
| 19 | Malformed known | `attachments` missing / id `0` / id non-string / filename missing / dup filename | skipped; first dup wins; never panics | — |
| 20 | ATT-01 boundary | 1 new file of 20 971 520 B, no limit | sent | — |
| 21 | ATT-01 +1 | 20 971 521 B | no request; `Invalid(Limits)` ATT-01 `attachments[0].size`, note file name | typed error |
| 22 | Interaction limit lower | `attachment_size_limit=10485760`, file 10 485 761 | no request, ATT-01 bound 10485760 | typed error |
| 23 | Interaction limit higher | limit 52 428 800, file 30 MiB | ATT-01 ok, ATT-02 fails (`request`) | typed error |
| 24 | Kept files exempt | known 10 MiB ×3 kept | sizes 0, no ATT-01/02 | — |
| 25 | ATT-02 boundary | new bytes + json_len = 26 214 400 | sent; +1 → ATT-02 `request` | typed error |
| 26 | ATT-03 | 10 used → sent; 11 used (kept+new mixed) → ATT-03 `attachments` | | typed error |
| 27 | Budget never mutates state | ATT-01 fail while Pending | no request; state stays `PENDING`; a following valid `reply` is sent as a type-4 callback | — |
| 28 | Lenient budget | Lenient + 21 MiB file | still ATT-01 error | typed error |
| 29 | `file_limit` parse | absent, `0`, `-1`, `1.5`, `"5"`, `u64::MAX` | None,None,None,None,None,`usize::MAX` (64-bit: u64::MAX) | — |
| 30 | Rendered, no alt, Strict | Thumbnail of rendered ref, alt None / `""` / `"  "` | `Err` ATT-06 `section[0].accessory.description`, `Actual::Text("")`/`""`/`"  "` | validation error |
| 31 | Rendered, no alt, Lenient | same | `Ok`, warning `MissingAlt{path:"section[0].accessory"}` | — |
| 32 | Rendered with alt | `.alt("Card")` | no violation, no warning | — |
| 33 | Rendered, file alt only | `Attachment::rendered(..).alt("x")`, item without alt | Strict error (file alt does not count) | validation error |
| 34 | Rendered via URL | `Media::url("attachment://<rendered name>")`, no alt | treated as rendered (Strict error) | validation error |
| 35 | Rendered in File | `File::new(rendered_ref)` | no alt rule | — |
| 36 | Plain no alt | non-rendered item without alt | `MissingAlt` warning in both modes (unchanged) | — |
| 37 | cost() | rendered without alt | `MissingAlt` warning, never Err | — |
| 38 | Order | CV2 + ATT-05 + ATT-06 file alt + rendered alt + MSG-09 | that order | — |
| 39 | `alt!` ok | `alt!("")`, 1024×`a`, 512×`😀` | compiles, `AltText` | — |
| 40 | `alt!` over | 1025×`a`, 513×`😀` | compile error `[ATT-06] limit at most 1024, actual 1025` / `1026` | compile error |
| 41 | `alt!` misuse | `alt!(x)`, `alt!()` | `[ATT-06] alt! takes one string literal`, help names `AltText::try_new` | compile error |
| 42 | `AltText::try_new` | 1024 ok; 1025 → `UiError::Limits` ATT-06 | | typed error |
| 43 | `validate_rendered_image_alt` | `None`, `Some("")`, `Some(" \n")` → 1 violation; `Some("a")`, `Some(" a ")` → empty | | — |
| 44 | `used_attachments` | empty tree; ref twice; URL to non-member | empty; once; not included | — |
| 45 | Testkit multipart | multipart request | `json()` = payload_json; `files()` lists parts; JSON request → `files()` empty | — |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/attachment.rs` -- `Attachment` (name/origin), `AttachmentSet::get/iter`, `AttachmentRef` (private fields: compile_fail doctest target for ATT-05).
- `crates/noctis-ui/src/limits/mod.rs` -- `V2Message` `validate_with` (L272–297, violation order), `cost_with`, `check_attachment_refs` (L501), `check_attachment_alts` (L523), `missing_alt` (L560: today skips rendered; split into rendered-violation vs warning by mode).
- `crates/noctis-ui/src/limits/walk.rs` -- `Site::url()` / `Site::image()` (L44–60), `v2_sites`; reuse for `used_attachments`.
- `crates/noctis-ui/src/message/mod.rs` -- `V2Message` (fields `pub(crate)`), add `used_attachments`.
- `crates/noctis-ui/src/literal.rs` -- `literal_newtype!`; add `AltText`. `crates/noctis-ui/src/lib.rs` L195–212 docs, L244 re-exports.
- `crates/noctis-ui/tests/validate.rs` L1414–1460 -- `cv2_11_rendered_image_needs_no_alt` must become the new rendered tests; keep `cv2_10_plain_attachment_without_alt_warns`.
- `crates/noctis-rules/src/attachment.rs` -- `validate_attachments` (ATT-03, ATT-01, ATT-06, ATT-02 order; `file_limit` replaces `attachment_bytes`); add `validate_rendered_image_alt`.
- `crates/noctis-macros/src/literal.rs` (`TextMacro`, `expand_text`), `crates/noctis-macros/src/lib.rs` L110–135 (`label`/`content` pattern).
- `crates/noctis/src/lib.rs` L1602–1611 (literal macro re-exports), `pub mod ui` (re-export `AltText`); `crates/noctis/tests/ui.rs` globs `tests/ui/**/*.rs`; model fixture `tests/ui/cv2/CV2-08_label_81.{rs,stderr}`.
- `crates/noctis-core/src/responder.rs` -- `ResponderError` (L352–436, delete `AttachmentsUnsupported` L375–378); `prepare` (L1578, drop the rejection); `send_reply` (L1686, type 4 `with_response=true` → known files), `send_update` (L1801), `edit_prepared` (L1918), `send_follow_up` (L2002), `notice` (L2052), `FollowupHandle` (L1050) and its `edit` (~L1065–1091); `SourceMessage` (L1148)/`RawMessage` (L1168)/`from_interaction` (L1184); `Original` (L1224, Copy today); `Shared::build` (L1349, `source.map_or` of nonces); body builders `callback_data`/`edit_body`/`create` (L2224–2251, the `"attachments": []` insert to replace).
- `crates/noctis-core/src/responder/transport.rs` -- `InteractionTarget` (L42, add `file_limit`), `callback` (L262, `CallbackBody::Json` → Raw for multipart), `send` (L376, JSON body → Form), `edit_reply`/`follow_up`/`edit_message`/`channel_edit`, `posted`/`callback_message_id` (parse attachments alongside).
- `crates/noctis-core/src/edge/callback.rs` L51–61 -- `CallbackBody::Raw { content_type, body }` already exists.
- `crates/noctis-core/src/responder/ergonomics.rs` L199–360 -- `ctx.update`/`respond`/`reply_text` route through `Shared` sends; L345 builds `Original` literally (add `files`).
- `crates/noctis-core/src/pipeline.rs` L893 and `crates/noctis-core/src/pipeline/component.rs` L577/L601 -- `InteractionTarget` and `SourceMessage::from_interaction` construction from `raw`.
- `crates/noctis-core/src/component.rs` L1287 -- `DETACHED_SOURCE` const.
- `crates/noctis-core/src/responder/tests.rs` -- `Fixture`, `discord()` answers, `requests()` (JSON only: add a multipart-aware helper), `attachments_and_bad_token` (L482–495, rewrite); `InteractionTarget {..}` literals at L121/139/153/544/616/927/1027 and `tests/ergonomics.rs` L276 need `file_limit`; `edge/test_support.rs` `Seen` has raw `body` and `headers`.
- `crates/noctis-testkit/src/fake.rs` L268–362 (`RecordedRequest`, `json()`), `crates/noctis-testkit/src/app/wire.rs` (`message_json` L279, `"attachments": []` L304, `run.bodies`, `callback`, `message`).
- `xtask/rules-pending.toml` L50–68 -- remove MSG-08 unit, ATT-04 unit, ATT-05 by-construction, ATT-06 diagnosed (keep ATT-07 → 10.4).
- `docs/discord-rules.md` L254 (MSG-08), L261–266 (ATT-01..06) -- enforcement text.
- `examples/project-template/.claude/skills/noctis/references/{components-v2.md §Attachments L253–278, responder.md L35–50, testing.md L181, limits-cheatsheet.md L94–123/L296–300, images.md}`, `SKILL.md` L23 (API hash), `examples/project-template/AGENTS.md` L53, `examples/project-template/src/modules/profile/view.rs` (already alt-texts the rendered card).
- Read-only: `~/.cargo/registry/src/*/twilight-http-0.17.1/src/request/multipart.rs` (`Form::json_part/file_part/content_type/build`, re-exported as `twilight_http::request::Form`); twilight-model 0.17.1 has no `attachment_size_limit` (read raw JSON).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/attachment.rs` -- add `validate_rendered_image_alt` + unit tests `att_06_rendered_image_alt_*` (matrix 43); module doc mentions it.
- `crates/noctis-ui/src/literal.rs`, `crates/noctis-ui/src/lib.rs` -- add `AltText` (ATT-06) with doctest; export.
- `crates/noctis-ui/src/message/mod.rs`, `crates/noctis-ui/src/limits/{mod.rs,walk.rs}` -- `used_attachments`; rendered-alt detection (ref origin or URL → member origin); Strict violation / Lenient + cost warning; update `validate` rustdoc order and the lib.rs Limits paragraph.
- `crates/noctis-ui/src/attachment.rs` -- `compile_fail,E0451` doctest building an `AttachmentRef` literally, line commented `// [ATT-05]`.
- `crates/noctis-ui/tests/validate.rs` -- replace `cv2_11_rendered_image_needs_no_alt` with matrix 30–38, 44 tests (names cite `att_06`/`att_05`).
- `crates/noctis-macros/src/{literal.rs,lib.rs}`, `crates/noctis/src/lib.rs` -- `ALT` macro + `alt!` re-export + `ui::AltText`; fixtures `crates/noctis/tests/ui/att/ATT-06_alt_1025.{rs,stderr}`, `ATT-06_alt_not_literal.{rs,stderr}`; boundary tests for matrix 39 in `crates/noctis/tests/components_macro.rs` or a new `crates/noctis/tests/alt.rs`.
- `crates/noctis-core/src/responder/files.rs` (new) -- `KnownFiles` (+ `from_message(&Value)`), entry building, budget check, `Form` building, `file_limit(raw)` parser; unit tests matrix 19, 20–29 named `att_01_*`, `att_02_*`, `att_03_*`, `att_04_*`, `msg_08_*`.
- `crates/noctis-core/src/responder/transport.rs` -- `file_limit`, multipart `send`/`callback`, response attachment parsing.
- `crates/noctis-core/src/responder.rs`, `responder/ergonomics.rs`, `component.rs`, `pipeline.rs`, `pipeline/component.rs` -- wire `KnownFiles` through `SourceMessage`/`Original`/`FollowupHandle`; replace `"attachments": []` insertion; `with_response` on uploading type 7; delete `AttachmentsUnsupported`.
- `crates/noctis-core/src/responder/tests.rs` (+ `tests/components.rs`) -- wire tests over `TestServer` for matrix 1–18, 21, 27 (parse multipart from `Seen.body` with the `content-type` boundary; assert no `authorization` header on interaction routes).
- `crates/noctis-testkit/src/{fake.rs,app/wire.rs,lib.rs}` -- multipart `json()`, `files()`/`RecordedFile`, attachment echo; tests in `crates/noctis-testkit/tests/` for matrix 45 and an end-to-end flow (reply with image → click → `ctx.update` same image keeps id, changed image uploads `files[0]`, removed image drops).
- `docs/discord-rules.md` -- ATT-01 (checked by the Responder, `ResponderError::Invalid`), ATT-04/MSG-08 (Responder keep-list, `files[n]` ↔ `attachments[].id`), ATT-05 (unreferenced members never sent), ATT-06 (Compile-time (literal: `alt!`) / Runtime; rendered image in Thumbnail/Media Gallery needs alt: Strict error, Lenient warning, FR-30) -- same ids, tiers parse as today.
- `xtask/rules-pending.toml` -- remove the four 9.1 entries.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` -- one "Amended (Story 9.1)" bullet under AD-22: used = referenced, keep-list ids from source/responses, budget in body building.
- Skill + template files from the Code Map -- document upload/diff/budget, `used_attachments`, `alt!`/`AltText`, rendered-alt rule, `RecordedRequest::files`; drop `[planned]`/`AttachmentsUnsupported` mentions; template `AGENTS.md` mentions `alt!`; re-bless `xtask/public-api.txt`, update `API hash`.

**Acceptance Criteria:**
- Given a handler whose View puts one rendered card in a MediaItem twice and in a Thumbnail, when it calls `ctx.respond(view)`, then exactly one request is sent: a multipart type-4 callback whose `payload_json` lists one attachment entry `{"id":0,"filename":<name>}` and whose single `files[0]` part carries the card's bytes.
- Given a message the bot sent with attachments `a` and `c`, when a component handler calls `ctx.update` with a View using `a` and a new `b`, then the type-7 `data.attachments` is `[{"id":"<a's id>"},{"id":0,"filename":b}]` in name order, only `b` is uploaded, and `c` is absent.
- Given an interaction with `attachment_size_limit` 10485760, when the handler responds with an 11 MiB file, or with 11 used files, or with a request over 25 MiB, then no HTTP request is made and the call returns `ResponderError::Invalid(UiError::Limits{..})` naming ATT-01, ATT-03 or ATT-02.
- Given Strict mode, when a View with a rendered image in a Thumbnail or MediaItem without alt text is sent, then the send fails with an ATT-06 violation at the site's `.description` path before any request; given Lenient mode, then it is sent and the report has a `MissingAlt` warning.
- Given `noctis::alt!` with a 1025-character literal, when `cargo check` runs, then it fails with `[ATT-06] limit at most 1024, actual 1025`.
- Given the testkit, when a bot test replies with an attachment and then clicks a button that re-renders the same View, then `RecordedRequest::files()` is non-empty for the reply and empty for the update, and the update's `json()` keeps the attachment by id.
- Given the finished story, when `xtask rules-catalog` and `skill-check` run, then both pass with the four 9.1 pending entries removed and the API hash matching the re-blessed snapshot.

## Spec Change Log

## Review Triage Log

### 2026-10-21 — Review pass
- verdicts: 35 findings — high 0, medium 4, low 12, false 19, maybe-false 0
- findings:
  - `[false]` `[reject]` (blind) `notice` stores `KnownFiles::EMPTY` and drops what the reply learned — `notice` always sends a text-only `TextDisplay` V2 message with an empty `AttachmentSet`, so there is nothing to learn or keep.
  - `[low]` `[reject]` (blind) kept entries carry no `description`, so a changed `Attachment::alt` on unchanged bytes never reaches Discord — real but rare (same bytes, new file-level alt); the visible alt is the component's; kept-entry `description` is undocumented and the spec's Design Notes deliberately send `{"id"}` only.
  - `[low]` `[reject]` (blind) ATT-02 counts `payload_json` + file sizes, not multipart framing — only a request within a few hundred bytes of 25 MiB is affected; the spec fixes `json_len` as the payload_json length (matrix 25), so changing it is not a direct correction.
  - `[low]` `[reject]` (blind) one upload is copied into the `Form` buffer and `form.build()` — transient copies are real, but the spec mandates twilight `Form` and no new dependency; avoiding them needs a streaming body, far more than a direct fix.
  - `[false]` `[reject]` (blind) `Files::plan` couples a peeked `used_attachments()` to `attachment_set().iter()` — `used_attachments` is literally `self.attachments.iter().filter(..)` over the same set (`message/mod.rs:130`), so the orders cannot diverge; no named harm.
  - `[low]` `[reject]` (blind) `file_limit` re-parses the raw interaction JSON — one extra small serde scan per interaction; folding it into the existing parse adds plumbing, and the spec places the parse in the pipeline.
  - `[false]` `[reject]` (blind) `validate_rendered_image_alt` ignores `limits` and returns an empty path — the spec mandates both, and the rustdoc says "The path is empty and `limits` is not read" (`noctis-rules/src/attachment.rs:160-165`).
  - `[medium]` `[patch]` (blind) no tests for attachments over the `Transport::Message` paths, ATT-03 through the Responder, follow-up budget — grouped with the vgap message-transport finding (ATT-03/ATT-02 part also covered by the budget patch); patch adds `message_transport_attachments` and Responder-level ATT-02/03 no-request tests.
  - `[false]` `[reject]` (blind) `RecordedFile` appears twice in `xtask/public-api.txt` — `noctis::testkit` re-exports it once (`noctis/src/lib.rs:2673`); the second block is cargo-public-api listing `noctis_testkit::fake::RecordedFile` and the root re-export, a deterministic snapshot artifact the gate compares as is; no user-visible harm.
  - `[low]` `[defer]` (blind) template `AGENTS.md` still says "[planned, Story 9.4]"; testing.md File Upload now bare "[planned]" — the 9.4 tags predate 9.1 and became stale through maintainer commit d9efbb4 (Story 9.3 owns the retag); no story owns File Upload submits, so a bare tag is honest.
  - `[low]` `[defer]` (blind) epics/maintainer-decisions edits in range; section 9 contradicts section 7 — those are maintainer-decision commits (d9efbb4..c14f940), not 9.1's `96d0434`; the section 7/9 overlap is pre-existing to this story.
  - `[false]` `[reject]` (blind) testkit `Wire::echo` silently drops unknown kept ids — the spec requires exactly that ("an unknown id is dropped"), and core only ever keeps ids learned from Discord.
  - `[low]` `[reject]` (blind) core tests duplicate the testkit's multipart parser — test-only duplication; the boundary is random so a collision inside file bytes is negligible; sharing needs a new test-support dependency path.
  - `[false]` `[reject]` (blind) uploading `update` silently forces `with_response=true` — the spec mandates it (Wire: "A type-7 update with at least one upload is sent `with_response=true`"); it changes only the response body, not the ack.
  - `[low]` `[reject]` (edge) concurrent `FollowupHandle::edit` calls read one `files` snapshot — needs two simultaneous edits of the same follow-up, which also already race on `format`; a per-handle async lock adds state not demonstrated.
  - `[low]` `[reject]` (edge) concurrent `@original` edits share a stale `Original.files` — same unlikely race as above, pre-existing shape for `Original`; a held lock is more than a direct fix.
  - `[low]` `[reject]` (edge) kept attachment whose alt changed is not resent — same root cause and decision as the blind kept-`description` row.
  - `[low]` `[reject]` (edge) ATT-02 ignores multipart overhead — same root cause and decision as the blind ATT-02 row.
  - `[false]` `[reject]` (edge) a message object with an id but no `attachments` overrides `body.kept` — Discord's message object always carries `attachments` (twilight `Message` requires it), and the testkit echo always sets it.
  - `[false]` `[reject]` (edge) Discord renames a sanitized filename so kept lookup misses — every name is content-addressed `<hex>.<ext>` (no second naming scheme), which Discord does not alter.
  - `[false]` `[reject]` (edge) testkit `parts` mis-parses `filename=` before `name=` — the fake only receives bodies built by twilight `Form`, which always writes `name="..."; filename="..."`.
  - `[false]` `[reject]` (edge) one malformed part makes the whole body non-multipart — same: twilight `Form` never emits a part without the blank-line terminator or a name.
  - `[medium]` `[patch]` (vgap, pre-verified) pipeline wiring of `attachment_size_limit` into `InteractionTarget::file_limit` is untested — patch adds a pipeline-level test (raw interaction with the limit, over-limit reply → ATT-01 with that bound, no callback).
  - `[medium]` `[patch]` (vgap, pre-verified) attachments over the message transport have no test — patch adds `message_transport_attachments` (reply, deferred finish, edit keeps by id).
  - `[low]` `[patch]` (vgap, pre-verified) testkit echo for deferred edits and follow-ups is not asserted end to end — caused by this change's new `outcome.rs`/`wire.rs` arms; patch adds a TestApp flow clicking a deferred-edited message and a follow-up holding files.
  - `[false]` `[reject]` (intent 1) "attached" narrowed to "referenced" — the spec deliberately settles it (Design Notes "Used = referenced"; V2 shows only referenced files) and AD-22 is amended accordingly.
  - `[false]` `[reject]` (intent 2) keep-list knowledge is local — the listed loss cases hold no files to keep (notice is text-only, a type-5 defer has no attachments, detached has no message) or degrade to a safe re-upload as the spec settles.
  - `[false]` `[reject]` (intent 3) Lenient over-length alt truncated, not rejected — the PRD (L668) defines Lenient as truncating text with "…"; the spec keeps Story 4.3's policy.
  - `[false]` `[reject]` (intent 4) only the site's alt counts — Discord shows the component `description`; Story 4.3's test already treats file alt that way; the spec settles it.
  - `[false]` `[reject]` (intent 5) rendered-alt rule tested at `validate`, not the send — `Shared::prepare` calls `noctis_ui::validate` before any body or transport and returns its error, so the send cannot go out.
  - `[medium]` `[patch]` (intent 6) Upload Budget bot-author surface only partly exercised — grouped with the vgap `attachment_size_limit` finding; patch adds the pipeline test plus Responder-level ATT-02/ATT-03 no-request tests.
  - `[false]` `[reject]` (intent 7) typed error reused — `ResponderError::Invalid(UiError::Limits)` is typed and names the rule id; the spec settles it.
  - `[false]` `[reject]` (intent 8) mixed edit diff tested only at the internal surface — the mixed keep/upload/drop case is pinned through `Shared::body`, the same path `ctx.update` uses; no bad outcome.
  - `[false]` `[reject]` (intent 9) ATT-07 and CV2-12 not addressed — ATT-07 stays tracked to Story 10.4 (`xtask/rules-pending.toml`, render formats, excluded by the spec's no-Epic-10 boundary); CV2-12 holds by construction.
  - `[false]` `[reject]` (intent 10) out-of-intent planning edits in the diff — they are separate maintainer-decision commits (d9efbb4..c14f940); the 9.1 commit `96d0434` touches neither file.

## Design Notes

- **Rule id for the rendered-alt check.** No catalog rule covers it. It reuses ATT-06 (alt text), which the story lists, instead of a new non-Discord id. The ATT-06 row gains the Noctis policy, with the same id in docs and `noctis-rules`.
- **Which alt counts.** Only the site's alt counts, never the file's. Discord shows the component `description`, and Story 4.3's existing `cv2_10_plain_attachment_without_alt_warns` test already treats file alt that way.
- **Lenient over-long alt.** In Lenient mode, alt over 1024 keeps Story 4.3's truncation with `…`; Strict rejects it. That is the established Lenient policy for display text.
- **Used = referenced.** V2 messages show only the attachments that components reference, so sending others would only spend the budget. This is how "names no longer referenced are dropped" is implemented.
- **Kept entries.** A kept entry carries only `{"id"}`. Sending `description` on kept attachments is not documented, and the component carries the visible alt anyway.
- **One budget site.** The budget check runs in body building, where kept vs new and the JSON length are known. That is also the last step before I/O, so there is no second check (AD-22).
- **Unknown ids degrade to re-upload, never to a broken keep-list.** The keep-list only ever names ids that came from Discord.
- **Golden multipart shape** (type 4):
  ```text
  --B\r\nContent-Disposition: form-data; name="payload_json"\r\nContent-Type: application/json\r\n\r\n{"type":4,"data":{"flags":32768,"components":[...],"allowed_mentions":{"parse":[]},"attachments":[{"id":0,"filename":"af1349b9f5f9a1a6.png"}]}}\r\n--B
  \r\nContent-Disposition: form-data; name="files[0]"; filename="af1349b9f5f9a1a6.png"\r\n\r\n<bytes>\r\n--B--
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass, including the new rules, ui, core wire, testkit and trybuild (`tests/ui/att/`) tests
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok (no new edges; no `noctis-ui` → `noctis-ports`)
- `cargo run --release -p xtask -- rules-catalog` -- ok, with MSG-08/ATT-04/ATT-05/ATT-06 pending entries removed
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- matches (install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if missing, per AGENTS.md)
- `cargo run --release -p xtask -- skill-check` -- ok (API hash updated)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `bash .github/scripts/check-build-config.sh` -- ok
- `test ! -d target/debug` -- no debug profile created

## Auto Run Result

- **Summary:** Content-addressed attachments (`<hex>.<ext>` names) with edit diffs: edits keep known Discord attachment ids (`{"id"}` only), upload only new bytes and drop unreferenced files; one Upload Budget check in body building enforces ATT-01 (per-file limit from the interaction's `attachment_size_limit`), ATT-02 (25 MiB total incl. payload_json) and ATT-03; rendered-image alt is validated under ATT-06; multipart via twilight `Form`; testkit fake parses multipart and echoes attachments.
- **Files changed:** `crates/noctis-core` (responder body/files plan, wire multipart, pipeline file limit, edge pool reuse of ready H1 senders), `crates/noctis-ui` (limits, attachment, message used-attachments), `crates/noctis-rules` (ATT-04/05/06/MSG-08 rules), `crates/noctis-testkit` (fake multipart recording, wire echo, outcome arms, `tests/attachments.rs`), `crates/noctis` (re-exports, trybuild `tests/ui/att/`), `docs/discord-rules.md`, `xtask/public-api.txt`, `xtask/rules-pending.toml`, project-template skill refs and `AGENTS.md`.
- **Review findings:** 35 total (high 0, medium 4, low 12, false 19). Patched: 4 medium + 1 low (pipeline `attachment_size_limit` wiring test; `message_transport_attachments`; Responder-level ATT-02/ATT-03 no-request tests; TestApp deferred-edit and follow-up file echo flow). Deferred: 2 low (stale "[planned, Story 9.4]" template tag, owned by Story 9.3; pre-existing maintainer-decision section 7/9 overlap). Rejected: 29, each with its reason in the Review Triage Log (kept-entry alt/description not resent, ATT-02 ignores multipart framing, concurrent-edit snapshot races, Form buffer copies, spec-mandated behaviors, and false findings).
- **Follow-up review recommended:** true (first pass, 4 medium patched; patched by verdict: high 0, medium 4, low 1). Named risk: the new pipeline/message-transport tests and the edge pool change (pool a ready H1 sender immediately) are unreviewed; the pool change touches connection reuse under load.
- **Verification:** all local gates green via `tools/story-loop/gates.sh` (fmt, clippy -D warnings, workspace tests, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** changed alt on unchanged bytes is not resent for kept entries; ATT-02 within a few hundred bytes of 25 MiB may pass locally yet exceed Discord's limit due to multipart framing; concurrent edits of the same message can use a stale keep-list (degrades to re-upload/drop).
