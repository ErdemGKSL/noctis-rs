//! Lowering the AST into `noctis_rules::component` descriptors, then the
//! static checks: `validate_components`, the literal checks the descriptors
//! cannot carry, and CID-02.
//!
//! The descriptors borrow nested slices, so the lists are built from the
//! deepest nesting level up, each level in its own stack frame (the pattern of
//! `noctis-ui`'s `limits/lower.rs`, with a recursion over the depth because a
//! literal tree may nest misplaced Containers and Rows arbitrarily). Leaves
//! (Section texts, gallery media) borrow only the
//! AST and are built first.
//!
//! A separate walk over the AST, indexed exactly like the descriptors
//! (spreads are skipped), records the span of every node path and literal
//! field path, the pre-order CV2-03 and CV2-04 contributions, the CID-02
//! receivers and the macro's own errors.

use std::collections::HashMap;

use noctis_rules::component::{
    self as rc, validate_button_label, validate_select_placeholder, validate_spacing,
};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use quote::ToTokens;
use syn::Expr;

use super::parse::{Arg, Kind, Lit, MediaArgs, Node, RouteNode, Sp, SpacingArg, lit_int};
use crate::literal::check_accent;

/// The URL a non-literal media lowers to: always valid.
const MEDIA_SENTINEL: &str = "https://x";
/// The URL a `file(..)` lowers to: always valid.
const FILE_SENTINEL: &str = "attachment://x";

/// Every error of a tree, in report order; empty when it passes.
pub(crate) fn check(nodes: &[Node]) -> Vec<syn::Error> {
    let limits = Limits::default();
    let mut facts = Facts::default();
    facts.walk_list(nodes, "", Place::List);
    let violations = with_descriptors(nodes, |tree| {
        rc::validate_components(tree, &limits)
            .err()
            .unwrap_or_default()
    });
    let mut errors: Vec<syn::Error> = violations
        .into_iter()
        .map(|v| {
            let sp = facts.span_of(&v, &limits);
            syn::Error::new_spanned(sp, v.to_string())
        })
        .collect();
    errors.extend(
        facts
            .own
            .drain(..)
            .map(|(sp, v)| syn::Error::new_spanned(sp, v.to_string())),
    );
    errors.extend(facts.cid_02());
    errors
}

/// Where a node sits, for the macro's own placement checks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The top level, a Container or a Row: placement is `validate_components`'s.
    List,
    /// A Media Gallery.
    Gallery,
}

/// What the AST walk records.
#[derive(Default)]
struct Facts<'a> {
    /// Node and literal field paths, as `noctis_rules::component` names them.
    paths: HashMap<String, Sp>,
    /// The CV2-03 weight of every counted node, in pre-order.
    counted: Vec<Sp>,
    /// The `discord_len` of every literal Text Display, in pre-order.
    texts: Vec<&'a Lit>,
    /// Errors the macro finds itself, in source order.
    own: Vec<(Sp, Violation)>,
    /// CID-02 candidates: key, receiver span and component path.
    routes: Vec<(String, Sp, String)>,
}

/// The descriptor kind of a node (its path name).
fn path_kind(kind: &Kind) -> &'static str {
    match kind {
        Kind::Item(_) | Kind::Hole(_) | Kind::Spread(_) => "opaque",
        other => other.name(),
    }
}

fn join(parent: &str, field: &str) -> String {
    if parent.is_empty() {
        field.to_owned()
    } else {
        format!("{parent}.{field}")
    }
}

/// A CV2-13 violation for an invalid Separator spacing.
fn spacing_violation(value: &Expr) -> Vec<Violation> {
    let text = value.to_token_stream().to_string();
    validate_spacing(0, &Limits::default())
        .into_iter()
        .map(|mut v| {
            v.actual = Actual::Text(text.clone());
            v
        })
        .collect()
}

impl<'a> Facts<'a> {
    fn walk_list(&mut self, nodes: &'a [Node], parent: &str, place: Place) {
        let mut j = 0;
        for node in nodes {
            if matches!(node.kind, Kind::Spread(_)) {
                continue;
            }
            let path = match place {
                Place::List => format!(
                    "{}{}[{j}]",
                    if parent.is_empty() {
                        String::new()
                    } else {
                        format!("{parent}.")
                    },
                    path_kind(&node.kind)
                ),
                Place::Gallery => format!("{parent}.item[{j}]"),
            };
            j += 1;
            self.walk(node, &path, place);
        }
    }

