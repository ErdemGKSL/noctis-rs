// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! A starter Noctis bot:
//!
//! - `setup` — `/setup start` opens a hand-written three-step setup flow (UD-4).
//! - `dashboard` — `/dashboard` opens a tabbed dashboard whose Roles tab is paginated by
//!   hand and whose Modules tab toggles features (UD-5, UD-6).
//! - `profile` — `/profile` and the "Show profile" user command render a card image into
//!   a Media Gallery (UD-2), with a modal to edit the bio.
//! - `voice` (template feature `voice`) — `/join channel`, `/leave`, `/play url`, `/skip`, `/stop` (FR-80).
//!
//! [`configure`] wires every Module, Adapter and piece of state onto an [`AppBuilder`];
//! `main.rs` and the tests share it.

pub mod adapters;
pub mod bootstrap;
pub mod modules;
pub mod observability;
pub mod strings;

use std::time::{Duration, SystemTime};

use noctis::prelude::*;

/// Read-only facts the dashboard shows. Registered once with `.state(..)` and
/// read in handlers with `ctx.state::<BotMeta>()` (AD-9).
#[derive(Debug, Clone)]
pub struct BotMeta {
    pub started_at: SystemTime,
    pub version: &'static str,
}

/// Mount every Module and Adapter. Token, shards and keys come from the builder the
/// caller passes in (`App::from_env()` in production, `App::builder()` for `TestApp` in tests).
pub fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
    // Event and Render failures go to our AlertSink; other origins log at `error`.
    let builder = observability::install(builder);
    builder
        .app_id("noctis-template")
        .locales(strings::catalog())
        // Theme tokens and keyframes for Templates; the same file the macros validate against.
        // `.theme` comes from `AppBuilderImageExt` (image feature, in the prelude) and
        // installs the ImagePlugin, which puts the Renderer on this App (AD-24).
        .theme(noctis::image::theme!())
        // Our own CacheStore Adapter replaces the in-memory default (FR-38).
        .cache_store(adapters::BoundedCacheStore::new(50_000))
        // The Discord resource cache behind `ctx.cache()`: the default caches GUILD,
        // CHANNEL and ROLE (what the `GUILDS` intent delivers); the profile card also
        // reads users, so add USER. MEMBER would need `.intents(Intents::GUILD_MEMBERS)`.
        .discord_cache(
            noctis::DiscordCacheConfig::default()
                .with_resources(noctis::DiscordCacheResources::USER),
        )
        // On Ctrl-C/SIGTERM, running handlers get 15 s to finish before they are
        // cancelled (reported as `ReportKind::Cancelled`); keep it below the
        // orchestrator's kill timeout. `NOCTIS_SHUTDOWN_GRACE` sets it too.
        .shutdown_grace(Duration::from_secs(15))
        // The Discord HTTP edge's defaults are tuned for Discord (warm HTTP/2 with PINGs,
        // hedged interaction acks); this call is optional. Never poll an API endpoint to
        // keep HTTP warm.
        .http(noctis::HttpConfig::default())
        // Every shard identifies with this presence. Change it at run time with
        // `app.presence().set(..)` (every shard) or `.set_shard(n, ..)`, and send gateway
        // commands such as Request Guild Members through `ctx.gateway()`; each shard sends
        // at most 110 per 60 s (`GatewayError::SendLimit`, GW-04).
        .presence(noctis::gateway::Presence::watching("/help"))
        .state(BotMeta {
            started_at: SystemTime::now(),
            version: env!("CARGO_PKG_VERSION"),
        })
        .module(modules::setup::module())
        .module(modules::dashboard::module())
        .module(modules::profile::module())
        // Lifecycle Hooks: App start, shards, Publish, Module toggles, shutdown.
        .module(modules::ops::module());
    // Prefix commands (template feature `prefix`, off by default): `!profile @user` and
    // `!badge gold` run the same handlers as `/profile` and `/badge`. Message text needs
    // the privileged MESSAGE_CONTENT intent (enable it in the Developer Portal too);
    // without `.intents(..)` the build fails with `[GW-05]` MissingIntent.
    // Voice channels (template feature `voice`, off by default; needs a C compiler and
    // cmake for libopus): `.voice(..)` installs voice and adds GUILD_VOICE_STATES, and
    // the `voice` Module's `/join`, `/leave`, `/play`, `/skip` and `/stop` call
    // `ctx.voice()`. At shutdown every call leaves its channel before the shards close.
    #[cfg(feature = "voice")]
    builder
        .voice(noctis::voice::VoiceConfig::default())
        .module(modules::voice::module());
    #[cfg(feature = "prefix")]
    builder.intents(noctis::twilight::model::gateway::Intents::MESSAGE_CONTENT).plugin(
        noctis::prefix::PrefixPlugin::new("!")
            .command::<modules::profile::Profile>()
            .command::<modules::profile::Badge>(),
    );
    builder
}
