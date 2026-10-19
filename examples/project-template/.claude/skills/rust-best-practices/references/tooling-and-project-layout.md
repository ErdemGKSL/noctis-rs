# Tooling and Project Layout

How to lay out crates and workspaces, configure Cargo, lints and formatting, set up CI, and pick dependencies.

**Contents**
1. Crate layout: thin `main.rs`, logic in `lib.rs`
2. Modules
3. Workspaces
4. `Cargo.toml` essentials: edition, MSRV, metadata
5. Lints: `[lints]`, clippy, rustfmt
6. Feature flags
7. Profiles
8. CI pipeline
9. Choosing crates
10. Dependency hygiene
11. Pitfalls

---

## 1. Crate layout

```text
my-app/
├── Cargo.toml
├── Cargo.lock          # commit it (for binaries *and* libraries, per current Cargo guidance)
├── rustfmt.toml        # optional
├── src/
│   ├── main.rs         # thin: parse args, init logging, call into lib, map errors to exit code
│   ├── lib.rs          # the actual program, testable
│   ├── config.rs
│   └── commands/
│       ├── mod.rs      # or commands.rs next to a commands/ dir
│       └── serve.rs
├── tests/              # integration tests (public API only)
├── benches/            # criterion/divan benchmarks
└── examples/           # runnable examples: cargo run --example foo
```

**Why a thin `main.rs`:** binaries can't be imported by integration tests, benches or
other crates. Putting the logic in `lib.rs` makes all of it testable, and `main` stays a
small adapter between the OS (args, env, exit code) and your code.

```rust
// src/main.rs
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args = my_app::Args::parse();
    tracing_subscriber::fmt::init();
    my_app::run(args)
}
```

Returning `anyhow::Result<()>` (or `ExitCode`) from `main` prints the error chain and
exits non-zero, instead of panicking.

## 2. Modules

- Prefer the `foo.rs` + `foo/` style over `foo/mod.rs` for new code (many `mod.rs` tabs
  in an editor are hard to tell apart); both are fine — be consistent.
- Organize by **feature/domain** (`users/`, `billing/`) rather than by kind
  (`models/`, `services/`, `utils/`) once a crate grows. A `utils` module is a smell.
- Default to private; widen to `pub(crate)` for crate-internal sharing; `pub` only for
  the real API. Re-export the public surface from `lib.rs` (`pub use`) so users don't
  depend on your internal file structure.

## 3. Workspaces

Split into multiple crates when parts have different dependencies or audiences (core
logic vs. CLI vs. server), when compile times hurt (crates compile in parallel and
cache separately), or when you publish pieces independently. Don't split prematurely —
a well-moduled single crate is simpler.

```toml
# Cargo.toml (workspace root, "virtual manifest")
[workspace]
resolver = "3"                # default for edition 2024
members = ["crates/*"]

[workspace.package]
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
repository = "https://github.com/me/my-app"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
thiserror = "2"
my-core = { path = "crates/my-core" }

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
```

```toml
# crates/my-cli/Cargo.toml
[package]
name = "my-cli"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
my-core.workspace = true
tokio.workspace = true

[lints]
workspace = true
```

One version of each dependency, declared once, and the same lint policy everywhere.

## 4. `Cargo.toml` essentials

- **`edition = "2024"`** for new crates (needs Rust 1.85+). Migrate older crates with
  `cargo fix --edition`.
- **`rust-version`** declares your MSRV. Cargo refuses to build on older compilers with
  a clear message instead of a cryptic error, and the resolver can pick dependency
  versions compatible with it. Test it in CI (see §8).
- For published crates: `description`, `license`, `repository`, `readme`, `keywords`,
  `categories`, and `include`/`exclude` to keep the package small.
- Version requirements: `serde = "1"` means `^1` (any 1.x) — fine for libraries. Don't
  pin exact versions (`=1.2.3`) in libraries; it causes resolution conflicts downstream.

## 5. Lints

Configure lints in `Cargo.toml` (`[lints]`, Rust 1.74+) so they apply to every target and
every developer's editor, instead of scattering `#![warn(...)]` attributes.

A good starting point for most crates:

