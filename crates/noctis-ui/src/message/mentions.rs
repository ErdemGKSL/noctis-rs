//! Who a message may ping: [`Mentions`], [`Users`] and [`Roles`] (MSG-03,
//! MSG-05).

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use twilight_model::id::Id;
use twilight_model::id::marker::{RoleMarker, UserMarker};

/// Which user mentions ping.
///
/// `All` and `Only` are separate variants, so `parse: ["users"]` together
/// with explicit ids cannot be built (MSG-03). `Only(vec![])` pings nobody,
/// like `None`.
///
/// ```
/// use noctis_ui::Users;
/// use twilight_model::id::Id;
///
/// assert_eq!(Users::only([Id::new(1), Id::new(2)]), Users::Only(vec![Id::new(1), Id::new(2)]));
/// assert_eq!(Users::default(), Users::None);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Users {
    /// No user mention pings.
    #[default]
    None,
    /// Every user mention pings (`parse: ["users"]`).
    All,
    /// Only these users are pinged (`users: [..]`), in order.
    Only(Vec<Id<UserMarker>>),
}

impl Users {
    /// [`Users::Only`] the given ids.
    pub fn only(ids: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        Self::Only(ids.into_iter().collect())
    }
}

/// Which role mentions ping; see [`Users`].
///
/// ```
/// use noctis_ui::Roles;
/// use twilight_model::id::Id;
///
/// assert_eq!(Roles::only([Id::new(5)]), Roles::Only(vec![Id::new(5)]));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Roles {
    /// No role mention pings.
    #[default]
    None,
    /// Every role mention pings (`parse: ["roles"]`).
    All,
    /// Only these roles are pinged (`roles: [..]`), in order.
    Only(Vec<Id<RoleMarker>>),
}

impl Roles {
    /// [`Roles::Only`] the given ids.
    pub fn only(ids: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        Self::Only(ids.into_iter().collect())
    }
}

/// A message's `allowed_mentions`: who its mentions may ping.
///
/// The default pings nobody (`{"parse": []}`), even though mentions in the
/// text still render (MSG-05); every body sends it. Opt in per type:
///
/// ```
/// use noctis_ui::{Mentions, Roles, Users};
/// use twilight_model::id::Id;
///
/// assert_eq!(serde_json::to_value(Mentions::default()).unwrap(), serde_json::json!({"parse": []}));
/// let m = Mentions::none().users(Users::All).roles(Roles::only([Id::new(5)])).everyone(true);
/// assert_eq!(
///     serde_json::to_value(&m).unwrap(),
///     serde_json::json!({"parse": ["users", "everyone"], "roles": ["5"]})
/// );
/// ```
///
/// `Users::All` takes no ids; explicit ids are `Users::Only` (MSG-03):
///
/// ```compile_fail,E0618
/// use noctis_ui::Users;
///
/// let _ = Users::All(Vec::new()); // [MSG-03]
/// ```
///
/// A message always has mentions; there is no "unset" that would fall back
/// to Discord's default of pinging users (MSG-05):
///
/// ```compile_fail,E0308
/// use noctis_ui::{Components, V2Message};
///
/// let _ = V2Message::new(Components::new()).mentions(None); // [MSG-05]
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mentions {
    users: Users,
    roles: Roles,
    everyone: bool,
    replied_user: bool,
}

impl Mentions {
    /// Pings nobody; the same as `Mentions::default()`.
    pub const fn none() -> Self {
        Self {
            users: Users::None,
            roles: Roles::None,
            everyone: false,
            replied_user: false,
        }
    }

    /// Which user mentions ping.
    #[must_use]
    pub fn users(mut self, users: Users) -> Self {
        self.users = users;
        self
    }

    /// Which role mentions ping.
    #[must_use]
    pub fn roles(mut self, roles: Roles) -> Self {
        self.roles = roles;
        self
    }

    /// Whether `@everyone` and `@here` ping (`parse: ["everyone"]`).
    #[must_use]
    pub fn everyone(mut self, everyone: bool) -> Self {
        self.everyone = everyone;
        self
    }

    /// Whether a reply pings the author of the replied-to message.
    #[must_use]
    pub fn replied_user(mut self, replied_user: bool) -> Self {
        self.replied_user = replied_user;
        self
    }

    /// The user setting.
    pub fn user_mentions(&self) -> &Users {
        &self.users
    }

    /// The role setting.
    pub fn role_mentions(&self) -> &Roles {
        &self.roles
    }

    /// Whether `@everyone` and `@here` ping.
    pub fn pings_everyone(&self) -> bool {
        self.everyone
    }

    /// Whether a reply pings the replied-to author.
    pub fn pings_replied_user(&self) -> bool {
        self.replied_user
    }
}

/// The `parse` array: `users`, `roles`, `everyone`, in that order.
struct Parse<'a>(&'a Mentions);

impl Serialize for Parse<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let m = self.0;
        let mut seq = s.serialize_seq(None)?;
        if m.users == Users::All {
            seq.serialize_element("users")?;
        }
        if m.roles == Roles::All {
            seq.serialize_element("roles")?;
        }
        if m.everyone {
            seq.serialize_element("everyone")?;
        }
        seq.end()
    }
}

/// `{"parse": [..]}`, then `users` and `roles` when they list ids, then
/// `replied_user` when `true`.
impl Serialize for Mentions {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("parse", &Parse(self))?;
        if let Users::Only(ids) = &self.users
            && !ids.is_empty()
        {
            map.serialize_entry("users", ids)?;
        }
        if let Roles::Only(ids) = &self.roles
            && !ids.is_empty()
        {
            map.serialize_entry("roles", ids)?;
        }
        crate::wire::flag(&mut map, "replied_user", self.replied_user)?;
        map.end()
    }
}
