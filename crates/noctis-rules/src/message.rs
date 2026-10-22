//! Message fields outside the component tree: legacy `content` (MSG-01),
//! allowed mentions (MSG-03..05), nonce and stickers (MSG-06), non-empty
//! bodies (MSG-09), webhook names (WH-04), webhook threads (WH-05) and the
//! non-interactive components of a webhook the app does not own (WH-02).
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
use crate::component::{Accessory, Button, ButtonStyle, Component};
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

/// WH-04: a webhook name of 1 to `webhook_name_len` characters (path
/// `name`), without `clyde` or `discord` in any ASCII case (Discord's Create
/// Webhook rule). A non-ASCII lookalike (`Díscord`) is not reported.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::message::validate_webhook_name;
///
/// let l = Limits::default();
/// assert!(validate_webhook_name("Audit log", &l).is_empty());
/// assert_eq!(validate_webhook_name("My Discord log", &l)[0].rule, RuleId::WH_04);
/// ```
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
    let lower = name.to_ascii_lowercase();
    if lower.contains("clyde") || lower.contains("discord") {
        out.push(Violation::new(
            RuleId::WH_04,
            "name",
            Bound::Describe("a name without `clyde` or `discord` (any case)"),
            Actual::Text(name.to_owned()),
        ));
    }
    out
}

/// The thread a webhook request declares (WH-05), for
/// [`validate_webhook_thread`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookThreadDecl<'a> {
    /// Neither `thread_id` nor `thread_name`.
    None,
    /// A `thread_id` (an existing thread).
    Id,
    /// A `thread_name` (a new forum or media post).
    Name(&'a str),
}

/// WH-05: a webhook request into a forum or media channel (`thread_only`)
/// names a thread; a `thread_name` has 1 to `webhook_thread_name_len`
/// characters and is only valid in a forum or media channel.
///
/// Reports, in this order: a missing thread in a forum or media channel
/// (path `thread_id`); a `thread_name` length out of range (path
/// `thread_name`, in any channel); a `thread_name` outside a forum or media
/// channel (path `thread_name`). A `thread_id` is always fine.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::message::{WebhookThreadDecl, validate_webhook_thread};
///
/// let l = Limits::default();
/// assert!(validate_webhook_thread(true, WebhookThreadDecl::Id, &l).is_empty());
/// assert_eq!(
///     validate_webhook_thread(true, WebhookThreadDecl::None, &l)[0].rule,
///     RuleId::WH_05
/// );
/// ```
pub fn validate_webhook_thread(
    thread_only: bool,
    thread: WebhookThreadDecl<'_>,
    limits: &Limits,
) -> Vec<Violation> {
    let mut out = Vec::new();
    match thread {
        WebhookThreadDecl::None if thread_only => out.push(Violation::new(
            RuleId::WH_05,
            "thread_id",
            Bound::Describe("a thread_id or a thread_name in a forum or media channel"),
            Actual::Text("none".to_owned()),
        )),
        WebhookThreadDecl::Name(name) => {
            text_between(
                RuleId::WH_05,
                "thread_name",
                1,
                limits.webhook_thread_name_len,
                name,
                &mut out,
            );
            if !thread_only {
                out.push(Violation::new(
                    RuleId::WH_05,
                    "thread_name",
                    Bound::Describe("a thread_name only in a forum or media channel"),
                    Actual::Text(name.to_owned()),
                ));
            }
        }
        WebhookThreadDecl::None | WebhookThreadDecl::Id => {}
    }
    out
}

const STATIC_ONLY: &str = "a non-interactive component (a link button, text, media, file, separator, container or section)";

