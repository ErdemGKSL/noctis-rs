//! The single-animation grammar of `animate-[…]` classes and `--animate-*`
//! Theme values (FR-69), takumi-free.
//!
//! takumi accepts the whole CSS `animation` shorthand, including comma
//! separated lists, timing functions and `var()`. This grammar is a strict
//! subset, so that every accepted value names exactly one `@keyframes` the
//! Theme or takumi defines:
//!
//! `<keyframes> <duration>[ <delay>][ <timing>][ <count>|infinite][ <direction>][ <fill>][ <play-state>]`,
//! in any order, with keywords only.

/// The built-in animations takumi ships keyframes for; a Theme must not
/// redefine them.
pub(crate) const BUILTIN: [&str; 4] = ["spin", "ping", "pulse", "bounce"];

/// The `animation-timing-function` keywords.
const TIMING: &[&str] = &[
    "linear",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "step-start",
    "step-end",
];
/// The `animation-direction` keywords.
const DIRECTION: &[&str] = &["normal", "reverse", "alternate", "alternate-reverse"];
/// The `animation-fill-mode` keywords.
const FILL: &[&str] = &["none", "forwards", "backwards", "both"];
/// The `animation-play-state` keywords.
const PLAY_STATE: &[&str] = &["running", "paused"];
/// Every keyword of the grammar plus the CSS-wide keywords: never a
/// keyframes name.
const RESERVED: &[&str] = &[
    "linear",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "step-start",
    "step-end",
    "infinite",
    "normal",
    "reverse",
    "alternate",
    "alternate-reverse",
    "none",
    "forwards",
    "backwards",
    "both",
    "running",
    "paused",
    "initial",
    "inherit",
    "unset",
    "revert",
    "revert-layer",
    "default",
];

/// The longest keyframes name, in bytes.
const MAX_NAME_LEN: usize = 64;

/// How the parts of an animation are separated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sep {
    /// `_`, as in an `animate-[…]` class; an empty part is an error.
    Underscore,
    /// Runs of ASCII whitespace, as in a Theme value.
    Whitespace,
}

/// The keyframes name of the single animation `value`, or `None` when
/// `value` is not one animation of this grammar.
pub(crate) fn parse(value: &str, sep: Sep) -> Option<&str> {
    let parts: Vec<&str> = match sep {
        Sep::Underscore => {
            let parts: Vec<&str> = value.split('_').collect();
            if parts.iter().any(|part| part.is_empty()) {
                return None;
            }
            parts
        }
        Sep::Whitespace => value.split_ascii_whitespace().collect(),
    };
    let mut name = None;
    let mut times = 0_u8;
    let mut seen = [false; 5];
    for part in parts {
        if let Some(zero) = time(part) {
            // The first time is the duration and must be positive.
            if times == 0 && zero {
                return None;
            }
            times += 1;
            if times > 2 {
                return None;
            }
            continue;
        }
        let category = if TIMING.contains(&part) {
            0
        } else if is_count(part) {
            1
        } else if DIRECTION.contains(&part) {
            2
        } else if FILL.contains(&part) {
            3
        } else if PLAY_STATE.contains(&part) {
            4
        } else if is_keyframes_name(part) && name.is_none() {
            name = Some(part);
            continue;
        } else {
            return None;
        };
        if std::mem::replace(&mut seen[category], true) {
            return None;
        }
    }
    if times == 0 {
        return None;
    }
    name
}

/// `(0|[1-9][0-9]{0,5})(\.[0-9]{1,3})?(ms|s)`: `Some(is_zero)` for a time.
fn time(part: &str) -> Option<bool> {
    let number = part.strip_suffix("ms").or_else(|| part.strip_suffix('s'))?;
    let (whole, fraction) = match number.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (number, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let whole_ok = digits(whole) && whole.len() <= 6 && (whole == "0" || !whole.starts_with('0'));
    let fraction_ok = fraction.is_none_or(|f| digits(f) && f.len() <= 3);
    (whole_ok && fraction_ok)
        .then(|| whole == "0" && fraction.is_none_or(|f| f.bytes().all(|b| b == b'0')))
}

/// `infinite` or `[1-9][0-9]{0,3}`.
fn is_count(part: &str) -> bool {
    part == "infinite"
        || (!part.is_empty()
            && part.len() <= 4
            && !part.starts_with('0')
            && part.bytes().all(|b| b.is_ascii_digit()))
}

/// `[a-z]([a-z0-9-]*[a-z0-9])?`, at most 64 bytes, and not a keyword.
pub(crate) fn is_keyframes_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    bytes.len() <= MAX_NAME_LEN
        && first.is_ascii_lowercase()
        && (last.is_ascii_lowercase() || last.is_ascii_digit())
        && bytes
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !RESERVED.contains(&name)
}

/// The token of an `animate-<token>` class or a `--animate-<token>`
/// property: a theme token with no `.` that is not a number.
pub(crate) fn is_animation_token(token: &str) -> bool {
    super::is_theme_token(token) && !token.contains('.') && token.parse::<f32>().is_err()
}

#[cfg(test)]
mod tests;
