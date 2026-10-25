//! The track input matrix.

use super::*;

fn reason(result: Result<Track, VoiceError>) -> String {
    match result {
        Err(VoiceError::InvalidInput { reason }) => reason,
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn files() {
    for path in ["a.wav", "音楽/ç.ogg"] {
        let track = Track::file(path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(track.source(), &TrackSource::File(PathBuf::from(path)));
    }
    // Not opened at construction.
    assert!(Track::file("/definitely/missing.wav").is_ok());
    assert!(reason(Track::file("")).contains("empty"));
}

#[test]
fn urls_ok() {
    for url in [
        "https://cdn.discordapp.com/attachments/1/2/a.ogg",
        "http://127.0.0.1:8080/x.mp3",
        "https://dïscord.com/api",
        "https://discord.com/apix",
        "https://discord.com/%2561pi/x",
    ] {
        let track = Track::url(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        assert!(matches!(track.source(), TrackSource::Url(_)), "{url}");
    }
}

#[test]
fn urls_refused() {
    for (url, rule) in [
        ("https://discord.com/api/v10/x", "Discord's API"),
        ("HTTPS://DISCORD.COM/API", "Discord's API"),
        ("https://discord.com./api", "Discord's API"),
        ("https://ptb.discord.com/api/x", "Discord's API"),
        ("https://discordapp.com/api", "Discord's API"),
        ("https://discord.com/%61pi/v10/x", "Discord's API"),
        ("https://discord.com/%41PI/x", "Discord's API"),
        ("ftp://h/a.mp3", "scheme"),
        ("file:///a", "scheme"),
        ("data:audio/ogg;base64,AAAA", "scheme"),
        ("https://u:p@h/a.mp3", "credentials"),
        ("https://u@h/a", "credentials"),
        ("", "malformed"),
        ("not a url", "malformed"),
        ("https://", "malformed"),
    ] {
        let reason = reason(Track::url(url));
        assert!(reason.contains(rule), "{url}: {reason}");
    }
}

#[test]
fn url_length() {
    let base = "https://cdn.example.com/";
    let ok = format!("{base}{}", "a".repeat(MAX_URL_LEN - base.len()));
    assert_eq!(ok.len(), 4096);
    assert!(Track::url(&ok).is_ok());
    let long = format!("{ok}a");
    assert!(reason(Track::url(&long)).contains("4096"));
}

#[test]
fn commands() {
    let track = Track::command("yt-dlp", ["-o", "-", "URL"]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(track.source(), &TrackSource::Command("yt-dlp".into()));
    assert_eq!(track.args, ["-o", "-", "URL"]);
    let shell = Track::command("sh", ["-c", "cat f.wav"]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(shell.args.len(), 2);
    assert!(reason(Track::command("", ["x"])).contains("empty"));
}

#[test]
fn titles_and_infos() {
    let long: String = "ç".repeat(257);
    let track = Track::file("a.wav")
        .unwrap_or_else(|e| panic!("{e}"))
        .title(long)
        .requested_by(Id::new(7));
    let info = track.into_info(TrackId::new(3));
    assert_eq!(info.title().map(|t| t.chars().count()), Some(256));
    assert_eq!(info.requested_by(), Some(Id::new(7)));
    assert_eq!(info.id().get(), 3);
    assert_eq!(info.id().to_string(), "3");
    let exact: String = "x".repeat(256);
    let kept = Track::file("a.wav")
        .unwrap_or_else(|e| panic!("{e}"))
        .title(exact.clone())
        .into_info(TrackId::new(4));
    assert_eq!(kept.title(), Some(exact.as_str()));
    let empty = Track::file("a.wav")
        .unwrap_or_else(|e| panic!("{e}"))
        .title("")
        .into_info(TrackId::new(5));
    assert_eq!(empty.title(), None);
    assert_eq!(empty.clone(), empty);
    assert_ne!(empty, kept);
    assert!(format!("{empty:?}").contains("TrackInfo"));
}

/// A missing file fails when the driver creates it, never before.
#[tokio::test]
async fn missing_file_fails_on_create() {
    let info = Track::file("/definitely/missing.wav")
        .unwrap_or_else(|e| panic!("{e}"))
        .into_info(TrackId::new(1));
    let client = reqwest::Client::new();
    let input = info.input(&client);
    assert!(!input.is_playable());
    let created = input
        .make_playable_async(
            songbird::input::codecs::get_codec_registry(),
            songbird::input::codecs::get_probe(),
        )
        .await;
    assert!(created.is_err());
}

/// A program that does not exist fails at create; nothing spawns before.
#[tokio::test]
async fn command_spawn_fails_on_create() {
    let info = Track::command("/definitely/not/a/program", ["x"])
        .unwrap_or_else(|e| panic!("{e}"))
        .into_info(TrackId::new(1));
    let mut compose = ChildCompose {
        program: "/definitely/not/a/program".into(),
        args: Vec::new(),
    };
    assert!(!compose.should_create_async());
    assert!(compose.create().is_err());
    assert!(compose.create_async().await.is_err());
    let created = info
        .input(&reqwest::Client::new())
        .make_playable_async(
            songbird::input::codecs::get_codec_registry(),
            songbird::input::codecs::get_probe(),
        )
        .await;
    assert!(created.is_err());
}
