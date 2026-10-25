//! [`VoiceConfig`]: the settings of the App's voice connections.

use std::ops::RangeInclusive;
use std::time::Duration;

/// The allowed `join_timeout` range.
pub(crate) const JOIN_TIMEOUT_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(1)..=Duration::from_mins(1);

/// The allowed `max_queue` range.
pub(crate) const MAX_QUEUE_RANGE: RangeInclusive<usize> = 1..=10_000;

/// The allowed `idle_leave` range (when set).
pub(crate) const IDLE_LEAVE_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(1)..=Duration::from_hours(24);

/// The allowed `media_connect_timeout` range.
pub(crate) const MEDIA_CONNECT_TIMEOUT_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(1)..=Duration::from_mins(1);

/// The largest `media_max_redirects`.
pub(crate) const MAX_REDIRECTS: u8 = 10;

/// The allowed `max_bitrate` range (when set), in bits per second: Discord's
/// voice channel bitrates, boosted guilds and stage channels included.
pub(crate) const MAX_BITRATE_RANGE: RangeInclusive<u32> = 8_000..=384_000;

/// The settings of the App's voice connections, passed to
/// [`AppBuilderVoiceExt::voice`](crate::AppBuilderVoiceExt::voice).
///
/// ```
/// use std::time::Duration;
///
/// use noctis_voice::VoiceConfig;
///
/// let config = VoiceConfig::default()
///     .join_timeout(Duration::from_secs(5))
///     .max_queue(200)
///     .idle_leave(Some(Duration::from_mins(5)))
///     .max_bitrate(Some(96_000));
/// assert_eq!(config.get_join_timeout(), Duration::from_secs(5));
/// assert!(config.is_self_deaf());
/// assert_eq!(config.get_max_queue(), 200);
/// assert_eq!(config.get_idle_leave(), Some(Duration::from_mins(5)));
/// assert_eq!(config.get_media_max_redirects(), 5);
/// assert_eq!(config.get_max_bitrate(), Some(96_000));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceConfig {
    join_timeout: Duration,
    self_deaf: bool,
    max_queue: usize,
    idle_leave: Option<Duration>,
    media_connect_timeout: Duration,
    media_max_redirects: u8,
    max_bitrate: Option<u32>,
    /// The recording driver (feature `testkit`).
    #[cfg(feature = "testkit")]
    recorder: Option<crate::recording::VoiceRecorder>,
}

impl Default for VoiceConfig {
    /// A 10 s join timeout; the bot joins self-deafened; queues hold at
    /// most 1000 tracks; no idle leave; media fetches connect within 10 s
    /// and follow at most 5 redirects; the encoder follows the channel's
    /// bitrate with no cap.
    fn default() -> Self {
        Self {
            join_timeout: Duration::from_secs(10),
            self_deaf: true,
            max_queue: 1000,
            idle_leave: None,
            media_connect_timeout: Duration::from_secs(10),
            media_max_redirects: 5,
            max_bitrate: None,
            #[cfg(feature = "testkit")]
            recorder: None,
        }
    }
}

impl VoiceConfig {
    /// The default config.
    pub fn new() -> Self {
        Self::default()
    }

    /// How long [`Voice::join`](crate::Voice::join) waits for Discord and
    /// the driver: 1 s..=60 s (default 10 s); another value is a build
    /// violation.
    #[must_use]
    pub const fn join_timeout(mut self, timeout: Duration) -> Self {
        self.join_timeout = timeout;
        self
    }

    /// Whether the bot joins self-deafened (default `true`: it does not
    /// receive audio).
    #[must_use]
    pub const fn self_deaf(mut self, deaf: bool) -> Self {
        self.self_deaf = deaf;
        self
    }

    /// The most tracks a guild's queue holds, the current one included:
    /// 1..=10000 (default 1000); another value is a build violation. An
    /// enqueue beyond it is [`VoiceError::QueueFull`](crate::VoiceError::QueueFull).
    #[must_use]
    pub const fn max_queue(mut self, max: usize) -> Self {
        self.max_queue = max;
        self
    }

    /// Leaves a connected call that stayed idle (no current track, or the
    /// current one paused) this long: `None` (the default) never leaves;
    /// `Some` takes 1 s..=24 h, another value is a build violation.
    #[must_use]
    pub const fn idle_leave(mut self, after: Option<Duration>) -> Self {
        self.idle_leave = after;
        self
    }

    /// How long a media fetch of a URL track may take to connect:
    /// 1 s..=60 s (default 10 s); another value is a build violation.
    #[must_use]
    pub const fn media_connect_timeout(mut self, timeout: Duration) -> Self {
        self.media_connect_timeout = timeout;
        self
    }

