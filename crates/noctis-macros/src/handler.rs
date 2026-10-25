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
//!
//! Middleware Extensions: any parameter after `ctx` may be an `Ext<T>` or
//! `Option<Ext<T>>` (detected syntactically: the last path segment is `Ext`
//! with one type argument, or `Option` of that). The other parameters keep
//! their shape; the bound method becomes a `move` closure that extracts each
//! Extension from the context's frozen Extensions (core's `__ext` /
//! `__ext_opt`) and calls the method with every argument in written order,
//! and the descriptor requires the non-`Option` types
//! (`__require_extensions`), which `App::build` checks.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, GenericArgument, ImplItemFn, PathArguments, PathSegment, Type};

use crate::command::is_responder;

const NO_ARGS: &str =
    "`#[handler]` takes no arguments; put routing arguments on the kind attribute";
const BY_VALUE: &str = "a #[handler] method takes `self` by value";
const ASYNC: &str = "a #[handler] method must be `async`";
const GENERIC: &str = "a #[handler] method cannot be generic";
const PARAMS: &str = "a #[handler] method takes `(self, ctx[, path | target | event][, Ext<T> | Option<Ext<T>> …][, r: <Kind>Responder<..>])`";
const EXT_ARGS: &str = "`Ext` takes one type: `Ext<T>`";
const EXT_CTX: &str =
    "the first parameter after `self` is the context (`ctx: …Ctx`); put `Ext<T>` after it";

/// An `Ext<T>` (`required`) or `Option<Ext<T>>` parameter's `T`.
pub(crate) struct ExtParam {
    pub(crate) inner: Type,
    pub(crate) required: bool,
}

/// The last path segment of `ty` when it is named `name`.
fn last_named<'t>(ty: &'t Type, name: &str) -> Option<&'t PathSegment> {
    match ty {
        Type::Path(path) if path.qself.is_none() => {
            path.path.segments.last().filter(|s| s.ident == name)
        }
        _ => None,
    }
}

/// The `T` of `Ext<T>`; `None` for another type; an error for an `Ext`
/// with other generic arguments.
fn ext_inner(ty: &Type) -> syn::Result<Option<Type>> {
    let Some(segment) = last_named(ty, "Ext") else {
        return Ok(None);
    };
    if let PathArguments::AngleBracketed(args) = &segment.arguments
        && args.args.len() == 1
        && let Some(GenericArgument::Type(inner)) = args.args.first()
    {
        return Ok(Some(inner.clone()));
    }
    Err(syn::Error::new_spanned(ty, EXT_ARGS))
}

/// Classifies `ty` as an `Ext<T>` or `Option<Ext<T>>` parameter; `None` for
/// any other type.
pub(crate) fn ext_param(ty: &Type) -> syn::Result<Option<ExtParam>> {
    if let Some(segment) = last_named(ty, "Option") {
        if let PathArguments::AngleBracketed(args) = &segment.arguments
            && args.args.len() == 1
            && let Some(GenericArgument::Type(inner)) = args.args.first()
        {
            return Ok(ext_inner(inner)?.map(|inner| ExtParam {
                inner,
                required: false,
            }));
        }
        return Ok(None);
    }
    Ok(ext_inner(ty)?.map(|inner| ExtParam {
        inner,
        required: true,
    }))
}

/// Whether `ty` is (or looks like) an `Ext` parameter: what hooks and
/// autocomplete methods reject.
pub(crate) fn is_ext(ty: &Type) -> bool {
    !matches!(ext_param(ty), Ok(None))
}

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
    } else if ctx.contains("HookCtx") {
        "#[derive(Hook)] struct Online; impl Online { #[handler] async fn handle(self, ctx: HookCtx, event: AllShardsReady) -> HandlerResult<()> { .. } }"
    } else if ctx.contains("EventCtx") {
        "#[derive(EventHandler)] struct AwardXp; impl AwardXp { #[handler] async fn handle(self, ctx: EventCtx, event: MessageCreate) -> HandlerResult<()> { .. } }"
    } else if ctx.contains("CommandCtx") {
        "#[derive(SlashCommand)] #[command(\"ping\", description = \"…\")] struct Ping; impl Ping { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { .. } }"
    } else {
        "#[derive(Button)] #[button(\"shop.buy\", defaults(style = Primary, label = \"Buy\"))] struct Buy { item: u64 } impl Buy { #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> { .. } }"
    }
}

