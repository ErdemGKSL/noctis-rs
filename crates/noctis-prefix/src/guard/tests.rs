//! Story 19.2: the permission algorithm (permissions.mdx), the override
//! order and the final decision.

use twilight_model::application::command::permissions::{CommandPermission, CommandPermissionType};
use twilight_model::channel::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};

use super::{allowed, channel_permissions, overridden};

const GUILD: Id<GuildMarker> = Id::new(100);
const OWNER: Id<UserMarker> = Id::new(1);
const USER: Id<UserMarker> = Id::new(2);
const MOD: Id<RoleMarker> = Id::new(10);
const MUTED: Id<RoleMarker> = Id::new(11);
const ADMIN: Id<RoleMarker> = Id::new(12);
const CHANNEL: Id<ChannelMarker> = Id::new(50);
const PARENT: Id<ChannelMarker> = Id::new(51);

fn role(id: Id<RoleMarker>) -> Option<Permissions> {
    match id.get() {
        100 => Some(Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES),
        10 => Some(Permissions::BAN_MEMBERS | Permissions::KICK_MEMBERS),
        11 => Some(Permissions::empty()),
        12 => Some(Permissions::ADMINISTRATOR),
        _ => None,
    }
}

fn overwrite(
    id: u64,
    kind: PermissionOverwriteType,
    allow: Permissions,
    deny: Permissions,
) -> PermissionOverwrite {
    PermissionOverwrite {
        allow,
        deny,
        id: Id::new(id),
        kind,
    }
}

fn perms(roles: &[Id<RoleMarker>], overwrites: &[PermissionOverwrite]) -> Permissions {
    channel_permissions(GUILD, OWNER, USER, roles, role, overwrites)
}

/// `compute_base_permissions`: `@everyone` plus every role; the owner and
/// ADMINISTRATOR get everything, overwrites included.
#[test]
fn base_permissions() {
    assert_eq!(
        perms(&[], &[]),
        Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES
    );
    assert!(perms(&[MOD], &[]).contains(Permissions::BAN_MEMBERS | Permissions::SEND_MESSAGES));
    let deny_all = [overwrite(
        100,
        PermissionOverwriteType::Role,
        Permissions::empty(),
        Permissions::all(),
    )];
    assert_eq!(perms(&[ADMIN], &deny_all), Permissions::all());
    assert_eq!(
        channel_permissions(GUILD, USER, USER, &[], role, &deny_all),
        Permissions::all()
    );
    // An unknown role grants nothing.
    assert_eq!(perms(&[Id::new(99)], &[]), perms(&[], &[]));
}

/// `compute_overwrites`: `@everyone`, then every role's allow and deny
/// together (allow wins), then the member's own overwrite.
#[test]
fn overwrites_in_order() {
    let role_kind = PermissionOverwriteType::Role;
    let member_kind = PermissionOverwriteType::Member;
    let overwrites = [
        overwrite(
            100,
            role_kind,
            Permissions::empty(),
            Permissions::SEND_MESSAGES,
        ),
        overwrite(
            10,
            role_kind,
            Permissions::SEND_MESSAGES,
            Permissions::BAN_MEMBERS,
        ),
        overwrite(
            11,
            role_kind,
            Permissions::empty(),
            Permissions::SEND_MESSAGES,
        ),
    ];
    assert!(!perms(&[], &overwrites).contains(Permissions::SEND_MESSAGES));
    // MOD allows what MUTED denies: the role allow wins.
    let both = perms(&[MOD, MUTED], &overwrites);
    assert!(both.contains(Permissions::SEND_MESSAGES));
    assert!(!both.contains(Permissions::BAN_MEMBERS));
    let mut with_member = overwrites.to_vec();
    with_member.push(overwrite(
        2,
        member_kind,
        Permissions::BAN_MEMBERS,
        Permissions::SEND_MESSAGES,
    ));
    let member = perms(&[MOD], &with_member);
    assert!(member.contains(Permissions::BAN_MEMBERS));
    assert!(!member.contains(Permissions::SEND_MESSAGES));
    // A member overwrite for someone else, or a role the user lacks, does
    // nothing.
    let other = [
        overwrite(3, member_kind, Permissions::empty(), Permissions::all()),
        overwrite(12, role_kind, Permissions::empty(), Permissions::all()),
    ];
    assert_eq!(perms(&[], &other), perms(&[], &[]));
}

