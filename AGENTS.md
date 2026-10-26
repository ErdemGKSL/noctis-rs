<!-- bmad:context -->
<!-- Verified 2026-10-19 against 053b23b. Managed by bmad-project-context; edits inside this block are replaced on refresh. Keep anything you want preserved outside the markers. -->

## noctis-rs

Noctis: a Rust Discord bot framework on twilight-rs (tokio, built-in sharding, Components V2 UI kit, takumi image/GIF rendering). Crates live in `crates/`, repo tooling in `xtask/`. Published on crates.io as `noctis`, with `noctis-*` sub-crates enabled through its cargo features. Planning artifacts go to `_bmad-output/planning-artifacts/`, implementation artifacts to `_bmad-output/implementation-artifacts/`. Project-level skills live in `.claude/skills/`.

## Policy

- Write everything that lands in the repo in English: code, comments, docs, BMad artifacts, skills. Chat with the user in Turkish.
- Never hand-edit `_bmad/` (installer-managed); put overrides in `_bmad/custom/config.toml` or use the `bmad-customize` skill.
- Every change to noctis's public API updates `examples/project-template/` in the same commit: its code, its `AGENTS.md` and its noctis usage skill. Bot authors' agents learn noctis from that skill alone, never from noctis's source.
- A public API change also re-blesses `xtask/public-api.txt` (`cargo run --release -p xtask -- api-snapshot --bless`) and updates the Usage Skill's `API hash` line to the hash it prints, in the same commit.

## Where things are

- Architecture contract: `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md`; follow its ADs (crate layers, `custom_id` format, ports) when adding or moving code.
- Maintainer decisions: `_bmad-output/planning-artifacts/maintainer-decisions.md`; binding for planning, implementation and review, and it wins over any older spec or story text that contradicts it.
- Requirements: `_bmad-output/planning-artifacts/prds/prd-noctis-rs-2026-10-19/prd.md` (FR/NFR ids).
- Discord rules and where each is enforced: `docs/discord-rules.md`; a new or changed limit updates this file and `noctis-rules` together, using the same rule id.

## Running and verifying

- Before writing, reviewing or refactoring any Rust (`.rs`, `Cargo.toml`, crate layout, crate choice), invoke the `rust-best-practices` skill and read only the `references/*.md` files the task touches.
- Before calling Rust work done, run `cargo fmt --all`, `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`, `cargo test --release --workspace` and `cargo hack test --release --workspace --each-feature` (cargo-hack 0.6), `cargo run --release -p xtask -- deps-check`, `cargo run --release -p xtask -- tw-grammar --check`, `cargo test --release -p noctis-image --features render --test tw_crosscheck`, `cargo run --release -p xtask -- rules-catalog`, `cargo run --release -p xtask -- api-snapshot`, `cargo run --release -p xtask -- skill-check`, `cargo run --release -p xtask -- docs-check` and `cargo run --release -p xtask -- release-check`, the scripts `bash .github/scripts/check-default-graph.sh` and `bash .github/scripts/release-retry-test.sh`, the token-less publish dry run `cargo publish --workspace --dry-run --locked --allow-dirty`, and build the facade's rustdoc with `RUSTDOCFLAGS="-D warnings" cargo doc --release -p noctis --no-deps` and the same with `--all-features`; also run `cargo deny check` (cargo-deny 0.20) and the MSRV check `cargo +1.91 check --release --workspace`. The project template is gated too, from the repo root with `--manifest-path examples/project-template/Cargo.toml` on each: `cargo fmt --check`, `cargo build --release --locked`, `cargo clippy --release --locked --all-targets -- -D warnings`, the same clippy with `--all-features`, `CI=true cargo test --release --locked` and the same test with `--all-features` (`CI=true` fails on a missing or changed snapshot instead of writing it). In the story loop, `cargo hack` runs only in the gates of the last story of each epic (maintainer decision, 2026-10-23, reaffirmed 2026-10-24; `tools/story-loop/gates.sh`, `epic-last.py`); agents never run it themselves. The CI job definitions live in `.github/workflows/` (`release.yml` in `.github/workflows-disabled/` until Story 14.8); keep them in sync when a story adds or changes a gate.
- Prefer `--release` for building, testing, checking and running (`cargo build --release`, `cargo test --release`, `cargo clippy --release`, `cargo run --release`). Debug builds are allowed (maintainer, 2026-10-24), but never incremental ones: keep `CARGO_INCREMENTAL=0` and `incremental = false`, because disk space in cloud sessions is limited. If the disk fills, delete `target/debug/` first.
- trybuild (UI tests) never passes `--release`; it is covered by the repo-root `.cargo/config.toml` (created by Story 1.1), whose `[profile.dev]` is release-equivalent (`opt-level = 3`, no debug info, debug assertions, overflow checks or incremental). Never weaken that file. trybuild suites are `compile_fail` only.

