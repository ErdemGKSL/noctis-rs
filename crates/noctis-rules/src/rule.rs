//! Rule ids from the Discord Rules Catalog and the [`Violation`] type that cites them.

use std::error::Error;
use std::fmt;

/// A stable rule id from the Discord Rules Catalog (`docs/discord-rules.md`),
/// such as `CV2-03`.
///
/// The set is closed: the only values are the associated constants
/// ([`RuleId::CMD_01`], [`RuleId::CV2_03`], …), listed in catalog order by
/// [`RuleId::ALL`]. A test compares that list with the catalog file.
///
/// ```
/// use noctis_rules::RuleId;
///
/// assert_eq!(RuleId::CV2_03.as_str(), "CV2-03");
/// assert_eq!(RuleId::lookup("CMD-01"), Some(RuleId::CMD_01));
/// assert_eq!(RuleId::lookup("cmd-01"), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleId(&'static str);

macro_rules! rule_ids {
    ($($konst:ident = $id:literal,)*) => {
        impl RuleId {
            $(
                #[doc = concat!("Rule `", $id, "`.")]
                pub const $konst: RuleId = RuleId($id);
            )*

            /// Every rule id, in catalog row order.
            pub const ALL: &[RuleId] = &[$(RuleId::$konst,)*];
        }
    };
}

rule_ids! {
    CMD_01 = "CMD-01", CMD_02 = "CMD-02", CMD_03 = "CMD-03", CMD_04 = "CMD-04",
    CMD_05 = "CMD-05", CMD_06 = "CMD-06", CMD_07 = "CMD-07", CMD_10 = "CMD-10",
    CMD_11 = "CMD-11", CMD_12 = "CMD-12", CMD_13 = "CMD-13", CMD_14 = "CMD-14",
    CMD_15 = "CMD-15", CMD_16 = "CMD-16",
    OPT_01 = "OPT-01", OPT_02 = "OPT-02", OPT_03 = "OPT-03", OPT_04 = "OPT-04",
    OPT_05 = "OPT-05", OPT_06 = "OPT-06", OPT_07 = "OPT-07", OPT_08 = "OPT-08",
    OPT_09 = "OPT-09", OPT_10 = "OPT-10", OPT_11 = "OPT-11", OPT_12 = "OPT-12",
    OPT_13 = "OPT-13", OPT_14 = "OPT-14",
    LOC_01 = "LOC-01", LOC_02 = "LOC-02", LOC_03 = "LOC-03", LOC_04 = "LOC-04",
    LOC_05 = "LOC-05", LOC_06 = "LOC-06",
    CTX_01 = "CTX-01", CTX_02 = "CTX-02", CTX_03 = "CTX-03",
    META_01 = "META-01", META_02 = "META-02", META_03 = "META-03", META_04 = "META-04",
    META_05 = "META-05",
    REG_01 = "REG-01", REG_02 = "REG-02", REG_03 = "REG-03", REG_04 = "REG-04",
    REG_05 = "REG-05", REG_06 = "REG-06",
    INT_01 = "INT-01", INT_02 = "INT-02", INT_03 = "INT-03", INT_04 = "INT-04",
    INT_05 = "INT-05", INT_06 = "INT-06", INT_07 = "INT-07", INT_08 = "INT-08",
    INT_09 = "INT-09", INT_10 = "INT-10", INT_11 = "INT-11", INT_12 = "INT-12",
    INT_13 = "INT-13",
    CID_01 = "CID-01", CID_02 = "CID-02", CID_03 = "CID-03", CID_04 = "CID-04",
    CID_05 = "CID-05", CID_06 = "CID-06", CID_07 = "CID-07",
    CV2_01 = "CV2-01", CV2_02 = "CV2-02", CV2_03 = "CV2-03", CV2_04 = "CV2-04",
    CV2_05 = "CV2-05", CV2_06 = "CV2-06", CV2_07 = "CV2-07", CV2_08 = "CV2-08",
    CV2_09 = "CV2-09", CV2_10 = "CV2-10", CV2_11 = "CV2-11", CV2_12 = "CV2-12",
    CV2_13 = "CV2-13", CV2_14 = "CV2-14", CV2_15 = "CV2-15", CV2_16 = "CV2-16",
    CV2_17 = "CV2-17",
    MOD_01 = "MOD-01", MOD_02 = "MOD-02", MOD_03 = "MOD-03", MOD_04 = "MOD-04",
    MOD_05 = "MOD-05", MOD_06 = "MOD-06", MOD_07 = "MOD-07", MOD_08 = "MOD-08",
    MOD_09 = "MOD-09",
    MSG_01 = "MSG-01", MSG_02 = "MSG-02", MSG_03 = "MSG-03", MSG_04 = "MSG-04",
    MSG_05 = "MSG-05", MSG_06 = "MSG-06", MSG_07 = "MSG-07", MSG_08 = "MSG-08",
    MSG_09 = "MSG-09",
    ATT_01 = "ATT-01", ATT_02 = "ATT-02", ATT_03 = "ATT-03", ATT_04 = "ATT-04",
    ATT_05 = "ATT-05", ATT_06 = "ATT-06", ATT_07 = "ATT-07",
    WH_01 = "WH-01", WH_02 = "WH-02", WH_03 = "WH-03", WH_04 = "WH-04",
    WH_05 = "WH-05",
    RL_01 = "RL-01", RL_02 = "RL-02", RL_03 = "RL-03", RL_04 = "RL-04",
    GW_01 = "GW-01", GW_02 = "GW-02", GW_03 = "GW-03", GW_04 = "GW-04",
    GW_05 = "GW-05", GW_06 = "GW-06", GW_07 = "GW-07",
}

