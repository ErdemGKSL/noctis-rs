//! `#[autocomplete(field)]`: the suggestion method of the option `field` of
//! a `#[derive(SlashCommand)]` struct (FR-10, AD-11; OPT-11, OPT-12,
//! INT-06; maintainer decisions §8).
//!
//! The method stays as written: `async fn <any>(ctx: AutocompleteCtx,
//! typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<T>>`.
//! Beside it, in the same `impl`, the macro emits the hidden
//! `__NOCTIS_AC_<field>` const (the derive's OPT-11 check reads it) and the
//! hidden `__noctis_ac_<field>()` binding, which core's
//! `__autocomplete_method` bounds by `Self: __AutocompleteField<{hash}>`
//! (the field exists and is flagged) and by `__SuggestionsFor<Value>` (the
//! return type matches the field's type). The macro writes the wrapper
//! closure itself, so no higher-ranked bound over the borrowed `&str` is
//! needed.

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ImplItemFn, Type};

use crate::command::{arg_name, segments};
use crate::common::{fnv1a64, plain};

const SIGNATURE: &str = "an `#[autocomplete]` method takes `(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>)`";
const NO_SELF: &str = "an `#[autocomplete]` method takes no `self`: the options are incomplete, so it gets `filled: Partial<Self>`";
const ASYNC: &str = "an `#[autocomplete]` method must be `async`";
const GENERIC: &str = "an `#[autocomplete]` method cannot be generic";
const ONE_FIELD: &str = "`#[autocomplete(field)]` names exactly one field";
const OTHER_ARGS: &str = "`#[autocomplete(field)]` takes no other arguments";
const INT_06: &str = "[INT-06] autocomplete cannot be deferred: `#[autocomplete]` takes no `auto_defer` or `response`";
const NO_EXT: &str = "autocomplete runs no Middleware, so it takes no `Ext<T>`";

/// The error of a bare `#[autocomplete]`.
fn bare(method: &str) -> String {
    format!(
        "`#[autocomplete]` needs the field it suggests for: `#[autocomplete(field)]`\n  = help: #[derive(SlashCommand)] struct Warn {{ #[option(description = \"…\", autocomplete)] rule: i64 }} impl Warn {{ #[autocomplete(rule)] async fn {method}(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<i64>> {{ .. }} }}"
    )
}

/// Whether `ty`'s last path segment is `name` without arguments.
fn last_is(ty: &Type, name: &str) -> bool {
    match ty {
        Type::Path(path) => {
            path.qself.is_none()
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == name && s.arguments.is_none())
        }
        Type::Group(g) => last_is(&g.elem, name),
        _ => false,
    }
}

/// Whether `ty` is `&str`.
fn is_str_ref(ty: &Type) -> bool {
    match ty {
        Type::Reference(r) => r.mutability.is_none() && last_is(&r.elem, "str"),
        Type::Group(g) => is_str_ref(&g.elem),
        _ => false,
    }
}

/// Parses the attribute arguments: exactly one field identifier.
fn parse_field(args: TokenStream, method: &Ident) -> syn::Result<Ident> {
    if args.is_empty() {
        return Err(syn::Error::new(Span::call_site(), bare(&plain(method))));
    }
    let segments = segments(args.clone());
    let deferral = segments.iter().enumerate().any(|(i, segment)| {
        arg_name(segment).is_some_and(|n| n == "auto_defer" || n == "response")
            && (i > 0 || segment.len() > 1)
    });
    if deferral {
        return Err(syn::Error::new_spanned(args, INT_06));
    }
    let field = match segments.first().map(Vec::as_slice) {
        Some([TokenTree::Ident(ident)]) => syn::parse2::<Ident>(ident.to_token_stream())?,
        _ => return Err(syn::Error::new_spanned(args, ONE_FIELD)),
    };
    let rest: TokenStream = segments
        .iter()
        .skip(1)
        .flat_map(|s| s.iter().cloned())
        .collect();
    match segments.get(1).map(Vec::as_slice) {
        None => Ok(field),
        // A second bare identifier reads as a second field.
        Some([TokenTree::Ident(_)]) => Err(syn::Error::new_spanned(rest, ONE_FIELD)),
        Some(_) => Err(syn::Error::new_spanned(rest, OTHER_ARGS)),
    }
}

/// Checks the method's shape.
fn check(method: &ImplItemFn) -> syn::Result<()> {
    let sig = &method.sig;
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new_spanned(receiver, NO_SELF));
    }
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(sig.fn_token, ASYNC));
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(&sig.generics, GENERIC));
    }
    let typed: Vec<&Type> = sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(&*t.ty),
            FnArg::Receiver(_) => None,
        })
        .collect();
    if let Some(ext) = typed.iter().find(|ty| crate::handler::is_ext(ty)) {
        return Err(syn::Error::new_spanned(ext, NO_EXT));
    }
    match typed.as_slice() {
        [ctx, typed, filled]
            if last_is(ctx, "AutocompleteCtx") && is_str_ref(typed) && !is_str_ref(filled) =>
        {
            Ok(())
        }
        [] => Err(syn::Error::new(sig.ident.span(), SIGNATURE)),
        _ => Err(syn::Error::new(sig.inputs.span(), SIGNATURE)),
    }
}

/// The hidden const and binding names of `field`.
pub(crate) fn names(field: &str, span: Span) -> (Ident, Ident) {
    (
        Ident::new(&format!("__NOCTIS_AC_{field}"), span),
        Ident::new(&format!("__noctis_ac_{field}"), span),
    )
}