```toml
[lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"                 # libraries: document the public API
unused_qualifications = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }

[lints.clippy]
# Groups first, with lower priority so the individual entries below override them.
pedantic = { level = "warn", priority = -1 }
# Selected allows: pedantic lints that are often more noise than signal.
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"          # remove for published libraries
# Selected restriction lints worth enabling:
unwrap_used = "warn"                  # libraries: prefer `?` or `expect` with a reason
dbg_macro = "warn"
todo = "warn"
print_stdout = "warn"                 # libraries shouldn't print; use tracing
undocumented_unsafe_blocks = "warn"
```

Notes:

- `priority = -1` on groups is required so specific lints can override the group.
- `clippy::pedantic` has occasional false positives; allow them per item with a reason
  rather than disabling the group:
  `#[expect(clippy::cast_possible_truncation, reason = "value is < 2^16 by construction")]`.
  `#[expect]` (Rust 1.81+) warns if the lint stops firing, so stale allows don't pile up.
- `clippy::nursery` and `clippy::restriction` are not meant to be enabled wholesale;
  pick individual lints.
- `unwrap_used` is great for libraries; in binaries or tests you may relax it
  (`clippy.toml`: `allow-unwrap-in-tests = true`).

**`clippy.toml`** holds lint *parameters*:

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
msrv = "1.85"                  # usually inferred from rust-version
too-many-arguments-threshold = 6
```

**`rustfmt.toml`** — the defaults are the community standard; resist heavy
customization. A few stable, commonly used options:

```toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
use_try_shorthand = true
# Nightly-only but popular (run with `cargo +nightly fmt`):
# imports_granularity = "Crate"
# group_imports = "StdExternalCrate"
```

## 6. Feature flags

- Features are **additive**: enabling one must never break or remove functionality,
  because Cargo unifies features across the whole dependency graph.
- Keep `default` features minimal for libraries; let users opt into heavy deps.
- Make optional dependencies optional, and refer to them with `dep:` so they don't
  create implicit features:

```toml
[features]
default = []
json = ["dep:serde_json"]
cli = ["dep:clap"]

[dependencies]
serde_json = { version = "1", optional = true }
clap = { version = "4", features = ["derive"], optional = true }
```

```rust
#[cfg(feature = "json")]
pub fn to_json(v: &Report) -> serde_json::Result<String> { serde_json::to_string(v) }
```

- Test combinations in CI: `--no-default-features`, `--all-features`, or every
  combination with `cargo hack --feature-powerset`.
- Document features in the README / crate docs (`document-features` crate, or
  `#[cfg_attr(docsrs, doc(cfg(feature = "json")))]`).

## 7. Profiles

```toml
[profile.release]
lto = "thin"            # "fat" for max perf in final binaries
codegen-units = 1
strip = "symbols"       # smaller binary
# panic = "abort"       # smaller/faster; disables catch_unwind

[profile.dev]
debug = "line-tables-only"   # faster builds, still useful backtraces

[profile.dev.package."*"]
opt-level = 2                # fast dependencies in dev (big win for e.g. image/crypto crates)
```

Faster builds also come from: `cargo check` while iterating, a faster linker (`mold`,
or `lld`, which is the default on x86_64 Linux since Rust 1.90), fewer proc-macro-heavy
deps, and splitting crates.

## 8. CI pipeline

A solid CI job runs, in order of speed:

```yaml
# .github/workflows/ci.yml (essentials)
- run: cargo fmt --all --check
- run: cargo clippy --workspace --all-targets --all-features -- -D warnings
- run: cargo test --workspace --all-features         # or cargo nextest run + cargo test --doc
- run: cargo doc --workspace --no-deps --all-features
  env: { RUSTDOCFLAGS: "-D warnings" }               # broken intra-doc links fail the build
```

Plus, in separate jobs:

- **Security & licenses:** `cargo deny check` (advisories, licenses, banned and
  duplicate crates, allowed sources) — or at least `cargo audit`.
- **Unused dependencies:** `cargo machete` (fast, heuristic) or `cargo udeps` (nightly,
  precise).
- **MSRV:** `cargo +1.85 check --workspace` (or `cargo hack --rust-version check`)
  matching `rust-version`.
