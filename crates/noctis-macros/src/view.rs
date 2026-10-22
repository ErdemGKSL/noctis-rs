//! `view!`: an HTML-like node tree, parsed at expansion and lowered to the
//! `noctis::image::Node` builder calls. Every `tw = "…"` literal is validated
//! like `tw!` (TW-01).
//!
//! ```text
//! view    := element                      (exactly one root)
//! element := '<' tag attr* '/' '>' | '<' tag attr* '>' child* '<' '/' tag '>'
//! tag     := div | span | p | h1 | h2 | h3
//! attr    := 'tw' '=' (lit | '{' expr '}') | 'style' '=' '{' expr '}'
//! child   := element | lit | '{' expr '}'
//!          | 'if' cond '{' child* '}' ('else' 'if' .. | 'else' '{' child* '}')?
//!          | 'for' pat 'in' expr '{' child* '}'
//! ```

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Pat, Token, braced, token};

use crate::common::combine;
use crate::theme::crate_theme;
use crate::tw::check_literal;

/// The tags `view!` accepts, as written.
const TAGS: &[(&str, &str)] = &[
    ("div", "Div"),
    ("span", "Span"),
    ("p", "P"),
    ("h1", "H1"),
    ("h2", "H2"),
    ("h3", "H3"),
];

/// The parsed body: one root element.
pub(crate) struct View {
    root: Element,
}

struct Element {
    /// The tag's variant name in `Tag` (`Div`).
    tag: &'static str,
    tw: Option<Tw>,
    style: Option<Expr>,
    children: Vec<Child>,
}

enum Tw {
    Lit(LitStr),
    Expr(Expr),
}

enum Child {
    Element(Element),
    Text(LitStr),
    Expr(Expr),
    If(If),
    For {
        pat: Box<Pat>,
        expr: Expr,
        body: Vec<Child>,
    },
}

struct If {
    cond: Expr,
    then: Vec<Child>,
    otherwise: Option<Else>,
}

enum Else {
    If(Box<If>),
    Block(Vec<Child>),
}

impl Parse for View {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error(
                "view! takes exactly one root element, such as `<div tw=\"flex\">..</div>`",
            ));
        }
        if !input.peek(Token![<]) {
            return Err(input.error(
                "view! takes exactly one root element, such as `<div tw=\"flex\">..</div>`",
            ));
        }
        let root = parse_element(input)?;
        if !input.is_empty() {
            return Err(input.error(
                "view! takes exactly one root element; wrap these elements in one `<div>`",
            ));
        }
        Ok(Self { root })
    }
}

fn parse_element(input: ParseStream<'_>) -> syn::Result<Element> {
    input.parse::<Token![<]>()?;
    let name = Ident::parse_any(input)?;
    let tag = tag_of(&name)?;
    let mut tw = None;
    let mut style = None;
    while !(input.peek(Token![>]) || input.peek(Token![/])) {
        if input.is_empty() {
            return Err(syn::Error::new(
                name.span(),
                format!("`<{name}` is missing its `>`"),
            ));
        }
        let attr = Ident::parse_any(input).map_err(|_| {
            input.error(format!(
                "expected an attribute (`tw` or `style`) or `>` in `<{name}>`"
            ))
        })?;
        match attr.to_string().as_str() {
            "tw" => {
                if tw.is_some() {
                    return Err(duplicate(&attr));
                }
                input.parse::<Token![=]>()?;
                tw = Some(if input.peek(LitStr) {
                    Tw::Lit(input.parse()?)
                } else if input.peek(token::Brace) {
                    Tw::Expr(braced_expr(input)?)
                } else {
                    return Err(input.error(
                        "`tw` takes a string literal (`tw=\"flex p-4\"`) or `{expr}` of type `Classes`",
                    ));
                });
            }
            "style" => {
                if style.is_some() {
                    return Err(duplicate(&attr));
                }
                input.parse::<Token![=]>()?;
                if !input.peek(token::Brace) {
                    return Err(input.error(
                        "`style` takes `{expr}` of type `Style`, such as `style={Style::new().width_pct(50.0)}`",
                    ));
                }
                style = Some(braced_expr(input)?);
            }
            "class" => {
                return Err(syn::Error::new(
                    attr.span(),
                    "unknown attribute `class`: use `tw` for classes (`tw=\"flex p-4\"`)",
                ));
            }
            _ => {
                return Err(syn::Error::new(
                    attr.span(),
                    format!("unknown attribute `{attr}`: elements take `tw` and `style`"),
                ));
            }
        }
    }
    if input.peek(Token![/]) {
        input.parse::<Token![/]>()?;
        input.parse::<Token![>]>()?;
        return Ok(Element {
            tag,
            tw,
            style,
            children: Vec::new(),
        });
    }
    input.parse::<Token![>]>()?;
    let mut children = Vec::new();
    loop {
        if input.is_empty() {
            return Err(syn::Error::new(
                name.span(),
                format!("`<{name}>` is missing its closing tag `</{name}>`"),
            ));
        }
        if input.peek(Token![<]) && input.peek2(Token![/]) {
            input.parse::<Token![<]>()?;
            input.parse::<Token![/]>()?;
            let close = Ident::parse_any(input)?;
            if close != name {
                return Err(syn::Error::new(
                    close.span(),
                    format!("mismatched closing tag: expected `</{name}>`, found `</{close}>`"),
                ));
            }
            input.parse::<Token![>]>()?;
            break;
        }
        children.push(parse_child(input)?);
    }
    Ok(Element {
        tag,
        tw,
        style,
        children,
    })
}

