//! trybuild `compile_fail` suites of the facade (AD-4, AD-28).
//!
//! Layout: `tests/ui/<area>/<RULE-ID>_<slug>.rs` with the blessed `.stderr`
//! beside it, `<area>` being the rule id's area lowercased (`CV2-05` lives in
//! `tests/ui/cv2/`). A diagnosed rule's `.stderr` contains `[<RULE-ID>]`; a
//! by-construction rule's fixture carries `// [<RULE-ID>]` on the offending
//! line. Fixtures that need a facade feature live in `tests/ui-<feature>/` and
//! get their own feature-gated suite here, added by the first story that needs
//! one. `cargo run --release -p xtask -- rules-catalog` checks the layout.
//!
//! `tests/types/<slug>.rs` holds `compile_fail` cases of the typed API that
//! no Discord rule governs (for example a non-payload event handler parameter);
//! `rules-catalog` does not read it, and it runs with the featureless suite.
//!
//! The featureless suite runs only when no optional facade feature is on,
//! because a feature can change rustc's output. There are no `pass` suites:
//! boundary passes live in `tests/boundary/`.
//!
//! Bless after an intended change or a toolchain bump (same commit):
//! `TRYBUILD=overwrite cargo test --release -p noctis --test ui`.
//!
//! trybuild runs `cargo check` without `--release`; the repo-root
//! `.cargo/config.toml` makes `[profile.dev]` release-equivalent, and
//! `assert_trybuild_dev_profile` proves it after every run.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers panic with a message naming the broken invariant"
)]

#[cfg(not(any(
    feature = "image",
    feature = "image-gif",
    feature = "image-emoji",
    feature = "default-font",
    feature = "i18n",
    feature = "prefix",
    feature = "testkit"
)))]
#[test]
fn ui() {
    harness::assert_pinned_toolchain();
    {
        let cases = trybuild::TestCases::new();
        cases.compile_fail("tests/ui/**/*.rs");
        cases.compile_fail("tests/types/**/*.rs");
        // The cases run when `cases` drops here.
    }
    harness::assert_trybuild_dev_profile();
}

/// The harness helpers; compiled with the suite only.
#[cfg(not(any(
    feature = "image",
    feature = "image-gif",
    feature = "image-emoji",
    feature = "default-font",
    feature = "i18n",
    feature = "prefix",
    feature = "testkit"
)))]
mod harness {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// The repository root (two levels above this crate's manifest).
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// `<target>`: the parent of `CARGO_TARGET_TMPDIR` (`<target>/tmp`).
    fn target_dir() -> PathBuf {
        Path::new(env!("CARGO_TARGET_TMPDIR"))
            .parent()
            .expect("CARGO_TARGET_TMPDIR has a parent")
            .to_path_buf()
    }

    fn rustc() -> Command {
        Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
    }

    fn stdout_of(cmd: &mut Command) -> String {
        let out = cmd.output().expect("run rustc");
        assert!(out.status.success(), "{cmd:?} failed");
        String::from_utf8(out.stdout).expect("rustc prints UTF-8")
    }

    /// T4: `.stderr` files are blessed on the toolchain pinned in
    /// `rust-toolchain.toml`; any other rustc words its errors differently.
    pub(super) fn assert_pinned_toolchain() {
        let toolchain = std::fs::read_to_string(repo_root().join("rust-toolchain.toml"))
            .expect("read rust-toolchain.toml");
        let channel = toolchain
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "channel").then(|| value.trim().trim_matches('"').to_owned())
            })
            .expect("rust-toolchain.toml has a channel");
        let version = stdout_of(rustc().arg("-V"));
        assert!(
            version.starts_with(&format!("rustc {channel} ")),
            "the UI tests run on the pinned toolchain {channel}, but `rustc -V` is `{}`; \
             run them on {channel}, and re-bless with `TRYBUILD=overwrite` in the commit that \
             bumps rust-toolchain.toml",
            version.trim()
        );
    }

    /// The host triple, as trybuild passes it to `--target`.
    fn host() -> String {
        stdout_of(rustc().arg("-vV"))
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .expect("`rustc -vV` prints the host")
            .to_owned()
    }

    /// T3: re-runs `cargo check` in trybuild's own project and target dirs and
    /// asserts every compiled artifact used the release-equivalent dev profile, so
    /// trybuild never produced a debug-grade build. At least one artifact must be
    /// seen; host-only artifacts are exempt from the `opt-level` check only,
    /// exactly as under `--release`.
    ///
    /// The trybuild internals mirrored here (project dir
    /// `<target>/tests/trybuild/noctis`, bin `noctis-tests`, `--target <host>`,
    /// `--offline`, `CARGO_INCREMENTAL=0`) were checked against trybuild 1.0.122;
    /// a trybuild upgrade must re-check them.
    pub(super) fn assert_trybuild_dev_profile() {
        let target = target_dir();
        let project = target.join("tests/trybuild/noctis");
        let host = host();
        let target_artifacts = target.join("tests/trybuild").join(&host);
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let out = Command::new(cargo)
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", target.join("tests/trybuild"))
            .env("CARGO_INCREMENTAL", "0")
            .env_remove("RUSTFLAGS")
            .args(["check", "--message-format=json", "--offline", "--target"])
            .arg(&host)
            .args(["--bin", "noctis-tests"])
            .output()
            .expect("run cargo check in the trybuild project");
        // The project's `main.rs` may hold the last failing fixture, so the exit
        // status is irrelevant; the dependencies' artifacts are what count.
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut artifacts = 0_usize;
        for line in stdout.lines() {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if message["reason"] != "compiler-artifact" {
                continue;
            }
            artifacts += 1;
            let profile = &message["profile"];
            // Host-only artifacts use `build-override`, whose `opt-level` defaults
            // to 0 under `--release` too; their other settings must match. They are
            // build scripts, proc macros and the host-built dependencies of proc
            // macros (`syn`, `quote`, ...): kind `lib`, but written under
            // `<target>/tests/trybuild/debug/` instead of `.../<host>/`.
            let build_kind = message["target"]["kind"].as_array().is_some_and(|kinds| {
                kinds
                    .iter()
                    .any(|k| k == "custom-build" || k == "proc-macro")
            });
            let in_target_dir = message["filenames"].as_array().is_some_and(|files| {
                files.iter().any(|f| {
                    f.as_str()
                        .is_some_and(|f| Path::new(f).starts_with(&target_artifacts))
                })
            });
            let host_only = build_kind || !in_target_dir;
            let release_like = (host_only || profile["opt_level"] == "3")
                && profile["debuginfo"] == 0
                && profile["debug_assertions"] == false
                && profile["overflow_checks"] == false;
            assert!(
                release_like,
                "trybuild compiled `{}` with a non-release-equivalent profile {profile}; \
                 restore .cargo/config.toml's [profile.dev]",
                message["package_id"]
            );
        }
        assert!(
            artifacts > 0,
            "cargo check in {} reported no compiler artifacts; stderr:\n{}",
            project.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
