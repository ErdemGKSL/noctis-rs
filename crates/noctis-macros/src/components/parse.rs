//! The `components!` grammar, parsed into an owned AST that keeps the tokens
//! (for spans and the expansion) and the values of string literals (for the
//! descriptors, which borrow them).

use std::cell::Cell;

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use syn::ext::IdentExt;
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    AngleBracketedGenericArguments, Block, Expr, ExprLit, Ident, Lit as SynLit, LitStr, Stmt,
    Token, braced, bracketed, parenthesized, token,
};

/// The node keywords.
const KEYWORDS: &[&str] = &[
    "container",
    "section",
    "text",
    "thumbnail",
    "gallery",
    "item",
    "file",
    "separator",
    "row",
    "link",
    "premium",
];

const EXPECTED_NODE: &str = "expected a node: `container`, `section`, `text`, `thumbnail`, `gallery`, `file`, `separator`, `row`, `link`, `premium`, a Route component (`route.button(..)` or `route.<kind>_select(..)`), a `{hole}` or a `..spread`";

/// The first and last span of some tokens, so an error covers all of them.
#[derive(Clone, Copy)]
pub(crate) struct Sp {
    pub(crate) start: Span,
    pub(crate) end: Span,
}

impl Sp {
    /// The span of `tokens`, from the first token to the last.
    pub(crate) fn of(tokens: &impl ToTokens) -> Self {
        let stream = tokens.to_token_stream();
        let mut iter = stream.into_iter();
        let first = iter.next();
        let start = first.as_ref().map_or_else(Span::call_site, TokenTree::span);
        let end = iter.last().or(first).map_or(start, |t| match t {
            TokenTree::Group(g) => g.span_close(),
            t => t.span(),
        });
        Self { start, end }
    }

    /// From `start` to `end`.
    pub(crate) fn new(start: Span, end: Span) -> Self {
        Self { start, end }
    }
}

impl ToTokens for Sp {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut a = proc_macro2::Punct::new('#', proc_macro2::Spacing::Alone);
        a.set_span(self.start);
        let mut b = proc_macro2::Punct::new('#', proc_macro2::Spacing::Alone);
        b.set_span(self.end);
        tokens.extend([TokenTree::Punct(a), TokenTree::Punct(b)]);
    }
}

/// A string literal with its unescaped value.
pub(crate) struct Lit {
    pub(crate) lit: LitStr,
    pub(crate) value: String,
}

impl Lit {
    fn new(lit: LitStr) -> Self {
        let value = lit.value();
        Self { lit, value }
    }
}

/// A text argument: a string literal (measured) or any expression (not).
pub(crate) enum Arg {
    Lit(Lit),
    Expr(Expr),
}

impl Arg {
    fn new(expr: Expr) -> Self {
        match lit_str(&expr) {
            Some(lit) => Self::Lit(Lit::new(lit.clone())),
            None => Self::Expr(expr),
        }
    }

    pub(crate) fn lit(&self) -> Option<&Lit> {
        match self {
            Self::Lit(l) => Some(l),
            Self::Expr(_) => None,
        }
    }
}

impl ToTokens for Arg {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Lit(l) => l.lit.to_tokens(tokens),
            Self::Expr(e) => e.to_tokens(tokens),
        }
    }
}

/// The string literal `expr` is, looking through invisible groups.
pub(crate) fn lit_str(expr: &Expr) -> Option<&LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: SynLit::Str(s),
            ..
        }) => Some(s),
        Expr::Group(g) => lit_str(&g.expr),
        _ => None,
    }
}

/// The integer literal `expr` is, looking through invisible groups.
pub(crate) fn lit_int(expr: &Expr) -> Option<&syn::LitInt> {
    match expr {
        Expr::Lit(ExprLit {
            lit: SynLit::Int(i),
            ..
        }) => Some(i),
        Expr::Group(g) => lit_int(&g.expr),
        _ => None,
    }
}

/// Media and its optional alt text (`thumbnail(..)`, `item(..)`).
pub(crate) struct MediaArgs {
    pub(crate) media: Arg,
    pub(crate) alt: Option<Arg>,
}

