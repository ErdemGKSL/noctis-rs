//! The one per-guild framework record (NFR-1, AD-25).
//!
//! Every piece of framework state the App keeps per guild lives in one
//! fixed-size, `Copy` [`GuildRecord`], stored in one [`GuildRecords`] map
//! bounded by the App's guild membership. A new per-guild value becomes a
//! field here (Story 13.3 adds the Module bitset), never a second map keyed
//! by guild id; the `memory_per_guild` bench measures what it costs.

use std::collections::HashMap;

use noctis_rules::Locale;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

/// The framework's state for one guild: its `preferred_locale`.
///
/// Fixed-size and `Copy` so its per-guild cost is known at compile time
/// (at most [`GUILD_RECORD_MAX_BYTES`]); Story 13.3 adds the Module bitset
/// as another `Copy` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuildRecord {
    /// The guild's `preferred_locale` as the gateway last announced it
    /// (or an interaction's `guild_locale` refreshed it).
    pub(crate) locale: Locale,
}

/// The size bound of one [`GuildRecord`] value; the map entry's key and
/// hash-table overhead are not included (the `memory_per_guild` bench
/// measures the whole per-guild cost against NFR-1's 1 KiB).
pub(crate) const GUILD_RECORD_MAX_BYTES: usize = 64;

const _: () = assert!(size_of::<GuildRecord>() <= GUILD_RECORD_MAX_BYTES);

/// The App's per-guild framework records.
pub(crate) type GuildRecords = HashMap<Id<GuildMarker>, GuildRecord>; // keyed-map: bounded by the App's guild membership (GUILD_CREATE inserts, GUILD_DELETE removes, interactions never insert)

#[cfg(test)]
mod tests {
    use super::{GUILD_RECORD_MAX_BYTES, GuildRecord};

    const fn assert_copy<T: Copy>() {}

    #[test]
    fn guild_record_is_copy_and_bounded() {
        const { assert_copy::<GuildRecord>() };
        assert!(size_of::<GuildRecord>() <= GUILD_RECORD_MAX_BYTES);
    }
}
