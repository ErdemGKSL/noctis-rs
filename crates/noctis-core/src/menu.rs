//! Context-menu commands: the [`ContextMenuHandler`] descriptor
//! `#[user_command]` and `#[message_command]` expand to, and the
//! [`MenuTargetError`] a call returns when the interaction does not carry its
//! target (FR-12, AD-26; CTX-01..03).

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use noctis_rules::command::MenuKind;
use noctis_ui::{ResolvedMessage, ResolvedUser};
use serde_json::Value;
use twilight_model::application::interaction::InteractionDataResolved;
use twilight_model::id::Id;
use twilight_model::id::marker::GenericMarker;

use crate::command::{CommandCtx, HandlerFuture, boxed, failed, run_detached, unknown};
use crate::handler::{HandlerError, HandlerResult};
use crate::metadata::CommandMetadata;
use crate::options::{CommandInput, empty_resolved};
use crate::responder::{
    Command, CommandResponder, DeferMode, Ephemerality, IntoResponse, Responder, Shared,
};

/// Why a context-menu interaction did not reach its handler.
///
/// Decoding never panics; the handler does not run.
///
/// ```
/// use noctis_core::{MenuKind, MenuTargetError};
///
/// let err = MenuTargetError::WrongKind { expected: MenuKind::User, found: MenuKind::Message };
/// assert_eq!(err.to_string(), "expected a USER command, got a MESSAGE command");
/// ```
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum MenuTargetError {
    /// The interaction JSON is not well formed.
    #[error("malformed context-menu interaction: {reason}")]
    Malformed {
        /// What is wrong.
        reason: String,
    },
    /// Not an `APPLICATION_COMMAND` interaction of type `USER` or `MESSAGE`.
    #[error(
        "not a context-menu command: interaction type {interaction_type}, command type {command_type}"
    )]
    NotContextMenu {
        /// The interaction `type`.
        interaction_type: u64,
        /// The command `data.type` (1 when absent).
        command_type: u64,
    },
    /// A `USER` command sent to a `MESSAGE` handler, or the reverse.
    #[error("expected a {} command, got a {} command", expected.name(), found.name())]
    WrongKind {
        /// The handler's kind.
        expected: MenuKind,
        /// The interaction's kind.
        found: MenuKind,
    },
    /// The target is missing from the interaction's resolved data.
    #[error("target {id} is not in the resolved data")]
    Unresolved {
        /// The target id.
        id: u64,
    },
}

impl MenuTargetError {
    fn malformed(reason: impl Into<String>) -> Self {
        Self::Malformed {
            reason: reason.into(),
        }
    }
}

/// The resolved target an erased menu call receives.
#[expect(
    clippy::large_enum_variant,
    reason = "built once per call and moved straight into the handler"
)]
pub(crate) enum Target {
    User(ResolvedUser),
    Message(ResolvedMessage),
}

/// The erased handler call: the handler builds its own responder (of its
/// defer mode) over the shared state.
type ErasedMenu = dyn Fn(CommandCtx, Target, Arc<Shared>) -> HandlerFuture + Send + Sync;

/// One context-menu command handler, as `#[user_command]` or
/// `#[message_command]` builds it; add it to a [`Module`](crate::Module).
///
/// Opaque and cheap to clone. `Debug` prints the name, the menu name, the
/// kind and the metadata. Annotating `async fn show` with
/// `#[user_command("Show profile")]` gives a `fn show()` that returns its
/// `ContextMenuHandler`. The handler receives a [`CommandCtx`] whose
/// [`command_path`](CommandCtx::command_path) is the menu name.
#[derive(Clone)]
pub struct ContextMenuHandler {
    name: &'static str,
    menu_name: &'static str,
    kind: MenuKind,
    metadata: CommandMetadata,
    ephemerality: Ephemerality,
    auto_defer: bool,
    call: Arc<ErasedMenu>,
    /// The Extension types its `Ext<T>` parameters require.
    requires: &'static [crate::kind::__ExtType],
}

