//! [`VoicePlugin`] and [`AppBuilderVoiceExt`]: installing voice on an App.

use futures_util::future::join_all;
use noctis_core::events::{GuildDelete, VoiceServerUpdate, VoiceStateUpdate};
use noctis_core::lifecycle::AppStarted;
use noctis_core::{
    AppBuilder, BuildViolation, EventCtx, EventHandler, HandlerResult, HookCtx, LifecycleHook,
    Module, Plugin,
};
use songbird::id::ChannelId;

use crate::call::VoiceShared;
use crate::config::VoiceConfig;

/// The id of the Module the plugin's handlers live in.
const MODULE: &str = "noctis-voice";

/// Installs voice on an App: the call map (App state), the handlers of the
/// bot's own `VOICE_STATE_UPDATE`, every `VOICE_SERVER_UPDATE` and
/// `GUILD_DELETE` (Module `noctis-voice`, which adds the
/// `GUILD_VOICE_STATES` intent) and a Shutdown Task that leaves every voice
/// channel before the shards close.
///
/// Add it with [`AppBuilderVoiceExt::voice`]; an out-of-range
/// [`VoiceConfig`] is a build violation.
///
/// ```
/// use noctis_core::App;
/// use noctis_voice::{VoiceConfig, VoicePlugin};
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
/// let mut builder = App::builder();
/// builder.plugin(VoicePlugin::new(VoiceConfig::default()));
/// assert!(builder.build().is_ok());
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct VoicePlugin {
    config: VoiceConfig,
}

impl VoicePlugin {
    /// A plugin with `config`.
    pub fn new(config: VoiceConfig) -> Self {
        Self { config }
    }
}

impl Plugin for VoicePlugin {
    fn install(&self, app: &mut AppBuilder) {
        for message in self.config.violations() {
            app.report(BuildViolation::plugin::<Self>(message));
        }
        app.state(VoiceShared::new(self.config.clone()));
        app.module(
            Module::new(MODULE)
                .__add(EventHandler::__new::<VoiceStateUpdate, _, _>(
                    "noctis_voice::on_voice_state",
                    true,
                    false,
                    on_voice_state,
                ))
                .__add(EventHandler::__new::<VoiceServerUpdate, _, _>(
                    "noctis_voice::on_voice_server",
                    true,
                    false,
                    on_voice_server,
                ))
                .__add(EventHandler::__new::<GuildDelete, _, _>(
                    "noctis_voice::on_guild_delete",
                    true,
                    false,
                    on_guild_delete,
                ))
                .__add(LifecycleHook::__new::<AppStarted, _, _>(
                    "noctis_voice::on_app_started",
                    on_app_started,
                )),
        );
        app.shutdown_task(MODULE, |app| {
            let shared = app.state::<VoiceShared>();
            async move {
                if let Some(shared) = shared {
                    leave_all(&shared).await;
                }
            }
        });
    }
}

/// The bot's own voice state: a move updates the call, no channel removes
/// it (and confirms a shutdown leave). Others' states are ignored.
async fn on_voice_state(ctx: EventCtx, event: VoiceStateUpdate) -> HandlerResult<()> {
    let Some(shared) = ctx.state::<VoiceShared>() else {
        return Ok(());
    };
    let state = event.0;
    let Some(guild) = state.guild_id else {
        return Ok(());
    };
    if ctx.gateway().current_user_id() != Some(state.user_id) {
        return Ok(());
    }
    match state.channel_id {
        None => {
            shared.left(guild);
            if let Some(entry) = shared.remove(guild) {
                // Disconnected (or left): drop the connection locally, no op 4.
                entry
                    .call
                    .lock()
                    .await
                    .update_state(state.session_id, None::<ChannelId>);
            }
        }
        Some(channel) => {
            if let Some(call) = shared.moved(guild, channel) {
                call.lock().await.update_state(
                    state.session_id,
                    Some(ChannelId::from(channel.into_nonzero())),
                );
            }
        }
    }
    Ok(())
}

/// A voice server for a guild with a call; a `null` endpoint is ignored
/// (Discord sends another).
async fn on_voice_server(ctx: EventCtx, event: VoiceServerUpdate) -> HandlerResult<()> {
    let Some(shared) = ctx.state::<VoiceShared>() else {
        return Ok(());
    };
    let (Some(endpoint), Some(call)) = (event.endpoint, shared.call(event.guild_id)) else {
        return Ok(());
    };
    call.lock().await.update_server(endpoint, event.token);
    Ok(())
}

/// The bot was removed from a guild: its call is dropped without op 4.
async fn on_guild_delete(ctx: EventCtx, event: GuildDelete) -> HandlerResult<()> {
    let Some(shared) = ctx.state::<VoiceShared>() else {
        return Ok(());
    };
    if let Some(entry) = shared.remove(event.id) {
        entry
            .call
            .lock()
            .await
            .update_state(String::new(), None::<ChannelId>);
    }
    Ok(())
}

/// Binds the `app` label of `noctis_voice_calls`.
async fn on_app_started(ctx: HookCtx, _: AppStarted) -> HandlerResult<()> {
    if let Some(shared) = ctx.state::<VoiceShared>() {
        shared.bind_app(ctx.app_id());
    }
    Ok(())
}

