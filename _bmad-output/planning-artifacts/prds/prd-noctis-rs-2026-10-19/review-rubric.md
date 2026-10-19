# PRD Quality Review — Noctis

- **PRD:** `_bmad-output/planning-artifacts/prds/prd-noctis-rs-2026-10-19/prd.md`
- **Rubric:** `.claude/skills/bmad-prd/assets/prd-validation-checklist.md`
- **Stakes / shape:** launch-level, developer product (framework/library), chain-top (feeds architecture → epics → stories)
- **Addendum:** not present (although §0 refers to `addendum.md`)

## Overall verdict

The PRD has a clear thesis: an opinionated twilight layer where everything that stores data is a swappable Adapter, plus compile-time safety that tescord lacks. It also has a disciplined glossary, specific NFRs with numbers, and honest Non-Goals. It is not yet launch-ready as a build contract. About 20 of the 51 FRs, including the Wizard, Navbar and Views that carry UJ-1, UJ-2 and SM-1, have no testable consequences. The PRD's own example Command Pattern (FR-8) expands into a structure Discord rejects. Two core runtime decisions are stated in ways that contradict themselves or the requirement they serve: FR-5 backpressure versus heartbeats and interaction deadlines, and the FR-17 Ref length bound versus the claim that Refs can't be guessed. Fix these before architecture locks the design. The rest is polish.

**Gate verdict:** Revise before handoff. Grade: Fair (no broken dimensions, one thin dimension, 4 high findings).

## Decision-readiness — adequate

Most choices are stated as decisions rather than "considerations". FR-9 explicitly rejects "first wins". FR-17 makes overflow policy configurable per Route. FR-33 chooses animated WebP with a GIF fallback. §7 states seven hard Non-Goals. The Open Questions (§10) are genuinely open and each names the FR it affects, which is good practice.

What's missing is any acknowledgement of the trade-offs inside the runtime model. The PRD contains no `[NOTE FOR PM]` callouts at all. Some places need one: FR-5 promises a bounded queue where "nothing is silently dropped", and also that a slow handler "does not delay heartbeats". FR-45 promises per-guild Module disable for bots whose Commands are global. FR-21 promises "the right deferred response" without saying who picks ephemeral versus public before the handler has run. A reviewer pushing back on any of these would find the objection dodged, not answered.

