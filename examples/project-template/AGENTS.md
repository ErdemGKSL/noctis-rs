<!-- bmad:context -->
<!-- Managed block. Ships with the Noctis project template; edits inside this block are replaced when the template is re-synced. Keep anything you want preserved outside the markers. -->

## noctis-bot-template

A Discord bot built on Noctis (Rust, tokio, twilight). It is a copy of the Noctis
project template: `/setup start` (Wizard), `/dashboard` (Navbar + Paginator),
`/profile` and "Show profile" (rendered card in a Media Gallery, bio modal), a custom
`CacheStore` Adapter and `en-US`/`tr` Locale files.

## Policy

- Learn Noctis **only** from `.claude/skills/noctis/` (the Noctis Usage Skill). Load it before writing or reviewing any bot code: commands, Routes, Views, modals, Templates, Adapters, i18n, tests.
- Never read Noctis source: not `../../crates/noctis*`, not `~/.cargo/registry/**/noctis*`, not `~/.cargo/git/**/noctis*`, not docs.rs source views, not `target/` expansions. If the skill does not cover something, say so and stop on that point; do not guess and do not open the source.
- For general Rust (types, errors, async, tests, crate layout), use the `rust-best-practices` skill and read only the `references/*.md` files the task touches.
- Follow the UI Doctrine in the skill (UD-1..UD-8): Components V2 Views for anything interactive, images only for read-only visuals, option-light commands that open Views, no per-message listeners or collectors.

## Where things are

- `src/main.rs` thin entry; `src/lib.rs` `configure()` mounts Modules, Adapters and state (shared by main and tests).
- `src/modules/{setup,dashboard,profile}.rs` one Module each; `src/adapters/` custom Adapters; `src/strings.rs` compiles `locales/*.yaml` (`en-US` is the default locale; file names are Discord locale codes).
- `noctis-theme.css` Theme tokens and keyframes for Templates.
- `tests/` flows (testkit), Adapter conformance, Template pixel snapshots.

## Running and verifying

- Before calling Rust work done, run `cargo fmt`, `cargo clippy --release --all-targets -- -D warnings` and `cargo test --release`.
- Always build, test, check and run with `--release`; never create a debug build (disk space in cloud sessions is limited). If the disk fills anyway, delete `target/debug/`.
- Run locally with `NOCTIS_TOKEN` set: `cargo run --release -- --publish-guild <test guild id>` once, then `cargo run --release`. See `README.md`.
- Compile errors and `BuildError`s name a Discord rule id such as `[CID-01]` or `[CMD-11]`. Look the id up in `.claude/skills/noctis/references/limits-cheatsheet.md` (and `troubleshooting.md`); fix the cause the error's `help:` line names instead of working around the check.

## Conventions that differ from defaults

- Never hand-build a `custom_id`, response type or interaction callback; build components from Route values and return Views.
- Every user-visible string goes through `locales/*.yaml`; add new keys to `en-US.yaml` first.
- Limits and Locales come from `noctis::rules` (`Locale::EnUs` = `en-US`); override limits on the App builder with `Limits`, never with hard-coded numbers.
- Dynamically built Command Patterns are checked with `noctis::rules::command::{expand, validate_tree}` before registering; never hand-split patterns. Dynamically declared options go through `noctis::rules::command::validate_options` and `measure_options` (as `with_options(opts.len(), measure_options(&opts, &limits))`) before registering.
- Dynamic component trees, modals, attachments and webhook names can be pre-checked with `noctis::rules::{component, modal, attachment, message}` (`validate_components`, `validate_modal`, `validate_attachments`, `validate_webhook_name`, …) and autocomplete responses with `noctis::rules::command::validate_autocomplete`; never hard-code their limits.
- Custom Payload field types implement `noctis::codec::{Field, FieldDecode}` and are tested with `noctis::codec::check::{assert_round_trip, assert_worst_case}` (see `references/routes-payloads-refs.md` §2); never hand-escape field text. Only `Option` fields may be appended to a Payload without a schema bump, since missing trailing fields decode as `None` through `FieldDecode::absent`; never override `absent` on your own types.
- Inside the noctis-rs monorepo this crate is standalone (`[workspace]` in its `Cargo.toml`); the monorepo root `Cargo.toml` must keep `exclude = ["examples/project-template"]`. After copying it out, switch the `noctis` dependency to the crates.io form shown in `Cargo.toml`.

<!-- /bmad:context -->
