//! Message flags per endpoint (MSG-07, INT-09) and [`MessageFormat`].
//!
//! Each endpoint has its own flag set, and a set has an associated const only
//! for the flags that endpoint accepts. None has `IS_COMPONENTS_V2`: a V2 body
//! always adds it and a legacy body never does (CV2-01).

use core::ops::{BitOr, BitOrAssign};

/// `SUPPRESS_EMBEDS` (`1 << 2`).
const SUPPRESS_EMBEDS: u64 = 1 << 2;
/// `EPHEMERAL` (`1 << 6`).
const EPHEMERAL: u64 = 1 << 6;
/// `SUPPRESS_NOTIFICATIONS` (`1 << 12`).
const SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;
/// `IS_COMPONENTS_V2` (`1 << 15`), added by every V2 body (CV2-01).
pub(crate) const IS_COMPONENTS_V2: u64 = 1 << 15;

/// The operations every flag set shares.
macro_rules! flag_set {
    ($name:ident) => {
        impl $name {
            /// No flag.
            pub const fn empty() -> Self {
                Self(0)
            }

            /// The flags of `self` and of `other`.
            #[must_use]
            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// Whether every flag of `other` is in `self`.
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// Whether no flag is set.
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// The flag bits set by the author; never `IS_COMPONENTS_V2`,
            /// which a V2 body adds itself.
            pub const fn bits(self) -> u64 {
                self.0
            }
        }

        impl BitOr for $name {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self {
                self.union(rhs)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                *self = self.union(rhs);
            }
        }
    };
}

/// The flags of a new message: Create Message and Execute Webhook (MSG-07).
///
/// ```
/// use noctis_ui::CreateFlags;
///
/// const QUIET: CreateFlags = CreateFlags::SUPPRESS_EMBEDS.union(CreateFlags::SUPPRESS_NOTIFICATIONS);
/// assert_eq!(QUIET.bits(), 4100);
/// assert!(QUIET.contains(CreateFlags::SUPPRESS_EMBEDS));
/// assert!(CreateFlags::default().is_empty());
/// ```
///
/// `IS_COMPONENTS_V2` is not a flag you set: a V2 body always adds it, and
/// nothing removes it:
///
/// ```compile_fail,E0599
/// use noctis_ui::CreateFlags;
///
/// let _ = CreateFlags::IS_COMPONENTS_V2; // [CV2-01]
/// ```
///
/// Create Message takes no `EPHEMERAL`; only interaction callbacks and
/// follow-ups do ([`CallbackFlags`]):
///
/// ```compile_fail,E0599
/// use noctis_ui::CreateFlags;
///
/// let _ = CreateFlags::EPHEMERAL; // [MSG-07]
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CreateFlags(u64);

impl CreateFlags {
    /// Hides the link embeds of `content` (`1 << 2`).
    pub const SUPPRESS_EMBEDS: Self = Self(SUPPRESS_EMBEDS);
    /// Sends without a push or desktop notification (`1 << 12`).
    pub const SUPPRESS_NOTIFICATIONS: Self = Self(SUPPRESS_NOTIFICATIONS);
}

flag_set!(CreateFlags);

/// The flags of an edit: Edit Message, Edit Original Interaction Response,
/// Edit Webhook Message and the data of callback type 7 (`UPDATE_MESSAGE`)
/// (MSG-07).
///
/// An edit sends `flags` every time, so leaving `SUPPRESS_EMBEDS` out clears
/// it.
///
/// ```
/// use noctis_ui::EditFlags;
///
/// let mut flags = EditFlags::empty();
/// flags |= EditFlags::SUPPRESS_EMBEDS;
/// assert_eq!(flags.bits(), 4);
/// ```
///
/// An edit cannot add `IS_COMPONENTS_V2` by hand, nor remove it:
///
/// ```compile_fail,E0599
/// use noctis_ui::EditFlags;
///
/// let _ = EditFlags::IS_COMPONENTS_V2; // [CV2-01]
/// ```
///
/// Notifications and ephemerality are fixed when the message is sent:
///
/// ```compile_fail,E0599
/// use noctis_ui::EditFlags;
///
/// let _ = EditFlags::SUPPRESS_NOTIFICATIONS; // [MSG-07]
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::EditFlags;
///
/// let _ = EditFlags::EPHEMERAL; // [MSG-07]
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EditFlags(u64);

impl EditFlags {
    /// Hides the link embeds of `content` (`1 << 2`); left out, it is
    /// cleared.
    pub const SUPPRESS_EMBEDS: Self = Self(SUPPRESS_EMBEDS);
}

flag_set!(EditFlags);

/// The flags of an interaction reply: the data of callback type 4
/// (`CHANNEL_MESSAGE_WITH_SOURCE`) and follow-up messages (INT-09, MSG-07).
///
/// ```
/// use noctis_ui::CallbackFlags;
///
/// let flags = CallbackFlags::EPHEMERAL | CallbackFlags::SUPPRESS_NOTIFICATIONS;
/// assert_eq!(flags.bits(), 4160);
/// assert!(!CallbackFlags::EPHEMERAL.contains(CallbackFlags::SUPPRESS_EMBEDS));
/// ```
///
/// Only the documented callback flags exist; there is no way to set any
/// other bit:
///
/// ```compile_fail,E0599
/// use noctis_ui::CallbackFlags;
///
/// let _ = CallbackFlags::CROSSPOSTED; // [INT-09]
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::CallbackFlags;
///
/// let _ = CallbackFlags::from_bits(1); // [INT-09]
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::CallbackFlags;
///
/// let _ = CallbackFlags::IS_COMPONENTS_V2; // [CV2-01]
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CallbackFlags(u64);

impl CallbackFlags {
    /// Hides the link embeds of `content` (`1 << 2`).
    pub const SUPPRESS_EMBEDS: Self = Self(SUPPRESS_EMBEDS);
    /// Only the invoking user sees the message (`1 << 6`).
    pub const EPHEMERAL: Self = Self(EPHEMERAL);
    /// Sends without a push or desktop notification (`1 << 12`).
    pub const SUPPRESS_NOTIFICATIONS: Self = Self(SUPPRESS_NOTIFICATIONS);
}

flag_set!(CallbackFlags);

/// The format of a message that already exists, for an edit: legacy
/// (`content`, embeds, legacy rows) or Components V2.
///
/// Core reads it from the message's flags; an edit of a legacy message into
/// V2 clears its legacy fields, and a V2 message never goes back (CV2-01).
///
/// ```
/// use noctis_ui::MessageFormat;
///
/// assert_eq!(MessageFormat::from_flags(4), MessageFormat::Legacy);
/// assert_eq!(MessageFormat::from_flags(32768 | 64), MessageFormat::V2);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageFormat {
    /// A message without `IS_COMPONENTS_V2`.
    Legacy,
    /// A message with `IS_COMPONENTS_V2`.
    V2,
}

impl MessageFormat {
    /// The format of a message whose `flags` field is `bits`: V2 when
    /// `IS_COMPONENTS_V2` (`1 << 15`) is set.
    pub const fn from_flags(bits: u64) -> Self {
        if bits & IS_COMPONENTS_V2 == 0 {
            Self::Legacy
        } else {
            Self::V2
        }
    }
}
