//! Message attachments: [`validate_attachments`] (ATT-01..03, ATT-06),
//! [`validate_attachment_ref`] (ATT-05), [`validate_rendered_image_alt`]
//! (ATT-06: a rendered image in a Thumbnail or Media Gallery needs alt text,
//! FR-30) and granular functions.
//!
//! The per-file limit is `Limits::attachment_bytes` unless the caller passes
//! the interaction's `attachment_size_limit` as `file_limit`, which then
//! replaces the field. The ATT-02 request total is the sum of every file size
//! plus `json_len` (the JSON payload's byte length), saturating.
//!
//! ATT-04 (`files[n]` naming) and ATT-07 (embed image formats) hold by
//! construction: Noctis generates the multipart parts and the Renderer only
//! produces allowed formats. The functions only report: they never truncate.

use crate::check::{at_most, finish, text_at_most};
use crate::{Actual, Bound, Limits, RuleId, Violation};

const ATTACHMENT_SCHEME: &str = "attachment://";

/// One file attached to a message.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachmentDecl<'a> {
    /// The filename (what `attachment://<filename>` refers to).
    pub filename: &'a str,
    /// The size in bytes (ATT-01, ATT-02).
    pub size: usize,
    /// The alt text (ATT-06).
    pub description: Option<&'a str>,
}

impl<'a> AttachmentDecl<'a> {
    /// A file without alt text.
    pub const fn new(filename: &'a str, size: usize) -> Self {
        Self {
            filename,
            size,
            description: None,
        }
    }

