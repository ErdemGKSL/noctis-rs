//! `#[derive(Payload)]`: `Payload` and `PayloadDecode` for a struct, also
//! the Payload half of `#[derive(Route)]`.

use noctis_rules::RuleId;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Member};

use crate::common::{error, reject_generics};

/// Checks that `input` is a non-generic struct (CID-01); `what` names the
/// derive in the error.
pub(crate) fn check_struct<'a>(input: &'a DeriveInput, what: &str) -> syn::Result<&'a Fields> {
    let generics = reject_generics(&input.generics, what);
    let fields = match &input.data {
        Data::Struct(data) => Ok(&data.fields),
        Data::Enum(data) => Err(error(
            data.enum_token,
            RuleId::CID_01,
            format!("{what} must be a struct, not an enum"),
            "use a struct with named, tuple or no fields; derive `PayloadEnum` on a unit enum to use it as a field",
        )),
        Data::Union(data) => Err(error(
            data.union_token,
            RuleId::CID_01,
            format!("{what} must be a struct, not a union"),
            "use a struct with named, tuple or no fields",
        )),
    };
    match (generics, fields) {
        (Ok(()), fields) => fields,
        (Err(e), Ok(_)) => Err(e),
        (Err(mut e), Err(f)) => {
            e.combine(f);
            Err(e)
        }
    }
}

/// Expands `#[derive(Payload)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = check_struct(input, "a Payload")?;
    Ok(impls(input, fields))
}

/// The `Payload` and `PayloadDecode` impls of a checked struct.
pub(crate) fn impls(input: &DeriveInput, fields: &Fields) -> TokenStream {
    let ident = &input.ident;
    let codec = quote!(::noctis::__private::codec);
    let types = fields.iter().map(|f| &f.ty);
    let members: Vec<Member> = fields.members().collect();
    let (out, reader) = if fields.is_empty() {
        (format_ident!("_out"), format_ident!("_f"))
    } else {
        (format_ident!("out"), format_ident!("f"))
    };
    let build = match fields {
        Fields::Named(_) => quote!(Self { #(#members: #reader.field()?,)* }),
        Fields::Unnamed(_) => {
            let reads = fields.iter().map(|_| quote!(#reader.field()?));
            quote!(Self(#(#reads),*))
        }
        Fields::Unit => quote!(Self),
    };
    quote! {
        #[automatically_derived]
        impl #codec::Payload for #ident {
            const MAX_ENCODED_LEN: ::core::option::Option<::core::primitive::usize> =
                #codec::sum_fields(&[#(<#types as #codec::Field>::MAX_LEN),*]);

            fn encode_fields(
                &self,
                #out: &mut #codec::FieldWriter<'_>,
            ) -> ::core::fmt::Result {
                #(#out.field(&self.#members)?;)*
                ::core::result::Result::Ok(())
            }
        }

        #[automatically_derived]
        impl<'de> #codec::PayloadDecode<'de> for #ident {
            fn decode_fields(
                #reader: &mut #codec::FieldReader<'de>,
            ) -> ::core::result::Result<Self, #codec::CodecError> {
                ::core::result::Result::Ok(#build)
            }
        }
    }
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

    #[test]
    fn cid_01_payload_structure_errors() {
        for input in [
            parse_quote!(
                struct P<T>(T);
            ),
            parse_quote!(
                struct P<'a>(&'a str);
            ),
            parse_quote!(
                struct P<const N: usize>;
            ),
            parse_quote!(
                struct P
                where
                    u8: Copy;
            ),
        ] {
            let m = message(&input);
            assert!(m.starts_with("[CID-01] a Payload cannot be generic"), "{m}");
            assert!(m.contains("\n  = help: "), "{m}");
        }
        let m = message(&parse_quote!(
            enum P {
                A,
            }
        ));
        assert!(
            m.starts_with("[CID-01] a Payload must be a struct, not an enum"),
            "{m}"
        );
        let m = message(&parse_quote!(union P { a: u8 }));
        assert!(
            m.starts_with("[CID-01] a Payload must be a struct, not a union"),
            "{m}"
        );
    }

    #[test]
    fn payload_shapes_expand() {
        for input in [
            parse_quote!(
                struct P {
                    a: u8,
                    b: bool,
                }
            ),
            parse_quote!(
                struct P(u8, bool);
            ),
            parse_quote!(
                struct P;
            ),
            parse_quote!(
                struct P {}
            ),
            parse_quote!(
                struct P();
            ),
        ] {
            assert_eq!(message(&input), "");
        }
    }
}
