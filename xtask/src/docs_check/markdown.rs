//! One Markdown file's links and anchors, read with pulldown-cmark, so code
//! spans and code blocks never count (L13, A11).

use std::collections::BTreeSet;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

use super::slug::Slugger;

/// The Markdown extensions GitHub renders that change what is a link.
const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

/// One link use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Link {
    /// 1-based line of the use.
    pub(super) line: usize,
    /// The target as written (reference links resolved, L11).
    pub(super) dest: String,
}

/// What a Markdown file links to and which anchors it has.
#[derive(Debug, Default)]
pub(super) struct Doc {
    pub(super) links: Vec<Link>,
    /// Heading slugs and HTML `id`/`name` values, lowercase (A1-A12).
    pub(super) anchors: BTreeSet<String>,
}

/// Parses `src`.
pub(super) fn parse(src: &str) -> Doc {
    let lines = LineIndex::new(src);
    let mut doc = Doc::default();
    let mut slugger = Slugger::default();
    let mut heading: Option<String> = None;
    for (event, range) in Parser::new_ext(src, OPTIONS).into_offset_iter() {
        match event {
            Event::Start(
                Tag::Link {
                    link_type,
                    dest_url,
                    ..
                }
                | Tag::Image {
                    link_type,
                    dest_url,
                    ..
                },
            ) if link_type != LinkType::Email => doc.links.push(Link {
                line: lines.line(range.start),
                dest: dest_url.into_string(),
            }),
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(text) = heading.take() {
                    doc.anchors.insert(slugger.next(&text));
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = heading.as_mut() {
                    heading.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(heading) = heading.as_mut() {
                    heading.push(' ');
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                let raw = &src[range.clone()];
                for attr in html_attrs(raw) {
                    match attr.name {
                        AttrName::Src | AttrName::Href => doc.links.push(Link {
                            line: lines.line(range.start + attr.offset),
                            dest: attr.value.to_owned(),
                        }),
                        AttrName::Id | AttrName::Name => {
                            doc.anchors.insert(attr.value.to_lowercase());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    doc
}

/// Byte offset → 1-based line.
struct LineIndex(Vec<usize>);

impl LineIndex {
    fn new(src: &str) -> Self {
        let starts = std::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self(starts)
    }

    fn line(&self, offset: usize) -> usize {
        self.0.partition_point(|&start| start <= offset)
    }
}

/// The HTML attributes the check reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrName {
    Src,
    Href,
    Id,
    Name,
}

/// One quoted attribute value inside raw HTML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Attr<'a> {
    name: AttrName,
    /// Byte offset of the attribute name in the scanned text.
    offset: usize,
    value: &'a str,
}

/// The `src`, `href`, `id` and `name` attributes of raw HTML whose value is
/// double or single quoted (L12, A9); names match in any ASCII case and must
/// follow whitespace, so `data-src` is not `src`.
fn html_attrs(html: &str) -> Vec<Attr<'_>> {
    const NAMES: [(&str, AttrName); 4] = [
        ("src", AttrName::Src),
        ("href", AttrName::Href),
        ("id", AttrName::Id),
        ("name", AttrName::Name),
    ];
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    for (start, _) in html.match_indices(|c: char| c.is_ascii_whitespace()) {
        let at = start + 1;
        let Some((word, name)) = NAMES.iter().find(|(word, _)| {
            bytes
                .get(at..at + word.len())
                .is_some_and(|b| b.eq_ignore_ascii_case(word.as_bytes()))
        }) else {
            continue;
        };
        let rest = &html[at + word.len()..];
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            continue;
        };
        if let Some((value, _)) = rest[1..].split_once(quote) {
            out.push(Attr {
                name: *name,
                offset: at,
                value,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dests(doc: &Doc) -> Vec<(usize, &str)> {
        doc.links
            .iter()
            .map(|l| (l.line, l.dest.as_str()))
            .collect()
    }

    /// L10-L13: images, reference links, raw HTML attributes; nothing in code.
    #[test]
    fn finds_links_outside_code() {
        let src = "\
# Top

[a](a.md) ![i](img/a.png) `[c](code.md)`
[r][ref] and [r][ref]

```text
[f](fenced.md)
```

    [i](indented.md)

<p align=\"center\"><img data-src=\"no.png\" src=\"logo.png\"></p>
Inline <a HREF='x.md'>x</a> and <a href=unquoted.md>u</a>.
<a href=\"mail\">m</a> <me@example.com>

[ref]: ref.md
";
        let doc = parse(src);
        assert_eq!(
            dests(&doc),
            [
                (3, "a.md"),
                (3, "img/a.png"),
                (4, "ref.md"),
                (4, "ref.md"),
                (12, "logo.png"),
                (13, "x.md"),
                (14, "mail"),
            ]
        );
    }

    /// A4-A6, A9, A11, A12.
    #[test]
    fn collects_anchors() {
        let src = "\
Title
=====

## Closed ##

## 1. The `custom_id` *layout* and [fixed-length](x.md) Refs

## Setup
## Setup

```
# not a heading
```

<a id=\"Custom\"></a><span name='other'></span>
";
        let doc = parse(src);
        let anchors: Vec<&str> = doc.anchors.iter().map(String::as_str).collect();
        assert_eq!(
            anchors,
            [
                "1-the-custom_id-layout-and-fixed-length-refs",
                "closed",
                "custom",
                "other",
                "setup",
                "setup-1",
                "title",
            ]
        );
    }

    #[test]
    fn html_attrs_need_whitespace_and_quotes() {
        let attrs = html_attrs("<img data-src=\"a\" src = 'b' id=c NAME=\"d\">");
        let got: Vec<(AttrName, &str)> = attrs.iter().map(|a| (a.name, a.value)).collect();
        assert_eq!(got, [(AttrName::Src, "b"), (AttrName::Name, "d")]);
    }

    #[test]
    fn lines_are_one_based() {
        let index = LineIndex::new("a\nb\n\nc");
        assert_eq!(
            [0, 1, 2, 3, 4, 5].map(|o| index.line(o)),
            [1, 1, 2, 2, 3, 4]
        );
    }
}