impl RuleId {
    /// The id as written in the catalog, for example `"CV2-03"`.
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// The area part before the `-`, for example `"CV2"` for `CV2-03`.
    pub fn area(self) -> &'static str {
        self.0.split_once('-').map_or(self.0, |(area, _)| area)
    }

    /// The two-digit number after the `-`, for example `3` for `CV2-03`.
    pub const fn number(self) -> u8 {
        let bytes = self.0.as_bytes();
        let len = bytes.len();
        (bytes[len - 2] - b'0') * 10 + (bytes[len - 1] - b'0')
    }

    /// Finds the id spelled exactly `id` (case-sensitive, no trimming).
    ///
    /// Returns `None` for anything that is not a catalog id, and never panics.
    pub const fn lookup(id: &str) -> Option<RuleId> {
        let mut i = 0;
        while i < Self::ALL.len() {
            if str_eq(Self::ALL[i].0, id) {
                return Some(Self::ALL[i]);
            }
            i += 1;
        }
        None
    }

    /// Whether this is a structural rule (CV2-03, CV2-05, CV2-06, CID-01, CID-02),
    /// which [`LimitMode::Lenient`](crate::LimitMode::Lenient) never truncates.
    pub const fn is_structural(self) -> bool {
        const STRUCTURAL: [RuleId; 5] = [
            RuleId::CV2_03,
            RuleId::CV2_05,
            RuleId::CV2_06,
            RuleId::CID_01,
            RuleId::CID_02,
        ];
        let mut i = 0;
        while i < STRUCTURAL.len() {
            if str_eq(STRUCTURAL[i].0, self.0) {
                return true;
            }
            i += 1;
        }
        false
    }
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The limit a [`Violation`] broke.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// The value must be at most this many (prints `at most N`).
    AtMost(usize),
    /// The value must be at least this many (prints `at least N`).
    AtLeast(usize),
    /// The value must lie in `min..=max` (prints `between MIN and MAX`).
    Between {
        /// Smallest allowed value.
        min: usize,
        /// Largest allowed value.
        max: usize,
    },
    /// The value must be exactly this many (prints `exactly N`).
    Exactly(usize),
    /// A non-numeric rule, described in words (printed as is).
    Describe(&'static str),
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtMost(n) => write!(f, "at most {n}"),
            Self::AtLeast(n) => write!(f, "at least {n}"),
            Self::Between { min, max } => write!(f, "between {min} and {max}"),
            Self::Exactly(n) => write!(f, "exactly {n}"),
            Self::Describe(text) => f.write_str(text),
        }
    }
}

