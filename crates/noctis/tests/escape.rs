//! Story 8.2: the user-text helpers `noctis::ui::escape_markdown` and
//! `escape_mentions` as properties (E10-E12), and MSG-05: a message that
//! lists mentions pings nobody unless it opts in (S1, S2).

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use noctis::twilight::model::id::Id;
use noctis::ui::{
    Components, CreateFlags, Mentions, TextDisplay, Users, V2Message, escape_markdown,
    escape_mentions,
};
use proptest::prelude::*;
use serde_json::json;

const ZWSP: char = '\u{200B}';

/// What Discord renders of backslash-escaped text: a `\` before ASCII
/// punctuation is dropped and the next character is literal.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && next.is_ascii_punctuation()
        {
            out.push(next);
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Text heavy in Markdown and mention syntax.
fn ascii_heavy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::sample::select(vec![
                "\\", "*", "_", "~", "`", "|", "[", "]", "#", ">", "-", "+", ".", " ", "\t",
                "\n", "\r\n", "1", "12", "@", "<", "/", "&", "!", ":", "everyone", "here",
                "EVERYONE", "Here", "a", "b",
            ])
            .prop_map(str::to_owned),
            1 => "[a-z0-9 ]{0,4}",
        ],
        0..24,
    )
    .prop_map(|parts| parts.concat())
}

/// Any Unicode text without U+200B.
fn unicode() -> impl Strategy<Value = String> {
    any::<String>().prop_map(|s| s.replace(ZWSP, ""))
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![ascii_heavy(), unicode()]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// E10: the helpers commute.
    #[test]
    fn e10_helpers_commute(x in text()) {
        prop_assert_eq!(
            escape_markdown(&escape_mentions(&x)),
            escape_mentions(&escape_markdown(&x))
        );
    }

    /// E11: `escape_mentions` only inserts U+200B, and no mention survives.
    #[test]
    fn e11_mentions_are_dead_and_text_is_kept(x in text()) {
        let out = escape_mentions(&x);
        prop_assert_eq!(out.replace(ZWSP, ""), x);
        let lower = out.to_ascii_lowercase();
        for armed in ["<@", "<#", "</", "@everyone", "@here"] {
            prop_assert!(!lower.contains(armed), "{armed} in {out:?}");
        }
    }

    /// E12: what Discord renders of `escape_markdown(x)` is `x`.
    #[test]
    fn e12_markdown_renders_the_users_text(x in text()) {
        prop_assert_eq!(unescape(&escape_markdown(&x)), x);
    }
}

/// S1 and S2 (MSG-05): listed mentions ping nobody by default; a message
/// opts in per id.
#[test]
fn msg_05_listed_mentions_ping_nobody() {
    let listed =
        || V2Message::new(Components::new().push(TextDisplay::new("<@1> <@&2> @everyone")));
    let body = serde_json::to_value(listed().create_body(CreateFlags::empty())).unwrap();
    assert_eq!(body["allowed_mentions"], json!({ "parse": [] }));

    let opted = listed().mentions(Mentions::none().users(Users::only([Id::new(1)])));
    let body = serde_json::to_value(opted.create_body(CreateFlags::empty())).unwrap();
    assert_eq!(
        body["allowed_mentions"],
        json!({ "parse": [], "users": ["1"] })
    );
}
