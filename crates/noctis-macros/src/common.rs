//! Error helpers and checks shared by the derives.

use noctis_rules::RuleId;
use quote::ToTokens;
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
