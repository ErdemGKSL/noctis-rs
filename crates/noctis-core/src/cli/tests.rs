//! Story 6.9, Matrix 24-27: the built-in parser.

use std::ffi::OsString;

use super::*;

fn id(value: u64) -> Option<Id<GuildMarker>> {
    Id::new_checked(value)
}

fn parsed(args: &[&str]) -> NoctisArgs {
    NoctisArgs::parse(args.iter().copied()).unwrap_or_else(|e| panic!("{args:?}: {e}"))
}

fn rejected(args: &[&str]) -> CliError {
    match NoctisArgs::parse(args.iter().copied()) {
        Ok(ok) => panic!("{args:?} parsed as {ok:?}"),
        Err(error) => error,
    }
}

fn mode(mode: Option<CliMode>, dry_run: bool) -> NoctisArgs {
    NoctisArgs {
        mode,
        dry_run,
        no_resume: false,
        features: None,
    }
}

fn no_resume() -> NoctisArgs {
    NoctisArgs {
        no_resume: true,
        ..NoctisArgs::default()
    }
}

/// Matrix 24.
#[test]
fn parse_accepts_every_mode() {
    let none: [&str; 0] = [];
    assert_eq!(NoctisArgs::parse(none), Ok(NoctisArgs::default()));
    assert_eq!(parsed(&["--publish"]), mode(Some(CliMode::Publish), false));
    let guild_42 = mode(Some(CliMode::PublishGuild(id(42))), false);
    assert_eq!(parsed(&["--publish-guild", "42"]), guild_42);
    assert_eq!(parsed(&["--publish-guild=42"]), guild_42);
    assert_eq!(
        parsed(&["--publish-guild"]),
        mode(Some(CliMode::PublishGuild(None)), false)
    );
    assert_eq!(
        parsed(&["--publish-guild", "--dry-run"]),
        mode(Some(CliMode::PublishGuild(None)), true)
    );
    assert_eq!(
        parsed(&["--publish-clear", "--guild", "42"]),
        mode(Some(CliMode::PublishClear(id(42))), false)
    );
    assert_eq!(
        parsed(&["--guild=42", "--publish-clear"]),
        mode(Some(CliMode::PublishClear(id(42))), false)
    );
    assert_eq!(
        parsed(&["--dry-run", "--publish"]),
        mode(Some(CliMode::Publish), true)
    );
    assert_eq!(
        parsed(&["--publish-guild", "007"]),
        mode(Some(CliMode::PublishGuild(id(7))), false)
    );
    assert_eq!(
        parsed(&["--publish-guild", "18446744073709551615"]),
        mode(Some(CliMode::PublishGuild(id(u64::MAX))), false)
    );
}

/// Matrix 25.
#[test]
fn parse_rejects_malformed_lines() {
    assert_eq!(
        rejected(&["--bogus"]),
        CliError::UnknownFlag {
            flag: "--bogus".into()
        }
    );
    assert_eq!(
        rejected(&["positional"]),
        CliError::UnknownFlag {
            flag: "positional".into()
        }
    );
    assert_eq!(
        rejected(&["--", "--publish"]),
        CliError::UnknownFlag { flag: "--".into() }
    );
    assert_eq!(
        rejected(&["--publish", "--publish"]),
        CliError::DuplicateFlag { flag: "--publish" }
    );
    assert_eq!(
        rejected(&["--publish", "--publish-clear"]),
        CliError::ConflictingModes {
            first: "--publish",
            second: "--publish-clear"
        }
    );
    assert_eq!(rejected(&["--dry-run"]), CliError::DryRunWithoutPublish);
    assert_eq!(
        rejected(&["--publish", "--dry-run", "--dry-run"]),
        CliError::DuplicateFlag { flag: "--dry-run" }
    );
    assert_eq!(rejected(&["--guild", "1"]), CliError::GuildWithoutClear);
    assert_eq!(
        rejected(&["--publish", "--guild", "1"]),
        CliError::GuildWithoutClear
    );
    for value in [
        "0",
        "18446744073709551616",
        "12a",
        "١٢",
        "000000000000000000001",
    ] {
        assert_eq!(
            rejected(&["--publish-guild", value]),
            CliError::InvalidGuildId {
                flag: "--publish-guild",
                value: value.into()
            },
            "{value}"
        );
    }
    assert_eq!(
        rejected(&["--publish-guild="]),
        CliError::InvalidGuildId {
            flag: "--publish-guild",
            value: String::new()
        }
    );
    assert_eq!(
        rejected(&["--publish-clear", "--guild"]),
        CliError::MissingValue { flag: "--guild" }
    );
    assert_eq!(
        rejected(&["--publish-clear", "--guild", "--dry-run"]),
        CliError::MissingValue { flag: "--guild" }
    );
    assert_eq!(
        rejected(&["--publish=1"]),
        CliError::UnexpectedValue { flag: "--publish" }
    );
    assert_eq!(
        rejected(&["--publish", "--dry-run=yes"]),
        CliError::UnexpectedValue { flag: "--dry-run" }
    );
}

