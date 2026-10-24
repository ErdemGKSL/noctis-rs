//! Runtime Module enablement (FR-45, AD-7, AD-23 stage 3): the per-App
//! [`Modules`] handle over the namespaced `ModuleStateStore`, its cached
//! `arc-swap` snapshot and the version poller.
//!
//! Each kept Module has one record under its id: whether it is enabled
//! globally and the guilds that override that. The snapshot holds a global
//! bitset over the Registry's kept Modules (pre-order) plus one override
//! bitset pair per guild that has an override; dispatch reads only the
//! snapshot.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use noctis_ports::{ModuleStateStore, PortError, decode_value, encode_value};
use serde::{Deserialize, Serialize};
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

#[cfg(test)]
mod tests;

/// Where a Module's state applies: everywhere, or in one guild.
///
/// Every [`Modules`] method takes `impl Into<ModuleScope>`, so a guild id,
/// an `Option` of one (`None` is [`Global`](Self::Global), as for DMs) or
/// the variant itself all work.
///
/// ```
/// use noctis_core::ModuleScope;
/// use twilight_model::id::Id;
///
/// let guild = Id::new(42);
/// assert_eq!(ModuleScope::from(guild), ModuleScope::Guild(guild));
/// assert_eq!(ModuleScope::from(None), ModuleScope::Global);
/// assert_eq!(ModuleScope::Guild(guild).to_string(), "guild 42");
/// assert_eq!(ModuleScope::Global.to_string(), "global");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleScope {
    /// Every guild without an override, and every DM.
    Global,
    /// One guild.
    Guild(Id<GuildMarker>),
}

impl From<Id<GuildMarker>> for ModuleScope {
    fn from(guild: Id<GuildMarker>) -> Self {
        Self::Guild(guild)
    }
}

impl From<Option<Id<GuildMarker>>> for ModuleScope {
    fn from(guild: Option<Id<GuildMarker>>) -> Self {
        guild.map_or(Self::Global, Self::Guild)
    }
}

impl fmt::Display for ModuleScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => f.write_str("global"),
            Self::Guild(guild) => write!(f, "guild {guild}"),
        }
    }
}

/// Why a [`Modules`] write or refresh failed.
///
/// ```
/// use noctis_core::ModuleStateError;
///
/// let e = ModuleStateError::UnknownModule { id: "nope".into() };
/// assert!(e.to_string().starts_with("unknown Module `nope`"));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModuleStateError {
    /// The id names no Module the Registry keeps (exact, case-sensitive).
    #[error(
        "unknown Module `{id}`: pass the id of a mounted Module (as written in `Module::new(..)`) \
         that the Boot Flags select"
    )]
    UnknownModule {
        /// The id as passed.
        id: String,
    },
    /// The `ModuleStateStore` call failed (or the context has no App).
    #[error("the ModuleStateStore failed: {0}")]
    Port(#[from] PortError),
}

/// The stored state of one Module (schema byte 0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    /// Enabled globally.
    pub(crate) global: bool,
    /// The guild overrides, sorted by id, unique.
    pub(crate) guilds: Vec<(u64, bool)>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            global: true,
            guilds: Vec::new(),
        }
    }
}

impl Record {
    /// Decodes the stored bytes of `module`: another schema reads as the
    /// default, malformed bytes too, with one `warn`. Duplicate guild
    /// entries keep the first stored entry for that guild.
    fn decode(module: &str, raw: &[u8]) -> Self {
        match decode_value::<Self>(0, raw) {
            Ok(Some(mut record)) => {
                record.guilds.sort_by_key(|&(guild, _)| guild);
                record.guilds.dedup_by_key(|&mut (guild, _)| guild);
                record
            }
            Ok(None) => Self::default(),
            Err(error) => {
                tracing::warn!(
                    target: "noctis::modules",
                    module,
                    %error,
                    "malformed Module state record; reading it as enabled with no overrides"
                );
                Self::default()
            }
        }
    }

    /// Sets the Module's own state in `scope`.
    fn set(&mut self, scope: ModuleScope, enabled: bool) {
        match scope {
            ModuleScope::Global => self.global = enabled,
            ModuleScope::Guild(guild) => {
                let guild = guild.get();
                match self.guilds.binary_search_by_key(&guild, |&(g, _)| g) {
                    Ok(at) => self.guilds[at].1 = enabled,
                    Err(at) => self.guilds.insert(at, (guild, enabled)),
                }
            }
        }
    }