### Findings
- **high** Backpressure policy contradicts itself (§4.1 FR-5). "When the bound is reached, new work waits; nothing is silently dropped" and "a handler that sleeps for 10 s does not delay heartbeats". If dispatch waits, the shard read loop stops being polled. In twilight that stalls heartbeats and lets queued interactions blow past Discord's 3 s acknowledgement window. The PRD has to choose what gives way when the bound is reached: drop events, shed lower-priority events, apply a per-shard bound, or keep a separate fast lane for interactions. *Fix:* state the saturation policy per event class: interactions are always acked within the FR-21 deadline, and gateway events follow a stated policy (buffer to N, then drop with a metric, or similar). Add a `[NOTE FOR PM]` if the policy is undecided.
- **medium** Per-guild Module disable conflicts with global Commands (§4.8 FR-45). "Its Commands can be left out of a guild's Publish" works only for guild-scoped Commands. For a global Command, a disabled Module still shows its Commands in that guild. Per-guild Publish for 400k guilds (UJ-3) is not feasible under Discord rate limits. *Fix:* state what the user sees when a disabled Module's global Command is invoked (for example a localized "disabled here" notice), and limit "left out of Publish" to guild-scoped Commands.
- **medium** Deferred response type is unspecified (§4.3 FR-21). "Noctis sends the right deferred response" leaves several things open. Ephemerality is fixed when the defer is sent. Component interactions can defer as either update or new message. FR-18 and FR-22 want ephemeral replies. *Fix:* define how a handler or Route declares its defer kind (ephemeral or public, update or reply), and what happens when a late response conflicts with the defer already sent. Replace "about 2.5 s" with an exact configurable default.
- **low** No `[NOTE FOR PM]` callouts anywhere. At launch stakes, the tensions above (and FR-47's sharing of Adapters across Apps) deserve visible flags. *Fix:* add callouts at FR-5, FR-21 and FR-45, or resolve them inline.

## Substance over theater — strong

The Vision (§1) can't be swapped into another PRD. It names tescord, twilight, Components V2 dashboards and the Adapter rule. The JTBDs (§2.1) are concrete and each drives features. Non-Users (§2.2) do real work. The NFRs carry product-specific thresholds (p99 ≤ 1 ms at 10k events/s on 4 cores; card render p50 ≤ 10 ms, p99 ≤ 50 ms; ≤ 1 KiB per idle guild) instead of boilerplate. Counter-metrics SM-C1 and SM-C2 are thoughtful, not decorative. There is no persona or innovation theater.

### Findings
- **low** NFR-4 "Every panic in a handler is caught" is close to boilerplate unless it is bounded (§5 NFR-4). It can't hold under `panic = "abort"` or for panics inside the Renderer pool. *Fix:* scope it: "under the default `panic = "unwind"` profile, a handler or render panic becomes an error response and a logged event; the shard keeps running."

## Strategic coherence — adequate

The thesis is coherent. Raw twilight forces every team to rebuild the same things, so Noctis supplies them as one layer with a single storage rule and stronger type safety than tescord. Features map to it cleanly, and each feature group lists the UJs it serves. Counter-metrics exist and are paired.

The weaknesses are in scope logic and validation. §8.1 puts 50 of 51 FRs in "MVP", so the MVP is simply v1. No MVP kind is named and there is no cut line, which at launch stakes means no plan for a slip. The Success Metrics validate ergonomics (SM-1, SM-3) and dogfooding by the author (SM-2), but nothing tests the thesis that *other teams* want this layer. That thesis is the stated reason to publish a crate at all.

### Findings
- **medium** "MVP" is all of v1, with no priority or cut line (§8.1). "FR-1 to FR-13 and FR-15 to FR-51" makes Testing kit (FR-51), Multiple Apps (FR-47), the render cache (FR-36) and the Raw escape hatch (FR-31) as launch-blocking as Commands and Routes. *Fix:* name the MVP kind (platform), split into must-ship-for-launch and fast-follow, and say what slips first.
- **medium** No metric validates external adoption or the launch itself (§9). SM-2 is the author's own bot, marked `[ASSUMPTION]`. For a crate published to crates.io at launch stakes, nothing measures whether anyone besides the author succeeds with it. *Fix:* add a launch-window SM, such as N external bots built from the Project Template within 90 days, or a time-to-first-dashboard for a new user, with a counter-metric for issue volume or migration complaints.
- **medium** SM-5 can't be measured as written (§9 SM-5). "Zero Limit-related 400 responses from Discord in production logs" gives no window, no source bot, and no way to tell Limit-related 400s apart from others. *Fix:* tie it to a named bot (SM-2's), set a window, and specify the error-code classification (for example 50035 with a component path).

## Done-ness clarity — thin

Where FRs carry consequences, the consequences are good: specific, testable and often adversarial (FR-9, FR-16, FR-19, FR-24, FR-28). The trouble is coverage. Roughly 20 FRs have no consequences: FR-11, FR-12, FR-14, FR-25, FR-26, FR-27, FR-30, FR-31, FR-36, FR-37, FR-39, FR-40, FR-41, FR-43, FR-44, FR-45, FR-46, FR-47, FR-48 and FR-51. They include the very widgets that UJ-1 and UJ-2 and SM-1 depend on. Story authors will have to invent acceptance criteria for the headline features.

Several consequences also depend on an outside specification or carry adjectives instead of bounds.

