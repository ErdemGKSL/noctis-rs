//! `#[derive(PayloadEnum)]`: a unit enum as a one-digit Payload field.

use noctis_rules::RuleId;
use noctis_rules::cid::PAYLOAD_ENUM_MAX_VARIANTS;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident};

use crate::common::{combine, error, reject_generics};

/// Checks the item and returns its variant names in declaration order.
fn variants(input: &DeriveInput) -> syn::Result<Vec<&Ident>> {
    let mut errors = Vec::new();
    if let Err(e) = reject_generics(&input.generics, "a PayloadEnum") {
        errors.push(e);
    }
    let mut names = Vec::new();
    match &input.data {
        Data::Enum(data) => {
            if data.variants.is_empty() {
                errors.push(error(
                    &input.ident,
                    RuleId::CID_01,
                    "a PayloadEnum needs at least one variant",
                    "add a unit variant",
                ));
            }
            for (i, v) in data.variants.iter().enumerate() {
                if !matches!(v.fields, Fields::Unit) {
                    errors.push(error(
                        v,
                        RuleId::CID_01,
                        format!("PayloadEnum variant `{}` has fields", v.ident),
                        "a PayloadEnum has unit variants only; use a struct Payload field list instead",
                    ));
                }
                if i == PAYLOAD_ENUM_MAX_VARIANTS {
                    errors.push(error(
                        v,
                        RuleId::CID_01,
                        format!(
                            "a PayloadEnum has at most {PAYLOAD_ENUM_MAX_VARIANTS} variants (one base62 digit); `{}` is variant {}",
                            v.ident,
                            i + 1
                        ),
                        "split the enum, or use an integer field",
                    ));
                }
                names.push(&v.ident);
            }
        }
        Data::Struct(data) => errors.push(error(
            data.struct_token,
            RuleId::CID_01,
            "PayloadEnum must be derived on an enum",
            "derive `Payload` on a struct, or `PayloadEnum` on a unit enum",
        )),
        Data::Union(data) => errors.push(error(
            data.union_token,
            RuleId::CID_01,
            "PayloadEnum must be derived on an enum",
            "derive `PayloadEnum` on a unit enum",
        )),
    }
    combine(errors)?;
    Ok(names)
}

/// Expands `#[derive(PayloadEnum)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let names = variants(input)?;
    let ident = &input.ident;
    let codec = quote!(::noctis::__private::codec);
    // At most 62 variants, checked above.
    let indices: Vec<u8> = (0..=u8::MAX).take(names.len()).collect();
    let count = u8::try_from(names.len()).unwrap_or(u8::MAX);
    Ok(quote! {
        #[automatically_derived]
        impl #codec::Field for #ident {
            const MAX_LEN: ::core::option::Option<::core::primitive::usize> =
                ::core::option::Option::Some(1);

            fn encode<W: ::core::fmt::Write + ?::core::marker::Sized>(
                &self,
                out: &mut W,
            ) -> ::core::fmt::Result {
                let index: ::core::primitive::u8 = match self {
                    #(Self::#names => #indices,)*
                };
                #codec::encode_variant(index, out)
            }
        }

        #[automatically_derived]
        impl<'de> #codec::FieldDecode<'de> for #ident {
            fn decode(
                raw: &'de ::core::primitive::str,
            ) -> ::core::result::Result<Self, #codec::FieldError> {
                match #codec::decode_variant(raw, #count)? {
                    #(#indices => ::core::result::Result::Ok(Self::#names),)*
                    _ => ::core::result::Result::Err(#codec::FieldError::Overflow),
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::{DeriveInput, parse_quote};

    use super::expand;

    fn message(input: &DeriveInput) -> String {
        expand(input).map_or_else(
            |e| {
                e.into_iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            },
            |_| String::new(),
        )
    }

    fn many(n: usize) -> DeriveInput {
        let names = (0..n).map(|i| quote::format_ident!("V{i}"));
        parse_quote!(enum K { #(#names),* })
    }

    #[test]
    fn cid_01_payload_enum_structure_errors() {
        assert_eq!(
            message(&parse_quote!(
                enum K {
                    A,
                    B,
                    C,
                }
            )),
            ""
        );
        assert_eq!(
            message(&parse_quote!(
                enum E {
                    A = 5,
                    B,
                }
            )),
            ""
        );
        assert_eq!(message(&many(62)), "");

        let m = message(&many(63));
        assert!(
            m.starts_with("[CID-01] a PayloadEnum has at most 62 variants"),
            "{m}"
        );
        assert!(m.contains("`V62` is variant 63"), "{m}");
        assert!(m.contains("\n  = help: "), "{m}");
        // A 63rd variant with fields reports both errors, the count once.
        let names = (0..62).map(|i| quote::format_ident!("V{i}"));
        let m = message(&parse_quote!(enum K { #(#names,)* V62(u8), V63 }));
        assert!(m.contains("variant `V62` has fields"), "{m}");
        assert_eq!(m.matches("at most 62 variants").count(), 1, "{m}");
        assert!(m.contains("`V62` is variant 63"), "{m}");

        let m = message(&parse_quote!(
            enum K {
                A(u8),
            }
        ));
        assert!(
            m.starts_with("[CID-01] PayloadEnum variant `A` has fields"),
            "{m}"
        );
        let m = message(&parse_quote!(
            enum K {
                A { x: u8 },
            }
        ));
        assert!(
            m.starts_with("[CID-01] PayloadEnum variant `A` has fields"),
            "{m}"
        );
        let m = message(&parse_quote!(
            enum K {}
        ));
        assert!(
            m.starts_with("[CID-01] a PayloadEnum needs at least one variant"),
            "{m}"
        );
        let m = message(&parse_quote!(
            enum K<T> {
                A,
                B(T),
            }
        ));
        assert!(
            m.starts_with("[CID-01] a PayloadEnum cannot be generic"),
            "{m}"
        );
        assert!(m.contains("variant `B` has fields"), "{m}");
        let m = message(&parse_quote!(
            struct K;
        ));
        assert!(
            m.starts_with("[CID-01] PayloadEnum must be derived on an enum"),
            "{m}"
        );
    }
}
