---
title: 'Story 4.2: V2 messages, legacy messages, flags and mentions'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: '460541b1e718b408062954f69a285a5035119adc'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/components-v2.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Bodies Discord rejects as empty (a V2Message with zero components, a LegacyMessage with empty content and no rows) serialize without any rule id or check.
    evidence: |-
      Discord answers both with a 400 (empty message / V2 needs at least one component). docs/discord-rules.md has no rule for an empty message, so neither the 4.3 runtime validator nor any later story is set to catch it; the 4.2 Matrix deliberately accepts both ("not a catalog rule") and the tests v2_empty_body and legacy_content_verbatim lock that in. Fix needs a new rule id in docs/discord-rules.md and noctis-rules plus runtime enforcement (Story 4.3 scope).
    location: >-
      crates/noctis-ui/src/message/mod.rs (V2Message::new, LegacyMessage::new); docs/discord-rules.md section 10/11
    severity: medium
  - summary: >-
      Template modules use V2Message unqualified, but noctis::prelude does not re-export it and no module imports it from noctis::ui.
    evidence: |-
      examples/project-template/src/modules/{setup,profile,dashboard}.rs already wrote UiResult<V2Message> with only noctis::prelude::* before this story; 4.2 made V2Message real at noctis::ui::V2Message, and the prelude (crates/noctis/src/lib.rs pub mod prelude) holds only Route/Payload/PayloadEnum/RouteComponents. The template does not compile until Story 14.2; decide when Views land (View, ViewCx, UiResult) whether the prelude re-exports V2Message or each module imports it, and align the skill's recipes (SKILL.md 3.1 uses V2Message with the prelude alone).
    location: >-
      examples/project-template/src/modules/setup.rs:12
    severity: low
---

<intent-contract>

## Intent

**Problem:** Story 4.1 gave `noctis-ui` a typed component tree (`Components`), but no message type around it. Nothing yet sets `IS_COMPONENTS_V2`, stops `content`/`embeds` on a V2 message, clears legacy fields when an edit switches a message to V2, offers per-endpoint flags, or defaults mentions to "ping nobody". So sends and edits can still fail with a 400 (FR-23, FR-25 mention default, NFR-6, UX-DR1; CV2-01, CV2-02, CV2-15, MSG-03, MSG-05, MSG-07, INT-09).

**Approach:** Add a `message` module to `noctis-ui` with these items:
- `V2Message` (components + attachments + mentions; it has no legacy fields).
- `LegacyMessage` (plain `content` + at most 5 Action Rows, the only legacy path).
- Typed flag sets per endpoint: `CreateFlags`, `EditFlags`, `CallbackFlags`. Associated consts exist only for the flags each endpoint allows.
- Owned `Mentions` / `Users` / `Roles`, defaulting to `parse: []`.
- `MessageFormat` and an opaque `MessageBody` that core serializes.

Then re-export everything through `noctis::ui`, add trybuild/doctest evidence and update the docs, the rules catalog, the skill and the template.

## Boundaries & Constraints

**Always:**
- Layering is unchanged. `noctis-ui` deps stay as in 4.1 (`arrayvec`, `blake3`, `bytes`, `noctis-codec`, `serde`, `thiserror`, `twilight-model`). No new crate is needed and there is no `noctis-rules` dep in this story.
- Flag values: SUPPRESS_EMBEDS `1<<2` (4), EPHEMERAL `1<<6` (64), SUPPRESS_NOTIFICATIONS `1<<12` (4096), IS_COMPONENTS_V2 `1<<15` (32768).
- Flag set API. Each flag set is a `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]` newtype over a private `u64` with:
  - `const fn empty()`, `const fn union(self, Self)`, `BitOr`, `BitOrAssign`, `const fn contains(self, Self) -> bool`, `const fn is_empty`, `const fn bits(self) -> u64`. `bits()` returns the author's flags only, never 32768.
  - Associated consts, and nothing else: `CreateFlags::{SUPPRESS_EMBEDS, SUPPRESS_NOTIFICATIONS}`, `EditFlags::{SUPPRESS_EMBEDS}`, `CallbackFlags::{SUPPRESS_EMBEDS, EPHEMERAL, SUPPRESS_NOTIFICATIONS}`.
  - No `from_bits`, `all`, `remove`, `IS_COMPONENTS_V2` or `IS_VOICE_MESSAGE`.
  - Each type's rustdoc names its endpoints: Create = Create Message and Execute Webhook. Edit = Edit Message, Edit Original Interaction Response, Edit Webhook Message and callback type 7 (UPDATE_MESSAGE) data. Callback = callback type 4 data and follow-up messages.
