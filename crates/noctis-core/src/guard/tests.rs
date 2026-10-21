//! Route Guard Matrix 19-22 (as pure checks) and 39.

use std::io;

use super::*;

const MANAGE_GUILD: Permissions = Permissions::MANAGE_GUILD;

/// Rows 18 to 22: the `permissions` check.
#[test]
fn permissions_check() {
    let denied = Some(__Denial {
        guard: "permissions",
        key: keys::GUARD_PERMISSIONS,
    });
    // Row 18: exactly the needed bits.
    assert_eq!(
        __check_permissions(Some(Permissions::from_bits_retain(32)), MANAGE_GUILD),
        None
    );
    // Row 19: a superset.
    assert_eq!(
        __check_permissions(Some(Permissions::from_bits_retain(48)), MANAGE_GUILD),
        None
    );
    // Row 20: ADMINISTRATOR satisfies every requirement.
    assert_eq!(
        __check_permissions(
            Some(Permissions::ADMINISTRATOR),
            MANAGE_GUILD | Permissions::BAN_MEMBERS
        ),
        None
    );
    // Row 21: none, or other bits.
    assert_eq!(
        __check_permissions(Some(Permissions::empty()), MANAGE_GUILD),
        denied
    );
    assert_eq!(
        __check_permissions(Some(Permissions::BAN_MEMBERS), MANAGE_GUILD),
        denied
    );
    // A partial match is a denial.
    assert_eq!(
        __check_permissions(Some(MANAGE_GUILD), MANAGE_GUILD | Permissions::BAN_MEMBERS),
        denied
    );
    // Row 22: a DM (no member).
    assert_eq!(__check_permissions(None, MANAGE_GUILD), denied);
}

#[test]
fn owner_check() {
    assert_eq!(__check_owner(Id::new(42), Id::new(42)), None);
    assert_eq!(
        __check_owner(Id::new(43), Id::new(42)),
        Some(__Denial {
            guard: "owner",
            key: keys::GUARD_OWNER,
        })
    );
    assert_eq!(__owner_id(&Id::<UserMarker>::new(7)), Id::new(7));
}

#[test]
fn custom_check() {
    let key = MessageKey::new("mod.staff_only");
    assert_eq!(
        __check_custom("auth::is_staff", Ok(Access::Allow)).ok(),
        Some(None)
    );
    assert_eq!(
        __check_custom("auth::is_staff", Ok(Access::Deny(key))).ok(),
        Some(Some(__Denial {
            guard: "auth::is_staff",
            key,
        }))
    );
    let error = __check_custom("g", Err(HandlerError::new(io::Error::other("db")))).unwrap_err();
    assert_eq!(error.to_string(), "db");
}

/// Row 39.
#[test]
fn access() {
    assert_eq!(Access::deny(), Access::Deny(keys::GUARD_FAILED));
    assert!(Access::Allow.is_allowed());
    assert!(!Access::deny().is_allowed());
    assert!(!Access::Deny(MessageKey::new("a.b")).is_allowed());
}

#[test]
fn denied_error_round_trips() {
    let error = GuardDenied::error(
        "dash.mod",
        __Denial {
            guard: "owner",
            key: keys::GUARD_OWNER,
        },
    );
    assert!(error.is_user());
    assert_eq!(error.user_message(), Some(keys::GUARD_OWNER));
    assert_eq!(
        GuardDenied::of(&error),
        Some(GuardDenied {
            route: "dash.mod",
            guard: "owner",
            key: keys::GUARD_OWNER,
        })
    );
    assert_eq!(
        GuardDenied::of(&HandlerError::user(keys::GUARD_OWNER)),
        None
    );
}

/// The default `__Guards` impl allows and is unguarded.
#[test]
fn default_guards_allow() {
    struct Plain;
    impl __Guards<()> for Plain {}
    const { assert!(!<Plain as __Guards<()>>::GUARDED) };
    let result = futures_util::FutureExt::now_or_never(Plain.__guard(&()));
    assert!(matches!(result, Some(Ok(None))));
}
