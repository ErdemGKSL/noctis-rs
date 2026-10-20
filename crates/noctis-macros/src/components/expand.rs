//! The builder expansion of a checked tree. Every runtime item is named
//! through `::noctis::__private::ui` in UFCS form, so no import is needed;
//! the expansion's own locals use mixed-site hygiene. Expressions keep their
//! spans and are evaluated once each, in source order.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::Ident;

use crate::literal::u32_literal;

use super::parse::{Arg, Kind, MediaArgs, Node, RouteNode, SpacingArg, lit_int};

fn ui() -> TokenStream {
    quote!(::noctis::__private::ui)
}

/// The whole body: a `Components`.
pub(crate) fn expand(nodes: &[Node]) -> TokenStream {
    let ui = ui();
    chain(
        quote!(#ui::Components::new()),
        &quote!(#ui::Components::push),
        nodes,
    )
}

/// `init` with every node pushed by `push`, in order; spreads push each item.
fn chain(init: TokenStream, push: &TokenStream, nodes: &[Node]) -> TokenStream {
    if !nodes.iter().any(|n| matches!(n.kind, Kind::Spread(_))) {
        return nodes.iter().fold(init, |acc, n| {
            let x = node(n);
            quote!(#push(#acc, #x))
        });
    }
    let c = Ident::new("__noctis_c", Span::mixed_site());
    let x = Ident::new("__noctis_x", Span::mixed_site());
    let stmts = nodes.iter().map(|n| {
        if let Kind::Spread(expr) = &n.kind {
            quote! {
                for #x in ::core::iter::IntoIterator::into_iter(#expr) {
                    #c = #push(#c, #x);
                }
            }
        } else {
            let value = node(n);
            quote!(#c = #push(#c, #value);)
        }
    });
    quote!({
        let mut #c = #init;
        #(#stmts)*
        #c
    })
}

fn media(arg: &Arg) -> TokenStream {
    let ui = ui();
    match arg {
        Arg::Lit(l) => {
            let lit = &l.lit;
            quote!(#ui::Media::url(#lit))
        }
        Arg::Expr(e) => quote!(#e),
    }
}

fn with_alt(ty: &TokenStream, m: &MediaArgs) -> TokenStream {
    let media = media(&m.media);
    let base = quote!(#ty::new(#media));
    match &m.alt {
        Some(alt) => quote!(#ty::alt(#base, #alt)),
        None => base,
    }
}

fn route(r: &RouteNode) -> TokenStream {
    let ui = ui();
    let RouteNode {
        receiver,
        method,
        arg,
        setters,
        kind,
        ..
    } = r;
    let mut out = if kind.is_select() {
        let options = arg.iter();
        quote!(#ui::RouteComponents::#method(#receiver #(, #options)*))
    } else {
        let base = quote!(#ui::RouteComponents::#method(#receiver));
        match arg {
            Some(label) => quote!(#base.label(#label)),
            None => base,
        }
    };
    for s in setters {
        let (m, t, a) = (&s.method, &s.turbofish, &s.args);
        out = quote!(#out.#m #t (#a));
    }
    out
}

fn node(n: &Node) -> TokenStream {
    let ui = ui();
    match &n.kind {
        Kind::Container { accent, children } => {
            let mut init = quote!(#ui::Container::new());
            if let Some(accent) = accent {
                let checked =
                    lit_int(accent).and_then(|i| i.base10_parse::<u32>().ok().map(u32_literal));
                let value = if let Some(lit) = checked {
                    quote!(#ui::Rgb::__from_checked(#lit))
                } else {
                    quote!(#accent)
                };
                init = quote!(#ui::Container::accent(#init, #value));
            }
            chain(init, &quote!(#ui::Container::push), children)
        }
        Kind::Section { texts, accessory } => {
            let texts = texts.iter().map(node);
            let accessory = node(accessory);
            quote!(#ui::Section::new([#(#texts),*], #accessory))
        }
        Kind::Text(arg) => quote!(#ui::TextDisplay::new(#arg)),
        Kind::Thumbnail(m) => with_alt(&quote!(#ui::Thumbnail), m),
        Kind::Item(m) => with_alt(&quote!(#ui::MediaItem), m),
        Kind::Gallery(items) => {
            let items = items.iter().map(node);
            quote!(#ui::MediaGallery::new([#(#items),*]))
        }
        Kind::File(expr) => quote!(#ui::File::new(#expr)),
        Kind::Separator { spacing, divider } => {
            let base = match spacing {
                Some(SpacingArg::Large) => quote!(#ui::Separator::large()),
                Some(SpacingArg::Small) => quote!(#ui::Separator::small()),
                _ => quote!(#ui::Separator::new()),
            };
            match divider {
                Some(d) => quote!(#ui::Separator::divider(#base, #d)),
                None => base,
            }
        }
        Kind::Row(items) => match items.as_slice() {
            [only] if matches!(&only.kind, Kind::Route(r) if r.kind.is_select()) => {
                let select = node(only);
                quote!(#ui::ActionRow::select(#select))
            }
            _ => {
                let buttons = items.iter().map(node);
                quote!(#ui::ActionRow::buttons([#(#ui::__macro::row_button(#buttons)),*]))
            }
        },
        Kind::Link { url, label } => {
            let (url, label) = (&url.lit, &label.lit);
            quote!(#ui::LinkButton::label(#ui::LinkButton::new(#url), #label))
        }
        Kind::Premium(expr) => quote!(#ui::PremiumButton::new(#expr)),
        Kind::Route(r) => route(r),
        Kind::Hole(stmts) => {
            if let [syn::Stmt::Expr(e, None)] = stmts.as_slice() {
                quote!(#e)
            } else {
                quote!({ #(#stmts)* })
            }
        }
        // Spreads are expanded by `chain`; elsewhere they are syntax errors.
        Kind::Spread(expr) => quote!(#expr),
    }
}