- Every body always contains `"allowed_mentions"`. V2 bodies always have `flags = 32768 | author bits`. Legacy bodies never set bit 15.
- Bodies borrow the message (`MessageBody<'a>`, opaque, `#[must_use]`, `Debug + Clone`, `Serialize`). They never emit `attachments`, `files`, `content` (V2, except the null clear), `embeds` (except the `[]` clear), `poll`, `sticker_ids` (except the `[]` clear) or `shared_client_theme`.
- An unresolved `custom_id` anywhere in a body is a serde error, inherited from 4.1. V2 components get pre-order numeric ids from `Components`. Legacy rows serialize without `id`s.
- Follow the 4.1 conventions: text is `impl Into<Cow<'static, str>>`, setters are by-value and chainable, getters have distinct names (as in `Attachment::alt_text`), and every public type is `Debug + Clone + Send + Sync` and documented, with doctests.
- Every public API change updates `examples/project-template/` (code, the noctis skill) and `xtask/public-api.txt` plus the skill's API hash in the same commit.
- All cargo commands use `--release`. Never edit `.cargo/config.toml` or `_bmad/`.

**Never:**
- No runtime validator, `cost` or truncation (4.3). No `label!`/`content!`/`placeholder!`, `try_*` constructors or `components!` (4.4). No modals (4.5).
- No Responder, HTTP, multipart, attachment diff or `with_components` query (Epics 6/7/9). No `escape_markdown`/`escape_mentions` (8.x). No embeds, polls or stickers API.
- No `Default` for `V2Message`/`LegacyMessage`.
- No fixture, doctest tag or test fn name may cite an id that is still pending after this story (e.g. `WH-03`, `INT-08`, `INT-10`, `MSG-01`, `MSG-08`, `CV2-03`, `ATT-05`). Citable ids: CV2-01, CV2-02, CV2-15, MSG-03, MSG-05, MSG-07, INT-09, plus ids already covered.

## I/O & Edge-Case Matrix

