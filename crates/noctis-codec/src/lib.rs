//! `custom_id` layout, `Route`/`Payload`/`Field` traits, `EncodedId`, `RefRecord` and `BoundedStr`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! # Payload fields
//!
//! A Payload field implements [`Field`] (encoding and worst-case width
//! `MAX_LEN`) and [`FieldDecode`] (decoding from a `&str` slice). Integers are
//! minimal-digit base62 (`0-9A-Za-z`), signed integers zig-zag first; `bool`
//! is `0`/`1`; `Option<T>` is `0` or `1` followed by `T`; Discord ids are
//! their `u64`.
//!
//! String-like fields (`BoundedStr<N>`, `String`, `Cow<str>`, `Vec<T>`) escape:
//!
//! | Character | Encoding |
//! |---|---|
//! | `!` | `!0` |
//! | `,` | `!1` |
//! | `:` | `!2` |
//! | `~` | `!3` |
//! | other printable ASCII (`0x20..=0x7E`) | itself |
//! | anything else (`String`/`Cow` only) | `!u` + 6 lowercase hex digits |
//!
//! Every encoding is printable ASCII without `,`, `:` or `~` (CID-07).
//! Decoding is canonical: when `decode(s)` returns `Ok(v)`, `encode(v)` is
//! exactly `s`, so leading zeros, signs, unnecessary escapes and over-long
//! inputs are [`FieldError`]s; decoding never panics.

mod base62;
mod bounded_str;
pub mod check;
mod error;
mod escape;
mod field;

pub use bounded_str::{BoundedStr, BoundedStrError};
pub use error::FieldError;
pub use field::{Field, FieldDecode};