/// What a Route component is built with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteKind {
    Button,
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect,
}

impl RouteKind {
    fn of(method: &str) -> Option<Self> {
        Some(match method {
            "button" => Self::Button,
            "string_select" => Self::StringSelect,
            "user_select" => Self::UserSelect,
            "role_select" => Self::RoleSelect,
            "mentionable_select" => Self::MentionableSelect,
            "channel_select" => Self::ChannelSelect,
            _ => return None,
        })
    }

    pub(crate) fn is_select(self) -> bool {
        self != Self::Button
    }
}

/// A method call after the Route constructor, re-applied in order.
pub(crate) struct Setter {
    pub(crate) method: Ident,
    pub(crate) turbofish: Option<AngleBracketedGenericArguments>,
    pub(crate) args: Punctuated<Expr, Token![,]>,
}

/// `route.button(..)` or `route.<kind>_select(..)` plus setters.
pub(crate) struct RouteNode {
    pub(crate) receiver: Expr,
    pub(crate) method: Ident,
    pub(crate) kind: RouteKind,
    /// The constructor's arguments: the label of `.button(label)`, the
    /// options of `.string_select(options)`.
    pub(crate) arg: Option<Arg>,
    pub(crate) setters: Vec<Setter>,
    /// Literal labels (`.button("..")`, `.label("..")`) or placeholders, in
    /// order.
    pub(crate) texts: Vec<Lit>,
}

/// The value of `separator(spacing = ..)`.
pub(crate) enum SpacingArg {
    Small,
    Large,
    /// Anything else, kept for the CV2-13 error.
    Invalid(Expr),
}

/// One node of the tree.
pub(crate) struct Node {
    /// Unique within one invocation.
    pub(crate) id: usize,
    pub(crate) sp: Sp,
    pub(crate) kind: Kind,
}

pub(crate) enum Kind {
    Container {
        accent: Option<Expr>,
        children: Vec<Node>,
    },
    Section {
        texts: Vec<Node>,
        accessory: Box<Node>,
    },
    Text(Arg),
    Thumbnail(MediaArgs),
    Item(MediaArgs),
    Gallery(Vec<Node>),
    File(Expr),
    Separator {
        spacing: Option<SpacingArg>,
        divider: Option<Expr>,
    },
    Row(Vec<Node>),
    Link {
        url: Lit,
        label: Lit,
    },
    Premium(Expr),
    Route(Box<RouteNode>),
    Hole(Vec<Stmt>),
    Spread(Expr),
}

impl Kind {
    /// The node's name in error messages.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Container { .. } => "container",
            Self::Section { .. } => "section",
            Self::Text(_) => "text",
            Self::Thumbnail(_) => "thumbnail",
            Self::Item(_) => "item",
            Self::Gallery(_) => "gallery",
            Self::File(_) => "file",
            Self::Separator { .. } => "separator",
            Self::Row(_) => "row",
            Self::Route(r) if r.kind.is_select() => "select",
            Self::Link { .. } | Self::Premium(_) | Self::Route(_) => "button",
            Self::Hole(_) => "hole",
            Self::Spread(_) => "spread",
        }
    }
}