    fn walk(&mut self, node: &'a Node, path: &str, place: Place) {
        self.paths.insert(path.to_owned(), node.sp);
        if place == Place::Gallery {
            match &node.kind {
                Kind::Item(m) => self.media(m, path),
                Kind::Hole(_) => {}
                other => self.own.push((
                    node.sp,
                    Violation::new(
                        RuleId::CV2_05,
                        path,
                        Bound::Describe("only items in a Media Gallery"),
                        Actual::Text(other.name().to_owned()),
                    ),
                )),
            }
            return;
        }
        self.counted.push(node.sp);
        match &node.kind {
            Kind::Container { accent, children } => {
                if let Some(accent) = accent {
                    self.accent(accent);
                }
                self.walk_list(children, path, Place::List);
            }
            Kind::Section { texts, accessory } => {
                for text in texts {
                    self.counted.push(text.sp);
                    match &text.kind {
                        Kind::Text(arg) => {
                            if let Some(lit) = arg.lit() {
                                self.texts.push(lit);
                            }
                        }
                        Kind::Hole(_) => {}
                        other => self.own.push((
                            text.sp,
                            Violation::new(
                                RuleId::CV2_07,
                                path,
                                Bound::Describe("only Text Displays before the `;` of a Section"),
                                Actual::Text(other.name().to_owned()),
                            ),
                        )),
                    }
                }
                let acc_path = join(path, "accessory");
                self.paths.insert(acc_path.clone(), accessory.sp);
                self.counted.push(accessory.sp);
                match &accessory.kind {
                    Kind::Thumbnail(m) => self.media(m, &acc_path),
                    Kind::Link { .. } | Kind::Premium(_) | Kind::Hole(_) => {
                        self.button(accessory, &acc_path);
                    }
                    // A select here is the `SectionAccessory` bound's CV2-07.
                    Kind::Route(_) => self.button(accessory, &acc_path),
                    other => self.own.push((
                        accessory.sp,
                        Violation::new(
                            RuleId::CV2_07,
                            acc_path,
                            Bound::Describe("a Section accessory is a button or a Thumbnail"),
                            Actual::Text(other.name().to_owned()),
                        ),
                    )),
                }
            }
            Kind::Text(arg) => {
                if let Some(lit) = arg.lit() {
                    self.texts.push(lit);
                }
            }
            Kind::Thumbnail(m) => self.media(m, path),
            Kind::Item(_) => self.own.push((
                node.sp,
                Violation::new(
                    RuleId::CV2_05,
                    path,
                    Bound::Describe("an item only inside a gallery"),
                    Actual::Text("item".to_owned()),
                ),
            )),
            Kind::Gallery(items) => self.walk_list(items, path, Place::Gallery),
            Kind::Separator {
                spacing: Some(SpacingArg::Invalid(value)),
                ..
            } => {
                let sp = Sp::of(value);
                for v in spacing_violation(value) {
                    self.own.push((sp, v));
                }
            }
            Kind::Row(items) => self.walk_list(items, path, Place::List),
            Kind::Link { .. } | Kind::Premium(_) | Kind::Route(_) => self.button(node, path),
            Kind::File(_) | Kind::Separator { .. } | Kind::Hole(_) | Kind::Spread(_) => {}
        }
    }

    /// Field spans and literal checks of a button or select at `path`.
    fn button(&mut self, node: &'a Node, path: &str) {
        match &node.kind {
            Kind::Link { url, label } => {
                self.paths.insert(join(path, "url"), Sp::of(&url.lit));
                self.paths.insert(join(path, "label"), Sp::of(&label.lit));
            }
            Kind::Route(r) => self.route(r, path),
            _ => {}
        }
    }

