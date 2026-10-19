//! The runtime Limit validator: [`validate`] and [`cost`] for a built
//! [`V2Message`] or [`LegacyMessage`] (FR-24, FR-78, AD-3, AD-4).
//!
//! Both lower the typed tree into `noctis_rules::component` descriptors and
//! call `validate_components` / `validate_legacy_components` / `cost`, so
//! counts, paths and violation order match `components!` exactly. On top of
//! that they check what the descriptors do not carry: duplicate `custom_id`s
//! (CID-02), `attachment://` references (ATT-05), attachment alt text
//! (ATT-06), legacy `content` (MSG-01), empty bodies (MSG-09) and mention id
//! counts (MSG-04).

mod lenient;
mod lower;
mod walk;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

use noctis_rules::attachment::{
    AttachmentDecl, validate_attachment_description, validate_attachment_ref,
};
use noctis_rules::component as rc;
use noctis_rules::len::discord_len;
use noctis_rules::message::{
    AllowedMentions, MentionIds, validate_content, validate_legacy_not_empty, validate_mentions,
    validate_v2_not_empty,
};
use noctis_rules::{Actual, Bound, LimitMode, Limits, RuleId, Violation};

use self::walk::Visit;
use crate::{
    AttachmentOrigin, AttachmentSet, LegacyMessage, Mentions, Roles, UiError, Users, V2Message,
};

use crate::placement::sealed;

/// A message [`validate`] and [`cost`] accept: [`V2Message`] or
/// [`LegacyMessage`]. It is sealed.
///
/// ```
/// use noctis_ui::{LegacyMessage, Validate, V2Message};
///
/// fn checked<M: Validate>() {}
/// checked::<V2Message>();
/// checked::<LegacyMessage>();
/// ```
pub trait Validate: sealed::Check {}

impl Validate for V2Message {}
impl Validate for LegacyMessage {}

/// What a message uses of the Limits budget, and the warnings found.
///
/// For a [`V2Message`], `components` counts every component at every depth
/// (CV2-03; Section texts and accessories included, select options and
/// gallery items not) and `text_chars` is the `discord_len` sum of every Text
/// Display (CV2-04). For a [`LegacyMessage`], `components` counts its Action
/// Rows and their items, and `text_chars` is the length of its `content`.
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_ui::{Components, TextDisplay, V2Message, cost};
///
/// let msg = V2Message::new(Components::new().push(TextDisplay::new("hi")));
/// let report = cost(&msg, &Limits::default());
/// assert_eq!((report.components, report.text_chars), (1, 2));
/// assert!(report.warnings.is_empty());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    /// Components at every depth (CV2-03).
    pub components: usize,
    /// Text Display characters (V2, CV2-04) or `content` characters (legacy).
    pub text_chars: usize,
    /// Truncations made in Lenient mode, then images without alt text, in
    /// pre-order.
    pub warnings: Vec<LimitWarning>,
}

/// A problem that does not stop a message from being sent.
///
/// Displays as `truncated to fit: <violation>` or
/// `<path>: image has no alt text`.
///
/// ```
/// use noctis_rules::{Actual, Bound, RuleId, Violation};
/// use noctis_ui::LimitWarning;
///
/// let alt = LimitWarning::MissingAlt { path: "gallery[0].item[0]".to_owned() };
/// assert_eq!(alt.to_string(), "gallery[0].item[0]: image has no alt text");
/// assert_eq!(alt.rule(), None);
///
/// let v = Violation::new(RuleId::CV2_08, "row[0].button[0].label", Bound::AtMost(80), Actual::Count(81));
/// let cut = LimitWarning::Truncated(v);
/// assert_eq!(cut.path(), "row[0].button[0].label");
/// assert_eq!(cut.rule(), Some(RuleId::CV2_08));
/// assert_eq!(
///     cut.to_string(),
///     "truncated to fit: [CV2-08] row[0].button[0].label: limit at most 80, actual 81"
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitWarning {
    /// Lenient mode truncated display text with `…`; the violation is the one
    /// it avoided, with the length before truncation.
    Truncated(Violation),
    /// A Thumbnail or gallery item that is not a rendered image has no alt
    /// text (empty or whitespace-only counts as none).
    MissingAlt {
        /// The node's path, such as `section[0].accessory` or
        /// `gallery[1].item[0]`.
        path: String,
    },
}

