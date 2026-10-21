//! The scaffolding every kind derive shares (AD-6, AD-26): the hidden
//! `__Kind` impl, the `Register` impl `Module::add::<T>()` calls, and the
//! missing-handler check (CID-04 for components and modals).
//!
//! `#[handler]` emits, inside the struct's own `impl`, the inherent items
//! `__NOCTIS_HAS_HANDLER` and `descriptor()`; they shadow the fallbacks of
//! core's blanket `__NoHandler` trait. Without a `#[handler]` method the
//! fallback const reads `false` and the derive's `const _` panics at
//! `cargo check` (E0080), and the fallback `descriptor()` keeps the
//! `Register` impl compiling, so that panic is the only error.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{Ident, LitStr};

use crate::common::plain;

/// What a kind derive declares for its struct.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    /// `#[derive(SlashCommand)]`.
    Slash,
    /// `#[derive(UserCommand)]` / `#[derive(MessageCommand)]`.
    Menu,
    /// `#[derive(EventHandler)]`.
    Event,
    /// `#[derive(Button)]` and the select derives.
    Component,
    /// `#[derive(Modal)]`.
    Modal,
}

impl Family {
    /// The kind marker type.
    fn marker(self) -> TokenStream {
        let core = core();
        match self {
            Self::Slash => quote!(#core::__SlashKind),
            Self::Menu => quote!(#core::__MenuKind),
            Self::Event => quote!(#core::__EventKind),
            Self::Component => quote!(#core::__ComponentKind),
            Self::Modal => quote!(#core::__ModalKind),
        }
    }

    /// The descriptor type.
    fn descriptor(self) -> TokenStream {
        let core = core();
        match self {
            Self::Slash => quote!(#core::CommandHandler),
            Self::Menu => quote!(#core::ContextMenuHandler),
            Self::Event => quote!(#core::EventHandler),
            Self::Component => quote!(#core::ComponentHandler),
            Self::Modal => quote!(#core::ModalHandler),
        }
    }

    /// The handler signature the missing-handler help shows.
    fn help_signature(self, name: &str) -> String {
        match self {
            Self::Slash => format!(
                "add impl {name} {{ #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> }}"
            ),
            Self::Menu => format!(
                "add impl {name} {{ #[handler] async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Responded> }} (`ResolvedMessage` for a message command)"
            ),
            Self::Event => format!(
                "add impl {name} {{ #[handler] async fn handle(self, ctx: EventCtx, event: MessageCreate) -> HandlerResult<()> }}"
            ),
            Self::Component => format!(
                "add impl {name} {{ #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> }}"
            ),
            Self::Modal => format!(
                "add impl {name} {{ #[handler] async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> }}"
            ),
        }
    }

    /// The text of the missing-handler panic.
    pub(crate) fn missing_handler(self, name: &str) -> String {
        let what = match self {
            Self::Component | Self::Modal => format!(
                "[CID-04] `{name}` has no #[handler] method, so no component can be built for it"
            ),
            Self::Slash | Self::Menu | Self::Event => {
                format!("`{name}` has no #[handler] method, so it cannot be registered")
            }
        };
        format!("{what}\n  = help: {}", self.help_signature(name))
    }
}

/// `::noctis::__private::core`.
pub(crate) fn core() -> TokenStream {
    quote!(::noctis::__private::core)
}

/// A string literal usable as a `panic!` format string.
pub(crate) fn panic_lit(text: &str) -> proc_macro2::Literal {
    proc_macro2::Literal::string(&text.replace('{', "{{").replace('}', "}}"))
}

/// What a kind derive emits besides its errors when it fails: the `__Kind`
/// and `Register` impls and the family impl `family_impl` with placeholder
/// values, so a `#[handler]` method and `Module::add::<T>()` add no
/// follow-up errors. There is no missing-handler check.
pub(crate) fn fallback(
    ident: &Ident,
    family: Family,
    family_impl: &TokenStream,
    errors: &TokenStream,
) -> TokenStream {
    let items = items(ident, family, false);
    quote! {
        #errors
        #items
        #family_impl
    }
}

/// The derive's `result`, or on an error the error with the placeholder
/// impls of `kind::fallback` (`family_impl` gives the family impl); an enum,
/// union or generic struct gets the error alone.
pub(crate) fn or_fallback(
    input: &syn::DeriveInput,
    family: Family,
    result: syn::Result<TokenStream>,
    family_impl: impl FnOnce() -> TokenStream,
) -> TokenStream {
    match result {
        Ok(tokens) => tokens,
        Err(e) => {
            let plain_struct = matches!(input.data, syn::Data::Struct(_))
                && input.generics.params.is_empty()
                && input.generics.where_clause.is_none();
            if plain_struct {
                fallback(
                    &input.ident,
                    family,
                    &family_impl(),
                    &e.into_compile_error(),
                )
            } else {
                e.into_compile_error()
            }
        }
    }
}

/// The `__Kind` impl, the `Register` impl and the missing-handler check of
/// the struct `ident`; `manual` is a declared `auto_defer = false`.
pub(crate) fn scaffolding(ident: &Ident, family: Family, manual: bool) -> TokenStream {
    let items = items(ident, family, manual);
    let check = missing_handler_check(ident, family);
    quote! {
        #items
        #check
    }
}

/// The `const _` that fails `cargo check` without a `#[handler]` method.
fn missing_handler_check(ident: &Ident, family: Family) -> TokenStream {
    let missing = panic_lit(&family.missing_handler(&plain(ident)));
    quote_spanned! {ident.span()=>
        #[allow(unused_imports)]
        const _: () = {
            use ::noctis::__private::core::__NoHandler as _;
            if !<#ident>::__NOCTIS_HAS_HANDLER {
                ::core::panic!(#missing)
            }
        };
    }
}

/// The `__Kind` and `Register` impls.
fn items(ident: &Ident, family: Family, manual: bool) -> TokenStream {
    let core = core();
    let name = plain(ident);
    let name_lit = LitStr::new(&name, Span::call_site());
    let marker = family.marker();
    let descriptor = family.descriptor();
    let defer = if manual {
        quote!(#core::Manual)
    } else {
        quote!(#core::AutoDefer)
    };
    quote! {
        #[automatically_derived]
        impl #core::__Kind for #ident {
            type Kind = #marker;
            type Defer = #defer;
            type Descriptor = #descriptor;
            const NAME: &'static ::core::primitive::str = ::core::concat!(::core::module_path!(), "::", #name_lit);
        }

        #[automatically_derived]
        impl #core::Register for #ident {
            #[allow(unused_imports)]
            fn __register(module: #core::Module) -> #core::Module {
                use ::noctis::__private::core::__NoHandler as _;
                module.__add(<#ident>::descriptor())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_handler_texts() {
        let c = Family::Component.missing_handler("Buy");
        assert!(c.starts_with(
            "[CID-04] `Buy` has no #[handler] method, so no component can be built for it\n  = help: add impl Buy { #[handler] async fn handle(self, ctx: ComponentCtx)"
        ));
        assert!(
            Family::Modal
                .missing_handler("Bio")
                .contains("ModalSubmitCtx")
        );
        let s = Family::Slash.missing_handler("Ping");
        assert!(s.starts_with("`Ping` has no #[handler] method"), "{s}");
        assert!(!s.contains("CID-04"));
        assert!(panic_lit("{x}").to_string().contains("{{x}}"));
    }

    #[test]
    fn scaffolding_names_the_struct() {
        let out = scaffolding(
            &Ident::new("Buy", Span::call_site()),
            Family::Component,
            false,
        )
        .to_string();
        assert!(out.contains("__ComponentKind"), "{out}");
        assert!(out.contains("AutoDefer"), "{out}");
        assert!(out.contains("Register for Buy"), "{out}");
        assert!(out.contains("__NOCTIS_HAS_HANDLER"), "{out}");
        let manual = scaffolding(&Ident::new("P", Span::call_site()), Family::Slash, true);
        assert!(manual.to_string().contains("Manual"));
    }
}
