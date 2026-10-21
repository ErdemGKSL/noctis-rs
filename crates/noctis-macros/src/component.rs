//! `#[derive(Button)]` and the five select derives (`StringSelectMenu`,
//! `UserSelectMenu`, `RoleSelectMenu`, `MentionableSelectMenu`,
//! `ChannelSelectMenu`): one struct is one component Route with its typed
//! Payload, its mandatory `defaults(..)` and its `#[handler]` method
//! (AD-6, AD-19, AD-26, FR-58, FR-59; CID-01, CID-03, CID-04).
//!
//! The derive emits the Route impls (`Route`, `Payload`, `PayloadDecode`,
//! `SelectRoute` for a select, the CID-01 proof), `Handled`,
//! `ComponentRoute` with the declared defaults, the kind scaffolding and the
//! inherent `pub fn build(self)`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Fields};

use crate::defaults::{self, DefaultsKind};
use crate::kind::{Family, scaffolding};
use crate::payload;
use crate::route::{AttrKind, Response, parse_kind_attr, route_impls};

/// A component derive.
#[derive(Clone, Copy)]
pub(crate) enum Component {
    Button,
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect,
}

impl Component {
    fn defaults_kind(self) -> DefaultsKind {
        match self {
            Self::Button => DefaultsKind::Button,
            Self::StringSelect => DefaultsKind::StringSelect,
            Self::UserSelect => DefaultsKind::UserSelect,
            Self::RoleSelect => DefaultsKind::RoleSelect,
            Self::MentionableSelect => DefaultsKind::MentionableSelect,
            Self::ChannelSelect => DefaultsKind::ChannelSelect,
        }
    }

    fn derive(self) -> &'static str {
        match self {
            Self::Button => "Button",
            Self::StringSelect => "StringSelectMenu",
            Self::UserSelect => "UserSelectMenu",
            Self::RoleSelect => "RoleSelectMenu",
            Self::MentionableSelect => "MentionableSelectMenu",
            Self::ChannelSelect => "ChannelSelectMenu",
        }
    }

    fn variant(self) -> TokenStream {
        match self {
            Self::Button => quote!(Button),
            Self::StringSelect => quote!(StringSelect),
            Self::UserSelect => quote!(UserSelect),
            Self::RoleSelect => quote!(RoleSelect),
            Self::MentionableSelect => quote!(MentionableSelect),
            Self::ChannelSelect => quote!(ChannelSelect),
        }
    }

    fn attr_kind(self) -> AttrKind {
        AttrKind {
            attr: self.defaults_kind().attr(),
            derive: self.derive(),
            select: !matches!(self, Self::Button),
        }
    }
}

/// The Payload impls of a struct with named fields or a unit struct.
pub(crate) fn payload_impls(input: &DeriveInput) -> TokenStream {
    let empty = Fields::Unit;
    let fields = match &input.data {
        syn::Data::Struct(data) => &data.fields,
        _ => &empty,
    };
    payload::impls(input, fields)
}

/// Expands the derive; on an error, the error and the placeholder impls
/// (`kind::fallback`).
pub(crate) fn derive_or_fallback(component: Component, input: &DeriveInput) -> TokenStream {
    let result = derive(component, input);
    crate::kind::or_fallback(input, Family::Component, result, || {
        let core = quote!(::noctis::__private::core);
        let ident = &input.ident;
        let variant = component.variant();
        let route = crate::kind::fallback_route(ident);
        quote! {
            impl #core::__Component for #ident {
                const ROUTE: &'static ::core::primitive::str = "";
                const KIND: #core::ComponentKind = #core::ComponentKind::#variant;
                const RESPONSE: #core::__Response = #core::__Response::Update;
            }

            #route
        }
    })
}

/// Expands a component derive.
pub(crate) fn derive(component: Component, input: &DeriveInput) -> syn::Result<TokenStream> {
    crate::common::kind_fields(input, component.derive())?;
    let ident = &input.ident;
    let kind = component.defaults_kind();
    let attr = parse_kind_attr(&input.attrs, ident, component.attr_kind())?;
    let declared = defaults::parse(kind, attr.defaults.as_ref(), &attr.attr)?;
    let ui = quote!(::noctis::__private::ui);
    let core = quote!(::noctis::__private::core);
    let route = route_impls(ident, &attr.args, &quote!(Self));
    let payload = payload_impls(input);
    let (defaults_ty, defaults) = declared.tokens(kind);
    let checks = declared.checks();
    let id = &attr.args.id;
    let variant = component.variant();
    let build = build_fn(component, &declared);
    let response = match attr.args.response {
        Response::Update => quote!(Update),
        Response::Ephemeral => quote!(Ephemeral),
        Response::Public => quote!(Public),
    };
    let scaffolding = scaffolding(ident, Family::Component, !attr.args.auto_defer);
    Ok(quote! {
        #route

        #payload

        #[automatically_derived]
        impl #ui::Handled for #ident {}

        #[automatically_derived]
        impl #ui::ComponentRoute for #ident {
            type Defaults = #defaults_ty;
            const DEFAULTS: Self::Defaults = #defaults;
        }

        #checks

        #[automatically_derived]
        impl #core::__Component for #ident {
            const ROUTE: &'static ::core::primitive::str = #id;
            const KIND: #core::ComponentKind = #core::ComponentKind::#variant;
            const RESPONSE: #core::__Response = #core::__Response::#response;
        }

        impl #ident {
            #build
        }

        #scaffolding
    })
}

