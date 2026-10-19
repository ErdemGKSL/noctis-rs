//! The trivial defaults of `LocaleSource` and `GuildAccessList`.

use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::{BoxFuture, GuildAccessList, LocaleSource, PortError};

/// The default [`LocaleSource`]: no runtime overlay for any locale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NoLocaleOverlay;

impl LocaleSource for NoLocaleOverlay {
    fn load<'a>(
        &'a self,
        _locale: &'a str,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}

/// The default [`GuildAccessList`]: the App serves every guild.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AllowAllGuilds;

impl GuildAccessList for AllowAllGuilds {
    fn allows(&self, _guild: Id<GuildMarker>) -> bool {
        true
    }
}
