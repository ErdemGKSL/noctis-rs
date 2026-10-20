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
    NoctisArgs { mode, dry_run }
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
