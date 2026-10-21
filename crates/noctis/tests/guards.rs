//! Route Guards through the facade (Matrix 4, 15, 16, 24, 38, and a
//! guarded derive inside a `macro_rules!`):
//! real `#[derive(Button)]`, `#[derive(StringSelectMenu)]` and
//! `#[derive(Modal)]` structs with `owner`, `permissions` and `guard`,
//! mounted with `App::builder()` and dispatched through the component
//! pipeline (detached: no request is sent; the after-Middleware sees each
//! run's outcome).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;

use noctis::__private::codec::{Parts, SelectNonce, encode};
use noctis::__private::core::__Guards;
use noctis::__private::core::__testing::{dispatch_detached, responded};
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::{Permissions, Runtime};

/// What the handlers, Guards and Middleware saw, in order.
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log(line: impl Into<String>) {
    LOG.lock().unwrap().push(line.into());
}

fn take_log() -> Vec<String> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

mod auth {
    use super::{Ban, Form, Wrapped, log};
    use noctis::prelude::*;

    /// Staff only: `target` 0 is refused with its own key.
    #[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
    pub async fn is_staff(_ctx: &ComponentCtx, this: &Ban) -> HandlerResult<Access> {
        log(format!("is_staff {}", this.target));
        Ok(if this.target == 0 {
            Access::Deny(MessageKey::new("mod.staff_only"))
        } else {
            Access::Allow
        })
    }

    /// `Wrapped`'s Guard, reached through a `$g:path` fragment.
    #[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
    pub async fn wrapped_ok(_ctx: &ComponentCtx, this: &Wrapped) -> HandlerResult<Access> {
        log(format!("wrapped_ok {}", this.owner));
        Ok(Access::Allow)
    }

    /// Any submit is fine.
    #[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
    pub async fn form_ok(_ctx: &ModalSubmitCtx, _this: &Form) -> HandlerResult<Access> {
        log("form_ok");
        Ok(Access::Allow)
    }
}

/// Every Guard kind on a button.
#[derive(Button)]
#[button(
    "g.ban",
    owner = user,
    permissions = MANAGE_GUILD,
    guard = auth::is_staff,
    response = ephemeral,
    defaults(style = Danger, label = "Ban"),
)]
struct Ban {
    user: Id<UserMarker>,
    target: u64,
}

impl Ban {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        log(format!(
            "ban {} {:?}",
            self.target,
            ctx.member_permissions().map(|p| p.bits())
        ));
        Ok(responded())
    }
}

/// Matrix 4: every permission name, joined (the union, ADMINISTRATOR
/// included).
#[derive(StringSelectMenu)]
#[string_select(
    "g.all",
    permissions = CREATE_INVITE | KICK_MEMBERS | BAN_MEMBERS | ADMINISTRATOR | MANAGE_CHANNELS
        | MANAGE_GUILD | ADD_REACTIONS | VIEW_AUDIT_LOG | PRIORITY_SPEAKER | STREAM | VIEW_CHANNEL
        | SEND_MESSAGES | SEND_TTS_MESSAGES | MANAGE_MESSAGES | EMBED_LINKS | ATTACH_FILES
        | READ_MESSAGE_HISTORY | MENTION_EVERYONE | USE_EXTERNAL_EMOJIS | VIEW_GUILD_INSIGHTS
        | CONNECT | SPEAK | MUTE_MEMBERS | DEAFEN_MEMBERS | MOVE_MEMBERS | USE_VAD
        | CHANGE_NICKNAME | MANAGE_NICKNAMES | MANAGE_ROLES | MANAGE_WEBHOOKS
        | MANAGE_GUILD_EXPRESSIONS | USE_SLASH_COMMANDS | REQUEST_TO_SPEAK | MANAGE_EVENTS
        | MANAGE_THREADS | CREATE_PUBLIC_THREADS | CREATE_PRIVATE_THREADS | USE_EXTERNAL_STICKERS
        | SEND_MESSAGES_IN_THREADS | USE_EMBEDDED_ACTIVITIES | MODERATE_MEMBERS
        | VIEW_CREATOR_MONETIZATION_ANALYTICS | USE_SOUNDBOARD | CREATE_GUILD_EXPRESSIONS
        | CREATE_EVENTS | USE_EXTERNAL_SOUNDS | SEND_VOICE_MESSAGES | SEND_POLLS
        | USE_EXTERNAL_APPS | PIN_MESSAGES | BYPASS_SLOWMODE,
    defaults(placeholder = "Pick"),
)]
struct All;