/// Parses a whole `components!` body.
pub(crate) fn parse(input: TokenStream) -> syn::Result<Vec<Node>> {
    let ids = Cell::new(0);
    (|stream: ParseStream<'_>| list(stream, &ids)).parse2(input)
}

/// `node,*` until the end of `input`.
fn list(input: ParseStream<'_>, ids: &Cell<usize>) -> syn::Result<Vec<Node>> {
    let mut out = Vec::new();
    while !input.is_empty() {
        out.push(node(input, ids)?);
        if input.is_empty() {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    Ok(out)
}

const NO_SPREAD: &str = "spreads go only at the top level or in a container; build a dynamic row with ActionRow::try_buttons and pass it as a {hole}";

/// `node,*` where a spread is not allowed.
fn list_no_spread(input: ParseStream<'_>, ids: &Cell<usize>) -> syn::Result<Vec<Node>> {
    let nodes = list(input, ids)?;
    reject_spreads(&nodes)?;
    Ok(nodes)
}

fn reject_spreads(nodes: &[Node]) -> syn::Result<()> {
    match nodes.iter().find(|n| matches!(n.kind, Kind::Spread(_))) {
        Some(n) => Err(syn::Error::new_spanned(n.sp, NO_SPREAD)),
        None => Ok(()),
    }
}

fn next_id(ids: &Cell<usize>) -> usize {
    let id = ids.get();
    ids.set(id + 1);
    id
}

/// Whether the next tokens are `keyword` followed by `(`, `[`, or (for
/// `separator` only) the end of the node.
fn peek_keyword(input: ParseStream<'_>) -> Option<Ident> {
    let fork = input.fork();
    let ident = Ident::parse_any(&fork).ok()?;
    let name = ident.to_string();
    if !KEYWORDS.contains(&name.as_str()) {
        return None;
    }
    let ends = fork.is_empty() || fork.peek(Token![,]) || fork.peek(Token![;]);
    if fork.peek(token::Paren) || fork.peek(token::Bracket) || (name == "separator" && ends) {
        Some(ident)
    } else {
        None
    }
}

/// An unknown word followed by `[ .. ]` and nothing else, such as `box [ ]`.
fn reject_unknown_keyword(input: ParseStream<'_>) -> syn::Result<()> {
    let fork = input.fork();
    let Ok(ident) = Ident::parse_any(&fork) else {
        return Ok(());
    };
    if !fork.peek(token::Bracket) {
        return Ok(());
    }
    let _: TokenTree = fork.parse()?;
    if fork.is_empty() || fork.peek(Token![,]) || fork.peek(Token![;]) {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "unknown node `{ident}`; expected `container`, `section`, `gallery` or `row` before `[`"
            ),
        ));
    }
    Ok(())
}

fn node(input: ParseStream<'_>, ids: &Cell<usize>) -> syn::Result<Node> {
    let id = next_id(ids);
    if input.peek(token::Brace) {
        let content;
        let brace = braced!(content in input);
        let stmts = content.call(Block::parse_within)?;
        let sp = Sp::new(brace.span.open(), brace.span.close());
        return Ok(Node {
            id,
            sp,
            kind: Kind::Hole(stmts),
        });
    }
    if input.peek(Token![..]) {
        let dots: Token![..] = input.parse()?;
        let expr: Expr = input.parse()?;
        let sp = Sp::new(dots.spans[0], Sp::of(&expr).end);
        return Ok(Node {
            id,
            sp,
            kind: Kind::Spread(expr),
        });
    }
    if let Some(kw) = peek_keyword(input) {
        Ident::parse_any(input)?;
        return keyword(&kw, input, ids, id);
    }
    reject_unknown_keyword(input)?;
    let expr: Expr = input.parse()?;
    let sp = Sp::of(&expr);
    let route = route(expr)?;
    Ok(Node {
        id,
        sp,
        kind: Kind::Route(Box::new(route)),
    })
}

/// `(..)` after a keyword: its content and closing span.
fn parens(input: ParseStream<'_>) -> syn::Result<(syn::parse::ParseBuffer<'_>, Span)> {
    let content;
    let paren = parenthesized!(content in input);
    Ok((content, paren.span.close()))
}

fn brackets(input: ParseStream<'_>) -> syn::Result<(syn::parse::ParseBuffer<'_>, Span)> {
    let content;
    let bracket = bracketed!(content in input);
    Ok((content, bracket.span.close()))
}

/// One expression filling `content`, with an optional trailing comma.
fn only_expr(content: ParseStream<'_>, what: &str) -> syn::Result<Expr> {
    if content.is_empty() {
        return Err(content.error(format!("{what} takes one argument")));
    }
    let expr: Expr = content.parse()?;
    if content.peek(Token![,]) {
        content.parse::<Token![,]>()?;
    }
    if !content.is_empty() {
        return Err(content.error(format!("{what} takes one argument")));
    }
    Ok(expr)
}

