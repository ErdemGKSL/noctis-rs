# Releasing Noctis

This runbook is for the maintainer. It covers how a Noctis release works, the one-time
setup before the first release, publishing 0.1.0, later releases, and what to do when
something goes wrong.

Noctis is 13 crates on crates.io: the `noctis` facade and its 12 `noctis-*` sub-crates.
They always share one version, `[workspace.package] version` in the root `Cargo.toml`.
`xtask`, `examples/music-bot` and `examples/ui-recipes` are never published.

## How a release works

The pipeline is [release-plz](https://release-plz.dev) 0.4.1, configured in
[`release-plz.toml`](../../release-plz.toml), and the workflow `release.yml`. Every
push to `main` runs its two jobs:

1. **`release-pr`** compares the committed [`CHANGELOG.md`](../../CHANGELOG.md) with
   the current version. While the changelog has no heading for that version, it opens
   (or updates) a pull request titled `chore: release`. That PR changes one thing: it
   inserts the version heading, such as
   `## [0.1.0](https://github.com/ErdemGKSL/noctis-rs/releases/tag/v0.1.0) - 2026-11-02`,
   right under `## [Unreleased]`. It never writes a commit list: the changelog is
   written by hand. This job only has the repository's `GITHUB_TOKEN`.
2. **`release`** runs `release-plz release` in the protected `release` environment,
   the only place that holds the crates.io token. It publishes only when the pushed
   commit merges a release PR (`release_always = false`); on any other push it
   releases nothing. It publishes the 13 crates in dependency order (one valid
   order is the one `cargo run --release -p xtask -- release-check` prints), then creates the tag
   `v<version>` and a GitHub release for `noctis` only. While crates.io rate-limits
   new crates, `.github/scripts/release-retry.sh` retries the whole step (12
   attempts, 11 minutes apart); each attempt skips the crates already published.

Every `ci.yml` run also guards the release offline:

- `publish-dry-run`: `cargo publish --workspace --dry-run --locked` packages every
  crate and builds it from its `.crate` file, without a token;
- `semver`: `.github/scripts/semver-check.sh` runs cargo-semver-checks on `noctis`
  with every feature against the latest version on crates.io (skipped until the first
  release exists);
- `release-check`: `xtask release-check` (crate metadata, versions, dependency edges,
  `release-plz.toml`, the changelog header) and the retry script's self-test;
- `deps-check`: `.github/scripts/check-default-graph.sh` keeps image, voice, prefix,
  testkit, Songbird and takumi out of the default `noctis` build;
- `each-feature` and `all-features`: the feature matrix.

The `noctis` dev-dependencies of `noctis-core`, `noctis-testkit` and `noctis-voice` are
path-only (`noctis = { path = "../noctis" }`, no version): `noctis` depends on those
crates, so a versioned dev-dependency would need `noctis` on crates.io first.
`cargo publish` strips path-only dev-dependencies. Keep them that way;
`release-check` fails otherwise.

## Before the first release (Story 14.8)

Do these once, in order.

1. **Turn CI back on.** Move every file of `.github/workflows-disabled/` to
   `.github/workflows/` (`git mv .github/workflows-disabled/*.yml .github/workflows/`),
   update the paths that name `workflows-disabled` (AGENTS.md, this guide), push, and
   fix every failing job until `ci.yml` is green on `main`.
2. **Let workflows open pull requests.** In the repository's Settings → Actions →
   General → Workflow permissions, choose **Read and write permissions** and tick
   **Allow GitHub Actions to create and approve pull requests**. Without it,
   `release-pr` cannot open the release PR.
3. **Create the crates.io token.** On crates.io, Account Settings → API Tokens → New
   Token: scopes `publish-new` and `publish-update`, crate patterns `noctis` and
   `noctis-*`, and an expiry date (put a reminder in your calendar a week before it).
4. **Create the `release` environment.** Settings → Environments → New environment,
   named `release`:
   - Deployment branches and tags: **Selected branches**, add `main`;
   - Environment secrets: add `CARGO_REGISTRY_TOKEN` with the token from step 3;
   - no required reviewers: the `release` job runs on every push to `main`, so a
     reviewer rule would hold every push for approval.
5. **Optional: protect `main`.** Settings → Rules → Rulesets → New branch ruleset for
   `main`, requiring the `ci.yml` status checks on pull requests, with yourself on
   the bypass list so direct pushes keep working. The `chore: release` PR runs no
   `ci.yml` (GitHub does not start workflows for `GITHUB_TOKEN` events), so it is
   merged through the bypass; CI runs on the merge push to `main`.
6. **Check the release PR.** The next push to `main` (the one from step 1 is enough)
   makes `release-pr` open `chore: release`. Its diff must be exactly two added lines
   in `CHANGELOG.md`, right after `## [Unreleased]` and its blank line:

   ```text
   ## [0.1.0](https://github.com/ErdemGKSL/noctis-rs/releases/tag/v0.1.0) - <date>

   ```

   Versions in the `Cargo.toml` files stay `0.1.0`. If the diff is anything else,
   close the PR and see [Troubleshooting](#troubleshooting).

## Publishing 0.1.0 (Story 14.9)

1. **Review the release PR.** Read the 0.1.0 entry under `## [Unreleased]` once more;
   anything you change on `main` updates the PR on the next push.
2. **Merge it with a merge commit** (not squash or rebase), so release-plz recognizes
   the merge on `main`.
3. **Watch the `release` job** in the Actions tab. crates.io accepts 5 new crates at
   once, then 1 every 10 minutes, so 0.1.0 (13 new crates) publishes 5 crates, then
   waits for the rate limit; the retries finish the rest in about 90 minutes (the job
   allows 180). If the job still stops, use **Re-run failed jobs**: crates already
   published are skipped.
4. **Check the result:**
   - crates.io lists all 13 crates at 0.1.0 (`noctis` and every `noctis-*`);
   - docs.rs built `noctis` with every feature (its build page shows no failure);
   - the tag `v0.1.0` and the GitHub release `v0.1.0` exist.
5. **Switch the project template to crates.io.** In
   `examples/project-template/Cargo.toml`, replace the `path` dependencies on `noctis`
   with `noctis = "0.1"` plus the same features (the commented line above them shows
   the form), refresh its `Cargo.lock`, and make its gates pass.
6. **Check the template and the roadmap.** Copy `examples/project-template/` out of
   the repository and check that it builds with `cargo build --release` against
   crates.io, and that the README roadmap marks Milestones 1-4 done.

## Later releases

- **Write the changelog as you go.** Every user-visible change goes under
  `## [Unreleased]` in `CHANGELOG.md`, in the existing subsections, with migration
  notes when a bot must change. Never commit a version heading yourself: a heading for
  the current version stops the release PR.
- **Bump the version.** Set `[workspace.package] version` in the root `Cargo.toml`
  (and the `noctis*` entries of `[workspace.dependencies]`) to the new version. Before
  1.0, a breaking change bumps the minor version (`0.1.x` → `0.2.0`); everything else
  bumps the patch version. The next push opens the release PR for that version.
- **Semver.** CI's `semver` job compares `noctis` with the latest release on every
  push, and release-plz runs cargo-semver-checks on `noctis` again in the release PR.
  A reported break needs a minor bump (or a fix). `noctis::image::unstable` is outside
  the semver promise (AD-28): the check still reports changes there, and they do not
  need a minor bump.
- Every change of the public API still updates `xtask/public-api.txt`, the project
  template and its Usage Skill in the same commit (see `AGENTS.md`).

## Troubleshooting

- **No release PR opens.** Check that step 2 of the setup is done and that the
  `release-pr` job ran (it is skipped on forks). A changelog that already has a heading
  for the current version opens no PR: remove it, or bump the version. Run
  `cargo run --release -p xtask -- release-check` locally.
- **429 / "too many new crates".** Expected on the first release; the retry script
  waits and retries. If it ran out of attempts, re-run the failed job later.
- **A crate fails to verify.** `cargo publish` builds each crate from its packaged
  files. Reproduce it with `cargo publish --workspace --dry-run --locked
  --allow-dirty`, fix it on `main` and merge again; already published crates are
  skipped, and a fix that changes a published crate needs a new patch version.
- **Partial publish.** Some crates are on crates.io and the job stopped. Re-run the
  failed job: release-plz skips what is published and continues in order. Never yank
  to "restart"; a published version cannot be reused.
- **Expired or revoked token.** The job fails with 401 or 403. Create a new token
  (setup step 3), replace the `CARGO_REGISTRY_TOKEN` secret of the `release`
  environment, and re-run the job.
- **docs.rs build failed.** Open the build log on docs.rs. Its image has a C compiler
  and cmake (libwebp, libopus); reproduce locally with
  `RUSTDOCFLAGS="-D warnings" cargo doc --release -p noctis --all-features --no-deps`,
  fix it, and release a patch version (docs.rs builds each version once).

## Local checks

Run these before merging a release PR, or after changing anything in the release
pipeline:

```sh
cargo run --release -p xtask -- release-check
cargo publish --workspace --dry-run --locked --allow-dirty
bash .github/scripts/check-default-graph.sh
bash .github/scripts/release-retry-test.sh
# Needs cargo-semver-checks 0.51; compares with crates.io, or with a git revision:
bash .github/scripts/semver-check.sh
bash .github/scripts/semver-check.sh --baseline-rev HEAD
```