#[cfg(unix)]
#[test]
fn parse_rejects_non_utf8_lossily() {
    use std::os::unix::ffi::OsStringExt;

    let raw = OsString::from_vec(vec![b'-', b'-', 0xff]);
    let error = NoctisArgs::parse([raw]).unwrap_err();
    assert_eq!(
        error,
        CliError::UnknownFlag {
            flag: "--\u{fffd}".into()
        }
    );
}

/// Every message names its flag and a fix.
#[test]
fn errors_name_the_flag_and_a_fix() {
    let cases = [
        (rejected(&["--bogus"]), "--bogus"),
        (rejected(&["--publish-guild", "0"]), "--publish-guild"),
        (rejected(&["--publish-clear", "--guild"]), "--guild"),
        (rejected(&["--publish=1"]), "--publish"),
        (
            rejected(&["--publish", "--publish-clear"]),
            "--publish-clear",
        ),
        (rejected(&["--dry-run"]), "--dry-run"),
        (rejected(&["--guild", "1"]), "--guild"),
        (CliError::MissingDevGuild, "--publish-guild"),
    ];
    for (error, flag) in cases {
        let text = error.to_string();
        assert!(text.contains(flag), "{text}");
        assert!(text.contains(": "), "{text}");
    }
}

/// Matrix 26.
#[test]
fn split_keeps_the_rest_untouched() {
    let (args, rest) = NoctisArgs::split([
        "bot",
        "--verbose",
        "--publish-guild",
        "5",
        "x",
        "--",
        "--dry-run",
    ])
    .unwrap();
    assert_eq!(args, mode(Some(CliMode::PublishGuild(id(5))), false));
    assert_eq!(rest, ["bot", "--verbose", "x", "--", "--dry-run"]);

    let (args, rest) = NoctisArgs::split(["bot"]).unwrap();
    assert_eq!(args, NoctisArgs::default());
    assert_eq!(rest, ["bot"]);

    assert_eq!(
        NoctisArgs::split(["bot", "--publish", "--publish-clear"]),
        Err(CliError::ConflictingModes {
            first: "--publish",
            second: "--publish-clear"
        })
    );
    assert_eq!(
        NoctisArgs::split(["bot", "--publish-guild", "0"]).map(|(a, _)| a),
        Err(CliError::InvalidGuildId {
            flag: "--publish-guild",
            value: "0".into()
        })
    );
}

#[cfg(unix)]
#[test]
fn split_passes_non_utf8_through() {
    use std::os::unix::ffi::OsStringExt;

    let raw = OsString::from_vec(vec![0xff, b'x']);
    let (args, rest) =
        NoctisArgs::split([OsString::from("bot"), raw.clone(), "--publish".into()]).unwrap();
    assert_eq!(args.mode(), Some(CliMode::Publish));
    assert_eq!(rest, [OsString::from("bot"), raw]);
}

/// Matrix 27.
#[test]
fn into_iter_round_trips() {
    let values = [
        NoctisArgs::default(),
        mode(Some(CliMode::Publish), false),
        mode(Some(CliMode::Publish), true),
        mode(Some(CliMode::PublishGuild(None)), false),
        mode(Some(CliMode::PublishGuild(None)), true),
        mode(Some(CliMode::PublishGuild(id(u64::MAX))), true),
        mode(Some(CliMode::PublishClear(None)), true),
        mode(Some(CliMode::PublishClear(id(42))), false),
    ];
    for value in values {
        let flags: Vec<OsString> = value.clone().into_iter().collect();
        assert_eq!(NoctisArgs::parse(flags.clone()), Ok(value), "{flags:?}");
    }
    let flags: Vec<OsString> = mode(Some(CliMode::PublishClear(id(42))), true)
        .into_iter()
        .collect();
    assert_eq!(flags, ["--publish-clear", "--guild", "42", "--dry-run"]);
}

