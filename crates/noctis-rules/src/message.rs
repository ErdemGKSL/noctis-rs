//! Message fields outside the component tree: legacy `content` (MSG-01),
//! allowed mentions (MSG-03..05), nonce and stickers (MSG-06), non-empty
//! bodies (MSG-09) and webhook names (WH-04).
//!
//! - MSG-02: Noctis has no embed API in v1, so there is nothing to check; an
//!   embed builder added later must add its limits to this module.
//! - MSG-03: [`MentionIds`] is either "all" or "only these ids", so `parse`
//!   and an id list can never be combined for one type:
//!
//! ```compile_fail
//! use noctis_rules::message::MentionIds;
//!
//! let _ = MentionIds::All(&[1_u64]); // [MSG-03]
//! ```
//!
//! - MSG-05: [`AllowedMentions::new`] (also the [`Default`]) pings nobody;
//!   pinging is opt-in.
//! - MSG-09: a Components V2 message needs at least one top-level component
//!   ([`validate_v2_not_empty`]); a legacy message needs non-blank `content`
//!   or at least one Action Row ([`validate_legacy_not_empty`]). Whitespace-only
//!   content counts as empty.
//!
//! The functions only report: they never truncate.

use crate::check::{at_most, text_at_most, text_between};
use crate::{Actual, Bound, Limits, RuleId, Violation};

/// Which ids of one type (users or roles) may be pinged (MSG-03).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MentionIds<'a> {
    /// Nobody. The default.
    #[default]
    None,
    /// Everyone of this type mentioned in the message (`parse`).
    All,
    /// Only these ids (≤ 100, MSG-04).
    Only(&'a [u64]),
}

/// The `allowed_mentions` of a message; the default pings nobody (MSG-05).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllowedMentions<'a> {
    /// User mentions.
    pub users: MentionIds<'a>,
    /// Role mentions.
    pub roles: MentionIds<'a>,
    /// Whether `@everyone` / `@here` ping.
    pub everyone: bool,
    /// Whether the replied-to user is pinged.
    pub replied_user: bool,
}

impl<'a> AllowedMentions<'a> {
    /// Pings nobody.
    pub const fn new() -> Self {
        Self {
            users: MentionIds::None,
            roles: MentionIds::None,
            everyone: false,
            replied_user: false,
        }
    }

    /// Sets the user mentions.
    #[must_use]
    pub const fn users(self, users: MentionIds<'a>) -> Self {
        Self { users, ..self }
    }

    /// Sets the role mentions.
    #[must_use]
    pub const fn roles(self, roles: MentionIds<'a>) -> Self {
        Self { roles, ..self }
    }

    /// Sets whether `@everyone` / `@here` ping.
    #[must_use]
    pub const fn everyone(self, everyone: bool) -> Self {
        Self { everyone, ..self }
    }

    /// Sets whether the replied-to user is pinged.
    #[must_use]
    pub const fn replied_user(self, replied_user: bool) -> Self {
        Self {
            replied_user,
            ..self
        }
    }
}

/// MSG-01: legacy `content` of at most `content_len` characters (path
/// `content`).
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::message::validate_content;
///
/// assert!(validate_content("hi", &Limits::default()).is_empty());
/// assert_eq!(validate_content(&"a".repeat(2001), &Limits::default())[0].rule, RuleId::MSG_01);
/// ```
pub fn validate_content(content: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(
        RuleId::MSG_01,
        "content",
        limits.content_len,
        content,
        &mut out,
    );
    out
}

/// MSG-04: at most `mention_users` user ids and `mention_roles` role ids
/// (paths `allowed_mentions.users`, then `allowed_mentions.roles`).
pub fn validate_mentions(mentions: &AllowedMentions<'_>, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    for (ids, max, path) in [
        (
            mentions.users,
            limits.mention_users,
            "allowed_mentions.users",
        ),
        (
            mentions.roles,
            limits.mention_roles,
            "allowed_mentions.roles",
        ),
    ] {
        if let MentionIds::Only(ids) = ids {
            at_most(RuleId::MSG_04, path, max, ids.len(), &mut out);
        }
    }
    out
}

