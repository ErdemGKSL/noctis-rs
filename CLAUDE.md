# vesper-rs

Rust project. Project-level skills live in `.claude/skills/`. Use them as described below.

## When to use which skill

### `rust-best-practices` — any Rust work
Invoke this skill **before** writing, reviewing, refactoring or explaining Rust code in
this repo: any `.rs` file, `Cargo.toml`, new modules or crates, fixing compiler or
borrow-checker errors, adding structs for JSON/config data, or choosing crates.
Read only the `references/*.md` files the task touches (the index is in its `SKILL.md`).
Before calling Rust work done, run `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`
and `cargo test`.

### `image-gen` — raster images
Use it when the user asks for a picture, icon, logo, illustration, banner, favicon,
mockup or any other bitmap asset, or for an edit to an existing image. It needs the
OpenAI Codex CLI installed and logged in (`codex login`). If `codex` is missing,
say so and ask the user before installing or logging in. Don't use it for SVG/vector
art, diagrams or charts. Those are written as code.

### BMad Method skills (`bmad-*`) — planning and agile workflow
Use these when the user wants structured product or engineering process, not for
everyday coding:

| Situation | Skill |
| --- | --- |
| Unsure what to do next, or a question about BMad | `bmad-help` |
| Rough idea → brainstorm, pressure-test, research | `bmad-brainstorming`, `bmad-forge-idea`, `bmad-deep-recon`, `bmad-prfaq` |
| Product brief / requirements / spec | `bmad-product-brief`, `bmad-prd`, `bmad-spec` |
| Architecture or UX design docs | `bmad-architecture`, `bmad-ux` |
| Break work into epics/stories, plan a sprint | `bmad-create-epics-and-stories`, `bmad-sprint-planning`, `bmad-correct-course` |
| Implement a story/feature end to end | `bmad-build` (combine with `rust-best-practices` for the Rust code) |
| Review code or documents, walk through a change | `bmad-code-review`, `bmad-review`, `bmad-walkthrough` |
| Generate E2E/API tests, run a retrospective | `bmad-qa-generate-e2e-tests`, `bmad-retrospective` |
| Talk to a persona (PM, architect, dev, analyst, UX) | `bmad-agent-*`, or `bmad-party-mode` for several at once |

Only start a BMad workflow when the user asks for planning, specs, stories, reviews or
a named BMad skill. A plain "fix this bug" or "add this function" is just Rust work:
use `rust-best-practices` alone. BMad output goes to `_bmad-output/`; its config lives
in `_bmad/` and shouldn't be edited by hand (use `bmad-customize`).

## Conventions
- Skills, code, comments and docs in this repo are written in English. Answer the user
  in the language they write in.
