//! The Theme file `noctis-theme.css` (TW-02), takumi-free.
//!
//! A Theme declares the tokens Tailwind utilities read (`--color-brand` makes
//! `bg-brand` draw), the theme animations (`--animate-<name>`) and the
//! `@keyframes` they use. The file is a strict subset of CSS, parsed with
//! `cssparser`:
//!
//! - comments are allowed anywhere;
//! - the top level holds only `@theme { … }` (no prelude, so `@theme inline`
//!   is an error) and `@keyframes <name> { … }`;
//! - `@theme` holds only `--<name>: <value>;` declarations. A name maps to the
//!   longest matching [`ThemeNamespace::css_prefix`] (`--font-weight-heavy` is
//!   [`ThemeNamespace::FontWeight`], not [`ThemeNamespace::Font`]) or to
//!   `--animate-`; the rest is the token, `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` of
//!   at most 64 bytes;
//! - values are kept trimmed and verbatim, and must not be empty;
//! - several `@theme` blocks merge; a name defined twice is an error, and so
//!   is a `@keyframes` or `--animate-` named `spin`, `ping`, `pulse` or
//!   `bounce` (the built-in animations);
//! - the file is at most 256 KiB.
//!
//! ```
//! use noctis_image::tw::{Theme, ThemeNamespace, ThemeTokens};
//!
//! let theme = Theme::parse("@theme { --color-brand: #5865f2; }")?;
//! assert!(theme.contains(ThemeNamespace::Color, "brand"));
//! assert!(noctis_image::tw::validate("bg-brand", &theme).is_ok());
//!
//! let err = Theme::parse("@keyframes spin {}").unwrap_err();
//! assert!(err.to_string().starts_with("[TW-02] 1:12: "));
//! # Ok::<(), noctis_image::tw::ThemeError>(())
//! ```

use std::collections::BTreeMap;
use std::fmt;

use cssparser::{Delimiter, ParseError, ParseErrorKind, Parser, SourceLocation, Token};

use super::{ThemeNamespace, ThemeTokens, is_theme_token};

/// The largest Theme file, in bytes (256 KiB).
const MAX_THEME_BYTES: usize = 256 * 1024;

/// The custom-property prefix of a theme animation.
const ANIMATE_PREFIX: &str = "--animate-";

/// The built-in animation names a Theme must not redefine.
const BUILTIN_ANIMATIONS: &[&str] = &["spin", "ping", "pulse", "bounce"];

/// Every namespace, in [`slot`] order.
const NAMESPACES: [ThemeNamespace; 11] = [
    ThemeNamespace::Color,
    ThemeNamespace::Spacing,
    ThemeNamespace::Container,
    ThemeNamespace::Text,
    ThemeNamespace::Font,
    ThemeNamespace::FontWeight,
    ThemeNamespace::Tracking,
    ThemeNamespace::Leading,
    ThemeNamespace::Radius,
    ThemeNamespace::Aspect,
    ThemeNamespace::Breakpoint,
];

/// The index of `ns` in [`NAMESPACES`]; the match is exhaustive, so a new
/// namespace fails to compile until it is listed.
const fn slot(ns: ThemeNamespace) -> usize {
    match ns {
        ThemeNamespace::Color => 0,
        ThemeNamespace::Spacing => 1,
        ThemeNamespace::Container => 2,
        ThemeNamespace::Text => 3,
        ThemeNamespace::Font => 4,
        ThemeNamespace::FontWeight => 5,
        ThemeNamespace::Tracking => 6,
        ThemeNamespace::Leading => 7,
        ThemeNamespace::Radius => 8,
        ThemeNamespace::Aspect => 9,
        ThemeNamespace::Breakpoint => 10,
    }
}

/// A parsed `noctis-theme.css`: its tokens, animations and keyframes.
///
/// It implements [`ThemeTokens`], so [`validate`](super::validate) accepts a
/// theme utility whose token it defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    digest: [u8; 32],
    /// Per namespace ([`slot`] order): token → value.
    tokens: [BTreeMap<String, String>; 11],
    /// `--animate-<name>` entries: (name, value), in file order.
    animations: Vec<(String, String)>,
    /// `@keyframes` blocks: (name, raw block text), in file order.
    keyframes: Vec<(String, String)>,
}

