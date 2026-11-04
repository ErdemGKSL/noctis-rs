//! The `module = "a/b"` argument every kind attribute shares (FR-81,
//! AD-31): the Module a discovered registration joins. The id is checked
//! here with the Module id grammar, so a bad id is a compile error; the
//! derive then submits a `Discovered` entry with `inventory`.

use proc_macro2::{TokenStream, TokenTree};
use quote::quote;
use syn::LitStr;

use crate::command::span_of;

/// The error of a value that is not one string literal.
pub(crate) const NOT_A_STRING: &str =
    "`module` takes the Module id as a string literal: write `module = \"profile\"`";

/// The most characters a Module id may have (`noctis_core::Module` ids).
const MAX_ID_LEN: usize = 64;

/// The `module = ".."` of a Route attribute (`RouteArgs` compares by
/// value; `LitStr` has no `PartialEq` without syn's `extra-traits`).
#[derive(Clone, Default)]
pub(crate) struct ModuleArg(pub(crate) Option<LitStr>);

impl PartialEq for ModuleArg {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref().map(LitStr::value) == other.0.as_ref().map(LitStr::value)
    }
}

impl Eq for ModuleArg {}

impl core::fmt::Debug for ModuleArg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.as_ref().map(LitStr::value).fmt(f)
    }
}

/// Whether `segment` is a `module = ..` argument.
pub(crate) fn is_module(segment: &[TokenTree]) -> bool {
    matches!(segment.first(), Some(TokenTree::Ident(i)) if i == "module")
}

/// Parses the whole `module = "a/b"` argument `segment`; `seen` is the
/// value already parsed for this attribute (a second one is an error).
pub(crate) fn parse(
    segment: &[TokenTree],
    seen: Option<&LitStr>,
    attribute: &str,
) -> syn::Result<LitStr> {
    if seen.is_some() {
        return Err(syn::Error::new(
            span_of(segment),
            format!("duplicate `{attribute}` argument `module`"),
        ));
    }
    let lit = match segment {
        [_, TokenTree::Punct(eq), TokenTree::Literal(lit)] if eq.as_char() == '=' => {
            let tokens = TokenStream::from(TokenTree::Literal(lit.clone()));
            syn::parse2::<LitStr>(tokens).ok()
        }
        _ => None,
    };
    let Some(lit) = lit else {
        return Err(syn::Error::new(span_of(segment), NOT_A_STRING));
    };
    check_lit(&lit)?;
    Ok(lit)
}

/// Checks the `module = ".."` value `lit` against the Module id grammar.
pub(crate) fn check_lit(lit: &LitStr) -> syn::Result<()> {
    check_id(&lit.value()).map_err(|reason| {
        syn::Error::new(
            lit.span(),
            format!("invalid Module id `{}` in `module`: {reason}", lit.value()),
        )
    })
}

/// The Module id grammar of `noctis_core::Module` (keep in step with
/// `noctis-core/src/module.rs`, `check_id`): 1 to 64 characters, segments
/// of `a-z`, `0-9`, `_` and `-` joined by single `/`. The reasons read as
/// `ModuleIdError`'s.
fn check_id(id: &str) -> Result<(), String> {
    const GRAMMAR: &str = "use segments of a-z, 0-9, `_` and `-` joined by `/`, such as \
                           `profile/badges`";
    if id.is_empty() {
        return Err(format!("the id is empty: {GRAMMAR}"));
    }
    let len = id.chars().count();
    if len > MAX_ID_LEN {
        return Err(format!(
            "the id has {len} characters: shorten it to at most {MAX_ID_LEN}"
        ));
    }
    let mut after_separator = true;
    for (index, ch) in id.chars().enumerate() {
        match ch {
            '/' if after_separator => {
                return Err(empty_segment(index));
            }
            '/' => after_separator = true,
            'a'..='z' | '0'..='9' | '_' | '-' => after_separator = false,
            _ => {
                return Err(format!(
                    "`{ch}` at index {index} is not allowed: use only a-z, 0-9, `_` and `-`, \
                     with `/` between segments"
                ));
            }
        }
    }
    if after_separator {
        return Err(empty_segment(len));
    }
    Ok(())
}

/// `ModuleIdError::EmptySegment`'s text.
fn empty_segment(index: usize) -> String {
    format!(
        "empty segment at index {index}: separate segments with exactly one `/`, with none at \
         the start or end"
    )
}

/// The `inventory::submit!` of the struct `ident` for the Module `module`;
/// nothing without one.
pub(crate) fn submit(ident: &syn::Ident, module: Option<&LitStr>) -> TokenStream {
    let Some(module) = module else {
        return TokenStream::new();
    };
    let core = quote!(::noctis::__private::core);
    quote! {
        #core::__inventory::submit! {
            #core::Discovered::__new(
                #module,
                <#ident as #core::__Kind>::NAME,
                <#ident as #core::Register>::__register,
            )
        }
    }
}

/// The `inventory::submit!` that registers the Template `ident` with the
/// Module `module` (`Module::template::<T>()`); nothing without one.
#[cfg(feature = "image")]
pub(crate) fn submit_template(ident: &syn::Ident, module: Option<&str>) -> TokenStream {
    let Some(module) = module else {
        return TokenStream::new();
    };
    let core = quote!(::noctis::__private::core);
    let name = LitStr::new(&ident.to_string(), ident.span());
    quote! {
        const _: () = {
            fn __noctis_register(module: #core::Module) -> #core::Module {
                module.template::<#ident>()
            }
            #core::__inventory::submit! {
                #core::Discovered::__new(
                    #module,
                    ::core::concat!(::core::module_path!(), "::", #name),
                    __noctis_register,
                )
            }
        };
    }
}

