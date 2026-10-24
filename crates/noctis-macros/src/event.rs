//! `#[derive(EventHandler)]` on a unit struct, with its optional
//! `#[event(ordered, droppable, flags = [..])]`: declares a typed gateway
//! event handler for `#[handler]` (FR-6, FR-73, AD-7).

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Meta};

use crate::command::{segments, span_of};
use crate::flags::Flags;

/// What `#[event(..)]` takes.
#[derive(Default)]
struct Args {
    ordered: bool,
    droppable: bool,
    /// `flags = [..]`: the Boot Flags.
    flags: Flags,
    /// Whether `flags` was given.
    has_flags: bool,
}

const EXPECTED: &str = "expected `ordered`, `droppable` or `flags = [\"beta\"]`";

/// Parses `ordered`, `droppable` and `flags = [..]` (any order, each at
/// most once).
fn parse_args(args: TokenStream) -> syn::Result<Args> {
    let mut parsed = Args::default();
    let segments = segments(args);
    if segments.len() == 1 && segments[0].is_empty() {
        return Ok(parsed);
    }
    for segment in segments {
        let tokens: TokenStream = segment.iter().cloned().collect();
        if matches!(segment.first(), Some(TokenTree::Ident(i)) if i == "flags") {
            if parsed.has_flags {
                return Err(syn::Error::new(
                    span_of(&segment),
                    "duplicate `#[event]` argument `flags`",
                ));
            }
            parsed.flags = Flags::parse(&segment)?;
            parsed.has_flags = true;
            continue;
        }
        let name = match segment.as_slice() {
            [TokenTree::Ident(ident)] => Some(ident.to_string()),
            _ => None,
        };
        let slot = match name.as_deref() {
            Some("ordered") => &mut parsed.ordered,
            Some("droppable") => &mut parsed.droppable,
            _ => {
                let span = segment
                    .first()
                    .map_or_else(Span::call_site, TokenTree::span);
                return Err(syn::Error::new(
                    span,
                    format!("unknown `#[event]` argument `{tokens}`: {EXPECTED}"),
                ));
            }
        };
        if *slot {
            return Err(syn::Error::new_spanned(
                &tokens,
                format!("duplicate `#[event]` argument `{tokens}`"),
            ));
        }
        *slot = true;
    }
    Ok(parsed)
}

const UNIT: &str = "`#[derive(EventHandler)]` needs a unit struct";

/// Expands the derive; on an error, the error and the placeholder impls
/// (`kind::fallback`).
pub(crate) fn derive_or_fallback(input: &DeriveInput) -> TokenStream {
    let result = derive(input);
    crate::kind::or_fallback(input, crate::kind::Family::Event, result, || {
        let ident = &input.ident;
        quote! {
            impl ::noctis::__private::core::__EventHandler for #ident {
                const ORDERED: ::core::primitive::bool = false;
                const DROPPABLE: ::core::primitive::bool = false;

                fn __new() -> Self {
                    ::core::unreachable!("the derive failed to compile")
                }
            }
        }
    })
}