/// The actual value that broke a [`Bound`].
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Actual {
    /// A count or length (printed as the number).
    Count(usize),
    /// The offending text (printed escaped, inside backticks).
    Text(String),
}

impl fmt::Display for Actual {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(n) => write!(f, "{n}"),
            Self::Text(text) => write!(f, "`{}`", text.escape_debug()),
        }
    }
}

/// A broken Discord rule: which rule, where, the limit and the actual value.
///
/// Displays as `[RULE-ID] path: limit <bound>, actual <actual>`; the `path: ` part
/// is left out when the path is empty. Each note adds a `\n  = note: …` line and
/// the help adds a final `\n  = help: …` line, the format compile errors use.
///
/// ```
/// use noctis_rules::{Actual, Bound, RuleId, Violation};
///
/// let v = Violation::new(RuleId::CV2_03, "components", Bound::AtMost(40), Actual::Count(41));
/// assert_eq!(v.to_string(), "[CV2-03] components: limit at most 40, actual 41");
///
/// let v = v.with_note("in the top-level container").with_help("split the message");
/// assert_eq!(
///     v.to_string(),
///     "[CV2-03] components: limit at most 40, actual 41\n  \
///      = note: in the top-level container\n  = help: split the message"
/// );
/// ```
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The rule that was broken.
    pub rule: RuleId,
    /// Where in the input the violation is (may be empty).
    pub path: String,
    /// The limit that was broken.
    pub limit: Bound,
    /// The value that broke it.
    pub actual: Actual,
    /// Extra context lines, printed as `= note: …` (may be empty).
    pub notes: Vec<String>,
    /// A suggested fix, printed as `= help: …`.
    pub help: Option<String>,
}

impl Violation {
    /// Creates a violation without notes or help.
    pub fn new(rule: RuleId, path: impl Into<String>, limit: Bound, actual: Actual) -> Self {
        Self {
            rule,
            path: path.into(),
            limit,
            actual,
            notes: Vec::new(),
            help: None,
        }
    }

    /// Appends a `= note:` line.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Sets the `= help:` line, replacing any earlier one.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.rule)?;
        if !self.path.is_empty() {
            write!(f, "{}: ", self.path)?;
        }
        write!(f, "limit {}, actual {}", self.limit, self.actual)?;
        for note in &self.notes {
            write!(f, "\n  = note: {note}")?;
        }
        if let Some(help) = &self.help {
            write!(f, "\n  = help: {help}")?;
        }
        Ok(())
    }
}

impl Error for Violation {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn has_id_shape(id: &str) -> bool {
        let Some((area, num)) = id.split_once('-') else {
            return false;
        };
        let mut area_chars = area.chars();
        area_chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && (2..=4).contains(&area.len())
            && area_chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && num.len() == 2
            && num.bytes().all(|b| b.is_ascii_digit())
    }

    #[test]
    fn all_has_126_unique_well_formed_ids() {
        assert_eq!(RuleId::ALL.len(), 126);
        let unique: HashSet<_> = RuleId::ALL.iter().collect();
        assert_eq!(unique.len(), 126);
        for id in RuleId::ALL {
            assert!(has_id_shape(id.as_str()), "{id}");
        }
    }

    #[test]
    fn consts_print_their_id() {
        assert_eq!(RuleId::CV2_03.as_str(), "CV2-03");
        assert_eq!(RuleId::META_05.as_str(), "META-05");
        assert_eq!(RuleId::CV2_03.to_string(), "CV2-03");
        assert_eq!(RuleId::META_05.to_string(), "META-05");
        assert_eq!(RuleId::CV2_03.area(), "CV2");
        assert_eq!(RuleId::META_05.area(), "META");
        assert_eq!(RuleId::CV2_03.number(), 3);
        assert_eq!(RuleId::META_05.number(), 5);
    }