/// The shape type and the INT-05 assertion of the parameters `typed` (the
/// context first, the `Ext` parameters left out).
fn shape(item: &ImplItemFn, typed: &[&Type]) -> syn::Result<(TokenStream, TokenStream)> {
    let core = quote!(::noctis::__private::core);
    let no_responder = quote_spanned! {item.sig.ident.span()=>
        #core::__assert_no_responder::<Self>();
    };
    let responder = |r: &Type| {
        quote_spanned! {r.span()=>
            #core::__assert_responder::<Self, #r>();
        }
    };
    match typed {
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
    let params: Vec<(&Type, Option<ExtParam>)> = method
        .sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(&*t.ty),
            FnArg::Receiver(_) => None,
        })
        .map(|ty| ext_param(ty).map(|ext| (ty, ext)))
        .collect::<syn::Result<_>>()?;
    if let Some((ty, Some(_))) = params.first() {
        return Err(syn::Error::new_spanned(ty, EXT_CTX));
    }
    let plain: Vec<&Type> = params
        .iter()
        .filter(|(_, ext)| ext.is_none())
        .map(|(ty, _)| *ty)
        .collect();
    let (shape, assertion) = shape(&method, &plain)?;
    let name = method.sig.ident.clone();
    let core = quote!(::noctis::__private::core);
    let has_ext = params.iter().any(|(_, ext)| ext.is_some());
    let method_ref = if has_ext {
        ext_wrapper(&name, &params)
    } else {
        quote_spanned!(name.span()=> Self::#name)
    };
    let bind = quote_spanned! {name.span()=>
        <<Self as #core::__Kind>::Kind as #core::__Bind<#shape, Self, _>>::__bind(
            <Self as #core::__Kind>::NAME,
            #method_ref,
        )
    };
    let attached = quote! {
        #core::__AttachRateLimited::__attach_rate_limited(
            #core::__AttachHooks::__attach(
                #bind,
                Self::__noctis_on_stale(),
                Self::__noctis_on_expired(),
            ),
            Self::__noctis_on_rate_limited(),
        )
    };
    let required: Vec<&Type> = params
        .iter()
        .filter_map(|(_, ext)| ext.as_ref().filter(|e| e.required).map(|e| &e.inner))
        .collect();
    let descriptor = if required.is_empty() {
        attached
    } else {
        quote! {
            #core::__RequireExtensions::__require_extensions(
                #attached,
                const { &[#(#core::__ExtType::of::<#required>()),*] },
            )
        }
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
            #descriptor
        }
    })
}

