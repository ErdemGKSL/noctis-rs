---
title: 'Story 9.2: Raw escape hatch for components and REST'
type: 'feature'
created: '2026-10-22'
status: 'done'
baseline_revision: '54c77825885b361255b672a09a3ba778ef39579c'
review_loop_iteration: 0
followup_review_recommended: false
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
      RawHttp's path rule passes percent-encoded dot segments, slashes and backslashes (`%2e%2e`, `%2f`, `%5c`), so the `.`/`..`, empty-segment and `api`-prefix rules can be sidestepped.
    evidence: |-
      `check_path` in crates/noctis-core/src/raw_http.rs never decodes escapes (as the spec states). Whether this does harm depends on whether Discord's edge (Cloudflare URL normalization) or a configured http_proxy decodes `%2E`/`%2F` and removes dot segments before routing; if it does, a path segment interpolated from user input could reach another route with the bot token. Settle by sending `channels/1/%2e%2e/%2e%2e/applications/@me` against Discord and checking which route answers.
    location: >-
      crates/noctis-core/src/raw_http.rs:210 (check_path)
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** A bot author cannot place JSON for a component type Noctis does not model. A click on a message whose components twilight cannot parse is dropped at the gateway. A handler cannot see unknown incoming components. There is also no supported way to call an endpoint twilight does not model (FR-31, CV2-16; FR-52 for raw requests).

**Approach:**
- `noctis_ui::RawComponent` is a checked, verbatim JSON node. It may go at the top level or in a Container, and it counts toward CV2-03.
- Unknown incoming components are exposed as `ctx.unknown_components()`. The gateway retries a type-3 or type-5 interaction that twilight cannot parse, with its components emptied.
- An extension trait `noctis::http::RawHttp` adds `.raw(method, path)` to the App's one twilight client. The request then passes the edge, the client's rate limiter and any configured proxy.

## Boundaries & Constraints

**Always:**
- **`RawComponent` (noctis-ui, new `src/component/raw.rs`).** A `#[derive(Debug, Clone, PartialEq)]` struct holding a `serde_json::Value`.
  - `RawComponent::new(value: Value) -> Result<Self, UiError>`, plus `TryFrom<Value>` (same check).
  - `as_json(&self) -> &Value` and `into_json(self) -> Value`.
  - `component_type(&self) -> u64`, the root `type`.
  - `count(&self) -> usize`, the CV2-03 count.
  - There is no mutation API, so the check made at construction holds for the node's whole life.
- **The check.** A *component object* is a JSON object whose `type` is an integer ≥ 1 (`Value::as_u64`, so up to `u64::MAX` is accepted). The walk starts at the root and descends only through three keys:
  - `components`, which must be an array whose elements are all component objects;
  - `accessory`, which must be a component object;
  - `component`, which must be a component object.

  Every other key is ignored, for example `default_values`, `emoji`, `options` and `items`. Each node is visited once, in pre-order.

  Violations (every one is collected, in pre-order, then returned as `UiError::Limits`):
  - CV2-16 at the node path when the node is not a component object:
    - `Bound::Describe("a component object: a JSON object with an integer `type` of at least 1")`;
    - `Actual::Text(<"array"|"string"|"number"|"bool"|"null"|"object without a valid type">)`.
  - CV2-16 at `<path>.components` when `components` is present but not an array (same Bound, Actual names the JSON kind).
  - CV2-14 at `<path>.id` for an `id` key on a component object, whatever its value:
    - `Bound::Describe("no `id`: Noctis numbers components (CV2-14)")`;
    - `Actual::Text(<the value as compact JSON>)`;
    - help ``remove `id`; Discord numbers raw components itself``.
  - CV2-16 `Bound::AtMost(32)`, `Actual::Count(depth)` at the first node deeper than 32 levels (the root is level 1). Its subtree is not walked further.

  Paths:
  - the root is `""`;
  - a child is `components[i]`, `accessory` or `component`, joined with `.`, for example `components[0].accessory.id`.

  `count` is the number of component objects in the walk, including the root.
- **Placement.**
  - `RawComponent` implements the sealed `TopLevel` and `ContainerChild` traits, through a new `ChildNode::Raw` variant and `child_from!`.
  - It does **not** implement `SectionAccessory`, `RowItem`, `RowSelect`, `ModalTopLevel` or `ModalInput`. The rules descriptor `Accessory` has no opaque variant, and modal raw input is out of scope.
  - A `components!` `{expr}` hole takes a `RawComponent` wherever a `TopLevel`/`ContainerChild` hole is legal. It counts as 1 at expansion; the real count is checked at run time, as today.
- **Wire.** `Wire for RawComponent` serializes the stored `Value` unchanged and never consumes an `Ids` number. Noctis-numbered nodes before and after it stay consecutive: text, raw, text gives `id` 1 and 2 (CV2-14). `Serialize` goes through `serialize_via_wire!`.
- **Validation and cost.**
  - Lowering (`limits/lower.rs`) maps `ChildNode::Raw(r)` to `rc::Component::Opaque(r.count())`. CV2-03, `cost()`, `CostReport.components`, `Budget::components_left`/`fits`/`fits_prefix` and `child_cost` therefore count it.
  - Raw text is never counted toward CV2-04.
  - Lenient mode never changes a raw node.
  - The path kind of a raw node is `opaque`, the same as `rc::Component::kind`. Examples: `opaque[1]` at the top level, `container[0].opaque[2]`.
