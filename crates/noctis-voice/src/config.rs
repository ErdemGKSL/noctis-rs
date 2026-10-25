//! [`VoiceConfig`]: the settings of the App's voice connections.

use std::ops::RangeInclusive;
use std::time::Duration;

/// The allowed `join_timeout` range.
pub(crate) const JOIN_TIMEOUT_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(1)..=Duration::from_mins(1);

/// The settings of the App's voice connections, passed to
/// [`AppBuilderVoiceExt::voice`](crate::AppBuilderVoiceExt::voice).
///
/// ```
/// use std::time::Duration;
///
/// use noctis_voice::VoiceConfig;
///
/// let config = VoiceConfig::default().join_timeout(Duration::from_secs(5));
/// assert_eq!(config.get_join_timeout(), Duration::from_secs(5));
/// assert!(config.is_self_deaf());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceConfig {
    join_timeout: Duration,
    self_deaf: bool,
}

impl Default for VoiceConfig {
    /// A 10 s join timeout; the bot joins self-deafened.
    fn default() -> Self {
        Self {
            join_timeout: Duration::from_secs(10),
            self_deaf: true,
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

    /// The join timeout.
    pub const fn get_join_timeout(&self) -> Duration {
        self.join_timeout
    }

    /// Whether the bot joins self-deafened.
    pub const fn is_self_deaf(&self) -> bool {
        self.self_deaf
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
}
