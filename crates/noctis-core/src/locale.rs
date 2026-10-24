//! User and Guild Locale resolution (FR-43, AD-16, AD-23 stage 4):
//! [`Locales`], [`LocaleRequest`] and the App's optional
//! [`LocaleResolver`], plus the crate's resolution engine, its per-guild
//! locale record and the per-run locale cell.
//!
//! Stage 4 of every run resolves the raw Discord codes against the locales
//! the App ships a file for (the union of its merged catalogs' files) and
//! its default locale (the first catalog's):
//!
//! - an interaction resolves its User Locale as User → Guild → default and
//!   its Guild Locale as Guild → User → default;
//! - a guild event resolves Guild → default, and its User Locale is the
//!   Guild Locale;
//! - each step takes the code when it has a file, else its
//!   [`Locale::sibling`] when that has one, else nothing; a code that is
//!   absent or not exactly one of Discord's 32 codes is skipped.
//!
//! The App's [`LocaleResolver`] then sees the result and may replace it, and
//! a before-Middleware may override it again (`BeforeCtx::set_locale`).

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use noctis_rules::Locale;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};

use crate::guild_record::{GuildRecord, GuildRecords};
use crate::middleware::Invocation;

/// The two locales of one run (FR-43): the User Locale, for ephemeral
/// single-user UI, and the Guild Locale, for shared content.
///
/// ```
/// use noctis_core::Locales;
/// use noctis_rules::Locale;
///
/// let locales = Locales { user: Locale::Tr, guild: Locale::EnUs };
/// assert_ne!(locales.user, locales.guild);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Locales {
    /// The User Locale: what `ctx.user_locale()` returns, and the locale of
    /// ephemeral responses and notices.
    pub user: Locale,
    /// The Guild Locale: what `ctx.guild_locale()` returns, and the locale
    /// of public responses.
    pub guild: Locale,
}

impl Locales {
    /// Both locales set to `locale`.
    pub(crate) const fn both(locale: Locale) -> Self {
        Self {
            user: locale,
            guild: locale,
        }
    }
}

/// What stage 4 hands the App's [`LocaleResolver`]: the run, where it comes
/// from, Discord's raw locales (parsed) and the built-in resolution.
///
/// Built only by Noctis (`#[non_exhaustive]`).
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct LocaleRequest {
    /// What the run answers: the Command, context menu, component, modal,
    /// autocomplete or event.
    pub invocation: Invocation,
    /// The guild, if there is one.
    pub guild_id: Option<Id<GuildMarker>>,
    /// The invoking user (`None` for events).
    pub user_id: Option<Id<UserMarker>>,
    /// Discord's `locale`, parsed: `None` when it is absent or not exactly
    /// one of the 32 codes; always `None` for events.
    pub discord_user: Option<Locale>,
    /// Discord's `guild_locale` for an interaction, or the guild's recorded
    /// `preferred_locale` (from `GUILD_CREATE`/`GUILD_UPDATE`) for an event
    /// and a `Dispatcher` Command; `None` when unknown.
    pub discord_guild: Option<Locale>,
    /// The built-in resolution of the two codes against the App's locale
    /// files.
    pub resolved: Locales,
}

/// The App's locale override (`AppBuilder::locale_resolver`): called once
/// per run at stage 4, after the built-in resolution, and its return value
/// is used verbatim (no locale-file check; a key renders through each
/// catalog's own fallback).
///
/// It is synchronous and must not do I/O: per-guild settings read from a
/// store belong in a before-Middleware (`BeforeCtx::set_guild_locale`). A
/// panic is caught and logged on `noctis::i18n`, and the run keeps the
/// built-in resolution.
///
/// Every `Fn(&LocaleRequest) -> Locales + Send + Sync + 'static` closure is
/// a resolver.
///
/// ```
/// use noctis_core::{LocaleRequest, LocaleResolver, Locales};
/// use noctis_rules::Locale;
///
/// // Guild 7 is a Brazilian community: its shared content is pt-BR.
/// let resolver = |request: &LocaleRequest| match request.guild_id {
///     Some(id) if id.get() == 7 => Locales { guild: Locale::PtBr, ..request.resolved },
///     _ => request.resolved,
/// };
/// fn takes(_: impl LocaleResolver) {}
/// takes(resolver);
/// ```
pub trait LocaleResolver: Send + Sync + 'static {
    /// The locales of the run `request` describes.
    fn resolve(&self, request: &LocaleRequest) -> Locales;
}

impl<F> LocaleResolver for F
where
    F: Fn(&LocaleRequest) -> Locales + Send + Sync + 'static,
{
    fn resolve(&self, request: &LocaleRequest) -> Locales {
        self(request)
    }
}

