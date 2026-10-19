//! The `Field`/`FieldDecode` traits and the built-in field encodings.

use core::fmt::{self, Write};
use std::borrow::Cow;

use twilight_model::id::Id;

use crate::FieldError;
use crate::base62::{self, U64_WIDTH};
use crate::escape::{EscapeWriter, escape, unescape};

/// A Payload field: a value with a `custom_id`-safe encoding and a known
/// worst-case width.
///
/// # Implementor contract
///
/// Every encoding `encode` writes must be ASCII, printable (`0x20..=0x7E`),
/// never contain `,`, `:` or `~`, and be at most `MAX_LEN` characters when
/// `MAX_LEN` is `Some` (CID-07). [`FieldDecode::decode`] must be total and
/// canonical: it never panics, and when it returns `Ok(v)`, `encode(v)` writes
/// exactly the input. Test both with [`crate::check::assert_round_trip`] and
/// [`crate::check::assert_worst_case`].
///
/// The easiest correct impl delegates to a built-in field:
///
/// ```
/// use noctis_codec::{Field, FieldDecode, FieldError, check};
///
/// #[derive(Debug, Clone, Copy, PartialEq)]
/// struct Percent(u8);
///
/// impl Field for Percent {
///     const MAX_LEN: Option<usize> = <u8 as Field>::MAX_LEN;
///     fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result {
///         self.0.encode(out)
///     }
/// }
///
/// impl<'de> FieldDecode<'de> for Percent {
///     fn decode(raw: &'de str) -> Result<Self, FieldError> {
///         let v = u8::decode(raw)?;
///         if v <= 100 { Ok(Self(v)) } else { Err(FieldError::Overflow) }
///     }
/// }
///
/// check::assert_round_trip(&Percent(42));
/// assert_eq!(Percent::decode("1c"), Ok(Percent(100)));
/// assert_eq!(Percent::decode("1d"), Err(FieldError::Overflow)); // 101
/// ```
pub trait Field {
    /// Worst-case encoded width in characters, or `None` when unbounded.
    const MAX_LEN: Option<usize>;

    /// Writes the encoding of `self` to `out`.
    ///
    /// # Errors
    ///
    /// Returns the error of `out` (for example a full fixed-capacity buffer).
    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result;
}

/// Decoding of a [`Field`] from a `custom_id` slice.
///
/// The `'de` lifetime lets borrowed fields such as `Cow<'de, str>` point into
/// the input; owned fields implement it for every `'de`.
pub trait FieldDecode<'de>: Field + Sized {
    /// Decodes a value from exactly `raw`.
    ///
    /// # Errors
    ///
    /// Returns a [`FieldError`] for every input that is not the canonical
    /// encoding of a value.
    fn decode(raw: &'de str) -> Result<Self, FieldError>;

    /// The value of this field when it is missing from the end of an older
    /// `custom_id` (FR-60).
    ///
    /// Appending a field to a Payload is compatible only when the new field
    /// has an absent value: an id encoded before the field existed then
    /// decodes with it. Only `Option<T>` overrides this (to `Some(None)`), so
    /// appended `Option` fields decode as `None` without a schema bump; every
    /// other field returns `None` here, and a missing field stays an error.
    fn absent() -> Option<Self> {
        None
    }
}

impl Field for bool {
    const MAX_LEN: Option<usize> = Some(1);

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        out.write_str(if *self { "1" } else { "0" })
    }
}

impl FieldDecode<'_> for bool {
    fn decode(raw: &str) -> Result<Self, FieldError> {
        match raw.as_bytes() {
            [] => Err(FieldError::Empty),
            [b'0'] => Ok(false),
            [b'1'] => Ok(true),
            [_] => Err(FieldError::InvalidChar { index: 0 }),
            long => Err(FieldError::TooLong {
                max: 1,
                actual: long.len(),
            }),
        }
    }
}

macro_rules! unsigned_fields {
    ($($t:ty => $width:literal),* $(,)?) => {$(
        impl Field for $t {
            const MAX_LEN: Option<usize> = Some($width);

            fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
                let mut buf = [0; U64_WIDTH];
                out.write_str(base62::encode_u64(u64::from(*self), &mut buf))
            }
        }

        impl FieldDecode<'_> for $t {
            fn decode(raw: &str) -> Result<Self, FieldError> {
                let v = base62::decode_u64(raw, $width)?;
                <$t>::try_from(v).map_err(|_| FieldError::Overflow)
            }
        }
    )*};
}

unsigned_fields!(u8 => 2, u16 => 3, u32 => 6, u64 => 11);

