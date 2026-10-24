//! Strict Tailwind v4 class grammar, takumi-free.
//!
//! takumi reads any class it cannot parse as a CSS variable lookup or drops
//! it silently: `bg-gradient-to-r` becomes `var(--color-gradient-to-r)` and
//! draws nothing. This module classifies a class the way takumi-core's
//! `TailwindValue::parse` does, over tables generated from the pinned
//! takumi-core sources (`cargo run --release -p xtask -- tw-grammar`), and
//! narrows the result so that only classes that draw something pass:
//!
//! - [`classify`] tells a built-in utility ([`Utility::Builtin`]) from one
//!   that only reads a theme token ([`Utility::Theme`]), a theme animation
//!   ([`Utility::Animation`], `animate-<token>`) or a theme `@keyframes`
//!   ([`Utility::Keyframes`], `animate-[<keyframes>_1s_…]`);
//! - [`validate`] accepts a class list when every class is built in or every
//!   token it reads is defined by the [`ThemeTokens`];
//! - [`suggest`] names the Tailwind v4 replacement of a known v3 class;
//! - [`Theme`] parses a crate's `noctis-theme.css` into the tokens
//!   [`validate`] reads ([`theme`]).
//!
//! ```
//! use noctis_image::tw::{self, NoTheme, Utility};
//!
//! assert_eq!(tw::classify("p-4").map(|c| c.utility), Some(Utility::Builtin));
//! let err = tw::validate("p-4 bg-gradient-to-r", &NoTheme).unwrap_err();
//! assert_eq!(
//!     err.to_string(),
//!     "unknown Tailwind class `bg-gradient-to-r`; use `bg-linear-to-r` (Tailwind v4)"
//! );
//! ```

mod animation;
mod generated;
mod suffix;
#[cfg(test)]
mod tests;
pub mod theme;

use std::collections::BTreeSet;
use std::fmt;

pub use generated::{TAKUMI_CORE_VERSION, ThemeNamespace};
pub use theme::{Theme, ThemeError, ThemeErrorKind};

use generated::{BREAKPOINTS, EXACT_RENAMES, FIXED, HINTS, PREFIX_RENAMES, PREFIXES, VAR_TARGETS};
use suffix::{Negation, Parser};

/// The longest class the grammar accepts, in bytes.
const MAX_CLASS_LEN: usize = 256;
/// The longest theme token (or custom variant) the grammar accepts, in bytes.
const MAX_TOKEN_LEN: usize = 64;
/// The namespaces a `token/opacity` utility reads: opacity mixes colors only.
const COLOR_ONLY: &[ThemeNamespace] = &[ThemeNamespace::Color];

/// A recognized class: its variant, its `!important` flag and its utility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class<'a> {
    /// The breakpoint variant before the `:`, if any.
    pub variant: Option<Variant<'a>>,
    /// Whether the class carries a leading or trailing `!`.
    pub important: bool,
    /// What the utility reads.
    pub utility: Utility<'a>,
}

/// A breakpoint variant (`sm:flex`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Variant<'a> {
    /// A built-in breakpoint: `sm`, `md`, `lg`, `xl` or `2xl`.
    Builtin(&'static str),
    /// A breakpoint only a theme defines (`--breakpoint-<token>`).
    Custom(&'a str),
}

/// What a utility reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Utility<'a> {
    /// A value takumi knows without a theme.
    Builtin,
    /// A theme token: the utility draws only when one of `namespaces` defines
    /// `token` (`bg-brand` reads `--color-brand`).
    Theme {
        /// The namespaces the token is looked up in, in takumi's order.
        namespaces: &'static [ThemeNamespace],
        /// The token after the utility prefix.
        token: &'a str,
    },
    /// A theme animation: `animate-<token>` draws only when the theme
    /// defines `--animate-<token>`.
    Animation {
        /// The token after `animate-`.
        token: &'a str,
    },
    /// An arbitrary animation (`animate-[fade_1s_linear]`) that draws only
    /// when the theme defines `@keyframes <name>`.
    Keyframes {
        /// The keyframes name the animation plays.
        name: &'a str,
    },
}

/// The theme tokens a class list may read.
pub trait ThemeTokens {
    /// Whether the theme defines `token` in namespace `ns`
    /// (`contains(Color, "brand")` for `--color-brand`).
    fn contains(&self, ns: ThemeNamespace, token: &str) -> bool;

    /// Whether the theme defines the animation `--animate-<token>`; none by
    /// default.
    fn contains_animation(&self, token: &str) -> bool {
        let _ = token;
        false
    }

