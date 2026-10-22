//! Lowering a typed tree into `noctis_rules::component` descriptors, so the
//! runtime validator and `components!` count, name paths and order
//! violations the same way.
//!
//! The descriptors borrow nested slices, so the lists are built bottom-up in
//! local `Vec`s, one per nesting level, each by its own pre-order pass:
//! Section texts, String Select options and gallery media first, then Action
//! Row items, then Container children, then the top level. Each pass hands
//! the lists of the level below out through a cursor in the same pre-order.

use std::borrow::Cow;
use std::slice;

use noctis_rules::component as rc;
use noctis_rules::modal as rm;

use crate::component::{
    AccessoryNode, ActionRow, Button, ButtonNode, ChildNode, Container, ContainerComponent,
    RowNode, SelectNode, TopNode,
};
use crate::modal::{ModalInputNode, ModalSelectNode, ModalTopNode};
use crate::{ButtonStyle, Components, Media, Modal, ModalOption, SelectOption, Spacing, Text};

/// Lowers a V2 tree and runs `f` on it.
pub(super) fn with_v2<R>(components: &Components, f: impl FnOnce(&[rc::Component<'_>]) -> R) -> R {
    let items = &components.items;
    let nodes = child_nodes(&components.items);

    // Level 1: leaves.
    let mut texts = Vec::new();
    let mut media = Vec::new();
    for node in &nodes {
        match &node.0 {
            ChildNode::Section(s) => {
                texts.push(s.texts.iter().map(|t| &*t.content).collect::<Vec<_>>());
            }
            ChildNode::Gallery(g) => media.push(
                g.items
                    .iter()
                    .map(|i| lower_media(&i.media, i.alt.as_deref(), i.spoiler))
                    .collect::<Vec<_>>(),
            ),
            _ => {}
        }
    }
    let row_nodes: Vec<&ActionRow> = nodes
        .iter()
        .filter_map(|n| match &n.0 {
            ChildNode::Row(r) => Some(r),
            _ => None,
        })
        .collect();
    let options = collect_options(&row_nodes);

    // Level 2: Action Row items.
    let rows = lower_rows(&row_nodes, &options);

    // Level 3: Container children and the other top-level nodes.
    let mut cursor = Cursor {
        texts: texts.iter(),
        media: media.iter(),
        rows: rows.iter(),
    };
    let mut containers = Vec::new();
    let mut slots = Vec::with_capacity(items.len());
    for item in items {
        match &item.0 {
            TopNode::Container(c) => {
                containers.push(
                    c.children
                        .iter()
                        .map(|child| lower_child(child, &mut cursor))
                        .collect::<Vec<_>>(),
                );
                slots.push(Slot::Container(c));
            }
            TopNode::Child(child) => slots.push(Slot::Done(lower_child(child, &mut cursor))),
        }
    }

    // Level 4: the top level.
    let mut boxes = containers.iter();
    let top: Vec<rc::Component<'_>> = slots
        .into_iter()
        .map(|slot| match slot {
            Slot::Done(node) => node,
            Slot::Container(c) => rc::Component::Container(lower_container(c, take(&mut boxes))),
        })
        .collect();
    f(&top)
}

/// Lowers legacy Action Rows and runs `f` on them.
pub(super) fn with_legacy<R>(rows: &[ActionRow], f: impl FnOnce(&[rc::Component<'_>]) -> R) -> R {
    let row_nodes: Vec<&ActionRow> = rows.iter().collect();
    let options = collect_options(&row_nodes);
    let lowered = lower_rows(&row_nodes, &options);
    let top: Vec<rc::Component<'_>> = lowered
        .iter()
        .map(|items| rc::Component::Row(items))
        .collect();
    f(&top)
}

/// A top-level node: lowered already, or a Container waiting for its
/// children.
enum Slot<'s> {
    Done(rc::Component<'s>),
    Container(&'s Container),
}

/// The lists of the level below, handed out in pre-order.
struct Cursor<'s> {
    texts: slice::Iter<'s, Vec<&'s str>>,
    media: slice::Iter<'s, Vec<rc::Media<'s>>>,
    rows: slice::Iter<'s, Vec<rc::Component<'s>>>,
}

/// The next list. The passes visit the same nodes in the same order, so it
/// always exists; an empty slice keeps a mismatch from panicking.
fn take<'s, T>(lists: &mut slice::Iter<'s, Vec<T>>) -> &'s [T] {
    let next = lists.next();
    debug_assert!(next.is_some(), "lowering passes disagree on the pre-order");
    next.map_or(&[], Vec::as_slice)
}

/// Every Container-level node (top-level children and Container children),
/// in pre-order.
fn child_nodes(items: &[crate::TopLevelComponent]) -> Vec<&ContainerComponent> {
    let mut out = Vec::new();
    for item in items {
        match &item.0 {
            TopNode::Container(c) => out.extend(&c.children),
            TopNode::Child(child) => out.push(child),
        }
    }
    out
}

/// A String Select's lowered options (their `default` flags decided by its
/// `default_values` list) and that list.
struct StringParts<'t> {
    options: Vec<rc::SelectOption<'t>>,
    defaults: Option<Vec<&'t str>>,
}

impl<'t> StringParts<'t> {
    fn of(options: &'t [SelectOption], list: Option<&'t [Cow<'static, str>]>) -> Self {
        Self {
            options: options
                .iter()
                .map(|o| lower_option(o, o.is_default(list)))
                .collect(),
            defaults: list.map(|l| l.iter().map(|v| &**v).collect()),
        }
    }
}

/// The options of every String Select of `rows`, in order.
fn collect_options<'t>(rows: &[&'t ActionRow]) -> Vec<StringParts<'t>> {
    rows.iter()
        .filter_map(|r| match &r.0 {
            RowNode::Select(SelectNode::String(s)) => {
                Some(StringParts::of(&s.options, s.default_values.as_deref()))
            }
            _ => None,
        })
        .collect()
}