/// Expands `#[derive(EventHandler)]` with its optional `#[event(..)]`.
pub(crate) fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    crate::common::kind_fields(input, "EventHandler")?;
    let ident = &input.ident;
    if !matches!(&input.data, Data::Struct(d) if matches!(d.fields, Fields::Unit)) {
        return Err(syn::Error::new_spanned(ident, UNIT));
    }
    let mut attrs = input.attrs.iter().filter(|a| a.path().is_ident("event"));
    let args = match attrs.next() {
        None => Args::default(),
        Some(attr) => {
            if let Some(second) = attrs.next() {
                return Err(syn::Error::new_spanned(
                    second,
                    "a struct takes one #[event(..)] attribute",
                ));
            }
            match &attr.meta {
                Meta::Path(_) => Args::default(),
                Meta::List(list) => parse_args(list.tokens.clone())?,
                Meta::NameValue(_) => {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[event]` takes `ordered`, `droppable` and `flags = [..]`: `#[event(ordered)]`",
                    ));
                }
            }
        }
    };
    let core = quote!(::noctis::__private::core);
    let (ordered, droppable) = (args.ordered, args.droppable);
    let scaffolding =
        crate::kind::scaffolding(ident, crate::kind::Family::Event, false, &args.flags);
    Ok(quote! {
        #[automatically_derived]
        impl #core::__EventHandler for #ident {
            const ORDERED: ::core::primitive::bool = #ordered;
            const DROPPABLE: ::core::primitive::bool = #droppable;

            fn __new() -> Self {
                Self
            }
        }

        #scaffolding
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: TokenStream) -> String {
        let input: DeriveInput = syn::parse2(src).unwrap();
        match derive(&input) {
            Ok(t) => t.to_string(),
            Err(e) => e.to_string(),
        }
    }

    /// Story 6.11 Matrix 12.
    #[test]
    fn expands() {
        let out = run(quote!(
            #[event(ordered)]
            pub struct AwardXp;
        ));
        assert!(
            out.contains("const ORDERED : :: core :: primitive :: bool = true"),
            "{out}"
        );
        assert!(
            out.contains("const DROPPABLE : :: core :: primitive :: bool = false"),
            "{out}"
        );
        assert!(out.contains("__EventKind"), "{out}");
        assert!(out.contains("Register for AwardXp"), "{out}");
        let out = run(quote!(
            struct Plain;
        ));
        assert!(
            out.contains("const ORDERED : :: core :: primitive :: bool = false"),
            "{out}"
        );
        let out = run(quote!(
            #[event]
            struct Plain;
        ));
        assert!(
            out.contains("const DROPPABLE : :: core :: primitive :: bool = false"),
            "{out}"
        );
    }

    #[test]
    fn args() {
        for (args, expected) in [
            (quote!(), (false, false)),
            (quote!(ordered), (true, false)),
            (quote!(droppable), (false, true)),
            (quote!(ordered, droppable), (true, true)),
            (quote!(droppable, ordered), (true, true)),
            (quote!(ordered,), (true, false)),
            (quote!(flags = ["beta"], ordered), (true, false)),
        ] {
            let parsed = parse_args(args.clone()).unwrap();
            assert_eq!((parsed.ordered, parsed.droppable), expected, "{args}");
        }
    }

    /// Story 6.11 Matrix 6 and 12: every rejected form names its fix.
    #[test]
    fn errors() {
        for (src, expected) in [
            (
                quote!(
                    struct S {
                        a: u8,
                    }
                ),
                UNIT,
            ),
            (
                quote!(
                    struct S {}
                ),
                UNIT,
            ),
            (
                quote!(
                    enum E {
                        A,
                    }
                ),
                "`#[derive(EventHandler)]` needs a struct with named fields or a unit struct",
            ),
            (
                quote!(
                    #[event(x)]
                    struct S;
                ),
                "unknown `#[event]` argument `x`: expected `ordered`, `droppable` or `flags = [\"beta\"]`",
            ),
            (
                quote!(
                    #[event(ordered, ordered)]
                    struct S;
                ),
                "duplicate `#[event]` argument `ordered`",
            ),
            (
                quote!(
                    #[event(ordered = true)]
                    struct S;
                ),
                "unknown `#[event]` argument `ordered = true`: expected `ordered`, `droppable` or `flags = [\"beta\"]`",
            ),
        ] {
            assert_eq!(run(src.clone()), expected, "{src}");
        }
    }

    /// Story 13.2 M1-M8 on `#[event]`.
    #[test]
    fn boot_flags() {
        let out = run(quote!(
            #[event(droppable, flags = ["beta", "eco"])]
            struct Gone;
        ));
        assert!(
            out.contains("__add_flagged (& [\"beta\" , \"eco\"]"),
            "{out}"
        );
        assert!(
            out.contains("const DROPPABLE : :: core :: primitive :: bool = true"),
            "{out}"
        );
        for (args, expected) in [
            (quote!(flags = []), crate::flags::EMPTY),
            (quote!(flags = "beta"), crate::flags::NOT_A_LIST),
            (quote!(flags = [beta]), crate::flags::NOT_A_LIST),
            (quote!(flags = [1]), crate::flags::NOT_A_LIST),
            (quote!(flags), crate::flags::NOT_A_LIST),
            (
                quote!(flags = ["Beta"]),
                "invalid Boot Flag `Beta` in `flags`: `B` at index 0 is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-`",
            ),
            (
                quote!(flags = ["all"]),
                "invalid Boot Flag `all` in `flags`: `all` is reserved: write `all` or `none` alone, never in a list or as a declared flag",
            ),
            (
                quote!(flags = ["a", "a"]),
                "invalid Boot Flag `a` in `flags`: a Boot Flag is given twice: keep one",
            ),
            (
                quote!(flags = ["a"], flags = ["b"]),
                "duplicate `#[event]` argument `flags`",
            ),
        ] {
            let out = run(quote!(
                #[event(#args)]
                struct S;
            ));
            assert_eq!(out, expected, "{args}");
        }
    }
}
