//! [`GuildAccessList`]: per-App guild allow and deny lists.

use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

/// Decides which guilds an App serves (white-label allow and deny lists).
pub trait GuildAccessList: Send + Sync + 'static {
    /// Whether the App serves `guild`.
    ///
    /// It is synchronous and runs on a pipeline stage that does no port I/O:
    /// never block. A remote-backed implementation keeps a local snapshot and
    /// refreshes it in the background.
    fn allows(&self, guild: Id<GuildMarker>) -> bool;
}