/// `#[noctis::module]` on `fn <name>() -> Module { .. }`: the fn unchanged
/// plus the `inventory::submit!` of its `DiscoveredModule` entry.
pub(crate) fn module_attr(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_module_attr(args, item).unwrap_or_else(syn::Error::into_compile_error)
}

/// The signature `#[noctis::module]` takes.
pub(crate) const MODULE_FN: &str = "`#[noctis::module]` goes on `fn <name>() -> Module { .. }`: \
                                    a free fn without arguments, generics, `async` or `unsafe` \
                                    that returns the Module";

fn expand_module_attr(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "`#[noctis::module]` takes no arguments: the fn's Module carries its id",
        ));
    }
    let function: syn::ItemFn =
        syn::parse2(item).map_err(|e| syn::Error::new(e.span(), MODULE_FN))?;
    let sig = &function.sig;
    let wrong = sig.asyncness.is_some()
        || matches!(sig.safety, syn::Safety::Unsafe(_))
        || sig.constness.is_some()
        || sig.abi.is_some()
        || !sig.inputs.is_empty()
        || sig.variadic.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
        || matches!(sig.output, syn::ReturnType::Default);
    if wrong {
        return Err(syn::Error::new_spanned(sig, MODULE_FN));
    }
    let ident = &sig.ident;
    let core = quote!(::noctis::__private::core);
    Ok(quote! {
        #function

        #core::__inventory::submit! {
            #core::DiscoveredModule::__new(#ident)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(tokens: TokenStream) -> Vec<TokenTree> {
        tokens.into_iter().collect()
    }

    #[test]
    fn parses_a_valid_id() {
        let lit = parse(&segment(quote!(module = "profile/badges")), None, "event").unwrap();
        assert_eq!(lit.value(), "profile/badges");
    }

    #[test]
    fn rejects_bad_values() {
        let err = |tokens: TokenStream| {
            parse(&segment(tokens), None, "#[event]")
                .unwrap_err()
                .to_string()
        };
        assert_eq!(err(quote!(module = profile)), NOT_A_STRING);
        assert_eq!(err(quote!(module)), NOT_A_STRING);
        assert!(err(quote!(module = "Profile")).contains("`P` at index 0"));
        assert!(err(quote!(module = "a//b")).contains("empty segment at index 2"));
        assert!(err(quote!(module = "a/")).contains("empty segment at index 2"));
        assert!(err(quote!(module = "")).contains("the id is empty"));
        let long = "a".repeat(65);
        assert!(err(quote!(module = #long)).contains("65 characters"));
    }

    #[test]
    fn rejects_a_second_module() {
        let first = LitStr::new("a", proc_macro2::Span::call_site());
        let err = parse(&segment(quote!(module = "b")), Some(&first), "#[hook]").unwrap_err();
        assert_eq!(err.to_string(), "duplicate `#[hook]` argument `module`");
    }

    #[test]
    fn submits_only_with_a_module() {
        let ident = syn::Ident::new("Ping", proc_macro2::Span::call_site());
        assert!(submit(&ident, None).is_empty());
        let module = LitStr::new("util", proc_macro2::Span::call_site());
        let out = submit(&ident, Some(&module)).to_string();
        assert!(out.contains("__inventory :: submit !"), "{out}");
        assert!(out.contains("Discovered :: __new"), "{out}");
        assert!(out.contains("\"util\""), "{out}");
    }

    #[test]
    fn module_attr_accepts_a_plain_fn() {
        let out = module_attr(
            TokenStream::new(),
            quote!(
                pub fn profile() -> Module {
                    Module::new("profile")
                }
            ),
        )
        .to_string();
        assert!(out.contains("pub fn profile"), "{out}");
        assert!(out.contains("DiscoveredModule :: __new (profile)"), "{out}");
    }

    #[test]
    fn module_attr_rejects_other_shapes() {
        let err = |args: TokenStream, item: TokenStream| module_attr(args, item).to_string();
        for item in [
            quote!(
                async fn m() -> Module {
                    todo!()
                }
            ),
            quote!(
                fn m(x: u8) -> Module {
                    todo!()
                }
            ),
            quote!(
                fn m<T>() -> Module {
                    todo!()
                }
            ),
            quote!(
                fn m() {}
            ),
            quote!(
                struct M;
            ),
        ] {
            assert!(err(TokenStream::new(), item).contains("goes on `fn <name>() -> Module"));
        }
        assert!(
            err(
                quote!(x),
                quote!(
                    fn m() -> Module {
                        todo!()
                    }
                )
            )
            .contains("takes no arguments")
        );
    }

    #[cfg(feature = "image")]
    #[test]
    fn template_submission_registers_the_template() {
        let ident = syn::Ident::new("Card", proc_macro2::Span::call_site());
        assert!(submit_template(&ident, None).is_empty());
        let out = submit_template(&ident, Some("profile")).to_string();
        assert!(out.contains("template :: < Card >"), "{out}");
        assert!(out.contains("\"profile\""), "{out}");
    }
}
