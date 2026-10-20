//! `components!`: a literal Components V2 tree, checked at `cargo check` and
//! expanded into the typed builders of `noctis-ui`.
//!
//! 1. [`parse`] reads the grammar into an owned AST that keeps spans.
//! 2. [`lower`] lowers it into `noctis_rules::component` descriptors, runs
//!    `validate_components` with `Limits::default()`, then the literal
//!    checks the descriptors cannot carry and CID-02.
//! 3. On any error the expansion is every error plus an empty `Components`;
//!    otherwise [`expand`] emits the builder calls.

mod expand;
mod lower;
mod parse;

use proc_macro2::TokenStream;
use quote::quote;

/// Expands a `components!` invocation.
pub(crate) fn components(input: TokenStream) -> TokenStream {
    let empty = quote!(::noctis::__private::ui::Components::new());
    let nodes = match parse::parse(input) {
        Ok(nodes) => nodes,
        Err(e) => {
            let e = e.into_compile_error();
            return quote!({ #e #empty });
        }
    };
    let errors = lower::check(&nodes);
    if errors.is_empty() {
        expand::expand(&nodes)
    } else {
        let errors = errors.iter().map(syn::Error::to_compile_error);
        quote!({ #(#errors)* #empty })
    }
}
