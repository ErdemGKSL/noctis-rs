//! [`TestApp`]: the bot's own App on local Discord fakes, driven by
//! simulated interactions.

mod command;
mod component;
mod outcome;
mod runs;
mod wire;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use noctis_core::__testing::AppProbe;
use noctis_core::{AppBuilder, Runtime, Token};
use serde_json::{Value, json};
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

pub use runs::{AutocompleteRun, ClickRun, CommandRun, MenuRun, SelectRun, SubmitRun};
use wire::Wire;

use crate::TestError;
use crate::fake::{FakeDiscordHttp, FakeGateway, GatewayBot};
use crate::response::{ModalForm, Response, RunCtx, Source, TestComponent};
use crate::seed::{Seeds, TestGuild, TestUser};

/// The default per-run wait.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// The first interaction id (`+ n` for run `n`).
const FIRST_INTERACTION_ID: u64 = 1_000_000_000_000_000_000;
/// The bot token the App runs with.
const TOKEN: &str = "noctis-test-token";
/// How often readiness is polled.
const POLL: Duration = Duration::from_millis(10);

/// The completion senders of the runs in flight, by interaction id.
type Waiters = Arc<Mutex<HashMap<u64, oneshot::Sender<()>>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The bot's App, built from its own builder and running against local
/// Discord fakes, with simulated users, guilds and interactions (FR-51).
///
/// [`build`](Self::build) takes the bot's `&mut AppBuilder` (so its Modules
/// and Plugins install as in production), points it at a local HTTP and
/// gateway fake, and runs it on [`Runtime::for_test`]. Every run sends one
/// `INTERACTION_CREATE` through the gateway fake, so the real shard parses
/// it and the real pipeline answers; the [`Response`] is read off the HTTP
/// fake once the pipeline task has ended. Requests a handler sends from a
/// task it spawned (a detached `tokio::spawn` follow-up) after it returned
/// are not part of that run's [`Response`]. Runs are serialized. No network
/// is used.
///
/// ```no_run
/// use noctis_core::App;
/// use noctis_testkit::{ResponseKind, TestApp, TestGuild, TestUser};
/// use twilight_model::id::Id;
///
/// # #[tokio::main] async fn main() -> Result<(), noctis_testkit::TestError> {
/// let mut builder = App::builder();
/// // configure(&mut builder) mounts the bot's Modules ...
/// let app = TestApp::build(&mut builder)
///     .await?
///     .guild(TestGuild::new(Id::new(10), "Guild").text_channel(Id::new(20), "logs"))
///     .user(TestUser::new(Id::new(40)));
/// let reply = app.command("ping").by(Id::new(40)).in_guild(Id::new(10)).run().await?;
/// assert_eq!(reply.kind(), ResponseKind::Reply);
/// app.stop().await?;
/// # Ok(()) }
/// ```
pub struct TestApp {
    http: FakeDiscordHttp,
    gateway: FakeGateway,
    wire: Arc<Wire>,
    probe: AppProbe,
    definitions: Vec<(Option<Id<GuildMarker>>, Value)>,
    waiters: Waiters,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), noctis_core::Error>>>,
    run_lock: tokio::sync::Mutex<()>,
    runs: AtomicU64,
    seeds: Seeds,
    /// The guild seeds already fed to the App's Discord cache.
    fed: Mutex<HashSet<Id<GuildMarker>>>,
    timeout: Duration,
}

impl fmt::Debug for TestApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestApp")
            .field("http", &self.http)
            .field("gateway", &self.gateway)
            .field("seeds", &self.seeds)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

/// One simulated interaction, ready to send.
pub(crate) struct Dispatch {
    /// The interaction type (2 command, 3 component, 4 autocomplete, 5
    /// modal submit).
    pub(crate) kind: u8,
    pub(crate) data: Value,
    pub(crate) ctx: RunCtx,
    /// The message the interaction is on (`message`).
    pub(crate) source: Option<Arc<Source>>,
}

/// Who runs, where and in which locale; unset parts take their defaults.
#[derive(Debug, Clone, Default)]
pub(crate) struct Who {
    pub(crate) by: Option<Id<UserMarker>>,
    pub(crate) locale: Option<String>,
    pub(crate) guild: Option<Id<GuildMarker>>,
    pub(crate) channel: Option<Id<ChannelMarker>>,
}