Ordering:
- JSON key order in V2 bodies: `flags`, then the legacy clears (if any), then `components`, then `allowed_mentions`.
- JSON key order in legacy bodies: `flags` (if emitted), then `content`, `components`, `allowed_mentions`.
- `C1` = `Components::new().push(TextDisplay::new("hi"))`, which serializes as `[{"type":10,"id":1,"content":"hi"}]`.
- `row` = `ActionRow::buttons([LinkButton::new("https://a.b").label("A")])`, which serializes as `{"type":1,"components":[{"type":2,"style":5,"label":"A","url":"https://a.b"}]}`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Flags empty | `CreateFlags::empty()`, `::default()`, likewise Edit/Callback | `bits()==0`, `is_empty()` | none |
| Flag values | `CreateFlags::SUPPRESS_EMBEDS`, `::SUPPRESS_NOTIFICATIONS`; `EditFlags::SUPPRESS_EMBEDS`; `CallbackFlags::{SUPPRESS_EMBEDS, EPHEMERAL, SUPPRESS_NOTIFICATIONS}` | bits 4, 4096; 4; 4, 64, 4096 | none |
| Flag union | `A | B`, `a |= b`, `A.union(B)`, `A | A` | bits OR-ed; idempotent; `contains(A)`, `contains(A|B)` true; `empty().contains(empty())` true; `A.contains(B)` false | none |
| Flags const | `const F: CreateFlags = CreateFlags::SUPPRESS_EMBEDS.union(CreateFlags::SUPPRESS_NOTIFICATIONS);` | compiles, bits 4100 | none |
| V2 create default | `V2Message::new(C1).create_body(CreateFlags::empty())` | `{"flags":32768,"components":[…C1],"allowed_mentions":{"parse":[]}}` | none |
| V2 create flags | `…create_body(CreateFlags::SUPPRESS_NOTIFICATIONS)` | `flags` 36864 | none |
| V2 callback ephemeral | `…callback_body(CallbackFlags::EPHEMERAL)`; all three flags | `flags` 32832; 36932 | none |
| V2 edit of V2 | `…edit_body(MessageFormat::V2, EditFlags::empty())`; with `SUPPRESS_EMBEDS` | `{"flags":32768,"components":…,"allowed_mentions":…}`; 32772; no clear keys | none |
| V2 edit of legacy | `…edit_body(MessageFormat::Legacy, EditFlags::empty())` | `{"flags":32768,"content":null,"embeds":[],"poll":null,"sticker_ids":[],"components":…,"allowed_mentions":{"parse":[]}}` | none |
| V2 empty body | `V2Message::new(Components::new())` | `"components":[]` (accepted; not a catalog rule) | none |
| V2 from | `V2Message::from(C1)` | equals `V2Message::new(C1)` JSON | none |
| V2 getters | `.components()`, `.attachment_set()` (empty by default), `.allowed_mentions()` (== `Mentions::default()`) | as stated; `.attachments(set)` replaces the set; bodies never emit it | none |
| Unresolved id | V2 or legacy message holding a `select` Route component, unresolved | `serde_json::to_value(body)` is `Err` naming "unresolved"; after `custom_ids_mut()` + `resolve`, it serializes | serde error |
| custom_ids_mut | `#[doc(hidden)] V2Message::custom_ids_mut()` / `LegacyMessage::custom_ids_mut()` | pre-order, same as `Components::custom_ids_mut` / rows in order | none |
| Mentions default | `Mentions::default()` == `Mentions::none()` | `{"parse":[]}` | none |
| Users all | `.users(Users::All)` | `{"parse":["users"]}` | none |
| Users only | `.users(Users::only([Id::new(1), Id::new(2)]))` == `Users::Only(vec![..])` | `{"parse":[],"users":["1","2"]}` (order and duplicates verbatim) | none |
| Only empty | `Users::Only(vec![])`, `Roles::Only(vec![])` | omitted: `{"parse":[]}` | none |
| Roles | `Roles::All`; `Roles::only([Id::new(5)])` | `"parse":["roles"]`; `"roles":["5"]` | none |
| Everyone / replied | `.everyone(true)`, `.replied_user(true)`; `false` | `"parse":["everyone"]`, `"replied_user":true`; false → omitted | none |
| Mixed | `Users::All` + `Roles::only([5])` + everyone | `{"parse":["users","everyone"],"roles":["5"]}`. The parse order is always users, roles, everyone | none |
| Mentions 101 ids | `Users::only((1..=101).map(Id::new))` | accepted and serialized (MSG-04 is runtime, Story 4.3) | none |
| Mentions on body | `V2Message::new(C1).mentions(m)` | body's `allowed_mentions` is `m`'s JSON | none |
| Legacy create | `LegacyMessage::new("héllo 😀").create_body(CreateFlags::empty())` | `{"content":"héllo 😀","components":[],"allowed_mentions":{"parse":[]}}` (no `flags` key) | none |
| Legacy flags | `create_body(CreateFlags::SUPPRESS_EMBEDS)`; `callback_body(CallbackFlags::EPHEMERAL)` | `"flags":4`; `"flags":64` | none |
| Legacy rows | `.rows([row.clone(), row])` | `"components":[row,row]` without `id`s; `.rows` again replaces | none |
| Legacy 0 / 5 rows | `.rows([])`, `.rows([row; 5])` (clones) | `[]`; 5 rows | none |
| Legacy edit of legacy | `edit_body(MessageFormat::Legacy, EditFlags::empty())` | `Ok`, `{"flags":0,"content":…,"components":[…],"allowed_mentions":…}` (`flags` always present on edits) | none |
| Legacy edit of V2 | `edit_body(MessageFormat::V2, any)` | — | `Err(UiError::V2ToLegacy)`, Display mentions "Components V2" and "CV2-01" |
| Legacy content limits | `""`, 2001 chars | accepted and serialized verbatim (MSG-01 runtime 4.3, literal 4.4) | none |
| Legacy getters | `.content_text()`, `.allowed_mentions()`, `.row_count()` | as set | none |
| Format from flags | `MessageFormat::from_flags(0)`, `(4)`, `(32768)`, `(32768|64)`, `(u64::MAX)` | Legacy, Legacy, V2, V2, V2 | none |
| [CV2-15] 6 rows | `LegacyMessage::new("x").rows([row; 6])` | does not compile; `.stderr` has `[CV2-15]` | compile error |
| [CV2-01] V2 flag | `EditFlags::IS_COMPONENTS_V2` (also Create/Callback) | E0599 | compile error |
| [CV2-02] legacy field | `V2Message::new(c).content("x")`, `.embeds(..)`, `.poll(..)`, `.sticker_ids(..)`, `.shared_client_theme(..)` | E0599 | compile error |
| [MSG-07] wrong endpoint | `EditFlags::SUPPRESS_NOTIFICATIONS`, `CreateFlags::EPHEMERAL`, `EditFlags::EPHEMERAL` | E0599 | compile error |
| [INT-09] callback flag | `CallbackFlags::CROSSPOSTED`, `CallbackFlags::from_bits(1)` | E0599 | compile error |
| [MSG-03] parse + ids | `Users::All(vec![])` | E0618 | compile error |
| [MSG-05] no mentions | `V2Message::new(c).mentions(None)` | E0308 | compile error |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/lib.rs` -- crate root. Add `mod message;` (a dir `message/{mod.rs,flags.rs,mentions.rs,body.rs}` or a single file, owned by the code) and `mod arity;`, a crate-doc "# Messages" section and the re-exports.
- `crates/noctis-ui/src/components.rs` -- `Components` (`Serialize` numbers ids; `custom_ids_mut` at L60). The V2 body serializes it as `"components"`.
- `crates/noctis-ui/src/component/layout.rs` L120-160 -- `ActionRow` (`Wire`, plain `Serialize` without ids, `custom_ids_mut` via `Wire`). Legacy rows reuse it.
- `crates/noctis-ui/src/wire.rs` -- reuse `WiredSeq(&rows, None)` for legacy rows, plus the `opt`/`flag` helpers.
- `crates/noctis-ui/src/placement.rs` -- pattern for sealed traits plus `#[diagnostic::on_unimplemented]` (copy the `sealed::Sealed` approach for `LegacyRows`).
- `crates/noctis-ui/src/attachment.rs` -- `AttachmentSet` (`Clone`, `Default`) is held by `V2Message`.
- `crates/noctis-ui/src/error.rs` -- add the `V2ToLegacy` variant.
- `crates/noctis-ui/tests/components.rs` -- reference for hand-written test Routes (`Payload`+`Route`+`Handled`+`SelectRoute`) and insta usage.
- `crates/noctis/src/lib.rs` L297-372 -- facade `pub mod ui`. Add the re-exports and doc bullets.
- `crates/noctis/tests/ui/msg/MSG-03_all_with_ids.rs` -- fixture style. `tests/ui.rs` globs `tests/ui/**/*.rs`, so no registration is needed.
- `crates/noctis-rules/src/message.rs` -- read-only. It holds the borrowed `AllowedMentions`/`MentionIds` and `validate_content`, which 4.3 will feed from `Mentions`/`LegacyMessage`. Do not change it.
- `docs/discord-rules.md` rows CV2-01 (L189), CV2-02 (L190), CV2-15 (L203), MSG-03 (L227), MSG-05 (L229), MSG-07 (L231), INT-09 (L164) -- How text to update. Leave the Enforcement columns unchanged.
- `xtask/rules-pending.toml` -- entries INT-09 (L262), CV2-01 (L297), CV2-02 (L302), CV2-15 (L347), MSG-01 (L397), MSG-05 (L402), MSG-07 (L407).
- `xtask/src/rules_catalog/mod.rs` L335-420 -- fixture rules: `tests/ui/<area>/<ID>_<slug>.rs`, where the area dir is the lowercase id prefix (`int/` for INT-09). By-construction evidence is a `// [ID]` tag after code. Diagnosed evidence is `[ID]` in `.stderr`.
- `xtask/src/skill_check/mention.rs` -- every public item's last path segment (types, methods, variants, consts) must appear as a token in the skill's Markdown.
- `xtask/src/skill_check/doctrine.rs` -- UD-1 flags `LegacyMessage`/`.content(` inside template handlers. Keep `LegacyMessage` out of the template's `src/`.
- `examples/project-template/src/modules/setup.rs` L262 -- `V2Message::new().container(…)`, to reconcile.
- `examples/project-template/.claude/skills/noctis/references/components-v2.md` §0 (L31-36), §12 (L363-366), L404; `views-and-widgets.md` L19, L144-149 (`AllowedMentions` → `Mentions`); `SKILL.md` L21 API hash.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-ui/src/error.rs`:
   - Add `V2ToLegacy`, with message `"a Components V2 message cannot be edited back into a legacy message (CV2-01: the IS_COMPONENTS_V2 flag cannot be removed)"`.
2. `crates/noctis-ui/src/arity.rs`:
   - `pub struct Arity<const N: usize>;`, documented as a type-level array length used only in bounds.
   - `#[diagnostic::on_unimplemented(message = "[CV2-15] a LegacyMessage holds at most 5 Action Rows", label = "more than 5 Action Rows", note = "`{Self}` is the number of rows; send the rest in another message, or build a V2Message")] pub trait LegacyRows: sealed::Sealed {}`, implemented for `Arity<0>`..=`Arity<5>` (a macro is fine).
