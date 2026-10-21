//! The Route Guards of a component or modal derive (FR-19, AD-23 stage 8):
//! the `owner` field check and the `__Guards` impl every kind derive emits.

use noctis_rules::RuleId;
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Ident, LitStr};

use crate::common::{error, plain};
use crate::route::GuardArgs;

/// The context a Route's Guards take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardCtx {
    /// `ComponentCtx` (a button or a select).
    Component,
    /// `ModalSubmitCtx` (a modal).
    Modal,
}

impl GuardCtx {
    fn ty(self) -> TokenStream {
        match self {
            Self::Component => quote!(::noctis::__private::core::ComponentCtx),
            Self::Modal => quote!(::noctis::__private::core::ModalSubmitCtx),
        }
    }
}

/// Checks that `owner = x` names a field of `ident`: one of `fields` (in
/// declaration order) and, on a modal, not one of its `inputs` (CID-03).
pub(crate) fn check_owner(
    guards: &GuardArgs,
    ident: &Ident,
    fields: &[&Ident],
    inputs: &[&Ident],
) -> syn::Result<()> {
    let Some(owner) = &guards.owner else {
        return Ok(());
    };
    let name = plain(owner);
    if inputs.iter().any(|i| plain(i) == name) {
        return Err(error(
            owner,
            RuleId::CID_03,
            format!("`owner` must name a Payload field; `{name}` is a modal input"),
            "name a field without an input attribute, of type `Id<UserMarker>`: submitted text is client input",
        ));
    }
    if fields.iter().any(|f| plain(f) == name) {
        return Ok(());
    }
    let listed = if fields.is_empty() {
        "it has no fields".to_owned()
    } else {
        let names: Vec<String> = fields.iter().map(|f| plain(f)).collect();
        format!("its fields are: {}", names.join(", "))
    };
    Err(error(
        owner,
        RuleId::CID_03,
        format!(
            "`owner = {name}` names no field of `{}`; {listed}",
            plain(ident)
        ),
        "name the Payload field of type `Id<UserMarker>` whose user alone may use this Route",
    ))
}

/// The `__Guards<C>` impl of `ident`: empty without a Guard; else
/// `GUARDED = true` and a `__guard` running owner, permissions, then the
/// custom Guard, the first denial winning.
pub(crate) fn guards_impl(ident: &Ident, guards: &GuardArgs, ctx: GuardCtx) -> TokenStream {
    let core = quote!(::noctis::__private::core);
    let ctx_ty = ctx.ty();
    if !guards.any() {
        return quote! {
            #[automatically_derived]
            impl #core::__Guards<#ctx_ty> for #ident {}
        };
    }
    let ok = quote!(::core::result::Result::Ok);
    let some = quote!(::core::option::Option::Some);
    let owner = guards.owner.as_ref().map(|field| {
        let id = quote_spanned!(field.span()=> #core::__owner_id(&self.#field));
        quote! {
            if let #some(denial) = #core::__check_owner(#ctx_ty::user_id(ctx), #id) {
                return #ok(#some(denial));
            }
        }
    });
    let permissions = guards.permissions.map(|bits| {
        quote! {
            if let #some(denial) = #core::__check_permissions(
                #ctx_ty::member_permissions(ctx),
                #core::Permissions::from_bits_retain(#bits),
            ) {
                return #ok(#some(denial));
            }
        }
    });
    let last = guards.guard.as_ref().map_or_else(
        || quote!(#ok(::core::option::Option::None)),
        |path| {
            let text = quote!(#path).to_string().replace(' ', "");
            let text = LitStr::new(&text, proc_macro2::Span::call_site());
            quote_spanned! {path_span(path)=>
                #core::__check_custom(#text, #path(ctx, self).await)
            }
        },
    );
    quote! {
        #[automatically_derived]
        impl #core::__Guards<#ctx_ty> for #ident {
            const GUARDED: ::core::primitive::bool = true;

            fn __guard<'a>(
                &'a self,
                ctx: &'a #ctx_ty,
            ) -> impl ::core::future::Future<
                Output = #core::HandlerResult<::core::option::Option<#core::__Denial>>,
            > + ::core::marker::Send + 'a {
                async move {
                    #owner
                    #permissions
                    #last
                }
            }
        }
    }
}