impl Theme {
    /// A Theme with no tokens, animations or keyframes; its digest is the
    /// blake3 hash of the empty text.
    pub fn empty() -> Self {
        Self {
            digest: *blake3::hash(b"").as_bytes(),
            tokens: Default::default(),
            animations: Vec::new(),
            keyframes: Vec::new(),
        }
    }

    /// Parses the text of a `noctis-theme.css` (see the [module docs](self)).
    ///
    /// # Errors
    ///
    /// The first [`ThemeError`] in file order, with its 1-based line and
    /// column; [`ThemeErrorKind::TooLarge`] (at 1:1) when `source` is over
    /// 256 KiB.
    pub fn parse(source: &str) -> Result<Self, ThemeError> {
        if source.len() > MAX_THEME_BYTES {
            return Err(ThemeError {
                line: 1,
                column: 1,
                kind: ThemeErrorKind::TooLarge {
                    bytes: source.len(),
                },
            });
        }
        let mut theme = Self::empty();
        theme.digest = *blake3::hash(source.as_bytes()).as_bytes();
        // cssparser does not strip a UTF-8 BOM; the size check and digest keep it.
        let mut parser = Parser::new(source.strip_prefix('\u{feff}').unwrap_or(source));
        theme.parse_top_level(&mut parser)?;
        Ok(theme)
    }

    /// The Theme of a `noctis-theme.css` that `theme!` validated at compile
    /// time; what the macro expands to.
    ///
    /// # Panics
    ///
    /// When `source` does not parse, which `theme!` rules out.
    #[doc(hidden)]
    pub fn __embedded(source: &'static str) -> Self {
        match Self::parse(source) {
            Ok(theme) => theme,
            Err(err) => panic!("noctis-theme.css is validated at compile time: {err}"),
        }
    }

    /// The blake3 hash of the source text (part of the render cache key).
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// The `--animate-<name>` entries as `(name, value)`, in file order.
    pub fn animations(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.animations
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    /// The `@keyframes` blocks as `(name, raw block text)`, in file order;
    /// the block text is the trimmed text between the braces.
    pub fn keyframes(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.keyframes
            .iter()
            .map(|(name, block)| (name.as_str(), block.as_str()))
    }

    /// Every token as a CSS custom property `(css_prefix + token, value)`,
    /// namespace by namespace, tokens in order; what the Renderer's `@theme`
    /// stylesheet declares.
    #[cfg(any(feature = "render", test))]
    pub(crate) fn custom_properties(&self) -> impl Iterator<Item = (String, &str)> + '_ {
        NAMESPACES.iter().flat_map(move |&ns| {
            self.tokens[slot(ns)]
                .iter()
                .map(move |(token, value)| (format!("{}{token}", ns.css_prefix()), value.as_str()))
        })
    }

    fn parse_top_level(&mut self, parser: &mut Parser<'_>) -> Result<(), ThemeError> {
        while let Some((at, token)) = next_token(parser) {
            match token {
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("theme") => {
                    let (open, token) = next_token(parser).ok_or_else(|| at.error(Syntax))?;
                    if !matches!(token, Token::CurlyBracketBlock) {
                        return Err(open.error(UnexpectedRule));
                    }
                    nested(parser, open, |p| self.parse_theme_block(p))?;
                }
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("keyframes") => {
                    let (name_at, token) = next_token(parser).ok_or_else(|| at.error(Syntax))?;
                    let Token::Ident(name) = token else {
                        return Err(name_at.error(Syntax));
                    };
                    let name = name.to_string();
                    if BUILTIN_ANIMATIONS.contains(&name.as_str()) {
                        return Err(name_at.error(BuiltinAnimation { name }));
                    }
                    if self.keyframes.iter().any(|(n, _)| *n == name) {
                        return Err(name_at.error(Duplicate { name }));
                    }
                    let (open, token) = next_token(parser).ok_or_else(|| name_at.error(Syntax))?;
                    if !matches!(token, Token::CurlyBracketBlock) {
                        return Err(open.error(Syntax));
                    }
                    let block = nested(parser, open, |p| {
                        let start = p.position();
                        while p.next_including_whitespace_and_comments().is_ok() {}
                        Ok(p.slice_from(start).trim().to_owned())
                    })?;
                    self.keyframes.push((name, block));
                }
                _ => return Err(at.error(UnexpectedRule)),
            }
        }
        Ok(())
    }