## Conventions that differ from defaults

- Start a BMad workflow (`bmad-*`) only when the user asks for planning, specs, stories, reviews or names a BMad skill; plain coding requests use `rust-best-practices` alone.
- When implementing a story with `bmad-build`, apply `rust-best-practices` to the Rust code it produces.
- Use `image-gen` only for raster images (pictures, icons, logos, mockups); write SVG, diagrams and charts as code instead.

## Known pitfalls

- A new cloud container lacks `nightly-2026-10-15` and `cargo-public-api` 0.52.0, which `api-snapshot` needs; install them with `rustup toolchain install nightly-2026-10-15 --profile minimal && cargo install cargo-public-api --version 0.52.0 --locked`.
- The story loop no longer uses a shared cargo queue: each lane runs on its own machine and its agents run cargo themselves (`tools/story-loop/README.md`, "Cargo"). If `~/.cargo/bin/cargo` is still the old queue shim, restore it with `ln -sf rustup ~/.cargo/bin/cargo`.
- The Codex CLI that `image-gen` needs does not survive a new cloud container; if `codex` is missing or logged out, ask the user before installing it or starting `codex login --device-auth`.

<!-- /bmad:context -->

## Maintainer workflow

These are the maintainer's standing preferences for agent sessions on this repo.

- Push every commit to `main` immediately and automatically (`git push origin HEAD:main`), together with the session branch if there is one; no PRs and no waiting for approval.
- Commit and push after every finished step, including `_bmad-output/`: cloud containers are ephemeral and anything unpushed is lost.
- Every 3-4 merged stories and at the end of each epic (maintainer, 2026-10-25; replaces "every third epic"), refresh `README.md` on main: status note, stories-done badge, epic status table, "What already works" bullets, and API-preview names that no longer match the real API. The story loop does it (`tools/story-loop/readme-due.py`).
- CI is back on (2026-10-26): `ci.yml` and `main.yml` run from `.github/workflows/`; keep `ci.yml` green on `main`. `release.yml` stays in `.github/workflows-disabled/` until the maintainer finishes Story 14.8 (runbook `docs/guides/release.md`, step 1). A story is done when every local gate passes and CI on `main` is green.
- Stories marked `[Maintainer-gated]` in `epics.md` (crates.io token or publish, repository settings) need the maintainer; skip them in unattended runs, report them as waiting, and continue.
- Work autonomously. The maintainer has delegated every decision, including architecture and feature-set questions, until the project is finished: decide, log each decision as a `(decision)` line in the relevant memlog, and continue.
- Delegate research on unfamiliar frameworks, large-document extraction and independent reviews to subagents, and keep only their conclusions in the main context.
- Verify crate versions and that a technology still exists against crates.io, docs.rs or the official source before relying on them, never from memory.
- Never send the maintainer's personal data (such as their email) to external services, including in HTTP headers like `User-Agent`.
- Every commit names the maintainer as co-author: `Co-Authored-By: Erdem <74366943+ErdemGKSL@users.noreply.github.com>` (`tools/story-loop/cp.sh` adds it).
- Stories are implemented by the unattended story loop in `tools/story-loop/` (setup and run steps in its `README.md`); commit only finished steps, never WIP.
