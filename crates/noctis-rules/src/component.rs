//! Components V2 trees and legacy Action Rows: [`validate_components`]
//! (CV2-03..13, CV2-16, CV2-17), [`validate_legacy_components`] (CV2-15),
//! [`cost`], and granular functions for single literals.
//!
//! `components!` (lowering literal syntax) and the runtime validator (lowering
//! a `noctis-ui` tree) both describe a message as a `&[`[`Component`]`]` and call
//! these functions, so the two count, name paths and order violations the same
//! way. The descriptors carry no `custom_id` or numeric `id`: CID-01, CID-02 and
//! CV2-14 belong to Routes and the runtime validator.
//!
//! # Paths
//!
//! A top-level node `i` is `kind[i]`; a child is `parent.kind[j]`, where `j` is
//! its position among all its siblings. The kinds are [`Component::kind`]:
//! `container`, `section`, `text`, `thumbnail`, `gallery`, `file`, `separator`,
//! `row`, `button`, `select`, `opaque`. A Section's accessory is
//! `P.accessory`, a gallery item `P.item[k]` and a select option
//! `P.options[k]`; fields append `.label`, `.url`, `.emoji`, `.placeholder`,
//! `.min_values`, `.max_values`, `.options`, `.default_values`,
//! `.description` or `.accent_color`. Child counts use the node path itself.
//! Message-wide rules (CV2-03, CV2-04, CV2-15 count) use the path `components`.
//!
//! # Rules
//!
//! - CV2-03: at most `components` nodes at every depth. Section texts and
//!   accessories count; select options and gallery items do not.
//! - CV2-04: the [`discord_len`] sum of every Text Display (top-level, nested,
//!   Section texts, misplaced ones too) is at most `text_total_len`.
//! - CV2-05: placement. Top level and Container children: Container (top level
//!   only), Section, Text Display, Media Gallery, File, Separator, Action Row;
//!   a Thumbnail only as a Section accessory; a Button only in an Action Row or
//!   as an accessory; a select only in an Action Row; only buttons and selects
//!   in an Action Row.
//! - CV2-06: an Action Row holds 1 to `row_buttons` items, or exactly one
//!   select alone.
//! - CV2-07: a Section holds 1 to `section_texts` texts and one accessory.
//! - CV2-08: button label length, link URL length (1 to `link_url_len`), no
//!   label or emoji on a premium button.
//! - CV2-09: select placeholder, `min_values` / `max_values` ranges and order,
//!   String Select options (1 to `select_options`, label / value 1-100,
//!   description ≤ 100), and the default count within `[min, max]`. Effective
//!   defaults are min 1 and max 1; the order is checked only when both
//!   effective values are in range, and defaults only when the order passed.
//!   `disabled` is allowed in messages.
//! - CV2-10: 1 to `gallery_items` gallery items; item description length.
//! - CV2-11: Thumbnail description length.
//! - CV2-12: a File component's URL is `attachment://<filename>`.
//! - CV2-13: `accent_color` in `0x000000..=0xFFFFFF`; Separator spacing 1 or 2
//!   ([`Spacing`] makes other values unrepresentable; [`validate_spacing`]
//!   checks a raw value).
//! - CV2-15: a legacy message holds up to `legacy_action_rows` top-level
//!   Action Rows and nothing else.
//! - CV2-16: [`Component::Opaque`]`(n)` stands for `n` raw (unknown) components
//!   or a `components!` `{expr}` hole; it counts `max(n, 1)` against CV2-03, is
//!   allowed at any position and counts as one Action Row item.
//! - CV2-17: Media Gallery item and Thumbnail URL length (1 to
//!   `media_url_len`).
//!
//! CV2-05, CV2-06, CV2-07 and CV2-10 are also enforced by the types in
//! `noctis-ui`; the checks here serve trees built from literals and raw input.
//! The functions only report: they never truncate. Lenient handling stays with
//! the caller through [`Limits::truncates`].
//!
//! A Section always has an accessory, and links take no `custom_id`:
//!
//! ```compile_fail
//! use noctis_rules::component::Section;
//!
//! let _ = Section::new(&["a"]); // [CV2-07]
//! ```
//!
//! ```compile_fail
//! use noctis_rules::component::{Button, ButtonStyle};
//!
//! let _ = Button::new(ButtonStyle::Link("u")).custom_id("x"); // [CV2-08]
//! ```
//!
//! ```compile_fail
//! use noctis_rules::component::Spacing;
//!
//! let _ = Spacing::Medium; // [CV2-13]
//! ```