    fn parse_theme_block(&mut self, parser: &mut Parser<'_>) -> Result<(), ThemeError> {
        while let Some((at, token)) = next_token(parser) {
            match token {
                Token::Ident(name) if name.starts_with("--") => {
                    let name = name.to_string();
                    parser.skip_whitespace();
                    let colon = Location(parser.current_source_location());
                    if parser.expect_colon().is_err() {
                        return Err(colon.error(Syntax));
                    }
                    let value = parser
                        .parse_until_after(Delimiter::Semicolon, |p| {
                            let start = p.position();
                            while p.next_including_whitespace_and_comments().is_ok() {}
                            Ok::<_, ParseError<ThemeError>>(p.slice_from(start).trim().to_owned())
                        })
                        .map_err(|err| from_parse(err, colon))?;
                    self.declare(at, name, value)?;
                }
                Token::Semicolon => {}
                Token::Ident(_) => return Err(at.error(NotCustomProperty)),
                Token::AtKeyword(_) => return Err(at.error(UnexpectedRule)),
                _ => return Err(at.error(Syntax)),
            }
        }
        Ok(())
    }

    /// Records `--<name>: <value>` declared at `at`.
    fn declare(&mut self, at: Location, name: String, value: String) -> Result<(), ThemeError> {
        if let Some(token) = name.strip_prefix(ANIMATE_PREFIX) {
            if !is_theme_token(token) {
                return Err(at.error(InvalidToken { name }));
            }
            if BUILTIN_ANIMATIONS.contains(&token) {
                return Err(at.error(BuiltinAnimation { name }));
            }
            if value.is_empty() {
                return Err(at.error(EmptyValue { name }));
            }
            if self.animations.iter().any(|(n, _)| n == token) {
                return Err(at.error(Duplicate { name }));
            }
            self.animations.push((token.to_owned(), value));
            return Ok(());
        }
        let Some(ns) = NAMESPACES
            .iter()
            .copied()
            .filter(|ns| name.starts_with(ns.css_prefix()))
            .max_by_key(|ns| ns.css_prefix().len())
        else {
            return Err(at.error(UnknownNamespace { name }));
        };
        let token = &name[ns.css_prefix().len()..];
        if !is_theme_token(token) {
            return Err(at.error(InvalidToken { name }));
        }
        if value.is_empty() {
            return Err(at.error(EmptyValue { name }));
        }
        let tokens = &mut self.tokens[slot(ns)];
        if tokens.contains_key(token) {
            return Err(at.error(Duplicate { name }));
        }
        tokens.insert(token.to_owned(), value);
        Ok(())
    }
}

impl ThemeTokens for Theme {
    fn contains(&self, ns: ThemeNamespace, token: &str) -> bool {
        self.tokens[slot(ns)].contains_key(token)
    }
}

/// Why a Theme file is rejected (TW-02), at a 1-based line and column.
///
/// Displays as `[TW-02] <line>:<column>: <message>`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("[TW-02] {line}:{column}: {kind}")]
#[non_exhaustive]
pub struct ThemeError {
    line: u32,
    column: u32,
    kind: ThemeErrorKind,
}

impl ThemeError {
    /// The 1-based line of the offending token.
    pub fn line(&self) -> u32 {
        self.line
    }

    /// The 1-based column of the offending token.
    pub fn column(&self) -> u32 {
        self.column
    }

    /// What is wrong.
    pub fn kind(&self) -> &ThemeErrorKind {
        &self.kind
    }
}

/// What is wrong in a Theme file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThemeErrorKind {
    /// Something other than `@theme { … }` or `@keyframes <name> { … }` at the
    /// top level, a prelude after `@theme`, or an at-rule inside `@theme`.
    UnexpectedRule,
    /// A declaration inside `@theme` that is not a custom property (`--…`).
    NotCustomProperty,
    /// A custom property under no theme namespace and not `--animate-`.
    UnknownNamespace {
        /// The custom property name.
        name: String,
    },
    /// The token after the namespace is not `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`
    /// of at most 64 bytes.
    InvalidToken {
        /// The custom property name.
        name: String,
    },
    /// A custom property with an empty value.
    EmptyValue {
        /// The custom property name.
        name: String,
    },
    /// A custom property or `@keyframes` name defined twice.
    Duplicate {
        /// The custom property or keyframes name.
        name: String,
    },
    /// A `@keyframes` or `--animate-` entry named after a built-in animation
    /// (`spin`, `ping`, `pulse`, `bounce`), which would render static.
    BuiltinAnimation {
        /// The keyframes or custom property name.
        name: String,
    },
    /// Malformed CSS: an unclosed block, a declaration without `:`, a
    /// `@keyframes` without a name or block.
    Syntax,
    /// The file is over 256 KiB (262 144 bytes).
    TooLarge {
        /// The file size in bytes.
        bytes: usize,
    },
}

