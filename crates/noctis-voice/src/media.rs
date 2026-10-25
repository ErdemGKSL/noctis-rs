//! The App's media HTTP client (AD-24 amended, AD-30): what URL tracks are
//! fetched with. It is never the Discord edge client and never carries the
//! bot token.

use std::time::Duration;

use reqwest::{Client, Url, redirect};

use crate::config::VoiceConfig;

/// Builds the App's one media client from `config`: connect timeout
/// `media_connect_timeout`, no proxy (environment proxies are ignored), no
/// default headers or user agent, and a redirect policy that refuses hop
/// `media_max_redirects + 1` and any hop to Discord's API. There is no
/// total or read timeout: a track streams for minutes and a paused one
/// stops reading. A connection that dies silently is detected by TCP
/// keepalive (30 s, then 3 probes 10 s apart) and fails the track; a live
/// server that stalls is ended with `skip`.
pub(crate) fn media_client(config: &VoiceConfig) -> Result<Client, String> {
    let max = usize::from(config.get_media_max_redirects());
    let policy = redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > max {
            attempt.error(format!("more than {max} redirects"))
        } else if is_discord_api(attempt.url()) {
            attempt.error("a redirect to Discord's API is refused")
        } else {
            attempt.follow()
        }
    });
    Client::builder()
        .connect_timeout(config.get_media_connect_timeout())
        .tcp_keepalive(Duration::from_secs(30))
        .tcp_keepalive_interval(Duration::from_secs(10))
        .tcp_keepalive_retries(3)
        .no_proxy()
        .redirect(policy)
        .build()
        .map_err(|e| format!("the media HTTP client could not be built: {e}"))
}

/// Whether `url` points at Discord's API: host `discord.com` or
/// `discordapp.com` (or a subdomain of either; case-insensitive, a trailing
/// dot ignored) with `api` as the first path segment, compared after one
/// percent-decoding (`%61pi` is `api`).
pub(crate) fn is_discord_api(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let discord = ["discord.com", "discordapp.com"].iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
    });
    discord
        && url
            .path_segments()
            .and_then(|mut segments| segments.next())
            .is_some_and(|first| percent_decode(first).eq_ignore_ascii_case(b"api"))
}

/// `segment` with every `%XX` decoded once; other bytes as they are.
fn percent_decode(segment: &str) -> Vec<u8> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|hex| bytes[i] == b'%' && hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = hex {
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    decoded
}

#[cfg(test)]
mod tests;