fn entry(id: CommandPermissionType, permission: bool) -> CommandPermission {
    CommandPermission { id, permission }
}

fn decide(
    command: Option<&[CommandPermission]>,
    app: &[CommandPermission],
    roles: &[Id<RoleMarker>],
) -> Option<bool> {
    overridden(command, app, GUILD, &[CHANNEL, PARENT], USER, roles)
}

/// The channel decides first: the Command's entry, its "All Channels"
/// entry, then the app-wide ones; a thread's parent counts.
#[test]
fn channel_overrides() {
    let all_channels = CommandPermissionType::Channel(Id::new(99));
    assert_eq!(
        decide(
            None,
            &[entry(CommandPermissionType::Channel(CHANNEL), false)],
            &[]
        ),
        Some(false)
    );
    assert_eq!(
        decide(
            None,
            &[entry(CommandPermissionType::Channel(PARENT), false)],
            &[]
        ),
        Some(false)
    );
    assert_eq!(
        decide(None, &[entry(all_channels, false)], &[]),
        Some(false)
    );
    // The Command's own channel entry wins over the app-wide one.
    let command = [entry(CommandPermissionType::Channel(CHANNEL), true)];
    assert_eq!(
        decide(Some(&command), &[entry(all_channels, false)], &[]),
        None
    );
    // A specific channel wins over "All Channels" on the same level.
    let app = [
        entry(all_channels, false),
        entry(CommandPermissionType::Channel(CHANNEL), true),
    ];
    assert_eq!(decide(None, &app, &[]), None);
}

/// Then the author: user, roles (any allow wins), `@everyone`, on the
/// Command before the app.
#[test]
fn member_overrides() {
    let user = CommandPermissionType::User(USER);
    let everyone = CommandPermissionType::Role(GUILD.cast());
    let moderator = CommandPermissionType::Role(MOD);
    let muted = CommandPermissionType::Role(MUTED);
    assert_eq!(decide(None, &[entry(everyone, false)], &[]), Some(false));
    assert_eq!(
        decide(
            None,
            &[entry(everyone, false), entry(moderator, true)],
            &[MOD]
        ),
        Some(true)
    );
    assert_eq!(
        decide(
            None,
            &[entry(moderator, true), entry(muted, false)],
            &[MOD, MUTED]
        ),
        Some(true)
    );
    assert_eq!(
        decide(None, &[entry(muted, false)], &[MOD, MUTED]),
        Some(false)
    );
    assert_eq!(
        decide(None, &[entry(moderator, true), entry(user, false)], &[MOD]),
        Some(false)
    );
    // The Command's entries come first.
    assert_eq!(
        decide(Some(&[entry(everyone, true)]), &[entry(user, false)], &[]),
        Some(true)
    );
    assert_eq!(decide(Some(&[]), &[], &[MOD]), None);
}

/// The decision: administrators always, then the override, then
/// `default_member_permissions` (no bits: administrators only).
#[test]
fn final_decision() {
    let send = Permissions::SEND_MESSAGES;
    let ban = Permissions::BAN_MEMBERS;
    assert!(allowed(send, None, None));
    assert!(!allowed(send, Some(ban), None));
    assert!(allowed(send | ban, Some(ban), None));
    assert!(!allowed(send, Some(Permissions::empty()), None));
    assert!(allowed(
        Permissions::all(),
        Some(Permissions::empty()),
        None
    ));
    assert!(allowed(send, Some(ban), Some(true)));
    assert!(!allowed(send | ban, Some(ban), Some(false)));
    assert!(allowed(Permissions::all(), None, Some(false)));
}