/// The span of a Guard path: its last segment (where a wrong signature is
/// reported).
fn path_span(path: &syn::Path) -> proc_macro2::Span {
    path.segments
        .last()
        .map_or_else(proc_macro2::Span::call_site, |s| s.ident.span())
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    fn ident(name: &str) -> Ident {
        Ident::new(name, proc_macro2::Span::call_site())
    }

    #[test]
    fn owner_field_checks() {
        let s = ident("S");
        let (user, page, bio) = (ident("user"), ident("page"), ident("bio"));
        let guards = |owner: &str| GuardArgs {
            owner: Some(ident(owner)),
            ..GuardArgs::default()
        };
        assert!(check_owner(&guards("user"), &s, &[&user, &page], &[]).is_ok());
        assert!(check_owner(&GuardArgs::default(), &s, &[], &[]).is_ok());
        assert_eq!(
            check_owner(&guards("usr"), &s, &[&user, &page], &[])
                .unwrap_err()
                .to_string(),
            "[CID-03] `owner = usr` names no field of `S`; its fields are: user, page\n  = help: name the Payload field of type `Id<UserMarker>` whose user alone may use this Route"
        );
        assert!(
            check_owner(&guards("usr"), &s, &[], &[])
                .unwrap_err()
                .to_string()
                .starts_with("[CID-03] `owner = usr` names no field of `S`; it has no fields\n")
        );
        assert!(
            check_owner(&guards("bio"), &s, &[&user, &bio], &[&bio])
                .unwrap_err()
                .to_string()
                .starts_with(
                    "[CID-03] `owner` must name a Payload field; `bio` is a modal input\n  = help: "
                )
        );
    }

    /// Matrix 15: no Guard, an empty impl.
    #[test]
    fn unguarded_impl_is_empty() {
        let out = guards_impl(&ident("S"), &GuardArgs::default(), GuardCtx::Component).to_string();
        assert_eq!(
            out,
            "# [automatically_derived] impl :: noctis :: __private :: core :: __Guards < :: noctis :: __private :: core :: ComponentCtx > for S { }"
        );
        let modal = guards_impl(&ident("S"), &GuardArgs::default(), GuardCtx::Modal).to_string();
        assert!(modal.contains("ModalSubmitCtx > for S { }"), "{modal}");
    }

    #[test]
    fn guarded_impl_runs_in_order() {
        let guards = GuardArgs {
            owner: Some(ident("user")),
            permissions: Some(32),
            guard: Some(parse_quote!(auth::is_staff)),
        };
        let out = guards_impl(&ident("Ban"), &guards, GuardCtx::Component).to_string();
        assert!(
            out.contains("const GUARDED : :: core :: primitive :: bool = true"),
            "{out}"
        );
        let owner = out.find("__check_owner").unwrap();
        let perms = out.find("__check_permissions").unwrap();
        let custom = out.find("__check_custom").unwrap();
        assert!(owner < perms && perms < custom, "{out}");
        assert!(out.contains("__owner_id (& self . user)"), "{out}");
        assert!(out.contains("from_bits_retain (32u64)"), "{out}");
        assert!(
            out.contains(
                "__check_custom (\"auth::is_staff\" , auth :: is_staff (ctx , self) . await)"
            ),
            "{out}"
        );
        let only = GuardArgs {
            permissions: Some(8),
            ..GuardArgs::default()
        };
        let out = guards_impl(&ident("S"), &only, GuardCtx::Modal).to_string();
        assert!(!out.contains("__check_owner"), "{out}");
        assert!(
            out.contains("ModalSubmitCtx :: member_permissions (ctx)"),
            "{out}"
        );
        assert!(
            out.contains(":: core :: result :: Result :: Ok (:: core :: option :: Option :: None)"),
            "{out}"
        );
    }
}
