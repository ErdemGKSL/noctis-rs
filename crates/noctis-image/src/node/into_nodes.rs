//! [`IntoNodes`]: what a child position accepts.

use std::borrow::Cow;

use super::Node;

pub(crate) mod sealed {
    use super::Node;

    /// The conversion behind [`IntoNodes`](super::IntoNodes); private, so
    /// the set of child types is closed.
    pub trait Sealed {
        /// Appends the nodes `self` yields to `out`.
        fn push_into(self, out: &mut Vec<Node>);
    }
}

use sealed::Sealed;

/// A value that yields 0..n child nodes (sealed).
///
/// - a [`Node`] (one), or a `Vec<Node>` (each);
/// - `&str`, `String`, `&String` and `Cow<'static, str>`: one text node,
///   drawn as is and never parsed;
/// - the integer primitives, `f32` and `f64`: one text node of their
///   `Display` text (`42u8` → `"42"`, `1.5f32` → `"1.5"`);
/// - `Option<T>` of these: nothing for `None`.
///
/// No impl exists for `Classes`, and nothing turns a string into markup.
pub trait IntoNodes: Sealed {}

impl<T: Sealed> IntoNodes for T {}

impl Sealed for Node {
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(self);
    }
}

impl Sealed for Vec<Node> {
    fn push_into(self, out: &mut Vec<Node>) {
        out.extend(self);
    }
}

impl Sealed for &str {
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(Node::text(self.to_owned()));
    }
}

impl Sealed for String {
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(Node::text(self));
    }
}

impl Sealed for &String {
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(Node::text(self.clone()));
    }
}

impl Sealed for Cow<'static, str> {
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(Node::text(self));
    }
}

impl<T: Sealed> Sealed for Option<T> {
    fn push_into(self, out: &mut Vec<Node>) {
        if let Some(value) = self {
            value.push_into(out);
        }
    }
}

macro_rules! display_text {
    ($($ty:ty),+ $(,)?) => {$(
        impl Sealed for $ty {
            fn push_into(self, out: &mut Vec<Node>) {
                out.push(Node::text(self.to_string()));
            }
        }
    )+};
}

display_text!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);