    #[test]
    fn area_and_number_rebuild_every_id() {
        for id in RuleId::ALL {
            assert_eq!(format!("{}-{:02}", id.area(), id.number()), id.as_str());
        }
    }

    #[test]
    fn lookup_finds_exact_ids() {
        assert_eq!(RuleId::lookup("CMD-01"), Some(RuleId::CMD_01));
        assert_eq!(RuleId::lookup("CV2-17"), Some(RuleId::CV2_17));
        for id in RuleId::ALL {
            assert_eq!(RuleId::lookup(id.as_str()), Some(*id));
        }
    }

    #[test]
    fn lookup_rejects_everything_else() {
        for miss in [
            "cmd-01", "CMD-1", "CMD-08", "CMD-01 ", " CMD-01", "", "XYZ-01",
        ] {
            assert_eq!(RuleId::lookup(miss), None, "{miss:?}");
        }
    }

    #[test]
    fn structural_ids_are_exactly_five() {
        let structural: Vec<_> = RuleId::ALL
            .iter()
            .filter(|r| r.is_structural())
            .copied()
            .collect();
        assert_eq!(
            structural,
            [
                RuleId::CID_01,
                RuleId::CID_02,
                RuleId::CV2_03,
                RuleId::CV2_05,
                RuleId::CV2_06,
            ]
        );
    }

    #[test]
    fn violation_display_with_path() {
        let v = Violation::new(
            RuleId::CV2_03,
            "components",
            Bound::AtMost(40),
            Actual::Count(41),
        );
        assert_eq!(
            v.to_string(),
            "[CV2-03] components: limit at most 40, actual 41"
        );
    }

    #[test]
    fn violation_display_without_path() {
        let v = Violation::new(
            RuleId::CMD_02,
            "",
            Bound::Between { min: 1, max: 32 },
            Actual::Count(0),
        );
        assert_eq!(v.to_string(), "[CMD-02] limit between 1 and 32, actual 0");
    }

    #[test]
    fn bound_and_actual_display() {
        assert_eq!(Bound::AtLeast(2).to_string(), "at least 2");
        assert_eq!(Bound::Exactly(1).to_string(), "exactly 1");
        assert_eq!(
            Bound::Describe("lowercase only").to_string(),
            "lowercase only"
        );
        assert_eq!(Actual::Text("Ab\n".into()).to_string(), "`Ab\\n`");
        let v = Violation::new(
            RuleId::CMD_03,
            "name",
            Bound::Describe("lowercase only"),
            Actual::Text("Ab\n".into()),
        );
        assert_eq!(
            v.to_string(),
            "[CMD-03] name: limit lowercase only, actual `Ab\\n`"
        );
        let _: &dyn Error = &v;
    }

    #[test]
    fn violation_display_with_notes_and_help() {
        let v = Violation::new(
            RuleId::CMD_11,
            "config set",
            Bound::Describe("a subcommand or a subcommand group, not both"),
            Actual::Text("set".into()),
        )
        .with_note("first")
        .with_note("second")
        .with_help("fix it");
        assert_eq!(v.notes, ["first", "second"]);
        assert_eq!(v.help.as_deref(), Some("fix it"));
        assert_eq!(
            v.to_string(),
            "[CMD-11] config set: limit a subcommand or a subcommand group, not both, \
             actual `set`\n  = note: first\n  = note: second\n  = help: fix it"
        );
    }

    #[test]
    fn violation_display_with_help_only() {
        let v = Violation::new(
            RuleId::CMD_03,
            "",
            Bound::Describe("lowercase"),
            Actual::Text("Ab".into()),
        )
        .with_help("use `ab`")
        .with_help("use `ab` instead");
        assert!(v.notes.is_empty());
        assert_eq!(
            v.to_string(),
            "[CMD-03] limit lowercase, actual `Ab`\n  = help: use `ab` instead"
        );
    }
}