    /// Resets the Module's own state in `scope`.
    fn reset(&mut self, scope: ModuleScope) {
        match scope {
            ModuleScope::Global => self.global = true,
            ModuleScope::Guild(guild) => {
                let guild = guild.get();
                if let Ok(at) = self.guilds.binary_search_by_key(&guild, |&(g, _)| g) {
                    self.guilds.remove(at);
                }
            }
        }
    }

    /// The Module's own state in `scope`.
    fn own(&self, scope: ModuleScope) -> bool {
        match scope {
            ModuleScope::Global => self.global,
            ModuleScope::Guild(guild) => self
                .guilds
                .binary_search_by_key(&guild.get(), |&(g, _)| g)
                .map_or(self.global, |at| self.guilds[at].1),
        }
    }
}

/// The kept Modules of a Registry, in pre-order, with each one's path
/// (ancestors root first, then itself) as indices.
#[derive(Debug, Default)]
pub(crate) struct ModuleIndex {
    ids: Vec<&'static str>,
    by_id: HashMap<&'static str, u32>,
    paths: Vec<Box<[u32]>>,
}

impl ModuleIndex {
    /// Appends the Module `id` under `ancestors` (root first, each already
    /// pushed). A repeated id keeps its first entry.
    pub(crate) fn push(&mut self, id: &'static str, ancestors: &[&'static str]) {
        if self.by_id.contains_key(id) {
            return;
        }
        let Ok(index) = u32::try_from(self.ids.len()) else {
            return;
        };
        let path = ancestors
            .iter()
            .filter_map(|ancestor| self.by_id.get(ancestor).copied())
            .chain([index])
            .collect();
        self.ids.push(id);
        self.by_id.insert(id, index);
        self.paths.push(path);
    }

    /// The kept Module ids, in pre-order.
    pub(crate) fn ids(&self) -> &[&'static str] {
        &self.ids
    }

    /// The index of `id`, if kept.
    fn index(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).map(|&i| i as usize)
    }

    /// The path of the Module at `index`.
    #[cfg(test)]
    pub(crate) fn path(&self, id: &str) -> Option<&[u32]> {
        self.index(id).map(|i| &*self.paths[i])
    }

    fn words(&self) -> usize {
        self.ids.len().div_ceil(64)
    }
}

/// One guild's overrides: which Modules have one (`mask`) and its value.
#[derive(Debug, Clone)]
struct Override {
    mask: Box<[u64]>,
    value: Box<[u64]>,
}

/// The cached state every dispatch reads (the only swapped state, AD-7).
#[derive(Debug, Clone)]
struct Snapshot {
    /// The store version it was read at (0 before any refresh).
    version: u64,
    /// The global bit of every kept Module.
    global: Box<[u64]>,
    /// The overrides of every guild that has one.
    guilds: HashMap<u64, Override>,
}

const fn bit(index: usize) -> (usize, u64) {
    (index / 64, 1 << (index % 64))
}

fn set_bit(words: &mut [u64], index: usize, on: bool) {
    let (word, mask) = bit(index);
    if on {
        words[word] |= mask;
    } else {
        words[word] &= !mask;
    }
}

fn get_bit(words: &[u64], index: usize) -> bool {
    let (word, mask) = bit(index);
    words[word] & mask != 0
}

impl Snapshot {
    /// Every kept Module enabled, at `version`.
    fn enabled(index: &ModuleIndex, version: u64) -> Self {
        let mut global = vec![0; index.words()].into_boxed_slice();
        for i in 0..index.ids.len() {
            set_bit(&mut global, i, true);
        }
        Self {
            version,
            global,
            guilds: HashMap::new(),
        }
    }

    /// Writes `record` as the state of the Module at `index`.
    fn apply(&mut self, words: usize, index: usize, record: &Record) {
        set_bit(&mut self.global, index, record.global);
        self.guilds.retain(|_, o| {
            set_bit(&mut o.mask, index, false);
            o.mask.iter().any(|&w| w != 0)
        });
        for &(guild, enabled) in &record.guilds {
            let o = self.guilds.entry(guild).or_insert_with(|| Override {
                mask: vec![0; words].into_boxed_slice(),
                value: vec![0; words].into_boxed_slice(),
            });
            set_bit(&mut o.mask, index, true);
            set_bit(&mut o.value, index, enabled);
        }
    }

