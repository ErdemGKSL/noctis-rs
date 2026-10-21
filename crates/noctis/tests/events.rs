//! `#[event]` through the facade (Story 5.6, Matrix 3, 11 and 42).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::events::{EventCtx, EventHandler, MemberAdd, MessageCreate, MessageDelete, UserUpdate};
use noctis::prelude::*;
use noctis::twilight::model::gateway::Intents;
use noctis::{BuildViolation, Runtime};

#[derive(EventHandler)]
struct Plain;

impl Plain {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _msg: MessageCreate) -> HandlerResult<()> {
        Ok(())
    }
}

#[derive(EventHandler)]
#[event(ordered)]
struct OnlyOrdered;

impl OnlyOrdered {
    #[handler]
    async fn handle(self, _: EventCtx, _: MessageDelete) -> HandlerResult<()> {
        Ok(())
    }
}

#[derive(EventHandler)]
#[event(droppable)]
struct OnlyDroppable;

impl OnlyDroppable {
    #[handler]
    async fn handle(self, _: EventCtx, _: MessageDelete) -> HandlerResult<()> {
        Ok(())
    }
}

#[derive(EventHandler)]
#[event(ordered, droppable)]
struct Both;

impl Both {
    #[handler]
    async fn handle(self, _: EventCtx, _: MessageDelete) -> HandlerResult<()> {
        Ok(())
    }
}

#[derive(EventHandler)]
#[event(droppable, ordered)]
struct BothReversed;

impl BothReversed {
    #[handler]
    async fn handle(self, _: EventCtx, _: MessageDelete) -> HandlerResult<()> {
        Ok(())
    }
}

mod nested {
    use super::*;

    /// A documented, public handler.
    #[derive(EventHandler)]
    pub struct Documented;

    impl Documented {
        #[handler]
        async fn handle(self, ctx: EventCtx, ev: UserUpdate) -> HandlerResult<()> {
            let _ = (ctx.shard(), ev.id);
            Ok(())
        }
    }
}

#[derive(EventHandler)]
struct Grüße;

impl Grüße {
    #[handler]
    async fn handle(self, _: EventCtx, _: UserUpdate) -> HandlerResult<()> {
        Ok(())
    }
}

#[derive(EventHandler)]
struct Greet;

impl Greet {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> {
        let _ = (ctx.guild_id(), ev.user.id);
        Ok(())
    }
}

fn expect(handler: &EventHandler, path: &str, event: &str, ordered: bool, droppable: bool) {
    assert_eq!(
        format!("{handler:?}"),
        format!(
            "EventHandler {{ handler: {path:?}, event: {event:?}, ordered: {ordered}, droppable: {droppable} }}"
        )
    );
}

/// Matrix 42: every accepted form, its flags and its handler path.
#[test]
fn macro_forms() {
    let here = module_path!();
    expect(
        &Plain::descriptor(),
        &format!("{here}::Plain"),
        "MESSAGE_CREATE",
        false,
        false,
    );
    expect(
        &OnlyOrdered::descriptor(),
        &format!("{here}::OnlyOrdered"),
        "MESSAGE_DELETE",
        true,
        false,
    );
    expect(
        &OnlyDroppable::descriptor(),
        &format!("{here}::OnlyDroppable"),
        "MESSAGE_DELETE",
        false,
        true,
    );
    expect(
        &Both::descriptor(),
        &format!("{here}::Both"),
        "MESSAGE_DELETE",
        true,
        true,
    );
    expect(
        &BothReversed::descriptor(),
        &format!("{here}::BothReversed"),
        "MESSAGE_DELETE",
        true,
        true,
    );
    expect(
        &nested::Documented::descriptor(),
        &format!("{here}::nested::Documented"),
        "USER_UPDATE",
        false,
        false,
    );
    expect(
        &Grüße::descriptor(),
        &format!("{here}::Grüße"),
        "USER_UPDATE",
        false,
        false,
    );
}

/// Matrix 3: a handler derives its intent.
#[tokio::test]
async fn one_handler_derives_intent() {
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .module(Module::new("profile").add::<Plain>())
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MESSAGES);
}

/// Matrix 11: a missing privileged intent fails the build with GW-05.
#[tokio::test]
async fn gw_05_missing_privileged_intent() {
    let err = App::builder()
        .runtime(&Runtime::for_test())
        .module(Module::new("welcome").add::<Greet>())
        .build()
        .unwrap_err();
    let path = format!("{}::Greet", module_path!());
    match err.violations() {
        [
            BuildViolation::MissingIntent {
                module,
                handler,
                event,
                intent,
            },
        ] => {
            assert_eq!(*module, "welcome");
            assert_eq!(*handler, path);
            assert_eq!(*event, "GUILD_MEMBER_ADD");
            assert_eq!(*intent, Intents::GUILD_MEMBERS);
        }
        other => panic!("{other:?}"),
    }
    let text = err.to_string();
    assert!(text.contains("[GW-05]") && text.contains(&path) && text.contains("GUILD_MEMBERS"));

    let app = App::builder()
        .runtime(&Runtime::for_test())
        .intents(Intents::GUILD_MEMBERS)
        .module(
            Module::new("welcome")
                .add::<Greet>()
                .add::<nested::Documented>(),
        )
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MEMBERS);
}