/// Forgets the shutdown waiters when the task ends or is dropped.
struct ClearLeaving<'a>(&'a VoiceShared);

impl Drop for ClearLeaving<'_> {
    fn drop(&mut self) {
        self.0.clear_leaving();
    }
}

/// The Shutdown Task: takes every call, sends op 4 leave for each and waits
/// for the bot's own `VOICE_STATE_UPDATE`s without a channel, so the frames
/// are on the wire before the shards close. Dropped at core's budget, it
/// drops the remaining calls.
async fn leave_all(shared: &VoiceShared) {
    let entries = shared.take_all();
    if entries.is_empty() {
        return;
    }
    let _clear = ClearLeaving(shared);
    let mut confirmations = Vec::with_capacity(entries.len());
    for (guild, entry) in &entries {
        let left = shared.await_left(*guild);
        let mut call = entry.call.lock().await;
        match call.leave().await {
            Ok(()) => confirmations.push(left),
            Err(error) => tracing::debug!(
                target: "noctis::voice",
                guild = guild.get(),
                %error,
                refused = ?entry.shard.take_error(),
                "op 4 leave at shutdown was not sent"
            ),
        }
    }
    join_all(confirmations).await;
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for noctis_core::AppBuilder {}
}

/// `.voice(..)` on the [`AppBuilder`]: installs (or reconfigures) the App's
/// one [`VoicePlugin`]. The last call wins and there is never a
/// `DuplicatePlugin`.
///
/// Sealed: only the `AppBuilder` implements it.
///
/// ```
/// use noctis_core::App;
/// use noctis_voice::{AppBuilderVoiceExt, VoiceConfig};
/// use twilight_model::gateway::Intents;
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
/// let mut builder = App::builder();
/// builder.voice(VoiceConfig::default());
/// let app = builder.build().unwrap();
/// assert!(app.intents().contains(Intents::GUILD_VOICE_STATES));
/// # }
/// ```
pub trait AppBuilderVoiceExt: sealed::Sealed {
    /// Installs voice with `config`.
    fn voice(&mut self, config: VoiceConfig) -> &mut Self;
}

impl AppBuilderVoiceExt for AppBuilder {
    fn voice(&mut self, config: VoiceConfig) -> &mut Self {
        match self.plugin_mut::<VoicePlugin>() {
            Some(plugin) => plugin.config = config,
            None => {
                self.plugin(VoicePlugin::new(config));
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use noctis_core::App;
    use twilight_model::gateway::Intents;
    use twilight_model::id::Id;

    use super::*;
    use crate::{VoiceError, VoiceExt};

    /// The build rows: the intent, the Module, the last `.voice(..)` wins.
    #[tokio::test]
    async fn build_installs_voice() {
        let mut builder = App::builder();
        builder
            .voice(VoiceConfig::default())
            .voice(VoiceConfig::default().join_timeout(Duration::from_secs(3)));
        assert_eq!(
            builder
                .plugin_mut::<VoicePlugin>()
                .map(|p| p.config.get_join_timeout()),
            Some(Duration::from_secs(3))
        );
        let app = builder.build().unwrap_or_else(|e| panic!("{e}"));
        assert!(app.intents().contains(Intents::GUILD_VOICE_STATES));
        assert_eq!(
            app.modules().ids().filter(|id| *id == MODULE).count(),
            1,
            "Module `{MODULE}` mounted once"
        );
        let voice = app.voice();
        assert_eq!(voice.active_calls(), 0);
        assert!(voice.call(Id::new(1)).is_none());
        assert!(format!("{voice:?}").contains("installed: true"));
    }

    /// An out-of-range join timeout is a plugin violation naming the range.
    #[tokio::test]
    async fn config_violation_fails_the_build() {
        let mut builder = App::builder();
        builder.voice(VoiceConfig::default().join_timeout(Duration::from_millis(999)));
        let error = builder
            .build()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("1 s..=60 s"), "{error}");
        assert!(error.contains("VoicePlugin"), "{error}");
    }

    /// Without the plugin every operation is `NotInstalled`.
    #[tokio::test]
    async fn not_installed() {
        let app = App::builder().build().unwrap_or_else(|e| panic!("{e}"));
        let voice = app.voice();
        let guild = Id::new(1);
        assert_eq!(
            voice.join(guild, Id::new(2)).await.err(),
            Some(VoiceError::NotInstalled)
        );
        assert_eq!(voice.leave(guild).await, Err(VoiceError::NotInstalled));
        assert!(voice.call(guild).is_none());
        assert_eq!(voice.active_calls(), 0);
    }

    /// Before `run` a join is the gateway's `NotRunning`, and no call is
    /// created.
    #[tokio::test]
    async fn vc_01_join_before_run() {
        let mut builder = App::builder();
        builder.voice(VoiceConfig::default());
        let app = builder.build().unwrap_or_else(|e| panic!("{e}"));
        let voice = app.voice();
        assert_eq!(
            voice.join(Id::new(1), Id::new(2)).await.err(),
            Some(VoiceError::Gateway(
                noctis_gateway::GatewayError::NotRunning
            ))
        );
        assert_eq!(voice.active_calls(), 0);
        assert_eq!(
            voice.leave(Id::new(1)).await,
            Err(VoiceError::Gateway(
                noctis_gateway::GatewayError::NotRunning
            ))
        );
    }
}