/// Story 12.3 P1, P2, P10: `--no-resume` runs the bot and round-trips.
#[test]
fn no_resume_runs_the_bot() {
    let none: [&str; 0] = [];
    let default = NoctisArgs::parse(none).unwrap();
    assert!(!default.no_resume());
    assert_eq!(default.mode(), None);

    let args = parsed(&["--no-resume"]);
    assert!(args.no_resume());
    assert_eq!(args.mode(), None);
    assert!(!args.dry_run());
    let flags: Vec<OsString> = args.clone().into_iter().collect();
    assert_eq!(flags, ["--no-resume"]);

    for value in [NoctisArgs::default(), no_resume()] {
        assert_eq!(NoctisArgs::parse(value.clone()), Ok(value));
    }
}

/// Story 12.3 P3-P7.
#[test]
fn no_resume_rejects_malformed_lines() {
    assert_eq!(
        rejected(&["--no-resume", "--no-resume"]),
        CliError::DuplicateFlag {
            flag: "--no-resume"
        }
    );
    for line in ["--no-resume=", "--no-resume=1", "--no-resume=ü"] {
        assert_eq!(
            rejected(&[line]),
            CliError::UnexpectedValue {
                flag: "--no-resume"
            },
            "{line}"
        );
    }
    let publishing: [&[&str]; 4] = [
        &["--no-resume", "--publish"],
        &["--publish", "--no-resume"],
        &["--publish-guild", "5", "--no-resume"],
        &["--publish-clear", "--guild", "5", "--no-resume"],
    ];
    for line in publishing {
        assert_eq!(rejected(line), CliError::NoResumeWithPublish, "{line:?}");
    }
    assert_eq!(
        rejected(&["--dry-run", "--no-resume"]),
        CliError::DryRunWithoutPublish
    );
    for text in ["--no-resum", "--NO-RESUME", "-no-resume", "--no-resume-x"] {
        assert_eq!(
            rejected(&[text]),
            CliError::UnknownFlag { flag: text.into() },
            "{text}"
        );
    }
}

/// Story 12.3 P8.
#[test]
fn split_reserves_no_resume() {
    let (args, rest) =
        NoctisArgs::split(["bot", "--no-resume", "--x", "--", "--no-resume"]).unwrap();
    assert!(args.no_resume());
    assert_eq!(rest, ["bot", "--x", "--", "--no-resume"]);
}

/// Story 12.3 P9.
#[cfg(unix)]
#[test]
fn no_resume_next_to_non_utf8() {
    use std::os::unix::ffi::OsStringExt;

    let raw = OsString::from_vec(vec![0xff]);
    let (args, rest) = NoctisArgs::split([OsString::from("--no-resume"), raw.clone()]).unwrap();
    assert!(args.no_resume());
    assert_eq!(rest, std::slice::from_ref(&raw));
    assert_eq!(
        NoctisArgs::parse([OsString::from("--no-resume"), raw]),
        Err(CliError::UnknownFlag {
            flag: "\u{fffd}".into()
        })
    );
}

/// Story 12.3 P11.
#[test]
fn no_resume_messages_name_the_flag() {
    for error in [
        CliError::NoResumeWithPublish,
        CliError::UnknownFlag { flag: "x".into() },
    ] {
        let text = error.to_string();
        assert!(text.contains("--no-resume"), "{text}");
    }
}

fn flags(list: &[&str]) -> BootFlags {
    BootFlags::only(list).expect("valid flags")
}

/// Story 13.2 C1-C3, C8.
#[test]
fn features_parse() {
    let ab = Some(&flags(&["a", "b"]));
    assert_eq!(parsed(&["--features", "a,b"]).features(), ab);
    assert_eq!(parsed(&["--features=b,a"]).features(), ab);
    assert_eq!(
        parsed(&["--features", "all"]).features(),
        Some(&BootFlags::all())
    );
    assert_eq!(
        parsed(&["--features=none"]).features(),
        Some(&BootFlags::none())
    );
    assert_eq!(parsed(&[]).features(), None);
    assert_eq!(parsed(&["--publish"]).features(), None);

    let publish = parsed(&["--publish", "--dry-run", "--features", "a"]);
    assert_eq!(publish.mode(), Some(CliMode::Publish));
    assert!(publish.dry_run());
    assert_eq!(publish.features(), Some(&flags(&["a"])));
    let run = parsed(&["--no-resume", "--features", "a"]);
    assert!(run.no_resume());
    assert_eq!(run.features(), Some(&flags(&["a"])));
    let clear = parsed(&["--publish-clear", "--guild", "1", "--features", "a"]);
    assert_eq!(clear.mode(), Some(CliMode::PublishClear(id(1))));
    assert_eq!(clear.features(), Some(&flags(&["a"])));
    let first = parsed(&["--features", "a", "--publish-guild", "7"]);
    assert_eq!(first.mode(), Some(CliMode::PublishGuild(id(7))));
}

