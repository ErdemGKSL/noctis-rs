//! Middleware and typed Extensions through the facade: every handler kind
//! takes `Ext<T>` parameters in several positions (Matrix 2-6 and 11), the
//! build check names a missing provider (Matrix 16 and 19), and a run's
//! Extensions never reach a port (Matrix 42).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};

use noctis::__private::core::__testing::{call_command, dispatch_detached, responded};
use noctis::events::{EventCtx, MessageCreate};
use noctis::ports::memory::{
    MemoryAssetStore, MemoryCacheStore, MemoryModuleStateStore, MemoryRateLimitStore,
    MemoryRefStore, MemoryRenderCache, MemorySessionStore,
};
use noctis::prelude::*;
use noctis::ui::ResolvedUser;
use noctis::{AfterCtx, BuildViolation, ExtensionMissing, Runtime};

/// What the handlers and Middleware saw, in order.
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether `Maybe` got its optional `Ext`, once it ran.
static MAYBE: Mutex<Option<bool>> = Mutex::new(None);

fn log(line: impl Into<String>) {
    LOG.lock().unwrap().push(line.into());
}

fn take_log() -> Vec<String> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

/// The value the root Middleware inserts.
#[derive(Debug, PartialEq, Eq)]
pub struct Settings(u32);

/// The value only the optional parameters ask for.
#[derive(Debug)]
pub struct Unset;

async fn load_settings(ctx: BeforeCtx) -> HandlerResult<Flow> {
    log(format!("before {}", ctx.handler()));
    ctx.insert(Settings(7));
    Ok(Flow::Continue)
}

async fn audit(ctx: AfterCtx) {
    log(format!(
        "after {:?} {}",
        ctx.outcome(),
        ctx.get::<Settings>().map_or(0, |s| s.0)
    ));
}

/// Matrix 2: one `Ext` after the context.
#[derive(Button)]
#[button("mw.one", defaults(style = Primary, label = "One"))]
struct One {
    n: u64,
}

impl One {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, s: Ext<Settings>) -> HandlerResult<Responded> {
        log(format!("one {} {}", self.n, s.0));
        Ok(responded())
    }
}

/// Matrix 3 and 6: an optional `Ext` and qualified paths.
#[derive(Button)]
#[button("mw.opt", defaults(style = Primary, label = "Opt"))]
struct Opt;

#[expect(
    unused_qualifications,
    reason = "Matrix 6: qualified paths are detected"
)]
impl Opt {
    #[handler]
    async fn handle(
        self,
        _ctx: ComponentCtx,
        s: noctis::Ext<Settings>,
        u: core::option::Option<noctis::Ext<Unset>>,
    ) -> HandlerResult<Responded> {
        log(format!("opt {} {}", s.0, u.is_some()));
        Ok(responded())
    }
}

/// A modal whose handler takes an `Ext`.
#[derive(Modal, Debug)]
#[modal("mw.bio", defaults(title = "Bio"))]
struct Bio {
    #[text_input("Bio")]
    bio: String,
}

impl Bio {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx, s: Ext<Settings>) -> HandlerResult<Responded> {
        log(format!("bio {} {}", self.bio, s.0));
        Ok(responded())
    }
}

/// Matrix 4: path, `Ext`, optional `Ext` and responder, in order.
#[derive(SlashCommand)]
#[command("cfg (get|set)", description = "Configure")]
struct Cfg;

impl Cfg {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        path: CfgPath,
        s: Ext<Settings>,
        u: Option<Ext<Unset>>,
        r: CommandResponder,
    ) -> HandlerResult<Responded> {
        log(format!("cfg {} {} {}", path.as_str(), s.0, u.is_some()));
        Ok(r.reply(noctis::ui::LegacyMessage::new("ok")).await?.done())
    }
}

/// Matrix 5: the `Ext` before the path.
#[derive(SlashCommand)]
#[command("mode (on|off)", description = "Mode")]
struct Mode;