/// MSG-06: a nonce of at most `nonce_len` characters (path `nonce`).
pub fn validate_nonce(nonce: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(RuleId::MSG_06, "nonce", limits.nonce_len, nonce, &mut out);
    out
}

/// MSG-06: at most `sticker_ids` stickers (path `sticker_ids`).
pub fn validate_sticker_ids(count: usize, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    at_most(
        RuleId::MSG_06,
        "sticker_ids",
        limits.sticker_ids,
        count,
        &mut out,
    );
    out
}

/// MSG-09: a Components V2 message has at least one top-level component
/// (path `components`).
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::message::validate_v2_not_empty;
///
/// assert!(validate_v2_not_empty(1, &Limits::default()).is_empty());
/// assert_eq!(validate_v2_not_empty(0, &Limits::default())[0].rule, RuleId::MSG_09);
/// ```
pub fn validate_v2_not_empty(components: usize, limits: &Limits) -> Vec<Violation> {
    let _ = limits;
    if components > 0 {
        return Vec::new();
    }
    vec![Violation::new(
        RuleId::MSG_09,
        "components",
        Bound::AtLeast(1),
        Actual::Count(components),
    )]
}

/// MSG-09: a legacy message has `content` that is not empty or
/// whitespace-only, or at least one Action Row (path `content`).
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::message::validate_legacy_not_empty;
///
/// let l = Limits::default();
/// assert!(validate_legacy_not_empty("hi", 0, &l).is_empty());
/// assert!(validate_legacy_not_empty("", 1, &l).is_empty());
/// assert_eq!(validate_legacy_not_empty(" \n", 0, &l)[0].rule, RuleId::MSG_09);
/// ```
pub fn validate_legacy_not_empty(content: &str, rows: usize, limits: &Limits) -> Vec<Violation> {
    let _ = limits;
    if rows > 0 || !content.trim().is_empty() {
        return Vec::new();
    }
    vec![Violation::new(
        RuleId::MSG_09,
        "content",
        Bound::Describe("non-empty content or at least one Action Row"),
        Actual::Text(content.to_owned()),
    )]
}