### Findings
- **high** The core UI widgets have no testable consequences (§4.4 FR-25, FR-26, FR-27; also FR-30, FR-36, FR-46, FR-47, FR-51). The Wizard (FR-27) has "per-Step validation" and "back/next/cancel" with no stated behaviour: what does a failed validation show, what does cancel leave behind, and what happens on a Step-state Ref expiry beyond UJ-1's prose? The Navbar (FR-26) doesn't say how it composes with the Paginator's reserved budget. FR-46 doesn't say whether a rate-limit check counts before or after the defer. *Fix:* add at least one verifiable consequence per FR. Prioritise the Wizard, Navbar, Views, rate limits and the Testing kit, since stories will lean on these first.
- **high** FR-8's own example pattern expands to a structure Discord rejects, and its semantics are delegated to an outside spec (§4.2 FR-8). `"config (set|get) (prefix|welcome)?"` produces both `config set` (subcommand) and `config set prefix` (`set` as a subcommand group). Discord does not allow one name to be both, and FR-9's own rule rejects it. The FR doesn't say whether this fails registration. "Using the same semantics as tescord" makes the done-condition depend on another codebase. *Fix:* spell out expansion semantics inline (ordering, `?` behaviour, maximum expansions). State that expansions colliding as subcommand and group fail with a typed error. Replace the example with a valid one.
- **high** The Ref length bound runs the wrong way for its security claim (§4.3 FR-17). "A Ref is a random key of at most 12 characters, so it can't be guessed" sets a ceiling where unguessability needs a floor. NFR-6 depends on this. *Fix:* state a minimum entropy (for example ≥ 64 bits from a CSPRNG) and a maximum length. Add a consequence that Refs are never sequential or derived from interaction data.
- **medium** Compile-time guarantees are stated as requirements with no feasibility tag (§4.4 FR-23, §4.5 FR-32, §4.7 FR-42). "An illegal placement doesn't compile", "unknown Tailwind classes in literal class strings are compile errors … same parser as rendering", and "a missing key in the default Locale is a compile error" each commit the architecture: running takumi's parser in a proc macro, and generating code from Locale files at build time. None is tagged `[ASSUMPTION]`. *Fix:* tag them, or add a fallback consequence (for example "or a failing test generated by the build") so architecture can de-risk them.
- **medium** NFR-3 is stated as a firm target, but Open Question 4 says it isn't decided (§5 NFR-3, §10 Q4, §9 SM-4). NFR-3 has hard numbers and no `[ASSUMPTION]` tag, while Q4 says "precise … NFR-3 latency targets … are set in architecture". SM-4 ("benchmarks pass in CI") then has no fixed pass line, and "under load" and "standard card" aren't defined. *Fix:* either tag NFR-3 as provisional and index it, or remove NFR-3 from Q4. Define the load profile and the reference template for the card benchmark.
- **low** FR-10 truncates silently while FR-24 refuses to send (§4.2 FR-10). Truncating more than 25 autocomplete choices with only a log warning is reasonable, but it is inconsistent with FR-24's strict default and lenient opt-in. *Fix:* state that this is a deliberate exception, or make it follow the same strict/lenient switch.

## Scope honesty — adequate

§7 Non-Goals are crisp and do real work (no own gateway, no voice, no web dashboard, tokio only, no hot reload, no Tailwind CLI, no remote renderer). §8.2 is explicit about what v1.x defers and why (Redis/SQL adapters, broadcastEval, the Primary Entry Point command type). There are 4 Open Questions, 6 assumptions and 0 notes. That density suits launch stakes, but every one of the open items concerns the platform rather than the product's own decisions. The decisions that are genuinely unresolved (FR-5, FR-21, FR-45) aren't flagged.

