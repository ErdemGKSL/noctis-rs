//! A starter Noctis bot:
//!
//! - `setup` — `/setup start` opens a hand-written three-step setup flow (UD-4).
//! - `dashboard` — `/dashboard` opens a tabbed dashboard whose Roles tab is paginated by
//!   hand and whose Modules tab toggles features (UD-5, UD-6).
//! - `profile` — `/profile` and the "Show profile" user command render a card image into
//!   a Media Gallery (UD-2), with a modal to edit the bio.
//! - `rank` — XP per message; an event handler posts an animated level-up card, and
//!   `/leaderboard` renders an activity line chart and the top members.
//! - `voice` (template feature `voice`) — `/join channel`, `/leave`, `/play url`, `/skip`, `/stop` (FR-80).
//!
//! [`configure`] wires the Adapters and state onto an [`AppBuilder`] and discovers every
//! Module (`#[noctis::module]`) and registration (`module = ".."`); `main.rs` and the
//! tests share it.

pub mod adapters;
pub mod bootstrap;
pub mod emoji;
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

/// Wire the Adapters and state and discover every Module. Token, shards and keys come from the builder the
/// caller passes in (`App::from_env()` in production, `App::builder()` for `TestApp` in tests).
pub fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
    // Event and Render failures go to our AlertSink; other origins log at `error`.
    let builder = observability::install(builder);
    builder
        .app_id("noctis-template")
        .locales(strings::catalog())
        // The application emojis this bot declares (`src/emoji.rs`, files in
        // `assets/emoji/`); a name two sets declare fails `build()` with `[EMO-03]`.
        .emojis::<emoji::Icons>()
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
        // Mounts every Module and registration the binary links, with no list here:
        // each feature folder declares its Module with `#[noctis::module]`, and each
        // Command, component, modal, event handler, hook and Template names its Module on
        // its attribute (`module = "profile"`), in whatever file it lives. Adding a
        // feature is adding files; `Module::add::<T>()` stays the explicit alternative.
        .discover();
    // Prefix commands (template feature `prefix`, off by default): every Command also
    // answers `!<path> <args>` (`!profile @user`, `!badge gold`) with the same handler;
    // there is no list here and no separate prefix code. `command_type = "slash"` on a
    // Command keeps it slash-only, `"prefix"` makes it prefix-only. Message text needs
    // the privileged MESSAGE_CONTENT intent (enable it in the Developer Portal too);
    // without `.intents(..)` the build fails with `[GW-05]` MissingIntent.
    // Voice channels (template feature `voice`, off by default; needs a C compiler and
    // cmake for libopus): `.voice(..)` installs voice and adds GUILD_VOICE_STATES, and
    // the `voice` Module's `/join`, `/leave`, `/play`, `/skip` and `/stop` call
    // `ctx.voice()`. At shutdown every call leaves its channel before the shards close.
    // The Opus encoder follows the channel's bitrate; cap it with
    // `VoiceConfig::default().max_bitrate(Some(96_000))`.
    #[cfg(feature = "voice")]
    builder.voice(noctis::voice::VoiceConfig::default());
    #[cfg(feature = "prefix")]
    builder
        .intents(noctis::twilight::model::gateway::Intents::MESSAGE_CONTENT)
        .plugin(noctis::prefix::PrefixPlugin::new("!"));
    builder
}
