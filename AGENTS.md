<!-- bmad:context -->
<!-- Verified 2026-10-19 against 053b23b. Managed by bmad-project-context; edits inside this block are replaced on refresh. Keep anything you want preserved outside the markers. -->

## vesper-rs

Rust project, greenfield: no crate exists yet. Planning artifacts go to `_bmad-output/planning-artifacts/`, implementation artifacts to `_bmad-output/implementation-artifacts/`. Project-level skills live in `.claude/skills/`.

## Policy

- Write everything that lands in the repo in English: code, comments, docs, BMad artifacts, skills. Chat with the user in Turkish.
- Never hand-edit `_bmad/` (installer-managed); put overrides in `_bmad/custom/config.toml` or use the `bmad-customize` skill.
- Every change to vesper's public API updates `examples/project-template/` in the same commit: its code, its `AGENTS.md` and its vesper usage skill. Bot authors' agents learn vesper from that skill alone, never from vesper's source.

## Running and verifying

- Before writing, reviewing or refactoring any Rust (`.rs`, `Cargo.toml`, crate layout, crate choice), invoke the `rust-best-practices` skill and read only the `references/*.md` files the task touches.
- TODO (no `Cargo.toml` yet): before calling Rust work done, run `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test`; replace this line with a hook or CI check once the crate exists.

## Conventions that differ from defaults

- Start a BMad workflow (`bmad-*`) only when the user asks for planning, specs, stories, reviews or names a BMad skill; plain coding requests use `rust-best-practices` alone.
- When implementing a story with `bmad-build`, apply `rust-best-practices` to the Rust code it produces.
- Use `image-gen` only for raster images (pictures, icons, logos, mockups); write SVG, diagrams and charts as code instead.

## Known pitfalls

- The Codex CLI that `image-gen` needs does not survive a new cloud container; if `codex` is missing or logged out, ask the user before installing it or starting `codex login --device-auth`.

<!-- /bmad:context -->
