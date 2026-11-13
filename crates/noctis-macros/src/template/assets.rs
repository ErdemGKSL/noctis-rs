//! `#[derive(Assets)]`: `impl noctis::image::Assets` for plain Template data
//! whose `#[asset]` / `#[asset(required)]` fields hold remote images
//! (IMG-03). Each field's images are declared under `<path>.<field>`, so a
//! Template's `#[asset] rows: Vec<Row>` names `rows[3].avatar`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, LitStr};

use super::{collect_assets, declare_call};
use crate::common::combine;

/// The derive's entry point.
pub(crate) fn derive(input: &DeriveInput) -> TokenStream {
    expand(input).unwrap_or_else(syn::Error::into_compile_error)
}

pub(super) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let data = match &input.data {
        Data::Struct(data) => data,
        Data::Enum(data) => {
            return Err(syn::Error::new_spanned(
                data.enum_token,
                "[IMG-03] `#[derive(Assets)]` needs a struct",
            ));
        }
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "[IMG-03] `#[derive(Assets)]` needs a struct",
            ));
        }
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "[IMG-03] `#[derive(Assets)]` needs a struct without generic or lifetime parameters, \
             like the Template that holds it",
        ));
    }
    let mut errors = Vec::new();
    let assets = collect_assets(&data.fields, &mut errors);
    combine(errors)?;
    let image = quote!(::noctis::__private::image);
    let ident = &input.ident;
    let calls = assets.iter().map(|asset| {
        let name = LitStr::new(&asset.name, proc_macro2::Span::call_site());
        declare_call(asset, &quote!(&#image::asset_path(path, #name)))
    });
    // Without `#[asset]` fields `path` and `out` are unused.
    let unused = assets.is_empty().then(|| quote!(let _ = (path, out);));
    Ok(quote! {
        impl #image::Assets for #ident {
            fn declare_assets(&self, path: &str, out: &mut #image::DeclaredImages) {
                #unused
                #(#calls)*
            }
        }

        // The struct held directly in an `#[asset]` field; `Option<T>` and
        // `Vec<T>` are covered by `noctis-image`'s generic impls.
        impl #image::AssetField for #ident {
            fn declare(&self, field: &str, _required: bool, out: &mut #image::DeclaredImages) {
                #image::Assets::declare_assets(self, field, out);
            }
        }
    })
}
