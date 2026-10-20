//! `#[autocomplete]`: turns `async fn name(ctx: AutocompleteCtx[, partial:
//! Partial<O>]) -> HandlerResult<Responded>` into `fn name() ->
//! AutocompleteHandler<O>` (FR-10, AD-11; INT-06, OPT-12).
//!
//! With a `partial` parameter the descriptor's options type is
//! `<P as PartialOptions>::Options`; without one the descriptor is generic
//! over any options struct, inferred where `#[option(autocomplete = path)]`
//! links it.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, ReturnType, Type};

use crate::command::check_shape;

const ATTR: &str = "#[autocomplete]";
const SIGNATURE: &str =
    "`#[autocomplete]` handlers take `(ctx: AutocompleteCtx[, partial: Partial<Options>])`";

/// Whether `ty`'s last path segment is `AutocompleteCtx`.
fn is_ctx(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            path.qself.is_none()
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "AutocompleteCtx" && s.arguments.is_none())
        }
        _ => false,
    }
}

/// `(ctx: AutocompleteCtx[, partial: P])`: the partial type, if taken.
fn check_params(item: &ItemFn) -> syn::Result<Option<&Type>> {
    let sig = &item.sig;
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new_spanned(receiver, SIGNATURE));
    }
    let typed: Vec<&syn::PatType> = sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(t),
            FnArg::Receiver(_) => None,
        })
        .collect();
    match typed.as_slice() {
        [ctx] | [ctx, _] if !is_ctx(&ctx.ty) => Err(syn::Error::new_spanned(&ctx.ty, SIGNATURE)),
        [_] => Ok(None),
        [_, partial] => Ok(Some(&*partial.ty)),
        [] => Err(syn::Error::new(sig.ident.span(), SIGNATURE)),
        _ => Err(syn::Error::new(sig.inputs.span(), SIGNATURE)),
    }
}

