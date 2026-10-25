//! `#[user_command]` and `#[message_command]` end to end through the JSON
//! test seam (Story 6.3, Matrix 29, 30, 36 and 45-55).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;

use noctis::__private::core::__testing::call_context_menu;
use noctis::prelude::*;
use noctis::ui::{ResolvedMessage, ResolvedUser};
use noctis::{
    CommandMetadata, ContextMenuHandler, HandlerError, IntegrationType, InteractionContext,
    MenuKind, MenuTargetError, Permissions, UnknownCommandPath,
};
use serde_json::{Value, json};

#[deny(missing_docs)]
mod menus {
    //! Generated descriptors keep their docs (Matrix 44).

    use std::sync::Mutex;

    use noctis::prelude::*;
    use noctis::ui::{ResolvedMessage, ResolvedUser};

    /// The `(menu name, user id, has member)` triples `show` received.
    pub static SHOWN: Mutex<Vec<(String, u64, bool)>> = Mutex::new(Vec::new());
    /// The `(menu name, message id)` pairs `quote` received.
    pub static QUOTED: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

    /// Shows a profile.
    #[derive(UserCommand)]
    #[user_command("Show profile")]
    pub(crate) struct Show;

    impl Show {
        #[handler]
        async fn handle(
            self,
            ctx: CommandCtx,
            target: ResolvedUser,
        ) -> HandlerResult<noctis::ui::LegacyMessage> {
            SHOWN.lock().unwrap().push((
                ctx.command_path().to_owned(),
                target.user.id.get(),
                target.member.is_some(),
            ));
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    /// Quotes a message.
    #[derive(MessageCommand)]
    #[message_command("Quote")]
    pub struct Quote;

    impl Quote {
        #[handler]
        async fn handle(
            self,
            ctx: CommandCtx,
            target: ResolvedMessage,
        ) -> HandlerResult<noctis::ui::LegacyMessage> {
            QUOTED
                .lock()
                .unwrap()
                .push((ctx.command_path().to_owned(), target.message.id.get()));
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }
}

use menus::{QUOTED, Quote, SHOWN, Show};

#[derive(UserCommand)]
#[user_command(
    "Moderate",
    permissions = KICK_MEMBERS,
    contexts = [Guild],
    integration_types = [GuildInstall],
    nsfw = false
)]
struct Moderate;

impl Moderate {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedUser,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

static UNICODE: Mutex<Vec<u64>> = Mutex::new(Vec::new());

#[derive(UserCommand)]
#[user_command("Profili göster 🚀")]
struct Unicode;

impl Unicode {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        target: ResolvedUser,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        UNICODE.lock().unwrap().push(target.user.id.get());
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(UserCommand)]
#[user_command("Fail")]
struct Fail;

impl Fail {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedUser,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Err(HandlerError::new(std::fmt::Error))
    }
}

fn user(id: &str) -> Value {
    json!({ "id": id, "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn member() -> Value {
    json!({
        "roles": [], "joined_at": "2024-01-01T00:00:00.000000+00:00",
        "deaf": false, "mute": false, "flags": 0, "pending": false, "permissions": "0"
    })
}

fn message(id: &str) -> Value {
    json!({
        "id": id, "channel_id": "2", "author": user("3"), "content": "ping",
        "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
        "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
        "attachments": [], "embeds": [], "pinned": false, "type": 0
    })
}

fn interaction(kind: u64, name: &str, target: &Value, resolved: Option<Value>) -> Vec<u8> {
    let mut data = json!({ "type": kind, "name": name, "target_id": target });
    if let Some(r) = resolved {
        data["resolved"] = r;
    }
    json!({ "type": 2, "data": data }).to_string().into_bytes()
}

async fn run(handler: &ContextMenuHandler, json: &[u8]) -> Result<(), HandlerError> {
    call_context_menu(handler, json).await
}

fn menu_error(result: Result<(), HandlerError>) -> MenuTargetError {
    let err = result.expect_err("expected an error");
    err.as_error()
        .downcast_ref::<MenuTargetError>()
        .unwrap_or_else(|| panic!("not a MenuTargetError: {err}"))
        .clone()
}

fn shown(id: u64) -> Option<(String, u64, bool)> {
    SHOWN
        .lock()
        .unwrap()
        .iter()
        .find(|(_, i, _)| *i == id)
        .cloned()
}

/// Matrix 29 and 30: the descriptors.
#[test]
fn ctx_03_descriptors() {
    let handler = Show::descriptor();
    assert_eq!(handler.kind(), MenuKind::User);
    assert_eq!(handler.menu_name(), "Show profile");
    assert!(handler.name().ends_with("::Show"), "{}", handler.name());
    assert_eq!(handler.metadata(), &CommandMetadata::new());
    assert_eq!(Quote::descriptor().kind(), MenuKind::Message);
    let module = Module::new("profile").add::<Show>().add::<Quote>();
    assert!(
        format!("{module:?}").contains("menus: [\"Show profile\", \"Quote\"]"),
        "{module:?}"
    );
}

/// Matrix 36: metadata on a context menu.
#[test]
fn meta_01_menu_metadata() {
    let handler = Moderate::descriptor();
    let meta = handler.metadata();
    assert_eq!(
        meta.default_member_permissions,
        Some(Permissions::KICK_MEMBERS)
    );
    assert_eq!(meta.contexts, Some(&[InteractionContext::Guild][..]));
    assert_eq!(
        meta.integration_types,
        Some(&[IntegrationType::GuildInstall][..])
    );
    assert_eq!(meta.nsfw, Some(false));
    assert!(meta.guilds.is_empty());
}

/// Matrix 45 and 46: a user target with and without its member.
#[tokio::test]
async fn ctx_03_user_target() {
    let resolved = json!({ "users": { "21": user("21") }, "members": { "21": member() } });
    run(
        &Show::descriptor(),
        &interaction(2, "Show profile", &json!("21"), Some(resolved)),
    )
    .await
    .unwrap();
    assert_eq!(shown(21), Some(("Show profile".to_owned(), 21, true)));

    let resolved = json!({ "users": { "22": user("22") } });
    run(
        &Show::descriptor(),
        &interaction(2, "Show profile", &json!("22"), Some(resolved)),
    )
    .await
    .unwrap();
    assert_eq!(shown(22), Some(("Show profile".to_owned(), 22, false)));
}

/// Matrix 47: a message target.
#[tokio::test]
async fn ctx_03_message_target() {
    let resolved = json!({ "messages": { "31": message("31") } });
    run(
        &Quote::descriptor(),
        &interaction(3, "Quote", &json!("31"), Some(resolved)),
    )
    .await
    .unwrap();
    assert!(QUOTED.lock().unwrap().contains(&("Quote".to_owned(), 31)));
}

/// Matrix 48: the target is missing.
#[tokio::test]
async fn target_unresolved() {
    let resolved = json!({ "users": { "21": user("21") } });
    for json in [
        interaction(2, "Show profile", &json!("99"), Some(resolved)),
        interaction(2, "Show profile", &json!("99"), None),
    ] {
        let err = menu_error(run(&Show::descriptor(), &json).await);
        assert_eq!(err, MenuTargetError::Unresolved { id: 99 });
        assert_eq!(err.to_string(), "target 99 is not in the resolved data");
    }
    assert!(shown(99).is_none());
}

/// Matrix 49: the wrong kind.
#[tokio::test]
async fn ctx_03_wrong_kind() {
    let err = menu_error(
        run(
            &Show::descriptor(),
            &interaction(3, "Show profile", &json!("1"), None),
        )
        .await,
    );
    assert_eq!(
        err,
        MenuTargetError::WrongKind {
            expected: MenuKind::User,
            found: MenuKind::Message
        }
    );
    assert_eq!(
        err.to_string(),
        "expected a USER command, got a MESSAGE command"
    );
    let err = menu_error(
        run(
            &Quote::descriptor(),
            &interaction(2, "Quote", &json!("1"), None),
        )
        .await,
    );
    assert_eq!(
        err,
        MenuTargetError::WrongKind {
            expected: MenuKind::Message,
            found: MenuKind::User
        }
    );
}

/// Matrix 50: not a context-menu command.
#[tokio::test]
async fn ctx_03_not_a_context_menu() {
    for (json, interaction_type, command_type) in [
        (
            json!({ "type": 2, "data": { "type": 1, "name": "x" } }),
            2,
            1,
        ),
        (json!({ "type": 2, "data": { "name": "x" } }), 2, 1),
        (
            json!({ "type": 2, "data": { "type": 4, "name": "x" } }),
            2,
            4,
        ),
        (
            json!({ "type": 3, "data": { "type": 2, "name": "x" } }),
            3,
            2,
        ),
    ] {
        let err = menu_error(run(&Show::descriptor(), json.to_string().as_bytes()).await);
        assert_eq!(
            err,
            MenuTargetError::NotContextMenu {
                interaction_type,
                command_type
            }
        );
    }
    assert_eq!(
        MenuTargetError::NotContextMenu {
            interaction_type: 2,
            command_type: 1
        }
        .to_string(),
        "not a context-menu command: interaction type 2, command type 1"
    );
}

/// Matrix 51: the name must match byte for byte.
#[tokio::test]
async fn name_mismatch() {
    let err = run(
        &Show::descriptor(),
        &interaction(2, "Show Profile", &json!("1"), None),
    )
    .await
    .unwrap_err();
    let unknown = err
        .as_error()
        .downcast_ref::<UnknownCommandPath>()
        .unwrap_or_else(|| panic!("{err}"));
    assert_eq!(
        unknown.to_string(),
        format!(
            "`Show Profile` is not a path of the Command `{}::menus::Show`",
            module_path!()
        )
    );
}

/// Matrix 52: a Unicode menu name.
#[tokio::test]
async fn unicode_name() {
    let resolved = json!({ "users": { "41": user("41") } });
    run(
        &Unicode::descriptor(),
        &interaction(2, "Profili göster 🚀", &json!("41"), Some(resolved)),
    )
    .await
    .unwrap();
    assert!(UNICODE.lock().unwrap().contains(&41));
}

/// Matrix 53: malformed interactions.
#[tokio::test]
async fn malformed() {
    for (json, reason) in [
        (b"{".to_vec(), "invalid JSON: "),
        (b"1".to_vec(), "the interaction is not an object"),
        (b"{}".to_vec(), "missing interaction `type`"),
        (
            br#"{"type":2,"data":[]}"#.to_vec(),
            "`data` is not an object",
        ),
        (br#"{"type":2}"#.to_vec(), "missing `data`"),
        (
            br#"{"type":2,"data":{"type":2.5}}"#.to_vec(),
            "`data.type` is not an integer",
        ),
        (
            br#"{"type":2,"data":{"type":2,"target_id":"1"}}"#.to_vec(),
            "missing `data.name`",
        ),
        (
            br#"{"type":2,"data":{"type":2,"name":"Show profile"}}"#.to_vec(),
            "missing `data.target_id`",
        ),
        (
            interaction(2, "Show profile", &json!("0"), None),
            "bad `data.target_id`: ",
        ),
        (
            interaction(2, "Show profile", &json!("abc"), None),
            "bad `data.target_id`: ",
        ),
        (
            interaction(2, "Show profile", &json!(true), None),
            "bad `data.target_id`: ",
        ),
        (
            interaction(2, "Show profile", &json!("1"), Some(json!({ "users": [] }))),
            "bad `resolved` data: ",
        ),
    ] {
        let err = menu_error(run(&Show::descriptor(), &json).await);
        let MenuTargetError::Malformed { reason: got } = &err else {
            panic!("{err}");
        };
        assert!(got.starts_with(reason), "{got} / {reason}");
        assert!(
            err.to_string()
                .starts_with("malformed context-menu interaction: ")
        );
    }
}

/// Matrix 54: a numeric `target_id`.
#[tokio::test]
async fn numeric_target_id() {
    let resolved = json!({ "users": { "51": user("51") } });
    run(
        &Show::descriptor(),
        &interaction(2, "Show profile", &json!(51), Some(resolved)),
    )
    .await
    .unwrap();
    assert!(shown(51).is_some());
}

/// Matrix 55: a handler error passes through unchanged.
#[tokio::test]
async fn handler_error_passes_through() {
    let resolved = json!({ "users": { "1": user("1") } });
    let err = run(
        &Fail::descriptor(),
        &interaction(2, "Fail", &json!("1"), Some(resolved)),
    )
    .await
    .unwrap_err();
    assert!(err.as_error().is::<std::fmt::Error>());
}

/// `ResolvedMessage` is a public type with its lookup.
#[test]
fn resolved_message_lookup() {
    let resolved: noctis::twilight::model::application::interaction::InteractionDataResolved =
        serde_json::from_value(json!({ "messages": { "7": message("7") } })).unwrap();
    let found = ResolvedMessage::from_resolved(&resolved, Id::new(7)).unwrap();
    assert_eq!(found.message.content, "ping");
}