    /// The own state of the Module at `index` in `scope`.
    fn own(&self, index: usize, scope: ModuleScope) -> bool {
        if let ModuleScope::Guild(guild) = scope
            && let Some(o) = self.guilds.get(&guild.get())
            && get_bit(&o.mask, index)
        {
            return get_bit(&o.value, index);
        }
        get_bit(&self.global, index)
    }
}

/// The App's Module state: the namespaced store, the index, the snapshot
/// and the write mutex (behind [`AppShared`](crate::handler::AppShared)).
pub(crate) struct State {
    store: Arc<dyn ModuleStateStore>,
    index: Arc<ModuleIndex>,
    snapshot: ArcSwap<Snapshot>,
    write: tokio::sync::Mutex<()>,
}

impl State {
    /// The state over `store` and `index`: all enabled at version 0.
    pub(crate) fn new(store: Arc<dyn ModuleStateStore>, index: Arc<ModuleIndex>) -> Self {
        let snapshot = ArcSwap::from_pointee(Snapshot::enabled(&index, 0));
        Self {
            store,
            index,
            snapshot,
            write: tokio::sync::Mutex::new(()),
        }
    }

    /// Whether the Module at `index` and every ancestor are enabled in
    /// `scope`.
    fn enabled_at(&self, index: usize, scope: ModuleScope) -> bool {
        let snapshot = self.snapshot.load();
        self.index.paths[index]
            .iter()
            .all(|&i| snapshot.own(i as usize, scope))
    }

    /// Whether the Module `id` is enabled in `scope` (unknown → `false`).
    pub(crate) fn is_enabled(&self, id: &str, scope: ModuleScope) -> bool {
        self.index
            .index(id)
            .is_some_and(|i| self.enabled_at(i, scope))
    }

    /// Reads one record (a missing key is the default).
    async fn read(&self, id: &'static str) -> Result<Record, PortError> {
        Ok(self
            .store
            .get(id)
            .await?
            .map(|raw| Record::decode(id, &raw))
            .unwrap_or_default())
    }

    /// Reloads the whole snapshot: `version()`, then every record in
    /// pre-order, then the swap, all under the write mutex so no swap
    /// races a `write`.
    pub(crate) async fn refresh(&self) -> Result<(), PortError> {
        let _serialized = self.write.lock().await;
        let version = self.store.version().await?;
        let words = self.index.words();
        let mut next = Snapshot::enabled(&self.index, version);
        for (i, &id) in self.index.ids.iter().enumerate() {
            let record = self.read(id).await?;
            next.apply(words, i, &record);
        }
        self.snapshot.store(Arc::new(next));
        Ok(())
    }

    /// The snapshot's version.
    fn version(&self) -> u64 {
        self.snapshot.load().version
    }

    /// Reads, changes and (when changed) writes the record of `id`, then
    /// updates the snapshot from it.
    async fn write(
        &self,
        id: &str,
        scope: ModuleScope,
        change: impl FnOnce(&mut Record),
    ) -> Result<(), ModuleStateError> {
        let Some(index) = self.index.index(id) else {
            return Err(ModuleStateError::UnknownModule { id: id.to_owned() });
        };
        let id = self.index.ids[index];
        let _serialized = self.write.lock().await;
        let before = self.read(id).await?;
        let mut record = before.clone();
        change(&mut record);
        let changed = record != before;
        if changed {
            let bytes = encode_value(0, &record).map_err(PortError::from)?;
            self.store.put(id, bytes, None).await?;
        }
        let words = self.index.words();
        let mut next = Snapshot::clone(&self.snapshot.load());
        next.apply(words, index, &record);
        self.snapshot.store(Arc::new(next));
        if changed {
            tracing::info!(
                target: "noctis::modules",
                module = id,
                scope = %scope,
                enabled = record.own(scope),
                "Module state changed"
            );
        }
        Ok(())
    }
}

/// Polls the store's version every `interval` (the first tick one
/// interval after this call) and refreshes the snapshot when it moved;
/// runs until aborted. A failing streak warns once and keeps the last
/// snapshot; the recovery logs once.
pub(crate) fn poll(
    state: Arc<State>,
    interval: Duration,
) -> impl Future<Output = ()> + Send + 'static {
    let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    async move { poll_loop(&state, ticks).await }
}