- **Raw attachment references.** `walk.rs` gains `Site::RawUrl(&str)`. Every JSON string value anywhere in a raw node (any key, any depth, arrays included) that starts with `attachment://` is one visit, with path `<raw path>.url`, in document order. `Site::url()` returns it; `image()` and `custom_id()` return `None`.
  - A URL naming a set member counts as used, so `V2Message::used_attachments` includes it and the Responder uploads or keeps it (Story 9.1).
  - A URL naming a non-member is the existing ATT-05 violation.
  - The rendered-alt rule and `MissingAlt` never apply to raw nodes.
- **Unknown incoming components.** New `noctis_ui::unknown_components(components: &[Value]) -> Vec<Value>` (public, re-exported as `noctis::ui::unknown_components`).
  - The message types Noctis models are 1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14 and 17.
  - The function walks `components` in pre-order. It descends into the `components` array of types 1, 9 and 17 and the `accessory` of type 9.
  - It returns a clone of every element that is not an object with a modeled integer `type`: unknown types, a missing or non-integer `type`, and non-objects. An unknown node is taken whole and its subtree is not walked.
  - A `components` or `accessory` of the wrong JSON shape inside a known node is skipped silently.
- **Context accessors.**
  - `ComponentCtx::unknown_components(&self) -> &[Value]` gives the result of `unknown_components` over the raw interaction's `message.components`. It is computed once in the component pipeline from the `RawJson` bytes and stored on `CtxInner` as a new field (default empty; no allocation when there is none).
  - `ModalSubmitCtx::unknown_components(&self) -> &[Value]` gives `ModalSubmission::unknown()` of the submit.
  - Detached contexts return `&[]`.
  - `ComponentCtx`, `ModalSubmitCtx` and `ExpiredCtx` gain `http(&self) -> &Arc<twilight_http::Client>`, added to `ctx_accessors!` with the same body as `CommandCtx::http`.
- **Gateway fallback (`noctis-gateway/src/shard.rs`).** `modal_submit_without_components` becomes `interaction_without_components`. When twilight fails to parse an `INTERACTION_CREATE` whose raw `type` is 3 or 5, it re-parses a copy:
  - with `message.components` set to `[]` when `message` is an object;
  - and, for type 5, also with `data.components` set to `[]`.

  The `RawJson` keeps the original bytes. Any other type, or a copy that still fails, keeps today's `warn` and is ignored. Noctis never reads twilight's `message.components`: the component pipeline reads only the raw bytes. Verify this with grep; there must be no use of `Interaction::message` components in `noctis-core`.
- **Edits never re-send unparsed components.** Every V2 update or edit body is built only from the handler's `V2Message`. Tests pin this for `ctx.update(view)`, a returned View under `response = update`, and `defer_update` + `edit_reply` on a source message holding unknown components.
- **Raw REST (noctis-core, new `src/raw_http.rs`; facade `pub mod http`).**
  - `pub trait RawHttp: sealed::Sealed { fn raw(&self, method: Method, path: impl Into<String>) -> RawRequest<'_>; }`, implemented only for `twilight_http::Client`. Through `Deref` it works on `ctx.http()` (an `&Arc<Client>`) and `app.http()`.
  - `Method` is `twilight_http::request::Method`, re-exported.
  - `RawRequest<'a>` is `#[must_use]`, with these methods:
    - `json<T: Serialize + ?Sized>(self, &T) -> Self`: the serialized bytes become the body, sent as `application/json` by twilight; a serialization error is kept and returned on await;
    - `body(self, Vec<u8>) -> Self`: JSON bytes, sent verbatim;
    - `form(self, twilight_http::request::Form) -> Self`: multipart.

    `json`, `body` and `form` each replace any body set before (the last call wins).
  - `without_token(self) -> Self` sets `use_authorization_token(false)`, for webhook and interaction-token routes.
  - `IntoFuture<Output = Result<RawResponse, RawHttpError>>` with `IntoFuture = Pin<Box<dyn Future<…> + Send + 'a>>`. Awaiting validates the path, then builds `RequestBuilder::raw(method, path)` and sends it with `Client::request::<serde_json::Value>`. The request therefore uses the client's proxy (the edge, which forwards to discord.com or the configured `http_proxy`), its rate limiter (off when proxied), its timeout and its token.
- **Path rule [N].** `path` is the part after `/api/v10/`, query included. Each of these fails with `RawHttpError::InvalidPath { path, reason }`, checked in this order, and no request is sent:
  1. empty;
  2. starts with `/`;
  3. contains a byte outside `0x21..=0x7E`, or `#`, or `\` (so space, control characters and any non-ASCII text are rejected; percent-encode them);
  4. the part before the first `?` has an empty segment (`a//b`, trailing `/`, or a leading `?`);
  5. a segment is `.` or `..`;
  6. the first segment is `api`, ASCII case-insensitive.

  There is no length cap. Percent escapes are not decoded or checked.
