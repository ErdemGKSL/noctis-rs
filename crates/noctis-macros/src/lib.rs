//! Proc macros: `command!`, user/message commands, the `Route`/`Payload`/`Options`/`Choices`/`Modal` derives, `components!`, `view!`/`tw!` and `i18n!`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! Implemented so far: the `Route`, `Payload` and `PayloadEnum` derives, the
//! literal macros `label!`, `placeholder!`, `content!` and `rgb!`, and
//! `components!`.
//!
//! Generated code names runtime items only through
//! `::noctis::__private::{codec, ui}` and std items through `::core`/`::std`
//! (AD-2). Errors are `[RULE-ID] what is wrong` plus a `= help:` line (or a
//! `noctis_rules::Violation`'s `Display`), spanned on the offending token.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod common;
mod components;
mod literal;
mod payload;
mod payload_enum;
mod route;

/// Derives `Route` and `Payload` for a struct and proves at compile time
/// that every `custom_id` of the Route fits 100 characters (CID-01).
///
/// Takes one `#[route("id", args…)]` attribute; see `noctis::prelude::Route`.
#[proc_macro_derive(Route, attributes(route))]
pub fn derive_route(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    route::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Payload` and `PayloadDecode` for a struct, for a hand-written
/// `Route`.
#[proc_macro_derive(Payload)]
pub fn derive_payload(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    payload::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Field` and `FieldDecode` for a unit enum of at most 62 variants,
/// encoded as the declaration index in one base62 digit.
#[proc_macro_derive(PayloadEnum)]
pub fn derive_payload_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    payload_enum::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A button label literal, checked at expansion against CV2-08 (at most 80
/// characters by `discord_len`); expands to a `noctis::ui::Label`.
#[proc_macro]
pub fn label(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::LABEL, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A select placeholder literal, checked at expansion against CV2-09 (at
/// most 150 characters); expands to a `noctis::ui::Placeholder`.
#[proc_macro]
pub fn placeholder(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::PLACEHOLDER, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Legacy message content literal, checked at expansion against MSG-01 (at
/// most 2000 characters); expands to a `noctis::ui::Content`.
#[proc_macro]
pub fn content(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::CONTENT, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A `0xRRGGBB` colour literal, checked at expansion against CV2-13;
/// expands to a `noctis::ui::Rgb`.
#[proc_macro]
pub fn rgb(input: TokenStream) -> TokenStream {
    literal::expand_rgb(&input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A literal Components V2 tree, checked at expansion with
/// `noctis_rules::component::validate_components` (and CID-02); expands to
/// the typed builders, yielding a `noctis::ui::Components`.
#[proc_macro]
pub fn components(input: TokenStream) -> TokenStream {
    components::components(input.into()).into()
}
