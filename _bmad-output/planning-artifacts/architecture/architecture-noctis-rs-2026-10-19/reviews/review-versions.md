# Review: versions and reality check

**Spine:** `../ARCHITECTURE-SPINE.md` (draft, 2026-10-19)
**Lens:** check that every committed decision was web-researched or tested, not asserted from training data. That covers current versions, whether each named technology still exists, and whether it fits.
**Date:** 2026-10-19. **Method:**
- Queried the crates.io API for every Stack row and named tool, with a User-Agent header.
- Downloaded and read the sources of `takumi 2.17.0`, `takumi-core 0.31.0`, `futures-util 0.3.34` and `cargo-public-api 0.52.0`.
- Compiled and ran a scratch project to test three claims: the Tailwind check, `catch_unwind` on a boxed future, and the CMD-01 regex. It used rustc 1.97.0 and edition 2024, and lives at `scratchpad/verify/`, which is temporary.

## Verdict

**Pass with two blocking findings.** Every Stack row names a version that exists on crates.io and is the current release line. The project's research files already confirmed twilight and takumi from source. Two committed rules don't hold up against the real crates:

- **AD-15:** takumi's Tailwind parser can't validate classes in the way the rule requires.
- **AD-3:** `unicode-ident` can't implement the CMD-01 name rule.

There are also four smaller gaps that the spine doesn't mention.

## Findings

### F1 (high): AD-15 "validate literal classes with takumi's own Rust parser" doesn't do what the rule promises

- The per-token parsers are `pub(crate)`: `TailwindValue::parse` and `TailwindProperty::parse` in `takumi-core/src/style/tw/mod.rs`. The research note `research/takumi-image-generation.md:247` suggests calling `TailwindValue::parse` from the proc macro, but that function can't be called from outside the crate.
- The only public entry point is `TailwindValues: FromStr`. It is re-exported stably as `takumi::prelude::TailwindValues`, and as `takumi_core::style::TailwindValues`. It never returns an error: it calls `filter_map(TailwindValue::parse)` and silently drops any token it doesn't know.
- A workaround exists and works: parse each token alone, then compare the result with `"".parse()` (`PartialEq` compares the parsed vector).
- **It still doesn't catch the case AD-15 exists to prevent.** Results from the test run:

  | token | does takumi accept it? |
  | --- | --- |
  | `bg-gradient-to-r` (the v3 trap named in the research) | **accepted** |
  | `bg-foo-bar`, `text-whatever`, `p-banana`, `rounded-qq` | **accepted** |
  | `nonsense-xyz` | rejected |

  Any `<known-prefix>-<word>` parses as a reference to a theme variable, such as `--color-foo-bar` or `--spacing-banana`, so it passes validation. Then it renders as nothing. That is exactly what AD-15's "classes that pass validation but render differently" says the rule prevents.
- Exact parity with takumi also means `noctis-macros` depends on `takumi-core`. That crate is semver-unstable (0.x, now at 0.31, with a minor bump almost every release), and pulls in a heavy host-side build: icu_*, image, skrifa, harfrust, taffy, tiny-skia, selectors and cssparser.
- `takumi::unstable::base` exists, but it is documented as having "no semver guarantee".
- **Fix:** the AD-15 rule needs to say three things:
  - which crate and path the macro uses (`takumi::prelude::TailwindValues`, or `takumi-core` pinned to the same minor as takumi);
  - what it checks: "known utility prefix", plus a Noctis-side check that the theme-variable token exists in the default theme or the declared `@theme`;
  - that this residual gap is known.
  The alternative is a generated allow-list, which is research open question 2. Narrow the "Prevents" bullet to what the chosen check can actually catch.

### F2 (high): AD-3 "only dependency … is `unicode-ident`" can't implement CMD-01

- CMD-01 (`docs/discord-rules.md:64`) is `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$`. The rules catalog itself says the check is "compiled with `regex` + Unicode script tables".
- `unicode-ident` exposes only `is_xid_start` and `is_xid_continue`. Those don't cover:
  - general categories `L` and `N`, as separate sets;
  - the scripts Devanagari and Thai, which include combining marks that aren't letters;
  - `-` and `ʼ`.
- I confirmed that the `regex` crate (1.13) matches Devanagari and Thai words with `\p{sc=Deva}` and `\p{sc=Thai}`, and rejects ASCII `'` and spaces.
- **Fix:** let `noctis-rules` depend on `regex` (or `regex-syntax` / `regex-automata` with Unicode tables), or on a Unicode general-category and script crate. Then make AD-3 and `docs/discord-rules.md` agree.

### F3 (medium): takumi pin is less than one day old, and the "exact" pin is ambiguous

- `takumi 2.17.0` was published 2026-10-19 at 11:05 UTC, which is the spine's own date.
- takumi releases very often. There were seven releases between 2.15.2 and 2.17.0 in six days (Oct 13 to Oct 19), and `takumi-core` moved to 0.31.0 the same day.
- The spine says "2.17.0 (pinned exact)", but the research's Cargo snippet uses `=2.17` (`takumi-image-generation.md:202`). That is a different requirement: it allows any 2.17.x.
- An exact pin in a library also blocks every bot author from taking takumi patch fixes, and conflicts with any other crate in their tree that needs `takumi ^2.18`.
- **Fix:**
  - State one requirement. `~2.17` / `=2.17` is suggested, so patches are allowed.
  - If F1 keeps `takumi-core`, pin it to the same minor.
  - Add a CI job that runs against the latest takumi, so the pin is bumped on purpose.

