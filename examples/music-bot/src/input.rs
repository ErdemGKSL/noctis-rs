//! What `/play` plays: a `yt-dlp` child process whose stdout is the audio.
//!
//! A query is a link only when `Track::url` accepts it (`http`/`https`, no
//! credentials, not Discord's API); anything else is a search
//! (`ytsearch1:<query>`), so `file:///etc/passwd` or `https://user:pw@host/` is never
//! fetched as a link. The target always follows `--`, so a query such as `--exec rm`
//! is never parsed as an option.

use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::voice::Track;

/// The prefix that makes `yt-dlp` search and take the first result.
pub const SEARCH_PREFIX: &str = "ytsearch1:";

/// The program a track runs.
pub const PROGRAM: &str = "yt-dlp";

/// What `yt-dlp` gets after `--` for `query`: the trimmed query when it is a URL
/// `Track::url` accepts, else `ytsearch1:<trimmed query>`; `None` for a blank query.
pub fn query_target(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if Track::url(query).is_ok() {
        Some(query.to_owned())
    } else {
        Some(format!("{SEARCH_PREFIX}{query}"))
    }
}

/// `yt-dlp`'s arguments for `target`: the best Opus audio (48 kHz Opus plays
/// by passthrough) or the best audio, one item, quiet, to stdout, then `--` and the
/// target.
pub fn ytdlp_args(target: &str) -> [String; 10] {
    [
        "-f",
        "bestaudio[acodec=opus]/bestaudio",
        "--no-playlist",
        "--playlist-items",
        "1",
        "--quiet",
        "-o",
        "-",
        "--",
        target,
    ]
    .map(str::to_owned)
}

/// The track `/play query` queues, requested by `user`: `yt-dlp` with
/// [`ytdlp_args`], titled with the trimmed query (cut to 256 chars by `Track::title`);
/// `None` for a blank query.
pub fn track_for(query: &str, user: Id<UserMarker>) -> Option<Track> {
    let target = query_target(query)?;
    let track = Track::command(PROGRAM, ytdlp_args(&target)).ok()?;
    Some(track.title(query.trim()).requested_by(user))
}

#[cfg(test)]
mod tests {
    use noctis::voice::TrackSource;

    use super::*;

    fn search(query: &str) -> String {
        format!("{SEARCH_PREFIX}{query}")
    }

    #[test]
    fn blank_queries_play_nothing() {
        for query in ["", "   ", "\t\n"] {
            assert_eq!(query_target(query), None, "{query:?}");
            assert!(track_for(query, Id::new(1)).is_none(), "{query:?}");
        }
    }

    #[test]
    fn words_are_searches() {
        assert_eq!(
            query_target("never gonna give you up"),
            Some(search("never gonna give you up"))
        );
        assert_eq!(query_target("  lofi beats  "), Some(search("lofi beats")));
        assert_eq!(query_target("Gülümse 🎵"), Some(search("Gülümse 🎵")));
    }

    #[test]
    fn accepted_urls_are_links() {
        for url in [
            "https://youtu.be/dQw4w9WgXcQ",
            "http://example.com/a.ogg",
            "HTTPS://Example.com/a",
        ] {
            assert!(Track::url(url).is_ok(), "{url}");
            assert_eq!(query_target(url).as_deref(), Some(url));
            assert_eq!(query_target(&format!(" {url} ")).as_deref(), Some(url));
        }
    }

    /// A URL `Track::url` refuses, another scheme or an option-like query is a
    /// search, never a link or an option.
    #[test]
    fn refused_urls_and_options_are_searches() {
        for query in [
            "https://user:pw@host/a",
            "https://discord.com/api/v10/x",
            "https://",
            "file:///etc/passwd",
            "ftp://h/a",
            "-x --exec rm",
            "--version",
        ] {
            assert_eq!(query_target(query), Some(search(query)), "{query}");
            let args = ytdlp_args(&query_target(query).unwrap_or_default());
            assert_eq!(args[8], "--");
            assert!(args[9].starts_with(SEARCH_PREFIX), "{args:?}");
        }
    }

    #[test]
    fn args_and_track() {
        assert_eq!(
            ytdlp_args("ytsearch1:x"),
            [
                "-f",
                "bestaudio[acodec=opus]/bestaudio",
                "--no-playlist",
                "--playlist-items",
                "1",
                "--quiet",
                "-o",
                "-",
                "--",
                "ytsearch1:x"
            ]
        );
        let track = track_for("  lofi  ", Id::new(7));
        let source = track.as_ref().map(Track::source);
        assert!(
            matches!(source, Some(TrackSource::Command(p)) if p == PROGRAM),
            "{source:?}"
        );
    }

    /// The longest query (the option's `max_length`) stays whole as the target.
    #[test]
    fn long_query_is_kept_whole() {
        let query = "q".repeat(512);
        assert_eq!(query_target(&query), Some(search(&query)));
        assert!(track_for(&query, Id::new(1)).is_some());
    }
}