/// Story 13.2 C4-C7.
#[test]
fn features_rejects_malformed_lines() {
    assert_eq!(rejected(&["--features"]), CliError::MissingFeatures);
    assert_eq!(
        rejected(&["--features", "--dry-run"]),
        CliError::MissingFeatures
    );
    assert_eq!(
        rejected(&["--features="]),
        CliError::InvalidFeatures {
            value: String::new(),
            reason: BootFlagError::Empty
        }
    );
    assert_eq!(
        rejected(&["--features", "a", "--features", "b"]),
        CliError::DuplicateFlag { flag: "--features" }
    );
    assert_eq!(
        rejected(&["--features", "A"]),
        CliError::InvalidFeatures {
            value: "A".into(),
            reason: BootFlagError::InvalidChar { index: 0, ch: 'A' }
        }
    );
    assert_eq!(
        rejected(&["--features", "a,,b"]),
        CliError::InvalidFeatures {
            value: "a,,b".into(),
            reason: BootFlagError::Empty
        }
    );
}

/// Story 13.2 C9.
#[test]
fn features_round_trip() {
    let with = |mut args: NoctisArgs, features: BootFlags| {
        args.features = Some(features);
        args
    };
    let values = [
        with(NoctisArgs::default(), flags(&["b", "a"])),
        with(NoctisArgs::default(), BootFlags::all()),
        with(no_resume(), BootFlags::none()),
        with(mode(Some(CliMode::Publish), true), flags(&["x"])),
        with(
            mode(Some(CliMode::PublishClear(id(42))), true),
            flags(&["eco_v2-x"]),
        ),
    ];
    for value in values {
        let flags: Vec<OsString> = value.clone().into_iter().collect();
        assert_eq!(NoctisArgs::parse(flags.clone()), Ok(value), "{flags:?}");
    }
    let rendered: Vec<OsString> = with(mode(Some(CliMode::Publish), true), flags(&["b", "a"]))
        .into_iter()
        .collect();
    assert_eq!(rendered, ["--publish", "--dry-run", "--features", "a,b"]);
    let rendered: Vec<OsString> = with(no_resume(), BootFlags::none()).into_iter().collect();
    assert_eq!(rendered, ["--no-resume", "--features", "none"]);
}

/// Story 13.2 C10.
#[test]
fn split_claims_features_before_the_marker() {
    let (args, rest) =
        NoctisArgs::split(["bot", "--x", "--features", "a", "--", "--features", "b"]).unwrap();
    assert_eq!(args.features(), Some(&flags(&["a"])));
    assert_eq!(rest, ["bot", "--x", "--", "--features", "b"]);
}

/// Story 13.2 C11.
#[cfg(unix)]
#[test]
fn features_non_utf8_value_is_lossy() {
    use std::os::unix::ffi::OsStringExt;

    let raw = OsString::from_vec(vec![b'a', 0xff]);
    assert_eq!(
        NoctisArgs::parse([OsString::from("--features"), raw]),
        Err(CliError::InvalidFeatures {
            value: "a\u{fffd}".into(),
            reason: BootFlagError::InvalidChar {
                index: 1,
                ch: '\u{fffd}'
            }
        })
    );
}

/// Story 13.2: the exact texts.
#[test]
fn features_messages() {
    assert_eq!(
        CliError::MissingFeatures.to_string(),
        "`--features` needs a value: write `--features a,b`, `--features all` or `--features none`"
    );
    assert_eq!(
        CliError::InvalidFeatures {
            value: "a,".into(),
            reason: BootFlagError::Empty
        }
        .to_string(),
        "`--features` got `a,`: a Boot Flag is empty: write flags such as `beta`, separated by \
         single commas"
    );
    assert_eq!(
        CliError::BootFlagsMismatch {
            given: flags(&["b", "a"]),
            built: BootFlags::none()
        }
        .to_string(),
        "`--features a,b` does not match the Boot Flags this App was built with (`none`): build \
         it with `App::from_env()`, which reads `--features`, or pass the same selection to \
         `AppBuilder::boot_flags`"
    );
    let unknown = CliError::UnknownFlag { flag: "--x".into() }.to_string();
    assert!(
        unknown.contains("--dry-run, --no-resume and --features <a,b|all|none>"),
        "{unknown}"
    );
}
