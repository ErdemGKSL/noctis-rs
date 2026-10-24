//! Error helpers and checks shared by the derives.

use noctis_rules::RuleId;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::Generics;

/// A compile error `[<rule>] <what>\n  = help: <help>` spanned on `tokens`.
pub(crate) fn error(
    tokens: impl ToTokens,
    rule: RuleId,
    what: impl core::fmt::Display,
    help: impl core::fmt::Display,
) -> syn::Error {
    syn::Error::new_spanned(tokens, format!("[{rule}] {what}\n  = help: {help}"))
}

/// Whether `text` is one locale key segment (`[A-Za-z0-9_-]+`).
pub(crate) fn is_key_segment(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Folds `errors` into one error, or `Ok` when there is none.
pub(crate) fn combine(errors: Vec<syn::Error>) -> syn::Result<()> {
    let mut errors = errors.into_iter();
    let Some(mut first) = errors.next() else {
        return Ok(());
    };
    for e in errors {
        first.combine(e);
    }
    Err(first)
}

/// Rejects type, lifetime and const parameters and `where` clauses (CID-01).
pub(crate) fn reject_generics(generics: &Generics, what: &str) -> syn::Result<()> {
    if generics.params.is_empty() && generics.where_clause.is_none() {
        return Ok(());
    }
    Err(error(
        generics,
        RuleId::CID_01,
        format!("{what} cannot be generic"),
        "remove the type, lifetime and const parameters and the `where` clause; its width must be known at compile time",
    ))
}

/// `::core::option::Option::Some(value)` or `::core::option::Option::None`.
pub(crate) fn option(value: Option<impl ToTokens>) -> TokenStream {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |v| quote!(::core::option::Option::Some(#v)),
    )
}

/// The 64-bit FNV-1a hash of `text`: the key of an autocomplete field
/// (`__AutocompleteField<{ fnv1a64("field") }>`), shared by the derive and
/// `#[autocomplete(field)]`.
pub(crate) fn fnv1a64(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The name of `ident` without a raw-identifier prefix.
pub(crate) fn plain(ident: &syn::Ident) -> String {
    use syn::ext::IdentExt;
    ident.unraw().to_string()
}

/// The named fields of a kind-derived struct (empty for a unit struct);
/// an enum, union, tuple struct or generic struct is an error naming the
/// derive (`<derive> needs a struct with named fields or a unit struct`).
pub(crate) fn kind_fields<'a>(
    input: &'a syn::DeriveInput,
    derive: &str,
) -> syn::Result<Vec<&'a syn::Field>> {
    let message =
        format!("`#[derive({derive})]` needs a struct with named fields or a unit struct");
    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            syn::Fields::Named(named) => named.named.iter().collect(),
            syn::Fields::Unit => Vec::new(),
            syn::Fields::Unnamed(unnamed) => {
                return Err(syn::Error::new_spanned(unnamed, message));
            }
        },
        syn::Data::Enum(data) => return Err(syn::Error::new_spanned(data.enum_token, message)),
        syn::Data::Union(data) => return Err(syn::Error::new_spanned(data.union_token, message)),
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(&input.generics, message));
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a64_matches_reference_values() {
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(fnv1a64("rule"), fnv1a64("rüle"));
    }
}
