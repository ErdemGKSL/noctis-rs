---
title: 'Story 1.1: Workspace skeleton and baseline CI'
type: 'chore'
created: '2026-10-19'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The repo has planning docs, `docs/` and `examples/project-template/` but no Cargo workspace, toolchain pin, supply-chain policy or CI, so no later story has a crate to land in or a gate to pass (Story 1.1, Epic 1; FR-48, FR-49 CI job, NFR-7, NFR-8; AD-2, AD-28, Stack, Structural Seed).

**Approach:** Create the virtual workspace exactly as the spine's Structural Seed: 12 empty, documented library crates plus `xtask`, the pinned toolchain, the release-equivalent dev profile, `deny.toml`, `release-plz.toml`, `docs/guides/`, and `ci.yml` with every baseline gate (plus `main.yml`/`release.yml` stubs); then replace the AGENTS.md TODO line with the real commands.

## Boundaries & Constraints

**Always:**
- Every cargo command (local and CI) uses `--release`; never create `target/debug/`.
- Crates inherit `version`, `edition`, `rust-version`, `license`, `repository` from `[workspace.package]` and use `[lints] workspace = true`.
- Every sub-crate's crate doc (`//!`) contains the exact phrase "internal; use `noctis`" and one line on its role (from the Structural Seed); every `Cargo.toml` `description` ends with "Internal; use `noctis`." (facade excepted).
- Versions are exactly those listed below (verified on crates.io 2026-10-19).
- `Cargo.lock` committed; root `.gitignore` contains `/target`.

**Never:**
- No internal crate edges, no external dependencies in any crate, no real API code (each later story adds its own; deps-check is Story 1.2).
- No `[profile.*]` in root `Cargo.toml`; no `[profile.release]` in `.cargo/config.toml`; never weaken the dev profile.
- No `xtask` subcommands yet (deps-check etc. arrive in Stories 1.2, 1.7, 1.8).
- Do not edit `examples/project-template/` (no public item is added; its `Cargo.toml` already names the facade features) or `_bmad/`.
- No license beyond the allowlist below.

</intent-contract>

## Code Map

