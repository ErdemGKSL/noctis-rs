//! `#[handler]`: marks the one `async` method of a kind-derived struct that
//! handles its interaction (AD-6, AD-26, FR-58).
//!
//! The method stays as written. Beside it, in the same `impl`, the macro
//! emits the hidden `__NOCTIS_HAS_HANDLER` const (the derive's
//! missing-handler check reads it) and the public `fn descriptor()`, which
//! binds the method through core's hidden `__Bind` trait of the struct's
//! kind. The parameters after `self` are classified syntactically: `ctx`,
//! then an optional path, target or event payload, then an optional
//! responder (a type whose last segment is `CommandResponder`,
//! `ComponentResponder`, `ModalResponder` or `Responder`). The INT-05 check
//! compares the derive's declared defer mode with that responder. The
//! descriptor attaches the struct's `#[on_stale]`/`#[on_expired]` methods,
//! Story 7.3: their inherent `__noctis_on_*()` items shadow core's
//! `__NoHooks` fallback, which answers "no hook".

use proc_macro2::TokenStream;
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, ImplItemFn, Type};

use crate::command::is_responder;

const NO_ARGS: &str =
    "`#[handler]` takes no arguments; put routing arguments on the kind attribute";
const BY_VALUE: &str = "a #[handler] method takes `self` by value";
const ASYNC: &str = "a #[handler] method must be `async`";
const GENERIC: &str = "a #[handler] method cannot be generic";
const PARAMS: &str =
    "a #[handler] method takes `(self, ctx[, path | target | event][, r: <Kind>Responder<..>])`";

/// The struct form the error of a free function shows, chosen by its
/// context parameter.
fn struct_form(item: &ImplItemFn) -> &'static str {
    let ctx = item
        .sig
        .inputs
        .iter()
        .find_map(|a| match a {
            FnArg::Typed(t) => Some(t.ty.to_token_stream().to_string()),
            FnArg::Receiver(_) => None,
        })
        .unwrap_or_default();
    if ctx.contains("ModalSubmitCtx") {
        "#[derive(Modal)] #[modal(\"bio\", defaults(title = \"Bio\"))] struct Bio { #[text_input(\"Bio\")] bio: String } impl Bio { #[handler] async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> { .. } }"
    } else if ctx.contains("EventCtx") {
        "#[derive(EventHandler)] struct AwardXp; impl AwardXp { #[handler] async fn handle(self, ctx: EventCtx, event: MessageCreate) -> HandlerResult<()> { .. } }"
    } else if ctx.contains("CommandCtx") {
        "#[derive(SlashCommand)] #[command(\"ping\", description = \"…\")] struct Ping; impl Ping { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { .. } }"
    } else {
        "#[derive(Button)] #[button(\"shop.buy\", defaults(style = Primary, label = \"Buy\"))] struct Buy { item: u64 } impl Buy { #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> { .. } }"
    }
}