3. `crates/noctis-ui/src/message/…`:
   - Flag sets per Boundaries; one private `macro_rules!` may generate the shared impls.
   - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum MessageFormat { Legacy, V2 }` with `const fn from_flags(bits: u64) -> Self`.
   - `Users`/`Roles` (`#[derive(Clone, Debug, Default, PartialEq, Eq)]`; `#[default] None`, `All`, `Only(Vec<Id<…Marker>>)`) with `fn only(impl IntoIterator<Item = Id<…>>) -> Self`.
   - `Mentions` (private fields; `Default`; `const fn none()`; `users`, `roles`, `everyone`, `replied_user` setters; `Serialize` per Matrix).
   - `V2Message` (`new(Components)`, `From<Components>`, `attachments(AttachmentSet)`, `mentions(Mentions)`, getters, hidden `custom_ids_mut`, `create_body`, `callback_body`, `edit_body(MessageFormat, EditFlags)`).
   - `LegacyMessage` (`new(impl Into<Cow<'static, str>>)`, `rows<const N: usize>([ActionRow; N]) where Arity<N>: LegacyRows`, `mentions`, getters, hidden `custom_ids_mut`, `create_body`, `callback_body`, `edit_body(..) -> Result<MessageBody<'_>, UiError>`).
   - `MessageBody<'a>` (private enum inside).
   - Doc comments state that `LegacyMessage` is the only legacy path, plain text only, and not for interactive UI (UD-1).
   - The `V2Message` docs carry five `compile_fail` doctests, one per forbidden method, each with a `// [CV2-02]` line.
   - The flag docs carry `compile_fail` doctests tagged `// [CV2-01]`, `// [MSG-07]` and `// [INT-09]`.
   - The `Mentions` docs carry `compile_fail` doctests tagged `// [MSG-03]` and `// [MSG-05]`.
   - Each `compile_fail` doctest has a passing twin or a shared setup so that it fails only on the tagged line.