/// WH-04: a webhook name of 1 to `webhook_name_len` characters (path `name`).
pub fn validate_webhook_name(name: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_between(
        RuleId::WH_04,
        "name",
        1,
        limits.webhook_name_len,
        name,
        &mut out,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max(rule: RuleId, path: &str, max: usize, count: usize) -> Violation {
        Violation::new(rule, path, Bound::AtMost(max), Actual::Count(count))
    }

    #[test]
    fn msg_01_content_length() {
        let l = Limits::default();
        for ok in [String::new(), "a".repeat(2000), "😀".repeat(1000)] {
            assert!(validate_content(&ok, &l).is_empty());
        }
        assert_eq!(
            validate_content(&"a".repeat(2001), &l),
            [max(RuleId::MSG_01, "content", 2000, 2001)]
        );
        assert_eq!(
            validate_content(&"😀".repeat(1001), &l),
            [max(RuleId::MSG_01, "content", 2000, 2002)]
        );
        let lowered = Limits {
            content_len: 5,
            ..Limits::default()
        };
        assert_eq!(
            validate_content("abcdef", &lowered),
            [max(RuleId::MSG_01, "content", 5, 6)]
        );
    }

    #[test]
    fn msg_04_mention_ids() {
        let l = Limits::default();
        let ids: Vec<u64> = (0..101).collect();
        for ok in [
            AllowedMentions::new(),
            AllowedMentions::new().users(MentionIds::Only(&ids[..100])),
            AllowedMentions::new().users(MentionIds::All),
            AllowedMentions::new().roles(MentionIds::Only(&[])),
        ] {
            assert!(validate_mentions(&ok, &l).is_empty());
        }
        let users = max(RuleId::MSG_04, "allowed_mentions.users", 100, 101);
        let roles = max(RuleId::MSG_04, "allowed_mentions.roles", 100, 101);
        let m = AllowedMentions::new().users(MentionIds::Only(&ids));
        assert_eq!(validate_mentions(&m, &l), std::slice::from_ref(&users));
        let m = AllowedMentions::new().roles(MentionIds::Only(&ids));
        assert_eq!(validate_mentions(&m, &l), std::slice::from_ref(&roles));
        let m = m.users(MentionIds::Only(&ids));
        assert_eq!(validate_mentions(&m, &l), [users, roles]);
        let lowered = Limits {
            mention_users: 1,
            ..Limits::default()
        };
        let m = AllowedMentions::new().users(MentionIds::Only(&ids[..2]));
        assert_eq!(
            validate_mentions(&m, &lowered),
            [max(RuleId::MSG_04, "allowed_mentions.users", 1, 2)]
        );
    }

    #[test]
    fn msg_05_default_parses_nothing() {
        let d = AllowedMentions::default();
        assert_eq!(d, AllowedMentions::new());
        assert_eq!(d.users, MentionIds::None);
        assert_eq!(d.roles, MentionIds::None);
        assert!(!d.everyone);
        assert!(!d.replied_user);
    }

    #[test]
    fn msg_06_nonce_and_stickers() {
        let l = Limits::default();
        let mixed = format!("{}a", "😀".repeat(12));
        for ok in ["", "aaaaaaaaaaaaaaaaaaaaaaaaa", mixed.as_str()] {
            assert!(validate_nonce(ok, &l).is_empty(), "{ok}");
        }
        assert_eq!(
            validate_nonce(&"a".repeat(26), &l),
            [max(RuleId::MSG_06, "nonce", 25, 26)]
        );
        assert_eq!(
            validate_nonce(&"😀".repeat(13), &l),
            [max(RuleId::MSG_06, "nonce", 25, 26)]
        );
        assert!(validate_sticker_ids(0, &l).is_empty());
        assert!(validate_sticker_ids(3, &l).is_empty());
        assert_eq!(
            validate_sticker_ids(4, &l),
            [max(RuleId::MSG_06, "sticker_ids", 3, 4)]
        );
        let lowered = Limits {
            sticker_ids: 1,
            ..Limits::default()
        };
        assert_eq!(
            validate_sticker_ids(2, &lowered),
            [max(RuleId::MSG_06, "sticker_ids", 1, 2)]
        );
    }

    #[test]
    fn msg_09_v2_not_empty() {
        let l = Limits::default();
        assert_eq!(
            validate_v2_not_empty(0, &l),
            [Violation::new(
                RuleId::MSG_09,
                "components",
                Bound::AtLeast(1),
                Actual::Count(0)
            )]
        );
        assert!(validate_v2_not_empty(1, &l).is_empty());
        assert!(validate_v2_not_empty(40, &l).is_empty());
    }

    #[test]
    fn msg_09_legacy_not_empty() {
        let l = Limits::default();
        let empty = |content: &str| {
            Violation::new(
                RuleId::MSG_09,
                "content",
                Bound::Describe("non-empty content or at least one Action Row"),
                Actual::Text(content.to_owned()),
            )
        };
        assert_eq!(validate_legacy_not_empty("", 0, &l), [empty("")]);
        assert_eq!(validate_legacy_not_empty(" ", 0, &l), [empty(" ")]);
        assert_eq!(
            validate_legacy_not_empty("  \n\t", 0, &l),
            [empty("  \n\t")]
        );
        assert!(validate_legacy_not_empty("", 1, &l).is_empty());
        assert!(validate_legacy_not_empty(" ", 5, &l).is_empty());
        assert!(validate_legacy_not_empty("x", 0, &l).is_empty());
    }

    #[test]
    fn wh_04_webhook_name() {
        let l = Limits::default();
        let between = |max, count| {
            Violation::new(
                RuleId::WH_04,
                "name",
                Bound::Between { min: 1, max },
                Actual::Count(count),
            )
        };
        for ok in ["a".to_owned(), "a".repeat(80), "😀".repeat(40)] {
            assert!(validate_webhook_name(&ok, &l).is_empty());
        }
        assert_eq!(validate_webhook_name("", &l), [between(80, 0)]);
        assert_eq!(
            validate_webhook_name(&"a".repeat(81), &l),
            [between(80, 81)]
        );
        assert_eq!(
            validate_webhook_name(&"😀".repeat(41), &l),
            [between(80, 82)]
        );
        let lowered = Limits {
            webhook_name_len: 10,
            ..Limits::default()
        };
        assert_eq!(
            validate_webhook_name(&"a".repeat(11), &lowered),
            [between(10, 11)]
        );
    }
}