- **`RawResponse`.**
  - `status() -> u16`;
  - `header(&self, name: &str) -> Option<&str>`: ASCII case-insensitive, first value; a value that is not UTF-8 gives `None`;
  - `bytes() -> &[u8]` and `into_bytes() -> bytes::Bytes`;
  - `json<T: DeserializeOwned>(&self) -> Result<T, RawHttpError>`, which fails with `Decode` on an empty or invalid body.

  Only 2xx responses become a `RawResponse`.
- **`RawHttpError`** (`thiserror`, `#[non_exhaustive]`, every message starting with `raw request:`):
  - `InvalidPath { path: String, reason: &'static str }`;
  - `Encode(serde_json::Error)`;
  - `Status { status: u16, body: Bytes }`, mapped from twilight `ErrorType::Response`;
  - `Body(twilight_http::response::DeserializeBodyError)`;
  - `Decode(serde_json::Error)`;
  - `Http(twilight_http::Error)` for everything else (timeout, transport, `Unauthorized`).

  It converts into `HandlerError` through the existing blanket `From`.
- **Facade.**
  - `noctis::http::{RawHttp, RawRequest, RawResponse, RawHttpError, Method}`;
  - `noctis::ui::{RawComponent, unknown_components}`;
  - the prelude adds `RawHttp`;
  - `noctis-core` re-exports the raw HTTP types at its root.
- Every public API change updates, in the same change:
  - the Usage Skill;
  - the template `AGENTS.md`;
  - `xtask/public-api.txt` (re-blessed);
  - the skill's `API hash`;
  - `xtask/src/skill_check/components.rs` `BUILDERS`, which gains `"RawComponent"` and needs an entry with `**Where:**`/`**Limits:**`/`**Builder:**`.

