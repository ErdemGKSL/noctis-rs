//! [`Track`], [`TrackInfo`], [`TrackId`] and [`TrackSource`]: what a guild's
//! queue plays.

use std::ffi::OsString;
use std::fmt;
use std::future::{Future, ready};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::sync::Arc;

use reqwest::Url;
use songbird::input::core::io::{MediaSource, ReadOnlySource};
use songbird::input::{AudioStream, AudioStreamError, ChildContainer, Compose, HttpRequest, Input};
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use crate::error::VoiceError;
use crate::media::is_discord_api;

/// The longest URL [`Track::url`] accepts, in bytes.
pub(crate) const MAX_URL_LEN: usize = 4096;

/// The longest title kept, in `char`s; a longer one is truncated.
pub(crate) const MAX_TITLE_CHARS: usize = 256;

/// Where a track's audio comes from.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackSource {
    /// A local file, opened only when the track becomes current.
    File(PathBuf),
    /// An `http`/`https` media URL, fetched through the App's media client
    /// only when the track becomes current.
    Url(String),
    /// A child process whose stdout is the audio, spawned only when the
    /// track becomes current (and killed when it stops). The program;
    /// its arguments are not shown.
    Command(OsString),
}

/// One track to enqueue: a source checked at construction, plus an optional
/// title and requester.
///
/// Nothing is opened, fetched or spawned until the track becomes the
/// queue's current track. The demux formats are the `noctis-voice`
/// features `ogg`, `mkv` (Matroska and `WebM`), `mp3`, `aac`, `isomp4`, `flac` and `wav`
/// (all on with the facade's `voice` feature); Opus is always decodable.
///
/// ```
/// use noctis_voice::{Track, TrackSource};
///
/// let track = Track::url("https://cdn.example.com/a.ogg")?.title("Intro");
/// assert!(matches!(track.source(), TrackSource::Url(_)));
/// assert!(Track::url("https://discord.com/api/v10/users/@me").is_err());
/// assert!(Track::file("").is_err());
/// let piped = Track::command("yt-dlp", ["-o", "-", "--", "https://example.com/v"])?;
/// assert!(matches!(piped.source(), TrackSource::Command(_)));
/// # Ok::<(), noctis_voice::VoiceError>(())
/// ```
#[derive(Debug, Clone)]
pub struct Track {
    source: TrackSource,
    args: Vec<OsString>,
    title: Option<String>,
    requested_by: Option<Id<UserMarker>>,
}

impl Track {
    fn new(source: TrackSource, args: Vec<OsString>) -> Self {
        Self {
            source,
            args,
            title: None,
            requested_by: None,
        }
    }

    /// A local file track; the file is opened when the track becomes
    /// current (a missing file is then a `TrackError`).
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] for an empty path.
    pub fn file(path: impl Into<PathBuf>) -> Result<Self, VoiceError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(invalid("the file path is empty"));
        }
        Ok(Self::new(TrackSource::File(path), Vec::new()))
    }

    /// An `http`/`https` media URL, fetched with the App's media client
    /// (connect timeout, redirect cap, no proxy, no bot token or Noctis
    /// headers).
    ///
    /// The media client fetches any public or private host (loopback,
    /// `10.0.0.0/8`, `169.254.169.254`, ...), directly or through
    /// redirects: a bot that takes URLs from users should allow-list hosts
    /// (for example only `cdn.discordapp.com` attachments) before calling
    /// this.
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] for a URL that is empty, malformed,
    /// longer than 4096 bytes, not `http`/`https`, carries credentials
    /// (`user:pass@`), or points at Discord's API (`discord.com/api`,
    /// `discordapp.com/api` or a subdomain's; the first path segment is
    /// percent-decoded once, so `%61pi` counts as `api`).
    pub fn url(url: impl AsRef<str>) -> Result<Self, VoiceError> {
        let url = check_url(url.as_ref())?;
        Ok(Self::new(TrackSource::Url(url.into()), Vec::new()))
    }

    /// A child process whose stdout is the audio (`yt-dlp -o - -- URL`,
    /// `ffmpeg … -f opus -`). The args are passed verbatim with no shell,
    /// but the program still parses them as options, so put `--` before
    /// user input. It is spawned when the track becomes current,
    /// with stdin and stderr closed, and killed and reaped when the track
    /// ends, is skipped or stopped, or the call is left.
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] for an empty program.
    pub fn command<I>(program: impl Into<OsString>, args: I) -> Result<Self, VoiceError>
    where
        I: IntoIterator,
        I::Item: Into<OsString>,
    {
        let program = program.into();
        if program.is_empty() {
            return Err(invalid("the command program is empty"));
        }
        Ok(Self::new(
            TrackSource::Command(program),
            args.into_iter().map(Into::into).collect(),
        ))
    }

    /// Sets the title (at most 256 `char`s; a longer one is truncated, an
    /// empty one clears it).
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        let mut title = title.into();
        if let Some((cut, _)) = title.char_indices().nth(MAX_TITLE_CHARS) {
            title.truncate(cut);
        }
        self.title = (!title.is_empty()).then_some(title);
        self
    }

    /// Sets the user who requested the track.
    #[must_use]
    pub fn requested_by(mut self, user: Id<UserMarker>) -> Self {
        self.requested_by = Some(user);
        self
    }

    /// The source.
    pub fn source(&self) -> &TrackSource {
        &self.source
    }

    /// The enqueued description with `id`.
    pub(crate) fn into_info(self, id: TrackId) -> TrackInfo {
        TrackInfo(Arc::new(InfoInner { id, track: self }))
    }
}

