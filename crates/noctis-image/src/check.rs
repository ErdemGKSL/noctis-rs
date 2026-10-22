//! The runtime half of TW-01: [`ClassCheck`] validates the unchecked
//! ([`Classes::dynamic`](crate::Classes::dynamic)) class strings of a view
//! tree.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};

use crate::Node;
use crate::tw::{InvalidClasses, Theme, validate};

/// How many distinct invalid strings a Warn check remembers.
const MAX_REMEMBERED: usize = 1024;
/// The longest class string a warning logs, in bytes.
const MAX_LOGGED_BYTES: usize = 512;

/// What a [`ClassCheck`] does with an invalid runtime class string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClassCheckMode {
    /// Log a warning once per distinct string, then render.
    Warn,
    /// Fail: the mode of test Apps.
    Deny,
}

/// Validates the runtime class strings of a view tree (TW-01).
///
/// Literal classes (`tw!`, `tw = "…"`) were validated against the crate's own
/// theme at compile time and are trusted; only [`Classes::dynamic`] strings
/// are checked.
///
/// [`Classes::dynamic`]: crate::Classes::dynamic
///
/// ```
/// use noctis_image::{ClassCheck, ClassCheckMode, Classes, div};
/// use noctis_image::tw::Theme;
///
/// let tree = div(Classes::dynamic("flx"));
/// let deny = ClassCheck::new(ClassCheckMode::Deny);
/// assert!(deny.check(&tree, &Theme::empty()).is_err());
/// let warn = ClassCheck::new(ClassCheckMode::Warn);
/// assert_eq!(warn.check(&tree, &Theme::empty()), Ok(1));
/// assert_eq!(warn.check(&tree, &Theme::empty()), Ok(0)); // warned once
/// ```
#[derive(Debug)]
pub struct ClassCheck {
    mode: ClassCheckMode,
    warned: Mutex<Warned>,
}

/// The blake3 digests of the strings already warned about.
#[derive(Debug, Default)]
struct Warned {
    seen: HashSet<[u8; 32]>,
    full: bool,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ClassCheck>();
};

impl ClassCheck {
    /// A check in `mode`.
    pub fn new(mode: ClassCheckMode) -> Self {
        Self {
            mode,
            warned: Mutex::new(Warned::default()),
        }
    }

    /// Validates every unchecked class string of `node`, pre-order, against
    /// `theme`.
    ///
    /// In [`ClassCheckMode::Warn`] each invalid string not seen before logs one
    /// `tracing` warning (target `noctis::image`, the string cut at 512 bytes)
    /// and the call returns how many strings it newly warned about. At most
    /// 1024 distinct strings are remembered; once full, one more warning says
    /// that further strings are not logged.
    ///
    /// # Errors
    ///
    /// In [`ClassCheckMode::Deny`], the [`InvalidClasses`] of the first invalid
    /// string.
    pub fn check(&self, node: &Node, theme: &Theme) -> Result<usize, InvalidClasses> {
        let mut warned = 0;
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            stack.extend(node.child_nodes().iter().rev());
            let classes = node.class_list();
            if classes.is_checked() {
                continue;
            }
            let Err(error) = validate(classes.as_str(), theme) else {
                continue;
            };
            match self.mode {
                ClassCheckMode::Deny => return Err(error),
                ClassCheckMode::Warn => {
                    if self.warn_once(classes.as_str(), &error) {
                        warned += 1;
                    }
                }
            }
        }
        Ok(warned)
    }

    /// Logs `error` for `classes` unless it was logged before; whether it
    /// logged it.
    fn warn_once(&self, classes: &str, error: &InvalidClasses) -> bool {
        let digest = *blake3::hash(classes.as_bytes()).as_bytes();
        let mut warned = self.warned.lock().unwrap_or_else(PoisonError::into_inner);
        if warned.seen.contains(&digest) {
            return false;
        }
        if warned.seen.len() >= MAX_REMEMBERED {
            if !warned.full {
                warned.full = true;
                tracing::warn!(
                    target: "noctis::image",
                    limit = MAX_REMEMBERED,
                    "unknown Tailwind classes in more runtime class strings than the limit; further strings are not logged"
                );
            }
            return false;
        }
        warned.seen.insert(digest);
        drop(warned);
        tracing::warn!(
            target: "noctis::image",
            classes = %log_excerpt(classes),
            error = %error,
            "unknown Tailwind class in a runtime class string"
        );
        true
    }
}

/// `classes` cut to at most 512 bytes at a char boundary, plus `…` when cut.
pub(crate) fn log_excerpt(classes: &str) -> Cow<'_, str> {
    if classes.len() <= MAX_LOGGED_BYTES {
        return Cow::Borrowed(classes);
    }
    let cut = (0..=MAX_LOGGED_BYTES)
        .rev()
        .find(|&index| classes.is_char_boundary(index))
        .unwrap_or(0);
    Cow::Owned(format!("{}…", &classes[..cut]))
}

#[cfg(test)]
mod tests;
