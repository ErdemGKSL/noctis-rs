//! The media HTTP matrix, against a local server (no network).

use std::time::{Duration, Instant};

use songbird::input::codecs::{get_codec_registry, get_probe};
use songbird::input::{AudioStreamError, Compose as _, HttpRequest, Input};

use super::*;
use crate::test_support::MediaServer;

fn client(redirects: u8) -> Client {
    media_client(&VoiceConfig::default().media_max_redirects(redirects))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// The lazy input's fetch, as the driver runs it.
async fn fetch(client: &Client, url: String) -> Result<(), String> {
    HttpRequest::new(client.clone(), url)
        .create_async()
        .await
        .map(drop)
        .map_err(|e| describe(&e))
}

/// The error and its sources.
fn describe(error: &AudioStreamError) -> String {
    let mut out = format!("{error:?}");
    if let AudioStreamError::Fail(inner) = error {
        let mut source: Option<&dyn std::error::Error> = Some(inner.as_ref());
        while let Some(e) = source {
            out.push_str(" / ");
            out.push_str(&e.to_string());
            source = e.source();
        }
    }
    out
}

#[test]
fn discord_api_hosts() {
    for (url, api) in [
        ("https://discord.com/api/v10/x", true),
        ("HTTPS://DISCORD.COM/API", true),
        ("https://discord.com./api", true),
        ("https://ptb.discord.com/api/x", true),
        ("https://discordapp.com/api", true),
        ("https://canary.discordapp.com/API/v9", true),
        ("https://discord.com/%61pi/v10/x", true),
        ("https://discord.com/%41PI/x", true),
        ("https://discord.com/%2561pi/x", false),
        ("https://discord.com/apix", false),
        ("https://discord.com/", false),
        ("https://dïscord.com/api", false),
        ("https://notdiscord.com/api", false),
        ("https://discord.com.evil.net/api", false),
        ("https://cdn.discordapp.com/attachments/1/2/a.ogg", false),
    ] {
        let parsed = Url::parse(url).unwrap_or_else(|e| panic!("{url}: {e}"));
        assert_eq!(is_discord_api(&parsed), api, "{url}");
    }
}

/// The GET carries only `host`, `accept` (and maybe `range` /
/// `accept-encoding`): no authorization, user agent or Noctis header.
#[tokio::test]
async fn request_headers_are_bare() {
    let server = MediaServer::start().await;
    fetch(&client(5), server.url("/wav"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let heads = server.heads();
    assert_eq!(heads.len(), 1, "{heads:?}");
    let head = heads[0].to_ascii_lowercase();
    for line in head.lines().skip(1).filter(|l| !l.is_empty()) {
        let name = line.split(':').next().unwrap_or_default().trim();
        assert!(
            ["host", "accept", "range", "accept-encoding"].contains(&name),
            "unexpected header `{line}`"
        );
        for banned in [
            "noctis",
            "discordbot",
            "bot ",
            "authorization",
            "user-agent",
        ] {
            assert!(!line.contains(banned), "`{line}` contains `{banned}`");
        }
    }
}

#[tokio::test]
async fn redirect_cap() {
    let server = MediaServer::start().await;
    // `/hop/4` is 5 redirects, `/hop/5` six.
    fetch(&client(5), server.url("/hop/4"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let refused = fetch(&client(5), server.url("/hop/5")).await;
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("more than 5 redirects")),
        "{refused:?}"
    );
    let none = fetch(&client(0), server.url("/hop/0")).await;
    assert!(
        none.as_ref()
            .is_err_and(|e| e.contains("more than 0 redirects")),
        "{none:?}"
    );
}

/// A redirect to Discord's API is refused by the policy, before any
/// connection to Discord.
#[tokio::test]
async fn redirect_to_discord_api_is_refused() {
    let server = MediaServer::start().await;
    let refused = fetch(&client(5), server.url("/discord")).await;
    assert!(
        refused.as_ref().is_err_and(|e| e.contains("Discord's API")),
        "{refused:?}"
    );
}

/// No listener: the fetch fails within the connect timeout.
#[tokio::test]
async fn connection_refused_fails_fast() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
    let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    drop(listener);
    let started = Instant::now();
    let failed = fetch(&client(5), format!("http://{addr}/wav")).await;
    assert!(failed.is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// Environment proxies are ignored (`no_proxy`): the check runs in a child
/// test process whose environment points every proxy at a dead port.
#[test]
fn proxy_environment_is_ignored() {
    let exe = std::env::current_exe().unwrap_or_else(|e| panic!("{e}"));
    let output = std::process::Command::new(exe)
        .args(["--exact", "media::tests::proxy_child", "--nocapture"])
        .env("NOCTIS_VOICE_PROXY_CHILD", "1")
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("ALL_PROXY", "http://127.0.0.1:1")
        .env("http_proxy", "http://127.0.0.1:1")
        .env("https_proxy", "http://127.0.0.1:1")
        .env("all_proxy", "http://127.0.0.1:1")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("1 passed"),
        "the child ran no test: {stdout}"
    );
}

/// The child half of `proxy_environment_is_ignored`; a no-op elsewhere.
#[tokio::test]
async fn proxy_child() {
    if std::env::var_os("NOCTIS_VOICE_PROXY_CHILD").is_none() {
        return;
    }
    let server = MediaServer::start().await;
    fetch(&client(5), server.url("/wav"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(server.heads().len(), 1);
}

/// Real decoding without a driver: the served WAV is probed and decoded by
/// symphonia, from HTTP, a file and a child process.
#[cfg(feature = "wav")]
#[tokio::test]
async fn wav_decodes_from_every_source() {
    use crate::track::{Track, TrackId};

    let playable = |input: Input| async move {
        input
            .make_playable_async(get_codec_registry(), get_probe())
            .await
            .map(|input| input.is_playable())
            .map_err(|e| e.to_string())
    };
    let server = MediaServer::start().await;
    let media = client(5);
    let url = Track::url(server.url("/hop/1"))
        .unwrap_or_else(|e| panic!("{e}"))
        .into_info(TrackId::new(1));
    assert_eq!(playable(url.input(&media)).await, Ok(true));

    let dir = std::env::temp_dir().join(format!("noctis-voice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let path = dir.join("a.wav");
    std::fs::write(&path, crate::test_support::wav()).unwrap_or_else(|e| panic!("{e}"));
    let file = Track::file(&path)
        .unwrap_or_else(|e| panic!("{e}"))
        .into_info(TrackId::new(2));
    assert_eq!(playable(file.input(&media)).await, Ok(true));
    let command = Track::command(
        "sh",
        [
            std::ffi::OsString::from("-c"),
            format!("cat '{}'", path.display()).into(),
        ],
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .into_info(TrackId::new(3));
    assert_eq!(playable(command.input(&media)).await, Ok(true));
    let _ = std::fs::remove_dir_all(&dir);
}