/// An [`VoiceError::InvalidInput`] with `reason`.
fn invalid(reason: impl Into<String>) -> VoiceError {
    VoiceError::InvalidInput {
        reason: reason.into(),
    }
}

/// Parses and checks a media URL.
fn check_url(raw: &str) -> Result<Url, VoiceError> {
    if raw.len() > MAX_URL_LEN {
        return Err(invalid(format!(
            "the URL is {} bytes long: at most {MAX_URL_LEN}",
            raw.len()
        )));
    }
    let url = Url::parse(raw).map_err(|e| invalid(format!("the URL is malformed: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid(format!(
            "the URL scheme `{}` is not http or https",
            url.scheme()
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid("the URL has no host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "the URL carries credentials (`user:pass@`); they are never sent",
        ));
    }
    if is_discord_api(&url) {
        return Err(invalid(
            "the URL points at Discord's API; media is never fetched from it",
        ));
    }
    Ok(url)
}

/// A track's id: unique per App (every enqueue gets a new one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(u64);

impl TrackId {
    pub(crate) const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// An enqueued track: what [`CallHandle::enqueue`](crate::CallHandle::enqueue),
/// [`now_playing`](crate::CallHandle::now_playing),
/// [`list`](crate::CallHandle::list) and the track events carry.
///
/// Cheap to clone (one `Arc`); two infos are equal when their ids are.
#[derive(Clone)]
pub struct TrackInfo(Arc<InfoInner>);

struct InfoInner {
    id: TrackId,
    track: Track,
}

impl TrackInfo {
    /// The id, unique per App.
    pub fn id(&self) -> TrackId {
        self.0.id
    }

    /// The title, if one was set.
    pub fn title(&self) -> Option<&str> {
        self.0.track.title.as_deref()
    }

    /// Where the audio comes from.
    pub fn source(&self) -> &TrackSource {
        &self.0.track.source
    }

    /// Who requested it, if set.
    pub fn requested_by(&self) -> Option<Id<UserMarker>> {
        self.0.track.requested_by
    }

    /// The lazy Songbird input of this track: nothing is opened, fetched or
    /// spawned until the driver creates it.
    pub(crate) fn input(&self, media: &reqwest::Client) -> Input {
        match &self.0.track.source {
            TrackSource::File(path) => songbird::input::File::new(path.clone()).into(),
            TrackSource::Url(url) => HttpRequest::new(media.clone(), url.clone()).into(),
            TrackSource::Command(program) => Input::Lazy(Box::new(ChildCompose {
                program: program.clone(),
                args: self.0.track.args.clone(),
            })),
        }
    }
}

impl PartialEq for TrackInfo {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for TrackInfo {}

impl fmt::Debug for TrackInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrackInfo")
            .field("id", &self.0.id)
            .field("title", &self.0.track.title)
            .field("source", &self.0.track.source)
            .field("requested_by", &self.0.track.requested_by)
            .finish()
    }
}

/// A lazily spawned child process: stdin and stderr closed, stdout the
/// audio. Its `ChildContainer` kills and reaps the child when the driver
/// drops the input.
pub(crate) struct ChildCompose {
    program: OsString,
    args: Vec<OsString>,
}

impl ChildCompose {
    fn spawn(&self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;
        Ok(AudioStream {
            input: Box::new(ReadOnlySource::new(ChildContainer::from(child))),
        })
    }
}

/// Written out as `#[async_trait]` expands it (no `async-trait`
/// dependency); creation is synchronous (`should_create_async` is `false`,
/// so Songbird runs it on its blocking pool).
impl Compose for ChildCompose {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        self.spawn()
    }

    fn create_async<'life0, 'async_trait>(
        &'life0 mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(ready(self.spawn()))
    }

    fn should_create_async(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