    fn route(&mut self, r: &'a RouteNode, path: &str) {
        let limits = Limits::default();
        // The last literal label goes into the descriptor (a button); the
        // others are checked here.
        if let Some((last, rest)) = r.labels.split_last() {
            self.paths.insert(join(path, "label"), Sp::of(&last.lit));
            for lit in rest {
                for v in validate_button_label(&lit.value, &limits) {
                    self.own.push((Sp::of(&lit.lit), v));
                }
            }
        }
        for arg in r.literal_args("placeholder") {
            if let Some(lit) = super::parse::lit_str(arg) {
                for mut v in validate_select_placeholder(&lit.value(), &limits) {
                    v.path = join(path, "placeholder");
                    self.own.push((Sp::of(lit), v));
                }
            }
        }
        for arg in r.literal_args("options") {
            if let Expr::Array(a) = arg
                && a.elems.len() > limits.select_options
            {
                self.own.push((
                    Sp::of(arg),
                    Violation::new(
                        RuleId::CV2_09,
                        join(path, "options"),
                        Bound::AtMost(limits.select_options),
                        Actual::Count(a.elems.len()),
                    ),
                ));
            }
        }
        // A later call overrides an earlier one, so only the last call of
        // each count is checked, and only when it is a literal.
        let mut counts: Vec<(usize, &Expr)> = Vec::new();
        for (name, min) in [("min_values", 0), ("max_values", 1)] {
            let Some(arg) = r.literal_args(name).last() else {
                continue;
            };
            let Some(n) = lit_int(arg).and_then(|i| i.base10_parse::<usize>().ok()) else {
                continue;
            };
            counts.push((n, arg));
            if n < min || n > limits.select_values {
                self.own.push((
                    Sp::of(arg),
                    Violation::new(
                        RuleId::CV2_09,
                        join(path, name),
                        Bound::Between {
                            min,
                            max: limits.select_values,
                        },
                        Actual::Count(n),
                    ),
                ));
            }
        }
        if let [(min, arg), (max, _)] = counts.as_slice()
            && r.literal_args("min_values")
                .last()
                .is_some_and(|a| std::ptr::eq(a, *arg))
            && min > max
        {
            self.own.push((
                Sp::of(*arg),
                Violation::new(
                    RuleId::CV2_09,
                    join(path, "min_values"),
                    Bound::Describe("at most `max_values`"),
                    Actual::Text(format!("{min} > {max}")),
                ),
            ));
        }
        if let Some(key) = cid_key(&r.receiver) {
            self.routes
                .push((key, Sp::of(&r.receiver), path.to_owned()));
        }
    }

    fn media(&mut self, m: &'a MediaArgs, path: &str) {
        if let Arg::Lit(l) = &m.media {
            self.paths.insert(join(path, "url"), Sp::of(&l.lit));
        }
        if let Some(Arg::Lit(l)) = &m.alt {
            self.paths.insert(join(path, "description"), Sp::of(&l.lit));
        }
    }

    /// CV2-13 for an integer literal accent; other expressions are not
    /// checked.
    fn accent(&mut self, accent: &Expr) {
        if let Some(lit) = lit_int(accent)
            && let Err(v) = check_accent(lit)
        {
            self.own.push((Sp::of(lit), *v));
        }
    }

    /// The span of a violation: the crossing node or literal for the
    /// message-wide CV2-03 and CV2-04, else its path or the longest recorded
    /// prefix of it.
    fn span_of(&self, v: &Violation, limits: &Limits) -> Sp {
        if v.path == "components" {
            if v.rule == RuleId::CV2_03
                && let Some(i) = crossing(self.counted.iter().map(|_| 1), limits.components)
            {
                return self.counted[i];
            }
            if v.rule == RuleId::CV2_04
                && let Some(i) = crossing(
                    self.texts
                        .iter()
                        .map(|l| noctis_rules::len::discord_len(&l.value)),
                    limits.text_total_len,
                )
            {
                return Sp::of(&self.texts[i].lit);
            }
        }
        let mut path = v.path.as_str();
        loop {
            if let Some(sp) = self.paths.get(path) {
                return *sp;
            }
            match path.rfind(['.', '[']) {
                Some(i) if i > 0 => path = &path[..i],
                _ => {
                    return Sp::new(
                        proc_macro2::Span::call_site(),
                        proc_macro2::Span::call_site(),
                    );
                }
            }
        }
    }

