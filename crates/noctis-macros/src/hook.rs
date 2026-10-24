//! `#[on_stale]` and `#[on_expired]`: the stale-id and expired-id methods
//! of a component or modal struct (AD-23 stage 2, FR-18, FR-60; Story 7.3).
//!
//! The method stays as written, with no `self` (the Payload did not
//! decode): `#[on_stale] async fn <any>(ctx[, r: <Kind>Responder<..>])` and
//! `#[on_expired] async fn <any>(ctx, expired: Expired[, r:
//! <Kind>Responder<..>])`. Beside it, in the same `impl`, the macro emits
//! the hidden `__noctis_on_stale()` / `__noctis_on_expired()` binding. These
//! inherent items shadow core's blanket `__NoHooks` fallback, so the
//! `descriptor()` that `#[handler]` emits attaches exactly the hooks the
//! struct has. The parameters are classified syntactically into a hook
//! shape carrying the context type (core's `__H`, `__HR`, `__HE` and
//! `__HER`), so a wrong context, `Expired` or responder type fails core's
//! `__BindStale`/`__BindExpired` bound with the expected shapes; a struct
//! of another kind fails `__HookKind`. The INT-05 assertions are those of
//! `#[handler]`: a hook inherits its Route's `auto_defer`.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ImplItemFn, Type};

use crate::command::is_responder;

const NO_EXT: &str =
    "`#[on_stale]`/`#[on_expired]` hooks run no Middleware, so they take no `Ext<T>`";

/// Which hook attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hook {
    /// `#[on_stale]`.
    Stale,
    /// `#[on_expired]`.
    Expired,
    /// `#[on_rate_limited]` (INT-14).
    RateLimited,
}

impl Hook {
    /// The attribute's name.
    const fn name(self) -> &'static str {
        match self {
            Self::Stale => "on_stale",
            Self::Expired => "on_expired",
            Self::RateLimited => "on_rate_limited",
        }
    }

    fn no_self(self) -> String {
        if self == Self::RateLimited {
            return "an `#[on_rate_limited]` method takes no `self`: the handler did not run"
                .to_owned();
        }
        format!(
            "an `#[{}]` method takes no `self`: the Payload did not decode",
            self.name()
        )
    }

    fn not_async(self) -> String {
        format!("an `#[{}]` method must be `async`", self.name())
    }

    fn generic(self) -> String {
        format!("an `#[{}]` method cannot be generic", self.name())
    }

    fn no_args(self) -> String {
        format!("`#[{}]` takes no arguments", self.name())
    }

    fn params(self) -> &'static str {
        match self {
            Self::Stale => "an `#[on_stale]` method takes `(ctx[, r: <Kind>Responder<..>])`",
            Self::Expired => {
                "an `#[on_expired]` method takes `(ctx, expired: Expired[, r: <Kind>Responder<..>])`"
            }
            Self::RateLimited => {
                "an `#[on_rate_limited]` method takes `(ctx: RateLimitedCtx, hit: RateLimitHit)`"
            }
        }
    }
}

