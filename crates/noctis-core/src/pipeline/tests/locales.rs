//! Story 11.3: User and Guild Locale resolution through the Command
//! pipeline and the `Dispatcher` (rows 6, 11, 25, 28-34, 36, 39, 42, 43
//! and 48).

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::test_support::{TBuy, guild_create};
use crate::{BeforeCtx, Flow, LocaleRequest, Locales, Middleware};

/// `loc.hi`: en-US "Hi", tr "Selam"; `loc.stop`: en-US "Stopped", tr
/// "Durdu"; a `de` file without them.
static STRINGS: noctis_i18n::Catalog = noctis_i18n::Catalog::__new(
    "noctis-core/loc-strings",
    Locale::EnUs,
    &[
        (Locale::De, &[]),
        (Locale::EnUs, &[("loc.hi", "Hi"), ("loc.stop", "Stopped")]),
        (Locale::Tr, &[("loc.hi", "Selam"), ("loc.stop", "Durdu")]),
    ],
    &[("loc.hi", &[]), ("loc.stop", &[])],
);

const HI: MessageKey = MessageKey::new("loc.hi");
const STOP: MessageKey = MessageKey::new("loc.stop");

/// `/where` (public) and `/mine` (ephemeral) log
/// `user guild response` locales and reply.
fn where_module(log: &Log) -> Module {
    let record = |log: &Log| {
        let log = log.clone();
        // Logged inside the future: after the before-Middleware.
        move |ctx: CommandCtx, _: CommandResponder| {
            let log = log.clone();
            async move {
                log.push(format!(
                    "{} {} {}",
                    ctx.user_locale(),
                    ctx.guild_locale(),
                    ctx.response_locale()
                ));
                Ok(v2("ok"))
            }
        }
    };
    Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::where",
            &["where"],
            "d",
            record(log),
        ))
        .__add(
            CommandHandler::__new_plain("m::mine", &["mine"], "d", record(log))
                .__with_ephemerality(Ephemerality::Ephemeral),
        )
}

/// A slash run of `name` with the top-level `extra` fields.
fn run_in(name: &str, extra: Value) -> Inbound {
    inbound(&interaction(slash(name, json!([])), extra))
}

/// Rows 6, 9 and 11 on the wire: a tr user in an en-US guild, a DM, an
/// unknown code.
#[tokio::test]
async fn interactions_resolve_user_then_guild() {
    let log = Log::default();
    let p = Pipeline::start_with(where_module(&log), |b| {
        b.locales(STRINGS);
    })
    .await;
    let tr_in_en = json!({ "locale": "tr", "guild_locale": "en-US", "guild_id": "9" });
    p.run(run_in("where", tr_in_en.clone())).await;
    p.run(run_in("mine", tr_in_en)).await;
    p.run(run_in("where", json!({ "locale": "tr" }))).await;
    p.run(run_in(
        "where",
        json!({ "locale": "xx-YY", "guild_locale": "de", "guild_id": "9" }),
    ))
    .await;
    assert_eq!(
        log.lines(),
        ["tr en-US en-US", "tr en-US tr", "tr tr tr", "de de de"]
    );
}

/// Row 25: an interaction's `guild_locale` refreshes an existing record
/// only.
#[tokio::test]
async fn interactions_refresh_existing_records() {
    let log = Log::default();
    let p = Pipeline::start_with(where_module(&log), |b| {
        b.locales(STRINGS);
    })
    .await;
    let locales = &p.fx.app.shared().locales;
    locales.observe(&guild_create(9, "tr"));
    p.run(run_in(
        "where",
        json!({ "locale": "tr", "guild_locale": "de", "guild_id": "9" }),
    ))
    .await;
    p.run(run_in(
        "where",
        json!({ "locale": "tr", "guild_locale": "de", "guild_id": "10" }),
    ))
    .await;
    assert_eq!(locales.guild(Some(Id::new(9))), Some(Locale::De));
    assert_eq!(locales.guild(Some(Id::new(10))), None);
}

/// Row 28: the Dispatcher's Guild Locale comes from the guild record.
#[tokio::test]
async fn dispatcher_reads_the_guild_record() {
    let log = Log::default();
    let p = Pipeline::start_with(where_module(&log), |b| {
        b.locales(STRINGS);
    })
    .await;
    p.fx.app.shared().locales.observe(&guild_create(4, "tr"));
    let invocation = CommandInvocation::new(
        CommandInput::new("where"),
        Id::new(5),
        Id::new(6),
        Id::new(3),
    )
    .guild_id(Some(Id::new(4)))
    .locale("de");
    p.dispatcher().run_command(invocation).await.unwrap();
    assert_eq!(log.lines(), ["de tr tr"]);
}

