//! Story 13.3 Matrix D1, D2, D6, D7 and D8: stage 3 answers a disabled
//! Module's Command with the `noctis.module.disabled` notice before any
//! defer, Middleware or handler.

use super::*;
use crate::ModuleScope;

/// `noctis.module.disabled` in en-US.
const DISABLED: &str = "This feature is disabled here.";

/// A `ping` interaction in `guild` (a DM for `None`).
fn ping_in(guild: Option<&str>) -> Value {
    let extra = guild.map_or_else(|| json!({}), |g| json!({ "guild_id": g }));
    interaction(slash("ping", json!([])), extra)
}

/// D1, D2: disabled in guild 1 only; even a late interaction gets the
/// type-4 notice, never an auto-defer.
#[tokio::test]
async fn disabled_command_gets_the_notice() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let p = Pipeline::start(Module::new("m").__add(logging(&log, "m::ping", &["ping"]))).await;
    p.fx.app
        .modules()
        .set_enabled("m", Id::new(1), false)
        .await
        .unwrap();
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    p.run(inbound_ago(
        &ping_in(Some("1")),
        Duration::from_millis(2_500),
    ))
    .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].1, CB);
    assert_eq!(requests[0].2["type"], 4);
    assert_eq!(requests[0].2["data"]["flags"], 32832);
    assert_eq!(requests[0].2["data"]["components"][0]["content"], DISABLED);
    assert!(log.lines().is_empty());
    let debug = capture.lines_with(&["DEBUG", "noctis::dispatch", "module disabled here"]);
    assert_eq!(debug.len(), 1, "{}", capture.text());
    assert!(debug[0].contains("module=\"m\"") && debug[0].contains("path=\"ping\""));
    // D2: another guild and a DM run the handler.
    p.run(inbound(&ping_in(Some("2")))).await;
    p.run(inbound(&ping_in(None))).await;
    assert_eq!(log.lines(), ["m::ping:ping", "m::ping:ping"]);
    assert!(p.reports().is_empty());
}

/// D7: a child Module's Command is disabled with its parent.
#[tokio::test]
async fn ancestor_disables_the_child() {
    let log = Log::default();
    let module =
        Module::new("p").module(Module::new("p/c").__add(logging(&log, "c::ping", &["ping"])));
    let p = Pipeline::start(module).await;
    p.fx.app
        .modules()
        .set_enabled("p", Id::new(1), false)
        .await
        .unwrap();
    p.run(inbound(&ping_in(Some("1")))).await;
    assert!(log.lines().is_empty());
    assert_eq!(
        p.requests()[0].2["data"]["components"][0]["content"],
        DISABLED
    );
}

/// D6: the Dispatcher answers a public notice reply to the message.
#[tokio::test]
async fn dispatcher_answers_a_public_notice() {
    let log = Log::default();
    let p = Pipeline::start(Module::new("m").__add(logging(&log, "m::ping", &["ping"]))).await;
    p.fx.app
        .modules()
        .set_enabled("m", Id::new(1), false)
        .await
        .unwrap();
    let invocation = CommandInvocation::new(
        CommandInput::new("ping"),
        Id::new(5),
        Id::new(6),
        Id::new(3),
    )
    .guild_id(Some(Id::new(1)));
    p.dispatcher().run_command(invocation).await.unwrap();
    assert!(log.lines().is_empty());
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(
        (&requests[0].0, requests[0].1.as_str()),
        (&Method::POST, "/api/v10/channels/5/messages")
    );
    assert_eq!(requests[0].2["flags"], 32768, "public");
    assert_eq!(requests[0].2["components"][0]["content"], DISABLED);
}

/// D8: a locale file overriding `noctis.module.disabled` wins for its
/// locale.
#[tokio::test]
async fn the_notice_is_localized() {
    static OVERRIDE: noctis_i18n::Catalog = noctis_i18n::Catalog::__new(
        "bot/locales",
        Locale::EnUs,
        &[
            (Locale::EnUs, &[]),
            (
                Locale::Tr,
                &[("noctis.module.disabled", "Bu özellik burada kapalı.")],
            ),
        ],
        &[],
    );
    let log = Log::default();
    let p = Pipeline::start_with(
        Module::new("m").__add(logging(&log, "m::ping", &["ping"])),
        |b| {
            b.locales(OVERRIDE);
        },
    )
    .await;
    p.fx.app
        .modules()
        .set_enabled("m", ModuleScope::Global, false)
        .await
        .unwrap();
    let mut raw = ping_in(None);
    raw["locale"] = json!("tr");
    p.run(inbound(&raw)).await;
    assert_eq!(
        p.requests()[0].2["data"]["components"][0]["content"],
        "Bu özellik burada kapalı."
    );
}

mod kinds {
    use noctis_codec::RouteShape;

    use super::super::components::{click, submit};
    use super::*;
    use crate::pipeline::component::run_component;
    use crate::test_support::test_route;
    use crate::{ComponentCtx, ComponentHandler, ComponentKind, ModalHandler, ModalSubmitCtx};

    test_route!(Pick, "pick", RouteShape::INLINE.select(), 0);
    test_route!(Bio, "bio", RouteShape::INLINE, 0);

    /// Module `m`: a select, a modal and a user menu, each logging.
    fn module(log: &Log) -> Module {
        let (pick, bio, menu) = (log.clone(), log.clone(), log.clone());
        Module::new("m")
            .__add(ComponentHandler::__new::<Pick, _, _>(
                "m::Pick",
                "pick",
                ComponentKind::StringSelect,
                move |_: Pick, _: ComponentCtx| {
                    pick.push("pick".to_owned());
                    async { Ok(Responded::new()) }
                },
            ))
            .__add(ModalHandler::__new::<Bio, _, _>(
                "m::Bio",
                "bio",
                move |_: Bio, _: ModalSubmitCtx| {
                    bio.push("bio".to_owned());
                    async { Ok(Responded::new()) }
                },
            ))
            .__add(ContextMenuHandler::__new_user(
                "m::show",
                "Show",
                CommandMetadata::new(),
                move |_: CommandCtx, _: ResolvedUser, _: CommandResponder| {
                    menu.push("show".to_owned());
                    async { Ok(v2("u")) }
                },
            ))
    }

    fn in_guild_1(mut raw: Value) -> Inbound {
        raw["guild_id"] = json!("1");
        inbound(&raw)
    }

    /// A select, a modal submit and a context-menu Command of a disabled
    /// Module each get the notice; no handler runs.
    #[tokio::test]
    async fn every_kind_gets_the_notice() {
        let log = Log::default();
        let p = Pipeline::start(module(&log)).await;
        p.fx.app
            .modules()
            .set_enabled("m", Id::new(1), false)
            .await
            .unwrap();
        let component = |inbound| {
            run_component(
                Arc::clone(p.fx.app.registry()),
                Arc::clone(p.fx.app.shared()),
                inbound,
                CancellationToken::new(),
            )
        };
        component(in_guild_1(click("4pick:0147", 3))).await;
        component(in_guild_1(submit("0bio:47", true))).await;
        let data = json!({
            "id": "9", "name": "Show", "type": 2, "target_id": "11",
            "resolved": { "users": { "11": user("11") } }
        });
        p.run(in_guild_1(interaction(data, json!({})))).await;
        assert!(log.lines().is_empty(), "{:?}", log.lines());
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        for request in &requests {
            assert_eq!(request.1, CB);
            assert_eq!(request.2["type"], 4);
            assert_eq!(request.2["data"]["flags"], 32832);
            assert_eq!(request.2["data"]["components"][0]["content"], DISABLED);
        }
    }
}