4. `crates/noctis-ui/src/lib.rs`:
   - Re-export `V2Message, LegacyMessage, MessageBody, MessageFormat, CreateFlags, EditFlags, CallbackFlags, Mentions, Users, Roles, Arity, LegacyRows`.
   - Add the crate-doc section with a runnable example.
5. `crates/noctis-ui/tests/messages.rs` (and committed `tests/snapshots/messages__*.snap`):
   - Cover every non-compile-error Matrix row.
   - Use `insta::assert_json_snapshot!` for each body kind (V2 create/callback/edit-V2/edit-legacy, legacy create/edit) and plain `assert_eq!` with `serde_json::json!` elsewhere.
   - Unit test names may cite only the ids allowed in Never (e.g. `cv2_01_v2_bodies_always_set_flag`, `cv2_02_legacy_to_v2_edit_clears_fields`, `msg_05_default_parses_nothing`, `msg_07_flag_bits`).
   - Add `assert_send_sync::<V2Message>()`, `<LegacyMessage>()` and `<MessageBody<'static>>()`.
6. `crates/noctis/src/lib.rs`:
   - Add the same items to `pub mod ui` and a doc bullet group "Messages" (V2 vs legacy, the three flag sets with their endpoints, mentions default, `MessageFormat`, `UiError::V2ToLegacy`).
   - Add a doctest building a V2 create body.
7. `crates/noctis/tests/messages.rs`:
   - Through `noctis::ui` only: a derived `#[route("m.ok")]` Route with `impl ::noctis::__private::ui::Handled`.
   - V2 message with a Container + row button: assert the create-body JSON.
   - Legacy message with 5 rows (CV2-15 boundary pass) and a legacy→V2 edit body.
8. `crates/noctis/tests/ui/` -- fixtures:
   - `cv2/CV2-01_v2_flag_not_settable.rs` (`let _ = noctis::ui::EditFlags::IS_COMPONENTS_V2; // [CV2-01]`).
   - `cv2/CV2-02_v2_message_content.rs` (`.content("hi"); // [CV2-02]`).
   - `cv2/CV2-15_six_legacy_rows.rs` (diagnosed; the `.stderr` must contain `[CV2-15]`).
   - `msg/MSG-03_users_all_with_ids.rs` (`Users::All(Vec::new()); // [MSG-03]`).
   - `msg/MSG-05_message_without_mentions.rs` (`.mentions(None); // [MSG-05]`).
   - `msg/MSG-07_edit_suppress_notifications.rs`, `msg/MSG-07_create_ephemeral.rs`.
   - `int/INT-09_callback_crossposted.rs` (`CallbackFlags::CROSSPOSTED; // [INT-09]`).
   - Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`, then inspect each `.stderr` to confirm it fails for the intended reason (E0599/E0618/E0308/E0277).
9. `docs/discord-rules.md` -- update How text only:
   - CV2-01: V2 bodies always add the flag; no flag set has `IS_COMPONENTS_V2`; `LegacyMessage::edit_body` on a V2 message returns `UiError::V2ToLegacy`.
   - CV2-02: also "`V2Message::edit_body(MessageFormat::Legacy, …)` sends `content: null, embeds: [], poll: null, sticker_ids: []`".
   - CV2-15: "`LegacyMessage::rows([ActionRow; N])` is bounded by `Arity<N>: LegacyRows` (N ≤ 5, `[CV2-15]`)".
   - MSG-03: "`noctis::ui::Mentions` with `Users`/`Roles` = `None | All | Only(ids)`".
   - MSG-05: "every V2 and legacy body sends `allowed_mentions`, default `parse: []`; `mentions` takes a `Mentions`, never an `Option`".
   - MSG-07: list the three types, their flags and endpoints; `IS_VOICE_MESSAGE` is not offered in v1.
   - INT-09: "`CallbackFlags`".
10. `xtask/rules-pending.toml`:
    - Remove INT-09, CV2-01, CV2-02, CV2-15, MSG-05 and MSG-07.
    - Change MSG-01's `story` to `"4.4"`.
11. `xtask/public-api.txt` -- run `cargo run --release -p xtask -- api-snapshot --bless`, then write the new hash into the skill's `SKILL.md`.
12. `examples/project-template/.claude/skills/noctis/references/components-v2.md`:
    - §0: `V2Message::new(body)` (not `[planned]`), plus the table row CV2-02 (not planned).
    - Replace §12 with "Messages: `V2Message`, `LegacyMessage`, flags and mentions", covering:
      - the constructors, setters and getters;
      - each body method with its endpoints and the exact flag sets;
      - legacy→V2 edit clears and `UiError::V2ToLegacy`;
      - `MessageFormat::from_flags`;
      - `Mentions`/`Users`/`Roles` with JSON examples;
      - CV2-15 `Arity`/`LegacyRows` and the error text;
      - "core's Responder picks the body and adds attachments [planned]";
      - UD-1: legacy is never for interactive UI.
    - Fix L404 to `V2Message::new(body)`.
    - Every new public item name must appear (skill-check mention check).
13. `examples/project-template/.claude/skills/noctis/references/views-and-widgets.md` -- `fn mentions(&self) -> Mentions { Mentions::none() }` and §7 `Mentions::none().users(Users::Only(vec![id]))` (`Users::{None, All, Only}`).
14. `examples/project-template/src/modules/setup.rs` L262 -- `Ok(V2Message::new(Components::new().push(Container::new()….push(TextDisplay::new(text)))))`, adding `Components` to the `noctis::ui` import. The template does not compile before 14.2, so this is checked by inspection. Do not add `LegacyMessage` to the template; template `AGENTS.md` changes only if a line names a changed item (none known).

**Acceptance Criteria:**
- Given a `V2Message`, when any of its three body methods is serialized, then `flags` has bit 32768 set, the JSON never has `content`/`embeds`/`poll`/`sticker_ids`/`shared_client_theme` except the four legacy→V2 clears, and `allowed_mentions` is present (`{"parse":[]}` by default).
- Given the new fixtures, when `cargo test --release -p noctis --test ui` runs, then each fails to compile, the CV2-15 `.stderr` contains `[CV2-15]`, and every other fixture's offending line carries its `// [ID]`.
- Given the rules-pending edits, when `cargo run --release -p xtask -- rules-catalog` runs, then it passes with CV2-01, CV2-02, CV2-15, MSG-05, MSG-07 and INT-09 covered by evidence and MSG-01 pending for 4.4.
- Given the facade, when a bot crate uses only `noctis::ui`, then it builds V2 and legacy bodies (facade test), and `api-snapshot` and `skill-check` pass with the skill documenting `V2Message`, `LegacyMessage`, the flag sets and mention opt-in.