    /// Sets the alt text.
    #[must_use]
    pub const fn description(self, description: &'a str) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }
}

/// Checks the attachments of one message request.
///
/// The order is ATT-03 (path `attachments`), then per file `k` ATT-01
/// (`attachments[k].size`) and ATT-06 (`attachments[k].description`), each
/// with a ``file `name` `` note, then ATT-02 (path `request`). `file_limit`,
/// when given, replaces `Limits::attachment_bytes`.
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::attachment::{AttachmentDecl, validate_attachments};
///
/// let files = [AttachmentDecl::new("a.png", 20_971_521)];
/// let errs = validate_attachments(&files, 0, None, &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::ATT_01);
/// assert_eq!(errs[0].path, "attachments[0].size");
/// ```
pub fn validate_attachments(
    attachments: &[AttachmentDecl<'_>],
    json_len: usize,
    file_limit: Option<usize>,
    limits: &Limits,
) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    at_most(
        RuleId::ATT_03,
        "attachments",
        limits.attachments,
        attachments.len(),
        &mut out,
    );
    for (k, file) in attachments.iter().enumerate() {
        let note = format!("file `{}`", file.filename);
        let start = out.len();
        check_file_size(
            file.size,
            file_limit,
            &format!("attachments[{k}].size"),
            limits,
            &mut out,
        );
        if let Some(d) = file.description {
            text_at_most(
                RuleId::ATT_06,
                format!("attachments[{k}].description"),
                limits.attachment_description_len,
                d,
                &mut out,
            );
        }
        for v in &mut out[start..] {
            v.notes.push(note.clone());
        }
    }
    let total = attachments
        .iter()
        .fold(json_len, |acc, f| acc.saturating_add(f.size));
    at_most(
        RuleId::ATT_02,
        "request",
        limits.request_bytes,
        total,
        &mut out,
    );
    finish(out)
}

fn check_file_size(
    size: usize,
    file_limit: Option<usize>,
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let max = file_limit.unwrap_or(limits.attachment_bytes);
    at_most(RuleId::ATT_01, path, max, size, out);
}

/// ATT-01: one file size against `file_limit` (the interaction's
/// `attachment_size_limit`) or else `Limits::attachment_bytes`. The path is
/// empty and there is no note.
pub fn validate_file_size(
    size: usize,
    file_limit: Option<usize>,
    limits: &Limits,
) -> Vec<Violation> {
    let mut out = Vec::new();
    check_file_size(size, file_limit, "", limits, &mut out);
    out
}

/// ATT-06: alt text of at most `attachment_description_len` characters (empty
/// is allowed). The path is empty.
pub fn validate_attachment_description(description: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(
        RuleId::ATT_06,
        "",
        limits.attachment_description_len,
        description,
        &mut out,
    );
    out
}

/// ATT-06 (Noctis policy, FR-30): a rendered image shown in a Thumbnail or
/// a Media Gallery item needs its own alt text. `alt` is that component's
/// `description`; `None`, empty or whitespace-only is one violation
/// (`Actual::Text` of the alt, `""` for `None`). The path is empty and
/// `limits` is not read.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::attachment::validate_rendered_image_alt;
///
/// let l = Limits::default();
/// assert_eq!(validate_rendered_image_alt(None, &l)[0].rule, RuleId::ATT_06);
/// assert!(validate_rendered_image_alt(Some("Profile card"), &l).is_empty());
/// ```
pub fn validate_rendered_image_alt(alt: Option<&str>, limits: &Limits) -> Vec<Violation> {
    let _ = limits;
    if alt.is_some_and(|a| !a.trim().is_empty()) {
        return Vec::new();
    }
    vec![Violation::new(
        RuleId::ATT_06,
        "",
        Bound::Describe("alt text on a rendered image (FR-30)"),
        Actual::Text(alt.unwrap_or_default().to_owned()),
    )]
}

/// ATT-05: an `attachment://<name>` URL must name a file of `attachments`
/// (exact, case-sensitive match). Any other URL, including a differently
/// cased scheme, is not an attachment reference and passes. The path is
/// empty.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::attachment::{AttachmentDecl, validate_attachment_ref};
///
/// let files = [AttachmentDecl::new("a.png", 10)];
/// assert!(validate_attachment_ref("attachment://a.png", &files, &Limits::default()).is_empty());
/// let errs = validate_attachment_ref("attachment://b.png", &files, &Limits::default());
/// assert_eq!(errs[0].rule, RuleId::ATT_05);
/// ```
pub fn validate_attachment_ref(
    url: &str,
    attachments: &[AttachmentDecl<'_>],
    limits: &Limits,
) -> Vec<Violation> {
    let _ = limits;
    let Some(name) = url.strip_prefix(ATTACHMENT_SCHEME) else {
        return Vec::new();
    };
    if !name.is_empty() && attachments.iter().any(|f| f.filename == name) {
        return Vec::new();
    }
    vec![Violation::new(
        RuleId::ATT_05,
        "",
        Bound::Describe("attachment://<name> of a file in this message"),
        Actual::Text(url.to_owned()),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max(rule: RuleId, path: &str, max: usize, count: usize) -> Violation {
        Violation::new(rule, path, Bound::AtMost(max), Actual::Count(count))
    }

    fn check(
        files: &[AttachmentDecl<'_>],
        json: usize,
        file_limit: Option<usize>,
    ) -> Vec<Violation> {
        check_with(files, json, file_limit, &Limits::default())
    }

    fn check_with(
        files: &[AttachmentDecl<'_>],
        json: usize,
        file_limit: Option<usize>,
        limits: &Limits,
    ) -> Vec<Violation> {
        match validate_attachments(files, json, file_limit, limits) {
            Ok(()) => Vec::new(),
            Err(errs) => {
                assert!(!errs.is_empty());
                errs
            }
        }
    }

    fn png(size: usize) -> AttachmentDecl<'static> {
        AttachmentDecl::new("a.png", size)
    }

    #[test]
    fn att_03_empty_is_ok() {
        assert_eq!(
            validate_attachments(&[], 0, None, &Limits::default()),
            Ok(())
        );
    }

    #[test]
    fn att_03_file_count() {
        let files = vec![png(1); 11];
        assert_eq!(check(&files[..10], 0, None), []);
        assert_eq!(
            check(&files, 0, None),
            [max(RuleId::ATT_03, "attachments", 10, 11)]
        );
        let lowered = Limits {
            attachments: 2,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&files[..3], 0, None, &lowered),
            [max(RuleId::ATT_03, "attachments", 2, 3)]
        );
    }

    #[test]
    fn att_01_file_size() {
        let size = |s: usize| max(RuleId::ATT_01, "attachments[0].size", 20_971_520, s);
        assert_eq!(check(&[png(20_971_520)], 0, None), []);
        assert_eq!(
            check(&[png(20_971_521)], 0, None),
            [size(20_971_521).with_note("file `a.png`")]
        );
        assert_eq!(
            check(&[png(10_485_761)], 0, Some(10_485_760)),
            [max(
                RuleId::ATT_01,
                "attachments[0].size",
                10_485_760,
                10_485_761
            )
            .with_note("file `a.png`")]
        );
        assert_eq!(
            check(&[png(52_428_800)], 0, Some(104_857_600)),
            [max(RuleId::ATT_02, "request", 26_214_400, 52_428_800)]
        );
        let lowered = Limits {
            attachment_bytes: 1000,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&[png(1001)], 0, None, &lowered),
            [max(RuleId::ATT_01, "attachments[0].size", 1000, 1001).with_note("file `a.png`")]
        );
        let l = Limits::default();
        assert!(validate_file_size(20_971_520, None, &l).is_empty());
        assert_eq!(
            validate_file_size(20_971_521, None, &l),
            [max(RuleId::ATT_01, "", 20_971_520, 20_971_521)]
        );
        assert_eq!(
            validate_file_size(10_485_761, Some(10_485_760), &l),
            [max(RuleId::ATT_01, "", 10_485_760, 10_485_761)]
        );
        assert!(validate_file_size(52_428_800, Some(104_857_600), &l).is_empty());
        assert_eq!(
            validate_file_size(1001, None, &lowered),
            [max(RuleId::ATT_01, "", 1000, 1001)]
        );
    }

    #[test]
    fn att_06_alt_text() {
        let desc = |s: usize| {
            max(RuleId::ATT_06, "attachments[0].description", 1024, s).with_note("file `a.png`")
        };
        let a1024 = "a".repeat(1024);
        let a1025 = "a".repeat(1025);
        let e512 = "😀".repeat(512);
        let e513 = "😀".repeat(513);
        for ok in [a1024.as_str(), e512.as_str(), ""] {
            assert_eq!(check(&[png(1).description(ok)], 0, None), []);
        }
        assert_eq!(check(&[png(1).description(&a1025)], 0, None), [desc(1025)]);
        assert_eq!(check(&[png(1).description(&e513)], 0, None), [desc(1026)]);
        let l = Limits::default();
        assert!(validate_attachment_description("", &l).is_empty());
        assert!(validate_attachment_description(&a1024, &l).is_empty());
        assert_eq!(
            validate_attachment_description(&a1025, &l),
            [max(RuleId::ATT_06, "", 1024, 1025)]
        );
    }

    #[test]
    fn att_02_request_size() {
        let two = [png(13_107_200), png(13_107_200)];
        assert_eq!(check(&two, 0, None), []);
        assert_eq!(
            check(&two, 1, None),
            [max(RuleId::ATT_02, "request", 26_214_400, 26_214_401)]
        );
        let errs = check(&[png(usize::MAX), png(1)], 0, Some(usize::MAX));
        assert_eq!(
            errs,
            [max(RuleId::ATT_02, "request", 26_214_400, usize::MAX)]
        );
    }

    #[test]
    fn att_03_att_01_att_06_att_02_order() {
        let alt = "a".repeat(1025);
        let mut files = vec![png(1); 11];
        files[0] = png(26_214_401).description(&alt);
        let rules: Vec<RuleId> = check(&files, 0, None).iter().map(|v| v.rule).collect();
        assert_eq!(
            rules,
            [
                RuleId::ATT_03,
                RuleId::ATT_01,
                RuleId::ATT_06,
                RuleId::ATT_02
            ]
        );
    }

    #[test]
    fn att_06_rendered_image_alt_missing() {
        let l = Limits::default();
        let miss = |alt: &str| {
            vec![Violation::new(
                RuleId::ATT_06,
                "",
                Bound::Describe("alt text on a rendered image (FR-30)"),
                Actual::Text(alt.to_owned()),
            )]
        };
        assert_eq!(validate_rendered_image_alt(None, &l), miss(""));
        assert_eq!(validate_rendered_image_alt(Some(""), &l), miss(""));
        assert_eq!(validate_rendered_image_alt(Some(" \n"), &l), miss(" \n"));
    }

    #[test]
    fn att_06_rendered_image_alt_present() {
        let l = Limits::default();
        assert!(validate_rendered_image_alt(Some("a"), &l).is_empty());
        assert!(validate_rendered_image_alt(Some(" a "), &l).is_empty());
    }

    #[test]
    fn att_05_attachment_refs() {
        let l = Limits::default();
        let a = [png(1)];
        let b = [AttachmentDecl::new("b.png", 1)];
        let miss = |url: &str| {
            vec![Violation::new(
                RuleId::ATT_05,
                "",
                Bound::Describe("attachment://<name> of a file in this message"),
                Actual::Text(url.to_owned()),
            )]
        };
        assert!(validate_attachment_ref("attachment://a.png", &a, &l).is_empty());
        assert_eq!(
            validate_attachment_ref("attachment://a.png", &b, &l),
            miss("attachment://a.png")
        );
        assert_eq!(
            validate_attachment_ref("attachment://", &a, &l),
            miss("attachment://")
        );
        assert_eq!(
            validate_attachment_ref("attachment://A.png", &a, &l),
            miss("attachment://A.png")
        );
        assert!(validate_attachment_ref("https://x/y.png", &a, &l).is_empty());
        assert!(validate_attachment_ref("ATTACHMENT://a.png", &a, &l).is_empty());
    }
}