    /// Whether the theme defines `@keyframes <name>`; none by default.
    fn contains_keyframes(&self, name: &str) -> bool {
        let _ = name;
        false
    }
}

/// A theme that defines no token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoTheme;

impl ThemeTokens for NoTheme {
    fn contains(&self, _ns: ThemeNamespace, _token: &str) -> bool {
        false
    }
}

/// How to fix an unknown class.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Suggestion {
    /// The Tailwind v4 name of a v3 class, with the variant and `!` kept.
    Rename(String),
    /// What to write instead, for a v3 utility v4 dropped.
    Hint(&'static str),
}

/// One class [`validate`] rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownClass {
    class: String,
    suggestion: Option<Suggestion>,
}

impl UnknownClass {
    /// The rejected class, as written.
    pub fn class(&self) -> &str {
        &self.class
    }

    /// The fix [`suggest`] knows for it, if any.
    pub fn suggestion(&self) -> Option<&Suggestion> {
        self.suggestion.as_ref()
    }
}

impl fmt::Display for UnknownClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown Tailwind class `{}`", self.class)?;
        match &self.suggestion {
            Some(Suggestion::Rename(to)) => write!(f, "; use `{to}` (Tailwind v4)"),
            Some(Suggestion::Hint(hint)) => write!(f, "; {hint}"),
            None => Ok(()),
        }
    }
}

/// The classes of a list that are neither built in nor defined by the theme.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", render_lines(.classes))]
#[non_exhaustive]
pub struct InvalidClasses {
    classes: Vec<UnknownClass>,
}

impl InvalidClasses {
    /// The rejected classes, in first-seen order, without duplicates.
    pub fn classes(&self) -> &[UnknownClass] {
        &self.classes
    }
}

fn render_lines(classes: &[UnknownClass]) -> String {
    classes
        .iter()
        .map(UnknownClass::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Classifies one class, or returns `None` when the grammar does not know it.
///
/// The grammar follows takumi-core's own lookup (variant, `!`, fixed
/// utilities, negative prefix, longest known prefix first) and narrows it:
/// classes are ASCII and at most 256 bytes, keywords are lowercase, only one
/// variant is allowed, and a theme token is `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`
/// of at most 64 bytes and is never negated.
pub fn classify(class: &str) -> Option<Class<'_>> {
    if class.is_empty() || class.len() > MAX_CLASS_LEN || !class.is_ascii() {
        return None;
    }
    let (variant, mut token) = match split_variant(class) {
        Some((variant, rest)) => (Some(parse_variant(variant)?), rest),
        None => (None, class),
    };
    if split_variant(token).is_some() {
        return None;
    }
    let mut important = false;
    if let Some(stripped) = token.strip_prefix('!') {
        important = true;
        token = stripped;
    }
    if let Some(stripped) = token.strip_suffix('!') {
        important = true;
        token = stripped;
    }
    Some(Class {
        variant,
        important,
        utility: classify_utility(token)?,
    })
}

/// The Tailwind v4 replacement for a known v3 class, or a hint for a v3
/// utility v4 dropped. `None` when nothing is known about the class.
pub fn suggest(class: &str) -> Option<Suggestion> {
    let (variant, rest) = match split_variant(class) {
        Some((variant, rest)) => (Some(variant), rest),
        None => (None, class),
    };
    let (lead, rest) = rest.strip_prefix('!').map_or(("", rest), |r| ("!", r));
    let (core, trail) = rest.strip_suffix('!').map_or((rest, ""), |r| (r, "!"));
    let unsigned = core.strip_prefix('-').unwrap_or(core);
    let rebuild = |utility: &str| {
        let variant = variant.map(|v| format!("{v}:")).unwrap_or_default();
        format!("{variant}{lead}{utility}{trail}")
    };
    if let Some((_, to)) = EXACT_RENAMES.iter().find(|(from, _)| *from == core) {
        return Some(Suggestion::Rename(rebuild(to)));
    }
    for (from, to, suffixes) in PREFIX_RENAMES {
        if let Some(rest) = core.strip_prefix(from)
            && suffixes.contains(&rest)
        {
            return Some(Suggestion::Rename(rebuild(&format!("{to}{rest}"))));
        }
    }
    HINTS
        .iter()
        .find(|(prefix, _)| unsigned.starts_with(prefix))
        .map(|(_, hint)| Suggestion::Hint(hint))
}

/// Checks a class list the way takumi splits it (`str::split_whitespace`).
///
/// A class passes when it is built in, or when the theme defines every token
/// it reads: its utility token in at least one of its namespaces, and a
/// custom variant in [`ThemeNamespace::Breakpoint`].
///
/// # Errors
///
/// Returns [`InvalidClasses`] listing every other class once, in first-seen
/// order, each with its [`suggest`]ion.
pub fn validate(classes: &str, theme: &dyn ThemeTokens) -> Result<(), InvalidClasses> {
    let mut seen = BTreeSet::new();
    let unknown: Vec<UnknownClass> = classes
        .split_whitespace()
        .filter(|class| seen.insert(*class))
        .filter(|class| !is_satisfied(class, theme))
        .map(|class| UnknownClass {
            class: class.to_owned(),
            suggestion: suggest(class),
        })
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(InvalidClasses { classes: unknown })
    }
}