/// `key = value` with `key` one of `keys`.
fn key_value(input: ParseStream<'_>, keys: &[&str], what: &str) -> syn::Result<(Ident, Expr)> {
    let key = Ident::parse_any(input)?;
    if !keys.contains(&key.to_string().as_str()) {
        let expected = keys
            .iter()
            .map(|k| format!("`{k}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        return Err(syn::Error::new(
            key.span(),
            format!("unknown {what} argument `{key}`; expected {expected}"),
        ));
    }
    input.parse::<Token![=]>()?;
    let value: Expr = input.parse()?;
    Ok((key, value))
}

/// `(key = value, ..)` with each key at most once.
fn key_values(
    content: ParseStream<'_>,
    keys: &[&str],
    what: &str,
) -> syn::Result<Vec<(Ident, Expr)>> {
    let mut out: Vec<(Ident, Expr)> = Vec::new();
    while !content.is_empty() {
        let (key, value) = key_value(content, keys, what)?;
        if out.iter().any(|(k, _)| *k == key) {
            return Err(syn::Error::new(
                key.span(),
                format!("duplicate {what} argument `{key}`"),
            ));
        }
        out.push((key, value));
        if content.is_empty() {
            break;
        }
        content.parse::<Token![,]>()?;
    }
    Ok(out)
}