impl TestApp {
    /// The user every run uses without `.by(..)`: id 1, named `tester`,
    /// always seeded.
    pub const DEFAULT_USER: Id<UserMarker> = Id::new(1);

    /// Builds `builder` against local Discord fakes and starts it.
    ///
    /// In order: checks that the current tokio runtime is multi-thread;
    /// starts the HTTP fake (one recommended shard) and the gateway fake;
    /// sets `.runtime(&Runtime::for_test())`, a token, `.http_proxy(..)`,
    /// `.gateway_url(..)` and `.publisher(false)` on the builder (after the
    /// bot's own configuration, so these win); builds the App; runs it and
    /// waits until its shard has a gateway session.
    ///
    /// # Errors
    ///
    /// [`TestError::Runtime`] without a multi-thread runtime,
    /// [`TestError::Io`] when a fake cannot start, [`TestError::Build`] with
    /// the App's `BuildError`, [`TestError::Start`] when the App stops while
    /// starting (such as `Error::LocaleOverlay` for a rejected runtime
    /// locale overlay), [`TestError::Timeout`] when its shard does not connect
    /// within 10 s.
    pub async fn build(builder: &mut AppBuilder) -> Result<Self, TestError> {
        let multi_thread = Handle::try_current()
            .is_ok_and(|handle| handle.runtime_flavor() == RuntimeFlavor::MultiThread);
        if !multi_thread {
            return Err(TestError::Runtime);
        }
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards: 1,
            ..GatewayBot::default()
        })
        .await?;
        let gateway = FakeGateway::start().await?;
        let wire = Arc::new(Wire::new());
        let answering = Arc::clone(&wire);
        http.fallback_with(move |request| answering.answer(request));
        let token = Token::new(TOKEN)
            .map_err(|_| TestError::Unsupported("the testkit's bot token was rejected"))?;
        builder
            .runtime(&Runtime::for_test())
            .token(token)
            .http_proxy(http.url())
            .gateway_url(gateway.url())
            .publisher(false);
        let waiters: Waiters = Arc::default();
        let observed = Arc::clone(&waiters);
        noctis_core::__testing::observe_runs(
            builder,
            Arc::new(move |id| {
                if let Some(done) = lock(&observed).remove(&id.get()) {
                    // The run may have timed out and dropped its receiver.
                    let _ = done.send(());
                }
            }),
        );
        let app = builder.build().map_err(TestError::Build)?;
        let probe = AppProbe::of(&app);
        let definitions = probe.commands();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(app.run_until(async move {
            // A dropped sender stops the App too.
            let _ = stopped.await;
        }));
        let mut seeds = Seeds::default();
        let tester = TestUser::new(Self::DEFAULT_USER).name("tester");
        seeds.users.insert(tester.id, tester);
        let mut test_app = Self {
            http,
            gateway,
            wire,
            probe,
            definitions,
            waiters,
            stop: Some(stop),
            task: Some(task),
            run_lock: tokio::sync::Mutex::new(()),
            runs: AtomicU64::new(0),
            seeds,
            fed: Mutex::new(HashSet::new()),
            timeout: DEFAULT_TIMEOUT,
        };
        test_app.wait_ready().await?;
        Ok(test_app)
    }

    /// Waits until the shard has a session, or the run task ends.
    async fn wait_ready(&mut self) -> Result<(), TestError> {
        let deadline = tokio::time::Instant::now() + DEFAULT_TIMEOUT;
        loop {
            if self.gateway.live_sessions() >= 1 {
                return Ok(());
            }
            if self.task.as_ref().is_none_or(JoinHandle::is_finished) {
                return match self.task.take() {
                    Some(task) => match task.await {
                        Ok(Err(error)) => Err(TestError::Start(error)),
                        _ => Err(TestError::Stopped),
                    },
                    None => Err(TestError::Stopped),
                };
            }
            if tokio::time::Instant::now() >= deadline {
                if let Some(task) = self.task.take() {
                    task.abort();
                }
                return Err(TestError::Timeout {
                    what: "the App's shard to connect",
                    after: DEFAULT_TIMEOUT,
                });
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Seeds a guild; a later seed with the same id replaces it.
    ///
    /// The next run feeds the seed to the App's Discord cache as an
    /// available `GUILD_CREATE` (so `ctx.cache()` reads its name, channels
    /// and roles under the configured resources); a replaced seed is fed
    /// again.
    #[must_use]
    pub fn guild(mut self, guild: TestGuild) -> Self {
        lock(&self.fed).remove(&guild.id);
        self.seeds.guilds.insert(guild.id, guild);
        self
    }

    /// Seeds a user; a later seed with the same id replaces it.
    #[must_use]
    pub fn user(mut self, user: TestUser) -> Self {
        self.seeds.users.insert(user.id, user);
        self
    }

    /// Sets how long each run waits for its pipeline to end (default
    /// 10 s).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// A slash-command run of `path` (`"setup start"`, `"config set
    /// prefix"`).
    pub fn command(&self, path: &str) -> CommandRun<'_> {
        CommandRun::new(self, path)
    }

    /// A user-command (context menu) run of `name`.
    pub fn user_command(&self, name: &str) -> MenuRun<'_> {
        MenuRun::new(self, name, 2)
    }

    /// A message-command (context menu) run of `name`.
    pub fn message_command(&self, name: &str) -> MenuRun<'_> {
        MenuRun::new(self, name, 3)
    }

    /// A click on a button from an earlier [`Response`].
    pub fn click(&self, component: TestComponent) -> ClickRun<'_> {
        ClickRun::new(self, component)
    }

    /// A select on a select menu from an earlier [`Response`].
    pub fn select(&self, component: TestComponent) -> SelectRun<'_> {
        SelectRun::new(self, component)
    }

    /// A submit of a modal from an earlier [`Response::modal`].
    pub fn submit(&self, form: ModalForm) -> SubmitRun<'_> {
        SubmitRun::new(self, form)
    }

    /// An autocomplete run of command `path` with `typed` in the focused
    /// `option`.
    pub fn autocomplete(&self, path: &str, option: &str, typed: &str) -> AutocompleteRun<'_> {
        AutocompleteRun::new(self, path, option, typed)
    }

    /// Stops the App gracefully and waits for it.
    ///
    /// # Errors
    ///
    /// [`TestError::Start`] when the App ended with an error,
    /// [`TestError::Stopped`] when its task was lost,
    /// [`TestError::Timeout`] when it does not stop within 30 s.
    pub async fn stop(mut self) -> Result<(), TestError> {
        if let Some(stop) = self.stop.take() {
            // The App may have stopped already.
            let _ = stop.send(());
        }
        let Some(task) = self.task.take() else {
            return Err(TestError::Stopped);
        };
        let after = Duration::from_secs(30);
        match tokio::time::timeout(after, task).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(TestError::Start(error)),
            Ok(Err(_)) => Err(TestError::Stopped),
            Err(_) => Err(TestError::Timeout {
                what: "the App to stop",
                after,
            }),
        }
    }

    /// The HTTP fake the App sends to. Register answers for endpoints the
    /// built-in Discord wire does not model (a webhook the bot creates or
    /// executes, a raw REST call) with
    /// [`route`](FakeDiscordHttp::route) /
    /// [`route_with`](FakeDiscordHttp::route_with); a registered route wins
    /// over the built-in answers, and an unknown path is Discord's `404`.
    /// Every request also shows up in the run's
    /// [`Response::http_calls`](crate::Response::http_calls).
    pub fn http_fake(&self) -> &FakeDiscordHttp {
        &self.http
    }

    /// The gateway fake the App's shard is connected to.
    #[doc(hidden)]
    pub fn gateway_fake(&self) -> &FakeGateway {
        &self.gateway
    }

    pub(crate) fn seeds(&self) -> &Seeds {
        &self.seeds
    }

    pub(crate) fn definitions(&self) -> &[(Option<Id<GuildMarker>>, Value)] {
        &self.definitions
    }

    /// The context of a run by `who` (unset parts take their defaults).
    pub(crate) fn resolve(&self, who: &Who) -> Result<RunCtx, TestError> {
        let user = who.by.unwrap_or(Self::DEFAULT_USER);
        self.seeds.user(user)?;
        let guild_locale = match who.guild {
            Some(guild) => Some(self.seeds.guild(guild)?.locale.clone()),
            None => None,
        };
        let (channel, channel_type) = wire::run_channel(&self.seeds, who.guild, user, who.channel)?;
        Ok(RunCtx {
            user,
            guild: who.guild,
            channel,
            channel_type,
            locale: who.locale.clone().unwrap_or_else(|| "en-US".to_owned()),
            guild_locale,
        })
    }

    /// The context of a run on `base` (the run that produced a message, or
    /// opened a modal) by `who`'s user and locale, when given.
    pub(crate) fn resolve_on(&self, base: &RunCtx, who: &Who) -> Result<RunCtx, TestError> {
        let mut ctx = base.clone();
        if let Some(by) = who.by {
            self.seeds.user(by)?;
            ctx.user = by;
        }
        if let Some(locale) = &who.locale {
            ctx.locale.clone_from(locale);
        }
        Ok(ctx)
    }

    /// Sends one interaction and folds what the App answered.
    pub(crate) async fn execute(&self, dispatch: Dispatch) -> Result<Response, TestError> {
        let mut d = wire::interaction_json(dispatch.kind, &self.seeds, &dispatch.ctx)?;
        d["data"] = dispatch.data;
        if let Some(source) = &dispatch.source {
            d["message"] = source.json.clone();
        }
        let _serial = self.run_lock.lock().await;
        self.feed_guild_seeds().await?;
        // Requests of an earlier, timed-out run are not this run's.
        drop(self.http.take_requests());
        let n = self.runs.fetch_add(1, Ordering::Relaxed) + 1;
        let id = FIRST_INTERACTION_ID + n;
        let token = format!("noctis-test-{n}");
        d["id"] = json!(id.to_string());
        d["token"] = json!(token);
        self.wire.begin(
            token,
            dispatch.kind,
            dispatch.ctx.channel,
            dispatch.source.as_ref().map(|s| {
                let attachments = s
                    .json
                    .get("attachments")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                (s.id, attachments)
            }),
        );
        let (done, finished) = oneshot::channel();
        lock(&self.waiters).insert(id, done);
        if self.gateway.dispatch("INTERACTION_CREATE", d) == 0 {
            lock(&self.waiters).remove(&id);
            self.wire.end();
            return Err(TestError::Stopped);
        }
        let waited = tokio::time::timeout(self.timeout, finished).await;
        lock(&self.waiters).remove(&id);
        let requests = self.http.take_requests();
        let events = self.wire.end();
        self.gateway.clear_frames();
        match waited {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(TestError::Stopped),
            Err(_) => {
                return Err(TestError::Timeout {
                    what: "the interaction's handler to finish",
                    after: self.timeout,
                });
            }
        }
        let run = outcome::Run {
            probe: &self.probe,
            ctx: &dispatch.ctx,
            source: dispatch.source.as_ref(),
        };
        Ok(run.fold(events, &requests))
    }
}

impl TestApp {
    /// Feeds every guild seed not yet fed to the App's Discord cache, as
    /// an available `GUILD_CREATE` (a no-op with the cache off). Runs under
    /// the run lock.
    ///
    /// # Errors
    ///
    /// [`TestError::Unsupported`] when a seed does not deserialize as a
    /// `Guild` (a testkit bug).
    async fn feed_guild_seeds(&self) -> Result<(), TestError> {
        let pending: Vec<&TestGuild> = {
            let mut fed = lock(&self.fed);
            self.seeds
                .guilds
                .values()
                .filter(|guild| fed.insert(guild.id))
                .collect()
        };
        for guild in pending {
            let event = guild.create_event().map_err(|_| {
                TestError::Unsupported("a guild seed did not deserialize as a Discord guild")
            })?;
            self.probe.feed_discord_cache(&event).await;
        }
        Ok(())
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Dropping the sender resolves the App's shutdown future; it then
        // shuts down in the background.
        drop(self.stop.take());
    }
}
