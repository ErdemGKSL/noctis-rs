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

#[event]
async fn plain(_ctx: EventCtx, _msg: MessageCreate) -> HandlerResult<()> {
    Ok(())
}

#[event(ordered)]
async fn only_ordered(_: EventCtx, _: MessageDelete) -> HandlerResult<()> {
    Ok(())
}

#[event(droppable)]
async fn only_droppable(_: EventCtx, _: MessageDelete) -> HandlerResult<()> {
    Ok(())
}

#[event(ordered, droppable)]
async fn both(_: EventCtx, _: MessageDelete) -> HandlerResult<()> {
    Ok(())
}

#[event(droppable, ordered)]
async fn both_reversed(_: EventCtx, _: MessageDelete) -> HandlerResult<()> {
    Ok(())
}

mod nested {
    use super::*;

    /// A documented, public handler.
    #[event]
    pub async fn documented(ctx: EventCtx, ev: UserUpdate) -> HandlerResult<()> {
        let _ = (ctx.shard(), ev.id);
        Ok(())
    }
}

#[event]
async fn grüße(_: EventCtx, _: UserUpdate) -> HandlerResult<()> {
    Ok(())
}

#[event]
async fn greet(ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> {
    let _ = (ctx.guild_id(), ev.user.id);
    Ok(())
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
        &plain(),
        &format!("{here}::plain"),
        "MESSAGE_CREATE",
        false,
        false,
    );
    expect(
        &only_ordered(),
        &format!("{here}::only_ordered"),
        "MESSAGE_DELETE",
        true,
        false,
    );
    expect(
        &only_droppable(),
        &format!("{here}::only_droppable"),
        "MESSAGE_DELETE",
        false,
        true,
    );
    expect(
        &both(),
        &format!("{here}::both"),
        "MESSAGE_DELETE",
        true,
        true,
    );
    expect(
        &both_reversed(),
        &format!("{here}::both_reversed"),
        "MESSAGE_DELETE",
        true,
        true,
    );
    expect(
        &nested::documented(),
        &format!("{here}::nested::documented"),
        "USER_UPDATE",
        false,
        false,
    );
    expect(
        &grüße(),
        &format!("{here}::grüße"),
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
        .module(Module::new("profile").add(plain()))
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MESSAGES);
}

/// Matrix 11: a missing privileged intent fails the build with GW-05.
#[tokio::test]
async fn gw_05_missing_privileged_intent() {
    let err = App::builder()
        .runtime(&Runtime::for_test())
        .module(Module::new("welcome").add(greet()))
        .build()
        .unwrap_err();
    let path = format!("{}::greet", module_path!());
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
                .add(greet())
                .add(nested::documented()),
        )
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MEMBERS);
}