**Never:**
- Raw nodes in Section accessories, Action Rows, legacy rows or modals.
- Rewriting raw JSON: no id injection, no truncation, no key changes.
- Checking `custom_id`s inside raw JSON. CID-02 and the FR-25 nonces ignore them; this is a documented caveat.
- A raw request that bypasses `Client::request`, a second HTTP client, or a new crate dependency (everything needed is already in the workspace).
- Exposing raw JSON of gateway message events (`MESSAGE_CREATE`/`UPDATE`): this story covers interactions only.
- Changes to the template's `src/`: the template needs no raw hatch.
- Webhook work (9.5) or defaults/overrides work (9.3).
- Editing `_bmad/` or weakening `.cargo/config.toml`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error |
|---|---|---|---|---|
| R1 | Minimal | `{"type":99}` | Ok, `count()=1`, `component_type()=99` | — |
| R2 | Nested | `{"type":99,"components":[{"type":10,"content":"a"},{"type":98}]}` | count 3 | — |
| R3 | Accessory + component keys | `{"type":9,"components":[{"type":10,"content":"a"}],"accessory":{"type":97}}`; `{"type":18,"component":{"type":4}}` | count 3; count 2 | — |
| R4 | Empty components | `{"type":99,"components":[]}` | count 1 | — |
| R5 | Root not object | `[]`, `"x"`, `1`, `true`, `null` | CV2-16, path `""`, Actual `array`/`string`/`number`/`bool`/`null` | `UiError::Limits` |
| R6 | Bad type | missing, `0`, `-1`, `1.5`, `"9"`, `null` | CV2-16 path `""`, Actual `object without a valid type` | `UiError::Limits` |
| R7 | Type max | `{"type":18446744073709551615}` | Ok | — |
| R8 | `components` not array | `{"type":99,"components":{}}` | CV2-16 at `components` | `UiError::Limits` |
| R9 | Bad child | `components:[1]`; `accessory:null`; `component:"x"` | CV2-16 at `components[0]` / `accessory` / `component` | `UiError::Limits` |
| R10 | `id` key | root `"id":5`, `"id":0`, `"id":null`; child `components[1].id` | CV2-14 at `id` / `components[1].id`, help names removal | `UiError::Limits` |
| R11 | Non-component `id`s | `default_values:[{"id":"1","type":"user"}]`, `emoji:{"id":"2"}` | Ok (not walked) | — |
| R12 | Depth | 32 nested levels; 33 | Ok; CV2-16 `AtMost(32)` `Count(33)` at the 33rd node's path | `UiError::Limits` |
| R13 | All violations | root `id` + `components:[1,{"type":0}]` | 3 violations in pre-order: CV2-14 `id`, CV2-16 `components[0]`, CV2-16 `components[1]` | `UiError::Limits` |
| R14 | Verbatim | unicode `"content":"😀ü"`, unknown keys `{"foo":{"bar":[1,2]}}` | `serde_json::to_value(&raw) == input` | — |
| P1 | Top level | text, raw, text in `Components` | `[{"type":10,"id":1,..},<raw as is>,{"type":10,"id":2,..}]` | — |
| P2 | In Container | Container[text, raw, text] | container id 1, texts 2 and 3, raw without id | — |
| P3 | Illegal place | `Section::new([t], raw)`, `ActionRow::buttons([raw])`, `ModalLabel::new(.., raw)` | compile error E0277 (`compile_fail` doctests, line `// [CV2-16]`) | compile error |
| P4 | Macro hole | `components![{raw}]`, `container [ {raw} ]` | compiles; runtime count used | — |
| V1 | CV2-03 boundary | 38 texts + raw(count 2) = 40; 39 texts + raw(count 2) = 41 | Ok; CV2-03 at `components`, actual 41 (Strict and Lenient) | `UiError::Limits` |
| V2 | cost/Budget | raw count 3 in Container | `cost().components` includes 3; `components_left` = 40 − total | — |
| V3 | Text not counted | raw with 5000-char `content` | no CV2-04, `text_chars` excludes it | — |
| V4 | Lenient | Lenient + over-long texts + raw | texts truncated; raw JSON unchanged | — |
| V5 | Raw attachment ref | raw `{"type":99,"media":{"url":"attachment://<a>"}}`, set has `a` | `used_attachments` yields `a`; Responder uploads `files[0]` | — |
| V6 | Raw ref non-member | `attachment://zz.png` in raw at top-level index 1 | ATT-05 at `opaque[1].url` | `UiError::Limits` |
| U1 | Unknown in click | source `message.components` = text, `{"type":99,"x":1}`, Container[`{"type":98}`], Section[accessory `{"type":97}`] | `ctx.unknown_components()` = [99, 98, 97 objects] in order, verbatim | — |
| U2 | Unknown container | `{"type":99,"components":[{"type":96}]}` | one entry (whole 99 object) | — |
| U3 | Odd elements | `1`, `{}`, `{"type":"x"}` | each collected | — |
| U4 | None | only known types / no `message` / detached | `&[]` | — |
| U5 | Legacy rows | no V2 flag, row holding `{"type":95}` | `[{"type":95}]` | — |
| U6 | Modal submit | top-level `{"type":99,"custom_id":"x"}` | `ModalSubmitCtx::unknown_components()` == `submission.unknown()` | — |
| G1 | twilight fails, type 3 | message component `{"type":300}` or `{"type":99,"content":5}` | Inbound Interaction, raw bytes unchanged; handler runs; U1 sees the node | — |
| G2 | type 5 from message | bad message component + Checkbox in data | both emptied in the copy; raw unchanged | — |
| G3 | twilight parses | `{"type":99}` | normal parse, no fallback | — |
| G4 | No fallback | type 2 unparsable; type 3 whose `data` is broken | `warn`, ignored (as today) | — |
| E1 | Update | click on G1/U1 message; `ctx.update(view)` / returned View | type-7 `data.components` equals the View's JSON exactly; no 99/98/97/300 | — |
| E2 | Deferred edit | `defer_update` then `edit_reply(view)` | PATCH `@original` components = View only | — |
| H1 | GET | `app.http().raw(Method::Get, "applications/@me").await` | fake: GET `/api/v10/applications/@me`, `authorization: Bot <t>`; `status()=200`, `bytes()` = fake body, `json::<Value>()` ok | — |
| H2 | JSON body | POST `.json(&json!({"content":"hi"}))` | body = `serde_json::to_vec`, `content-type: application/json` | — |
| H3 | Raw body / form | `.body(b"{}".to_vec())`; `.form(Form)` | body verbatim; multipart, `files()` lists the part | — |
| H4 | Last body wins | `.json(a).form(f)`; `.form(f).json(a)` | multipart; JSON | — |
| H5 | No token | `.without_token()` | no `authorization` header | — |
| H6 | Query, percent | `channels/1/messages?limit=2`; PUT `…/reactions/%F0%9F%91%8D/@me` | path + query forwarded unchanged | — |
| H7 | 204 | fake 204 | Ok, `status()=204`, empty bytes; `json::<Value>()` → `Decode` | `Decode` on json |
| H8 | Error status | 404 `{"code":10003}`; 500 | `Status{status:404, body}` / `Status{500,..}` | typed error |
| H9 | Invalid paths | `""`, `/channels/1`, `api/v10/x`, `API/x`, `a//b`, `a/`, `?x=1`, `a/./b`, `a/../b`, `a b`, `a/ü`, `a#f`, `a\b`, `a\n` | `InvalidPath` with the reason of rules 1–6; fake log empty | `InvalidPath` |
| H10 | Valid edges | `@me`, `a`, `a?`, `a?b/c//d` | accepted | — |
| H11 | Encode fail | `.json(&HashMap<(u8,u8),u8>)` with one entry | `Encode`, no request | `Encode` |
| H12 | Proxied: no local limiter | `http_proxy(fake)`; fake answers `remaining 0, reset-after 60` | second raw GET returns within 1 s; 2 requests logged | — |
| H13 | Client limiter honored | `twilight_http::Client::builder().proxy(fake authority, true).token(..)` (limiter on); `reset-after 0.5` | second raw GET completes ≥ 400 ms after the first | — |
| H14 | One client from ctx | TestApp command and component handlers call `ctx.http().raw(..)` | requests land in the TestApp fake log, next to the callbacks | — |
| H15 | Timeout | `http_timeout(200ms)`, fake delayed 2 s | `Http` error, kind timed out | `Http` |
| H16 | Header lookup | response header `X-RateLimit-Bucket: b` | `header("x-ratelimit-bucket")=Some("b")`; missing → `None` | — |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/component/layout.rs` -- `ChildNode` (add `Raw`), `ContainerComponent` `Wire` match, `child_from!`, `serialize_via_wire!`.
- `crates/noctis-ui/src/component/mod.rs` -- module list and re-exports (add `raw`, `pub use raw::RawComponent`).
- `crates/noctis-ui/src/placement.rs` -- `sealed!`, `top_level!`, `container_child!` (add `RawComponent`); the compile_fail doctests for P3 go on `RawComponent`.
- `crates/noctis-ui/src/wire.rs` -- `Wire`/`Ids`; raw ignores `ids`.
- `crates/noctis-ui/src/limits/lower.rs` -- `lower_child` (≈L160–200) maps Raw to `rc::Component::Opaque(count)`; `child_nodes` passes it through.
- `crates/noctis-ui/src/limits/walk.rs` -- `Site`, `child_kind` (`Raw => "opaque"`), `child_sites` (raw URL visits).
- `crates/noctis-ui/src/limits/{mod.rs,lenient.rs,budget.rs}` -- every `ChildNode` match: `check_attachment_refs` L525, `referenced_names` (used by `used_attachments`, `message/mod.rs` L130), Lenient walk skips raw.
- `crates/noctis-ui/src/modal/submit.rs` L166–310 -- `ModalSubmission::unknown()` already keeps unknown submit components.
- `crates/noctis-ui/src/lib.rs` -- docs and re-exports (`RawComponent`, `unknown_components`).
- `crates/noctis-rules/src/component.rs` L117 (`Opaque(usize)` counts `max(n,1)`), L700 (Opaque legal anywhere), test `cv2_03_cv2_16_nested_and_opaque` in `component/tests.rs` L134. Read-only.
- `crates/noctis-rules/src/rule.rs` L62 -- `RuleId::CV2_14`/`CV2_16` exist.
- `crates/noctis-gateway/src/shard.rs` L45–145 -- `classify`, the parse fallback `modal_submit_without_components` (generalize); unit tests near L312–370.
- `crates/noctis-core/src/component.rs` L157–260 `ctx_accessors!` (add `http`), L282–330 `ComponentCtx`/`ModalSubmitCtx` (add `unknown_components`).
- `crates/noctis-core/src/command.rs` L100–129 `CtxInner` (add the unknown list; default empty in `detached_parts` and every literal), L290–297 `CommandCtx::http` (body to copy), L317 `detached_http`.
- `crates/noctis-core/src/pipeline/component.rs` L520–640 -- `run_component_over` has `raw` bytes and builds `CtxInner` for both families; L496 `ModalSubmission::from_slice(raw)`.
- `crates/noctis-core/src/responder.rs` L1158–1224 -- `SourceMessage::from_interaction` (raw message read; may share the parse); update/edit bodies come only from the `V2Message` (E1/E2 evidence).
- `crates/noctis-core/src/pipeline/tests/components.rs` -- pipeline tests over a recording upstream with raw interaction JSON (add U1–U6, E1, E2).
- `crates/noctis-core/src/app.rs` L2248 `http_client` (edge proxy, limiter off when proxied); `crates/noctis-core/src/lib.rs` L167–280 (modules and re-exports); `handler.rs` L182 blanket `From<E> for HandlerError`.
- `~/.cargo/registry/src/*/twilight-http-0.17.1/src/request/base.rs` L59 `RequestBuilder::raw`, L85 `body`, L95 `form`, L113 `json`, L128 `use_authorization_token`; `client/mod.rs` L2889 `Client::request` (proxy, token, `Endpoint` → ratelimiter); `error.rs` L110 `ErrorType::Response{body,error,status}`; `response/mod.rs` L190 `headers`, L196 `status`, L234 `bytes`; `request/mod.rs` L76 `Method`. Read-only.
- `crates/noctis-testkit/tests/http_fake.rs` L25 `app(fake, ..)`, L540 `proxied_app_has_no_local_ratelimiter` (model for H12); `crates/noctis-testkit/src/fake.rs` L200–230 `FakeResponse::{json,empty,header}`, L268 `RecordedRequest`, L363 `json()`, L395 `files()`.
- `crates/noctis/src/lib.rs` L564 core re-exports, L1582 `pub mod twilight`, L1825 prelude, L1909 `pub mod ui` (L2489 list).
- `xtask/src/skill_check/components.rs` L22 `BUILDERS`.
- Skill: `examples/project-template/.claude/skills/noctis/references/components-v2.md` §11 L496 (rewrite, drop `[planned]`, `ctx.raw_message_components()` → `ctx.unknown_components()`), `references/app-and-sharding.md` L539–541 (replace `ctx.http_raw().request(..)` with `ctx.http().raw(..)`), `references/commands.md` L856/L870–875 (ctx accessor tables: `http()` on component/modal/expired ctx, `unknown_components()`), `references/modals.md` L261–266, `references/limits-cheatsheet.md` L266 (CV2-16), `SKILL.md` L23 (API hash); `examples/project-template/AGENTS.md` L62 (HTTP bullet).
- `docs/discord-rules.md` L191 (CV2-03), L203 (CV2-14), L204 (CV2-16).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ui/src/component/raw.rs` (new), `component/mod.rs`, `layout.rs`, `placement.rs`, `lib.rs` -- `RawComponent` with check, `Wire`, placement, rustdoc with runnable examples and the P3 `compile_fail` doctests; add `pub fn unknown_components`. Unit tests R1–R14, P1, P2, U1–U5 named `cv2_16_*` / `cv2_14_raw_*` (for example `cv2_16_raw_root_not_object`, `cv2_14_raw_id_rejected`, `cv2_16_unknown_components_pre_order`).
- `crates/noctis-ui/src/limits/{lower.rs,walk.rs,mod.rs,lenient.rs,budget.rs}` -- Opaque lowering, `opaque` path kind, raw URL sites; tests V1–V6 in `crates/noctis-ui/tests/validate.rs` (`cv2_03_raw_*`, `att_05_raw_*`) and a `components!` hole test for P4 in `crates/noctis/tests/components_macro.rs`.
- `crates/noctis-gateway/src/shard.rs` -- `interaction_without_components`; tests G1–G4 (`cv2_16_click_unparsable_message_still_dispatches`, …).
- `crates/noctis-core/src/command.rs`, `component.rs`, `pipeline/component.rs` -- the unknown list on `CtxInner`, `unknown_components()` on both contexts, `http()` in `ctx_accessors!`; pipeline tests U1, U4, U6, E1, E2 in `pipeline/tests/components.rs` (`cv2_16_*`, `fr_31_*`).
- `crates/noctis-core/src/raw_http.rs` (new), `lib.rs` -- `RawHttp`, `RawRequest`, `RawResponse`, `RawHttpError`, path check. Unit tests H9/H10 on the path check, without the network.
- `crates/noctis-testkit/tests/raw_http.rs` (new) -- H1–H8, H11–H16 over `FakeDiscordHttp` (H14 through `TestApp` handlers); every invalid-path and encode case asserts that the fake log is empty.
- `crates/noctis/src/lib.rs` -- `pub mod http`, `ui` re-exports, prelude `RawHttp`, crate docs line next to `App::http`.
- `xtask/src/skill_check/components.rs` -- add `"RawComponent"` to `BUILDERS`.
- `docs/discord-rules.md` -- the same rule ids, tiers that still parse:
  - CV2-03 How: raw nodes count `RawComponent::count()`;
  - CV2-14: Compile-time (by construction) / Runtime (raw JSON). `RawComponent::new` rejects `id`, and Noctis never numbers raw nodes;
  - CV2-16 How: `RawComponent` (checked, verbatim, top level/Container only), `unknown_components`/`ctx.unknown_components()`, the gateway fallback, edits from the View only, with the test names.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` -- one "Amended (Story 9.2)" bullet under AD-12 (RawComponent placement and counting, the unknown list, the twilight parse fallback for types 3/5) and one under AD-24 (raw REST is the `RawHttp` extension over the one client: no second client).
- Skill and template files from the Code Map:
  - in `components-v2.md` §11, a `### 11a. Raw component: `RawComponent`` entry with **Where:** (top level, Container), **Limits:** (CV2-03 count, CV2-14 no `id`, CV2-16, depth 32), **Builder:** (`RawComponent::new(json!(..))?`), stability caveats (no placement or field checks, `custom_id`s not checked, Discord may change or reject unknown types, `attachment://` URLs are tracked) and the receive side;
  - a raw REST subsection (example `ctx.http().raw(Method::Post, format!("channels/{id}/messages")).json(&body).await?`, path rule, `without_token`, errors, the caveat that the Responder never sees raw interaction-token requests, rate limited / proxied);
  - template `AGENTS.md` HTTP bullet mentions `ctx.http().raw(..)` as the last resort;
  - re-bless `xtask/public-api.txt` and update the `API hash`.

**Acceptance Criteria:**
- Given a View with 38 Text Displays and a `RawComponent` of count 2, when it is sent with `ctx.respond(view)`, then one type-4 callback is sent and its raw node equals the input JSON byte for byte, with no `id`; with 39 texts the call returns `ResponderError::Invalid(UiError::Limits{..})` naming CV2-03 with actual 41, and no request is sent.
- Given a click on a message whose `components` hold a type-300 node that twilight cannot parse, when it arrives through the gateway, then the Route's handler runs and `ctx.unknown_components()` returns that node verbatim.
- Given that handler, when it calls `ctx.update(view)`, then the type-7 body's `components` are exactly the View's rendered JSON and contain no unknown node.
- Given a modal submit with an unknown top-level component, when its handler runs, then `ctx.unknown_components()` returns it.
- Given `ctx.http().raw(Method::Get, "applications/@me")` in a TestApp handler, when it is awaited, then the request reaches the App's configured proxy (the fake) at `/api/v10/applications/@me` with the bot token, and a client with a local limiter delays a request the previous response's bucket exhausted.
- Given an invalid raw path, when the request is awaited, then `RawHttpError::InvalidPath` is returned and no request is sent.
- Given the finished story, when `xtask rules-catalog`, `api-snapshot` and `skill-check` run, then all pass, with `RawComponent` holding a component entry and the API hash matching the re-blessed snapshot.

## Spec Change Log

## Review Triage Log

### 2026-10-22 — Review pass
- verdicts: 24 findings — high 0, medium 2, low 10, false 10, maybe-false 2
- findings:
  - `[medium]` `[patch]` (blind) `RawHttpError::InvalidPath` Display embeds the whole rejected path, leaking webhook/interaction tokens into logs — verified: the default error sink logs `%Chain(error)` (sink.rs L268), so a malformed `webhooks/1/<token>/` path prints the token, contradicting the redacting `RawRequest` Debug; patch: drop the path from the `#[error]` text.
  - `[maybe-false]` `[defer]` (blind) percent escapes (`%2e%2e`, `%2f`, `%5c`) bypass the dot-segment/empty-segment/api rules — the spec deliberately leaves escapes undecoded; harm depends on edge/proxy normalization; settle by probing Discord with an encoded `..` path; deferred as medium (unverified).
  - `[low]` `[reject]` (blind) `RawHttpError::Status` drops response headers, so a 429's `retry-after` is unreadable — real but rare (twilight's limiter or the proxy absorbs 429s) and the fix adds public surface to a shape the spec fixed.
  - `[low]` `[reject]` (blind) no way to set request headers (`X-Audit-Log-Reason`), `.body` always JSON — feature gap, not a defect in everyday raw use; fix adds public surface.
  - `[false]` `[reject]` (blind) `RawComponent` accepts modeled `type`s, bypassing CV2-05/CV2-07 — the spec deliberately accepts any `type` ≥ 1 (matrix R3 uses type-9 and type-18 roots) and the skill documents "no placement rules and no field checks inside a raw node"; Discord's 400 is the documented outcome.
  - `[low]` `[reject]` (blind) type-2 message context menu on a message twilight cannot parse, and MESSAGE_CREATE/UPDATE, are not covered by the fallback — twilight parses unknown u8 types as `Component::Unknown`, so only type > 255 or malformed known fields fail; rare, spec G4 and Never keep it out, spine keeps `RawJson` only for interactions; fix adds a new branch.
  - `[low]` `[reject]` (blind) `source_unknowns` parses message components into `Value` on every click — real but small (deserializes only `message.components`, at most 40 nodes); spec mandates eager computation; a lazy `OnceLock` adds complexity.
  - `[low]` `[reject]` (blind) `MODELED` type list is hand-maintained and repeated in a test — drift only if a future story forgets it; the fix is a new guard test, not a direct correction.
  - `[low]` `[patch]` (blind) `cv2_03_raw_view_through_respond` checks "no request carries the message" only inside `if let Ok(over)` — verified at crates/noctis-testkit/tests/raw_http.rs L571; patch: assert over `app.http_fake().requests()` unconditionally.
  - `[low]` `[reject]` (blind) placement rejections are `compile_fail` doctests only, labelled `[CV2-16]`, diagnostic text unpinned — labels follow spec P3; the unpinned message is a regression risk only; adding trybuild cases is more than a direct correction.
  - `[false]` `[reject]` (blind) raw type-10 `content` not counted toward CV2-04 — the spec deliberately excludes raw text (V3) and the skill caveat documents it.
  - `[false]` `[reject]` (blind) every raw `attachment://` violation reports at `opaque[i].url` — the spec defines exactly `<raw path>.url`; the name in the violation tells them apart.
  - `[false]` `[reject]` (blind) `unreachable!` in `from_twilight` is a panic site — the arm is unreachable: `error.kind()` is checked to be `Response` immediately before `into_parts()`.
  - `[maybe-false]` `[defer]` (edge) percent-encoded dot segments/slashes bypass the path rule — same root cause as the blind percent-escape finding; deferred with it.
  - `[false]` `[reject]` (edge) modeled-type raw roots skip typed checks — same refutation as the blind modeled-type finding (deliberate, documented caveat).
  - `[low]` `[patch]` (edge) the Err path of `rawview over` leaves "no request sent" unchecked — same defect as the blind test-conditional finding; patched with it.
  - `[low]` `[patch]` (vgap) `RawRequest`'s token-hiding `Debug` is not pinned by any test — pre-verified gap; patch: add `raw_request_debug_hides_the_path` in `raw_http.rs` tests.
  - `[medium]` `[patch]` (vgap other) `InvalidPath` Display/Debug includes the full path with its token — same defect as the blind InvalidPath finding; patched with it.
  - `[low]` `[reject]` (intent) "received message" narrowed to click source messages (no MESSAGE_CREATE, no message context menu) — same reasoning as the blind type-2 finding: spine keeps `RawJson` only for interactions, twilight parses unknown u8 types, the remaining case is rare.
  - `[false]` `[reject]` (intent) raw nodes limited to top level and Containers — the epic says "inside a View"; the rules `Accessory` descriptor has no opaque variant and no current Discord row/accessory type is unmodeled; the spec settled this deliberately (Design Notes).
  - `[false]` `[reject]` (intent) the App client's own limiter is never exercised by a raw request — raw requests use the same `Client::request` as every App call, and H13 proves the limiter applies to that call.
  - `[false]` `[reject]` (intent) edit-path coverage stops at component flows — `SourceMessage` stores no components (responder.rs L1161), so no edit path can re-send them; other edits build only from the given `V2Message`.
  - `[false]` `[reject]` (intent) template source code unchanged — the template needs no raw hatch (spec Never); its `AGENTS.md` and skill are updated, as maintainer decision 5 requires.
  - `[false]` `[reject]` (intent) raw-node count deferred to run time in `components!` holes — existing, documented hole semantics (CV2-03 row); a raw node is dynamic data.

## Design Notes

- **Placement.** The rules descriptor `Accessory` has only Button/Thumbnail, while `Component::Opaque` is already legal at the top level and in Containers. Allowing raw there needs no `noctis-rules` API change. An accessory or row raw node can follow when Discord ships one.
- **No `id` instead of renumbering.** Verbatim output and Noctis-assigned ids (CV2-14) can only both hold if raw nodes carry no `id`. Discord numbers components that have none, so rejecting `id` keeps every id unique without rewriting JSON.
- **Raw `attachment://` tracking.** Raw nodes are scanned for `attachment://` URLs because a raw media node would otherwise reference a file the Responder never uploads, which ends in a Discord 400 instead of a typed error.
- **Raw REST as an extension trait.** Making `ctx.http()` return a wrapper would break every documented `ctx.http().create_message(..)` call. The extension trait keeps the story's `ctx.http().raw(..)` spelling and also covers `app.http()`, `EventCtx` and the Middleware contexts.
- **Path rule.** It is Noctis's own rule [N], with no Discord rule id. It keeps a raw request inside `/api/v10/` on the edge: no scheme, fragment, dot segment or double prefix.
- Example:
  ```rust
  let poll = RawComponent::new(json!({"type": 99, "question": "Lunch?"}))?;
  let view = V2Message::new(Components::new().push(TextDisplay::new("Vote")).push(poll));
  let r = ctx.http().raw(Method::Get, format!("channels/{}/pins", ch)).await?;
  let pins: serde_json::Value = r.json()?;
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass, including the new ui, gateway, core pipeline, testkit `raw_http` and doctest (`compile_fail`) tests
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok (no new edges or crates)
- `cargo run --release -p xtask -- rules-catalog` -- ok (CV2-14 Runtime and CV2-16 have unit tests citing `cv2_14`/`cv2_16`)
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- matches (install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if missing, per AGENTS.md)
- `cargo run --release -p xtask -- skill-check` -- ok (`RawComponent` entry, API hash)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `test ! -d target/debug` -- no debug profile created

## Auto Run Result

- **Summary:** Story 9.2 adds a raw escape hatch: `RawComponent` (verbatim JSON node, `type` >= 1, no `id`, allowed at top level and inside Containers, scanned for `attachment://` refs) and `ctx.http().raw(method, path)` via an extension trait on the twilight client (path rule [N] keeps requests inside `/api/v10/`, token-redacting `RawRequest` Debug, typed `RawHttpError`); the gateway falls back to raw JSON for interactions twilight cannot parse.
- **Files changed:** `crates/noctis-ui/src/component/raw.rs` (RawComponent), `crates/noctis-core/src/raw_http.rs` (raw REST), `crates/noctis-gateway/src/shard.rs` (raw interaction fallback), `crates/noctis-ui/src/{placement,limits/*}.rs` (opaque placement/limit walk), `crates/noctis-core/src/pipeline/component.rs` (source unknowns), `crates/noctis/src/lib.rs` (re-exports), tests in `noctis-testkit/tests/raw_http.rs`, `noctis-ui/tests/validate.rs`, pipeline tests, CV2-05 trybuild stderr; `docs/discord-rules.md` (CV2-14/CV2-16), ARCHITECTURE-SPINE, template `AGENTS.md` + noctis skill references, `xtask/public-api.txt`, xtask api/skill checks.
- **Review findings:** 24 total (high 0, medium 2, low 10, false 10, maybe-false 2). Patched: 1 medium (`InvalidPath` Display leaked tokenized path; path dropped from message), 2 low (unconditional "no request sent" assertion in `cv2_03_raw_view_through_respond`; new `raw_request_debug_hides_the_path` test). Deferred: percent-encoded dot segments/slashes bypassing the path rule (maybe-false, needs Discord probe). Rejected: 429 headers dropped, no custom headers/non-JSON body, modeled types accepted raw, message-context/MESSAGE_CREATE fallback not covered, eager `source_unknowns`, hand-maintained MODELED list, compile_fail-only placement diagnostics, raw text not counted in CV2-04, opaque url path naming, `unreachable!` in `from_twilight`, top-level/Container-only placement, limiter coverage, edit-path coverage, template source unchanged, hole raw counts at runtime — reasons in the triage log.
- **Follow-up review recommended:** false (patched by verdict: high 0, medium 1, low 2).
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy, test, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91); CI paused by maintainer.
- **Residual risks:** percent-encoded path escapes depend on Discord edge normalization (deferred); raw nodes bypass typed CV2 checks by design, so Discord 400s are the failure mode; 429 `retry-after` not exposed on `RawHttpError::Status`.
