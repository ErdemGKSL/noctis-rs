//! Proc macros: `command!`, user/message commands, the `Route`/`Payload`/`Options`/`Choices`/`Modal` derives, `components!`, `view!`/`tw!` and `i18n!`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! Generated code names runtime items only through
//! `::noctis::__private::codec` and std items through `::core`/`::std`
//! (AD-2). Errors are `[RULE-ID] what is wrong` plus a `= help:` line,
//! spanned on the offending token.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod common;
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