### F4 (medium): `cargo public-api` needs a nightly toolchain, and the spine doesn't say so

- `cargo-public-api 0.52.0` (current, 2026-05-25) builds rustdoc JSON. Its README says you must install **nightly-2025-08-02 or later**, which doesn't have to be the active toolchain.
- AD-17 (`xtask api-snapshot`) and the CI line in the Structural Seed (`public-api`) assume a stable-only CI.
- The tool's output can also change between nightlies and `public-api` versions. If the nightly floats, the snapshot hash that `skill-check` compares can churn for reasons that have nothing to do with Noctis.
- **Fix:** in AD-17 or the CI convention, pin `cargo-public-api` and a dated nightly together, for example in `rust-toolchain` for that job or in an `xtask` constant. Alternatively, use the `public-api` library crate (0.52.2) from `xtask` with `rustdoc-json`.

### F5 (low): MSRV 1.91 is correct but has no CI job

- The MSRV is set by takumi 2.17.0 (`rust-version = 1.91`). Other floors are twilight 0.17.1 at 1.89, and trybuild 1.0.122 and proptest 1.12 at 1.88. Edition 2024 needs 1.85. So 1.91 is right.
- No CI step builds on 1.91, and takumi releases could raise its MSRV within the pin.
- **Fix:** add a `cargo +1.91 check` job to the CI line.

### F6 (low): twilight 0.17.1 is current but stale upstream

- 0.17.1 (2025-12-13) is still the newest release on crates.io for all five crates the spine lists.
- `main` has more than 76 unreleased commits, including breaking ones. The project's twilight research recorded this, and the spine already lists "twilight 0.18 migration" under Deferred.
- One thing should also be deferred, or tied to AD-12: twilight 0.18 adds Checkbox and CheckboxGroup to twilight-model. AD-12's reason for owning the V2 model ("Radio Group and Checkbox are missing") will then be only partly true. The decision still stands, because of Radio Group and the drop-unknown behaviour, but the stated reason will go out of date.

## Checked and confirmed (no action)

| Item | Spine | crates.io now (2026-10-19) | Notes |
| --- | --- | --- | --- |
| Rust MSRV / edition | 1.91 / 2024 | — | matches takumi's `rust-version` (F5) |
| tokio | 1.53 | 1.53.3 (Oct 13) | MSRV 1.71. An LTS line 1.51.x is also maintained; consider stating that Noctis follows the latest line, not LTS. |
| twilight-gateway / http / model / cache-inmemory / http-ratelimiting | 0.17.1 | 0.17.1 (2025-12-13) | `rust-version` 1.89; verified in the research from source and compiled code |
| takumi | 2.17.0 | 2.17.0 (2026-10-19) | see F1, F3 |
| serde / serde_json | 1.0 | 1.0.229 / 1.0.154 | |
| postcard | 1.1 | 1.1.3 | |
| thiserror | 2.0 | 2.0.21 | `thiserror-impl` uses syn 3 |
| tracing / metrics | 0.1.44 / 0.24 | 0.1.44 / 0.24.6 | |
| arc-swap | 1.9 | 1.9.3 (2026-10-18) | |
| moka | 0.12 | 0.12.16 | the async cache needs moka's `future` feature |
| rayon | 1.12 | 1.12.0 | matches takumi's `rayon ^1.12` |
| blake3 | 1.8 | 1.8.7 | |
| bytes | 1.12 | 1.12.1 | |
| syn / quote | 3.0 / 1.0 | 3.0.9 / 1.0.47 | syn 3.0.0 came out 2026-07-18. syn 3 depends on `quote ^1.0.35`. The ecosystem has moved: `darling_core 0.24`, `serde_derive`, `thiserror-impl` and `tokio-macros` all use syn 3, so the proc-macro tree won't build two copies of syn. |
| trybuild / insta / proptest | 1.0 / 1.49 / 1.12 | 1.0.122 / 1.49.0 / 1.12.0 | |
| unicode-ident | named in AD-3 | 1.0.27 | exists, but is the wrong tool (F2) |
| cargo-public-api | named in AD-17 | 0.52.0 (+ `public-api` 0.52.2) | needs nightly (F4) |
| `BoxFuture` (AD-14) | not sourced | `futures-util 0.3.34` / `futures-core` | `futures_util::future::BoxFuture<'a, T>` = `Pin<Box<dyn Future<Output = T> + Send + 'a>>`. **Add `futures-util` to the Stack.** It is already a transitive dependency via moka. |
| `catch_unwind` on futures (AD-13) | not sourced | `futures_util::FutureExt::catch_unwind` (feature `std`, on by default) | Confirmed by running it: `AssertUnwindSafe(boxed_fut).catch_unwind().await` returns `Err` on a panic. The `AssertUnwindSafe` wrapper is required because `BoxFuture` isn't `UnwindSafe`; state this in AD-13. A panic inside a task spawned with `tokio::spawn` also surfaces as `JoinError::is_panic`. |
| crate names `noctis`, `noctis-core`, `noctis-macros`, `noctis-ui` | — | all free on crates.io | |

## Suggested Stack additions

- `futures-util` 0.3 (`BoxFuture`, `catch_unwind`).
- `regex` 1.13, or a Unicode-category crate (F2).
- A nightly toolchain pinned for `cargo-public-api`, and its version (F4).
