//! The App's media HTTP client (AD-24 amended, AD-30): what URL tracks are
//! fetched with. It is never the Discord edge client and never carries the
//! bot token.

use reqwest::{Client, Url, redirect};

use crate::config::VoiceConfig;

/// Builds the App's one media client from `config`: connect timeout
/// `media_connect_timeout`, no proxy (environment proxies are ignored), no
/// default headers or user agent, and a redirect policy that refuses hop
/// `media_max_redirects + 1` and any hop to Discord's API. There is no
/// total timeout: a track streams for minutes.
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
        .no_proxy()
        .redirect(policy)
        .build()
        .map_err(|e| format!("the media HTTP client could not be built: {e}"))
}

/// Whether `url` points at Discord's API: host `discord.com` or
/// `discordapp.com` (or a subdomain of either; case-insensitive, a trailing
/// dot ignored) with `api` as the first path segment.
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
            .is_some_and(|first| first.eq_ignore_ascii_case("api"))
}

#[cfg(test)]
mod tests;