/// The shape type and the INT-05 assertion of the parameters after `ctx`.
fn shape(item: &ImplItemFn) -> syn::Result<(TokenStream, TokenStream)> {
    let core = quote!(::noctis::__private::core);
    let typed: Vec<&Type> = item
        .sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(&*t.ty),
            FnArg::Receiver(_) => None,
        })
        .collect();
    let no_responder = quote_spanned! {item.sig.ident.span()=>
        #core::__assert_no_responder::<Self>();
    };
    let responder = |r: &Type| {
        quote_spanned! {r.span()=>
            #core::__assert_responder::<Self, #r>();
        }
    };
    match typed.as_slice() {
        [_] => Ok((quote!(#core::__C), no_responder)),
        [_, r] if is_responder(r) => Ok((quote!(#core::__CR<#r>), responder(r))),
        [_, x] => Ok((quote!(#core::__CX<#x>), no_responder)),
        [_, x, r] if is_responder(r) && !is_responder(x) => {
            Ok((quote!(#core::__CXR<#x, #r>), responder(r)))
        }
        // A second parameter of any other type is a type error naming the
        // kind's expected parameters.
        [_, x, r] => Ok((quote!(#core::__CXR<#x, #r>), no_responder)),
        _ => Err(syn::Error::new(item.sig.inputs.span(), PARAMS)),
    }
}

/// Expands `#[handler] item`. On an error in a method taking `self`, the
/// method stays, with the error and placeholder items, so the derive's
/// missing-handler check adds no second error.
pub(crate) fn expand_or_fallback(args: TokenStream, item: TokenStream) -> TokenStream {
    match expand(args, item.clone()) {
        Ok(tokens) => tokens,
        Err(e) => {
            let error = e.into_compile_error();
            match syn::parse2::<ImplItemFn>(item) {
                Ok(method) if method.sig.receiver().is_some() => {
                    let core = quote!(::noctis::__private::core);
                    quote! {
                        #error
                        #method

                        #[doc(hidden)]
                        pub const __NOCTIS_HAS_HANDLER: ::core::primitive::bool = true;

                        #[doc(hidden)]
                        pub fn descriptor() -> <Self as #core::__Kind>::Descriptor {
                            ::core::unreachable!("the #[handler] method failed to compile")
                        }
                    }
                }
                Ok(method) => quote!(#error #method),
                Err(_) => error,
            }
        }
    }
}

/// Expands `#[handler] item`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(args, NO_ARGS));
    }
    let mut method: ImplItemFn = syn::parse2(item)?;
    let sig = &method.sig;
    match sig.receiver() {
        None => {
            let help = struct_form(&method);
            return Err(syn::Error::new(
                sig.ident.span(),
                format!(
                    "`#[handler]` marks a method of a kind-derived struct, and {BY_VALUE}; free-function handlers are withdrawn\n  = help: {help}"
                ),
            ));
        }
        Some(r) if !matches!(r.kind, syn::ReceiverKind::Value) => {
            return Err(syn::Error::new_spanned(r, BY_VALUE));
        }
        Some(_) => {}
    }
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(sig.fn_token, ASYNC));
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(&sig.generics, GENERIC));
    }
    let (shape, assertion) = shape(&method)?;
    let name = method.sig.ident.clone();
    let core = quote!(::noctis::__private::core);
    let bind = quote_spanned! {name.span()=>
        <<Self as #core::__Kind>::Kind as #core::__Bind<#shape, Self, _>>::__bind(
            <Self as #core::__Kind>::NAME,
            Self::#name,
        )
    };
    // A handler needs no `.await` to be `async` (the kind requires it).
    method
        .attrs
        .push(syn::parse_quote!(#[allow(clippy::unused_async)]));
    Ok(quote! {
        #method

        #[doc(hidden)]
        pub const __NOCTIS_HAS_HANDLER: ::core::primitive::bool = true;

        /// The descriptor `Module::add::<Self>()` registers: this struct's
        /// `#[handler]` method with its kind declaration.
        #[must_use]
        #[allow(unused_imports)]
        pub fn descriptor() -> <Self as #core::__Kind>::Descriptor {
            use #core::__NoHooks as _;
            #assertion
            #core::__AttachHooks::__attach(
                #bind,
                Self::__noctis_on_stale(),
                Self::__noctis_on_expired(),
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: TokenStream, item: TokenStream) -> String {
        match expand(args, item) {
            Ok(t) => t.to_string(),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn shapes() {
        let out = run(
            quote!(),
            quote!(
                async fn handle(self, ctx: CommandCtx) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert!(out.contains(":: __C ,"), "{out}");
        // Story 7.3: the descriptor attaches the struct's hooks.
        assert!(out.contains("__NoHooks as _"), "{out}");
        assert!(
            out.contains("__AttachHooks :: __attach (")
                && out.contains("Self :: __noctis_on_stale ()")
                && out.contains("Self :: __noctis_on_expired ()"),
            "{out}"
        );
        assert!(out.contains("__assert_no_responder"), "{out}");
        assert!(out.contains("__NOCTIS_HAS_HANDLER"), "{out}");
        let out = run(
            quote!(),
            quote!(
                async fn h(
                    mut self,
                    ctx: CommandCtx,
                    path: ConfigPath,
                    r: CommandResponder<Manual>,
                ) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert!(
            out.contains("__CXR < ConfigPath , CommandResponder < Manual >"),
            "{out}"
        );
        assert!(
            out.contains("__assert_responder :: < Self , CommandResponder < Manual >"),
            "{out}"
        );
        let out = run(
            quote!(),
            quote!(
                async fn h(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert!(out.contains("__CR < CommandResponder >"), "{out}");
        for responder in [
            quote!(ComponentResponder),
            quote!(ComponentResponder<Manual>),
            quote!(ModalResponder),
        ] {
            let out = run(
                quote!(),
                quote!(
                    async fn h(self, ctx: ComponentCtx, r: #responder) -> HandlerResult<Responded> {
                        Ok(r.done())
                    }
                ),
            );
            assert!(out.contains("__CR <"), "{out}");
            assert!(out.contains("__assert_responder"), "{out}");
        }
        let out = run(
            quote!(),
            quote!(
                async fn h(self, ctx: EventCtx, e: MessageCreate) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert!(out.contains("__CX < MessageCreate >"), "{out}");
    }

    /// Story 6.11 Matrix 8 and 53.
    #[test]
    fn shape_errors() {
        let ok = quote!(
            async fn h(self, ctx: ComponentCtx) {}
        );
        assert_eq!(run(quote!(x), ok.clone()), NO_ARGS);
        assert_eq!(
            run(
                quote!(),
                quote!(
                    async fn h(&self, ctx: C) {}
                )
            ),
            BY_VALUE
        );
        assert_eq!(
            run(
                quote!(),
                quote!(
                    async fn h(self: Box<Self>, ctx: C) {}
                )
            ),
            BY_VALUE
        );
        assert_eq!(
            run(
                quote!(),
                quote!(
                    fn h(self, ctx: C) {}
                )
            ),
            ASYNC
        );
        assert_eq!(
            run(
                quote!(),
                quote!(
                    async fn h<T>(self, ctx: T) {}
                )
            ),
            GENERIC
        );
        assert_eq!(
            run(
                quote!(),
                quote!(
                    async fn h(self) {}
                )
            ),
            PARAMS
        );
        let free = run(
            quote!(),
            quote!(
                async fn h(ctx: ComponentCtx) {}
            ),
        );
        assert!(free.contains("= help: #[derive(Button)]"), "{free}");
        let free = run(
            quote!(),
            quote!(
                async fn h(ctx: CommandCtx) {}
            ),
        );
        assert!(free.contains("#[derive(SlashCommand)]"), "{free}");
        let free = run(
            quote!(),
            quote!(
                async fn h(ctx: EventCtx, e: E) {}
            ),
        );
        assert!(free.contains("#[derive(EventHandler)]"), "{free}");
        let free = run(
            quote!(),
            quote!(
                async fn h(ctx: ModalSubmitCtx) {}
            ),
        );
        assert!(free.contains("#[derive(Modal)]"), "{free}");
        assert!(run(quote!(), ok).contains("descriptor"));
    }
}
