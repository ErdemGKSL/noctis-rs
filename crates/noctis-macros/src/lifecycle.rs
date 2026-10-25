//! `#[derive(Hook)]` on a unit struct, with its optional
//! `#[hook(flags = [..])]`: declares a Lifecycle Hook for `#[handler]`
//! (FR-54, AD-21). The handler's event parameter (a `noctis::lifecycle`
//! type) selects the event.

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Meta};

use crate::command::{segments, span_of};
use crate::flags::Flags;

const EXPECTED: &str = "expected `flags = [\"beta\"]`";

const UNIT: &str = "`#[derive(Hook)]` needs a unit struct";

const NAME_VALUE: &str = "`#[hook]` takes `flags = [..]`: `#[hook(flags = [\"beta\"])]`";

/// Parses `flags = [..]` (at most once); every other argument is an error.
fn parse_args(args: TokenStream) -> syn::Result<Flags> {
    let mut flags = Flags::default();
    let segments = segments(args);
    if segments.len() == 1 && segments[0].is_empty() {
        return Ok(flags);
    }
    for segment in segments {
        if matches!(segment.first(), Some(TokenTree::Ident(i)) if i == "flags") {
            if !flags.is_empty() {
                return Err(syn::Error::new(
                    span_of(&segment),
                    "duplicate `#[hook]` argument `flags`",
                ));
            }
            flags = Flags::parse(&segment)?;
            continue;
        }
        let tokens: TokenStream = segment.iter().cloned().collect();
        let span = segment
            .first()
            .map_or_else(Span::call_site, TokenTree::span);
        return Err(syn::Error::new(
            span,
            format!("unknown `#[hook]` argument `{tokens}`: {EXPECTED}"),
        ));
    }
    Ok(flags)
}

/// Expands the derive; on an error, the error and the placeholder impls
/// (`kind::fallback`).
pub(crate) fn derive_or_fallback(input: &DeriveInput) -> TokenStream {
    let result = derive(input);
    crate::kind::or_fallback(input, crate::kind::Family::Hook, result, || {
        let ident = &input.ident;
        quote! {
            impl ::noctis::__private::core::__LifecycleHook for #ident {
                fn __new() -> Self {
                    ::core::unreachable!("the derive failed to compile")
                }
            }
        }
    })
}

/// Expands `#[derive(Hook)]` with its optional `#[hook(..)]`.
pub(crate) fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let unit = matches!(&input.data, Data::Struct(d) if matches!(d.fields, Fields::Unit));
    if !unit || !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(ident, UNIT));
    }
    let mut attrs = input.attrs.iter().filter(|a| a.path().is_ident("hook"));
    let flags = match attrs.next() {
        None => Flags::default(),
        Some(attr) => {
            if let Some(second) = attrs.next() {
                return Err(syn::Error::new_spanned(
                    second,
                    "a struct takes one #[hook(..)] attribute",
                ));
            }
            match &attr.meta {
                Meta::Path(_) => Flags::default(),
                Meta::List(list) => parse_args(list.tokens.clone())?,
                Meta::NameValue(_) => return Err(syn::Error::new_spanned(attr, NAME_VALUE)),
            }
        }
    };
    let core = quote!(::noctis::__private::core);
    let scaffolding = crate::kind::scaffolding(ident, crate::kind::Family::Hook, false, &flags);
    Ok(quote! {
        #[automatically_derived]
        impl #core::__LifecycleHook for #ident {
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

    /// H1, H2.
    #[test]
    fn expands() {
        for src in [
            quote!(
                pub struct Up;
            ),
            quote!(
                #[hook]
                pub struct Up;
            ),
            quote!(
                #[hook()]
                pub struct Up;
            ),
        ] {
            let out = run(src.clone());
            assert!(out.contains("__LifecycleHook for Up"), "{out}");
            assert!(out.contains("__LifecycleKind"), "{out}");
            assert!(out.contains("LifecycleHook ;"), "{out}");
            assert!(out.contains("Register for Up"), "{out}");
            assert!(out.contains("module . __add (<"), "{out}");
            assert!(out.contains("__NOCTIS_HAS_HANDLER"), "{out}");
        }
    }

    /// H3, H4, H6: the Boot Flags, checked as on `#[event]`.
    #[test]
    fn boot_flags() {
        let out = run(quote!(
            #[hook(flags = ["beta"])]
            struct Up;
        ));
        assert!(out.contains("__add_flagged (& [\"beta\"]"), "{out}");
        for (args, expected) in [
            (quote!(flags = []), crate::flags::EMPTY),
            (quote!(flags = "beta"), crate::flags::NOT_A_LIST),
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
                "duplicate `#[hook]` argument `flags`",
            ),
        ] {
            let out = run(quote!(
                #[hook(#args)]
                struct S;
            ));
            assert_eq!(out, expected, "{args}");
        }
        // A 33-character flag and a non-ASCII one fail like on `#[event]`.
        for flag in ["a".repeat(33), "bêta".to_owned()] {
            let lit = syn::LitStr::new(&flag, Span::call_site());
            let hook = run(quote!(
                #[hook(flags = [#lit])]
                struct S;
            ));
            let event_input: DeriveInput = syn::parse2(quote!(
                #[event(flags = [#lit])]
                struct S;
            ))
            .unwrap();
            let event = crate::event::derive(&event_input).unwrap_err().to_string();
            assert_eq!(hook, event, "{flag}");
        }
    }

    /// H5-H9: every rejected form names its fix.
    #[test]
    fn errors() {
        for (src, expected) in [
            (
                quote!(
                    #[hook(ordered)]
                    struct S;
                ),
                "unknown `#[hook]` argument `ordered`: expected `flags = [\"beta\"]`",
            ),
            (
                quote!(
                    #[hook(x = 1)]
                    struct S;
                ),
                "unknown `#[hook]` argument `x = 1`: expected `flags = [\"beta\"]`",
            ),
            (
                quote!(
                    #[hook]
                    #[hook]
                    struct S;
                ),
                "a struct takes one #[hook(..)] attribute",
            ),
            (
                quote!(
                    #[hook = "x"]
                    struct S;
                ),
                NAME_VALUE,
            ),
            (
                quote!(
                    struct S(u8);
                ),
                UNIT,
            ),
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
                    enum E {
                        A,
                    }
                ),
                UNIT,
            ),
            (
                quote!(
                    struct S<T>;
                ),
                UNIT,
            ),
        ] {
            assert_eq!(run(src.clone()), expected, "{src}");
        }
    }
}