fn is_satisfied(class: &str, theme: &dyn ThemeTokens) -> bool {
    let Some(class) = classify(class) else {
        return false;
    };
    let variant_ok = match class.variant {
        Some(Variant::Custom(token)) => theme.contains(ThemeNamespace::Breakpoint, token),
        _ => true,
    };
    let utility_ok = match class.utility {
        Utility::Theme { namespaces, token } => {
            namespaces.iter().any(|ns| theme.contains(*ns, token))
        }
        Utility::Animation { token } => theme.contains_animation(token),
        Utility::Keyframes { name } => theme.contains_keyframes(name),
        _ => true,
    };
    variant_ok && utility_ok
}

/// takumi-core's `split_variant`: splits at the first top-level `:`; a `:`
/// inside brackets, quotes or after `\` is not a separator.
fn split_variant(token: &str) -> Option<(&str, &str)> {
    let bytes = token.as_bytes();
    let mut stack: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            quote @ (b'\'' | b'"') => {
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' => stack.push(b')'),
            b'[' => stack.push(b']'),
            b'{' => stack.push(b'}'),
            closing @ (b')' | b']' | b'}') => {
                if stack.last() == Some(&closing) {
                    stack.pop();
                }
            }
            b':' if stack.is_empty() => {
                return Some((token.get(..index)?, token.get(index + 1..)?));
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// A built-in breakpoint (lowercase only) or a theme-defined one.
fn parse_variant(token: &str) -> Option<Variant<'_>> {
    if let Some(builtin) = BREAKPOINTS.iter().find(|b| **b == token) {
        return Some(Variant::Builtin(builtin));
    }
    // takumi accepts identifier bytes only, so a custom variant has no `.`.
    (is_theme_token(token) && !token.contains('.')).then_some(Variant::Custom(token))
}

/// takumi-core's `TailwindProperty::parse` and `parse_prefix_suffix`, narrowed.
fn classify_utility(token: &str) -> Option<Utility<'_>> {
    if FIXED.binary_search(&token).is_ok() {
        return Some(Utility::Builtin);
    }
    let (body, negative) = match token.strip_prefix('-') {
        Some(stripped) => (stripped, true),
        None => (token, false),
    };
    for (dash, _) in body.match_indices('-').rev() {
        let (Some(prefix), Some(suffix)) = (body.get(..dash), body.get(dash + 1..)) else {
            continue;
        };
        let Some(parsers) = lookup(PREFIXES, prefix) else {
            continue;
        };
        if let Some(matched) = match_parsers(parsers, suffix) {
            return (!negative || matched).then_some(Utility::Builtin);
        }
        if prefix == "animate" && !negative {
            // takumi reads any other suffix as an animation and stops here.
            return animation_utility(suffix);
        }
        if let Some(namespaces) = lookup(VAR_TARGETS, prefix)
            && takumi_reads_var(namespaces, suffix)
        {
            // takumi stops at this prefix, so a shorter one is never tried.
            return theme_utility(namespaces, suffix).filter(|_| !negative);
        }
    }
    None
}

/// An `animate-` suffix no built-in keyword matched: an arbitrary single
/// animation or a theme animation token.
fn animation_utility(suffix: &str) -> Option<Utility<'_>> {
    if let Some(inner) = suffix.strip_prefix('[') {
        let name = animation::parse(inner.strip_suffix(']')?, animation::Sep::Underscore)?;
        return Some(if animation::BUILTIN.contains(&name) {
            Utility::Builtin
        } else {
            Utility::Keyframes { name }
        });
    }
    animation::is_animation_token(suffix).then_some(Utility::Animation { token: suffix })
}