async fn poll_loop(state: &State, mut ticks: tokio::time::Interval) {
    let mut failing = false;
    loop {
        ticks.tick().await;
        let result = match state.store.version().await {
            Ok(version) if version == state.version() => Ok(()),
            Ok(_) => state.refresh().await,
            Err(error) => Err(error),
        };
        // Only the first failure and the recovery of a streak log.
        match (result, failing) {
            (Ok(()), true) => {
                failing = false;
                tracing::info!(target: "noctis::modules", "the Module state is readable again");
            }
            (Err(error), false) => {
                failing = true;
                tracing::warn!(
                    target: "noctis::modules",
                    %error,
                    "could not read the Module state; keeping the last snapshot"
                );
            }
            (Ok(()), false) | (Err(_), true) => {}
        }
    }
}

/// The App's runtime Module switches, from `ctx.modules()` or
/// [`App::modules`](crate::App::modules).
///
/// A Module is enabled in a scope when it and every ancestor are: its own
/// state in `Guild(g)` is g's override if any, else its global state
/// (default enabled). DMs and guildless events use
/// [`ModuleScope::Global`]. Ids match the Registry's kept Modules exactly.
///
/// [`is_enabled`](Self::is_enabled) reads the cached snapshot (no I/O);
/// writes go through the App's `ModuleStateStore` (key = Module id) and
/// update this App's snapshot at once. Other Apps sharing the store see a
/// change within one `module_state_poll` (default 5 s). A toggle never
/// re-Publishes. Cheap to clone.
///
/// ```
/// use noctis_core::{CommandCtx, HandlerResult, ModuleScope};
///
/// async fn off(ctx: CommandCtx) -> HandlerResult<bool> {
///     let modules = ctx.modules();
///     modules.set_enabled("profile", ModuleScope::Global, false).await?;
///     Ok(modules.is_enabled("profile", ctx.guild_id()))
/// }
/// ```
#[derive(Clone)]
pub struct Modules {
    /// The App's state; `None` without an App.
    state: Option<Arc<State>>,
}

impl Modules {
    /// The handle of the App behind `app`, or a detached one.
    pub(crate) fn of(app: Option<&Arc<crate::handler::AppShared>>) -> Self {
        Self {
            state: app.map(|app| Arc::clone(&app.module_state)),
        }
    }

    fn state(&self) -> Result<&Arc<State>, PortError> {
        self.state
            .as_ref()
            .ok_or_else(|| PortError::unavailable("this context has no App"))
    }

    /// Whether the Module `module` and every ancestor are enabled in
    /// `scope`, from the cached snapshot (no port call). An unknown id is
    /// `false`; a context without an App answers `true`.
    pub fn is_enabled(&self, module: &str, scope: impl Into<ModuleScope>) -> bool {
        self.state
            .as_ref()
            .is_none_or(|state| state.is_enabled(module, scope.into()))
    }

    /// Sets the Module's own state in `scope`: its global state, or the
    /// guild's override. An unchanged record is not written.
    ///
    /// # Errors
    ///
    /// [`ModuleStateError::UnknownModule`] for an id the Registry does not
    /// keep (no port call), else [`ModuleStateError::Port`].
    pub async fn set_enabled(
        &self,
        module: &str,
        scope: impl Into<ModuleScope>,
        enabled: bool,
    ) -> Result<(), ModuleStateError> {
        let scope = scope.into();
        self.state()?
            .write(module, scope, |record| record.set(scope, enabled))
            .await
    }

    /// Resets the Module's own state in `scope`: removes the guild's
    /// override, or sets the global state back to enabled (overrides
    /// kept).
    ///
    /// # Errors
    ///
    /// As [`set_enabled`](Self::set_enabled).
    pub async fn reset(
        &self,
        module: &str,
        scope: impl Into<ModuleScope>,
    ) -> Result<(), ModuleStateError> {
        let scope = scope.into();
        self.state()?
            .write(module, scope, |record| record.reset(scope))
            .await
    }

    /// Reloads the whole snapshot from the store now.
    ///
    /// # Errors
    ///
    /// [`ModuleStateError::Port`]; the last snapshot is kept.
    pub async fn refresh(&self) -> Result<(), ModuleStateError> {
        Ok(self.state()?.refresh().await?)
    }

    /// The kept Module ids, in Registry pre-order (empty without an App).
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.state
            .iter()
            .flat_map(|state| state.index.ids().iter().copied())
    }
}

impl fmt::Debug for Modules {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (modules, version) = self
            .state
            .as_ref()
            .map_or((0, 0), |s| (s.index.ids.len(), s.version()));
        f.debug_struct("Modules")
            .field("modules", &modules)
            .field("version", &version)
            .finish()
    }
}