/// The position of `locale` in [`Locale::ALL`] (its declaration order).
const fn index(locale: Locale) -> u8 {
    locale as u8
}

/// The locale at position `i` of [`Locale::ALL`]; `en-US` out of range.
fn at(i: u16) -> Locale {
    Locale::ALL
        .get(usize::from(i))
        .copied()
        .unwrap_or(Locale::EnUs)
}

/// A set of locales: one bit per [`Locale::ALL`] position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LocaleSet(u32);

impl LocaleSet {
    /// The set of `locales`.
    pub(crate) fn of(locales: impl IntoIterator<Item = Locale>) -> Self {
        Self(
            locales
                .into_iter()
                .fold(0, |bits, locale| bits | 1 << index(locale)),
        )
    }

    /// Whether `locale` is in the set.
    pub(crate) const fn contains(self, locale: Locale) -> bool {
        self.0 & (1 << index(locale)) != 0
    }
}

/// Parses a raw Discord code exactly; an absent, empty, wrong-case or
/// unknown code is `None`.
pub(crate) fn parse(code: Option<&str>) -> Option<Locale> {
    code.and_then(|code| code.parse().ok())
}

/// One resolution step: `code` when it has a file, else its sibling when
/// that has one, else nothing.
fn step(available: LocaleSet, code: Option<Locale>) -> Option<Locale> {
    let code = code?;
    if available.contains(code) {
        return Some(code);
    }
    code.sibling()
        .filter(|&sibling| available.contains(sibling))
}

/// The built-in resolution (FR-43): the User Locale is
/// `step(user) ∥ step(guild) ∥ default` and the Guild Locale
/// `step(guild) ∥ step(user) ∥ default`. An event (no `user`) gets
/// `step(guild) ∥ default` for both.
pub(crate) fn resolve(
    available: LocaleSet,
    default: Locale,
    user: Option<Locale>,
    guild: Option<Locale>,
) -> Locales {
    let (user, guild) = (step(available, user), step(available, guild));
    Locales {
        user: user.or(guild).unwrap_or(default),
        guild: guild.or(user).unwrap_or(default),
    }
}

/// The App's locale table (in `AppShared`): which locales have a file, the
/// default, the optional resolver and the per-guild locale record.
pub(crate) struct AppLocales {
    available: LocaleSet,
    default: Locale,
    resolver: Option<Arc<dyn LocaleResolver>>,
    /// The [`GuildRecord`] (`preferred_locale`) of every guild the gateway
    /// announced, fed by `GUILD_CREATE`, `GUILD_UPDATE` and `GUILD_DELETE`
    /// (bounded by the App's guild membership: interactions never insert).
    guilds: RwLock<GuildRecords>,
}

impl AppLocales {
    /// The table of the merged `catalogs` (deduplicated, in merge order):
    /// every catalog's files and the first catalog's default; `{en-US}`
    /// and `en-US` without catalogs.
    pub(crate) fn new(
        catalogs: &[noctis_i18n::Catalog],
        resolver: Option<Arc<dyn LocaleResolver>>,
    ) -> Self {
        let (available, default) = match catalogs.first() {
            None => (LocaleSet::of([Locale::EnUs]), Locale::EnUs),
            Some(first) => (
                LocaleSet::of(catalogs.iter().flat_map(noctis_i18n::Catalog::locales)),
                first.default_locale(),
            ),
        };
        Self {
            available,
            default,
            resolver,
            guilds: RwLock::new(GuildRecords::new()),
        }
    }

    /// The App's default locale.
    pub(crate) fn default_locale(&self) -> Locale {
        self.default
    }

    /// Which locales have a file.
    pub(crate) fn available(&self) -> LocaleSet {
        self.available
    }

    /// The built-in resolution of `user` and `guild`.
    pub(crate) fn builtin(&self, user: Option<Locale>, guild: Option<Locale>) -> Locales {
        resolve(self.available, self.default, user, guild)
    }

    /// The built-in resolution of the raw codes (pre-stage-4 notices).
    pub(crate) fn builtin_raw(&self, user: Option<&str>, guild: Option<&str>) -> Locales {
        self.builtin(parse(user), parse(guild))
    }

    /// Stage 4: the built-in resolution of `request`'s codes, then the
    /// App's resolver once (a panic keeps the built-in one, with one
    /// `error` on `noctis::i18n` naming `handler`).
    pub(crate) fn resolve_run(&self, request: Stage4, handler: &'static str) -> Locales {
        let resolved = self.builtin(request.discord_user, request.discord_guild);
        let Some(custom) = &self.resolver else {
            return resolved;
        };
        let request = LocaleRequest {
            invocation: request.invocation,
            guild_id: request.guild_id,
            user_id: request.user_id,
            discord_user: request.discord_user,
            discord_guild: request.discord_guild,
            resolved,
        };
        match std::panic::catch_unwind(AssertUnwindSafe(|| custom.resolve(&request))) {
            Ok(locales) => locales,
            Err(panic) => {
                tracing::error!(
                    target: "noctis::i18n",
                    handler,
                    panic = crate::worker::panic_message(&*panic),
                    "the LocaleResolver panicked; the run keeps the built-in locales"
                );
                resolved
            }
        }
    }