use crate::check::{at_most, between, finish, join, text_at_most, text_between};
use crate::len::discord_len;
use crate::{Actual, Bound, Limits, RuleId, Violation};

const ACCENT_MAX: u32 = 0x00FF_FFFF;
const ACCENT_RANGE: &str = "a colour from 0x000000 to 0xFFFFFF";
const ATTACHMENT_SCHEME: &str = "attachment://";

/// One node of a Components V2 (or legacy) message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Component<'a> {
    /// A Container (type 17).
    Container(Container<'a>),
    /// A Section (type 9).
    Section(Section<'a>),
    /// A Text Display (type 10) with its content.
    Text(&'a str),
    /// A Thumbnail (type 11); valid only as a Section accessory.
    Thumbnail(Media<'a>),
    /// A Media Gallery (type 12) with its items.
    Gallery(&'a [Media<'a>]),
    /// A File (type 13) with its `attachment://` URL.
    File(&'a str),
    /// A Separator (type 14).
    Separator(Separator),
    /// An Action Row (type 1) with its items.
    Row(&'a [Component<'a>]),
    /// A Button (type 2).
    Button(Button<'a>),
    /// A select menu (types 3, 5, 6, 7, 8).
    Select(Select<'a>),
    /// `n` raw components or a macro hole (CV2-16); counts `max(n, 1)`.
    Opaque(usize),
}

impl Component<'_> {
    /// The path name of this node: `container`, `section`, `text`,
    /// `thumbnail`, `gallery`, `file`, `separator`, `row`, `button`, `select`
    /// or `opaque`.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Container(_) => "container",
            Self::Section(_) => "section",
            Self::Text(_) => "text",
            Self::Thumbnail(_) => "thumbnail",
            Self::Gallery(_) => "gallery",
            Self::File(_) => "file",
            Self::Separator(_) => "separator",
            Self::Row(_) => "row",
            Self::Button(_) => "button",
            Self::Select(_) => "select",
            Self::Opaque(_) => "opaque",
        }
    }
}

/// A Container: children, an optional accent colour and a spoiler flag.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Container<'a> {
    /// The children.
    pub children: &'a [Component<'a>],
    /// The accent colour as `0xRRGGBB` (CV2-13).
    pub accent_color: Option<u32>,
    /// Whether the container is a spoiler.
    pub spoiler: bool,
}

impl<'a> Container<'a> {
    /// A container without accent colour or spoiler.
    pub const fn new(children: &'a [Component<'a>]) -> Self {
        Self {
            children,
            accent_color: None,
            spoiler: false,
        }
    }

    /// Sets the accent colour.
    #[must_use]
    pub const fn accent_color(self, color: u32) -> Self {
        Self {
            accent_color: Some(color),
            ..self
        }
    }

    /// Sets the spoiler flag.
    #[must_use]
    pub const fn spoiler(self, spoiler: bool) -> Self {
        Self { spoiler, ..self }
    }
}

/// A Section: 1-3 Text Displays and one accessory (CV2-07).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section<'a> {
    /// The Text Display contents.
    pub texts: &'a [&'a str],
    /// The accessory.
    pub accessory: Accessory<'a>,
}

impl<'a> Section<'a> {
    /// A section with its texts and accessory.
    pub const fn new(texts: &'a [&'a str], accessory: Accessory<'a>) -> Self {
        Self { texts, accessory }
    }
}