impl LimitWarning {
    /// Where the warning is.
    pub fn path(&self) -> &str {
        match self {
            Self::Truncated(v) => &v.path,
            Self::MissingAlt { path } => path,
        }
    }

    /// The rule a truncation avoided breaking; `None` for missing alt text.
    pub fn rule(&self) -> Option<RuleId> {
        match self {
            Self::Truncated(v) => Some(v.rule),
            Self::MissingAlt { .. } => None,
        }
    }
}

impl fmt::Display for LimitWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(v) => write!(f, "truncated to fit: {v}"),
            Self::MissingAlt { path } => write!(f, "{path}: image has no alt text"),
        }
    }
}

/// Checks `message` against `limits` before it is sent.
///
/// In [`LimitMode::Strict`] (the default) `message` is never changed. In
/// [`LimitMode::Lenient`] over-long display text is first truncated with `…`
/// (Text Displays to fit the CV2-04 total, button labels, select
/// placeholders, String Select option labels and descriptions, gallery item,
/// Thumbnail and attachment alt text, legacy content), each truncation a
/// [`LimitWarning::Truncated`]; everything else (counts, URLs, option values,
/// `min_values` / `max_values`, ids, CID-02, ATT-05, MSG-04, MSG-09) still
/// fails.
///
/// The violations come in this order: the `noctis_rules::component` result
/// in its own order, then CID-02, ATT-05, ATT-06, MSG-01 (legacy), MSG-09
/// and MSG-04. Each names its path, such as
/// `container[0].section[2].accessory.label`. Unresolved `custom_id`s are
/// not compared. Every warning is returned in the [`CostReport`] and logged
/// with `tracing::warn!` (target `noctis::ui`, fields `rule` and `path`),
/// also when the message then fails.
///
/// # Errors
///
/// [`UiError::Limits`] with every violation found.
///
/// ```
/// use noctis_rules::{Actual, Bound, Limits, RuleId};
/// use noctis_ui::{Components, Container, LinkButton, Section, TextDisplay, UiError, V2Message, validate};
///
/// let label = "a".repeat(81);
/// let mut msg = V2Message::new(Components::new().push(
///     Container::new()
///         .push(TextDisplay::new("a"))
///         .push(TextDisplay::new("b"))
///         .push(Section::new([TextDisplay::new("c")], LinkButton::new("https://x.y").label(label))),
/// ));
/// let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
///     panic!("expected a limits error");
/// };
/// assert_eq!(violations[0].rule, RuleId::CV2_08);
/// assert_eq!(violations[0].path, "container[0].section[2].accessory.label");
/// assert_eq!((&violations[0].limit, &violations[0].actual), (&Bound::AtMost(80), &Actual::Count(81)));
/// ```
///
/// Lenient mode truncates the label instead:
///
/// ```
/// use noctis_rules::{LimitMode, Limits};
/// use noctis_ui::{ActionRow, Components, CreateFlags, LimitWarning, LinkButton, V2Message, validate};
///
/// let mut limits = Limits::default();
/// limits.mode = LimitMode::Lenient;
/// let row = ActionRow::buttons([LinkButton::new("https://x.y").label("a".repeat(81))]);
/// let mut msg = V2Message::new(Components::new().push(row));
/// let report = validate(&mut msg, &limits)?;
/// assert!(matches!(&report.warnings[0], LimitWarning::Truncated(v) if v.path == "row[0].button[0].label"));
/// let json = serde_json::to_value(msg.create_body(CreateFlags::empty())).unwrap();
/// assert_eq!(json["components"][0]["components"][0]["label"], format!("{}…", "a".repeat(79)));
/// # Ok::<(), noctis_ui::UiError>(())
/// ```
pub fn validate<M: Validate>(message: &mut M, limits: &Limits) -> Result<CostReport, UiError> {
    message.validate_with(limits)
}

/// Measures `message` without changing or sending it: its component count,
/// text characters and missing-alt warnings (no truncation is done or
/// reported, and nothing is logged).
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_ui::{ActionRow, LegacyMessage, LimitWarning, LinkButton, cost};
///
/// let row = ActionRow::buttons([LinkButton::new("https://a.b"), LinkButton::new("https://c.d")]);
/// let report = cost(&LegacyMessage::new("héllo").rows([row]), &Limits::default());
/// assert_eq!((report.components, report.text_chars), (3, 5));
/// ```
pub fn cost<M: Validate>(message: &M, limits: &Limits) -> CostReport {
    message.cost_with(limits)
}

impl sealed::Sealed for V2Message {}