/// Expands `#[autocomplete] item`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        let deferral = crate::command::segments(args.clone())
            .iter()
            .filter_map(|segment| crate::command::arg_name(segment))
            .any(|name| name == "auto_defer" || name == "response");
        let message = if deferral {
            "[INT-06] autocomplete cannot be deferred: `#[autocomplete]` takes no `auto_defer` or `response`"
        } else {
            "`#[autocomplete]` takes no arguments"
        };
        return Err(syn::Error::new_spanned(args, message));
    }
    let item: ItemFn = syn::parse2(item)?;
    check_shape(&item, ATTR)?;
    let partial = check_params(&item)?;

    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        ..
    } = &item;
    let name = &sig.ident;
    let fn_name = name.to_string();
    let fn_name = fn_name.strip_prefix("r#").unwrap_or(&fn_name).to_owned();
    let mut inner = sig.clone();
    inner.ident = Ident::new("__noctis_autocomplete", name.span());
    let inner = inner.into_token_stream();
    let output_span = match &sig.output {
        ReturnType::Default => sig.ident.span(),
        ReturnType::Type(_, ty) => ty.span(),
    };
    let core = quote!(::noctis::__private::core);
    let handler = Ident::new("__noctis_autocomplete", output_span);
    let label = quote!(::core::concat!(::core::module_path!(), "::", #fn_name));
    let (generics, options, call) = if let Some(partial) = partial {
        let options = quote!(<#partial as #core::PartialOptions>::Options);
        let constructor = Ident::new("__new", output_span);
        let call = quote_spanned! {output_span=>
            #core::AutocompleteHandler::#constructor::<#partial, _, _>(#label, #handler)
        };
        (TokenStream::new(), options, call)
    } else {
        let o = Ident::new("__NoctisOptions", Span::call_site());
        let constructor = Ident::new("__new_plain", output_span);
        let call = quote_spanned! {output_span=>
            #core::AutocompleteHandler::#constructor::<_, _>(#label, #handler)
        };
        (quote!(<#o: #core::Options>), o.into_token_stream(), call)
    };
    Ok(quote! {
        #(#attrs)*
        #vis fn #name #generics() -> #core::AutocompleteHandler<#options> {
            // A handler needs no `.await` to be `async` (the macro requires it).
            #[allow(clippy::unused_async)]
            #inner #block
            #call
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPED: &str = "async fn suggest(ctx: AutocompleteCtx, partial: Partial<O>) -> HandlerResult<Responded> { Ok(ctx.respond(Vec::new())) }";
    const PLAIN: &str = "async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> { Ok(ctx.respond(Vec::new())) }";

    fn error(args: TokenStream, src: &str) -> String {
        expand(args, src.parse().unwrap()).unwrap_err().to_string()
    }

    /// Matrix 28 and the typed form.
    #[test]
    fn expands_both_forms() {
        let out = expand(
            TokenStream::new(),
            format!("/// docs\npub(crate) {TYPED}").parse().unwrap(),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("pub (crate) fn suggest () -> :: noctis :: __private :: core :: AutocompleteHandler < < Partial < O > as :: noctis :: __private :: core :: PartialOptions > :: Options >"), "{out}");
        assert!(out.contains("# [doc = \" docs\"]"), "{out}");
        assert!(out.contains("# [allow (clippy :: unused_async)]"), "{out}");
        assert!(
            out.contains(
                "async fn __noctis_autocomplete (ctx : AutocompleteCtx , partial : Partial < O >)"
            ),
            "{out}"
        );
        assert!(out.contains("AutocompleteHandler :: __new :: < Partial < O > , _ , _ > (:: core :: concat ! (:: core :: module_path ! () , \"::\" , \"suggest\") , __noctis_autocomplete)"), "{out}");
        let out = expand(TokenStream::new(), PLAIN.parse().unwrap())
            .unwrap()
            .to_string();
        assert!(out.contains("fn suggest < __NoctisOptions : :: noctis :: __private :: core :: Options > () -> :: noctis :: __private :: core :: AutocompleteHandler < __NoctisOptions >"), "{out}");
        assert!(
            out.contains("AutocompleteHandler :: __new_plain :: < _ , _ >"),
            "{out}"
        );
        let out = expand(
            TokenStream::new(),
            "async fn r#type(ctx: noctis::AutocompleteCtx) -> HandlerResult<Responded> { todo!() }"
                .parse()
                .unwrap(),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("\"type\""), "{out}");
    }

    /// Matrix 31: every signature message.
    #[test]
    fn rejects_bad_signatures() {
        assert_eq!(
            error(quote!(ephemeral), PLAIN),
            "`#[autocomplete]` takes no arguments"
        );
        assert_eq!(
            error(quote!(foo), PLAIN),
            "`#[autocomplete]` takes no arguments"
        );
        for args in [quote!(auto_defer = false), quote!(response = ephemeral)] {
            assert_eq!(
                error(args, PLAIN),
                "[INT-06] autocomplete cannot be deferred: `#[autocomplete]` takes no `auto_defer` or `response`"
            );
        }
        assert_eq!(
            error(
                TokenStream::new(),
                "fn f(ctx: AutocompleteCtx) -> HandlerResult<Responded> { todo!() }"
            ),
            "`#[autocomplete]` needs an `async fn`"
        );
        assert_eq!(
            error(
                TokenStream::new(),
                "async fn f<T>(ctx: AutocompleteCtx) -> HandlerResult<Responded> { todo!() }"
            ),
            "`#[autocomplete]` handlers cannot be generic"
        );
        for src in [
            "async fn f(&self, ctx: AutocompleteCtx) -> HandlerResult<Responded> { todo!() }",
            "async fn f() -> HandlerResult<Responded> { todo!() }",
            "async fn f(ctx: AutocompleteCtx, a: Partial<O>, b: u8) -> HandlerResult<Responded> { todo!() }",
            "async fn f(ctx: CommandCtx) -> HandlerResult<Responded> { todo!() }",
            "async fn f(p: Partial<O>, ctx: AutocompleteCtx) -> HandlerResult<Responded> { todo!() }",
        ] {
            assert_eq!(error(TokenStream::new(), src), SIGNATURE, "{src}");
        }
    }
}