/// A Section accessory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Accessory<'a> {
    /// A button.
    Button(Button<'a>),
    /// A Thumbnail.
    Thumbnail(Media<'a>),
}

/// A Media Gallery item or a Thumbnail: URL, description and spoiler flag.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Media<'a> {
    /// The media URL (CV2-17).
    pub url: &'a str,
    /// The alt text (CV2-10 for gallery items, CV2-11 for thumbnails).
    pub description: Option<&'a str>,
    /// Whether the media is a spoiler.
    pub spoiler: bool,
}

impl<'a> Media<'a> {
    /// Media without description or spoiler.
    pub const fn new(url: &'a str) -> Self {
        Self {
            url,
            description: None,
            spoiler: false,
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

    /// Sets the spoiler flag.
    #[must_use]
    pub const fn spoiler(self, spoiler: bool) -> Self {
        Self { spoiler, ..self }
    }
}

/// A Separator: divider line and spacing.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Separator {
    /// Whether a divider line is drawn.
    pub divider: bool,
    /// The padding size.
    pub spacing: Spacing,
}

impl Separator {
    /// A separator with a divider and small spacing.
    pub const fn new() -> Self {
        Self {
            divider: true,
            spacing: Spacing::Small,
        }
    }

    /// Sets whether a divider line is drawn.
    #[must_use]
    pub const fn divider(self, divider: bool) -> Self {
        Self { divider, ..self }
    }

    /// Sets the spacing.
    #[must_use]
    pub const fn spacing(self, spacing: Spacing) -> Self {
        Self { spacing, ..self }
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}

/// Separator padding (CV2-13: only 1 or 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Spacing {
    /// Small padding (1). The default.
    #[default]
    Small,
    /// Large padding (2).
    Large,
}

impl Spacing {
    /// The Discord value: 1 or 2.
    pub const fn value(self) -> u8 {
        match self {
            Self::Small => 1,
            Self::Large => 2,
        }
    }
}

/// A button style; a link carries its URL.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonStyle<'a> {
    /// Primary (1).
    Primary,
    /// Secondary (2).
    Secondary,
    /// Success (3).
    Success,
    /// Danger (4).
    Danger,
    /// Link (5) with its URL (CV2-08).
    Link(&'a str),
    /// Premium (6): no label, no emoji (CV2-08).
    Premium,
}

impl ButtonStyle<'_> {
    /// The Discord style value (1..=6).
    pub const fn value(self) -> u8 {
        match self {
            Self::Primary => 1,
            Self::Secondary => 2,
            Self::Success => 3,
            Self::Danger => 4,
            Self::Link(_) => 5,
            Self::Premium => 6,
        }
    }
}

/// A button: style, optional label and whether it has an emoji.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button<'a> {
    /// The style (and URL for links).
    pub style: ButtonStyle<'a>,
    /// The label (CV2-08).
    pub label: Option<&'a str>,
    /// Whether the button has an emoji.
    pub emoji: bool,
}

impl<'a> Button<'a> {
    /// A button without label or emoji.
    pub const fn new(style: ButtonStyle<'a>) -> Self {
        Self {
            style,
            label: None,
            emoji: false,
        }
    }

    /// Sets the label.
    #[must_use]
    pub const fn label(self, label: &'a str) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    /// Sets whether the button has an emoji.
    #[must_use]
    pub const fn emoji(self, emoji: bool) -> Self {
        Self { emoji, ..self }
    }
}

/// The kind of a select menu; a String Select carries its options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectKind<'a> {
    /// String Select (3) with its options.
    String(&'a [SelectOption<'a>]),
    /// User Select (5).
    User,
    /// Role Select (6).
    Role,
    /// Mentionable Select (7).
    Mentionable,
    /// Channel Select (8).
    Channel,
}

