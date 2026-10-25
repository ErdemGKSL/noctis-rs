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

use crate::apps::Apps;
use crate::common::plain;
use crate::flags::Flags;

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
    /// `#[derive(Hook)]` (a Lifecycle Hook).
    Hook,
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
            Self::Hook => quote!(#core::__LifecycleKind),
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
            Self::Hook => quote!(#core::LifecycleHook),
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
            Self::Hook => format!(
                "add impl {name} {{ #[handler] async fn handle(self, ctx: HookCtx, event: AppStarted) -> HandlerResult<()> }}"
            ),
        }
    }

    /// The text of the missing-handler panic.
    pub(crate) fn missing_handler(self, name: &str) -> String {
        let what = match self {
            Self::Component | Self::Modal => format!(
                "[CID-04] `{name}` has no #[handler] method, so no component can be built for it"
            ),
            Self::Slash | Self::Menu | Self::Event | Self::Hook => {
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
    let items = items(ident, family, false, &Flags::default(), &Apps::default());
    quote! {
        #errors
        #items
        #family_impl
    }
}

/// The placeholder Route impls of a component or modal struct whose derive
/// failed: a valid placeholder id and an empty Payload that never decodes,
/// so the `#[handler]` binding's codec bounds hold and the derive's error
/// stays the only one.
pub(crate) fn fallback_route(ident: &Ident) -> TokenStream {
    let codec = quote!(::noctis::__private::codec);
    quote! {
        impl #codec::Route for #ident {
            const ID: #codec::RouteId = #codec::RouteId::new("noctis.invalid");
            const SHAPE: #codec::RouteShape = #codec::RouteShape::INLINE;
            const SCHEMA: ::core::primitive::u8 = 0;
            type Payload = Self;
        }

        impl #codec::DeclaredRuntime for #ident {}

        impl #codec::Payload for #ident {
            const MAX_ENCODED_LEN: ::core::option::Option<::core::primitive::usize> =
                ::core::option::Option::Some(0);

            fn encode_fields(
                &self,
                _: &mut #codec::FieldWriter<'_>,
            ) -> ::core::fmt::Result {
                ::core::result::Result::Ok(())
            }
        }

        impl<'de> #codec::PayloadDecode<'de> for #ident {
            fn decode_fields(
                _: &mut #codec::FieldReader<'de>,
            ) -> ::core::result::Result<Self, #codec::CodecError> {
                ::core::result::Result::Err(#codec::CodecError::Empty)
            }
        }
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
/// the struct `ident`; `manual` is a declared `auto_defer = false`, `flags`
/// the attribute's Boot Flags (`flags = [..]`).
pub(crate) fn scaffolding(
    ident: &Ident,
    family: Family,
    manual: bool,
    flags: &Flags,
) -> TokenStream {
    scaffolding_scoped(ident, family, manual, flags, &Apps::default())
}

/// [`scaffolding`] for a registration restricted to the App ids `apps`
/// (`apps = [..]` of the Command macros; empty for unrestricted).
pub(crate) fn scaffolding_scoped(
    ident: &Ident,
    family: Family,
    manual: bool,
    flags: &Flags,
    apps: &Apps,
) -> TokenStream {
    let items = items(ident, family, manual, flags, apps);
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

/// The `__Kind` and `Register` impls; a flagged registration registers with
/// `Module::__add_flagged`, an App-restricted one with
/// `Module::__add_scoped`.
fn items(ident: &Ident, family: Family, manual: bool, flags: &Flags, apps: &Apps) -> TokenStream {
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
    let add = if !apps.is_empty() {
        let (flags, apps) = (flags.slice(), apps.slice());
        quote!(module.__add_scoped(#flags, #apps, <#ident>::descriptor()))
    } else if flags.is_empty() {
        quote!(module.__add(<#ident>::descriptor()))
    } else {
        let flags = flags.slice();
        quote!(module.__add_flagged(#flags, <#ident>::descriptor()))
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
                #add
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
        let h = Family::Hook.missing_handler("Up");
        assert!(
            h.starts_with("`Up` has no #[handler] method, so it cannot be registered\n  = help: add impl Up { #[handler] async fn handle(self, ctx: HookCtx, event: AppStarted) -> HandlerResult<()> }"),
            "{h}"
        );
        assert!(panic_lit("{x}").to_string().contains("{{x}}"));
    }

    #[test]
    fn scaffolding_names_the_struct() {
        let out = scaffolding(
            &Ident::new("Buy", Span::call_site()),
            Family::Component,
            false,
            &Flags::default(),
        )
        .to_string();
        assert!(out.contains("__add (<"), "{out}");
        assert!(out.contains("__ComponentKind"), "{out}");
        assert!(out.contains("AutoDefer"), "{out}");
        assert!(out.contains("Register for Buy"), "{out}");
        assert!(out.contains("__NOCTIS_HAS_HANDLER"), "{out}");
        let manual = scaffolding(
            &Ident::new("P", Span::call_site()),
            Family::Slash,
            true,
            &Flags::default(),
        );
        assert!(manual.to_string().contains("Manual"));
        let flags =
            Flags::parse(&quote!(flags = ["beta"]).into_iter().collect::<Vec<_>>()).unwrap();
        let flagged = scaffolding(
            &Ident::new("P", Span::call_site()),
            Family::Slash,
            false,
            &flags,
        )
        .to_string();
        assert!(
            flagged.contains("__add_flagged (& [\"beta\"] , <"),
            "{flagged}"
        );
        let apps = Apps::parse(&quote!(apps = ["main"]).into_iter().collect::<Vec<_>>()).unwrap();
        let scoped = scaffolding_scoped(
            &Ident::new("P", Span::call_site()),
            Family::Slash,
            false,
            &Flags::default(),
            &apps,
        )
        .to_string();
        assert!(
            scoped.contains("__add_scoped (& [] , & [\"main\"] , <"),
            "{scoped}"
        );
        let both = scaffolding_scoped(
            &Ident::new("P", Span::call_site()),
            Family::Slash,
            false,
            &flags,
            &apps,
        )
        .to_string();
        assert!(
            both.contains("__add_scoped (& [\"beta\"] , & [\"main\"] , <"),
            "{both}"
        );
    }
}