- **Features:** `cargo hack check --feature-powerset` for libraries with features.
- **Unsafe code:** `cargo +nightly miri test` if the crate has `unsafe`.
- **Semver (published libs):** `cargo semver-checks`.

Use `Swatinem/rust-cache` (or similar) to cache `target/` and the registry; pin the
toolchain with a `rust-toolchain.toml` if reproducibility matters.

Locally, `-D warnings` belongs in CI, not in `#![deny(warnings)]` in the source — a new
compiler release adding a lint would otherwise break users' builds.

## 9. Choosing crates

Well-established defaults by purpose. Prefer these unless the project already uses an
alternative.

| Purpose | Crate(s) | Notes |
| --- | --- | --- |
| Serialization | `serde` + `serde_json` / `toml` / `serde_yaml_ng` | derive feature; see `serde.md` |
| Library errors | `thiserror` | typed error enums |
| Application errors | `anyhow` (or `eyre`/`color-eyre`) | context + chains |
| Async runtime | `tokio` | enable only needed features |
| HTTP client | `reqwest` | `default-features = false, features = ["json", "rustls-tls"]` |
| HTTP server | `axum` | tower ecosystem; `actix-web` also mature |
| CLI parsing | `clap` (derive) | `#[derive(Parser)]`; `argh`/`lexopt` for tiny binaries |
| Logging / tracing | `tracing` + `tracing-subscriber` | structured, async-aware; not `println!` |
| SQL | `sqlx` (compile-time checked queries) | `diesel` / `sea-orm` as ORMs |
| Date/time | `jiff` (modern, tz-aware), `chrono`, `time` | pick one per project |
| IDs | `uuid` (`v4`, `v7` for sortable) | `ulid` alternative |
| Regex | `regex` | compile once (`LazyLock`) |
| Data parallelism | `rayon` | CPU-bound work |
| Iterator extras | `itertools` | `join`, `tuple_windows`, `sorted_by_key`… |
| Enum ↔ string | `strum` | `Display`, `EnumString`, `EnumIter` derives |
| Boilerplate derives | `derive_more` | `From`, `Display`, `Deref`, `Constructor`… |
| Discord bots | `serenity` / `poise`, `twilight` | |
| Testing | `insta`, `proptest`, `rstest`, `tempfile`, `wiremock` | see `testing.md` |
| Benchmarks | `criterion`, `divan` | see `performance.md` |
| Global statics | `std::sync::LazyLock` / `OnceLock` | replaces `lazy_static`, `once_cell` |

Before adding a less common crate, check: maintenance activity, downloads/reverse deps
on crates.io / lib.rs, `unsafe` usage, transitive dependency count (`cargo tree`), and
license.

## 10. Dependency hygiene

- **Every dependency is code you ship and must trust.** Prefer std when it's close
  enough; avoid crates for one-liners.
- **Turn off default features** you don't need — they often pull in large trees (TLS
  backends, compression, multiple runtimes):
  `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }`.
- Look for duplicates: `cargo tree -d`. Two major versions of the same crate double
  compile time and binary size.
- Keep `Cargo.lock` committed and update deliberately (`cargo update`, Dependabot /
  Renovate), with CI as the safety net.
- Use `cargo add` / `cargo remove` instead of hand-editing to keep formatting consistent.
- Don't re-export dependencies' types in your public API unless you mean to — it ties
  your semver to theirs.

## 11. Pitfalls

- **All logic in `main.rs`** — untestable; split into `lib.rs` early.
- **`#![deny(warnings)]` in source** — breaks on new compiler versions; enforce in CI.
- **Blanket `#![allow(clippy::all)]` or `#![allow(dead_code)]`** at the crate root to
  silence noise — hides real problems. Fix or allow narrowly with a reason.
- **Non-additive features** (`feature = "sqlite"` and `feature = "postgres"` that
  conflict) — use runtime config or separate crates instead.
- **Huge `utils.rs` / `helpers.rs`** — move functions next to the types they serve.
- **Unpinned toolchain in CI with `-D warnings`** — a new clippy release fails a
  previously green PR; pin, or accept and fix promptly.
- **Path dependencies without a version** in published crates — `cargo publish` rejects
  them; add `version = "…"` alongside `path`.