/// A select menu (CV2-09).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Select<'a> {
    /// The kind (and options of a String Select).
    pub kind: SelectKind<'a>,
    /// The placeholder.
    pub placeholder: Option<&'a str>,
    /// `min_values` (Discord's default: 1).
    pub min_values: Option<usize>,
    /// `max_values` (Discord's default: 1).
    pub max_values: Option<usize>,
    /// The number of `default_values` (user, role, mentionable and channel
    /// selects only).
    pub default_values: usize,
    /// Whether the select is disabled (messages only, MOD-03).
    pub disabled: bool,
}

impl<'a> Select<'a> {
    /// A select with everything else unset.
    pub const fn new(kind: SelectKind<'a>) -> Self {
        Self {
            kind,
            placeholder: None,
            min_values: None,
            max_values: None,
            default_values: 0,
            disabled: false,
        }
    }

    /// Sets the placeholder.
    #[must_use]
    pub const fn placeholder(self, placeholder: &'a str) -> Self {
        Self {
            placeholder: Some(placeholder),
            ..self
        }
    }

    /// Sets `min_values`.
    #[must_use]
    pub const fn min_values(self, min: usize) -> Self {
        Self {
            min_values: Some(min),
            ..self
        }
    }

    /// Sets `max_values`.
    #[must_use]
    pub const fn max_values(self, max: usize) -> Self {
        Self {
            max_values: Some(max),
            ..self
        }
    }

    /// Sets the number of `default_values`.
    #[must_use]
    pub const fn default_values(self, count: usize) -> Self {
        Self {
            default_values: count,
            ..self
        }
    }

    /// Sets whether the select is disabled.
    #[must_use]
    pub const fn disabled(self, disabled: bool) -> Self {
        Self { disabled, ..self }
    }
}

/// One String Select option.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectOption<'a> {
    /// The label (1-100).
    pub label: &'a str,
    /// The value (1-100).
    pub value: &'a str,
    /// The description (≤ 100).
    pub description: Option<&'a str>,
    /// Whether the option is selected by default.
    pub default: bool,
}

impl<'a> SelectOption<'a> {
    /// An option without description, not a default.
    pub const fn new(label: &'a str, value: &'a str) -> Self {
        Self {
            label,
            value,
            description: None,
            default: false,
        }
    }

    /// Sets the description.
    #[must_use]
    pub const fn description(self, description: &'a str) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }

    /// Sets whether the option is selected by default.
    #[must_use]
    pub const fn default(self, default: bool) -> Self {
        Self { default, ..self }
    }
}

/// The CV2-03 and CV2-04 budget a tree uses, from [`cost`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cost {
    /// Components at every depth (CV2-03).
    pub components: usize,
    /// The [`discord_len`] sum of every Text Display (CV2-04).
    pub text_len: usize,
}

/// The CV2-03 count of one node and its subtree (saturating).
fn node_count(node: &Component<'_>) -> usize {
    match node {
        Component::Container(c) => children_count(c.children).saturating_add(1),
        Component::Section(s) => s.texts.len().saturating_add(2),
        Component::Row(items) => children_count(items).saturating_add(1),
        Component::Opaque(n) => (*n).max(1),
        Component::Text(_)
        | Component::Thumbnail(_)
        | Component::Gallery(_)
        | Component::File(_)
        | Component::Separator(_)
        | Component::Button(_)
        | Component::Select(_) => 1,
    }
}