impl All {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        log("all");
        Ok(responded())
    }
}

/// A modal whose owner is a Payload field.
#[derive(Modal)]
#[modal("g.form", owner = user, guard = auth::form_ok, defaults(title = "Form"))]
struct Form {
    user: Id<UserMarker>,
    #[text_input("Bio")]
    bio: String,
}

impl Form {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        log(format!("form {} {}", self.user, self.bio));
        Ok(responded())
    }
}

/// A guarded button declared by a `macro_rules!` that takes the owner
/// field as `$owner:ident` and the custom Guard as `$g:path`: the derive's
/// generated `self`/`ctx` must resolve under the macro's hygiene.
macro_rules! guarded_button {
    ($name:ident, $owner:ident, $g:path) => {
        #[derive(Button)]
        #[button("g.wrapped", owner = $owner, guard = $g, defaults(style = Primary, label = "Go"))]
        struct $name {
            $owner: Id<UserMarker>,
        }

        impl $name {
            #[handler]
            async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
                log(format!("wrapped {}", self.$owner));
                Ok(responded())
            }
        }
    };
}

guarded_button!(Wrapped, owner, auth::wrapped_ok);

/// Matrix 15: no Guard argument.
#[derive(Button)]
#[button("g.open", defaults(style = Primary, label = "Open"))]
struct Open;

impl Open {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        log(format!(
            "open {:?}",
            ctx.member_permissions().map(|p| p.bits())
        ));
        Ok(responded())
    }
}

async fn audit(ctx: AfterCtx) {
    log(format!("after {:?}", ctx.outcome()));
}

fn app() -> App {
    App::builder()
        .runtime(&Runtime::for_test())
        .module(
            Module::new("g")
                .after(Middleware::after(audit))
                .add::<Ban>()
                .add::<All>()
                .add::<Form>()
                .add::<Open>()
                .add::<Wrapped>(),
        )
        .build()
        .unwrap()
}

fn user(id: u64) -> String {
    format!(
        r#"{{"id":"{id}","username":"u","discriminator":"0000","avatar":null,"global_name":null}}"#
    )
}

/// A click by `by` on `custom_id` (component `kind`, with `values`):
/// in guild 9 with the member `permissions`, or in a DM for `None`.
fn click(custom_id: &str, kind: u8, by: u64, permissions: Option<&str>) -> String {
    let who = match permissions {
        Some(p) => format!(
            r#""guild_id":"9","member":{{"user":{},"roles":[],"joined_at":"2024-01-01T00:00:00.000000+00:00","deaf":false,"mute":false,"flags":0,"permissions":"{p}"}}"#,
            user(by)
        ),
        None => format!(r#""user":{}"#, user(by)),
    };
    format!(
        r#"{{"id":"1000","application_id":"7","type":3,"token":"tok","version":1,
        "authorizing_integration_owners":{{}},"entitlements":[],{who},
        "data":{{"custom_id":"{custom_id}","component_type":{kind},"values":["a"]}}}}"#
    )
}

/// A submit of `Form` whose Payload `user` is `owner_b62` (base62), by
/// `by`.
fn submit(owner_b62: &str, by: u64) -> String {
    format!(
        r#"{{"id":"1001","application_id":"7","type":5,"token":"tok","version":1,
        "authorizing_integration_owners":{{}},"entitlements":[],"user":{},
        "data":{{"custom_id":"0g.form:{owner_b62}","components":[
            {{"type":18,"id":1,"component":{{"type":4,"id":2,"custom_id":"bio","value":"hi"}}}}]}}}}"#,
        user(by)
    )
}

fn ban(user: u64, target: u64) -> String {
    encode::<Ban>(
        &Ban {
            user: Id::new(user),
            target,
        },
        &Parts::new(),
    )
    .unwrap()
    .to_string()
}

#[test]
fn guarded_flags() {
    const { assert!(<Ban as __Guards<ComponentCtx>>::GUARDED) };
    const { assert!(<All as __Guards<ComponentCtx>>::GUARDED) };
    const { assert!(<Form as __Guards<ModalSubmitCtx>>::GUARDED) };
    const { assert!(!<Open as __Guards<ComponentCtx>>::GUARDED) };
    const { assert!(<Wrapped as __Guards<ComponentCtx>>::GUARDED) };
}