/// The items of every row, in order.
fn lower_rows<'s>(
    rows: &[&'s ActionRow],
    options: &'s [StringParts<'s>],
) -> Vec<Vec<rc::Component<'s>>> {
    let mut options = options.iter();
    rows.iter()
        .map(|r| match &r.0 {
            RowNode::Buttons(buttons) => buttons
                .iter()
                .map(|b| rc::Component::Button(lower_button(b)))
                .collect(),
            RowNode::Select(s) => vec![rc::Component::Select(lower_select(s, &mut options))],
        })
        .collect()
}

fn lower_child<'s>(child: &'s ContainerComponent, cursor: &mut Cursor<'s>) -> rc::Component<'s> {
    match &child.0 {
        ChildNode::Section(s) => {
            let accessory = match &s.accessory {
                AccessoryNode::Button(b) => rc::Accessory::Button(lower_button(b)),
                AccessoryNode::Thumbnail(t) => {
                    rc::Accessory::Thumbnail(lower_media(&t.media, t.alt.as_deref(), t.spoiler))
                }
            };
            rc::Component::Section(rc::Section::new(take(&mut cursor.texts), accessory))
        }
        ChildNode::Text(t) => rc::Component::Text(&t.content),
        ChildNode::Gallery(_) => rc::Component::Gallery(take(&mut cursor.media)),
        ChildNode::File(f) => rc::Component::File(f.attachment().url()),
        ChildNode::Separator(s) => {
            rc::Component::Separator(rc::Separator::new().divider(s.divider).spacing(
                match s.spacing {
                    Spacing::Small => rc::Spacing::Small,
                    Spacing::Large => rc::Spacing::Large,
                },
            ))
        }
        ChildNode::Row(_) => rc::Component::Row(take(&mut cursor.rows)),
        ChildNode::Raw(r) => rc::Component::Opaque(r.count()),
    }
}

fn lower_container<'s>(c: &'s Container, children: &'s [rc::Component<'s>]) -> rc::Container<'s> {
    let out = rc::Container::new(children).spoiler(c.spoiler);
    match c.accent {
        Some(accent) => out.accent_color(accent.value()),
        None => out,
    }
}

fn lower_media<'s>(media: &'s Media, alt: Option<&'s str>, spoiler: bool) -> rc::Media<'s> {
    let out = rc::Media::new(media.as_str()).spoiler(spoiler);
    match alt {
        Some(alt) => out.description(alt),
        None => out,
    }
}