    /// CID-02: every repeated literal receiver, then its first use.
    fn cid_02(&self) -> Vec<syn::Error> {
        let mut errors = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for (i, (key, first, first_path)) in self.routes.iter().enumerate() {
            if seen.contains(&key.as_str()) {
                continue;
            }
            seen.push(key);
            let dupes: Vec<_> = self.routes[i + 1..]
                .iter()
                .filter(|(k, _, _)| k == key)
                .collect();
            if dupes.is_empty() {
                continue;
            }
            for (_, sp, _) in &dupes {
                errors.push(crate::common::error(
                    sp,
                    RuleId::CID_02,
                    format!("duplicate custom_id: `{key}` is already used at {first_path}"),
                    "give each component its own Route value (a different payload or Route)",
                ));
            }
            errors.push(syn::Error::new_spanned(
                first,
                format!("[CID-02] first use of the duplicate custom_id `{key}`"),
            ));
        }
        errors
    }
}

/// The index of the item whose running sum first exceeds `limit`.
pub(crate) fn crossing(weights: impl Iterator<Item = usize>, limit: usize) -> Option<usize> {
    let mut sum = 0_usize;
    for (i, w) in weights.enumerate() {
        sum = sum.saturating_add(w);
        if sum > limit {
            return Some(i);
        }
    }
    None
}

/// The CID-02 key of a receiver whose `custom_id` is fixed by its tokens, or
/// `None` when it is left to the runtime validator.
pub(crate) fn cid_key(receiver: &Expr) -> Option<String> {
    fn literal(e: &Expr) -> bool {
        match e {
            Expr::Lit(_) => true,
            Expr::Unary(u) => matches!(u.op, syn::UnOp::Neg(_)) && matches!(*u.expr, Expr::Lit(_)),
            Expr::Group(g) => literal(&g.expr),
            _ => false,
        }
    }
    fn upper_path(path: &syn::Path) -> bool {
        path.segments.last().is_some_and(|s| {
            s.arguments.is_none()
                && s.ident
                    .to_string()
                    .starts_with(|c: char| c.is_ascii_uppercase())
        })
    }
    let qualifies = match receiver {
        Expr::Struct(s) => {
            s.qself.is_none()
                && s.rest.is_none()
                && s.dot2_token.is_none()
                && s.fields
                    .iter()
                    .all(|f| f.colon_token.is_some() && literal(&f.expr))
        }
        Expr::Call(c) => {
            matches!(&*c.func, Expr::Path(p) if p.qself.is_none() && upper_path(&p.path))
                && c.args.iter().all(literal)
        }
        // A unit struct, or the same binding used twice.
        Expr::Path(p) => p.qself.is_none() && (upper_path(&p.path) || p.path.get_ident().is_some()),
        _ => false,
    };
    qualifies.then(|| receiver.to_token_stream().to_string())
}

/// Leaf storage, built first; it borrows only the AST.
#[derive(Default)]
struct Leaves<'a> {
    texts: HashMap<usize, Vec<&'a str>>,
    media: HashMap<usize, Vec<rc::Media<'a>>>,
}

fn lower_media(m: &MediaArgs) -> rc::Media<'_> {
    let url = m.media.lit().map_or(MEDIA_SENTINEL, |l| l.value.as_str());
    let media = rc::Media::new(url);
    match m.alt.as_ref().and_then(Arg::lit) {
        Some(alt) => media.description(&alt.value),
        None => media,
    }
}

impl<'a> Leaves<'a> {
    fn collect(&mut self, nodes: &'a [Node]) {
        for node in nodes {
            self.node(node);
        }
    }

    fn node(&mut self, node: &'a Node) {
        match &node.kind {
            Kind::Container { children, .. } | Kind::Row(children) => self.collect(children),
            Kind::Section { texts, accessory } => {
                let lowered = texts
                    .iter()
                    .map(|t| match &t.kind {
                        Kind::Text(Arg::Lit(l)) => l.value.as_str(),
                        _ => "",
                    })
                    .collect();
                self.texts.insert(node.id, lowered);
                self.node(accessory);
            }
            Kind::Gallery(items) => {
                let lowered = items
                    .iter()
                    .filter(|i| !matches!(i.kind, Kind::Spread(_)))
                    .map(|i| match &i.kind {
                        Kind::Item(m) => lower_media(m),
                        _ => rc::Media::new(MEDIA_SENTINEL),
                    })
                    .collect();
                self.media.insert(node.id, lowered);
            }
            _ => {}
        }
    }
}