fn tag_of(name: &Ident) -> syn::Result<&'static str> {
    let text = name.to_string();
    if let Some((_, variant)) = TAGS.iter().find(|(tag, _)| *tag == text) {
        return Ok(variant);
    }
    let message = match text.as_str() {
        "img" | "svg" => "`<img>`/`<svg>` are planned (Story 10.7)".to_owned(),
        _ => format!("unknown tag `<{text}>`: use `div`, `span`, `p`, `h1`, `h2` or `h3`"),
    };
    Err(syn::Error::new(name.span(), message))
}

fn duplicate(attr: &Ident) -> syn::Error {
    syn::Error::new(attr.span(), format!("duplicate attribute `{attr}`"))
}

/// `{ expr }`, the whole group being one expression.
fn braced_expr(input: ParseStream<'_>) -> syn::Result<Expr> {
    let content;
    braced!(content in input);
    let expr: Expr = content.parse()?;
    if !content.is_empty() {
        return Err(content.error("expected one expression in `{..}`"));
    }
    Ok(expr)
}

fn parse_child(input: ParseStream<'_>) -> syn::Result<Child> {
    if input.peek(Token![<]) {
        return Ok(Child::Element(parse_element(input)?));
    }
    if input.peek(LitStr) {
        return Ok(Child::Text(input.parse()?));
    }
    if input.peek(token::Brace) {
        return Ok(Child::Expr(braced_expr(input)?));
    }
    if input.peek(Token![if]) {
        return Ok(Child::If(parse_if(input)?));
    }
    if input.peek(Token![for]) {
        input.parse::<Token![for]>()?;
        let pat = Pat::parse_multi_with_leading_vert(input)?;
        input.parse::<Token![in]>()?;
        let expr = Expr::parse_without_eager_brace(input)?;
        let body = parse_block(input)?;
        return Ok(Child::For {
            pat: Box::new(pat),
            expr,
            body,
        });
    }
    Err(input
        .error("text must be a string literal or `{expr}`: write `\"Hello\"` or `{&self.name}`"))
}

fn parse_if(input: ParseStream<'_>) -> syn::Result<If> {
    input.parse::<Token![if]>()?;
    let cond = Expr::parse_without_eager_brace(input)?;
    let then = parse_block(input)?;
    let otherwise = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        Some(if input.peek(Token![if]) {
            Else::If(Box::new(parse_if(input)?))
        } else {
            Else::Block(parse_block(input)?)
        })
    } else {
        None
    };
    Ok(If {
        cond,
        then,
        otherwise,
    })
}

/// `{ child* }`.
fn parse_block(input: ParseStream<'_>) -> syn::Result<Vec<Child>> {
    let content;
    braced!(content in input);
    let mut children = Vec::new();
    while !content.is_empty() {
        children.push(parse_child(&content)?);
    }
    Ok(children)
}

/// Every `tw = "…"` literal of the tree, in source order.
fn literals<'a>(element: &'a Element, out: &mut Vec<&'a LitStr>) {
    if let Some(Tw::Lit(lit)) = &element.tw {
        out.push(lit);
    }
    children_literals(&element.children, out);
}

fn children_literals<'a>(children: &'a [Child], out: &mut Vec<&'a LitStr>) {
    for child in children {
        match child {
            Child::Element(element) => literals(element, out),
            Child::If(branch) => if_literals(branch, out),
            Child::For { body, .. } => children_literals(body, out),
            Child::Text(_) | Child::Expr(_) => {}
        }
    }
}

fn if_literals<'a>(branch: &'a If, out: &mut Vec<&'a LitStr>) {
    children_literals(&branch.then, out);
    match &branch.otherwise {
        Some(Else::If(next)) => if_literals(next, out),
        Some(Else::Block(children)) => children_literals(children, out),
        None => {}
    }
}

/// `view! { .. }`.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let view: View = syn::parse2(input)?;
    let mut lits = Vec::new();
    literals(&view.root, &mut lits);
    let theme = crate_theme(lits.first().map_or_else(Span::call_site, |l| l.span()))?;
    combine(
        lits.iter()
            .flat_map(|lit| check_literal(lit, &theme.theme))
            .collect(),
    )?;
    let tracking = theme.tracking();
    let node = lower_element(&view.root);
    Ok(quote! {{
        #tracking
        #node
    }})
}