## Spec Change Log

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 25 findings — high 0, medium 3, low 7, false 14, maybe-false 1
- findings:
  - `[low]` `[defer]` (blind) Template setup.rs uses `V2Message` without importing it; prelude does not re-export it — real, but pre-existing: profile.rs, dashboard.rs and setup.rs:175 already used `V2Message` via the prelude alone before this story; the prelude/import decision belongs with the View stories (template compiles from 14.2). Deferred.
  - `[false]` `[reject]` (blind) MSG-04 has no check and no `Mentions` to `AllowedMentions` bridge — the intent's Never excludes the runtime validator (4.3) and the Matrix row "Mentions 101 ids" accepts 101 ids by design; the bridge is 4.3's input per the Code Map.
  - `[medium]` `[defer]` (blind) Empty legacy and empty V2 bodies serialize though Discord rejects them with a 400 — real, but no catalog rule exists for it (pre-existing gap in docs/discord-rules.md) and runtime checks are excluded by the intent (4.3); the Matrix deliberately accepts both. Deferred with edge rows 1-2.
  - `[low]` `[patch]` (blind) MSG-01 moved to 4.4 with no note; responder.md presents "≤ 2000 chars, MSG-01" as given while components-v2.md marks the check planned — the "no note" half is false (spec Task 10 and Design Notes record the move); the doc disagreement is real in responder.md §6, which this diff edited. Patch: mark the length check as planned in responder.md §6.
  - `[false]` `[reject]` (blind) Legacy rows accept Route buttons/selects despite "never for interactive UI" — UD-1 is a doctrine rule enforced by review and skill-check on the template, not a type guarantee; CV2-15 allows buttons and selects in legacy rows and the spec Matrix row "Unresolved id" requires Route components in legacy rows.
  - `[false]` `[reject]` (blind) `LegacyMessage::rows` takes only a fixed-size array; no `try_rows` — the intent's Never puts every `try_*` constructor in Story 4.4; the array bound is the CV2-15 compile-time tier the intent asks for.
  - `[low]` `[patch]` (blind) responder.md §6 says only content/embeds/poll are cleared on a legacy→V2 edit, leaving out `sticker_ids` — confirmed at responder.md L142 while body.rs, discord-rules.md CV2-02 and components-v2.md list four fields. Patch: add `sticker_ids` to the list.
  - `[low]` `[reject]` (blind) `MessageFormat::from_flags` takes only a raw `u64` — real ergonomic cost (`.bits()` at call sites) but no wrong outcome shown; the spec sets this exact signature and the fix adds public surface (`From<MessageFlags>`).
  - `[false]` `[reject]` (blind) `LegacyMessage::custom_ids_mut` allocates a Vec while V2 is lazy — `Components::custom_ids_mut` (components.rs L62-68) also collects into a Vec and returns `into_iter()`; both paths behave the same.
  - `[false]` `[reject]` (blind) Duplicate mention ids are kept — the Matrix row "Users only" fixes "order and duplicates verbatim"; any count over 100 is MSG-04, runtime in 4.3 per the intent's Never.
  - `[false]` `[reject]` (blind) Callback type 7 / INT-08 deferred original: no guidance on deriving `MessageFormat` — the Responder is excluded by the intent's Never (Epics 6/7/9); no code in this diff derives a format, so no wrong derivation occurs here.
  - `[false]` `[reject]` (blind) Webhook `with_components=true` requirement missing from the docs — WH-01 documents it in docs/discord-rules.md and the intent's Never excludes the `with_components` query (Epics 6/7/9).
  - `[maybe-false]` `[reject]` (blind) `SUPPRESS_EMBEDS` is documented as hiding link embeds of `content` yet accepted on V2 bodies — would need Discord's behaviour for URLs inside Text Display on V2 messages to settle; if true it is only a doc wording issue (low), so rejected with this note.
  - `[false]` `[reject]` (blind) public-api.txt lists the `LegacyRows` impls twice — it is generator output from `api-snapshot --bless`, which lists trait impls in both the type's block and the trait's block; the check compares the same output, so the hash matches what the gate verifies.
  - `[medium]` `[defer]` (edge) V2Message from empty Components serializes `components: []`, which Discord rejects — same root cause as blind row 3 (no empty-message rule). Deferred.
  - `[medium]` `[defer]` (edge) `LegacyMessage::new("")` with zero rows serializes, which Discord rejects — same root cause as blind row 3. Deferred.
  - `[false]` `[reject]` (edge) Users/Roles `Only` with more than 100 ids serialize unchecked — MSG-04 is runtime (4.3), excluded by the intent's Never; Matrix row "Mentions 101 ids" requires acceptance.
  - `[false]` `[reject]` (edge) Duplicate ids passed to `only()` serialize verbatim — the Matrix fixes duplicates verbatim; the count limit is MSG-04 (4.3).
  - `[low]` `[reject]` (edge) A legacy→legacy edit leaves earlier embeds/poll/stickers in place — noctis has no embeds, poll or sticker API, so a legacy message noctis sent has none; only a message sent outside noctis could, which is unlikely, and the fix adds fields the Matrix's legacy edit body does not have.
  - `[low]` `[defer]` (edge) Template setup.rs uses `V2Message` unqualified — same root cause as blind row 1 (pre-existing prelude assumption). Deferred.
  - `[false]` `[reject]` (intent) The V2ToLegacy error depends on a caller-supplied `MessageFormat` — the spec deliberately settles the guarantee at `edit_body(MessageFormat, ..)` and the intent's Never excludes the Responder that reads the real flags; the epics AC ("a typed error") is met at this API.
  - `[false]` `[reject]` (intent) "Never fail with a 400" is not exercised at the send surface — the intent's Never excludes the Responder/HTTP (Epics 6/7/9); the AC is stated at serialization.
  - `[low]` `[patch]` (intent) MSG-01 listed under the story but moved to 4.4; responder.md still reads as a guarantee — the move is deliberate and recorded (Design Notes: the literal tier needs 4.4's macro); the responder.md wording is the real part, same root cause as blind row 4. Patch as there.
  - `[false]` `[reject]` (intent) FR-25/UX-DR1 mention default proven on message types, not Views — Views are a later epic; the skill marks the View `mentions` hook [planned], and every body the View will produce already defaults to `parse: []`.
  - `[false]` `[reject]` (intent) MSG-05 and CV2-01 compile_fail evidence covers a neighbouring property — the AC asks the fixture to try "the missing field, method or flag"; `.mentions(None)` and `IS_COMPONENTS_V2` are exactly the missing setter and flag, and the spec Tasks 8 name those fixtures.
  - `[false]` `[reject]` (intent) Flag sets reject `IS_VOICE_MESSAGE`, which is valid — the spec's Boundaries deliberately leave it out ("not offered in v1"); omitting a valid flag cannot cause a 400.

## Design Notes

- **Flags at body time, not on the message:** the same `V2Message` (e.g. a View render) goes out through create, callback or edit, and only the endpoint knows which flags are legal. So core passes the endpoint's flag set when it asks for the body. Callback type 7 is an edit of an existing message, so it takes `EditFlags`. Q5 (legacy→V2 on a modal submit) stays core's typed error.
- **Why CV2-15 lands here and MSG-01 moves to 4.4:** the story's "at most 5 Action Rows" is only honest with the arity bound, which is a tiny sealed trait (logged in the memlog; 4.4 reuses `Arity<N>`). MSG-01's diagnosed tier needs a literal-measuring macro plus a content newtype, which is 4.4's design space (`label!`/`placeholder!`).
- **Legacy→V2 clear values** follow research §2.1: `content`/`poll` → `null`, `embeds`/`sticker_ids` → `[]`.
- Sketch:
  ```rust
  let body = V2Message::new(components).mentions(Mentions::none().users(Users::All));
  let json = serde_json::to_value(body.edit_body(MessageFormat::Legacy, EditFlags::empty()))?;
  LegacyMessage::new("hi").edit_body(MessageFormat::V2, EditFlags::empty()) // Err(UiError::V2ToLegacy)
  ```

## Verification

**Commands** (repo root; CI is paused, so local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (noctis-ui `messages` snapshots, doctests incl. compile_fail, facade `messages` and `ui` suites, xtask tests).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- exit 0 each.
- `bash .github/scripts/check-build-config.sh` -- exit 0; `test ! -d target/debug` -- true.

## Auto Run Result

- **Summary:** Added `V2Message` and `LegacyMessage` bodies in noctis-ui with per-endpoint flag sets (`CreateFlags`, `CallbackFlags`, `EditFlags`; `IS_COMPONENTS_V2` cannot be set by hand), a typed `Mentions` model that defaults to `parse: []` and is required on every body, `MessageFormat`-aware edit bodies (legacy→V2 clears content/poll to `null` and embeds/sticker_ids to `[]`; V2→legacy returns `UiError::V2ToLegacy`), and a compile-time cap of 5 legacy Action Rows through the sealed `Arity<N>` bound (CV2-15). The MSG-01 length check moves to Stories 4.3 (runtime) and 4.4 (literal).
- **Files changed:**
  - `crates/noctis-ui/src/message/{mod,body,flags,mentions}.rs`: message bodies, flag sets, mentions and edit-body serialization.
  - `crates/noctis-ui/src/arity.rs`: sealed `Arity<N>` bound that caps legacy rows (4.4 reuses it).
  - `crates/noctis-ui/src/{lib,error,placement}.rs`: re-exports and the `UiError::V2ToLegacy` variant.
  - `crates/noctis-ui/tests/messages.rs` plus 8 insta snapshots: tests for the serialization matrix.
  - `crates/noctis/src/lib.rs`, `crates/noctis/tests/messages.rs`: facade re-exports and facade tests.
  - `crates/noctis/tests/ui/{cv2,int,msg}/*`: compile_fail fixtures for CV2-01, CV2-02, CV2-15, INT-09, MSG-03, MSG-05 and MSG-07.
  - `docs/discord-rules.md`, `xtask/rules-pending.toml`: where each rule is enforced.
  - `xtask/public-api.txt`: re-blessed API snapshot.
  - `examples/project-template/` (skill SKILL.md, components-v2.md, responder.md, views-and-widgets.md; src/modules/setup.rs): template and usage skill updated to the new API.
- **Review findings (25: high 0, medium 3, low 7, false 14, maybe-false 1):**
  - Patched (3 low; 0 high, 0 medium): responder.md §6 now marks the MSG-01 length check as planned (blind 4 and intent 3, same root cause) and lists `sticker_ids` among the fields cleared on a legacy→V2 edit.
  - Deferred (5): empty V2/legacy bodies serialize although Discord rejects them (3 medium; no catalog rule exists, and runtime checks belong to 4.3). The template's `setup.rs` uses `V2Message` through the prelude only (2 low; this was already the case before this story, belongs with the View stories, and the template compiles from 14.2).
  - Rejected (17):
    - MSG-04 has no check and no AllowedMentions bridge: runtime check and bridge are Story 4.3.
    - Route components accepted in legacy rows: UD-1 is a doctrine rule, and CV2-15 allows them.
    - No `try_rows`: every `try_*` constructor is in 4.4.
    - `from_flags` takes a raw `u64`: this is the signature the spec sets, and no wrong outcome was shown.
    - `custom_ids_mut` allocates a Vec: same as `Components`.
    - Duplicate mention ids are kept (x2): the Matrix keeps them verbatim.
    - More than 100 mention ids are accepted: MSG-04 is runtime in 4.3.
    - Callback type 7 format derivation: belongs to the Responder, which is excluded from this story.
    - Webhook `with_components` missing: already documented in WH-01, and excluded from this story.
    - `SUPPRESS_EMBEDS` on V2 bodies (maybe-false): at most a doc wording issue.
    - public-api.txt lists impls twice: this is what the generator outputs.
    - A legacy→legacy edit leaves old embeds in place: noctis has no embeds API.
    - V2ToLegacy depends on a caller-supplied format: this is the guarantee the spec sets.
    - The "no 400" acceptance criterion is not tested at send time: the HTTP layer is excluded from this story.
    - Mention default is proven on message types, not Views: Views come in a later epic.
    - The MSG-05/CV2-01 fixtures are adequate: they try exactly the missing setter and flag.
    - `IS_VOICE_MESSAGE` is left out: it is not offered in v1.
- **Follow-up review recommended:** false (patched entries: high 0, medium 0, low 3).
- **Verification:** All local gates in gates.sh passed: fmt, clippy -D warnings, tests with `INSTA_UPDATE=no`, cargo hack each-feature, MSRV 1.91 check, cargo deny, the xtask checks (deps-check, rules-catalog, api-snapshot, skill-check) and the build-config check; no `target/debug` was created. CI is paused by the maintainer, so the local gates were the whole check.
- **Residual risks:** Discord returns a 400 for empty bodies, and noctis does not reject them yet (needs a new catalog rule, at the latest in 4.3). MSG-01 and MSG-04 are not enforced until Stories 4.3 and 4.4. The `MessageFormat` passed to edits is supplied by the caller until the Responder reads the real message flags. Whether `SUPPRESS_EMBEDS` has any effect on V2 bodies has not been checked.