/// Lowers `nodes` into descriptors and runs `f` on the top-level slice.
fn with_descriptors<R>(nodes: &[Node], f: impl FnOnce(&[rc::Component<'_>]) -> R) -> R {
    let mut leaves = Leaves::default();
    leaves.collect(nodes);
    // levels[k]: the lists at nesting depth k, each with its parent's id.
    let mut levels: Vec<Vec<(Option<usize>, &[Node])>> = vec![vec![(None, nodes)]];
    loop {
        let next: Vec<(Option<usize>, &[Node])> = levels[levels.len() - 1]
            .iter()
            .flat_map(|(_, list)| list.iter())
            .filter_map(|n| match &n.kind {
                Kind::Container { children, .. } | Kind::Row(children) => {
                    Some((Some(n.id), children.as_slice()))
                }
                _ => None,
            })
            .collect();
        if next.is_empty() {
            break;
        }
        levels.push(next);
    }
    lower_level(&levels, levels.len() - 1, &leaves, &HashMap::new(), f)
}

/// Lowers the lists at depth `k` (whose Container and Row children are in
/// `deeper`), then the shallower levels, and finally calls `top`.
fn lower_level<'a: 's, 's, R>(
    levels: &[Vec<(Option<usize>, &'a [Node])>],
    k: usize,
    leaves: &'a Leaves<'a>,
    deeper: &HashMap<usize, &'s [rc::Component<'s>]>,
    top: impl FnOnce(&[rc::Component<'_>]) -> R,
) -> R {
    let lowered: Vec<(Option<usize>, Vec<rc::Component<'s>>)> = levels[k]
        .iter()
        .map(|(parent, list)| {
            let nodes = list
                .iter()
                .filter(|n| !matches!(n.kind, Kind::Spread(_)))
                .map(|n| lower_node(n, leaves, deeper))
                .collect();
            (*parent, nodes)
        })
        .collect();
    if k == 0 {
        let list = lowered.first().map_or(&[][..], |(_, l)| l.as_slice());
        return top(list);
    }
    let map: HashMap<usize, &[rc::Component<'_>]> = lowered
        .iter()
        .filter_map(|(parent, list)| parent.map(|p| (p, list.as_slice())))
        .collect();
    lower_level(levels, k - 1, leaves, &map, top)
}

fn lower_node<'a: 's, 's>(
    node: &'a Node,
    leaves: &'a Leaves<'a>,
    deeper: &HashMap<usize, &'s [rc::Component<'s>]>,
) -> rc::Component<'s> {
    let children = || deeper.get(&node.id).copied().unwrap_or(&[]);
    match &node.kind {
        Kind::Container { .. } => rc::Component::Container(rc::Container::new(children())),
        Kind::Row(_) => rc::Component::Row(children()),
        Kind::Section { accessory, .. } => {
            let texts = leaves.texts.get(&node.id).map_or(&[][..], Vec::as_slice);
            let accessory = match &accessory.kind {
                Kind::Thumbnail(m) => rc::Accessory::Thumbnail(lower_media(m)),
                Kind::Link { .. } | Kind::Premium(_) | Kind::Route(_) => {
                    match lower_button(accessory) {
                        Some(b) => rc::Accessory::Button(b),
                        None => placeholder_accessory(),
                    }
                }
                _ => placeholder_accessory(),
            };
            rc::Component::Section(rc::Section::new(texts, accessory))
        }
        Kind::Text(arg) => rc::Component::Text(arg.lit().map_or("", |l| l.value.as_str())),
        Kind::Thumbnail(m) => rc::Component::Thumbnail(lower_media(m)),
        Kind::Gallery(_) => {
            rc::Component::Gallery(leaves.media.get(&node.id).map_or(&[][..], Vec::as_slice))
        }
        Kind::File(_) => rc::Component::File(FILE_SENTINEL),
        Kind::Separator { .. } => rc::Component::Separator(rc::Separator::new()),
        Kind::Link { .. } | Kind::Premium(_) | Kind::Route(_) => {
            lower_button(node).map_or(rc::Component::Opaque(1), rc::Component::Button)
        }
        Kind::Item(_) | Kind::Hole(_) | Kind::Spread(_) => rc::Component::Opaque(1),
    }
}

/// A hole or an invalid accessory: a Secondary button, always valid.
fn placeholder_accessory() -> rc::Accessory<'static> {
    rc::Accessory::Button(rc::Button::new(rc::ButtonStyle::Secondary))
}

fn lower_button(node: &Node) -> Option<rc::Button<'_>> {
    match &node.kind {
        Kind::Link { url, label } => {
            Some(rc::Button::new(rc::ButtonStyle::Link(&url.value)).label(&label.value))
        }
        Kind::Premium(_) => Some(rc::Button::new(rc::ButtonStyle::Premium)),
        Kind::Route(r) => {
            let b = rc::Button::new(rc::ButtonStyle::Secondary);
            Some(match r.labels.last() {
                Some(l) => b.label(&l.value),
                None => b,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use super::*;
    use crate::components::parse::parse;

    fn tree(tokens: proc_macro2::TokenStream) -> Vec<Node> {
        parse(tokens).unwrap_or_else(|e| panic!("{e}"))
    }

    fn messages(tokens: proc_macro2::TokenStream) -> Vec<String> {
        check(&tree(tokens))
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn cv2_03_count_matches_rules() {
        let nodes = tree(quote! {
            container [ text("a"), section [ text("b"), text(x); accessory = thumbnail(m) ], row [ {b}, P.build() ], ..more ],
            {hole},
            gallery [ item(m), item(n) ],
        });
        let mut facts = Facts::default();
        facts.walk_list(&nodes, "", Place::List);
        let rules = with_descriptors(&nodes, |t| rc::cost(t, &Limits::default()).components);
        // container, text, section + 2 texts + accessory, row + 2, hole, gallery
        assert_eq!(rules, 11);
        assert_eq!(facts.counted.len(), rules);
    }

    #[test]
    fn cv2_03_crossing_node() {
        let texts = (0..41).map(|_| quote!(text("a"),));
        let msgs = messages(quote!(#(#texts)*));
        assert_eq!(msgs.len(), 1);
        assert!(
            msgs[0].starts_with(
                "[CV2-03] components: limit at most 40, actual 41\n  = note: text[0]: 1"
            )
        );
        assert_eq!(crossing([1; 41].into_iter(), 40), Some(40));
        assert_eq!(crossing([1; 40].into_iter(), 40), None);
    }

    #[test]
    fn cv2_04_crossing_literal() {
        let a = "a".repeat(2000);
        let b = "b".repeat(2000);
        let nodes = tree(quote! {
            text(#a), text(var), container [ section [ text(#b), text("c"); accessory = {x} ] ],
        });
        let mut facts = Facts::default();
        facts.walk_list(&nodes, "", Place::List);
        let lens = facts
            .texts
            .iter()
            .map(|l| noctis_rules::len::discord_len(&l.value));
        let i = crossing(lens, Limits::default().text_total_len);
        assert_eq!(i.map(|i| facts.texts[i].value.as_str()), Some("c"));
        let msgs = messages(quote!(text(#a), text(#b), text(var)));
        assert!(msgs.is_empty(), "{msgs:?}");
    }

    #[test]
    fn rules_and_own_checks() {
        let msgs = messages(quote! {
            row [],
            container [ container [] ],
            thumbnail(m),
            item(m),
            section [ row [ P.build() ]; accessory = text("x") ],
            container(accent = 0x1000000) [],
            separator(spacing = medium),
            row [ P.build().label("a").label("b") ],
            row [ Q.build().options([o, o]).min_values(3).max_values(2) ],
        });
        let ids: Vec<_> = msgs.iter().map(|m| &m[..8]).collect();
        assert_eq!(
            ids,
            [
                "[CV2-06]", "[CV2-05]", "[CV2-05]", "[CV2-05]", "[CV2-07]", "[CV2-07]", "[CV2-13]",
                "[CV2-13]", "[CV2-09]"
            ],
            "{msgs:#?}"
        );
    }

    #[test]
    fn select_matrix_cases() {
        let long = "p".repeat(151);
        let msgs = messages(quote! {
            row [ Q.build().options([o]).placeholder(#long) ],
        });
        assert_eq!(msgs.len(), 1, "{msgs:#?}");
        assert!(
            msgs[0].starts_with("[CV2-09] row[0].button[0].placeholder"),
            "{msgs:#?}"
        );

        let msgs = messages(quote! {
            row [ Q.build().options([o, o, o]).min_values(3).max_values(2) ],
        });
        assert_eq!(msgs.len(), 1, "{msgs:#?}");
        assert!(msgs[0].starts_with("[CV2-"), "{msgs:#?}");
        assert!(msgs[0].contains("min_values"), "{msgs:#?}");

        let msgs = messages(quote!(container[item(m)]));
        assert_eq!(msgs.len(), 1, "{msgs:#?}");
        assert!(msgs[0].starts_with("[CV2-05]"), "{msgs:#?}");
    }

    #[test]
    fn select_counts_follow_the_last_call() {
        let msgs = messages(quote! {
            row [ Q.build().options([o, o]).min_values(3).min_values(n).max_values(2) ],
            row [ R.build().options([o, o]).max_values(0).max_values(m) ],
        });
        assert!(msgs.is_empty(), "{msgs:#?}");
    }

    #[test]
    fn cid_02_keys() {
        let key = |e: Expr| cid_key(&e);
        assert_eq!(
            key(parse_quote!(Press { n: 1 })).as_deref(),
            Some("Press { n : 1 }")
        );
        assert_eq!(
            key(parse_quote!(Press { n: -1 })).as_deref(),
            Some("Press { n : - 1 }")
        );
        assert_eq!(key(parse_quote!(Page(1))).as_deref(), Some("Page (1)"));
        assert_eq!(key(parse_quote!(Refresh)).as_deref(), Some("Refresh"));
        assert_eq!(key(parse_quote!(route_var)).as_deref(), Some("route_var"));
        assert_eq!(
            key(parse_quote!(crate::Refresh)).as_deref(),
            Some("crate :: Refresh")
        );
        for no in [
            parse_quote!(Press { n }),
            parse_quote!(Press { n: x }),
            parse_quote!(Press { n: 1, ..base }),
            parse_quote!(page(1)),
            parse_quote!(Page(x)),
            parse_quote!(Wrap::<u8>),
        ] {
            assert_eq!(key(no), None);
        }
    }

    #[test]
    fn cid_02_duplicates() {
        let msgs = messages(quote! {
            row [ Press { n: 1 }.build(), Press { n: 1 }.build().label("x"), Press { n: 1 }.build() ],
            row [ Press { n: 2 }.build(), Press { n }.build(), Press { n }.build() ],
        });
        assert_eq!(msgs.len(), 3, "{msgs:#?}");
        assert!(msgs[0].starts_with(
            "[CID-02] duplicate custom_id: `Press { n : 1 }` is already used at row[0].button[0]"
        ));
        assert!(msgs[1].starts_with("[CID-02] duplicate custom_id"));
        assert_eq!(
            msgs[2],
            "[CID-02] first use of the duplicate custom_id `Press { n : 1 }`"
        );
    }

    #[test]
    fn cid_02_duplicates_across_places() {
        let msgs = messages(quote! {
            section [ text("a"); accessory = Acc.build().label("x") ],
            container [ row [ Acc.build() ] ],
            row [ U.build(), U.build() ],
            row [ Page(1).build(), Page(1).build() ],
            row [ Same.build() ],
            row [ Same.build().options([o]) ],
        });
        let dups: Vec<_> = msgs
            .iter()
            .filter(|m| m.starts_with("[CID-02] duplicate"))
            .collect();
        assert_eq!(dups.len(), 4, "{msgs:#?}");
        for (key, at) in [
            ("Acc", "section[0].accessory"),
            ("U", "row[2].button[0]"),
            ("Page (1)", "row[3].button[0]"),
            ("Same", "row[4].button[0]"),
        ] {
            let text = format!("`{key}` is already used at {at}");
            assert!(dups.iter().any(|m| m.contains(&text)), "{text}: {msgs:#?}");
        }
        let firsts = msgs.iter().filter(|m| m.contains("first use")).count();
        assert_eq!(firsts, 4, "{msgs:#?}");
    }
}