macro_rules! signed_fields {
    ($($t:ty => $u:ty),* $(,)?) => {$(
        impl Field for $t {
            const MAX_LEN: Option<usize> = <$u as Field>::MAX_LEN;

            fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
                // Zig-zag: 0, -1, 1, -2, … map to 0, 1, 2, 3, …
                let n = *self;
                ((n << 1) ^ (n >> (<$t>::BITS - 1))).cast_unsigned().encode(out)
            }
        }

        impl FieldDecode<'_> for $t {
            fn decode(raw: &str) -> Result<Self, FieldError> {
                let u = <$u>::decode(raw)?;
                Ok((u >> 1).cast_signed() ^ -((u & 1).cast_signed()))
            }
        }
    )*};
}

signed_fields!(i8 => u8, i16 => u16, i32 => u32, i64 => u64);

impl<T> Field for Id<T> {
    const MAX_LEN: Option<usize> = <u64 as Field>::MAX_LEN;

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        self.get().encode(out)
    }
}

impl<T> FieldDecode<'_> for Id<T> {
    fn decode(raw: &str) -> Result<Self, FieldError> {
        Id::new_checked(u64::decode(raw)?).ok_or(FieldError::ZeroId)
    }
}

impl<T: Field> Field for Option<T> {
    const MAX_LEN: Option<usize> = match T::MAX_LEN {
        Some(n) => n.checked_add(1),
        None => None,
    };

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        match self {
            None => out.write_char('0'),
            Some(v) => {
                out.write_char('1')?;
                v.encode(out)
            }
        }
    }
}

impl<'de, T: FieldDecode<'de>> FieldDecode<'de> for Option<T> {
    fn decode(raw: &'de str) -> Result<Self, FieldError> {
        match raw.as_bytes().first() {
            None => Err(FieldError::Empty),
            Some(b'0') if raw.len() == 1 => Ok(None),
            Some(b'0') => Err(FieldError::TrailingInput { index: 1 }),
            // The tag is one ASCII byte, so `1..` is a char boundary.
            Some(b'1') => T::decode(&raw[1..]).map(Some).map_err(|e| e.offset(1)),
            Some(_) => Err(FieldError::InvalidChar { index: 0 }),
        }
    }

    fn absent() -> Option<Self> {
        Some(None)
    }
}

impl Field for String {
    const MAX_LEN: Option<usize> = None;

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        escape(self, out)
    }
}

impl FieldDecode<'_> for String {
    fn decode(raw: &str) -> Result<Self, FieldError> {
        unescape(raw, true).map(Cow::into_owned)
    }
}

impl Field for Cow<'_, str> {
    const MAX_LEN: Option<usize> = None;

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        escape(self, out)
    }
}

impl<'de> FieldDecode<'de> for Cow<'de, str> {
    fn decode(raw: &'de str) -> Result<Self, FieldError> {
        unescape(raw, true)
    }
}

impl<T: Field> Field for Vec<T> {
    const MAX_LEN: Option<usize> = None;

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        let mut out = EscapeWriter { inner: out };
        self.iter().try_for_each(|e| {
            e.encode(&mut out)?;
            out.write_char(',')
        })
    }
}