/// WH-02: a webhook the app does not own sends only non-interactive
/// components. Reports one violation per Button whose style is not
/// [`ButtonStyle::Link`] (a Premium button counts as interactive) and per
/// select, at any depth: Action Rows, Section accessories and Container
/// children. Paths follow [`validate_components`](crate::component::validate_components)
/// (`row[0].button[1]`, `section[2].accessory`). [`Component::Opaque`] is
/// skipped: raw JSON is checked where it is built.
///
/// ```
/// use noctis_rules::RuleId;
/// use noctis_rules::component::{Button, ButtonStyle, Component};
/// use noctis_rules::message::validate_static_components;
///
/// let link = [Component::Button(Button::new(ButtonStyle::Link("https://a.b")))];
/// assert!(validate_static_components(&[Component::Row(&link)]).is_empty());
/// let go = [Component::Button(Button::new(ButtonStyle::Primary))];
/// let errs = validate_static_components(&[Component::Row(&go)]);
/// assert_eq!((errs[0].rule, errs[0].path.as_str()), (RuleId::WH_02, "row[0].button[0]"));
/// ```
pub fn validate_static_components(components: &[Component<'_>]) -> Vec<Violation> {
    let mut out = Vec::new();
    for (i, node) in components.iter().enumerate() {
        static_node(node, &format!("{}[{i}]", node.kind()), &mut out);
    }
    out
}

fn interactive(rule_path: &str, kind: &str, out: &mut Vec<Violation>) {
    out.push(Violation::new(
        RuleId::WH_02,
        rule_path,
        Bound::Describe(STATIC_ONLY),
        Actual::Text(kind.to_owned()),
    ));
}

fn static_button(button: &Button<'_>, path: &str, out: &mut Vec<Violation>) {
    if !matches!(button.style, ButtonStyle::Link(_)) {
        interactive(path, "button", out);
    }
}

fn static_node(node: &Component<'_>, path: &str, out: &mut Vec<Violation>) {
    let children = match node {
        Component::Button(b) => {
            static_button(b, path, out);
            return;
        }
        Component::Select(_) => {
            interactive(path, "select", out);
            return;
        }
        Component::Section(s) => {
            if let Accessory::Button(b) = &s.accessory {
                static_button(b, &format!("{path}.accessory"), out);
            }
            return;
        }
        Component::Container(c) => c.children,
        Component::Row(items) => items,
        _ => return,
    };
    for (j, child) in children.iter().enumerate() {
        static_node(child, &format!("{path}.{}[{j}]", child.kind()), out);
    }
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

    #[test]
    fn wh_04_reserved_words() {
        let l = Limits::default();
        let reserved = |name: &str| {
            Violation::new(
                RuleId::WH_04,
                "name",
                Bound::Describe("a name without `clyde` or `discord` (any case)"),
                Actual::Text(name.to_owned()),
            )
        };
        for bad in ["Clyde", "my discord log", "DISCORD"] {
            assert_eq!(validate_webhook_name(bad, &l), [reserved(bad)], "{bad}");
        }
        for ok in ["clyd e", "Díscord"] {
            assert!(validate_webhook_name(ok, &l).is_empty(), "{ok}");
        }
        let long = format!("clyde{}", "a".repeat(80));
        assert_eq!(validate_webhook_name(&long, &l).len(), 2);
    }

    fn wh05(path: &str, bound: Bound, actual: Actual) -> Violation {
        Violation::new(RuleId::WH_05, path, bound, actual)
    }

    #[test]
    fn wh_05_forum_needs_a_thread() {
        let l = Limits::default();
        assert_eq!(
            validate_webhook_thread(true, WebhookThreadDecl::None, &l),
            [wh05(
                "thread_id",
                Bound::Describe("a thread_id or a thread_name in a forum or media channel"),
                Actual::Text("none".to_owned())
            )]
        );
        assert!(validate_webhook_thread(true, WebhookThreadDecl::Id, &l).is_empty());
        assert!(validate_webhook_thread(true, WebhookThreadDecl::Name("Daily log"), &l).is_empty());
        assert!(validate_webhook_thread(false, WebhookThreadDecl::None, &l).is_empty());
        assert!(validate_webhook_thread(false, WebhookThreadDecl::Id, &l).is_empty());
    }

    #[test]
    fn wh_05_thread_name_bounds() {
        let l = Limits::default();
        let between = |count| {
            wh05(
                "thread_name",
                Bound::Between { min: 1, max: 100 },
                Actual::Count(count),
            )
        };
        assert_eq!(
            validate_webhook_thread(true, WebhookThreadDecl::Name(""), &l),
            [between(0)]
        );
        let ok = "a".repeat(100);
        assert!(validate_webhook_thread(true, WebhookThreadDecl::Name(&ok), &l).is_empty());
        let over = "a".repeat(101);
        assert_eq!(
            validate_webhook_thread(true, WebhookThreadDecl::Name(&over), &l),
            [between(101)]
        );
    }

    #[test]
    fn wh_05_thread_name_off_forum() {
        let l = Limits::default();
        assert_eq!(
            validate_webhook_thread(false, WebhookThreadDecl::Name("x"), &l),
            [wh05(
                "thread_name",
                Bound::Describe("a thread_name only in a forum or media channel"),
                Actual::Text("x".to_owned())
            )]
        );
        assert_eq!(
            validate_webhook_thread(false, WebhookThreadDecl::Name(""), &l).len(),
            2
        );
    }

    fn wh02(path: &str, kind: &str) -> Violation {
        Violation::new(
            RuleId::WH_02,
            path,
            Bound::Describe(STATIC_ONLY),
            Actual::Text(kind.to_owned()),
        )
    }

    #[test]
    fn wh_02_static_components_nested() {
        use crate::component::{Container, Media, Section, Select, SelectKind};
        let link = Button::new(ButtonStyle::Link("https://a.b"));
        let go = Button::new(ButtonStyle::Primary).label("Go");
        let premium = Button::new(ButtonStyle::Premium);
        let row = [
            Component::Button(link),
            Component::Select(Select::new(SelectKind::User)),
        ];
        let children = [
            Component::Text("x"),
            Component::Section(Section::new(&["y"], Accessory::Button(go))),
            Component::Row(&row),
            Component::Section(Section::new(
                &["z"],
                Accessory::Thumbnail(Media::new("https://a.b/c.png")),
            )),
        ];
        let premium_row = [Component::Button(premium)];
        let tree = [
            Component::Container(Container::new(&children)),
            Component::Row(&premium_row),
            Component::Opaque(3),
            Component::Section(Section::new(&["w"], Accessory::Button(link))),
        ];
        assert_eq!(
            validate_static_components(&tree),
            [
                wh02("container[0].section[1].accessory", "button"),
                wh02("container[0].row[2].select[1]", "select"),
                wh02("row[1].button[0]", "button"),
            ]
        );
        assert!(validate_static_components(&[Component::Opaque(1)]).is_empty());
        let links = [Component::Button(link)];
        assert!(validate_static_components(&[Component::Row(&links)]).is_empty());
    }
}
