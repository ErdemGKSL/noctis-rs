//! Ranks: XP per message and a level-up card posted from the event handler
//! (`events.rs`), and `/leaderboard`, two rendered images in one message: the server's
//! activity as a line chart and the top members with their avatars (`handlers.rs`).
//!
//! Layout: `mod.rs` (the Module, the storage keys and the XP rules), `view.rs` (the
//! three Templates and the render functions), `handlers.rs` (the Command),
//! `events.rs` (the gateway event handler).

mod events;
mod handlers;
pub mod view;

use std::time::{SystemTime, UNIX_EPOCH};

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::prelude::*;

pub use handlers::Leaderboard;

/// The Module, declared here; `events.rs` and `handlers.rs` name it with `module =
/// "rank"`, and the dashboard's Modules tab switches it on and off per server.
#[noctis::module]
fn module() -> Module {
    Module::new("rank")
}

/// XP needed per level: every message worth XP adds 1.
pub const XP_PER_LEVEL: u64 = 100;
/// Days the activity chart shows.
pub const ACTIVITY_DAYS: u64 = 14;
/// Members `/leaderboard` reads at most. The Kv has no scan, so members are numbered
/// as they earn their first XP (an atomic `incr`), and the board reads those slots.
/// Past this many members, keep XP in a database Adapter and query the top rows there.
pub const MAX_RANKED_MEMBERS: u64 = 250;

/// The level `xp` has just reached, when this point of XP completed one.
pub fn level_reached(xp: u64) -> Option<u64> {
    (xp > 0 && xp.is_multiple_of(XP_PER_LEVEL)).then_some(xp / XP_PER_LEVEL)
}

/// Today as whole days since the Unix epoch (UTC), the activity chart's x axis.
pub fn today() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}

pub(crate) fn xp_key(guild: Id<GuildMarker>, user: Id<UserMarker>) -> String {
    format!("xp:{guild}:{user}")
}

/// How many members have earned XP in `guild`; each one's slot is `member_key(guild, n)`.
pub(crate) fn member_count_key(guild: Id<GuildMarker>) -> String {
    format!("rank:members:{guild}")
}

pub(crate) fn member_key(guild: Id<GuildMarker>, slot: u64) -> String {
    format!("rank:member:{guild}:{slot}")
}

/// Messages worth XP in `guild` on `day` (see [`today`]).
pub(crate) fn day_key(guild: Id<GuildMarker>, day: u64) -> String {
    format!("rank:day:{guild}:{day}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_is_reached_on_each_hundredth_point() {
        assert_eq!(level_reached(0), None);
        assert_eq!(level_reached(99), None);
        assert_eq!(level_reached(100), Some(1));
        assert_eq!(level_reached(101), None);
        assert_eq!(level_reached(700), Some(7));
    }
}
