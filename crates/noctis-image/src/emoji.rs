//! Discord custom emoji tokens in text (IMG-04), takumi-free.
//!
//! A token is Discord's markdown form `<:name:id>` (static) or `<a:name:id>`
//! (animated), ASCII only:
//!
//! - the name is 2..=32 of `[A-Za-z0-9_]`;
//! - the id is 17..=20 ASCII digits, parsed as a non-zero `u64`.
//!
//! Anything else stays literal text. [`split`] cuts a text into
//! [`Piece`]s and [`scan`] lists the distinct emoji in raw bytes (the
//! Renderer scans a job's postcard data with it); both share one byte
//! matcher and allocate nothing when the input has no `<`.
//!
//! ```
//! use noctis_image::emoji::{self, Piece};
//!
//! let pieces: Vec<_> = emoji::split("hi <:pepe:123456789012345678>!").collect();
//! assert_eq!(pieces.len(), 3);
//! assert!(matches!(pieces[1], Piece::Emoji(e) if !e.animated));
//! ```

use std::collections::HashSet;
use std::num::NonZeroU64;

#[cfg(test)]
mod tests;

/// Shortest emoji name Discord accepts.
const NAME_MIN: usize = 2;
/// Longest emoji name Discord accepts.
const NAME_MAX: usize = 32;
/// Fewest digits of a snowflake id in a token.
const ID_MIN: usize = 17;
/// Most digits of a snowflake id in a token (`u64::MAX` has 20).
const ID_MAX: usize = 20;

/// One custom emoji found in text: its id and whether the token was
/// `<a:..>`. The name is not kept; Discord draws an emoji by id alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CustomEmoji {
    /// The emoji's snowflake id.
    pub id: NonZeroU64,
    /// `true` for an `<a:name:id>` token.
    pub animated: bool,
}

/// A piece of text cut by [`split`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    /// Literal text, never empty.
    Text(&'a str),
    /// A custom emoji token.
    Emoji(CustomEmoji),
}

/// Cuts `text` into literal text and custom emoji tokens, in order.
///
/// Concatenating the text pieces with the tokens restores `text`; empty text
/// pieces are never yielded.
#[must_use]
pub fn split(text: &str) -> Split<'_> {
    Split { text, pos: 0 }
}

/// The distinct `(id, animated)` pairs of the custom emoji tokens in
/// `bytes`, in order of first appearance. `bytes` need not be UTF-8.
#[must_use]
pub fn scan(bytes: &[u8]) -> Vec<CustomEmoji> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut from = 0;
    while let Some((at, emoji, len)) = next_token(bytes, from) {
        if seen.insert(emoji) {
            found.push(emoji);
        }
        from = at + len;
    }
    found
}

/// Iterator returned by [`split`].
#[derive(Clone, Debug)]
pub struct Split<'a> {
    text: &'a str,
    /// Start of the part not yet yielded.
    pos: usize,
}

impl<'a> Iterator for Split<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        let rest = self.text.get(self.pos..).filter(|r| !r.is_empty())?;
        match next_token(self.text.as_bytes(), self.pos) {
            Some((at, emoji, len)) if at == self.pos => {
                self.pos += len;
                Some(Piece::Emoji(emoji))
            }
            // A token starts and ends on ASCII bytes, so `at` is a char
            // boundary.
            Some((at, ..)) => {
                let piece = &self.text[self.pos..at];
                self.pos = at;
                Some(Piece::Text(piece))
            }
            None => {
                self.pos = self.text.len();
                Some(Piece::Text(rest))
            }
        }
    }
}

impl std::iter::FusedIterator for Split<'_> {}

/// The first token at or after `from`: its start, the emoji and its length.
fn next_token(bytes: &[u8], from: usize) -> Option<(usize, CustomEmoji, usize)> {
    let mut at = from;
    loop {
        at += bytes.get(at..)?.iter().position(|&b| b == b'<')?;
        if let Some((emoji, len)) = token_at(&bytes[at..]) {
            return Some((at, emoji, len));
        }
        at += 1;
    }
}

/// Matches a token at the start of `bytes` (which starts with `<`).
fn token_at(bytes: &[u8]) -> Option<(CustomEmoji, usize)> {
    let mut i = 1;
    let animated = bytes.get(i) == Some(&b'a');
    if animated {
        i += 1;
    }
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let name = run_len(&bytes[i..], |b| b.is_ascii_alphanumeric() || b == b'_');
    if !(NAME_MIN..=NAME_MAX).contains(&name) {
        return None;
    }
    i += name;
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let digits = run_len(&bytes[i..], |b| b.is_ascii_digit());
    if !(ID_MIN..=ID_MAX).contains(&digits) {
        return None;
    }
    let id = bytes[i..i + digits].iter().try_fold(0u64, |acc, &d| {
        acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
    })?;
    let id = NonZeroU64::new(id)?;
    i += digits;
    if bytes.get(i) != Some(&b'>') {
        return None;
    }
    Some((CustomEmoji { id, animated }, i + 1))
}

/// Length of the leading run of bytes matching `pred`.
fn run_len(bytes: &[u8], pred: impl Fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|&&b| pred(b)).count()
}