impl sealed::Check for V2Message {
    fn validate_with(&mut self, limits: &Limits) -> Result<CostReport, UiError> {
        let mut warnings = Vec::new();
        if limits.mode == LimitMode::Lenient {
            lenient::fit_v2(self, limits, &mut warnings);
        }
        let (result, c) = lower::with_v2(&self.components, |tree| {
            (
                rc::validate_components(tree, limits),
                rc::cost(tree, limits),
            )
        });
        let mut violations = result.err().unwrap_or_default();
        let sites = walk::v2_sites(&self.components);
        check_custom_ids(&sites, &mut violations);
        check_attachment_refs(&sites, &self.attachments, limits, &mut violations);
        check_attachment_alts(&self.attachments, limits, &mut violations);
        violations.extend(validate_v2_not_empty(self.components.len(), limits));
        violations.extend(check_mentions(&self.mentions, limits));
        missing_alt(&sites, &mut warnings);
        finish(
            CostReport {
                components: c.components,
                text_chars: c.text_len,
                warnings,
            },
            violations,
        )
    }

    fn cost_with(&self, limits: &Limits) -> CostReport {
        let c = lower::with_v2(&self.components, |tree| rc::cost(tree, limits));
        let mut warnings = Vec::new();
        missing_alt(&walk::v2_sites(&self.components), &mut warnings);
        CostReport {
            components: c.components,
            text_chars: c.text_len,
            warnings,
        }
    }
}

impl sealed::Sealed for LegacyMessage {}

impl sealed::Check for LegacyMessage {
    fn validate_with(&mut self, limits: &Limits) -> Result<CostReport, UiError> {
        let mut warnings = Vec::new();
        if limits.mode == LimitMode::Lenient {
            lenient::fit_legacy(self, limits, &mut warnings);
        }
        let (result, c) = lower::with_legacy(&self.rows, |tree| {
            (
                rc::validate_legacy_components(tree, limits),
                rc::cost(tree, limits),
            )
        });
        let mut violations = result.err().unwrap_or_default();
        check_custom_ids(&walk::row_sites(&self.rows), &mut violations);
        violations.extend(validate_content(&self.content, limits));
        violations.extend(validate_legacy_not_empty(
            &self.content,
            self.rows.len(),
            limits,
        ));
        violations.extend(check_mentions(&self.mentions, limits));
        finish(
            CostReport {
                components: c.components,
                text_chars: discord_len(&self.content),
                warnings,
            },
            violations,
        )
    }

    fn cost_with(&self, limits: &Limits) -> CostReport {
        let c = lower::with_legacy(&self.rows, |tree| rc::cost(tree, limits));
        CostReport {
            components: c.components,
            text_chars: discord_len(&self.content),
            warnings: Vec::new(),
        }
    }
}

/// Logs every warning, then returns the report or the violations.
fn finish(report: CostReport, violations: Vec<Violation>) -> Result<CostReport, UiError> {
    for w in &report.warnings {
        tracing::warn!(
            target: "noctis::ui",
            rule = w.rule().map_or("", RuleId::as_str),
            path = w.path(),
            "{w}"
        );
    }
    if violations.is_empty() {
        Ok(report)
    } else {
        Err(UiError::Limits { violations })
    }
}