impl Mode {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        s: Ext<Settings>,
        path: ModePath,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new(format!(
            "{} {}",
            path.as_str(),
            s.0
        )))
    }
}

/// Only optional Extensions: builds without a provider, runs detached.
#[derive(SlashCommand)]
#[command("maybe", description = "Maybe")]
struct Maybe;

impl Maybe {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        s: Option<Ext<Settings>>,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        *MAYBE.lock().unwrap() = Some(s.is_some());
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

/// A context menu with the target, then an `Ext`.
#[derive(UserCommand)]
#[user_command("Show settings")]
struct ShowSettings;

impl ShowSettings {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedUser,
        s: Ext<Settings>,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new(format!("{}", s.0)))
    }
}

/// Matrix 11: an event handler with the event, then an `Ext`.
#[derive(EventHandler)]
struct OnMessage;

impl OnMessage {
    #[handler]
    async fn handle(
        self,
        _ctx: EventCtx,
        _ev: MessageCreate,
        s: Ext<Settings>,
    ) -> HandlerResult<()> {
        log(format!("event {}", s.0));
        Ok(())
    }
}

/// A Module holding every kind, under a root that provides `Settings`.
fn provided() -> Module {
    Module::new("root")
        .before(Middleware::before(load_settings).provides::<Settings>())
        .after(Middleware::after(audit))
        .module(
            Module::new("kinds")
                .add::<One>()
                .add::<Opt>()
                .add::<Bio>()
                .add::<Cfg>()
                .add::<Mode>()
                .add::<Maybe>()
                .add::<ShowSettings>()
                .add::<OnMessage>(),
        )
}

/// A click on `custom_id`.
fn click(custom_id: &str) -> String {
    format!(
        r#"{{"id":"1000","application_id":"7","type":3,"token":"tok","version":1,
        "authorizing_integration_owners":{{}},"entitlements":[],
        "user":{{"id":"3","username":"u","discriminator":"0000","avatar":null,"global_name":null}},
        "message":{{"id":"77","channel_id":"55","flags":32768,"content":"","tts":false,
        "author":{{"id":"7","username":"bot","discriminator":"0","avatar":null}},
        "timestamp":"2026-10-20T12:00:00.000000+00:00","edited_timestamp":null,
        "mention_everyone":false,"mentions":[],"mention_roles":[],"attachments":[],
        "embeds":[],"pinned":false,"type":0}},
        "data":{{"custom_id":"{custom_id}","component_type":2}}}}"#
    )
}

/// A submit of the `Bio` modal.
fn submit() -> String {
    r#"{"id":"1001","application_id":"7","type":5,"token":"tok","version":1,
    "authorizing_integration_owners":{},"entitlements":[],
    "user":{"id":"3","username":"u","discriminator":"0000","avatar":null,"global_name":null},
    "data":{"custom_id":"0mw.bio:","components":[
        {"type":18,"id":1,"component":{"type":4,"id":2,"custom_id":"bio","value":"hi"}}]}}"#
        .to_owned()
}

/// The `custom_id` of `One { n }`.
fn one(n: u64) -> String {
    noctis::__private::codec::encode::<One>(&One { n }, &noctis::__private::codec::Parts::new())
        .unwrap()
        .to_string()
}

/// Matrix 34: a detached call runs no Middleware, so a required `Ext` is
/// an `ExtensionMissing` and an optional one is `None`.
#[tokio::test]
async fn detached_calls_run_no_middleware() {
    let err = call_command(&Cfg::descriptor(), "cfg get")
        .await
        .unwrap_err();
    let missing = err
        .as_error()
        .downcast_ref::<ExtensionMissing>()
        .expect("an ExtensionMissing");
    let here = module_path!();
    assert_eq!(missing.handler(), format!("{here}::Cfg"));
    assert_eq!(missing.extension(), format!("{here}::Settings"));
    assert_eq!(
        missing.to_string(),
        format!(
            "handler `{here}::Cfg` takes `Ext<{here}::Settings>`, but no Middleware inserted it \
             in this run"
        )
    );
    call_command(&Maybe::descriptor(), "maybe").await.unwrap();
    assert_eq!(*MAYBE.lock().unwrap(), Some(false));
}

