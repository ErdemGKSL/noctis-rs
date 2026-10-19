//! Custom Payload field types and their round-trip / worst-case-width checks
//! (`noctis::codec`, see `.claude/skills/noctis/references/routes-payloads-refs.md` §2).

use noctis::codec::check::{CheckError, assert_round_trip, assert_worst_case, round_trip};
use noctis::codec::{BoundedStr, Field, FieldDecode, FieldError};

/// A percentage, 0..=100, carried in a Payload as one `u8`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Percent(u8);

impl Field for Percent {
    const MAX_LEN: Option<usize> = <u8 as Field>::MAX_LEN;
    fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result {
        self.0.encode(out)
    }
}

impl<'de> FieldDecode<'de> for Percent {
    fn decode(raw: &'de str) -> Result<Self, FieldError> {
        let v = u8::decode(raw)?;
        if v <= 100 {
            Ok(Self(v))
        } else {
            Err(FieldError::Overflow)
        }
    }
}

#[test]
fn percent_round_trips() {
    assert_round_trip(&Percent(0));
    assert_round_trip(&Percent(42));
    assert_worst_case(&Percent(100));
}

#[test]
fn percent_rejects_out_of_range() {
    // 101 is a valid u8 encoding but not a valid Percent.
    assert_eq!(
        round_trip(&Percent(101)),
        Err(CheckError::Decode {
            encoded: "1d".to_owned(),
            error: FieldError::Overflow,
        })
    );
}

#[test]
fn option_field_is_absent_compatible() {
    // A trailing `Option` field missing from an older id decodes as `None`.
    assert_eq!(<Option<Percent>>::absent(), Some(None));
    // Every other field has no absent value: missing stays an error.
    assert_eq!(Percent::absent(), None);
    assert_eq!(<BoundedStr<6>>::absent(), None);
}

#[test]
fn bounded_str_round_trips() {
    const CODE: BoundedStr<6> = BoundedStr::from_static("a,b:c!");
    assert_round_trip(&CODE);
    assert_worst_case(&BoundedStr::<6>::from_static("!!!!!!"));
    assert!(BoundedStr::<6>::new("çay").is_err());
}