fn lower_button(button: &Button) -> rc::Button<'_> {
    let (style, label, emoji) = match &button.0 {
        ButtonNode::Route(d) => {
            let style = match d.style {
                ButtonStyle::Primary => rc::ButtonStyle::Primary,
                ButtonStyle::Secondary => rc::ButtonStyle::Secondary,
                ButtonStyle::Success => rc::ButtonStyle::Success,
                ButtonStyle::Danger => rc::ButtonStyle::Danger,
            };
            (style, d.label.as_ref().map(Text::as_str), d.emoji.is_some())
        }
        ButtonNode::Link(l) => (
            rc::ButtonStyle::Link(&l.url),
            l.label.as_deref(),
            l.emoji.is_some(),
        ),
        ButtonNode::Premium(_) => (rc::ButtonStyle::Premium, None, false),
    };
    let out = rc::Button::new(style).emoji(emoji);
    match label {
        Some(label) => out.label(label),
        None => out,
    }
}

fn lower_select<'s>(
    select: &'s SelectNode,
    options: &mut slice::Iter<'s, StringParts<'s>>,
) -> rc::Select<'s> {
    let mut defaults = None;
    let kind = match select {
        SelectNode::String(_) => {
            let next = options.next();
            debug_assert!(next.is_some(), "lowering passes disagree on the pre-order");
            defaults = next.and_then(|p| p.defaults.as_deref());
            rc::SelectKind::String(next.map_or(&[], |p| p.options.as_slice()))
        }
        SelectNode::User(_) => rc::SelectKind::User,
        SelectNode::Role(_) => rc::SelectKind::Role,
        SelectNode::Mentionable(_) => rc::SelectKind::Mentionable,
        SelectNode::Channel(_) => rc::SelectKind::Channel,
    };
    let common = select.common();
    let mut out = rc::Select::new(kind)
        .default_values(select.default_count())
        .disabled(common.disabled);
    if let Some(list) = defaults {
        out = out.string_defaults(list);
    }
    if let Some(p) = common.placeholder.as_ref() {
        out = out.placeholder(p.as_str());
    }
    if let Some(min) = common.min_values {
        out = out.min_values(usize::from(min));
    }
    if let Some(max) = common.max_values {
        out = out.max_values(usize::from(max));
    }
    out
}

fn lower_option(option: &SelectOption, default: bool) -> rc::SelectOption<'_> {
    let out = rc::SelectOption::new(&option.label, &option.value).default(default);
    match option.description.as_deref() {
        Some(d) => out.description(d),
        None => out,
    }
}

/// Lowers a modal into `noctis_rules::modal` descriptors and runs `f` on it.
///
/// Level 1 collects, in pre-order, the String Select options, the Radio and
/// Checkbox Group options and the File Upload types; level 2 builds the
/// Labels, borrowing them through cursors in the same order.
pub(super) fn with_modal<R>(modal: &Modal, f: impl FnOnce(&rm::Modal<'_>) -> R) -> R {
    let inputs = || {
        modal.components.iter().filter_map(|c| match &c.0 {
            ModalTopNode::Label(l) => Some(&l.input),
            ModalTopNode::Text(_) => None,
        })
    };

    // Level 1: leaves.
    let mut select_options = Vec::new();
    let mut modal_options = Vec::new();
    let mut file_types = Vec::new();
    for input in inputs() {
        match input {
            ModalInputNode::Select(ModalSelectNode::String(s)) => {
                select_options.push(
                    s.options
                        .iter()
                        .map(|o| lower_option(o, o.default))
                        .collect::<Vec<_>>(),
                );
            }
            ModalInputNode::RadioGroup(r) => modal_options.push(lower_modal_options(&r.options)),
            ModalInputNode::CheckboxGroup(g) => {
                modal_options.push(lower_modal_options(&g.options));
            }
            ModalInputNode::FileUpload(u) => {
                file_types.push(u.file_types.iter().map(|t| &**t).collect::<Vec<&str>>());
            }
            ModalInputNode::TextInput(_)
            | ModalInputNode::Select(
                ModalSelectNode::User(_)
                | ModalSelectNode::Role(_)
                | ModalSelectNode::Mentionable(_)
                | ModalSelectNode::Channel(_),
            )
            | ModalInputNode::Checkbox(_) => {}
        }
    }

    // Level 2: the top level.
    let mut cursor = ModalCursor {
        select_options: select_options.iter(),
        modal_options: modal_options.iter(),
        file_types: file_types.iter(),
    };
    let components: Vec<rm::ModalComponent<'_>> = modal
        .components
        .iter()
        .map(|c| match &c.0 {
            ModalTopNode::Text(t) => rm::ModalComponent::Text(&t.content),
            ModalTopNode::Label(l) => {
                let label = rm::Label::new(l.label.as_str(), lower_input(&l.input, &mut cursor));
                rm::ModalComponent::Label(match l.description.as_ref() {
                    Some(d) => label.description(d.as_str()),
                    None => label,
                })
            }
        })
        .collect();
    f(&rm::Modal::new(modal.title.as_str(), &components))
}

/// The leaf lists of a modal, handed out in pre-order.
struct ModalCursor<'s> {
    select_options: slice::Iter<'s, Vec<rc::SelectOption<'s>>>,
    modal_options: slice::Iter<'s, Vec<rm::ModalOption<'s>>>,
    file_types: slice::Iter<'s, Vec<&'s str>>,
}

