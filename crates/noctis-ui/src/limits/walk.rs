//! The pre-order walk behind the checks `noctis-rules` cannot make from
//! descriptors: `custom_id`s (CID-02), `attachment://` references (ATT-05)
//! and missing alt text.
//!
//! Paths use the kind names and positions of `noctis_rules::component`, so a
//! node gets the same path prefix here as in a rules violation.

use crate::component::{
    AccessoryNode, ActionRow, Button, ButtonNode, ChildNode, ContainerComponent, File, MediaItem,
    RowNode, SelectNode, Thumbnail, TopNode,
};
use crate::route::CustomId;
use crate::{Components, Media};

/// A node the extra checks look at.
#[derive(Clone, Copy)]
pub(super) enum Site<'t> {
    /// A button in a row or as a Section accessory.
    Button(&'t Button),
    /// A select in a row.
    Select(&'t SelectNode),
    /// A File component.
    File(&'t File),
    /// A Thumbnail accessory.
    Thumbnail(&'t Thumbnail),
    /// A Media Gallery item.
    Item(&'t MediaItem),
}

impl<'t> Site<'t> {
    /// The Route `custom_id` of a button or select.
    pub(super) fn custom_id(self) -> Option<&'t CustomId> {
        match self {
            Self::Button(b) => match &b.0 {
                ButtonNode::Route(d) => Some(&d.custom_id),
                ButtonNode::Link(_) | ButtonNode::Premium(_) => None,
            },
            Self::Select(s) => Some(&s.common().custom_id),
            Self::File(_) | Self::Thumbnail(_) | Self::Item(_) => None,
        }
    }

    /// The URL of a File, Thumbnail or gallery item.
    pub(super) fn url(self) -> Option<&'t str> {
        match self {
            Self::File(f) => Some(f.attachment().url()),
            Self::Thumbnail(t) => Some(t.media.as_str()),
            Self::Item(i) => Some(i.media.as_str()),
            Self::Button(_) | Self::Select(_) => None,
        }
    }

    /// The media and alt text of a Thumbnail or gallery item.
    pub(super) fn image(self) -> Option<(&'t Media, Option<&'t str>)> {
        match self {
            Self::Thumbnail(t) => Some((&t.media, t.alt.as_deref())),
            Self::Item(i) => Some((&i.media, i.alt.as_deref())),
            Self::Button(_) | Self::Select(_) | Self::File(_) => None,
        }
    }
}

/// A [`Site`] and its path.
pub(super) struct Visit<'t> {
    pub(super) path: String,
    pub(super) site: Site<'t>,
}

/// The path name of a Container child, as `noctis_rules::component` names it.
pub(super) fn child_kind(node: &ChildNode) -> &'static str {
    match node {
        ChildNode::Section(_) => "section",
        ChildNode::Text(_) => "text",
        ChildNode::Gallery(_) => "gallery",
        ChildNode::File(_) => "file",
        ChildNode::Separator(_) => "separator",
        ChildNode::Row(_) => "row",
    }
}

/// Every site of a V2 tree, in pre-order.
pub(super) fn v2_sites(components: &Components) -> Vec<Visit<'_>> {
    let mut out = Vec::new();
    for (i, item) in components.items.iter().enumerate() {
        match &item.0 {
            TopNode::Container(c) => {
                for (j, child) in c.children.iter().enumerate() {
                    let path = format!("container[{i}].{}[{j}]", child_kind(&child.0));
                    child_sites(child, path, &mut out);
                }
            }
            TopNode::Child(child) => {
                let path = format!("{}[{i}]", child_kind(&child.0));
                child_sites(child, path, &mut out);
            }
        }
    }
    out
}

/// Every site of legacy rows (`row[i]`), in order.
pub(super) fn row_sites(rows: &[ActionRow]) -> Vec<Visit<'_>> {
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        row_sites_at(row, &format!("row[{i}]"), &mut out);
    }
    out
}

fn child_sites<'t>(child: &'t ContainerComponent, path: String, out: &mut Vec<Visit<'t>>) {
    match &child.0 {
        ChildNode::Section(s) => {
            let path = format!("{path}.accessory");
            let site = match &s.accessory {
                AccessoryNode::Button(b) => Site::Button(b),
                AccessoryNode::Thumbnail(t) => Site::Thumbnail(t),
            };
            out.push(Visit { path, site });
        }
        ChildNode::Gallery(g) => {
            for (k, item) in g.items.iter().enumerate() {
                out.push(Visit {
                    path: format!("{path}.item[{k}]"),
                    site: Site::Item(item),
                });
            }
        }
        ChildNode::File(f) => out.push(Visit {
            path,
            site: Site::File(f),
        }),
        ChildNode::Row(row) => row_sites_at(row, &path, out),
        ChildNode::Text(_) | ChildNode::Separator(_) => {}
    }
}

fn row_sites_at<'t>(row: &'t ActionRow, path: &str, out: &mut Vec<Visit<'t>>) {
    match &row.0 {
        RowNode::Buttons(buttons) => {
            for (j, b) in buttons.iter().enumerate() {
                out.push(Visit {
                    path: format!("{path}.button[{j}]"),
                    site: Site::Button(b),
                });
            }
        }
        RowNode::Select(s) => out.push(Visit {
            path: format!("{path}.select[0]"),
            site: Site::Select(s),
        }),
    }
}
