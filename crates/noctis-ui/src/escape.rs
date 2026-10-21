//! Neutralizing user text before it goes into a Text Display.
//!
//! Noctis never escapes on its own: the author passes user-controlled text
//! (names, reasons, bios) through [`escape_markdown`] and, where a mention
//! must not render, [`escape_mentions`]. Both only insert characters, so
//! what Discord shows is exactly the user's text.

/// The zero-width space [`escape_mentions`] inserts.
const ZWSP: char = '\u{200B}';

/// Characters that are Markdown anywhere in a line.
const fn is_inline_markup(c: char) -> bool {
    matches!(c, '\\' | '*' | '_' | '~' | '`' | '|' | '[' | ']')
}

/// Characters that are Markdown only as the first non-blank of a line.
const fn is_line_start_markup(c: char) -> bool {
    matches!(c, '#' | '>' | '-' | '+')
}

/// Escapes Discord Markdown in `text` with backslashes, so it renders as
/// written.
///
/// - Everywhere: `\ * _ ~` `` ` `` `| [ ]` (bold, italics, underline,
///   strikethrough, code, spoilers, masked links, and the backslash itself).
/// - At the start of the text or of a line (after optional ASCII spaces and
///   tabs): `# > - +` (headings, subtext, quotes, lists), and the `.` that
///   ends a run of ASCII digits (numbered lists). Mid-line, these stay as
///   they are.
///
/// Mentions are not Markdown; use [`escape_mentions`] for them. The two
/// helpers commute, so their order does not matter.
///
/// Escape a text once: the function is not idempotent, and a second pass
/// makes the first pass's backslashes visible.
///
/// ```
/// use noctis_ui::escape_markdown;
///
/// assert_eq!(escape_markdown("**hi** [x](https://e.com)"), r"\*\*hi\*\* \[x\](https://e.com)");
/// assert_eq!(escape_markdown("# title\n1. item"), "\\# title\n1\\. item");
/// assert_eq!(escape_markdown("a # b"), "a # b");
/// ```
#[must_use]
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut chars = text.chars().peekable();
    let mut line_start = true;
    while let Some(c) = chars.next() {
        if line_start {
            match c {
                ' ' | '\t' => {
                    out.push(c);
                    continue;
                }
                '0'..='9' => {
                    out.push(c);
                    while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                        out.push(d);
                        chars.next();
                    }
                    if chars.peek() == Some(&'.') {
                        out.push('\\');
                        out.push('.');
                        chars.next();
                    }
                    line_start = false;
                    continue;
                }
                _ if is_line_start_markup(c) => {
                    out.push('\\');
                    out.push(c);
                    line_start = false;
                    continue;
                }
                _ => {}
            }
        }
        if is_inline_markup(c) {
            out.push('\\');
        }
        out.push(c);
        line_start = c == '\n';
    }
    out
}

/// Stops mentions in `text` from rendering or pinging, by inserting a
/// zero-width space (U+200B).
///
/// The space goes after `@` when `everyone` or `here` follows (ASCII
/// case-insensitive), and after `<` when `@`, `#` or `/` follows (user,
/// role, channel and command mentions). Emoji (`<:e:1>`), timestamps
/// (`<t:1:R>`) and links stay as they are; text such as `mail@here.io` gets
/// a harmless extra space.
///
/// A zero-width space rather than a backslash, because [`escape_markdown`]
/// doubles a backslash and would re-arm the mention; with U+200B the two
/// helpers commute. A message's `allowed_mentions` still decides who is
/// pinged (nobody by default); this helper only changes what renders.
///
/// Escape a text once: the function is not idempotent.
///
/// ```
/// use noctis_ui::escape_mentions;
///
/// assert_eq!(escape_mentions("hi <@1> @everyone"), "hi <\u{200B}@1> @\u{200B}everyone");
/// assert_eq!(escape_mentions("<t:1:R>"), "<t:1:R>");
/// ```
#[must_use]
pub fn escape_mentions(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 6);
    for (i, c) in text.char_indices() {
        out.push(c);
        let rest = &text.as_bytes()[i + c.len_utf8()..];
        let arms = match c {
            '@' => {
                starts_with_ignore_case(rest, b"everyone") || starts_with_ignore_case(rest, b"here")
            }
            '<' => matches!(rest.first(), Some(b'@' | b'#' | b'/')),
            _ => false,
        };
        if arms {
            out.push(ZWSP);
        }
    }
    out
}

