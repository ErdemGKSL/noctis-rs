//! [`DiscordCache`]: the typed Discord resource cache.

use twilight_model::channel::{Channel, Message};
use twilight_model::gateway::event::Event;
use twilight_model::guild::{Guild, Member, Role};
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use twilight_model::user::User;

use crate::{BoxFuture, PortError};

/// The Discord resource cache: typed, not bytes.
///
/// Every read returns an owned `twilight_model` value; no reference, guard or
/// lifetime-bound value crosses the port. `None` means not cached. An
/// out-of-process implementation serializes the values itself.
pub trait DiscordCache: Send + Sync + 'static {
    /// Applies a gateway event.
    fn update<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), PortError>>;

    /// The cached guild.
    fn guild(&self, guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>>;

    /// The cached channel.
    fn channel(
        &self,
        channel: Id<ChannelMarker>,
    ) -> BoxFuture<'_, Result<Option<Channel>, PortError>>;

    /// The cached role.
    fn role(&self, role: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>>;

    /// The cached member of `guild`.
    fn member(
        &self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
    ) -> BoxFuture<'_, Result<Option<Member>, PortError>>;

    /// The cached user.
    fn user(&self, user: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>>;

    /// The cached message.
    fn message(
        &self,
        message: Id<MessageMarker>,
    ) -> BoxFuture<'_, Result<Option<Message>, PortError>>;

    /// The ids of the guild's cached channels; `None` if the guild is not
    /// cached.
    fn channel_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>>;

    /// The ids of the guild's cached roles; `None` if the guild is not cached.
    fn role_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>>;

    /// The user ids of the guild's cached members; `None` if the guild is not
    /// cached.
    fn member_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>>;
}