/// Rows 29 and 31: a resolver's value is used verbatim (no pt-BR file:
/// keys render through the catalog's fallback); it sees the parsed codes
/// and the built-in resolution.
#[tokio::test]
async fn resolver_overrides_verbatim() {
    let requests: Arc<Mutex<Vec<LocaleRequest>>> = Arc::default();
    let seen = Arc::clone(&requests);
    let log = Log::default();
    let module = where_module(&log).__add(CommandHandler::__new_plain(
        "m::hi",
        &["hi"],
        "d",
        |ctx: CommandCtx, _: CommandResponder| async move { ctx.reply_text(HI).public().await },
    ));
    let p = Pipeline::start_with(module, move |b| {
        b.locales(STRINGS)
            .locale_resolver(move |r: &LocaleRequest| {
                seen.lock().unwrap().push(*r);
                match r.guild_id {
                    Some(id) if id.get() == 7 => Locales {
                        guild: Locale::PtBr,
                        ..r.resolved
                    },
                    _ => r.resolved,
                }
            });
    })
    .await;
    let extra = json!({ "locale": "xx", "guild_locale": "tr", "guild_id": "7" });
    p.run(run_in("where", extra.clone())).await;
    p.run(run_in("hi", extra)).await;
    assert_eq!(log.lines(), ["tr pt-BR pt-BR"]);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].discord_user, None);
    assert_eq!(requests[0].discord_guild, Some(Locale::Tr));
    assert_eq!(requests[0].resolved, Locales::both(Locale::Tr));
    assert_eq!(
        requests[0].invocation,
        Invocation::Command { path: "where" }
    );
    assert_eq!(requests[0].user_id, Some(Id::new(3)));
    let sent = p.requests();
    assert_eq!(
        sent.last().unwrap().2["data"]["components"][0]["content"],
        "Hi"
    );
}

/// Rows 30, 32 and 33: an identity resolver changes nothing; it runs once
/// per Command, context menu and Dispatcher run, and never for an
/// unknown Command.
#[tokio::test]
async fn resolver_once_per_run() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let log = Log::default();
    let menu = {
        let log = log.clone();
        ContextMenuHandler::__new_user(
            "m::show",
            "Show",
            CommandMetadata::new(),
            move |ctx: CommandCtx, _: ResolvedUser, _: CommandResponder| {
                log.push(format!("menu {}", ctx.user_locale()));
                async { Ok(v2("u")) }
            },
        )
    };
    let p = Pipeline::start_with(where_module(&log).__add(menu), move |b| {
        b.locales(STRINGS)
            .locale_resolver(move |r: &LocaleRequest| {
                counter.fetch_add(1, Ordering::SeqCst);
                r.resolved
            });
    })
    .await;
    p.run(run_in("where", json!({ "locale": "tr" }))).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let data = json!({
        "id": "9", "name": "Show", "type": 2, "target_id": "11",
        "resolved": { "users": { "11": user("11") } }
    });
    p.run(inbound(&interaction(data, json!({ "locale": "tr" }))))
        .await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let invocation = CommandInvocation::new(
        CommandInput::new("where"),
        Id::new(5),
        Id::new(6),
        Id::new(3),
    );
    p.dispatcher().run_command(invocation).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    p.run(run_in("nope", json!({ "locale": "tr" }))).await;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(log.lines(), ["tr tr tr", "menu tr", "en-US en-US en-US"]);
}

/// Row 34: a panicking resolver logs one `error` on `noctis::i18n` and
/// the handler runs with the built-in locales.
#[tokio::test]
async fn resolver_panic_keeps_the_builtin_locales() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    tracing::callsite::rebuild_interest_cache();
    let log = Log::default();
    let p = Pipeline::start_with(where_module(&log), |b| {
        b.locales(STRINGS)
            .locale_resolver(|_: &LocaleRequest| -> Locales { panic!("resolver") });
    })
    .await;
    p.run(run_in("where", json!({ "locale": "tr" }))).await;
    assert_eq!(log.lines(), ["tr tr tr"]);
    let lines = capture.lines_with(&["ERROR", "noctis::i18n", "handler=\"m::where\""]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(p.reports().is_empty());
}