    /// How many redirects a media fetch follows: 0..=10 (default 5; 0
    /// follows none); another value is a build violation.
    #[must_use]
    pub const fn media_max_redirects(mut self, max: u8) -> Self {
        self.media_max_redirects = max;
        self
    }

    /// Caps the Opus encoder's bitrate, in bits per second. The encoder
    /// follows the joined channel's bitrate (64 kbps when the channel is not
    /// in the Discord cache); `Some(max)` caps it at `max`, which takes
    /// 8000..=384000, another value is a build violation. `None` (the
    /// default) follows the channel. Passthrough frames keep the bitrate
    /// they were encoded at.
    #[must_use]
    pub const fn max_bitrate(mut self, max: Option<u32>) -> Self {
        self.max_bitrate = max;
        self
    }

    /// The encoder bitrate cap, if any.
    pub const fn get_max_bitrate(&self) -> Option<u32> {
        self.max_bitrate
    }

    /// The most tracks a queue holds.
    pub const fn get_max_queue(&self) -> usize {
        self.max_queue
    }

    /// The idle leave delay, if any.
    pub const fn get_idle_leave(&self) -> Option<Duration> {
        self.idle_leave
    }

    /// The media connect timeout.
    pub const fn get_media_connect_timeout(&self) -> Duration {
        self.media_connect_timeout
    }

    /// The media redirect cap.
    pub const fn get_media_max_redirects(&self) -> u8 {
        self.media_max_redirects
    }

    /// The join timeout.
    pub const fn get_join_timeout(&self) -> Duration {
        self.join_timeout
    }

    /// Whether the bot joins self-deafened.
    pub const fn is_self_deaf(&self) -> bool {
        self.self_deaf
    }

    /// The default config with the recording driver (feature `testkit`,
    /// which only `noctis`'s `testkit` feature turns on; tests only).
    ///
    /// A join sends op 4 through the gateway as usual but resolves once
    /// Discord's `VOICE_STATE_UPDATE` and `VOICE_SERVER_UPDATE` arrived,
    /// without connecting the voice driver (no UDP, no voice WebSocket). Every
    /// queue plays on the [`VoiceRecorder`](crate::VoiceRecorder) that
    /// [`recorder`](Self::recorder) returns: it records the queue's commands
    /// and track events and never opens a file, fetches a URL or spawns a
    /// process; the test reports a track started, finished or failed. The
    /// builder setters keep the recorder.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use noctis_voice::VoiceConfig;
    ///
    /// let config = VoiceConfig::recording().join_timeout(Duration::from_secs(2));
    /// assert!(config.recorder().is_some());
    /// assert_eq!(config, config.clone());
    /// assert_ne!(config, VoiceConfig::recording());
    /// assert_ne!(config, VoiceConfig::default());
    /// ```
    #[cfg(feature = "testkit")]
    pub fn recording() -> Self {
        Self {
            recorder: Some(crate::recording::VoiceRecorder::new()),
            ..Self::default()
        }
    }

    /// The recorder of a [`recording`](Self::recording) config; `None`
    /// for any other config. Clones share one record.
    #[cfg(feature = "testkit")]
    pub fn recorder(&self) -> Option<crate::recording::VoiceRecorder> {
        self.recorder.clone()
    }