/// Whether `bytes` starts with the ASCII `word`, ignoring ASCII case.
fn starts_with_ignore_case(bytes: &[u8], word: &[u8]) -> bool {
    bytes
        .get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn md(input: &str, expected: &str) {
        assert_eq!(escape_markdown(input), expected, "input {input:?}");
    }

    #[track_caller]
    fn me(input: &str, expected: &str) {
        assert_eq!(escape_mentions(input), expected, "input {input:?}");
    }

    #[test]
    fn escape_markdown_empty() {
        md("", "");
    }

    #[test]
    fn escape_markdown_plain() {
        md("plain text 123", "plain text 123");
    }

    #[test]
    fn escape_markdown_emphasis() {
        md("**b** __u__ ~~s~~", r"\*\*b\*\* \_\_u\_\_ \~\~s\~\~");
    }

    #[test]
    fn escape_markdown_spoiler() {
        md("||sp||", r"\|\|sp\|\|");
    }

    #[test]
    fn escape_markdown_code() {
        md("`c`", r"\`c\`");
        md("```\nx\n```", "\\`\\`\\`\nx\n\\`\\`\\`");
    }

    #[test]
    fn escape_markdown_masked_link() {
        md("[x](https://e.com)", r"\[x\](https://e.com)");
    }

    #[test]
    fn escape_markdown_backslash() {
        md(r"a\b", r"a\\b");
    }

    #[test]
    fn escape_markdown_headings() {
        md("# h", r"\# h");
        md("## h", r"\## h");
        md("-# sub", r"\-# sub");
    }

    #[test]
    fn escape_markdown_quotes_and_lists() {
        md("> q", r"\> q");
        md(">>> q", r"\>>> q");
        md("- i", r"\- i");
        md("+ i", r"\+ i");
    }

    #[test]
    fn escape_markdown_numbered_lists() {
        md("1. i", r"1\. i");
        md("  12. i", r"  12\. i");
        md("1.5 kg", r"1\.5 kg");
    }

    #[test]
    fn escape_markdown_mid_line_markers() {
        md("a # b - c > d 3. e", "a # b - c > d 3. e");
    }

    #[test]
    fn escape_markdown_line_starts() {
        md("x\n# y", "x\n\\# y");
        md("x\r\n- y", "x\r\n\\- y");
        md("\n\n> z", "\n\n\\> z");
    }

    #[test]
    fn escape_markdown_tab_indent() {
        md("\t# t", "\t\\# t");
    }

    #[test]
    fn escape_markdown_unicode() {
        md("😀*é", "😀\\*é");
        md("＊ｆ", "＊ｆ");
    }

    #[test]
    fn escape_markdown_mentions_untouched() {
        md("<@1> @everyone", "<@1> @everyone");
    }

    #[test]
    fn escape_markdown_doubles_length() {
        let out = escape_markdown(&"*".repeat(4000));
        assert_eq!(out.chars().count(), 8000);
    }

    #[test]
    fn escape_mentions_empty() {
        me("", "");
    }

    #[test]
    fn escape_mentions_everyone_here() {
        me("@everyone", "@\u{200B}everyone");
        me("@HERE", "@\u{200B}HERE");
    }

    #[test]
    fn escape_mentions_users_and_roles() {
        me("<@1>", "<\u{200B}@1>");
        me("<@!1>", "<\u{200B}@!1>");
        me("<@&2>", "<\u{200B}@&2>");
    }

    #[test]
    fn escape_mentions_channels_and_commands() {
        me("<#3>", "<\u{200B}#3>");
        me("</ban:4>", "<\u{200B}/ban:4>");
    }

    #[test]
    fn escape_mentions_non_mentions() {
        for s in ["@someone", "a@b.com", "<:e:1>", "<t:1:R>", "<https://x>"] {
            me(s, s);
        }
    }

    #[test]
    fn escape_mentions_over_escapes_harmlessly() {
        me("mail@here.io", "mail@\u{200B}here.io");
    }

    #[test]
    fn escape_mentions_repeated_openers() {
        me("@@everyone", "@@\u{200B}everyone");
        me("<<@1>", "<<\u{200B}@1>");
    }

    #[test]
    fn escape_mentions_truncated() {
        me("@", "@");
        me("<", "<");
        me("@ever", "@ever");
        me("<@", "<\u{200B}@");
    }

    #[test]
    fn escape_mentions_unicode() {
        me("😀<@1>", "😀<\u{200B}@1>");
    }
}