/// The inherent `build(self)` of a component struct.
fn build_fn(component: Component, declared: &defaults::Defaults) -> TokenStream {
    let ui = quote!(::noctis::__private::ui);
    let defaults = quote!(&<Self as #ui::ComponentRoute>::DEFAULTS);
    let (ret, body, doc) = match component {
        Component::Button => (
            quote!(#ui::RouteButton<Self>),
            quote!(#ui::__macro::build_button(self, #defaults)),
            "The button of this value: its declared `defaults(..)`, this value as its Payload. Override one field with the setters.",
        ),
        Component::StringSelect => match declared {
            defaults::Defaults::Select {
                options: Some(_), ..
            } => (
                quote!(#ui::StringSelect),
                quote!(#ui::__macro::build_string_select(self, #defaults)),
                "The String Select of this value: its declared `defaults(..)` (with the options of its `Choices` enum), this value as its Payload.",
            ),
            _ => (
                quote!(#ui::StringSelectBuilder<Self>),
                quote!(#ui::__macro::build_string_select_builder(self, &<Self as #ui::ComponentRoute>::DEFAULTS.select)),
                "The String Select of this value without options: call `.options(..)` before placing it (CV2-09).",
            ),
        },
        Component::UserSelect => (
            quote!(#ui::UserSelect),
            quote!(#ui::__macro::build_user_select(self, #defaults)),
            "The User Select of this value: its declared `defaults(..)`, this value as its Payload.",
        ),
        Component::RoleSelect => (
            quote!(#ui::RoleSelect),
            quote!(#ui::__macro::build_role_select(self, #defaults)),
            "The Role Select of this value: its declared `defaults(..)`, this value as its Payload.",
        ),
        Component::MentionableSelect => (
            quote!(#ui::MentionableSelect),
            quote!(#ui::__macro::build_mentionable_select(self, #defaults)),
            "The Mentionable Select of this value: its declared `defaults(..)`, this value as its Payload.",
        ),
        Component::ChannelSelect => (
            quote!(#ui::ChannelSelect),
            quote!(#ui::__macro::build_channel_select(self, #defaults)),
            "The Channel Select of this value: its declared `defaults(..)`, this value as its Payload.",
        ),
    };
    quote! {
        #[doc = #doc]
        #[must_use]
        pub fn build(self) -> #ret {
            #body
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(component: Component, src: TokenStream) -> String {
        let input: DeriveInput = syn::parse2(src).unwrap();
        match derive(component, &input) {
            Ok(t) => t.to_string(),
            Err(e) => e
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n---\n"),
        }
    }

    #[test]
    fn button() {
        let out = run(
            Component::Button,
            quote! {
                #[button("shop.buy", defaults(style = Primary, label = "Buy"))]
                struct Buy { item: u64 }
            },
        );
        assert!(out.contains("RouteId :: new (\"shop.buy\")"), "{out}");
        assert!(out.contains("RouteShape :: INLINE ;"), "{out}");
        assert!(!out.contains("SelectRoute"), "{out}");
        assert!(
            out.contains("impl :: noctis :: __private :: ui :: Handled for Buy"),
            "{out}"
        );
        assert!(out.contains("ComponentKind :: Button"), "{out}");
        assert!(
            out.contains(
                "pub fn build (self) -> :: noctis :: __private :: ui :: RouteButton < Self >"
            ),
            "{out}"
        );
        assert!(out.contains("Register for Buy"), "{out}");
    }

    /// Story 7.2 Matrix 52 and 53: `response` and `auto_defer`.
    #[test]
    fn response_and_auto_defer() {
        let out = run(
            Component::Button,
            quote! {
                #[button("shop.buy", defaults(style = Primary, label = "Buy"))]
                struct Buy;
            },
        );
        assert!(out.contains("__Response :: Update"), "{out}");
        assert!(
            out.contains("type Defer = :: noctis :: __private :: core :: AutoDefer"),
            "{out}"
        );
        let out = run(
            Component::UserSelect,
            quote! {
                #[user_select("pick", response = ephemeral, auto_defer = false, defaults(placeholder = "p"))]
                struct Pick;
            },
        );
        assert!(out.contains("__Response :: Ephemeral"), "{out}");
        assert!(
            out.contains("type Defer = :: noctis :: __private :: core :: Manual"),
            "{out}"
        );
        let out = run(
            Component::Button,
            quote! {
                #[button("b", response = public, auto_defer = true, defaults(style = Primary, label = "B"))]
                struct B;
            },
        );
        assert!(out.contains("__Response :: Public"), "{out}");
        assert!(out.contains("AutoDefer"), "{out}");
        for (arg, start) in [
            (
                quote!(response = edit),
                "[CID-03] `response` takes `update`, `ephemeral` or `public`",
            ),
            (quote!(response = "update"), "[CID-03] `response` takes"),
            (
                quote!(auto_defer = 1),
                "[CID-03] `auto_defer` takes `true` or `false`",
            ),
            (quote!(auto_defer = yes), "[CID-03] `auto_defer` takes"),
            (
                quote!(response = update, response = public),
                "[CID-03] #[button] argument `response` is given twice",
            ),
            (
                quote!(auto_defer = false, auto_defer = false),
                "[CID-03] #[button] argument `auto_defer` is given twice",
            ),
        ] {
            let e = run(
                Component::Button,
                quote!(
                    #[button("a", #arg, defaults(style = Primary, label = "a"))]
                    struct B;
                ),
            );
            assert!(e.starts_with(start), "{start}\n=> {e}");
        }
    }

    #[test]
    fn selects() {
        let out = run(
            Component::StringSelect,
            quote! {
                #[string_select("pick", defaults(placeholder = "Pick"))]
                struct Pick;
            },
        );
        assert!(out.contains("RouteShape :: INLINE . select ()"), "{out}");
        assert!(out.contains("SelectRoute for Pick"), "{out}");
        assert!(out.contains("StringSelectBuilder < Self >"), "{out}");
        let out = run(
            Component::StringSelect,
            quote! {
                #[string_select("pick", defaults(placeholder = "Pick", options = Colour))]
                struct Pick;
            },
        );
        assert!(
            out.contains("-> :: noctis :: __private :: ui :: StringSelect {"),
            "{out}"
        );
        let out = run(
            Component::ChannelSelect,
            quote! {
                #[channel_select("ch", signed, defaults(placeholder = "Pick", channel_types = [GuildText]))]
                struct Ch { n: u8 }
            },
        );
        assert!(out.contains(". select () . signed ()"), "{out}");
        assert!(out.contains("ComponentKind :: ChannelSelect"), "{out}");
    }

    /// Story 6.11 Matrix 6, 33, 44, 45.
    #[test]
    #[expect(clippy::too_many_lines, reason = "one table of error cases")]
    fn errors() {
        let shape = run(
            Component::Button,
            quote!(
                #[button("a", defaults(style = Primary, label = "a"))]
                struct T(u8);
            ),
        );
        assert_eq!(
            shape,
            "`#[derive(Button)]` needs a struct with named fields or a unit struct"
        );
        let shape = run(
            Component::UserSelect,
            quote!(
                enum E {
                    A,
                }
            ),
        );
        assert_eq!(
            shape,
            "`#[derive(UserSelectMenu)]` needs a struct with named fields or a unit struct"
        );
        let generic = run(
            Component::Button,
            quote!(
                struct G<T> {
                    t: T,
                }
            ),
        );
        assert!(
            generic.contains("needs a struct with named fields"),
            "{generic}"
        );
        assert_eq!(
            run(
                Component::Button,
                quote!(
                    #[button("a")]
                    struct B;
                )
            ),
            DefaultsKind::Button.needs()
        );
        assert!(
            run(
                Component::Button,
                quote!(
                    struct B;
                )
            )
            .starts_with(
                "[CID-03] #[derive(Button)] needs a #[button(\"id\", defaults(..))] attribute"
            )
        );
        let e = run(
            Component::StringSelect,
            quote!(
                #[string_select("a", select, defaults(placeholder = "p"))]
                struct S;
            ),
        );
        assert!(
            e.starts_with("[CID-03] unknown #[string_select] argument `select`"),
            "{e}"
        );
        assert!(
            e.contains("every select kind carries the select nonce"),
            "{e}"
        );
        for arg in [quote!(view), quote!(opens_modal)] {
            let e = run(
                Component::Button,
                quote!(
                    #[button("a", #arg, defaults(style = Primary, label = "a"))]
                    struct B;
                ),
            );
            assert!(e.starts_with("[CID-03] unknown #[button] argument"), "{e}");
        }
        for arg in [quote!(owner = user), quote!(rate_limit(1, 2))] {
            let e = run(
                Component::Button,
                quote!(
                    #[button("a", #arg, defaults(style = Primary, label = "a"))]
                    struct B;
                ),
            );
            assert!(e.contains("is a runtime argument of #[button] that this Noctis release does not support yet"), "{e}");
        }
        let e = run(
            Component::Button,
            quote!(
                #[button("a", by_ref, signed, defaults(style = Primary, label = "a"))]
                struct B;
            ),
        );
        assert!(e.contains("cannot be `signed`"), "{e}");
        for id in ["", "Shop", "_x", "a:b", "shöp", "abcdefghijklmnopqrstuvwxy"] {
            let e = run(
                Component::Button,
                quote!(
                    #[button(#id, defaults(style = Primary, label = "a"))]
                    struct B;
                ),
            );
            assert!(e.starts_with("[CID-03] route id"), "{id}: {e}");
        }
        let e = run(
            Component::Button,
            quote!(
                #[button("a", defaults(style = Primary, label = "a"), defaults(label = "b"))]
                struct B;
            ),
        );
        assert!(e.contains("`defaults` is given twice"), "{e}");
    }
}