/// Expands `#[autocomplete(field)] item`. On an error, the method stays,
/// with the error and, when the field is named, placeholder items so the
/// derive's OPT-11 check adds no second error.
pub(crate) fn expand_or_fallback(args: TokenStream, item: TokenStream) -> TokenStream {
    match expand(args.clone(), item.clone()) {
        Ok(tokens) => tokens,
        Err(e) => {
            let error = e.into_compile_error();
            let Ok(method) = syn::parse2::<ImplItemFn>(item) else {
                return error;
            };
            let field = match segments(args).first().map(Vec::as_slice) {
                Some([TokenTree::Ident(ident)]) => Some(plain(&syn::parse_quote!(#ident))),
                _ => None,
            };
            let Some(field) = field else {
                return quote!(#error #method);
            };
            let (flag, binding) = names(&field, Span::call_site());
            let core = quote!(::noctis::__private::core);
            quote! {
                #error
                #method

                #[doc(hidden)]
                #[allow(non_upper_case_globals)]
                pub const #flag: ::core::primitive::bool = true;

                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub fn #binding() -> #core::__AutocompleteMethod {
                    #core::__AutocompleteMethod::__missing(#field)
                }
            }
        }
    }
}

/// Expands `#[autocomplete(field)] item`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let mut method: ImplItemFn = syn::parse2(item)?;
    let field = parse_field(args, &method.sig.ident)?;
    check(&method)?;
    let field_name = plain(&field);
    let hash = fnv1a64(&field_name);
    let (flag, binding) = names(&field_name, field.span());
    let name = method.sig.ident.clone();
    let label = plain(&name);
    let core = quote!(::noctis::__private::core);
    let (ctx, typed, filled) = (
        Ident::new("__noctis_ctx", Span::mixed_site()),
        Ident::new("__noctis_typed", Span::mixed_site()),
        Ident::new("__noctis_filled", Span::mixed_site()),
    );
    let output = match &method.sig.output {
        syn::ReturnType::Default => method.sig.ident.span(),
        syn::ReturnType::Type(_, ty) => ty.span(),
    };
    let bind = quote_spanned! {output=>
        #core::__autocomplete_method::<Self, #hash, _, _, _>(
            ::core::concat!(::core::module_path!(), "::", #label),
            move |#ctx, #typed: ::std::string::String, #filled| async move {
                Self::#name(#ctx, &#typed, #filled).await
            },
        )
    };
    method
        .attrs
        .push(syn::parse_quote!(#[allow(clippy::unused_async)]));
    Ok(quote! {
        #method

        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #flag: ::core::primitive::bool = true;

        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn #binding() -> #core::__AutocompleteMethod {
            #bind
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const METHOD: &str = "async fn rule(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<i64>> { todo!() }";

    fn run(args: TokenStream, src: &str) -> String {
        match expand(args, src.parse().unwrap()) {
            Ok(t) => t.to_string(),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn expands() {
        let out = run(quote!(rule), METHOD);
        assert!(
            out.contains("pub const __NOCTIS_AC_rule : :: core :: primitive :: bool = true"),
            "{out}"
        );
        assert!(out.contains("pub fn __noctis_ac_rule ()"), "{out}");
        let hash = fnv1a64("rule").to_string();
        assert!(
            out.contains(&format!("< Self , {hash}u64 , _ , _ , _ >")),
            "{out}"
        );
        assert!(out.contains("Self :: rule ("), "{out}");
        assert!(out.contains("# [allow (clippy :: unused_async)]"), "{out}");
        // Matrix 54: non-ASCII field names.
        let out = run(quote!(rüle), METHOD);
        assert!(out.contains("__NOCTIS_AC_rüle"), "{out}");
        let out = run(quote!(r#type), METHOD);
        assert!(out.contains("__noctis_ac_type"), "{out}");
    }

    /// Story 6.11 Matrix 20 and 21.
    #[test]
    fn rejects() {
        assert!(run(quote!(), METHOD).starts_with("`#[autocomplete]` needs the field"));
        assert!(run(quote!(), METHOD).contains("= help: #[derive(SlashCommand)]"));
        assert_eq!(run(quote!(a, b), METHOD), ONE_FIELD);
        assert_eq!(run(quote!("rule"), METHOD), ONE_FIELD);
        assert_eq!(run(quote!(rule, foo = 1), METHOD), OTHER_ARGS);
        assert_eq!(run(quote!(rule, auto_defer = false), METHOD), INT_06);
        assert_eq!(run(quote!(rule, response = ephemeral), METHOD), INT_06);
        assert_eq!(
            run(
                quote!(rule),
                "async fn f(&self, ctx: AutocompleteCtx, t: &str, p: P) {}"
            ),
            NO_SELF
        );
        assert_eq!(
            run(quote!(rule), "fn f(ctx: AutocompleteCtx, t: &str, p: P) {}"),
            ASYNC
        );
        assert_eq!(
            run(
                quote!(rule),
                "async fn f<T>(ctx: AutocompleteCtx, t: &str, p: P) {}"
            ),
            GENERIC
        );
        for src in [
            "async fn f(ctx: AutocompleteCtx) {}",
            "async fn f(t: &str, ctx: AutocompleteCtx, p: P) {}",
            "async fn f() {}",
            "async fn f(ctx: AutocompleteCtx, t: String, p: P) {}",
        ] {
            assert_eq!(run(quote!(rule), src), SIGNATURE, "{src}");
        }
    }

    /// Middleware Matrix 10: autocomplete takes no `Ext<T>`.
    #[test]
    fn autocomplete_takes_no_ext() {
        let src = "async fn rule(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>, a: Ext<A>) -> HandlerResult<Suggestions<i64>> { todo!() }";
        assert_eq!(run(quote!(rule), src), NO_EXT);
    }
}