impl ContextMenuHandler {
    /// A `USER` command handler; what `#[user_command]` expands to.
    #[doc(hidden)]
    pub fn __new_user<D, F, Fut, R>(
        name: &'static str,
        menu_name: &'static str,
        metadata: CommandMetadata,
        f: F,
    ) -> Self
    where
        D: DeferMode,
        F: Fn(CommandCtx, ResolvedUser, CommandResponder<D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        let call = move |ctx: CommandCtx, target: Target, shared: Arc<Shared>| -> HandlerFuture {
            match target {
                Target::User(user) => boxed::<_, _>(f(ctx, user, Responder::new(shared))),
                Target::Message(_) => wrong_kind(MenuKind::User, MenuKind::Message),
            }
        };
        Self {
            name,
            menu_name,
            kind: MenuKind::User,
            metadata,
            ephemerality: Ephemerality::Public,
            auto_defer: D::AUTO_DEFER,
            call: Arc::new(call),
            requires: &[],
        }
    }

    /// A `MESSAGE` command handler; what `#[message_command]` expands to.
    #[doc(hidden)]
    pub fn __new_message<D, F, Fut, R>(
        name: &'static str,
        menu_name: &'static str,
        metadata: CommandMetadata,
        f: F,
    ) -> Self
    where
        D: DeferMode,
        F: Fn(CommandCtx, ResolvedMessage, CommandResponder<D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        let call = move |ctx: CommandCtx, target: Target, shared: Arc<Shared>| -> HandlerFuture {
            match target {
                Target::Message(message) => boxed::<_, _>(f(ctx, message, Responder::new(shared))),
                Target::User(_) => wrong_kind(MenuKind::Message, MenuKind::User),
            }
        };
        Self {
            name,
            menu_name,
            kind: MenuKind::Message,
            metadata,
            ephemerality: Ephemerality::Public,
            auto_defer: D::AUTO_DEFER,
            call: Arc::new(call),
            requires: &[],
        }
    }

    /// The handler's path (`module_path!()::name`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The menu name as written in the attribute (CTX-01).
    pub fn menu_name(&self) -> &'static str {
        self.menu_name
    }

    /// `User` or `Message`, from the macro used (CTX-03).
    pub fn kind(&self) -> MenuKind {
        self.kind
    }

    /// The declared metadata; [`CommandMetadata::new`] when none was given.
    pub fn metadata(&self) -> &CommandMetadata {
        &self.metadata
    }

    /// The declared ephemerality: [`Ephemerality::Ephemeral`] with
    /// `response = ephemeral` in the attribute, else
    /// [`Ephemerality::Public`] (INT-10).
    pub fn ephemerality(&self) -> Ephemerality {
        self.ephemerality
    }

    /// Whether the pipeline auto-defers the command (INT-01): `false` when
    /// the attribute declares `auto_defer = false`, whose handler takes
    /// `CommandResponder<Manual>` (INT-05).
    pub fn auto_defer(&self) -> bool {
        self.auto_defer
    }

    /// Replaces the ephemerality; what the macros append for
    /// `response = ephemeral`.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_ephemerality(self, ephemerality: Ephemerality) -> Self {
        Self {
            ephemerality,
            ..self
        }
    }

    /// Sets the Extension types the handler requires; what
    /// `__RequireExtensions` calls.
    pub(crate) fn __with_requires(self, requires: &'static [crate::kind::__ExtType]) -> Self {
        Self { requires, ..self }
    }

    /// The Extension types its `Ext<T>` parameters require, in parameter
    /// order.
    pub(crate) fn requires(&self) -> &'static [crate::kind::__ExtType] {
        self.requires
    }

    /// Starts the handler with its decoded `target`.
    pub(crate) fn call(
        &self,
        ctx: CommandCtx,
        target: Target,
        shared: Arc<Shared>,
    ) -> HandlerFuture {
        (self.call)(ctx, target, shared)
    }
}

