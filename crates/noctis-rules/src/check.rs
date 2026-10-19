//! Small helpers shared by the rule modules: push a violation when a count or
//! length is out of range, and join violation paths.

use crate::len::discord_len;
use crate::{Actual, Bound, RuleId, Violation};

/// `parent.field`, or `field` when `parent` is empty.
pub(crate) fn join(parent: &str, field: &str) -> String {
    if parent.is_empty() {
        field.to_owned()
    } else {
        format!("{parent}.{field}")
    }
}

/// Pushes `rule` with `AtMost(max)` when `count > max`.
pub(crate) fn at_most(
    rule: RuleId,
    path: impl Into<String>,
    max: usize,
    count: usize,
    out: &mut Vec<Violation>,
) {
    if count > max {
        out.push(Violation::new(
            rule,
            path,
            Bound::AtMost(max),
            Actual::Count(count),
        ));
    }
}

/// Pushes `rule` with `Between { min, max }` when `count` is outside `min..=max`.
pub(crate) fn between(
    rule: RuleId,
    path: impl Into<String>,
    min: usize,
    max: usize,
    count: usize,
    out: &mut Vec<Violation>,
) {
    if count < min || count > max {
        out.push(Violation::new(
            rule,
            path,
            Bound::Between { min, max },
            Actual::Count(count),
        ));
    }
}

/// [`at_most`] on the [`discord_len`] of `text`.
pub(crate) fn text_at_most(
    rule: RuleId,
    path: impl Into<String>,
    max: usize,
    text: &str,
    out: &mut Vec<Violation>,
) {
    at_most(rule, path, max, discord_len(text), out);
}

/// [`between`] on the [`discord_len`] of `text`.
pub(crate) fn text_between(
    rule: RuleId,
    path: impl Into<String>,
    min: usize,
    max: usize,
    text: &str,
    out: &mut Vec<Violation>,
) {
    between(rule, path, min, max, discord_len(text), out);
}

/// `Ok(())` for no violations, else `Err(out)` (never an empty `Err`).
pub(crate) fn finish(out: Vec<Violation>) -> Result<(), Vec<Violation>> {
    if out.is_empty() { Ok(()) } else { Err(out) }
}
