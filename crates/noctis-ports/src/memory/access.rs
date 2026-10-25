//! [`GuildAccess`]: the in-memory allow or deny list.

use std::collections::HashSet;
use std::fmt;
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::GuildAccessList;

/// The listed guild ids.
type Guilds = HashSet<Id<GuildMarker>>; // keyed-map: holds only the guild ids the bot author lists

/// A guild allow list or deny list for white-label Apps.
///
/// [`GuildAccess::allow`] serves only the listed guilds (an empty allow list
/// serves no guild); [`GuildAccess::deny`] serves every guild except the
/// listed ones (an empty deny list serves every guild). Interactions and
/// events without a guild (DMs) are always served: the list is not
/// consulted for them.
///
/// The list can change while the App runs: share it through an `Arc` and
/// call [`GuildAccess::insert`] or [`GuildAccess::remove`]; the next
/// interaction sees the change. Its mode (allow or deny) is fixed.
///
/// ```
/// use std::sync::Arc;
///
/// use noctis_ports::{GuildAccessList, memory::GuildAccess};
/// use twilight_model::id::Id;
///
/// let customer = Id::new(10);
/// let access = Arc::new(GuildAccess::allow([customer]));
/// assert!(access.allows(customer));
/// assert!(!access.allows(Id::new(11)));
///
/// assert!(access.insert(Id::new(11)));
/// assert!(access.allows(Id::new(11)));
/// assert!(access.remove(Id::new(11)));
/// assert!(!access.remove(Id::new(11)));
///
/// let main = GuildAccess::deny([customer]);
/// assert!(!main.allows(customer));
/// assert!(main.allows(Id::new(11)));
/// ```
pub struct GuildAccess {
    allow_list: bool,
    guilds: RwLock<Guilds>,
}

impl GuildAccess {
    /// Serves only `guilds` (duplicates are ignored).
    #[must_use]
    pub fn allow(guilds: impl IntoIterator<Item = Id<GuildMarker>>) -> Self {
        Self::with_mode(true, guilds)
    }

    /// Serves every guild except `guilds` (duplicates are ignored).
    #[must_use]
    pub fn deny(guilds: impl IntoIterator<Item = Id<GuildMarker>>) -> Self {
        Self::with_mode(false, guilds)
    }

    fn with_mode(allow_list: bool, guilds: impl IntoIterator<Item = Id<GuildMarker>>) -> Self {
        Self {
            allow_list,
            guilds: RwLock::new(guilds.into_iter().collect()),
        }
    }

    /// Adds `guild` to the list: in allow mode it is then served, in deny
    /// mode it is not. Returns whether it was newly added.
    pub fn insert(&self, guild: Id<GuildMarker>) -> bool {
        self.write().insert(guild)
    }

    /// Removes `guild` from the list. Returns whether it was listed.
    pub fn remove(&self, guild: Id<GuildMarker>) -> bool {
        self.write().remove(&guild)
    }

    /// Whether `guild` is listed (whatever the mode).
    #[must_use]
    pub fn contains(&self, guild: Id<GuildMarker>) -> bool {
        self.read().contains(&guild)
    }

    /// `true` for an allow list, `false` for a deny list.
    #[must_use]
    pub fn is_allow_list(&self) -> bool {
        self.allow_list
    }

    // A panic while the lock is held cannot leave the set half-updated
    // (every operation is one `HashSet` call), so a poisoned lock is
    // recovered.
    fn read(&self) -> RwLockReadGuard<'_, Guilds> {
        self.guilds.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Guilds> {
        self.guilds.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl GuildAccessList for GuildAccess {
    fn allows(&self, guild: Id<GuildMarker>) -> bool {
        self.contains(guild) == self.allow_list
    }
}

impl fmt::Debug for GuildAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut guilds: Vec<_> = self.read().iter().copied().collect();
        guilds.sort_unstable();
        f.debug_struct("GuildAccess")
            .field("mode", &if self.allow_list { "allow" } else { "deny" })
            .field("guilds", &guilds)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Id<GuildMarker> = Id::new(1);
    const H: Id<GuildMarker> = Id::new(2);

    #[test]
    fn allow_list_matrix() {
        let access = GuildAccess::allow([G]);
        assert!(access.is_allow_list());
        assert!(access.allows(G));
        assert!(!access.allows(H));

        let empty = GuildAccess::allow([]);
        assert!(!empty.allows(G));
        assert!(!empty.allows(H));

        let duplicates = GuildAccess::allow([G, G]);
        assert!(duplicates.allows(G));
        assert!(!duplicates.allows(H));
        assert!(duplicates.remove(G));
        assert!(!duplicates.allows(G));
    }

    #[test]
    fn deny_list_matrix() {
        let access = GuildAccess::deny([G]);
        assert!(!access.is_allow_list());
        assert!(!access.allows(G));
        assert!(access.allows(H));

        let empty = GuildAccess::deny([]);
        assert!(empty.allows(G));
        assert!(empty.allows(H));
    }

    #[test]
    fn runtime_updates() {
        let access = std::sync::Arc::new(GuildAccess::allow([G]));
        let shared: std::sync::Arc<dyn GuildAccessList> = access.clone();
        assert!(!shared.allows(H));
        assert!(access.insert(H));
        assert!(!access.insert(H));
        assert!(shared.allows(H));
        assert!(access.contains(H));
        assert!(access.remove(H));
        assert!(!shared.allows(H));
        assert!(!access.remove(H));
        assert!(!access.contains(H));
    }

    #[test]
    fn debug_lists_mode_and_sorted_guilds() {
        let access = GuildAccess::deny([H, G]);
        assert_eq!(
            format!("{access:?}"),
            "GuildAccess { mode: \"deny\", guilds: [Id<GuildMarker>(1), Id<GuildMarker>(2)] }"
        );
    }

    #[test]
    fn poisoned_lock_is_recovered() {
        let access = GuildAccess::allow([G]);
        let joined = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = access.guilds.write();
            panic!("poison the lock");
        }));
        assert!(joined.is_err());
        assert!(access.guilds.is_poisoned());
        assert!(access.allows(G));
        assert!(access.insert(H));
        assert!(access.allows(H));
    }
}