impl fmt::Debug for ContextMenuHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextMenuHandler")
            .field("name", &self.name)
            .field("menu_name", &self.menu_name)
            .field("kind", &self.kind)
            .field("ephemerality", &self.ephemerality)
            .field("auto_defer", &self.auto_defer)
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

fn wrong_kind(expected: MenuKind, found: MenuKind) -> HandlerFuture {
    failed(HandlerError::new(MenuTargetError::WrongKind {
        expected,
        found,
    }))
}

/// The decoded parts of a context-menu interaction.
pub(crate) struct MenuInput {
    pub(crate) kind: MenuKind,
    pub(crate) name: String,
    target: Id<GenericMarker>,
    resolved: InteractionDataResolved,
}

impl MenuInput {
    /// Decodes an interaction object (the gateway `d` or the HTTP body).
    pub(crate) fn from_json(raw: &[u8]) -> Result<Self, MenuTargetError> {
        let value: Value = serde_json::from_slice(raw)
            .map_err(|e| MenuTargetError::malformed(format!("invalid JSON: {e}")))?;
        let Value::Object(mut root) = value else {
            return Err(MenuTargetError::malformed(
                "the interaction is not an object",
            ));
        };
        let interaction_type = root
            .get("type")
            .and_then(Value::as_u64)
            .ok_or_else(|| MenuTargetError::malformed("missing interaction `type`"))?;
        let data = match root.remove("data") {
            Some(Value::Object(data)) => Some(data),
            Some(_) => return Err(MenuTargetError::malformed("`data` is not an object")),
            None => None,
        };
        let command_type = match data.as_ref().and_then(|d| d.get("type")) {
            None | Some(Value::Null) => 1,
            Some(t) => t
                .as_u64()
                .ok_or_else(|| MenuTargetError::malformed("`data.type` is not an integer"))?,
        };
        let not_menu = MenuTargetError::NotContextMenu {
            interaction_type,
            command_type,
        };
        if interaction_type != 2 {
            return Err(not_menu);
        }
        let mut data = data.ok_or_else(|| MenuTargetError::malformed("missing `data`"))?;
        let kind = match command_type {
            2 => MenuKind::User,
            3 => MenuKind::Message,
            _ => return Err(not_menu),
        };
        let name = data
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| MenuTargetError::malformed("missing `data.name`"))?
            .to_owned();
        let target = match data.remove("target_id") {
            None | Some(Value::Null) => {
                return Err(MenuTargetError::malformed("missing `data.target_id`"));
            }
            Some(id) => serde_json::from_value::<Id<GenericMarker>>(id)
                .map_err(|e| MenuTargetError::malformed(format!("bad `data.target_id`: {e}")))?,
        };
        let resolved = match data.remove("resolved") {
            None | Some(Value::Null) => empty_resolved(),
            Some(r) => serde_json::from_value(r)
                .map_err(|e| MenuTargetError::malformed(format!("bad `resolved` data: {e}")))?,
        };
        Ok(Self {
            kind,
            name,
            target,
            resolved,
        })
    }

    /// The typed target, or [`MenuTargetError::Unresolved`].
    pub(crate) fn target(&self) -> Result<Target, MenuTargetError> {
        let unresolved = MenuTargetError::Unresolved {
            id: self.target.get(),
        };
        match self.kind {
            MenuKind::User => ResolvedUser::from_resolved(&self.resolved, self.target.cast())
                .map(Target::User)
                .ok_or(unresolved),
            MenuKind::Message => ResolvedMessage::from_resolved(&self.resolved, self.target.cast())
                .map(Target::Message)
                .ok_or(unresolved),
        }
    }
}