### Findings
- **medium** Multi-App Adapter sharing is underspecified (§4.8 FR-47, §3 App). "White-label bots, which share Adapters where the author chooses" doesn't say whether Routes, Refs, SessionStore keys and rate-limit keys are namespaced per App. Without that, two Apps sharing a RefStore can resolve each other's Refs. *Fix:* add a consequence that Adapter keys are namespaced per App id by default.
- **low** FR-14's scope status is ambiguous (§4.2 FR-14, §8.1). The heading carries `[ASSUMPTION: v1 includes an opt-in module]` and §8.1 says "with the FR-14 prefix module opt-in". A reader can't tell whether it is launch-blocking. *Fix:* say "in scope, behind the `prefix` feature, not enabled by default", or move it to §8.2.
- **low** Launch artefacts aren't scoped. §8.1 lists "Documentation" with no bound: docs.rs coverage, a tescord migration guide for UJ-4, the crates.io publish and the changelog (§6). *Fix:* list the documentation deliverables required at launch.

## Downstream usability — adequate

This PRD is chain-top, so this dimension matters. The Glossary (§3) is strong. Terms are capitalised and used consistently across FRs and UJs, and the Adapter names match exactly between §3 and FR-38. FR, NFR, SM and UJ IDs are contiguous and unique, and each feature group says which UJs it realises. A few references don't resolve and a few terms are missing from the glossary.

### Findings
- **medium** The referenced addendum doesn't exist, yet "how" leaks into the PRD (§0, §5 NFR-7, §10 Q3). §0 says technical "how" lives in `addendum.md`, and no addendum exists in the PRD folder. Meanwhile the PRD names takumi, `tracing`, edition 2024, MSRV 1.91, twilight 0.17.1/0.18 and the `bucket()` API. *Fix:* create the addendum and move the implementation details there, or drop the §0 claim and accept these as constraints.
- **low** Some terms used in FRs are missing from the Glossary: Extension (FR-20), error sink (FR-22), Pack (§4.8), upload size budget (FR-33). *Fix:* add them to §3.

## Shape fit — strong

For a developer product, the shape is right. The "users" are bot authors and an AI agent, and the UJs follow a named developer through realistic tasks (Deniz, Ece, Mert) with edge cases (UJ-1's expired Ref). Treating an AI agent as a first-class user (UJ-5, FR-50, SM-3) is unusual and well-earned. It is backed by a testable metric (≥ 4 of 5 trials). The PRD is neither over-formalised nor missing a section a library PRD needs. The one exception is API ergonomics: no example snippet or target shape of the builder API anchors SM-1's "≤ 150 lines".

### Findings
- **low** UJ-5's protagonist is unnamed ("An AI agent"). Acceptable for an agent, but naming the agent setup (for example "a Claude Code agent with only the Project Template") would make SM-3 reproducible. *Fix:* name the agent and its constraints in UJ-5 and SM-3.

## Mechanical notes

- §0 says "Inline `[ASSUMPTION]` tags are collected in §10", but the Assumptions Index is §11 (§10 is Open Questions).
- Assumptions Index roundtrip: all 6 inline tags (FR-14, FR-33, FR-35, FR-50, NFR-1, SM-2) appear in §11, and all §11 entries appear inline. FR-35 and SM-2 use a bare `[ASSUMPTION]` with no inline text.
- Glossary "Limits … Research §0 lists them all" doesn't say which of the four research reports it means (presumably the Components V2 report).
- "Template" (image layout) and "Project Template" (starter repo) overload one word. Consider "Image Template" or "Card Template".
- §4.7 says "Realizes UJ-4", but UJ-4 doesn't mention localization (it covers patterns and typed options). §4.2 already covers UJ-4.
- UJ-1 says "runs the bot with the publish flag, and `/setup start` appears in the test guild". Per FR-13 that needs `--publish-guild <id>`, not `--publish` (global).
- ID continuity: FR-1–51, NFR-1–8, SM-1–5, SM-C1–C2 and UJ-1–5 are contiguous, with no duplicates.
- Required sections for this stakes and type are present (Vision, Users, UJs, Glossary, FRs, NFRs, API/versioning, Non-Goals, MVP, SMs, Open Questions, Assumptions). A launch/release criteria section is absent; see Scope honesty.