    fn guilds_read(&self) -> std::sync::RwLockReadGuard<'_, GuildRecords> {
        self.guilds.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn guilds_write(&self) -> std::sync::RwLockWriteGuard<'_, GuildRecords> {
        self.guilds.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// The recorded locale of `guild_id`, if any.
    pub(crate) fn guild(&self, guild_id: Option<Id<GuildMarker>>) -> Option<Locale> {
        self.guilds_read()
            .get(&guild_id?)
            .map(|record| record.locale)
    }

    /// How many guilds have a record.
    #[cfg(test)]
    pub(crate) fn recorded(&self) -> usize {
        self.guilds_read().len()
    }

    /// Records `guild_id`'s `preferred_locale`, or removes the record when
    /// it does not parse.
    fn set_guild(&self, guild_id: Id<GuildMarker>, preferred: &str) {
        match preferred.parse::<Locale>() {
            Ok(locale) => {
                self.guilds_write().insert(guild_id, GuildRecord { locale });
            }
            Err(_) => {
                self.guilds_write().remove(&guild_id);
            }
        }
    }

    /// Updates the guild record from one inbound gateway event:
    /// `GUILD_CREATE` (available) and `GUILD_UPDATE` set it, `GUILD_DELETE`
    /// without `unavailable: true` removes it; outages and every other
    /// event leave it alone.
    pub(crate) fn observe(&self, event: &Event) {
        match event {
            Event::GuildCreate(create) => {
                if let GuildCreate::Available(guild) = &**create {
                    self.set_guild(guild.id, &guild.preferred_locale);
                }
            }
            Event::GuildUpdate(update) => self.set_guild(update.id, &update.preferred_locale),
            Event::GuildDelete(delete) if delete.unavailable != Some(true) => {
                self.guilds_write().remove(&delete.id);
            }
            _ => {}
        }
    }

    /// An interaction's `guild_locale` refreshes an existing record of
    /// `guild_id`; it never inserts one.
    pub(crate) fn refresh(&self, guild_id: Option<Id<GuildMarker>>, locale: Option<Locale>) {
        let (Some(guild_id), Some(locale)) = (guild_id, locale) else {
            return;
        };
        if self
            .guilds_read()
            .get(&guild_id)
            .map(|record| record.locale)
            == Some(locale)
        {
            return;
        }
        if let Some(record) = self.guilds_write().get_mut(&guild_id) {
            record.locale = locale;
        }
    }
}

impl fmt::Debug for AppLocales {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppLocales")
            .field("available", &self.available)
            .field("default", &self.default)
            .field("resolver", &self.resolver.is_some())
            .finish_non_exhaustive()
    }
}

/// What stage 4 knows of a run before the resolver sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stage4 {
    pub(crate) invocation: Invocation,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) user_id: Option<Id<UserMarker>>,
    pub(crate) discord_user: Option<Locale>,
    pub(crate) discord_guild: Option<Locale>,
}

/// The per-run locale cell: the run's [`Locales`] in one atomic, shared by
/// the handler context, the Responder, the notices and the Middleware; a
/// before-Middleware override reaches every reader.
pub(crate) struct LocaleCell(AtomicU16);

impl LocaleCell {
    /// A shared cell holding `locales`.
    pub(crate) fn new(locales: Locales) -> Arc<Self> {
        Arc::new(Self(AtomicU16::new(pack(locales))))
    }

    /// The run's current locales.
    pub(crate) fn get(&self) -> Locales {
        unpack(self.0.load(Ordering::Acquire))
    }

    /// Replaces the locales with `f` of the current ones.
    pub(crate) fn update(&self, f: impl Fn(Locales) -> Locales) {
        let _ = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
                Some(pack(f(unpack(bits))))
            });
    }
}

impl fmt::Debug for LocaleCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.get(), f)
    }
}

/// The user locale's index in the low byte, the guild's in the high byte.
fn pack(locales: Locales) -> u16 {
    u16::from(index(locales.user)) | u16::from(index(locales.guild)) << 8
}

fn unpack(bits: u16) -> Locales {
    Locales {
        user: at(bits & 0xFF),
        guild: at(bits >> 8),
    }
}

#[cfg(test)]
mod tests;