/// Decodes `json` and runs `handler`: the decode errors, then the kind,
/// then the exact menu name ([`UnknownCommandPath`](crate::UnknownCommandPath)),
/// then the target; the test seam for context menus (see `crate::__testing`).
pub(crate) fn call_context_menu(
    handler: &ContextMenuHandler,
    json: &[u8],
) -> BoxFuture<'static, HandlerResult<()>> {
    let input = match MenuInput::from_json(json) {
        Ok(input) => input,
        Err(error) => return failed(HandlerError::new(error)),
    };
    if input.kind != handler.kind {
        return failed(HandlerError::new(MenuTargetError::WrongKind {
            expected: handler.kind,
            found: input.kind,
        }));
    }
    if input.name != handler.menu_name {
        return unknown(&input.name, handler.name);
    }
    match input.target() {
        Ok(target) => {
            let path = handler.menu_name;
            let ctx = CommandCtx::detached(path, CommandInput::empty(path), handler.ephemerality);
            run_detached(|r| handler.call(ctx, target, r), handler.ephemerality)
        }
        Err(error) => failed(HandlerError::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use noctis_ui::Components;
    use serde_json::json;

    use super::*;
    use crate::UnknownCommandPath;

    static USERS: Mutex<Vec<(String, u64, bool)>> = Mutex::new(Vec::new());
    static MESSAGES: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

    async fn show(
        ctx: CommandCtx,
        target: ResolvedUser,
        _: CommandResponder,
    ) -> HandlerResult<Components> {
        USERS.lock().map_err(|_| fmt::Error)?.push((
            ctx.command_path().to_owned(),
            target.user.id.get(),
            target.member.is_some(),
        ));
        Ok(crate::command::tests::msg())
    }

    async fn quote(
        ctx: CommandCtx,
        target: ResolvedMessage,
        r: CommandResponder,
    ) -> HandlerResult<crate::Responded> {
        MESSAGES
            .lock()
            .map_err(|_| fmt::Error)?
            .push((ctx.command_path().to_owned(), target.message.id.get()));
        Ok(r.reply(crate::command::tests::msg()).await?.done())
    }

    async fn fails(
        _: CommandCtx,
        _: ResolvedUser,
        _: CommandResponder,
    ) -> HandlerResult<Components> {
        Err(HandlerError::new(fmt::Error))
    }

    fn user_handler(menu: &'static str) -> ContextMenuHandler {
        ContextMenuHandler::__new_user("m::show", menu, CommandMetadata::new(), show)
    }

    fn message_handler() -> ContextMenuHandler {
        ContextMenuHandler::__new_message("m::quote", "Quote", CommandMetadata::new(), quote)
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

    fn menu_error(result: HandlerResult<()>) -> MenuTargetError {
        let Err(err) = result else {
            panic!("expected an error");
        };
        err.as_error()
            .downcast_ref::<MenuTargetError>()
            .unwrap_or_else(|| panic!("not a MenuTargetError: {err}"))
            .clone()
    }

    fn seen_user(id: u64) -> Option<(String, u64, bool)> {
        USERS
            .lock()
            .ok()
            .and_then(|u| u.iter().find(|(_, i, _)| *i == id).cloned())
    }

    /// Matrix 45 and 46: a user target with and without its member.
    #[tokio::test]
    async fn ctx_03_user_target() {
        let h = user_handler("Show profile");
        let resolved = json!({ "users": { "11": user("11") }, "members": { "11": member() } });
        let ok = call_context_menu(
            &h,
            &interaction(2, "Show profile", &json!("11"), Some(resolved)),
        )
        .await;
        assert!(ok.is_ok(), "{ok:?}");
        assert_eq!(seen_user(11), Some(("Show profile".to_owned(), 11, true)));

        let resolved = json!({ "users": { "12": user("12") } });
        let ok = call_context_menu(
            &h,
            &interaction(2, "Show profile", &json!("12"), Some(resolved)),
        )
        .await;
        assert!(ok.is_ok(), "{ok:?}");
        assert_eq!(seen_user(12), Some(("Show profile".to_owned(), 12, false)));
    }

    /// Matrix 47: a message target.
    #[tokio::test]
    async fn ctx_03_message_target() {
        let h = message_handler();
        let resolved = json!({ "messages": { "44": message("44") } });
        let ok =
            call_context_menu(&h, &interaction(3, "Quote", &json!("44"), Some(resolved))).await;
        assert!(ok.is_ok(), "{ok:?}");
        assert!(
            MESSAGES
                .lock()
                .is_ok_and(|m| m.contains(&("Quote".to_owned(), 44)))
        );
    }

    /// Matrix 48: a target missing from the resolved data.
    #[tokio::test]
    async fn target_missing() {
        let h = user_handler("Show profile");
        let resolved = json!({ "users": { "11": user("11") } });
        for json in [
            interaction(2, "Show profile", &json!("99"), Some(resolved)),
            interaction(2, "Show profile", &json!("99"), None),
        ] {
            let err = menu_error(call_context_menu(&h, &json).await);
            assert_eq!(err, MenuTargetError::Unresolved { id: 99 });
            assert_eq!(err.to_string(), "target 99 is not in the resolved data");
        }
        let err = menu_error(
            call_context_menu(
                &message_handler(),
                &interaction(3, "Quote", &json!("99"), None),
            )
            .await,
        );
        assert_eq!(err, MenuTargetError::Unresolved { id: 99 });
    }

    /// Matrix 49: the wrong kind, both ways.
    #[tokio::test]
    async fn ctx_03_wrong_kind() {
        let err = menu_error(
            call_context_menu(
                &user_handler("Quote"),
                &interaction(3, "Quote", &json!("1"), None),
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
            call_context_menu(
                &message_handler(),
                &interaction(2, "Quote", &json!("1"), None),
            )
            .await,
        );
        assert_eq!(
            err.to_string(),
            "expected a MESSAGE command, got a USER command"
        );
    }

    /// Matrix 50: not a context-menu interaction.
    #[tokio::test]
    async fn ctx_03_not_a_context_menu() {
        let h = user_handler("X");
        for (json, interaction_type, command_type) in [
            (
                json!({ "type": 2, "data": { "type": 1, "name": "X" } }),
                2,
                1,
            ),
            (json!({ "type": 2, "data": { "name": "X" } }), 2, 1),
            (
                json!({ "type": 2, "data": { "type": 4, "name": "X" } }),
                2,
                4,
            ),
            (
                json!({ "type": 3, "data": { "type": 2, "name": "X" } }),
                3,
                2,
            ),
            (json!({ "type": 3 }), 3, 1),
        ] {
            let err = menu_error(call_context_menu(&h, json.to_string().as_bytes()).await);
            assert_eq!(
                err,
                MenuTargetError::NotContextMenu {
                    interaction_type,
                    command_type
                },
                "{json}"
            );
        }
        let err = MenuTargetError::NotContextMenu {
            interaction_type: 2,
            command_type: 1,
        };
        assert_eq!(
            err.to_string(),
            "not a context-menu command: interaction type 2, command type 1"
        );
    }

    /// Matrix 51 and 52: exact name match, Unicode names.
    #[tokio::test]
    async fn name_mismatch_and_unicode() {
        let h = user_handler("Show profile");
        let Err(err) =
            call_context_menu(&h, &interaction(2, "Show Profile", &json!("1"), None)).await
        else {
            panic!("expected an error");
        };
        let unknown = err
            .as_error()
            .downcast_ref::<UnknownCommandPath>()
            .unwrap_or_else(|| panic!("{err}"));
        assert_eq!(
            unknown.to_string(),
            "`Show Profile` is not a path of the Command `m::show`"
        );

        let name = "Profili göster 🚀";
        let h = user_handler(name);
        let resolved = json!({ "users": { "13": user("13") } });
        let ok = call_context_menu(&h, &interaction(2, name, &json!("13"), Some(resolved))).await;
        assert!(ok.is_ok(), "{ok:?}");
        assert_eq!(seen_user(13), Some((name.to_owned(), 13, false)));
    }

    /// Matrix 53: malformed interactions.
    #[tokio::test]
    async fn malformed() {
        let h = user_handler("X");
        let cases: [(Vec<u8>, &str); 13] = [
            (b"{".to_vec(), "invalid JSON: "),
            (b"[]".to_vec(), "the interaction is not an object"),
            (br#"{"data":{}}"#.to_vec(), "missing interaction `type`"),
            (
                br#"{"type":2,"data":1}"#.to_vec(),
                "`data` is not an object",
            ),
            (br#"{"type":2}"#.to_vec(), "missing `data`"),
            (
                br#"{"type":2,"data":{"type":"2"}}"#.to_vec(),
                "`data.type` is not an integer",
            ),
            (
                br#"{"type":2,"data":{"type":2,"target_id":"1"}}"#.to_vec(),
                "missing `data.name`",
            ),
            (
                br#"{"type":2,"data":{"type":2,"name":"X"}}"#.to_vec(),
                "missing `data.target_id`",
            ),
            (
                interaction(2, "X", &json!("0"), None),
                "bad `data.target_id`: ",
            ),
            (
                interaction(2, "X", &json!("abc"), None),
                "bad `data.target_id`: ",
            ),
            (
                interaction(2, "X", &json!(true), None),
                "bad `data.target_id`: ",
            ),
            (
                interaction(2, "X", &json!("1"), Some(json!("x"))),
                "bad `resolved` data: ",
            ),
            (
                interaction(2, "X", &json!("1"), Some(json!({ "users": 1 }))),
                "bad `resolved` data: ",
            ),
        ];
        for (json, reason) in cases {
            let err = menu_error(call_context_menu(&h, &json).await);
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
        let h = user_handler("Show profile");
        let resolved = json!({ "users": { "123": user("123") } });
        let ok = call_context_menu(
            &h,
            &interaction(2, "Show profile", &json!(123), Some(resolved)),
        )
        .await;
        assert!(ok.is_ok(), "{ok:?}");
    }

    /// Matrix 55: a handler error passes through.
    #[tokio::test]
    async fn handler_error_passes_through() {
        let h = ContextMenuHandler::__new_user("m::f", "F", CommandMetadata::new(), fails);
        let resolved = json!({ "users": { "1": user("1") } });
        let Err(err) =
            call_context_menu(&h, &interaction(2, "F", &json!("1"), Some(resolved))).await
        else {
            panic!("expected an error");
        };
        assert!(err.as_error().is::<fmt::Error>());
    }

    #[test]
    fn accessors_and_debug() {
        let h = message_handler();
        assert_eq!(h.name(), "m::quote");
        assert_eq!(h.menu_name(), "Quote");
        assert_eq!(h.kind(), MenuKind::Message);
        assert_eq!(h.metadata(), &CommandMetadata::new());
        let debug = format!("{h:?}");
        assert!(debug.starts_with("ContextMenuHandler { name: \"m::quote\", menu_name: \"Quote\", kind: Message, ephemerality: Public, auto_defer: true, metadata: CommandMetadata {"), "{debug}");
        assert_eq!(h.ephemerality(), Ephemerality::Public);
        assert!(h.auto_defer());
        let h = h.__with_ephemerality(Ephemerality::Ephemeral);
        assert_eq!(h.ephemerality(), Ephemerality::Ephemeral);
        let module = crate::Module::new("m")
            .__add(h)
            .__add(user_handler("Show profile"));
        assert_eq!(module.menus().len(), 2);
        assert!(format!("{module:?}").contains("menus: [\"Quote\", \"Show profile\"]"));
    }
}
