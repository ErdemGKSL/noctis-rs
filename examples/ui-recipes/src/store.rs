//! The recipes' data: a read-only item catalog and two in-memory stores, registered
//! with `AppBuilder::state` and read in handlers with `ctx.state::<T>()`.

// recipe: store
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, RoleMarker};

/// The longest item name the catalog holds; it bounds a catalog row's width, which
/// `pagination::ROW_MAX_CHARS` sizes pages from.
pub const ITEM_NAME_MAX: usize = 100;
/// The most ticket systems a guild can have: the switcher holds at most 25 options.
pub const MAX_SYSTEMS: usize = 25;

/// Everything `configure` registers as App state.
#[derive(Debug, Clone, Default)]
pub struct Data {
    /// The list the pagination recipe pages through.
    pub catalog: Catalog,
    /// What the step flow saves.
    pub setup: SetupStore,
    /// What the settings screen edits.
    pub tickets: TicketStore,
}

impl Data {
    /// A 300-item catalog and empty stores.
    pub fn sample() -> Self {
        Self {
            catalog: Catalog::sample(300),
            ..Self::default()
        }
    }
}

/// A read-only list of item names, each at most [`ITEM_NAME_MAX`] characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog(pub Vec<String>);

impl Catalog {
    /// `n` deterministic names (`Item 001 …`) of varying length.
    pub fn sample(n: usize) -> Self {
        const FILLER: &str = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do \
                              eiusmod tempor incididunt ut labore et dolore magna aliqua";
        Self(
            (1..=n)
                .map(|i| {
                    let mut name = format!("Item {i:03} {}", &FILLER[..(i * 7) % FILLER.len()]);
                    name.truncate(ITEM_NAME_MAX); // ASCII only, so any index is a char boundary
                    name.trim_end().to_owned()
                })
                .collect(),
        )
    }
}

// The stores stand in for the bot's database: a real bot keeps this data in its own
// storage behind a port or a `State<T>` handle, and every method here becomes a query.

/// A guild's saved setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupConfig {
    /// Where the bot posts its logs.
    pub log_channel: Id<ChannelMarker>,
    /// Who moderates.
    pub mod_role: Id<RoleMarker>,
}

/// Saved setups by guild (a cheap clone of one shared map).
#[derive(Debug, Clone, Default)]
pub struct SetupStore(Arc<Mutex<BTreeMap<Id<GuildMarker>, SetupConfig>>>);

impl SetupStore {
    /// Saves `config` for `guild`, replacing an older one.
    pub fn save(&self, guild: Id<GuildMarker>, config: SetupConfig) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(guild, config);
    }

    /// The setup saved for `guild`, if any.
    pub fn get(&self, guild: Id<GuildMarker>) -> Option<SetupConfig> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&guild)
            .copied()
    }
}

/// One ticket system of a guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketSystem {
    /// Unique across guilds; the id every settings component carries.
    pub id: u32,
    /// Shown in the switcher and the name Section.
    pub name: String,
    /// Whether it takes new tickets.
    pub enabled: bool,
    /// The roles that handle its tickets.
    pub roles: Vec<Id<RoleMarker>>,
}

#[derive(Debug, Default)]
struct Tickets {
    by_guild: BTreeMap<Id<GuildMarker>, Vec<TicketSystem>>,
    next_id: u32,
}

/// Ticket systems by guild (a cheap clone of one shared store).
#[derive(Debug, Clone, Default)]
pub struct TicketStore(Arc<Mutex<Tickets>>);

impl TicketStore {
    /// A store holding `systems` for `guild` (test fixtures).
    pub fn with(guild: Id<GuildMarker>, systems: Vec<TicketSystem>) -> Self {
        let next_id = systems.iter().map(|t| t.id).max().unwrap_or(0);
        let tickets = Tickets {
            by_guild: BTreeMap::from([(guild, systems)]),
            next_id,
        };
        Self(Arc::new(Mutex::new(tickets)))
    }

    /// `guild`'s systems, oldest first.
    pub fn systems(&self, guild: Id<GuildMarker>) -> Vec<TicketSystem> {
        self.lock()
            .by_guild
            .get(&guild)
            .cloned()
            .unwrap_or_default()
    }