    /// Whether joins skip the voice driver (the recording driver).
    #[cfg_attr(
        not(feature = "testkit"),
        expect(
            clippy::unused_self,
            reason = "only the recording driver reads the config"
        )
    )]
    pub(crate) fn gateway_only(&self) -> bool {
        #[cfg(feature = "testkit")]
        return self.recorder.is_some();
        #[cfg(not(feature = "testkit"))]
        false
    }

    /// One message per out-of-range setting.
    pub(crate) fn violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !JOIN_TIMEOUT_RANGE.contains(&self.join_timeout) {
            out.push(format!(
                "join_timeout {:?} is out of range: use 1 s..=60 s",
                self.join_timeout
            ));
        }
        if !MAX_QUEUE_RANGE.contains(&self.max_queue) {
            out.push(format!(
                "max_queue {} is out of range: use 1..=10000",
                self.max_queue
            ));
        }
        if let Some(after) = self.idle_leave
            && !IDLE_LEAVE_RANGE.contains(&after)
        {
            out.push(format!(
                "idle_leave {after:?} is out of range: use None or 1 s..=24 h"
            ));
        }
        if !MEDIA_CONNECT_TIMEOUT_RANGE.contains(&self.media_connect_timeout) {
            out.push(format!(
                "media_connect_timeout {:?} is out of range: use 1 s..=60 s",
                self.media_connect_timeout
            ));
        }
        if self.media_max_redirects > MAX_REDIRECTS {
            out.push(format!(
                "media_max_redirects {} is out of range: use 0..=10",
                self.media_max_redirects
            ));
        }
        if let Some(max) = self.max_bitrate
            && !MAX_BITRATE_RANGE.contains(&max)
        {
            out.push(format!(
                "max_bitrate {max} is out of range: use None or 8000..=384000"
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let config = VoiceConfig::new();
        assert_eq!(config, VoiceConfig::default());
        assert_eq!(config.get_join_timeout(), Duration::from_secs(10));
        assert!(config.is_self_deaf());
        assert!(!config.self_deaf(false).is_self_deaf());
    }

    #[test]
    fn join_timeout_bounds() {
        for (timeout, ok) in [
            (Duration::ZERO, false),
            (Duration::from_millis(999), false),
            (Duration::from_secs(1), true),
            (Duration::from_mins(1), true),
            (Duration::from_mins(1) + Duration::from_nanos(1), false),
        ] {
            let violations = VoiceConfig::default().join_timeout(timeout).violations();
            assert_eq!(violations.is_empty(), ok, "{timeout:?}");
            if !ok {
                assert!(violations[0].contains("1 s..=60 s"), "{violations:?}");
            }
        }
    }

    #[test]
    fn queue_and_media_bounds() {
        let check = |config: VoiceConfig, ok: bool, range: &str| {
            let violations = config.violations();
            assert_eq!(violations.is_empty(), ok, "{config:?}: {violations:?}");
            if !ok {
                assert!(violations[0].contains(range), "{violations:?}");
            }
        };
        let base = VoiceConfig::default;
        assert_eq!(base().get_max_queue(), 1000);
        assert_eq!(base().get_idle_leave(), None);
        assert_eq!(base().get_media_connect_timeout(), Duration::from_secs(10));
        assert_eq!(base().get_media_max_redirects(), 5);
        for (max, ok) in [(0, false), (1, true), (10_000, true), (10_001, false)] {
            check(base().max_queue(max), ok, "1..=10000");
        }
        let day = Duration::from_hours(24);
        for (after, ok) in [
            (None, true),
            (Some(Duration::from_millis(999)), false),
            (Some(Duration::from_secs(1)), true),
            (Some(day), true),
            (Some(day + Duration::from_nanos(1)), false),
        ] {
            check(base().idle_leave(after), ok, "1 s..=24 h");
        }
        for (timeout, ok) in [
            (Duration::ZERO, false),
            (Duration::from_millis(999), false),
            (Duration::from_secs(1), true),
            (Duration::from_mins(1), true),
            (Duration::from_mins(1) + Duration::from_nanos(1), false),
        ] {
            check(base().media_connect_timeout(timeout), ok, "1 s..=60 s");
        }
        for (max, ok) in [(0, true), (10, true), (11, false)] {
            check(base().media_max_redirects(max), ok, "0..=10");
        }
    }

    #[test]
    fn max_bitrate_bounds() {
        assert_eq!(VoiceConfig::default().get_max_bitrate(), None);
        for (max, ok) in [
            (None, true),
            (Some(8_000), true),
            (Some(96_000), true),
            (Some(384_000), true),
            (Some(0), false),
            (Some(7_999), false),
            (Some(384_001), false),
            (Some(u32::MAX), false),
        ] {
            let config = VoiceConfig::default().max_bitrate(max);
            assert_eq!(config.get_max_bitrate(), max);
            let violations = config.violations();
            assert_eq!(violations.is_empty(), ok, "{max:?}: {violations:?}");
            if !ok {
                assert!(violations[0].contains("8000..=384000"), "{violations:?}");
            }
        }
    }

    /// The config rows of the recording driver: identity equality, the
    /// setters keep the recorder, the default has none.
    #[cfg(feature = "testkit")]
    #[test]
    fn recording_config() {
        assert!(VoiceConfig::default().recorder().is_none());
        assert!(!VoiceConfig::default().gateway_only());
        let config = VoiceConfig::recording();
        let recorder = config.recorder().unwrap_or_else(|| panic!("a recorder"));
        assert!(config.gateway_only());
        assert_eq!(config, config.clone());
        assert_ne!(config, VoiceConfig::recording());
        assert_ne!(config, VoiceConfig::default());
        let tuned = config
            .clone()
            .join_timeout(Duration::from_secs(2))
            .self_deaf(false)
            .max_queue(5)
            .idle_leave(Some(Duration::from_secs(3)))
            .media_connect_timeout(Duration::from_secs(4))
            .media_max_redirects(1)
            .max_bitrate(Some(96_000));
        assert_eq!(tuned.recorder(), Some(recorder.clone()));
        assert!(tuned.violations().is_empty());
        assert_ne!(Some(recorder), VoiceConfig::recording().recorder());
        assert!(format!("{config:?}").contains("VoiceRecorder { guilds: 0 }"));
    }
}