use ThemeErrorKind::{
    BuiltinAnimation, Duplicate, EmptyValue, InvalidToken, NotCustomProperty, Syntax,
    UnexpectedRule, UnknownNamespace,
};

impl fmt::Display for ThemeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedRule => f.write_str(
                "only `@theme { --<name>: <value>; }` and `@keyframes <name> { … }` are allowed \
                 (no `@theme` prelude, no other rules)",
            ),
            Self::NotCustomProperty => f.write_str(
                "`@theme` holds only custom properties: `--<namespace><token>: <value>;`",
            ),
            Self::UnknownNamespace { name } => {
                let prefixes: Vec<String> = NAMESPACES
                    .iter()
                    .map(|ns| format!("`{}`", ns.css_prefix()))
                    .chain([format!("`{ANIMATE_PREFIX}`")])
                    .collect();
                write!(
                    f,
                    "`{name}` is in no theme namespace; use one of {}",
                    prefixes.join(", ")
                )
            }
            Self::InvalidToken { name } => write!(
                f,
                "`{name}`: the token after the namespace must match \
                 `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` and be at most 64 bytes"
            ),
            Self::EmptyValue { name } => write!(f, "`{name}` has an empty value"),
            Self::Duplicate { name } => write!(f, "`{name}` is defined twice"),
            Self::BuiltinAnimation { name } => write!(
                f,
                "`{name}` redefines a built-in animation (`spin`, `ping`, `pulse`, `bounce`), \
                 which would render static; give it another name"
            ),
            Self::Syntax => f.write_str(
                "malformed CSS: expected `--<name>: <value>;` inside `@theme { … }`, \
                 `@keyframes <name> { … }`, and closed blocks",
            ),
            Self::TooLarge { bytes } => write!(
                f,
                "the theme file is {bytes} bytes; the limit is {MAX_THEME_BYTES} bytes (256 KiB)"
            ),
        }
    }
}

/// A cssparser location (0-based line, 1-based column).
#[derive(Clone, Copy)]
struct Location(SourceLocation);

impl Location {
    fn error(self, kind: ThemeErrorKind) -> ThemeError {
        ThemeError {
            line: self.0.line.saturating_add(1),
            column: self.0.column,
            kind,
        }
    }
}

/// The next token that is not whitespace or a comment, with its location.
fn next_token<'i>(parser: &mut Parser<'i>) -> Option<(Location, Token<'i>)> {
    parser.skip_whitespace();
    let at = Location(parser.current_source_location());
    parser
        .next_including_whitespace_and_comments()
        .ok()
        .map(|token| (at, token.clone()))
}

/// Parses the block just opened at `open`; an unclosed block is a
/// [`ThemeErrorKind::Syntax`] at `open`.
fn nested<'i, T>(
    parser: &mut Parser<'i>,
    open: Location,
    parse: impl FnOnce(&mut Parser<'i>) -> Result<T, ThemeError>,
) -> Result<T, ThemeError> {
    let mut end = 0;
    let value = parser
        .parse_nested_block(|p| {
            let value = parse(p).map_err(custom)?;
            // Everything up to the closing brace (or the end of the input).
            while p.next_including_whitespace_and_comments().is_ok() {}
            end = p.position().byte_index();
            Ok(value)
        })
        .map_err(|err| from_parse(err, open))?;
    // A closed block leaves the parser after its `}`; an unclosed one stops
    // at the end of the input, where the block ended.
    if parser.position().byte_index() == end {
        return Err(open.error(Syntax));
    }
    Ok(value)
}

fn custom(err: ThemeError) -> ParseError<ThemeError> {
    ParseError::custom(err)
}

/// A cssparser error as a [`ThemeError`]: its own errors carry no location,
/// so they are [`ThemeErrorKind::Syntax`] at `fallback`.
fn from_parse(err: ParseError<ThemeError>, fallback: Location) -> ThemeError {
    match err.kind {
        ParseErrorKind::Custom(err) => err,
        ParseErrorKind::Basic(_) => fallback.error(Syntax),
    }
}

#[cfg(test)]
mod tests;