    /// Adds a system to `guild` and returns its id; `None` at [`MAX_SYSTEMS`].
    pub fn add(&self, guild: Id<GuildMarker>) -> Option<u32> {
        let mut tickets = self.lock();
        let id = tickets.next_id.checked_add(1)?;
        let systems = tickets.by_guild.entry(guild).or_default();
        if systems.len() >= MAX_SYSTEMS {
            return None;
        }
        systems.push(TicketSystem {
            id,
            name: format!("Ticket system {id}"),
            enabled: true,
            roles: Vec::new(),
        });
        tickets.next_id = id;
        Some(id)
    }

    /// Renames `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn rename(&self, guild: Id<GuildMarker>, id: u32, name: &str) -> bool {
        self.edit(guild, id, |t| name.clone_into(&mut t.name))
    }

    /// Enables or disables `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn set_enabled(&self, guild: Id<GuildMarker>, id: u32, enabled: bool) -> bool {
        self.edit(guild, id, |t| t.enabled = enabled)
    }

    /// Sets the roles of `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn set_roles(&self, guild: Id<GuildMarker>, id: u32, roles: &[Id<RoleMarker>]) -> bool {
        self.edit(guild, id, |t| roles.clone_into(&mut t.roles))
    }

    /// Deletes `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn delete(&self, guild: Id<GuildMarker>, id: u32) -> bool {
        let mut tickets = self.lock();
        let Some(systems) = tickets.by_guild.get_mut(&guild) else {
            return false;
        };
        let before = systems.len();
        systems.retain(|t| t.id != id);
        systems.len() < before
    }

    fn edit(&self, guild: Id<GuildMarker>, id: u32, f: impl FnOnce(&mut TicketSystem)) -> bool {
        let mut tickets = self.lock();
        let found = tickets
            .by_guild
            .get_mut(&guild)
            .and_then(|systems| systems.iter_mut().find(|t| t.id == id));
        found.map(f).is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tickets> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A state type `configure` did not register.
#[derive(Debug)]
struct MissingState(&'static str);

impl fmt::Display for MissingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no `{}` state; register it with AppBuilder::state",
            self.0
        )
    }
}

impl std::error::Error for MissingState {}

/// The error for a missing `T` state: `ctx.state::<T>().ok_or_else(missing::<T>)?`.
pub fn missing<T>() -> HandlerError {
    HandlerError::new(MissingState(std::any::type_name::<T>()))
}
// recipe-end: store

#[cfg(test)]
mod tests {
    use super::*;

    const G: Id<GuildMarker> = Id::new(10);

    #[test]
    fn sample_names_are_deterministic_bounded_and_varied() {
        let catalog = Catalog::sample(300);
        assert_eq!(catalog, Catalog::sample(300));
        assert_eq!(catalog.0.len(), 300);
        assert!(catalog.0.iter().all(|n| n.len() <= ITEM_NAME_MAX));
        assert!(catalog.0[0].starts_with("Item 001"));
        let lengths: std::collections::BTreeSet<usize> =
            catalog.0.iter().map(String::len).collect();
        assert!(lengths.len() > 10, "{lengths:?}");
    }

    #[test]
    fn ticket_store_scopes_ids_to_their_guild() {
        let store = TicketStore::default();
        let id = store.add(G).unwrap_or_default();
        assert_eq!(store.systems(G)[0].name, format!("Ticket system {id}"));
        assert!(!store.rename(Id::new(11), id, "x"));
        assert!(store.rename(G, id, "x"));
        assert!(store.set_enabled(G, id, false));
        assert!(store.set_roles(G, id, &[Id::new(30)]));
        assert_eq!(
            store.systems(G),
            [TicketSystem {
                id,
                name: "x".to_owned(),
                enabled: false,
                roles: vec![Id::new(30)]
            }]
        );
        assert!(store.delete(G, id));
        assert!(!store.delete(G, id));
        assert!(!store.set_enabled(G, id, true));
    }

    #[test]
    fn ticket_store_stops_at_the_cap() {
        let store = TicketStore::default();
        for _ in 0..MAX_SYSTEMS {
            assert!(store.add(G).is_some());
        }
        assert_eq!(store.add(G), None);
        assert_eq!(store.systems(G).len(), MAX_SYSTEMS);
    }

    #[test]
    fn missing_state_names_the_type() {
        let err = missing::<Catalog>();
        assert!(err.to_string().contains("Catalog"), "{err}");
    }
}