/// CID-02: every resolved `custom_id` once; each later use is a violation
/// noting the first.
fn check_custom_ids(sites: &[Visit<'_>], out: &mut Vec<Violation>) {
    let mut first: HashMap<&str, String> = HashMap::new();
    for visit in sites {
        let Some(id) = visit.site.custom_id().and_then(|id| id.get()) else {
            continue;
        };
        let path = format!("{}.custom_id", visit.path);
        match first.entry(id) {
            Entry::Vacant(e) => {
                e.insert(path);
            }
            Entry::Occupied(e) => out.push(
                Violation::new(
                    RuleId::CID_02,
                    path,
                    Bound::Describe("a custom_id unique within the message"),
                    Actual::Text(id.to_owned()),
                )
                .with_note(format!("first used at {}", e.get())),
            ),
        }
    }
}

/// ATT-05: every `attachment://` URL names a file of `set`.
fn check_attachment_refs(
    sites: &[Visit<'_>],
    set: &AttachmentSet,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let files: Vec<AttachmentDecl<'_>> = set
        .iter()
        .map(|a| AttachmentDecl::new(a.name(), a.bytes().len()))
        .collect();
    for visit in sites {
        let Some(url) = visit.site.url() else {
            continue;
        };
        for mut v in validate_attachment_ref(url, &files, limits) {
            v.path = format!("{}.url", visit.path);
            out.push(v);
        }
    }
}

/// ATT-06: the alt text of every attachment, in set order.
fn check_attachment_alts(set: &AttachmentSet, limits: &Limits, out: &mut Vec<Violation>) {
    for (k, attachment) in set.iter().enumerate() {
        let Some(alt) = attachment.alt_text() else {
            continue;
        };
        for mut v in validate_attachment_description(alt, limits) {
            v.path = format!("attachments[{k}].description");
            out.push(v.with_note(format!("file `{}`", attachment.name())));
        }
    }
}

/// MSG-04: the explicit user and role ids.
fn check_mentions(mentions: &Mentions, limits: &Limits) -> Vec<Violation> {
    let user_ids: Vec<u64> = match mentions.user_mentions() {
        Users::Only(ids) => ids.iter().map(|id| id.get()).collect(),
        Users::None | Users::All => Vec::new(),
    };
    let role_ids: Vec<u64> = match mentions.role_mentions() {
        Roles::Only(ids) => ids.iter().map(|id| id.get()).collect(),
        Roles::None | Roles::All => Vec::new(),
    };
    let users = match mentions.user_mentions() {
        Users::None => MentionIds::None,
        Users::All => MentionIds::All,
        Users::Only(_) => MentionIds::Only(&user_ids),
    };
    let roles = match mentions.role_mentions() {
        Roles::None => MentionIds::None,
        Roles::All => MentionIds::All,
        Roles::Only(_) => MentionIds::Only(&role_ids),
    };
    let allowed = AllowedMentions::new()
        .users(users)
        .roles(roles)
        .everyone(mentions.pings_everyone())
        .replied_user(mentions.pings_replied_user());
    validate_mentions(&allowed, limits)
}

/// Thumbnails and gallery items that are not rendered images and have no
/// alt text of their own.
fn missing_alt(sites: &[Visit<'_>], out: &mut Vec<LimitWarning>) {
    for visit in sites {
        let Some((media, alt)) = visit.site.image() else {
            continue;
        };
        let rendered = media
            .attachment()
            .is_some_and(|r| r.origin() == AttachmentOrigin::Rendered);
        if !rendered && alt.is_none_or(|a| a.trim().is_empty()) {
            out.push(LimitWarning::MissingAlt {
                path: visit.path.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionRow, Components, Container, LinkButton, Media, MediaGallery, MediaItem, Section,
        TextDisplay, Thumbnail,
    };

    /// The walk and the lowered tree name the same node the same way: an
    /// over-long label or alt at each site gives a rules violation whose
    /// path is the walk's path plus the field.
    #[test]
    fn walk_paths_match_rules_paths() {
        let long = "a".repeat(2000);
        let link = || LinkButton::new("https://x.y").label(long.clone());
        let body = Components::new()
            .push(TextDisplay::new("t"))
            .push(
                Container::new()
                    .push(TextDisplay::new("t"))
                    .push(Section::new([TextDisplay::new("t")], link()))
                    .push(ActionRow::buttons([link(), link()]))
                    .push(MediaGallery::new([
                        MediaItem::new(Media::url("https://i")).alt(long.clone())
                    ])),
            )
            .push(Section::new(
                [TextDisplay::new("t")],
                Thumbnail::new(Media::url("https://i")).alt(long.clone()),
            ))
            .push(ActionRow::buttons([link()]));
        let errs = lower::with_v2(&body, |tree| {
            rc::validate_components(tree, &Limits::default())
        })
        .unwrap_err();
        let field_paths: Vec<&str> = errs
            .iter()
            .filter(|v| v.rule != RuleId::CV2_04)
            .map(|v| v.path.as_str())
            .collect();
        let sites = walk::v2_sites(&body);
        let walk_paths: Vec<String> = sites
            .iter()
            .map(|s| match s.site {
                walk::Site::Button(_) => format!("{}.label", s.path),
                _ => format!("{}.description", s.path),
            })
            .collect();
        assert_eq!(field_paths, walk_paths);
        assert_eq!(
            walk_paths,
            [
                "container[1].section[1].accessory.label",
                "container[1].row[2].button[0].label",
                "container[1].row[2].button[1].label",
                "container[1].gallery[3].item[0].description",
                "section[2].accessory.description",
                "row[3].button[0].label",
            ]
        );
    }
}