/// Matrix 16 and 19: a provider in a sibling does not count; every missing
/// `(handler, T)` is one violation, in fold order (commands before
/// components), with the others.
#[test]
fn missing_providers_are_build_violations() {
    let err = App::builder()
        .runtime(&Runtime::for_test())
        .module(
            Module::new("root")
                .module(
                    Module::new("kinds")
                        .add::<One>()
                        .add::<Cfg>()
                        .add::<Maybe>(),
                )
                .module(
                    Module::new("sibling")
                        .before(Middleware::before(load_settings).provides::<Settings>()),
                ),
        )
        .build()
        .unwrap_err();
    let here = module_path!();
    let missing: Vec<_> = err
        .violations()
        .iter()
        .filter_map(|v| match v {
            BuildViolation::MissingExtension {
                module,
                handler,
                extension,
            } => Some((*module, *handler, *extension)),
            _ => None,
        })
        .collect();
    let one = format!("{here}::One");
    let cfg = format!("{here}::Cfg");
    let settings = format!("{here}::Settings");
    assert_eq!(
        missing,
        [
            ("kinds", cfg.as_str(), settings.as_str()),
            ("kinds", one.as_str(), settings.as_str()),
        ]
    );
    let text = err
        .violations()
        .iter()
        .find(|v| matches!(v, BuildViolation::MissingExtension { .. }))
        .unwrap()
        .to_string();
    assert_eq!(
        text,
        format!(
            "handler `{here}::Cfg` (Module `kinds`) takes `Ext<{here}::Settings>`, but no \
             before-Middleware on its Module path provides it: add \
             `.before(Middleware::before(f).provides::<{here}::Settings>())` to Module `kinds` or \
             an ancestor, or take `Option<Ext<{here}::Settings>>`"
        )
    );
}

/// Matrix 2-6, 11 and 42: every kind builds with its Module path's
/// provider; clicks and submits run the chain and hand the value to the
/// handler, and no port store gets an entry.
#[tokio::test]
async fn every_kind_takes_extensions() {
    let refs = Arc::new(MemoryRefStore::new());
    let sessions = Arc::new(MemorySessionStore::new());
    let states = Arc::new(MemoryModuleStateStore::new());
    let limits = Arc::new(MemoryRateLimitStore::new());
    let cache = Arc::new(MemoryCacheStore::new());
    let renders = Arc::new(MemoryRenderCache::new());
    let assets = Arc::new(MemoryAssetStore::new());
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .intents(noctis::twilight::model::gateway::Intents::MESSAGE_CONTENT)
        .ref_store(Arc::clone(&refs))
        .session_store(Arc::clone(&sessions))
        .module_state_store(Arc::clone(&states))
        .rate_limit_store(Arc::clone(&limits))
        .cache_store(Arc::clone(&cache))
        .render_cache(Arc::clone(&renders))
        .asset_store(Arc::clone(&assets))
        .module(provided());
    let app = builder.build().unwrap();
    dispatch_detached(&app, &click(&one(4))).await;
    dispatch_detached(&app, &click("0mw.opt:")).await;
    dispatch_detached(&app, &submit()).await;
    let here = module_path!();
    assert_eq!(
        take_log(),
        [
            format!("before {here}::One"),
            "one 4 7".to_owned(),
            "after Completed 7".to_owned(),
            format!("before {here}::Opt"),
            "opt 7 false".to_owned(),
            "after Completed 7".to_owned(),
            format!("before {here}::Bio"),
            "bio hi 7".to_owned(),
            "after Completed 7".to_owned(),
        ]
    );
    let entries = [
        refs.usage().entries,
        sessions.usage().entries,
        states.usage().entries,
        limits.usage().entries,
        cache.usage().entries,
        renders.usage().entries,
        assets.usage().entries,
    ];
    assert_eq!(entries, [0; 7]);
}