Everything is new except the two edits marked (edit).
- `Cargo.toml` -- virtual workspace root (members, exclude, package, deps, lints).
- `Cargo.lock`, `.gitignore`, `clippy.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `deny.toml`, `release-plz.toml` -- root policy files.
- `crates/<name>/{Cargo.toml,src/lib.rs}` for `noctis, noctis-rules, noctis-codec, noctis-ui, noctis-ports, noctis-gateway, noctis-core, noctis-macros, noctis-i18n, noctis-image, noctis-prefix, noctis-testkit`.
- `xtask/{Cargo.toml,src/main.rs}` -- repo tooling binary, `publish = false`.
- `docs/guides/README.md` -- placeholder so the dir exists (FR-74 guides: concept map, Cluster deployment).
- `.github/scripts/check-build-config.sh` -- asserts toolchain/dev-profile values.
- `.github/workflows/{ci.yml,main.yml,release.yml}`.
- `AGENTS.md` (edit) -- "Running and verifying" TODO line and the "Greenfield: no crate exists yet" sentence.
- `examples/project-template/Cargo.toml` (read-only) -- own empty `[workspace]`, path-depends on `../../crates/noctis` with features `image, default-font, i18n, testkit`; these feature names must exist.
- Spine `ARCHITECTURE-SPINE.md` §Stack (l.1106), §Structural Seed (l.1143), AD-28 (l.994), AD-21 TLS features (l.723) -- read-only source of the values below.

## Tasks & Acceptance

**Execution:**
- `Cargo.toml` -- `[workspace] resolver = "3"`, `members = ["crates/noctis", …all 12…, "xtask"]`, `exclude = ["examples/project-template"]`. `[workspace.package] version = "0.1.0"`, `edition = "2024"`, `rust-version = "1.91"`, `license = "MIT OR Apache-2.0"`, `repository = "https://github.com/ErdemGKSL/noctis-rs"`. `[workspace.dependencies]`: the 12 internal crates as `{ path = "crates/<name>", version = "0.1.0" }`; and the Stack: `tokio = "1.53"`, `twilight-gateway = { version = "0.17.1", default-features = false, features = ["rustls-platform-verifier", "twilight-http", "zstd"] }`, `twilight-http = { version = "0.17.1", default-features = false, features = ["rustls-platform-verifier", "decompression"] }`, `twilight-model`, `twilight-cache-inmemory`, `twilight-http-ratelimiting` = `"0.17.1"`, `rustls = { version = "0.23.46", default-features = false, features = ["ring", "std", "tls12", "logging"] }`, `takumi = { version = "=2.17.0", default-features = false }`, `takumi-core = { version = "=0.31.0", default-features = false }`, `serde = { version = "1.0", features = ["derive"] }`, `serde_json = "1.0"`, `postcard = "1.1"`, `thiserror = "2.0"`, `futures-util = "0.3"`, `tracing = "0.1.44"`, `metrics = "0.24"`, `arc-swap = "1.9"`, `moka = "0.12"`, `rayon = "1.12"`, `blake3 = "1.8"`, `bytes = "1.12"`, `arrayvec = "0.7"`, `getrandom = "0.4"`, `regex = "1.13"`, `syn = "3.0"`, `quote = "1.0"`, `proc-macro2 = "1.0"`, `trybuild = "1.0"`, `insta = "1.49"`, `proptest = "1.12"`, `hyper = "1.12"`, `hyper-util = "0.1.21"`, `http-body-util = "0.1.5"`, `tokio-websockets = "0.13"`, `yaml-rust2 = "0.13"`, `cssparser = "0.38"`, `criterion = "0.8"`, `dhat = "0.3"`, `png = "0.18"`, `toml = "1.1"`, `pulldown-cmark = "0.13"`, `cargo_metadata = "0.23"`. `[workspace.lints.rust]`: `unsafe_code = "deny"`, `missing_docs = "warn"`, `unused_qualifications = "warn"`, `rust_2018_idioms = { level = "warn", priority = -1 }`. `[workspace.lints.clippy]`: `pedantic = { level = "warn", priority = -1 }`, `must_use_candidate = "allow"`, and `"warn"` for `unwrap_used, expect_used, dbg_macro, todo, print_stdout, undocumented_unsafe_blocks`. No `[profile.*]`.
- `clippy.toml` -- `allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`.
- `rust-toolchain.toml` -- `[toolchain] channel = "1.97.0"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`.
- `.cargo/config.toml` -- only `[profile.dev]` with the five lines `opt-level = 3`, `debug = false`, `debug-assertions = false`, `overflow-checks = false`, `incremental = false`, plus a comment: release-equivalent so trybuild's non-`--release` runs stay release-grade (AD-28); never weaken.
- `crates/*/Cargo.toml` + `src/lib.rs` -- 11 sub-crates as plain `lib`s (no deps). `noctis-macros`: `[lib] proc-macro = true`. `noctis-image`: `[features] default = []`, `render = []` (comment: adds takumi and the core/ports/ui edges in Epic 10), and `pub mod tw {}` with a doc comment (strict Tailwind class grammar, takumi-free, Story 10.1).
- `crates/noctis/` -- facade; `readme = "../../README.md"`; `[features] default = []` and empty `image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `prefix`, `testkit` (each with a one-line comment; `testkit` for `[dev-dependencies]` only). `lib.rs`: `#![deny(missing_docs)]` and crate docs listing the features (image off by default).
- `xtask/` -- bin, `publish = false`, no deps; `main` returns `std::process::ExitCode`: no args or `help` prints usage to stderr (no tasks yet) and exits 0, any other arg prints `unknown task: <arg>` to stderr and exits 2.
- `deny.toml` (cargo-deny 0.20 schema) -- `[graph] targets` = the 8 Rust 1.97 tier-1 targets: `aarch64-apple-darwin`, `aarch64-pc-windows-msvc`, `aarch64-unknown-linux-gnu`, `i686-pc-windows-msvc`, `i686-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` (comment: keeps wasm-only `webpki-root-certs`, CDLA-Permissive-2.0, out). `[licenses] allow` exactly `MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, CC0-1.0` (comments: MPL-2.0 for cssparser, maintainer-approved; a new license needs maintainer approval); `exceptions = [{ crate = "xxhash-rust", allow = ["BSL-1.0"] }, { crate = "jpeg-encoder", allow = ["IJG"] }]` (via takumi); `unused-allowed-license = "allow"`, `unused-license-exception = "allow"`; `[licenses.private] ignore = true`. `[bans] multiple-versions = "warn"`, `deny = [{ crate = "aws-lc-rs", reason = "AD-21: ring only" }, { crate = "aws-lc-sys", reason = "AD-21: ring only" }]`. `[advisories]` defaults. `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`.
- `release-plz.toml` -- `[workspace]` defaults; `[[package]] name = "xtask"`, `release = false`; comment that Story 14.7 completes release config.
- `.github/scripts/check-build-config.sh` -- `set -euo pipefail`; `grep -qxF` each required line (`channel = "1.97.0"`, `components = ["rustfmt", "clippy"]` in `rust-toolchain.toml`; `[profile.dev]` and the five dev lines in `.cargo/config.toml`); print the missing line/file and exit 1 on any miss. Executable bit set.
- `.github/workflows/ci.yml` -- `on: push: branches: [main]` and `pull_request`; `permissions: contents: read`; `env: CARGO_TERM_COLOR: always`. Each job: `actions/checkout@v7`, `run: rustup toolchain install` (installs from `rust-toolchain.toml`), `Swatinem/rust-cache@v2`. Jobs (all blocking unless noted): `build-config` (`bash .github/scripts/check-build-config.sh`); `fmt` (`cargo fmt --all --check`); `clippy` (`cargo clippy --release --workspace --all-targets --all-features -- -D warnings`); `test` (`cargo test --release --workspace`); `each-feature` (`taiki-e/install-action@v2` with `tool: cargo-hack@0.6`, then `cargo hack test --release --workspace --each-feature`); `all-features` (`cargo build --release --workspace --all-features`); `deny` (install `cargo-deny@0.20`, `cargo deny check`); `msrv` (`rustup toolchain install 1.91 --profile minimal`, `cargo +1.91 check --release --workspace`); `template-build` (`continue-on-error: true`, comment: tracking mode until Story 14.2; `working-directory: examples/project-template`, `cargo build --release`).
- `.github/workflows/main.yml` -- stub, `on: workflow_dispatch`, jobs `benches` (comment: Stories 10.11, 12.6, 14.3 against `budgets.toml`) and `latest-takumi` (Story 10.11), each one `echo` step.
- `.github/workflows/release.yml` -- stub, `on: workflow_dispatch`, job `release-plz` (comment: Story 14.7, protected environment per Story 14.8) with one `echo` step.
- `docs/guides/README.md` -- one paragraph: FR-74 guides (concept map, Cluster deployment) land here.
- `AGENTS.md` -- replace the TODO bullet with: "Before calling Rust work done, run `cargo fmt --all`, `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`, `cargo test --release --workspace` and `cargo hack test --release --workspace --each-feature` (cargo-hack 0.6); CI (`.github/workflows/ci.yml`) runs the same gates plus `cargo deny check` and the MSRV check." Change "Greenfield: no crate exists yet." to "Crates live in `crates/`, repo tooling in `xtask/`." Keep the `.cargo/config.toml` bullet unchanged.
- `Cargo.lock` -- generate with `cargo generate-lockfile` (or any `--release` build) and commit.

**Acceptance Criteria:**
- Given the finished tree, when `cargo metadata --format-version 1 --no-deps` runs, then `workspace_members` holds exactly the 12 crates plus `xtask`, none under `examples/project-template`, and every package has version `0.1.0`, edition `2024`, rust-version `1.91`, license `MIT OR Apache-2.0`; `noctis-macros` has a `proc-macro` target; `xtask` has `publish == []`; `noctis` has exactly the features `default, image, image-gif, image-emoji, default-font, i18n, prefix, testkit` and `noctis-image` exactly `default, render`, all empty.
- Given the tree, when `cargo build --release --workspace` and `cargo hack test --release --workspace --each-feature` run, then both succeed and `target/debug/` does not exist.
- Given the tree, when `cargo doc` output or `src/lib.rs` of each sub-crate is read, then its crate doc contains "internal; use `noctis`", and `crates/noctis/src/lib.rs` contains `#![deny(missing_docs)]`.
- Given `.cargo/config.toml` with any one of the five dev lines removed or changed, when `check-build-config.sh` runs, then it exits non-zero naming the missing line; with the files intact it exits 0.
- Given the tree, when `cargo deny check` runs, then it passes (advisories, bans, licenses, sources).
- Given `cargo run --release -p xtask -- frobnicate`, then exit code is 2 and stderr names the task; with no args exit code is 0.
- Given a push to `main`, when `ci.yml` runs, then jobs build-config, fmt, clippy, test, each-feature, all-features, deny and msrv pass and the run concludes `success` while template-build is allowed to fail.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (also logged in the architecture `.memlog.md`): workspace starts at `0.1.0`; no internal edges or external deps yet, so `Cargo.lock` lists only workspace crates and deny trivially resolves (unused-license settings are `allow` because the allowlist and takumi exceptions are policy for later crates); only AD-21's TLS crates and takumi set `default-features = false` at workspace level so members opt into features; `unsafe_code = "deny"` rather than `forbid` so a later justified `// SAFETY:` exception is possible; stubs use `workflow_dispatch` only, and filling stories add triggers. "`[profile.release]` stays the real release profile" is read as: no profile overrides anywhere except `.cargo/config.toml`'s `[profile.dev]`. CLI tools are version-pinned in CI (`cargo-hack@0.6`, `cargo-deny@0.20`), not in `Cargo.toml`. `missing_docs` applies to every crate via workspace lints; give `xtask/src/main.rs` a `//!` doc too. `rustup toolchain install` without arguments needs rustup >= 1.28 (current on GitHub runners); if a CI run shows otherwise, use `rustup toolchain install 1.97.0 --profile minimal -c rustfmt,clippy`.

## Verification

**Commands** (repo root; install tools once if missing: `cargo install --locked cargo-hack@0.6.45 cargo-deny@0.20.2`; `rustup toolchain install 1.91 --profile minimal`):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0, no warnings.
- `cargo test --release --workspace` -- exit 0.
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo build --release --workspace --all-features` -- exit 0.
- `cargo +1.91 check --release --workspace` -- exit 0.
- `cargo deny check` -- exit 0.
- `bash .github/scripts/check-build-config.sh` -- exit 0.
- `cargo metadata --format-version 1 --no-deps | jq -r '.workspace_members[]'` -- 13 entries, none from `examples/project-template`.
- `test ! -d target/debug` -- true.
- After the main session pushes: `gh run list --branch main --workflow ci.yml --limit 1` -- conclusion `success`.

**Manual checks:**
- `(cd examples/project-template && cargo build --release)` may fail (template uses unlanded API); only confirm it fails in compilation, not in resolving `noctis` features.