fn children_count(nodes: &[Component<'_>]) -> usize {
    nodes
        .iter()
        .fold(0, |acc: usize, n| acc.saturating_add(node_count(n)))
}

/// The CV2-04 text length of one node and its subtree (saturating).
fn node_text_len(node: &Component<'_>) -> usize {
    match node {
        Component::Text(t) => discord_len(t),
        Component::Container(c) => children_text_len(c.children),
        Component::Row(items) => children_text_len(items),
        Component::Section(s) => s
            .texts
            .iter()
            .fold(0, |acc: usize, t| acc.saturating_add(discord_len(t))),
        _ => 0,
    }
}

fn children_text_len(nodes: &[Component<'_>]) -> usize {
    nodes
        .iter()
        .fold(0, |acc: usize, n| acc.saturating_add(node_text_len(n)))
}

/// The CV2-03 component count and CV2-04 text length of a tree.
///
/// Every node counts once at every depth, a Section's texts and accessory
/// included; [`Component::Opaque`]`(n)` counts `max(n, 1)`; select options and
/// gallery items do not count. Both sums saturate.
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_rules::component::{Component, Container, cost};
///
/// let inner = [Component::Text("ab")];
/// let tree = [Component::Text("😀"), Component::Container(Container::new(&inner))];
/// let c = cost(&tree, &Limits::default());
/// assert_eq!((c.components, c.text_len), (3, 4));
/// ```
pub fn cost(components: &[Component<'_>], limits: &Limits) -> Cost {
    let _ = limits;
    Cost {
        components: children_count(components),
        text_len: children_text_len(components),
    }
}

/// Where a node sits, for CV2-05.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Top,
    Container,
    Row,
}

/// Checks the components of a Components V2 message.
///
/// Paths and rules are described in the [module docs](self). The order is
/// CV2-03 (with one `kind[i]: n` note per top-level node), CV2-04, then a
/// pre-order walk; per node its CV2-05 placement check, then its own checks:
/// Container: CV2-13, then the children; Section: CV2-07, then the accessory;
/// Gallery: CV2-10 count, then per item the URL (CV2-17) and description
/// (CV2-10); File: CV2-12; Row: CV2-06, then the items; Button: label (length,
/// or premium label), URL (link), emoji (premium); Select: placeholder,
/// `min_values`, `max_values`, order, options count, per option label, value
/// and description, default values; Thumbnail (misplaced): URL, description.
/// Every node's fields are checked wherever it sits.
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::component::{Component, validate_components};
///
/// let tree = vec![Component::Text("a"); 41];
/// let errs = validate_components(&tree, &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::CV2_03);
/// assert_eq!(errs[0].path, "components");
/// ```
pub fn validate_components(
    components: &[Component<'_>],
    limits: &Limits,
) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    let total = children_count(components);
    if total > limits.components {
        let v = components.iter().enumerate().fold(
            Violation::new(
                RuleId::CV2_03,
                "components",
                Bound::AtMost(limits.components),
                Actual::Count(total),
            ),
            |v, (i, node)| v.with_note(format!("{}[{i}]: {}", node.kind(), node_count(node))),
        );
        out.push(v);
    }
    at_most(
        RuleId::CV2_04,
        "components",
        limits.text_total_len,
        children_text_len(components),
        &mut out,
    );
    for (i, node) in components.iter().enumerate() {
        check_node(
            node,
            &format!("{}[{i}]", node.kind()),
            Place::Top,
            limits,
            &mut out,
        );
    }
    finish(out)
}

/// Checks the components of a legacy (non-V2) message (CV2-15).
///
/// The order is the CV2-15 count (path `components`), then per top-level node
/// either the CV2-15 kind violation (only Action Rows and
/// [`Component::Opaque`], which counts as one row; children are not walked) or
/// the row's checks as in [`validate_components`]. CV2-03 and CV2-04 are never
/// checked for legacy messages.
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::component::{Component, validate_legacy_components};
///
/// let errs = validate_legacy_components(&[Component::Text("x")], &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::CV2_15);
/// assert_eq!(errs[0].path, "text[0]");
/// ```
pub fn validate_legacy_components(
    components: &[Component<'_>],
    limits: &Limits,
) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    at_most(
        RuleId::CV2_15,
        "components",
        limits.legacy_action_rows,
        components.len(),
        &mut out,
    );
    for (i, node) in components.iter().enumerate() {
        let path = format!("{}[{i}]", node.kind());
        match node {
            Component::Row(_) => check_node(node, &path, Place::Top, limits, &mut out),
            Component::Opaque(_) => {}
            _ => out.push(Violation::new(
                RuleId::CV2_15,
                path,
                Bound::Describe("only Action Rows in a legacy message"),
                Actual::Text(node.kind().to_owned()),
            )),
        }
    }
    finish(out)
}

/// CV2-05 for `node` at `place`.
fn check_placement(node: &Component<'_>, path: &str, place: Place, out: &mut Vec<Violation>) {
    let describe = match (place, node) {
        (_, Component::Opaque(_)) | (Place::Row, Component::Button(_) | Component::Select(_)) => {
            None
        }
        (Place::Row, _) => Some("only buttons and selects in an Action Row"),
        (_, Component::Thumbnail(_)) => Some("a Thumbnail only as a Section accessory"),
        (_, Component::Button(_)) => {
            Some("a Button only in an Action Row or as a Section accessory")
        }
        (_, Component::Select(_)) => Some("a select only in an Action Row"),
        (Place::Container, Component::Container(_)) => Some("no Container inside a Container"),
        _ => None,
    };
    if let Some(describe) = describe {
        out.push(Violation::new(
            RuleId::CV2_05,
            path,
            Bound::Describe(describe),
            Actual::Text(node.kind().to_owned()),
        ));
    }
}

/// CV2-05, then the node's own checks and its children, in order.
fn check_node(
    node: &Component<'_>,
    path: &str,
    place: Place,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    check_placement(node, path, place, out);
    match node {
        Component::Container(c) => {
            if let Some(color) = c.accent_color {
                check_accent_color(color, &join(path, "accent_color"), out);
            }
            check_children(c.children, path, Place::Container, limits, out);
        }
        Component::Section(s) => {
            between(
                RuleId::CV2_07,
                path,
                1,
                limits.section_texts,
                s.texts.len(),
                out,
            );
            let accessory = join(path, "accessory");
            match &s.accessory {
                Accessory::Button(b) => check_button(b, &accessory, limits, out),
                Accessory::Thumbnail(m) => check_thumbnail(m, &accessory, limits, out),
            }
        }
        Component::Gallery(items) => {
            between(
                RuleId::CV2_10,
                path,
                1,
                limits.gallery_items,
                items.len(),
                out,
            );
            for (k, item) in items.iter().enumerate() {
                let item_path = format!("{path}.item[{k}]");
                check_media_url(item.url, &join(&item_path, "url"), limits, out);
                if let Some(d) = item.description {
                    text_at_most(
                        RuleId::CV2_10,
                        join(&item_path, "description"),
                        limits.gallery_description_len,
                        d,
                        out,
                    );
                }
            }
        }
        Component::File(url) => check_file_url(url, path, out),
        Component::Row(items) => {
            check_row_shape(items, path, limits, out);
            check_children(items, path, Place::Row, limits, out);
        }
        Component::Button(b) => check_button(b, path, limits, out),
        Component::Select(s) => check_select(s, path, limits, out),
        Component::Thumbnail(m) => check_thumbnail(m, path, limits, out),
        Component::Text(_) | Component::Separator(_) | Component::Opaque(_) => {}
    }
}

fn check_children(
    children: &[Component<'_>],
    parent: &str,
    place: Place,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    for (j, child) in children.iter().enumerate() {
        let path = format!("{parent}.{}[{j}]", child.kind());
        check_node(child, &path, place, limits, out);
    }
}

/// CV2-06: 1 to `row_buttons` items, or exactly one select alone.
fn check_row_shape(items: &[Component<'_>], path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let has_select = items.iter().any(|i| matches!(i, Component::Select(_)));
    if has_select && items.len() > 1 {
        out.push(Violation::new(
            RuleId::CV2_06,
            path,
            Bound::Describe("exactly one select, alone in its row"),
            Actual::Count(items.len()),
        ));
    } else {
        between(
            RuleId::CV2_06,
            path,
            1,
            limits.row_buttons,
            items.len(),
            out,
        );
    }
}

/// CV2-08: label (length, or premium label), URL (link), emoji (premium).
fn check_button(button: &Button<'_>, path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let premium = matches!(button.style, ButtonStyle::Premium);
    if let Some(label) = button.label {
        let label_path = join(path, "label");
        if premium {
            out.push(Violation::new(
                RuleId::CV2_08,
                label_path,
                Bound::Describe("no label on a premium button"),
                Actual::Text(label.to_owned()),
            ));
        } else {
            text_at_most(
                RuleId::CV2_08,
                label_path,
                limits.button_label_len,
                label,
                out,
            );
        }
    }
    if let ButtonStyle::Link(url) = button.style {
        text_between(
            RuleId::CV2_08,
            join(path, "url"),
            1,
            limits.link_url_len,
            url,
            out,
        );
    }
    if premium && button.emoji {
        out.push(Violation::new(
            RuleId::CV2_08,
            join(path, "emoji"),
            Bound::Describe("no emoji on a premium button"),
            Actual::Text("true".to_owned()),
        ));
    }
}

/// CV2-09 for a select at `path` (also used by modal selects).
///
/// Placeholder, `min_values`, `max_values`, order (only when both effective
/// values are in range), options count, per option label / value /
/// description, then the default count (only when the order check ran and
/// passed).
pub(crate) fn check_select(
    select: &Select<'_>,
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let max_values = limits.select_values;
    if let Some(p) = select.placeholder {
        text_at_most(
            RuleId::CV2_09,
            join(path, "placeholder"),
            limits.select_placeholder_len,
            p,
            out,
        );
    }
    if let Some(min) = select.min_values {
        between(
            RuleId::CV2_09,
            join(path, "min_values"),
            0,
            max_values,
            min,
            out,
        );
    }
    if let Some(max) = select.max_values {
        between(
            RuleId::CV2_09,
            join(path, "max_values"),
            1,
            max_values,
            max,
            out,
        );
    }
    let min = select.min_values.unwrap_or(1);
    let max = select.max_values.unwrap_or(1);
    let in_range = min <= max_values && (1..=max_values).contains(&max);
    if in_range && min > max {
        out.push(Violation::new(
            RuleId::CV2_09,
            path,
            Bound::Describe("min_values ≤ max_values"),
            Actual::Text(format!("{min} > {max}")),
        ));
    }
    let ordered = in_range && min <= max;
    if let SelectKind::String(options) = select.kind {
        check_select_options(options, path, limits, out);
    }
    if !ordered {
        return;
    }
    let defaults_path = join(path, "default_values");
    let defaults = match select.kind {
        SelectKind::String(options) => {
            if select.default_values > 0 {
                out.push(Violation::new(
                    RuleId::CV2_09,
                    defaults_path.as_str(),
                    Bound::Describe(
                        "default_values only on user, role, mentionable or channel selects",
                    ),
                    Actual::Count(select.default_values),
                ));
            }
            options.iter().filter(|o| o.default).count()
        }
        _ => select.default_values,
    };
    if defaults > 0 {
        between(RuleId::CV2_09, defaults_path, min, max, defaults, out);
    }
}

/// CV2-09: the String Select options count, then per option label, value and
/// description.
fn check_select_options(
    options: &[SelectOption<'_>],
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let options_path = join(path, "options");
    between(
        RuleId::CV2_09,
        options_path.as_str(),
        1,
        limits.select_options,
        options.len(),
        out,
    );
    for (k, option) in options.iter().enumerate() {
        let p = format!("{options_path}[{k}]");
        text_between(
            RuleId::CV2_09,
            join(&p, "label"),
            1,
            limits.select_option_label_len,
            option.label,
            out,
        );
        text_between(
            RuleId::CV2_09,
            join(&p, "value"),
            1,
            limits.select_option_value_len,
            option.value,
            out,
        );
        if let Some(d) = option.description {
            text_at_most(
                RuleId::CV2_09,
                join(&p, "description"),
                limits.select_option_description_len,
                d,
                out,
            );
        }
    }
}

/// CV2-17 URL, then CV2-11 description of a Thumbnail at `path`.
fn check_thumbnail(media: &Media<'_>, path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    check_media_url(media.url, &join(path, "url"), limits, out);
    if let Some(d) = media.description {
        text_at_most(
            RuleId::CV2_11,
            join(path, "description"),
            limits.thumbnail_description_len,
            d,
            out,
        );
    }
}

fn check_media_url(url: &str, path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    text_between(RuleId::CV2_17, path, 1, limits.media_url_len, url, out);
}

fn check_file_url(url: &str, path: &str, out: &mut Vec<Violation>) {
    let ok = url
        .strip_prefix(ATTACHMENT_SCHEME)
        .is_some_and(|name| !name.is_empty());
    if !ok {
        out.push(Violation::new(
            RuleId::CV2_12,
            path,
            Bound::Describe("an attachment://<filename> URL"),
            Actual::Text(url.to_owned()),
        ));
    }
}

fn check_accent_color(color: u32, path: &str, out: &mut Vec<Violation>) {
    if color > ACCENT_MAX {
        out.push(Violation::new(
            RuleId::CV2_13,
            path,
            Bound::Describe(ACCENT_RANGE),
            Actual::Text(format!("{color:#X}")),
        ));
    }
}

/// CV2-08: a button label of at most `button_label_len` characters (empty is
/// allowed). The path is empty.
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_rules::component::validate_button_label;
///
/// assert!(validate_button_label("Go", &Limits::default()).is_empty());
/// assert_eq!(validate_button_label(&"a".repeat(81), &Limits::default()).len(), 1);
/// ```
pub fn validate_button_label(label: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(RuleId::CV2_08, "", limits.button_label_len, label, &mut out);
    out
}

/// CV2-08: a link button URL of 1 to `link_url_len` characters. The path is
/// empty.
pub fn validate_link_url(url: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_between(RuleId::CV2_08, "", 1, limits.link_url_len, url, &mut out);
    out
}

/// CV2-09: a select placeholder of at most `select_placeholder_len`
/// characters. The path is empty.
pub fn validate_select_placeholder(placeholder: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(
        RuleId::CV2_09,
        "",
        limits.select_placeholder_len,
        placeholder,
        &mut out,
    );
    out
}

/// CV2-10: a Media Gallery item description of at most
/// `gallery_description_len` characters. The path is empty.
pub fn validate_gallery_description(description: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(
        RuleId::CV2_10,
        "",
        limits.gallery_description_len,
        description,
        &mut out,
    );
    out
}

/// CV2-11: a Thumbnail description of at most `thumbnail_description_len`
/// characters. The path is empty.
pub fn validate_thumbnail_description(description: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(
        RuleId::CV2_11,
        "",
        limits.thumbnail_description_len,
        description,
        &mut out,
    );
    out
}

/// CV2-17: a Media Gallery item or Thumbnail URL of 1 to `media_url_len`
/// characters. The path is empty.
pub fn validate_media_url(url: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    check_media_url(url, "", limits, &mut out);
    out
}

/// CV2-13: an accent colour from `0x000000` to `0xFFFFFF` (what `rgb!` calls).
/// The path is empty.
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_rules::component::validate_accent_color;
///
/// assert!(validate_accent_color(0x5865F2, &Limits::default()).is_empty());
/// let errs = validate_accent_color(0x1000000, &Limits::default());
/// assert_eq!(errs[0].to_string(), "[CV2-13] limit a colour from 0x000000 to 0xFFFFFF, actual `0x1000000`");
/// ```
pub fn validate_accent_color(color: u32, limits: &Limits) -> Vec<Violation> {
    let _ = limits;
    let mut out = Vec::new();
    check_accent_color(color, "", &mut out);
    out
}

/// CV2-13: a raw Separator spacing value of 1 (small) or 2 (large). The path
/// is empty.
pub fn validate_spacing(spacing: u8, limits: &Limits) -> Vec<Violation> {
    let _ = limits;
    if matches!(spacing, 1 | 2) {
        Vec::new()
    } else {
        vec![Violation::new(
            RuleId::CV2_13,
            "",
            Bound::Describe("1 (small) or 2 (large)"),
            Actual::Text(spacing.to_string()),
        )]
    }
}

#[cfg(test)]
mod tests;