/// The runs share [`LOG`], so they run in one test, in order.
#[tokio::test(flavor = "multi_thread")]
async fn guarded_routes_through_the_pipeline() {
    let app = app();
    take_log();
    every_guard_kind_on_a_button(&app).await;
    every_permission_name_on_a_select(&app).await;
    modal_owner_and_custom_guard(&app).await;
    guarded_derive_inside_a_macro(&app).await;
}

/// The `macro_rules!`-declared button: its owner Guard and its `$g:path`
/// custom Guard both run.
async fn guarded_derive_inside_a_macro(app: &App) {
    let id = encode::<Wrapped>(&Wrapped { owner: Id::new(42) }, &Parts::new())
        .unwrap()
        .to_string();
    dispatch_detached(app, &click(&id, 2, 42, None)).await;
    assert_eq!(
        take_log(),
        ["wrapped_ok 42", "wrapped 42", "after Completed"]
    );
    dispatch_detached(app, &click(&id, 2, 3, None)).await;
    assert_eq!(take_log(), ["after Denied"]);
}

/// Matrix 16, 18, 20, 24 and 38: the owner with `MANAGE_GUILD` (or
/// ADMINISTRATOR) passes the built-in Guards, then the custom Guard
/// decides; anyone else is denied before it runs.
async fn every_guard_kind_on_a_button(app: &App) {
    dispatch_detached(app, &click(&ban(42, 1), 2, 42, Some("32"))).await;
    assert_eq!(
        take_log(),
        ["is_staff 1", "ban 1 Some(32)", "after Completed"]
    );
    dispatch_detached(app, &click(&ban(42, 1), 2, 42, Some("8"))).await;
    assert_eq!(
        take_log(),
        ["is_staff 1", "ban 1 Some(8)", "after Completed"]
    );
    // The custom Guard's own key.
    dispatch_detached(app, &click(&ban(42, 0), 2, 42, Some("32"))).await;
    assert_eq!(take_log(), ["is_staff 0", "after Denied"]);
    // Another user, a missing permission, a DM: no custom Guard call.
    dispatch_detached(app, &click(&ban(42, 1), 2, 43, Some("32"))).await;
    dispatch_detached(app, &click(&ban(42, 1), 2, 42, Some("16"))).await;
    dispatch_detached(app, &click(&ban(42, 1), 2, 42, None)).await;
    assert_eq!(take_log(), ["after Denied", "after Denied", "after Denied"]);
    // Matrix 15 and 38: unguarded, and `member_permissions()`.
    let open = encode::<Open>(&Open, &Parts::new()).unwrap().to_string();
    dispatch_detached(app, &click(&open, 2, 3, Some("32"))).await;
    dispatch_detached(app, &click(&open, 2, 3, None)).await;
    assert_eq!(
        take_log(),
        [
            "open Some(32)",
            "after Completed",
            "open None",
            "after Completed"
        ]
    );
}

/// Matrix 4: all 51 names; only a member holding every one (or
/// ADMINISTRATOR) passes.
async fn every_permission_name_on_a_select(app: &App) {
    let parts = Parts::new().with_nonce(SelectNonce::new(1).unwrap());
    let id = encode::<All>(&All, &parts).unwrap().to_string();
    let all = Permissions::all().bits().to_string();
    dispatch_detached(app, &click(&id, 3, 3, Some("8"))).await;
    dispatch_detached(app, &click(&id, 3, 3, Some(&all))).await;
    dispatch_detached(app, &click(&id, 3, 3, Some("32"))).await;
    assert_eq!(
        take_log(),
        [
            "all",
            "after Completed",
            "all",
            "after Completed",
            "after Denied"
        ]
    );
}

/// Matrix 34 (facade): a modal's owner Guard.
async fn modal_owner_and_custom_guard(app: &App) {
    // 42 is `g` in base62.
    dispatch_detached(app, &submit("g", 42)).await;
    assert_eq!(take_log(), ["form_ok", "form 42 hi", "after Completed"]);
    dispatch_detached(app, &submit("g", 3)).await;
    assert_eq!(take_log(), ["after Denied"]);
}