fn lower_input<'s>(input: &'s ModalInputNode, cursor: &mut ModalCursor<'s>) -> rm::LabelChild<'s> {
    match input {
        ModalInputNode::TextInput(t) => {
            let mut out = rm::TextInput::new(t.style).required(t.required);
            if let Some(min) = t.min_length {
                out = out.min_length(usize::from(min));
            }
            if let Some(max) = t.max_length {
                out = out.max_length(usize::from(max));
            }
            if let Some(v) = t.value.as_deref() {
                out = out.value(v);
            }
            if let Some(p) = t.placeholder.as_ref() {
                out = out.placeholder(p.as_str());
            }
            rm::LabelChild::TextInput(out)
        }
        ModalInputNode::Select(s) => {
            let kind = match s {
                ModalSelectNode::String(_) => {
                    rc::SelectKind::String(take(&mut cursor.select_options))
                }
                ModalSelectNode::User(_) => rc::SelectKind::User,
                ModalSelectNode::Role(_) => rc::SelectKind::Role,
                ModalSelectNode::Mentionable(_) => rc::SelectKind::Mentionable,
                ModalSelectNode::Channel(_) => rc::SelectKind::Channel,
            };
            let common = s.common();
            let mut select = rc::Select::new(kind)
                .default_values(s.default_count())
                .disabled(false);
            if let Some(p) = common.placeholder.as_ref() {
                select = select.placeholder(p.as_str());
            }
            if let Some(min) = common.min_values {
                select = select.min_values(usize::from(min));
            }
            if let Some(max) = common.max_values {
                select = select.max_values(usize::from(max));
            }
            rm::LabelChild::Select(rm::ModalSelect::new(select).required(common.required))
        }
        ModalInputNode::FileUpload(u) => {
            let mut out = rm::FileUpload::new()
                .required(u.required)
                .file_types(take(&mut cursor.file_types));
            if let Some(min) = u.min_values {
                out = out.min_values(usize::from(min));
            }
            if let Some(max) = u.max_values {
                out = out.max_values(usize::from(max));
            }
            rm::LabelChild::FileUpload(out)
        }
        ModalInputNode::RadioGroup(r) => rm::LabelChild::RadioGroup(
            rm::RadioGroup::new(take(&mut cursor.modal_options)).required(r.required),
        ),
        ModalInputNode::CheckboxGroup(g) => {
            let mut out =
                rm::CheckboxGroup::new(take(&mut cursor.modal_options)).required(g.required);
            if let Some(min) = g.min_values {
                out = out.min_values(usize::from(min));
            }
            if let Some(max) = g.max_values {
                out = out.max_values(usize::from(max));
            }
            rm::LabelChild::CheckboxGroup(out)
        }
        // A Checkbox is never required (MOD-07 by construction).
        ModalInputNode::Checkbox(c) => {
            rm::LabelChild::Checkbox(rm::Checkbox::new().default(c.default))
        }
    }
}

fn lower_modal_options(options: &[ModalOption]) -> Vec<rm::ModalOption<'_>> {
    options
        .iter()
        .map(|o| {
            let out = rm::ModalOption::new(&o.label, &o.value).default(o.default);
            match o.description.as_deref() {
                Some(d) => out.description(d),
                None => out,
            }
        })
        .collect()
}