fn lower_element(element: &Element) -> TokenStream {
    let tag = format_ident!("{}", element.tag);
    let mut out =
        quote!(::noctis::__private::image::Node::element(::noctis::__private::image::Tag::#tag));
    match &element.tw {
        Some(Tw::Lit(lit)) => {
            out.extend(quote!(.classes(::noctis::__private::image::Classes::__literal(#lit))));
        }
        Some(Tw::Expr(expr)) => out.extend(quote!(.classes(#expr))),
        None => {}
    }
    if let Some(style) = &element.style {
        out.extend(quote!(.style(#style)));
    }
    for child in &element.children {
        let child = lower_child(child);
        out.extend(quote!(.child(#child)));
    }
    out
}

fn lower_child(child: &Child) -> TokenStream {
    match child {
        Child::Element(element) => lower_element(element),
        Child::Text(lit) => quote!(::noctis::__private::image::Node::text(#lit)),
        Child::Expr(expr) => quote!(#expr),
        Child::If(branch) => lower_if(branch),
        Child::For { pat, expr, body } => {
            let nodes = nodes_ident();
            let pushes = pushes(&nodes, body);
            quote! {{
                #[allow(unused_mut)]
                let mut #nodes = ::std::vec::Vec::<::noctis::__private::image::Node>::new();
                for #pat in #expr {
                    #pushes
                }
                #nodes
            }}
        }
    }
}

fn lower_if(branch: &If) -> TokenStream {
    let cond = &branch.cond;
    let then = node_list(&branch.then);
    let otherwise = match &branch.otherwise {
        Some(Else::If(next)) => lower_if(next),
        Some(Else::Block(children)) => node_list(children),
        None => node_list(&[]),
    };
    quote!(if #cond #then else { #otherwise })
}

/// `{ let mut nodes = Vec::new(); <push each child>; nodes }`.
fn node_list(children: &[Child]) -> TokenStream {
    let nodes = nodes_ident();
    let pushes = pushes(&nodes, children);
    quote! {{
        #[allow(unused_mut)]
        let mut #nodes = ::std::vec::Vec::<::noctis::__private::image::Node>::new();
        #pushes
        #nodes
    }}
}

fn pushes(nodes: &Ident, children: &[Child]) -> TokenStream {
    children
        .iter()
        .map(|child| {
            let child = lower_child(child);
            quote!(::noctis::__private::image::Node::__push(&mut #nodes, #child);)
        })
        .collect()
}

/// A hygienic local, so user expressions cannot name it.
fn nodes_ident() -> Ident {
    Ident::new("__noctis_nodes", Span::mixed_site())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(tokens: TokenStream) -> String {
        match syn::parse2::<View>(tokens) {
            Ok(_) => panic!("expected a parse error"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn view_grammar_ok() {
        let view: View = syn::parse2(quote! {
            <div tw="flex" style={Style::new()}>
                "text"
                {&self.name}
                <span tw={classes} />
                if a { <p>"a"</p> } else if b { "b" } else { {c} }
                for x in xs { <h1>{x}</h1> <h2/> <h3/> }
            </div>
        })
        .unwrap();
        let mut lits = Vec::new();
        literals(&view.root, &mut lits);
        assert_eq!(lits.len(), 1);
        assert_eq!(view.root.children.len(), 5);
    }

    #[test]
    fn view_body_shape_errors() {
        assert!(err(quote!()).contains("exactly one root element"));
        assert!(err(quote!(<div/><div/>)).contains("exactly one root element"));
        assert!(err(quote!("text")).contains("exactly one root element"));
    }

    #[test]
    fn view_tag_errors() {
        assert_eq!(
            err(quote!(<img/>)),
            "`<img>`/`<svg>` are planned (Story 10.7)"
        );
        assert_eq!(
            err(quote!(<svg/>)),
            "`<img>`/`<svg>` are planned (Story 10.7)"
        );
        assert_eq!(
            err(quote!(<button/>)),
            "unknown tag `<button>`: use `div`, `span`, `p`, `h1`, `h2` or `h3`"
        );
        assert_eq!(
            err(quote!(<div></span>)),
            "mismatched closing tag: expected `</div>`, found `</span>`"
        );
        assert_eq!(
            err(quote!(<div>)),
            "`<div>` is missing its closing tag `</div>`"
        );
        assert_eq!(err(quote!(<div)), "`<div` is missing its `>`");
    }

    #[test]
    fn view_attribute_errors() {
        assert_eq!(
            err(quote!(<div class="x"/>)),
            "unknown attribute `class`: use `tw` for classes (`tw=\"flex p-4\"`)"
        );
        assert_eq!(
            err(quote!(<div tw="a" tw="b"/>)),
            "duplicate attribute `tw`"
        );
        assert_eq!(
            err(quote!(<div style={a} style={b}/>)),
            "duplicate attribute `style`"
        );
        assert_eq!(
            err(quote!(<div onclick={f}/>)),
            "unknown attribute `onclick`: elements take `tw` and `style`"
        );
        assert!(err(quote!(<div tw=5/>)).starts_with("`tw` takes a string literal"));
        assert!(err(quote!(<div style="x"/>)).starts_with("`style` takes `{expr}`"));
    }

    #[test]
    fn view_bare_text_error() {
        assert_eq!(
            err(quote!(<p>Hello</p>)),
            "text must be a string literal or `{expr}`: write `\"Hello\"` or `{&self.name}`"
        );
        assert!(err(quote!(<p>42</p>)).starts_with("text must be a string literal"));
    }
}