/// The `move` closure bound in place of `Self::<name>` when the method
/// takes `Ext` parameters: it takes the other parameters with their written
/// types, extracts each Extension from the context and calls the method
/// with every argument in written order.
fn ext_wrapper(name: &syn::Ident, params: &[(&Type, Option<ExtParam>)]) -> TokenStream {
    let core = quote!(::noctis::__private::core);
    let ctx = quote::format_ident!("__noctis_ctx");
    let arg = |index: usize| quote::format_ident!("__noctis_a{}", index);
    let mut inputs = Vec::new();
    let mut extract = Vec::new();
    let mut args = vec![quote!(#ctx)];
    for (index, (ty, ext)) in params.iter().enumerate() {
        if index == 0 {
            inputs.push(quote!(#ctx: #ty));
            continue;
        }
        let ident = arg(index);
        match ext {
            None => inputs.push(quote!(#ident: #ty)),
            Some(ExtParam {
                inner,
                required: true,
            }) => extract.push(quote! {
                let #ident = match #core::__ext::<#inner, _>(&#ctx, <Self as #core::__Kind>::NAME) {
                    ::core::result::Result::Ok(e) => e,
                    ::core::result::Result::Err(e) => return ::core::result::Result::Err(e),
                };
            }),
            Some(ExtParam {
                inner,
                required: false,
            }) => extract.push(quote! {
                let #ident = #core::__ext_opt::<#inner, _>(&#ctx);
            }),
        }
        args.push(quote!(#ident));
    }
    quote! {
        move |__noctis_this: Self, #(#inputs),*| async move {
            #(#extract)*
            Self::#name(__noctis_this, #(#args),*).await
        }
    }
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

    /// Middleware Matrix 1-9 and 11: `Ext<T>` / `Option<Ext<T>>`
    /// parameters.
    #[test]
    #[expect(clippy::too_many_lines, reason = "one case per matrix row")]
    fn ext_parameters() {
        // Matrix 1: no Ext, no wrapper.
        let plain = run(
            quote!(),
            quote!(
                async fn handle(self, ctx: CommandCtx) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert!(!plain.contains("__ext") && !plain.contains("__require_extensions"));
        assert!(plain.contains(", Self :: handle ,"), "{plain}");
        // Matrix 2: one Ext.
        let out = run(
            quote!(),
            quote!(
                async fn handle(self, ctx: ComponentCtx, s: Ext<A>) -> HandlerResult<Responded> {
                    todo!()
                }
            ),
        );
        assert!(out.contains(":: __C , Self , _ >"), "{out}");
        assert!(
            out.contains("move | __noctis_this : Self , __noctis_ctx : ComponentCtx | async move"),
            "{out}"
        );
        assert!(
            out.contains("__ext :: < A , _ > (& __noctis_ctx , < Self as :: noctis :: __private :: core :: __Kind > :: NAME)"),
            "{out}"
        );
        assert!(
            out.contains("Self :: handle (__noctis_this , __noctis_ctx , __noctis_a1) . await"),
            "{out}"
        );
        assert!(
            out.contains("__require_extensions (") && out.contains("__ExtType :: of :: < A > ()"),
            "{out}"
        );
        // Matrix 3: optional only.
        let out = run(
            quote!(),
            quote!(
                async fn handle(self, ctx: ComponentCtx, s: Option<Ext<A>>) -> HandlerResult<()> {
                    todo!()
                }
            ),
        );
        assert!(out.contains(":: __C , Self , _ >"), "{out}");
        assert!(
            out.contains("__ext_opt :: < A , _ > (& __noctis_ctx)"),
            "{out}"
        );
        assert!(!out.contains("__require_extensions"), "{out}");
        // Matrix 4: mixed, order kept, requires only `A`.
        let out = run(
            quote!(),
            quote!(
                async fn handle(
                    self,
                    ctx: CommandCtx,
                    p: P,
                    a: Ext<A>,
                    b: Option<Ext<B>>,
                    r: CommandResponder,
                ) -> HandlerResult<()> {
                    todo!()
                }
            ),
        );
        assert!(out.contains("__CXR < P , CommandResponder >"), "{out}");
        assert!(
            out.contains(
                "__noctis_ctx : CommandCtx , __noctis_a1 : P , __noctis_a4 : CommandResponder"
            ),
            "{out}"
        );
        assert!(
            out.contains("Self :: handle (__noctis_this , __noctis_ctx , __noctis_a1 , __noctis_a2 , __noctis_a3 , __noctis_a4)"),
            "{out}"
        );
        assert!(
            out.contains(
                "const { & [:: noctis :: __private :: core :: __ExtType :: of :: < A > ()] }"
            ),
            "{out}"
        );
        assert!(
            out.contains("__assert_responder :: < Self , CommandResponder >"),
            "{out}"
        );
        // Matrix 5: Ext before the path.
        let out = run(
            quote!(),
            quote!(
                async fn handle(self, ctx: CommandCtx, a: Ext<A>, p: P) -> HandlerResult<()> {
                    todo!()
                }
            ),
        );
        assert!(out.contains("__CX < P >"), "{out}");
        assert!(
            out.contains(
                "Self :: handle (__noctis_this , __noctis_ctx , __noctis_a1 , __noctis_a2)"
            ),
            "{out}"
        );
        // Matrix 6: qualified paths.
        let out = run(
            quote!(),
            quote!(
                async fn handle(
                    self,
                    ctx: CommandCtx,
                    a: noctis::Ext<A>,
                    b: core::option::Option<noctis::Ext<B>>,
                ) -> HandlerResult<()> {
                    todo!()
                }
            ),
        );
        assert!(out.contains(":: __C , Self , _ >"), "{out}");
        assert!(
            out.contains("__ext :: < A , _ >") && out.contains("__ext_opt :: < B , _ >"),
            "{out}"
        );
        // Matrix 11: an event.
        let out = run(
            quote!(),
            quote!(
                async fn handle(
                    self,
                    ctx: EventCtx,
                    ev: MessageCreate,
                    a: Ext<A>,
                ) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert!(out.contains("__CX < MessageCreate >"), "{out}");
        assert!(out.contains("__ExtType :: of :: < A > ()"), "{out}");
        // Matrix 7: bad generics.
        for ty in [
            quote!(Ext),
            quote!(Ext<A, B>),
            quote!(Ext<'a, A>),
            quote!(Option<Ext>),
        ] {
            let out = run(
                quote!(),
                quote!(
                    async fn handle(self, ctx: CommandCtx, a: #ty) -> HandlerResult<()> {
                        Ok(())
                    }
                ),
            );
            assert_eq!(out, EXT_ARGS, "{ty}");
        }
        // Matrix 8: an Ext as the context.
        let out = run(
            quote!(),
            quote!(
                async fn handle(self, a: Ext<A>) -> HandlerResult<()> {
                    Ok(())
                }
            ),
        );
        assert_eq!(out, EXT_CTX);
        // Matrix 9: too many non-Ext parameters.
        let out = run(
            quote!(),
            quote!(
                async fn handle(self, ctx: C, x: X, y: Y, r: CommandResponder, z: Z) {}
            ),
        );
        assert_eq!(out, PARAMS);
        assert!(PARAMS.contains("Ext<T> | Option<Ext<T>>"));
    }
}