/// `media [, alt = text]`.
fn media_args(content: ParseStream<'_>, what: &str) -> syn::Result<MediaArgs> {
    if content.is_empty() {
        return Err(content.error(format!(
            "{what} takes the media, then optionally `alt = ..`"
        )));
    }
    let media = Arg::new(content.parse()?);
    let mut alt = None;
    if content.peek(Token![,]) {
        content.parse::<Token![,]>()?;
        if !content.is_empty() {
            let (_, value) = key_value(content, &["alt"], what)?;
            alt = Some(Arg::new(value));
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
    }
    if !content.is_empty() {
        return Err(content.error(format!(
            "{what} takes the media, then optionally `alt = ..`"
        )));
    }
    Ok(MediaArgs { media, alt })
}

fn keyword(kw: &Ident, input: ParseStream<'_>, ids: &Cell<usize>, id: usize) -> syn::Result<Node> {
    let name = kw.to_string();
    let start = kw.span();
    let (kind, end) = match name.as_str() {
        "container" => {
            let mut accent = None;
            if input.peek(token::Paren) {
                let (content, _) = parens(input)?;
                for (_, value) in key_values(&content, &["accent"], "container")? {
                    accent = Some(value);
                }
            }
            let (content, end) = brackets(input)?;
            let children = list(&content, ids)?;
            (Kind::Container { accent, children }, end)
        }
        "section" => {
            let (content, end) = brackets(input)?;
            let (texts, accessory) = section_body(&content, ids)?;
            (
                Kind::Section {
                    texts,
                    accessory: Box::new(accessory),
                },
                end,
            )
        }
        "text" => {
            let (content, end) = parens(input)?;
            (Kind::Text(Arg::new(only_expr(&content, "text")?)), end)
        }
        "thumbnail" | "item" => {
            let (content, end) = parens(input)?;
            let args = media_args(&content, &name)?;
            let kind = if name == "item" {
                Kind::Item(args)
            } else {
                Kind::Thumbnail(args)
            };
            (kind, end)
        }
        "gallery" => {
            let (content, end) = brackets(input)?;
            (Kind::Gallery(list_no_spread(&content, ids)?), end)
        }
        "file" => {
            let (content, end) = parens(input)?;
            (Kind::File(only_expr(&content, "file")?), end)
        }
        "separator" => {
            let mut spacing = None;
            let mut divider = None;
            let mut end = start;
            if input.peek(token::Paren) {
                let (content, close) = parens(input)?;
                end = close;
                for (key, value) in key_values(&content, &["spacing", "divider"], "separator")? {
                    if key == "spacing" {
                        spacing = Some(spacing_arg(value));
                    } else {
                        divider = Some(value);
                    }
                }
            }
            (Kind::Separator { spacing, divider }, end)
        }
        "row" => {
            let (content, end) = brackets(input)?;
            (Kind::Row(list_no_spread(&content, ids)?), end)
        }
        "link" => {
            let (content, end) = parens(input)?;
            (link_args(&content, start)?, end)
        }
        _ => {
            // "premium"
            let (content, end) = parens(input)?;
            (Kind::Premium(only_expr(&content, "premium")?), end)
        }
    };
    Ok(Node {
        id,
        sp: Sp::new(start, end),
        kind,
    })
}

fn spacing_arg(value: Expr) -> SpacingArg {
    if let Expr::Path(p) = &value
        && p.qself.is_none()
        && p.attrs.is_empty()
    {
        if p.path.is_ident("small") {
            return SpacingArg::Small;
        }
        if p.path.is_ident("large") {
            return SpacingArg::Large;
        }
    }
    SpacingArg::Invalid(value)
}

const LINK_FORM: &str =
    "link needs literal `url` and `label`, in that order: link(url = \"https://…\", label = \"…\")";

fn link_args(content: ParseStream<'_>, start: Span) -> syn::Result<Kind> {
    let fail = |span: Span| syn::Error::new(span, LINK_FORM);
    let pairs = key_values(content, &["url", "label"], "link")?;
    let [(url_key, url), (label_key, label)] =
        <[(Ident, Expr); 2]>::try_from(pairs).map_err(|_| fail(start))?;
    if url_key != "url" || label_key != "label" {
        return Err(fail(url_key.span()));
    }
    let url = lit_str(&url).ok_or_else(|| fail(url.span()))?.clone();
    let label = lit_str(&label).ok_or_else(|| fail(label.span()))?.clone();
    Ok(Kind::Link {
        url: Lit::new(url),
        label: Lit::new(label),
    })
}

/// `text,* ; accessory = node [,]`.
fn section_body(content: ParseStream<'_>, ids: &Cell<usize>) -> syn::Result<(Vec<Node>, Node)> {
    let mut texts = Vec::new();
    while !content.peek(Token![;]) {
        if content.is_empty() {
            return Err(content.error(
                "expected `;` then `accessory = ..` in a section: section [ text(..), ..; accessory = .. ]",
            ));
        }
        texts.push(node(content, ids)?);
        if content.peek(Token![;]) {
            break;
        }
        if !content.peek(Token![,]) {
            return Err(content.error(
                "expected `,` or `;` then `accessory = ..` in a section: section [ text(..), ..; accessory = .. ]",
            ));
        }
        content.parse::<Token![,]>()?;
    }
    reject_spreads(&texts)?;
    content.parse::<Token![;]>()?;
    let key = Ident::parse_any(content)?;
    if key != "accessory" {
        return Err(syn::Error::new(
            key.span(),
            "expected `accessory = ..` after `;` in a section",
        ));
    }
    content.parse::<Token![=]>()?;
    let accessory = node(content, ids)?;
    reject_spreads(core::slice::from_ref(&accessory))?;
    if content.peek(Token![,]) {
        content.parse::<Token![,]>()?;
    }
    if !content.is_empty() {
        return Err(content.error("a section has exactly one accessory, last"));
    }
    Ok((texts, accessory))
}

/// Splits a Route component expression into receiver, constructor and
/// setters.
fn route(expr: Expr) -> syn::Result<RouteNode> {
    let whole = Sp::of(&expr);
    let mut setters = Vec::new();
    let mut cur = expr;
    loop {
        match cur {
            Expr::MethodCall(mc) => {
                let method = mc.method.to_string();
                if method == "button_with" {
                    return Err(syn::Error::new(
                        mc.method.span(),
                        "`button_with` is not available yet (planned, Story 9.3); use `.button(label)`",
                    ));
                }
                if let Some(kind) = RouteKind::of(&method) {
                    setters.reverse();
                    return route_node(*mc.receiver, mc.method, kind, mc.args, setters);
                }
                setters.push(Setter {
                    method: mc.method,
                    turbofish: mc.turbofish,
                    args: mc.args,
                });
                cur = *mc.receiver;
            }
            Expr::Group(g) => cur = *g.expr,
            Expr::Call(call) if is_button_with(&call.func) => {
                return Err(syn::Error::new_spanned(
                    &call.func,
                    "`button_with` is not available yet (planned, Story 9.3); use `.button(label)`",
                ));
            }
            _ => return Err(syn::Error::new_spanned(whole, EXPECTED_NODE)),
        }
    }
}

fn is_button_with(func: &Expr) -> bool {
    matches!(func, Expr::Path(p) if p.path.is_ident("button_with"))
}

fn route_node(
    receiver: Expr,
    method: Ident,
    kind: RouteKind,
    args: Punctuated<Expr, Token![,]>,
    setters: Vec<Setter>,
) -> syn::Result<RouteNode> {
    let n = args.len();
    let ok = match kind {
        RouteKind::Button => n <= 1,
        RouteKind::StringSelect => n == 1,
        _ => n == 0,
    };
    if !ok {
        let what = match kind {
            RouteKind::Button => "`.button(..)` takes no argument or one label",
            RouteKind::StringSelect => "`.string_select(..)` takes one argument: the options",
            _ => "a user, role, mentionable or channel select takes no argument",
        };
        return Err(syn::Error::new(method.span(), what));
    }
    let arg = args.into_iter().next().map(Arg::new);
    let setter_name = if kind.is_select() {
        "placeholder"
    } else {
        "label"
    };
    let mut texts = Vec::new();
    if kind == RouteKind::Button
        && let Some(Arg::Lit(l)) = &arg
    {
        texts.push(Lit::new(l.lit.clone()));
    }
    for s in &setters {
        if s.method == setter_name
            && s.args.len() == 1
            && let Some(lit) = s.args.first().and_then(lit_str)
        {
            texts.push(Lit::new(lit.clone()));
        }
    }
    Ok(RouteNode {
        receiver,
        method,
        kind,
        arg,
        setters,
        texts,
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    fn err(input: TokenStream) -> String {
        match parse(input) {
            Ok(_) => "ok".to_owned(),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn parses_every_form() {
        let nodes = parse(quote! {
            container(accent = rgb!(0x5865_F2)) [
                text("### Settings"),
                separator(spacing = large),
                separator,
                section [ text("Logs"), text(name); accessory = Press { n: 1 }.button("Change") ],
                gallery [ item("https://x.y/a.png", alt = "A") ],
                row [ Press { n: 2 }.button("Save").style(ButtonStyle::Primary), link(url = "https://x.y", label = "Docs") ],
                row [ Pick.user_select().placeholder("Who").max_values(3) ],
                ..extra,
            ],
            {footer},
            section [ ; accessory = thumbnail(card, alt = "x") ],
            file(card),
            row [ premium(sku) ],
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(nodes.len(), 5);
        let Kind::Container { children, .. } = &nodes[0].kind else {
            panic!("container");
        };
        let names: Vec<_> = children.iter().map(|c| c.kind.name()).collect();
        assert_eq!(
            names,
            [
                "text",
                "separator",
                "separator",
                "section",
                "gallery",
                "row",
                "row",
                "spread"
            ]
        );
        let Kind::Row(items) = &children[5].kind else {
            panic!("row");
        };
        let Kind::Route(r) = &items[0].kind else {
            panic!("route");
        };
        assert_eq!(r.receiver.to_token_stream().to_string(), "Press { n : 2 }");
        assert_eq!(r.setters.len(), 1);
        assert_eq!(r.texts.len(), 1);
    }

    #[test]
    fn syntax_errors() {
        assert!(err(quote!(box [])).starts_with("unknown node `box`"));
        assert!(err(quote!(5)).starts_with("expected a node"));
        assert!(err(quote!(my_var)).starts_with("expected a node"));
        assert!(err(quote!(container(color = x) [])).starts_with("unknown container argument"));
        assert!(err(quote!(row[link(label = "x")])).starts_with("link needs"));
        assert!(err(quote!(row[link(url = u, label = "x")])).starts_with("link needs"));
        assert!(
            err(quote!(section [ text("a") accessory = x.button() ]))
                .starts_with("expected `,` or `;`")
        );
        assert!(err(quote!(row[P.button_with(x)])).contains("Story 9.3"));
        assert!(err(quote!(row[..buttons])).starts_with("spreads go only"));
        assert!(err(quote!(row[P.button("a", "b")])).contains("takes no argument or one label"));
        assert!(err(quote!(separator(spacing = large, spacing = small))).starts_with("duplicate"));
        assert_eq!(err(quote!(routes[0].button())), "ok");
    }
}
