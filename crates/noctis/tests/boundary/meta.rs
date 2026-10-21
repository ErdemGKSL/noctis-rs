//! Metadata boundary passes (META-01, META-03): every permission name, a
//! guild-scoped command without `contexts`, and the largest guild id.

use noctis::Permissions;
use noctis::prelude::*;
use noctis::twilight::model::id::Id;

#[derive(SlashCommand)]
#[command(
    "every",
    description = "d",
    permissions = CREATE_INVITE
        | KICK_MEMBERS
        | BAN_MEMBERS
        | ADMINISTRATOR
        | MANAGE_CHANNELS
        | MANAGE_GUILD
        | ADD_REACTIONS
        | VIEW_AUDIT_LOG
        | PRIORITY_SPEAKER
        | STREAM
        | VIEW_CHANNEL
        | SEND_MESSAGES
        | SEND_TTS_MESSAGES
        | MANAGE_MESSAGES
        | EMBED_LINKS
        | ATTACH_FILES
        | READ_MESSAGE_HISTORY
        | MENTION_EVERYONE
        | USE_EXTERNAL_EMOJIS
        | VIEW_GUILD_INSIGHTS
        | CONNECT
        | SPEAK
        | MUTE_MEMBERS
        | DEAFEN_MEMBERS
        | MOVE_MEMBERS
        | USE_VAD
        | CHANGE_NICKNAME
        | MANAGE_NICKNAMES
        | MANAGE_ROLES
        | MANAGE_WEBHOOKS
        | MANAGE_GUILD_EXPRESSIONS
        | USE_SLASH_COMMANDS
        | REQUEST_TO_SPEAK
        | MANAGE_EVENTS
        | MANAGE_THREADS
        | CREATE_PUBLIC_THREADS
        | CREATE_PRIVATE_THREADS
        | USE_EXTERNAL_STICKERS
        | SEND_MESSAGES_IN_THREADS
        | USE_EMBEDDED_ACTIVITIES
        | MODERATE_MEMBERS
        | VIEW_CREATOR_MONETIZATION_ANALYTICS
        | USE_SOUNDBOARD
        | CREATE_GUILD_EXPRESSIONS
        | CREATE_EVENTS
        | USE_EXTERNAL_SOUNDS
        | SEND_VOICE_MESSAGES
        | SEND_POLLS
        | USE_EXTERNAL_APPS
        | PIN_MESSAGES
        | BYPASS_SLOWMODE
)]
struct Meta01EveryPermission;

impl Meta01EveryPermission {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command("scoped", description = "d", guilds = [42], permissions = MANAGE_GUILD, nsfw = true)]
struct Meta03GuildWithoutContexts;

impl Meta03GuildWithoutContexts {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command("max", description = "d", guilds = [18446744073709551615])]
struct Meta03U64MaxGuild;

impl Meta03U64MaxGuild {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[test]
fn meta_01_every_permission_is_all() {
    assert_eq!(
        Meta01EveryPermission::descriptor()
            .metadata()
            .default_member_permissions,
        Some(Permissions::all())
    );
}

#[test]
fn meta_03_guild_scoped_metadata() {
    let handler = Meta03GuildWithoutContexts::descriptor();
    assert_eq!(handler.metadata().guilds, [Id::new(42)]);
    assert_eq!(handler.metadata().nsfw, Some(true));
    let handler = Meta03U64MaxGuild::descriptor();
    assert_eq!(handler.metadata().guilds, [Id::new(u64::MAX)]);
}