/// The first parser whose value grammar accepts `suffix`, as whether the match
/// can be negated; `None` when no parser accepts it.
fn match_parsers(parsers: &[Parser], suffix: &str) -> Option<bool> {
    parsers.iter().find_map(|parser| {
        let rule = parser.value.iter().find(|rule| rule.matches(suffix))?;
        Some(parser.negatable && rule.negation == Negation::Allowed)
    })
}

/// Whether takumi-core's `var_expression` yields a variable for `suffix` in
/// at least one namespace group, which ends its prefix search.
fn takumi_reads_var(namespaces: &[ThemeNamespace], suffix: &str) -> bool {
    if suffix.starts_with('[') {
        return false;
    }
    match suffix.split_once('/') {
        Some((_, opacity)) => {
            namespaces.contains(&ThemeNamespace::Color) && parse_opacity_modifier(opacity)
        }
        None => !namespaces.is_empty(),
    }
}

/// takumi-core's `var_expression` over a `VAR_TARGETS` entry, narrowed.
fn theme_utility<'a>(
    namespaces: &'static [ThemeNamespace],
    suffix: &'a str,
) -> Option<Utility<'a>> {
    if suffix.starts_with('[') {
        return None;
    }
    if let Some((token, opacity)) = suffix.split_once('/') {
        let opacity_ok = parse_opacity_modifier(opacity) && suffix::is_opacity(opacity);
        return (namespaces.contains(&ThemeNamespace::Color)
            && opacity_ok
            && is_theme_token(token))
        .then_some(Utility::Theme {
            namespaces: COLOR_ONLY,
            token,
        });
    }
    // A number is a spacing step or a value, never a token name.
    if suffix.parse::<f32>().is_ok() || !is_theme_token(suffix) {
        return None;
    }
    Some(Utility::Theme {
        namespaces,
        token: suffix,
    })
}

/// takumi-core's `parse_opacity_modifier`: a number within `0..=100`.
fn parse_opacity_modifier(modifier: &str) -> bool {
    modifier
        .parse::<f32>()
        .is_ok_and(|percentage| (0.0..=100.0).contains(&percentage))
}

/// `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`, at most 64 bytes.
pub(crate) fn is_theme_token(token: &str) -> bool {
    let edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let bytes = token.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(&first), Some(&last)) => {
            bytes.len() <= MAX_TOKEN_LEN
                && edge(first)
                && edge(last)
                && bytes
                    .iter()
                    .all(|&b| edge(b) || matches!(b, b'.' | b'_' | b'-'))
        }
        _ => false,
    }
}

fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .and_then(|index| table.get(index))
        .map(|&(_, value)| value)
}

/// Sample classes for the takumi cross-check (`tests/tw_crosscheck.rs`).
#[doc(hidden)]
pub mod __crosscheck {
    use super::ThemeNamespace;
    use super::generated::{FIXED, PREFIXES, VAR_TARGETS};
    use super::suffix::Negation;

    /// Every class the cross-check compares with takumi: each fixed utility,
    /// each (prefix, parser, rule) sample, a negative sample per negatable
    /// rule of a negatable parser, variant and `!` forms, a theme token per
    /// `VAR_TARGETS` prefix and a custom variant.
    pub fn samples() -> Vec<String> {
        let mut out: Vec<String> = FIXED.iter().map(|key| (*key).to_owned()).collect();
        for (prefix, parsers) in PREFIXES {
            let mut first = None;
            for parser in *parsers {
                for rule in parser.value {
                    let samples = rule.samples();
                    if parser.negatable
                        && rule.negation == Negation::Allowed
                        && let Some(sample) = samples.first()
                    {
                        out.push(format!("-{prefix}-{sample}"));
                    }
                    for sample in samples {
                        let class = format!("{prefix}-{sample}");
                        first.get_or_insert_with(|| class.clone());
                        out.push(class);
                    }
                }
            }
            if let Some(first) = first {
                out.extend([
                    format!("sm:{first}"),
                    format!("2xl:{first}"),
                    format!("!{first}"),
                    format!("{first}!"),
                ]);
            }
        }
        for (prefix, namespaces) in VAR_TARGETS {
            out.push(format!("{prefix}-noctis-tok"));
            if namespaces.first() == Some(&ThemeNamespace::Color) {
                out.push(format!("{prefix}-noctis-tok/50"));
            }
        }
        out.extend([
            "animate-noctis-tok".to_owned(),
            "animate-[noctis-tok_1s_linear_infinite]".to_owned(),
            "animate-[spin_1s]".to_owned(),
        ]);
        out.push("brand:flex".to_owned());
        let mut seen = std::collections::BTreeSet::new();
        out.retain(|class| seen.insert(class.clone()));
        out
    }
}