/// Rows 36 and 42: a before-Middleware's Guild Locale override (after a
/// resolver) reaches the handler and a public `reply_text`.
#[tokio::test]
async fn middleware_overrides_reach_the_handler_and_replies() {
    let log = Log::default();
    let override_guild = Middleware::before(|ctx: BeforeCtx| async move {
        assert_eq!(ctx.guild_locale(), Locale::EnUs);
        ctx.set_guild_locale(Locale::Tr);
        Ok(Flow::Continue)
    });
    let module = where_module(&log)
        .before(override_guild)
        .__add(CommandHandler::__new_plain(
            "m::hi",
            &["hi"],
            "d",
            |ctx: CommandCtx, _: CommandResponder| async move { ctx.reply_text(HI).public().await },
        ));
    let p = Pipeline::start_with(module, |b| {
        b.locales(STRINGS)
            .locale_resolver(|_: &LocaleRequest| Locales::both(Locale::EnUs));
    })
    .await;
    p.run(run_in("where", json!({ "locale": "de" }))).await;
    p.run(run_in("hi", json!({ "locale": "de" }))).await;
    assert_eq!(log.lines(), ["en-US tr tr"]);
    let sent = p.requests();
    assert_eq!(
        sent.last().unwrap().2["data"]["components"][0]["content"],
        "Selam"
    );
}

/// Row 39: a User Locale override reaches the notices of a stop and of a
/// user error.
#[tokio::test]
async fn overrides_reach_the_notices() {
    let stop = Middleware::before(|ctx: BeforeCtx| async move {
        ctx.set_user_locale(Locale::Tr);
        Ok(
            if ctx.invocation() == (Invocation::Command { path: "stop" }) {
                Flow::Stop(Some(STOP))
            } else {
                Flow::Continue
            },
        )
    });
    let module = Module::new("m")
        .before(stop)
        .__add(CommandHandler::__new_plain(
            "m::stop",
            &["stop"],
            "d",
            |_: CommandCtx, _: CommandResponder| async { Ok(v2("never")) },
        ))
        .__add(CommandHandler::__new_plain(
            "m::fail",
            &["fail"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                Err::<V2Message, _>(HandlerError::user(STOP))
            },
        ));
    let p = Pipeline::start_with(module, |b| {
        b.locales(STRINGS);
    })
    .await;
    p.run(run_in("stop", json!({ "locale": "en-US" }))).await;
    p.run(run_in("fail", json!({ "locale": "en-US" }))).await;
    let sent = p.requests();
    assert_eq!(sent.len(), 2, "{sent:?}");
    for request in &sent {
        assert_notice(request, "Durdu");
    }
}

/// Row 43: an override made after the auto-defer still reaches the later
/// edit's keys.
#[tokio::test]
async fn override_after_the_auto_defer() {
    let slow = Middleware::before(|ctx: BeforeCtx| async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        ctx.set_locale(Locale::Tr);
        Ok(Flow::Continue)
    });
    let module = Module::new("m")
        .before(slow)
        .__add(CommandHandler::__new_plain(
            "m::keyed",
            &["keyed"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                let button = noctis_ui::__macro::build_button(
                    TBuy { item: 1 },
                    &noctis_ui::__macro::ButtonDefaults {
                        style: noctis_ui::ButtonStyle::Primary,
                        label: Some(noctis_ui::__macro::DefaultText::Key(HI)),
                        emoji: None,
                        disabled: false,
                    },
                );
                Ok(V2Message::new(
                    Components::new().push(ActionRow::buttons([button])),
                ))
            },
        ));
    let p = Pipeline::start_with(module, |b| {
        b.locales(STRINGS).ack_deadline(Duration::from_millis(50));
    })
    .await;
    p.run(run_in("keyed", json!({ "locale": "en-US" }))).await;
    let sent = p.requests();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0].2["type"], 5, "{sent:?}");
    assert_eq!(sent[1].0, Method::PATCH, "{sent:?}");
    assert_eq!(
        sent[1].2["components"][0]["components"][0]["label"], "Selam",
        "{sent:?}"
    );
}

/// Row 48: `Debug` prints the resolved pair.
#[tokio::test]
async fn debug_prints_the_resolved_pair() {
    let seen: Arc<Mutex<Option<String>>> = Arc::default();
    let record = Arc::clone(&seen);
    let module = Module::new("m").__add(CommandHandler::__new_plain(
        "m::dbg",
        &["dbg"],
        "d",
        move |ctx: CommandCtx, _: CommandResponder| {
            *record.lock().unwrap() = Some(format!("{ctx:?}"));
            async { Ok(v2("ok")) }
        },
    ));
    let p = Pipeline::start_with(module, |b| {
        b.locales(STRINGS);
    })
    .await;
    p.run(run_in(
        "dbg",
        json!({ "locale": "tr", "guild_locale": "de", "guild_id": "9" }),
    ))
    .await;
    let debug = seen.lock().unwrap().take().unwrap();
    assert!(
        debug.contains("user_locale: Tr, guild_locale: De"),
        "{debug}"
    );
}
