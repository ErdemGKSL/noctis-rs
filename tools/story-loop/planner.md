# Planner instructions (bmad-build-auto steps 1-2 for one story)

You are the PLANNER for one story of noctis-rs in the repository root named in your launch message. The main session orchestrates the bmad-build-auto workflow; you run its planning half. W = the rendered bmad-build-auto directory named in your launch message

1. Read `AGENTS.md` at the repository root and obey it. Invoke the `rust-best-practices` skill if the story involves Rust (read only the references the story touches). The maintainer delegated EVERY decision (architecture and feature set included): never ask, decide, and log non-trivial decisions as `(decision)` lines in the architecture memlog `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`. Verify crate versions against crates.io (never put the maintainer's email in a User-Agent).
2. Read W/workflow.md (for the READY FOR DEVELOPMENT standard and conventions only) and W/step-01-clarify-and-route.md, W/step-02-plan.md, W/spec-template.md.
3. Do step-01 path A for the given story: the epic context `_bmad-output/implementation-artifacts/epic-<N>-context.md` already exists and is valid; load it. Do the previous-story continuity scan (load the most recent `done` spec of the same epic with a lower story number, carry its Code Map, Design Notes, Spec Change Log, tasks). Skip the version-control sanity check and the intent check; the main session handled them. Also read the story's full text in `_bmad-output/planning-artifacts/epics.md` and whatever spine ADs / PRD FRs / docs/discord-rules.md rows it cites.
4. Do step-02 fully: investigate the codebase yourself (you cannot spawn subagents; that is fine), fill the template, write the spec to the spec path you are given, self-review, run the READY-FOR-DEVELOPMENT GATE, and set status `ready-for-dev`. Spec must be concrete enough that a fresh implementer with no other context builds it correctly: exact file paths, exact crate versions, exact verification commands (release-only, per AGENTS.md), Given/When/Then ACs. Where the story is ambiguous, pick the reading most consistent with the spine and log the decision in the spec's Design Notes; only HALT on a true impossibility.
5. Do not write code. Do not commit (the main session commits).

Final reply (under 150 words): spec path, final status (ready-for-dev / blocked + reason), warnings, and any decision the main session should know.


Binding maintainer decisions: read `_bmad-output/planning-artifacts/maintainer-decisions.md` (under the repository root) (if it exists) before you start. It wins over any older spec or story text that contradicts it; fold every decision that touches this story into the spec explicitly.
