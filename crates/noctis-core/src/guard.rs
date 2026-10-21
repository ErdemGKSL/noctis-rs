//! Route Guards (AD-23 stage 8, FR-19): the checks a component or modal
//! Route declares on its kind attribute (`owner = field`,
//! `permissions = A | B`, `guard = path::to::fn`), run after the
//! before-Middleware and before the handler.
//!
//! The derive emits a hidden [`__Guards`] impl for every component and
//! modal struct; the pipeline starters wrap the handler future with it when
//! the Route declares a Guard. A denial sends its localized ephemeral
//! notice (`noctis.guard.owner`, `noctis.guard.permissions`, or the key a
//! custom Guard chose, `noctis.guard.failed` by default) and the handler
//! never runs.

use std::error::Error;
use std::fmt;
use std::future::Future;

use noctis_i18n::keys;
use noctis_rules::MessageKey;
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use crate::handler::{HandlerError, HandlerResult};

/// What a custom Guard (`guard = path::to::fn`) decides.
///
/// A custom Guard is an `async fn` taking the Route's context and the
/// decoded struct, returning `HandlerResult<Access>`:
///
/// ```
/// use noctis_core::{Access, ComponentCtx, HandlerResult};
/// use noctis_rules::MessageKey;
///
/// struct Ban;
///
/// async fn is_staff(ctx: &ComponentCtx, _this: &Ban) -> HandlerResult<Access> {
///     Ok(if ctx.guild_id().is_some() {
///         Access::Allow
///     } else {
///         Access::Deny(MessageKey::new("mod.staff_only"))
///     })
/// }
///
/// assert_eq!(Access::deny(), Access::Deny(noctis_i18n::keys::GUARD_FAILED));
/// assert!(Access::Allow.is_allowed());
/// assert!(!Access::deny().is_allowed());
/// ```
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// The interaction may reach the handler.
    Allow,
    /// The interaction is refused with this key's localized ephemeral
    /// notice; the handler does not run.
    Deny(MessageKey),
}

impl Access {
    /// A denial with the generic `noctis.guard.failed` notice.
    pub const fn deny() -> Self {
        Self::Deny(keys::GUARD_FAILED)
    }

    /// `true` only for [`Allow`](Self::Allow).
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Which Guard refused an interaction, and the notice it chose.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct __Denial {
    /// `"owner"`, `"permissions"` or the custom Guard's path as written.
    pub guard: &'static str,
    /// The notice key.
    pub key: MessageKey,
}

/// The Guards of a component (`C = ComponentCtx`) or modal
/// (`C = ModalSubmitCtx`) struct; every kind derive implements it, an empty
/// impl when the Route declares no Guard.
#[doc(hidden)]
pub trait __Guards<C> {
    /// Whether the Route declares a Guard; the starters wrap the handler
    /// future only then.
    const GUARDED: bool = false;

    /// Runs the Guards in order (owner, permissions, custom): the first
    /// denial, or `None` when every Guard allowed.
    fn __guard<'a>(
        &'a self,
        ctx: &'a C,
    ) -> impl Future<Output = HandlerResult<Option<__Denial>>> + Send + 'a {
        let _ = (self, ctx);
        async { Ok(None) }
    }
}

/// The `owner` Guard: denies unless `user` is `owner`.
#[doc(hidden)]
#[must_use]
pub fn __check_owner(user: Id<UserMarker>, owner: Id<UserMarker>) -> Option<__Denial> {
    (user != owner).then_some(__Denial {
        guard: "owner",
        key: keys::GUARD_OWNER,
    })
}

/// The `permissions` Guard: allows when `have` contains `need` or
/// `ADMINISTRATOR`; denies without a member (`None`: a DM).
#[doc(hidden)]
#[must_use]
pub fn __check_permissions(have: Option<Permissions>, need: Permissions) -> Option<__Denial> {
    match have {
        Some(have) if have.contains(need) || have.contains(Permissions::ADMINISTRATOR) => None,
        _ => Some(__Denial {
            guard: "permissions",
            key: keys::GUARD_PERMISSIONS,
        }),
    }
}

/// The custom Guard `path`'s result as a denial; its error stays the
/// error.
#[doc(hidden)]
pub fn __check_custom(
    path: &'static str,
    result: HandlerResult<Access>,
) -> HandlerResult<Option<__Denial>> {
    result.map(|access| match access {
        Access::Allow => None,
        Access::Deny(key) => Some(__Denial { guard: path, key }),
    })
}

/// A field `owner = field` may name: only `Id<UserMarker>`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`owner` needs a field of type `Id<UserMarker>`, found `{Self}`",
    label = "not an `Id<UserMarker>`"
)]
pub trait __OwnerField {
    /// The user id.
    fn __owner(&self) -> Id<UserMarker>;
}

impl __OwnerField for Id<UserMarker> {
    fn __owner(&self) -> Id<UserMarker> {
        *self
    }
}

/// The user id of the `owner` field `field`.
#[doc(hidden)]
#[must_use]
pub fn __owner_id<T: __OwnerField>(field: &T) -> Id<UserMarker> {
    field.__owner()
}

/// The sentinel error a guarded starter returns on a denial; the pipeline
/// turns it into `Outcome::Denied` (no histogram, no sink report).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuardDenied {
    pub(crate) route: &'static str,
    pub(crate) guard: &'static str,
    pub(crate) key: MessageKey,
}

impl fmt::Display for GuardDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Guard `{}` denied Route `{}` (notice `{}`)",
            self.guard, self.route, self.key
        )
    }
}

impl Error for GuardDenied {}

impl GuardDenied {
    /// The user-condition error of `denial` on `route`.
    pub(crate) fn error(route: &'static str, denial: __Denial) -> HandlerError {
        HandlerError::user_condition(
            Self {
                route,
                guard: denial.guard,
                key: denial.key,
            },
            denial.key,
        )
    }

    /// The denial `error` carries, if it is one.
    pub(crate) fn of(error: &HandlerError) -> Option<Self> {
        error.as_error().downcast_ref::<Self>().copied()
    }
}

#[cfg(test)]
mod tests;
