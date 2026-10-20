//! `#[event]`: turns an `async fn(ctx: EventCtx, event: <payload>)` into a
//! descriptor function returning an `EventHandler` (FR-6, AD-7).

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, ReturnType, Safety};

/// The flags `#[event(..)]` takes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Args {
    ordered: bool,
    droppable: bool,
}

const PARAMS: &str = "`#[event]` handlers take `(ctx: EventCtx, event: <payload>)`";
const PLAIN: &str = "`#[event]` handlers must be a plain `async fn`";

/// Parses `ordered`, `droppable` (any order, each at most once).
fn parse_args(args: TokenStream) -> syn::Result<Args> {
    let mut parsed = Args::default();
    let mut segments: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tt in args {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => segments.push(Vec::new()),
            _ => {
                if let Some(last) = segments.last_mut() {
                    last.push(tt);
                }
            }
        }
    }
    if segments.len() > 1 && segments.last().is_some_and(Vec::is_empty) {
        segments.pop();
    }
    if segments.len() == 1 && segments[0].is_empty() {
        return Ok(parsed);
    }
    for segment in segments {
        let tokens: TokenStream = segment.iter().cloned().collect();
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
                    format!(
                        "unknown `#[event]` argument `{tokens}`: expected `ordered` or `droppable`"
                    ),
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

/// Checks the signature: a plain `async fn` with two typed parameters.
fn check(item: &ItemFn) -> syn::Result<()> {
    let sig = &item.sig;
    if let Some(constness) = &sig.constness {
        return Err(syn::Error::new_spanned(constness, PLAIN));
    }
    if let Safety::Unsafe(unsafety) = &sig.safety {
        return Err(syn::Error::new_spanned(unsafety, PLAIN));
    }
    if let Safety::Safe(safety) = &sig.safety {
        return Err(syn::Error::new_spanned(safety, PLAIN));
    }
    if let Some(abi) = &sig.abi {
        return Err(syn::Error::new_spanned(abi, PLAIN));
    }
    if item.modifiers.require_empty().is_err() {
        return Err(syn::Error::new(sig.fn_token.span(), PLAIN));
    }
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            sig.fn_token,
            "`#[event]` needs an `async fn`",
        ));
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            "`#[event]` handlers cannot be generic",
        ));
    }
    if let Some(variadic) = &sig.variadic {
        return Err(syn::Error::new_spanned(variadic, PLAIN));
    }
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new_spanned(receiver, PARAMS));
    }
    if sig.inputs.len() != 2 || !sig.inputs.iter().all(|a| matches!(a, FnArg::Typed(_))) {
        let span = if sig.inputs.is_empty() {
            sig.ident.span()
        } else {
            sig.inputs.span()
        };
        return Err(syn::Error::new(span, PARAMS));
    }
    Ok(())
}

/// The handler's name without a raw-identifier prefix.
fn plain_name(ident: &Ident) -> String {
    let name = ident.to_string();
    name.strip_prefix("r#")
        .map_or_else(|| name.clone(), str::to_owned)
}

/// Expands `#[event(args)] item`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = parse_args(args)?;
    let item: ItemFn = syn::parse2(item)?;
    check(&item)?;
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        ..
    } = item;
    let name = sig.ident.clone();
    let path = plain_name(&name);
    let mut inner = sig.clone();
    inner.ident = Ident::new("__noctis_event", name.span());
    let output_span = match &sig.output {
        ReturnType::Default => sig.ident.span(),
        ReturnType::Type(_, ty) => ty.span(),
    };
    let (ordered, droppable) = (args.ordered, args.droppable);
    let new = quote_spanned! {output_span=>
        ::noctis::__private::core::EventHandler::__new
    };
    let inner = inner.into_token_stream();
    Ok(quote! {
        #(#attrs)*
        #vis fn #name() -> ::noctis::__private::core::EventHandler {
            #inner #block
            #new(
                ::core::concat!(::core::module_path!(), "::", #path),
                #ordered,
                #droppable,
                __noctis_event,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(args: TokenStream, item: TokenStream) -> String {
        expand(args, item).unwrap_err().to_string()
    }

    const FN: &str = "async fn h(ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> { Ok(()) }";

    fn item(src: &str) -> TokenStream {
        src.parse().unwrap()
    }

    #[test]
    fn expands() {
        let out = expand(
            quote!(ordered, droppable),
            item(&format!("/// docs\npub {FN}")),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains("pub fn h () -> :: noctis :: __private :: core :: EventHandler"),
            "{out}"
        );
        assert!(out.contains("async fn __noctis_event"), "{out}");
        assert!(
            out.contains("\"h\") , true , true , __noctis_event"),
            "{out}"
        );
        assert!(out.contains("doc"), "{out}");
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
        ] {
            let parsed = parse_args(args.clone()).unwrap();
            assert_eq!((parsed.ordered, parsed.droppable), expected, "{args}");
        }
    }

    /// Matrix 43: every rejected form names its fix.
    #[test]
    fn errors() {
        let cases: [(TokenStream, &str, &str); 14] = [
            (
                quote!(),
                "fn h(ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> { Ok(()) }",
                "`#[event]` needs an `async fn`",
            ),
            (
                quote!(),
                "async fn h() -> HandlerResult<()> { Ok(()) }",
                PARAMS,
            ),
            (
                quote!(),
                "async fn h(ctx: EventCtx) -> HandlerResult<()> { Ok(()) }",
                PARAMS,
            ),
            (
                quote!(),
                "async fn h(ctx: EventCtx, e: MessageCreate, x: u8) -> HandlerResult<()> { Ok(()) }",
                PARAMS,
            ),
            (
                quote!(),
                "async fn h(&self, e: MessageCreate) -> HandlerResult<()> { Ok(()) }",
                PARAMS,
            ),
            (
                quote!(),
                "async fn h<T>(ctx: EventCtx, e: T) -> HandlerResult<()> { Ok(()) }",
                "`#[event]` handlers cannot be generic",
            ),
            (
                quote!(),
                "async fn h(ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> where u8: Copy { Ok(()) }",
                "`#[event]` handlers cannot be generic",
            ),
            (
                quote!(),
                "const async fn h(ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> { Ok(()) }",
                PLAIN,
            ),
            (
                quote!(),
                "async unsafe fn h(ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> { Ok(()) }",
                PLAIN,
            ),
            (
                quote!(),
                "async extern \"C\" fn h(ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> { Ok(()) }",
                PLAIN,
            ),
            (
                quote!(x),
                FN,
                "unknown `#[event]` argument `x`: expected `ordered` or `droppable`",
            ),
            (
                quote!(ordered, ordered),
                FN,
                "duplicate `#[event]` argument `ordered`",
            ),
            (
                quote!(ordered = true),
                FN,
                "unknown `#[event]` argument `ordered = true`: expected `ordered` or `droppable`",
            ),
            (
                quote!(ordered, droppable, droppable),
                FN,
                "duplicate `#[event]` argument `droppable`",
            ),
        ];
        for (args, src, expected) in cases {
            assert_eq!(error(args, item(src)), expected, "{src}");
        }
    }

    #[test]
    fn raw_and_unicode_names() {
        let out = expand(
            quote!(),
            item("async fn r#type(c: EventCtx, e: UserUpdate) -> HandlerResult<()> { Ok(()) }"),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("\"type\""), "{out}");
        let out = expand(
            quote!(),
            item("async fn grüß(c: EventCtx, e: UserUpdate) -> HandlerResult<()> { Ok(()) }"),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("\"grüß\""), "{out}");
    }
}