/// List elements must be owned: `T` implements `FieldDecode` for every `'de`,
/// so `Vec<Cow<'_, str>>` or another borrowing element type does not decode.
/// For an element's `InvalidChar`, `BadEscape` or `TrailingInput`, the index is
/// relative to that element after the list is unescaped.
impl<T: for<'x> FieldDecode<'x>> FieldDecode<'_> for Vec<T> {
    fn decode(raw: &str) -> Result<Self, FieldError> {
        let list = unescape(raw, true)?;
        if list.is_empty() {
            return Ok(Vec::new());
        }
        let body = list.strip_suffix(',').ok_or(FieldError::BadList)?;
        body.split(',').map(T::decode).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use twilight_model::id::Id;
    use twilight_model::id::marker::GenericMarker;

    use super::{Field, FieldDecode};
    use crate::FieldError;
    use crate::check::worst_case;

    fn enc<T: Field>(v: &T) -> String {
        let mut out = String::new();
        v.encode(&mut out).unwrap();
        out
    }

    const fn assert_copy<T: Copy>() {}
    const _: () = {
        assert_copy::<bool>();
        assert_copy::<u8>();
        assert_copy::<i64>();
        assert_copy::<Id<GenericMarker>>();
        assert_copy::<Option<Option<u32>>>();
    };

    #[test]
    fn only_option_has_an_absent_value() {
        assert_eq!(<Option<u8>>::absent(), Some(None));
        assert_eq!(<Option<Option<u8>>>::absent(), Some(None));
        assert_eq!(<u8>::absent(), None);
        assert_eq!(<String>::absent(), None);
        assert_eq!(<bool>::absent(), None);
    }

    #[test]
    fn cid_01_widths_match_ad_6() {
        assert_eq!(<bool as Field>::MAX_LEN, Some(1));
        assert_eq!(<u8 as Field>::MAX_LEN, Some(2));
        assert_eq!(<i8 as Field>::MAX_LEN, Some(2));
        assert_eq!(<u16 as Field>::MAX_LEN, Some(3));
        assert_eq!(<i16 as Field>::MAX_LEN, Some(3));
        assert_eq!(<u32 as Field>::MAX_LEN, Some(6));
        assert_eq!(<i32 as Field>::MAX_LEN, Some(6));
        assert_eq!(<u64 as Field>::MAX_LEN, Some(11));
        assert_eq!(<i64 as Field>::MAX_LEN, Some(11));
        assert_eq!(<Id<GenericMarker> as Field>::MAX_LEN, Some(11));
        assert_eq!(<Option<u8> as Field>::MAX_LEN, Some(3));
        assert_eq!(<Option<String> as Field>::MAX_LEN, None);
        assert_eq!(<String as Field>::MAX_LEN, None);
        assert_eq!(<Cow<'_, str> as Field>::MAX_LEN, None);
        assert_eq!(<Vec<u8> as Field>::MAX_LEN, None);
    }

    #[test]
    fn cid_01_bool_worst_case_width() {
        worst_case(&true).unwrap();
        worst_case(&false).unwrap();
    }

    #[test]
    fn cid_01_unsigned_worst_case_width() {
        worst_case(&u8::MAX).unwrap();
        worst_case(&u16::MAX).unwrap();
        worst_case(&u32::MAX).unwrap();
        worst_case(&u64::MAX).unwrap();
    }

    #[test]
    fn cid_01_signed_worst_case_width() {
        worst_case(&i8::MIN).unwrap();
        worst_case(&i16::MIN).unwrap();
        worst_case(&i32::MIN).unwrap();
        worst_case(&i64::MIN).unwrap();
    }

    #[test]
    fn cid_01_id_and_option_worst_case_width() {
        worst_case(&Id::<GenericMarker>::new(u64::MAX)).unwrap();
        worst_case(&Some(255u8)).unwrap();
        worst_case(&Some(Some(i64::MIN))).unwrap();
    }

    #[test]
    fn encode_matrix() {
        assert_eq!(enc(&false), "0");
        assert_eq!(enc(&true), "1");
        assert_eq!(enc(&0u8), "0");
        assert_eq!(enc(&61u8), "z");
        assert_eq!(enc(&62u8), "10");
        assert_eq!(enc(&255u8), "47");
        assert_eq!(enc(&0i8), "0");
        assert_eq!(enc(&-1i8), "1");
        assert_eq!(enc(&1i8), "2");
        assert_eq!(enc(&127i8), "46");
        assert_eq!(enc(&-128i8), "47");
        assert_eq!(enc(&65535u16), "H31");
        assert_eq!(enc(&i16::MIN), "H31");
        assert_eq!(enc(&u32::MAX), "4gfFC3");
        assert_eq!(enc(&i32::MIN), "4gfFC3");
        assert_eq!(enc(&u64::MAX), "LygHa16AHYF");
        assert_eq!(enc(&i64::MIN), "LygHa16AHYF");
        assert_eq!(enc(&i64::MAX), "LygHa16AHYE");
        assert_eq!(enc(&Id::<GenericMarker>::new(u64::MAX)), "LygHa16AHYF");
        assert_eq!(enc(&Id::<GenericMarker>::new(1)), "1");
        assert_eq!(enc(&None::<u8>), "0");
        assert_eq!(enc(&Some(255u8)), "147");
        assert_eq!(enc(&Some(None::<u8>)), "10");
        assert_eq!(enc(&Some(Some(0u8))), "110");
        assert_eq!(enc(&"é😀".to_owned()), "!u0000e9!u01f600");
        assert_eq!(enc(&"\n".to_owned()), "!u00000a");
        assert_eq!(enc(&" ".to_owned()), " ");
        assert_eq!(enc(&Cow::Borrowed("a,b")), "a!1b");
        assert_eq!(enc(&Vec::<u8>::new()), "");
        assert_eq!(enc(&vec![1u8, 2]), "1!12!1");
        assert_eq!(enc(&vec![String::new()]), "!1");
        assert_eq!(enc(&vec![vec![1u8]]), "1!01!1");
    }

    #[test]
    fn decode_integers() {
        assert_eq!(u8::decode(""), Err(FieldError::Empty));
        assert_eq!(u8::decode("0"), Ok(0));
        assert_eq!(u8::decode("47"), Ok(255));
        assert_eq!(u8::decode("48"), Err(FieldError::Overflow));
        assert_eq!(u8::decode("zz"), Err(FieldError::Overflow));
        assert_eq!(u16::decode("H32"), Err(FieldError::Overflow));
        assert_eq!(u64::decode("LygHa16AHYG"), Err(FieldError::Overflow));
        assert_eq!(u64::decode("zzzzzzzzzzz"), Err(FieldError::Overflow));
        assert_eq!(
            u8::decode("100"),
            Err(FieldError::TooLong { max: 2, actual: 3 })
        );
        assert_eq!(
            u64::decode("100000000000"),
            Err(FieldError::TooLong {
                max: 11,
                actual: 12
            })
        );
        assert_eq!(u8::decode("00"), Err(FieldError::LeadingZero));
        assert_eq!(u8::decode("01"), Err(FieldError::LeadingZero));
        assert_eq!(
            i8::decode("007"),
            Err(FieldError::TooLong { max: 2, actual: 3 })
        );
        for (raw, index) in [("-1", 0), ("+1", 0), (" 1", 0), ("1 ", 1), ("é", 0)] {
            assert_eq!(u8::decode(raw), Err(FieldError::InvalidChar { index }));
        }
        assert_eq!(i8::decode("47"), Ok(-128));
        assert_eq!(i8::decode("48"), Err(FieldError::Overflow));
        assert_eq!(i64::decode("LygHa16AHYF"), Ok(i64::MIN));
    }

    #[test]
    fn decode_bool_id_option() {
        assert_eq!(bool::decode(""), Err(FieldError::Empty));
        assert_eq!(
            bool::decode("11"),
            Err(FieldError::TooLong { max: 1, actual: 2 })
        );
        assert_eq!(bool::decode("2"), Err(FieldError::InvalidChar { index: 0 }));
        assert_eq!(bool::decode("a"), Err(FieldError::InvalidChar { index: 0 }));
        assert_eq!(Id::<GenericMarker>::decode(""), Err(FieldError::Empty));
        assert_eq!(Id::<GenericMarker>::decode("0"), Err(FieldError::ZeroId));
        assert_eq!(Option::<u8>::decode(""), Err(FieldError::Empty));
        assert_eq!(
            Option::<u8>::decode("2"),
            Err(FieldError::InvalidChar { index: 0 })
        );
        assert_eq!(
            Option::<u8>::decode("0x"),
            Err(FieldError::TrailingInput { index: 1 })
        );
        assert_eq!(Option::<u8>::decode("1"), Err(FieldError::Empty));
        assert_eq!(
            Option::<u8>::decode("1-"),
            Err(FieldError::InvalidChar { index: 1 })
        );
        assert_eq!(
            Option::<u8>::decode("1100"),
            Err(FieldError::TooLong { max: 2, actual: 3 })
        );
        assert_eq!(Option::<Option<u8>>::decode("10"), Ok(Some(None)));
        assert_eq!(Option::<Option<u8>>::decode("110"), Ok(Some(Some(0))));
    }

    #[test]
    fn decode_strings_and_lists() {
        assert_eq!(String::decode("!u0000e9"), Ok("é".to_owned()));
        for raw in ["!u0000E9", "!u00e9", "!u000041", "!u00d800", "!u110000"] {
            assert_eq!(String::decode(raw), Err(FieldError::BadEscape { index: 0 }));
        }
        assert!(matches!(
            Cow::<'_, str>::decode("abc"),
            Ok(Cow::Borrowed("abc"))
        ));
        assert!(matches!(Cow::<'_, str>::decode("a!1"), Ok(Cow::Owned(s)) if s == "a,"));
        assert_eq!(Vec::<u8>::decode(""), Ok(vec![]));
        assert_eq!(Vec::<u8>::decode("1!12!1"), Ok(vec![1, 2]));
        assert_eq!(Vec::<u8>::decode("1!12"), Err(FieldError::BadList));
        assert_eq!(Vec::<u8>::decode("1"), Err(FieldError::BadList));
        assert_eq!(Vec::<u8>::decode("!1"), Err(FieldError::Empty));
        assert_eq!(Vec::<String>::decode("!1"), Ok(vec![String::new()]));
        assert_eq!(Vec::<Vec<u8>>::decode("1!01!1"), Ok(vec![vec![1]]));
    }

    #[test]
    fn cid_07_hostile_strings_encode_printable_ascii() {
        let hostile = "!,:~ \t\n\r\u{0}\u{7f}é😀\u{202e}\u{feff}";
        let check = |s: &str| {
            assert!(s.bytes().all(|b| (0x20..=0x7E).contains(&b)), "{s:?}");
            assert!(!s.contains([',', ':', '~']), "{s:?}");
        };
        check(&enc(&hostile.to_owned()));
        check(&enc(&Cow::Borrowed(hostile)));
        check(&enc(&vec![hostile.to_owned(), String::new()]));
        check(&enc(&Some(hostile.to_owned())));
        crate::check::round_trip(&hostile.to_owned()).unwrap();
    }
}