/// The hook shape and the INT-05 assertion of the method's parameters.
fn shape(method: &ImplItemFn, hook: Hook) -> syn::Result<(TokenStream, TokenStream)> {
    let core = quote!(::noctis::__private::core);
    let sig = &method.sig;
    let typed: Vec<&Type> = sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(&*t.ty),
            FnArg::Receiver(_) => None,
        })
        .collect();
    let no_responder = quote_spanned! {sig.ident.span()=>
        #core::__assert_no_responder::<Self>();
    };
    let responder = |r: &Type| {
        quote_spanned! {r.span()=>
            #core::__assert_responder::<Self, #r>();
        }
    };
    if let Some(ext) = typed.iter().find(|ty| crate::handler::is_ext(ty)) {
        return Err(syn::Error::new_spanned(ext, NO_EXT));
    }
    let count = || {
        let span = if typed.is_empty() {
            sig.ident.span()
        } else {
            sig.inputs.span()
        };
        syn::Error::new(span, hook.params())
    };
    match (hook, typed.as_slice()) {
        (Hook::Stale, [c]) => Ok((quote!(#core::__H<#c>), no_responder)),
        (Hook::Stale, [c, r]) if is_responder(r) => Ok((quote!(#core::__HR<#c, #r>), responder(r))),
        // A second parameter of another type is a type error listing the
        // hook shapes.
        (Hook::Stale, [c, x]) => Ok((quote!(#core::__HR<#c, #x>), no_responder)),
        (Hook::Expired, [c, e]) if !is_responder(e) => {
            Ok((quote!(#core::__HE<#c, #e>), no_responder))
        }
        (Hook::Expired, [c, e, r]) if !is_responder(e) && is_responder(r) => {
            Ok((quote!(#core::__HER<#c, #e, #r>), responder(r)))
        }
        (Hook::Expired, [c, e, r]) if !is_responder(e) => {
            Ok((quote!(#core::__HER<#c, #e, #r>), no_responder))
        }
        _ => Err(count()),
    }
}

/// The `#[on_rate_limited]` method and its hidden binding: the method's
/// parameters and return type are checked by `__RateLimitedHook::__new`'s
/// bounds, and a struct without rate limits (an event handler) by
/// `__AttachRateLimited` in `descriptor()`.
fn rate_limited(mut method: ImplItemFn) -> TokenStream {
    let name = method.sig.ident.clone();
    let core = quote!(::noctis::__private::core);
    let binding = Ident::new("__noctis_on_rate_limited", name.span());
    let item = quote_spanned! {name.span()=>
        #[doc(hidden)]
        pub fn #binding() -> #core::__RateLimitedHook {
            #core::__RateLimitedHook::__new(Self::#name)
        }
    };
    method
        .attrs
        .push(syn::parse_quote!(#[allow(clippy::unused_async)]));
    quote! {
        #method

        #item
    }
}

/// Checks the method's receiver, `async` and generics.
fn check(method: &ImplItemFn, hook: Hook) -> syn::Result<()> {
    let sig = &method.sig;
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new_spanned(receiver, hook.no_self()));
    }
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(sig.fn_token, hook.not_async()));
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(&sig.generics, hook.generic()));
    }
    Ok(())
}

/// Expands `#[on_stale] item` or `#[on_expired] item`. On an error the
/// method stays, with the error; without a binding the struct falls back
/// to "no hook", so no second error follows.
pub(crate) fn expand_or_fallback(hook: Hook, args: TokenStream, item: TokenStream) -> TokenStream {
    match expand(hook, args, item.clone()) {
        Ok(tokens) => tokens,
        Err(e) => {
            let error = e.into_compile_error();
            match syn::parse2::<ImplItemFn>(item) {
                Ok(method) => quote!(#error #method),
                Err(_) => error,
            }
        }
    }
}

/// Expands `#[on_stale] item` or `#[on_expired] item`.
pub(crate) fn expand(hook: Hook, args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(args, hook.no_args()));
    }
    let mut method: ImplItemFn = syn::parse2(item)?;
    check(&method, hook)?;
    if hook == Hook::RateLimited {
        return Ok(rate_limited(method));
    }
    let (shape, assertion) = shape(&method, hook)?;
    let name = method.sig.ident.clone();
    let core = quote!(::noctis::__private::core);
    let (binding, hook_ty, bind_trait) = match hook {
        Hook::Stale => (
            Ident::new("__noctis_on_stale", name.span()),
            quote!(#core::__StaleHook),
            quote!(#core::__BindStale),
        ),
        Hook::Expired | Hook::RateLimited => (
            Ident::new("__noctis_on_expired", name.span()),
            quote!(#core::__ExpiredHook),
            quote!(#core::__BindExpired),
        ),
    };
    let item = quote_spanned! {name.span()=>
        #[doc(hidden)]
        pub fn #binding() -> #hook_ty<<<Self as #core::__Kind>::Kind as #core::__HookKind>::Ctx> {
            #assertion
            <<Self as #core::__Kind>::Kind as #bind_trait<#shape, Self, _>>::__bind(Self::#name)
        }
    };
    // A hook needs no `.await` to be `async` (the binding requires it).
    method
        .attrs
        .push(syn::parse_quote!(#[allow(clippy::unused_async)]));
    Ok(quote! {
        #method

        #item
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(hook: Hook, args: TokenStream, src: &str) -> String {
        match expand(hook, args, src.parse().unwrap()) {
            Ok(t) => t.to_string(),
            Err(e) => e.to_string(),
        }
    }

    /// Story 7.3 Matrix 38: every allowed shape.
    #[test]
    fn shapes() {
        let s = Hook::Stale;
        let e = Hook::Expired;
        let out = run(
            s,
            quote!(),
            "async fn f(ctx: ComponentCtx) -> HandlerResult<V2Message> { todo!() }",
        );
        assert!(out.contains("pub fn __noctis_on_stale ()"), "{out}");
        assert!(out.contains("__H < ComponentCtx >"), "{out}");
        assert!(out.contains("__BindStale <"), "{out}");
        assert!(out.contains("__assert_no_responder :: < Self >"), "{out}");
        assert!(out.contains("Self :: f"), "{out}");
        assert!(out.contains("# [allow (clippy :: unused_async)]"), "{out}");
        let out = run(
            s,
            quote!(),
            "async fn f(ctx: ModalSubmitCtx, r: ModalResponder) -> HandlerResult<Responded> { todo!() }",
        );
        assert!(
            out.contains("__HR < ModalSubmitCtx , ModalResponder >"),
            "{out}"
        );
        assert!(
            out.contains("__assert_responder :: < Self , ModalResponder >"),
            "{out}"
        );
        let out = run(
            e,
            quote!(),
            "async fn f(ctx: ComponentCtx, why: Expired) -> HandlerResult<V2Message> { todo!() }",
        );
        assert!(out.contains("pub fn __noctis_on_expired ()"), "{out}");
        assert!(out.contains("__HE < ComponentCtx , Expired >"), "{out}");
        assert!(out.contains("__BindExpired <"), "{out}");
        assert!(out.contains("__ExpiredHook <"), "{out}");
        let out = run(
            e,
            quote!(),
            "async fn f(ctx: ComponentCtx, why: Expired, r: ComponentResponder<Manual>) -> HandlerResult<Responded> { todo!() }",
        );
        assert!(
            out.contains("__HER < ComponentCtx , Expired , ComponentResponder < Manual > >"),
            "{out}"
        );
        assert!(out.contains("__assert_responder"), "{out}");
        // Wrong types are left to the type check.
        let out = run(s, quote!(), "async fn f(ctx: ComponentCtx, x: u8) {}");
        assert!(out.contains("__HR < ComponentCtx , u8 >"), "{out}");
        let out = run(e, quote!(), "async fn f(ctx: CommandCtx, x: u8, y: u8) {}");
        assert!(out.contains("__HER < CommandCtx , u8 , u8 >"), "{out}");
    }

    /// Story 7.3 Matrix 39.
    #[test]
    fn errors() {
        let s = Hook::Stale;
        let e = Hook::Expired;
        assert_eq!(
            run(s, quote!(), "async fn f(self, ctx: ComponentCtx) {}"),
            "an `#[on_stale]` method takes no `self`: the Payload did not decode"
        );
        assert_eq!(
            run(
                e,
                quote!(),
                "async fn f(&self, ctx: ComponentCtx, x: Expired) {}"
            ),
            "an `#[on_expired]` method takes no `self`: the Payload did not decode"
        );
        assert_eq!(
            run(s, quote!(), "fn f(ctx: ComponentCtx) {}"),
            "an `#[on_stale]` method must be `async`"
        );
        assert_eq!(
            run(e, quote!(), "fn f(ctx: ComponentCtx, x: Expired) {}"),
            "an `#[on_expired]` method must be `async`"
        );
        assert_eq!(
            run(s, quote!(), "async fn f<T>(ctx: T) {}"),
            "an `#[on_stale]` method cannot be generic"
        );
        assert_eq!(
            run(
                e,
                quote!(),
                "async fn f(ctx: C, x: Expired) where C: Copy {}"
            ),
            "an `#[on_expired]` method cannot be generic"
        );
        assert_eq!(
            run(s, quote!(x), "async fn f(ctx: ComponentCtx) {}"),
            "`#[on_stale]` takes no arguments"
        );
        assert_eq!(
            run(
                e,
                quote!(auto_defer = false),
                "async fn f(ctx: ComponentCtx, x: Expired) {}"
            ),
            "`#[on_expired]` takes no arguments"
        );
        for src in [
            "async fn f() {}",
            "async fn f(ctx: ComponentCtx, x: Expired, r: ComponentResponder) {}",
        ] {
            assert_eq!(run(s, quote!(), src), s.params(), "{src}");
        }
        for src in [
            "async fn f() {}",
            "async fn f(ctx: ComponentCtx) {}",
            "async fn f(ctx: ComponentCtx, r: ComponentResponder) {}",
            "async fn f(ctx: ComponentCtx, r: ComponentResponder, x: Expired) {}",
            "async fn f(ctx: ComponentCtx, x: Expired, r: ComponentResponder, y: u8) {}",
        ] {
            assert_eq!(run(e, quote!(), src), e.params(), "{src}");
        }
        assert_eq!(
            s.params(),
            "an `#[on_stale]` method takes `(ctx[, r: <Kind>Responder<..>])`"
        );
        assert_eq!(
            e.params(),
            "an `#[on_expired]` method takes `(ctx, expired: Expired[, r: <Kind>Responder<..>])`"
        );
        // The fallback keeps the method beside the error.
        let out = expand_or_fallback(
            s,
            quote!(),
            quote!(
                fn f(ctx: ComponentCtx) {}
            ),
        )
        .to_string();
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("fn f (ctx : ComponentCtx)"), "{out}");
        assert!(!out.contains("__noctis_on_stale"), "{out}");
    }

    /// Story 13.4 G17: the `#[on_rate_limited]` binding and its errors.
    #[test]
    fn int_14_on_rate_limited() {
        let r = Hook::RateLimited;
        let out = run(
            r,
            quote!(),
            "async fn slow(ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> { todo!() }",
        );
        assert!(out.contains("pub fn __noctis_on_rate_limited ()"), "{out}");
        assert!(
            out.contains("__RateLimitedHook :: __new (Self :: slow)"),
            "{out}"
        );
        assert_eq!(
            run(
                r,
                quote!(),
                "async fn f(self, ctx: RateLimitedCtx, hit: RateLimitHit) {}"
            ),
            "an `#[on_rate_limited]` method takes no `self`: the handler did not run"
        );
        assert_eq!(
            run(
                r,
                quote!(),
                "fn f(ctx: RateLimitedCtx, hit: RateLimitHit) {}"
            ),
            "an `#[on_rate_limited]` method must be `async`"
        );
        assert_eq!(
            run(r, quote!(), "async fn f<T>(ctx: T, hit: RateLimitHit) {}"),
            "an `#[on_rate_limited]` method cannot be generic"
        );
        assert_eq!(
            run(
                r,
                quote!(x),
                "async fn f(ctx: RateLimitedCtx, hit: RateLimitHit) {}"
            ),
            "`#[on_rate_limited]` takes no arguments"
        );
        assert_eq!(
            r.params(),
            "an `#[on_rate_limited]` method takes `(ctx: RateLimitedCtx, hit: RateLimitHit)`"
        );
    }

    /// Middleware Matrix 10: hooks take no `Ext<T>`.
    #[test]
    fn hooks_take_no_ext() {
        for (hook, src) in [
            (Hook::Stale, "async fn f(ctx: ComponentCtx, a: Ext<A>) {}"),
            (
                Hook::Expired,
                "async fn f(ctx: ComponentCtx, e: Expired, a: Option<Ext<A>>) {}",
            ),
            (
                Hook::Expired,
                "async fn f(ctx: ComponentCtx, e: Expired, a: noctis::Ext<A, B>) {}",
            ),
        ] {
            assert_eq!(run(hook, quote!(), src), NO_EXT, "{src}");
        }
    }
}
