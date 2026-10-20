//! `#[derive(Choices)]`: a unit enum as a modal option set whose labels,
//! values and descriptions are checked at expansion (MOD-07), with its
//! variant count at the type level (`Count = Arity<N>`).

use noctis_rules::len::discord_len;
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Data, DeriveInput, Fields, Ident, LitStr, Token, Variant};

use crate::common::{combine, error, option};

const HELP: &str = "a Choices enum has unit variants, each with #[choice(\"Label\")] or #[choice(\"Label\", value = \"v\", description = \"d\")]";

/// One variant's `#[choice(..)]`.
struct Choice {
    label: LitStr,
    value: Option<LitStr>,
    description: Option<LitStr>,
}

impl Parse for Choice {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let label: LitStr = input.parse()?;
        let mut value = None;
        let mut description = None;
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let lit: LitStr = input.parse()?;
            let slot = match name.to_string().as_str() {
                "value" => &mut value,
                "description" => &mut description,
                other => {
                    return Err(syn::Error::new(
                        name.span(),
                        format!(
                            "unknown #[choice] argument `{other}`; expected `value` or `description`"
                        ),
                    ));
                }
            };
            if slot.replace(lit).is_some() {
                return Err(syn::Error::new(
                    name.span(),
                    format!("`{name}` is given twice"),
                ));
            }
        }
        Ok(Self {
            label,
            value,
            description,
        })
    }
}

/// A checked variant: ident, label, value and description.
struct Checked {
    ident: Ident,
    label: String,
    value: String,
    description: Option<String>,
}

/// MOD-07 for one option text: 1 to 100 (or at most 100 for a description).
fn check_text(lit: &LitStr, what: &str, min: usize, errors: &mut Vec<syn::Error>) {
    let max = Limits::default().modal_option_text_len;
    let len = discord_len(&lit.value());
    if len < min || len > max {
        let limit = if min == 0 {
            Bound::AtMost(max)
        } else {
            Bound::Between { min, max }
        };
        let v = Violation::new(RuleId::MOD_07, what, limit, Actual::Count(len));
        errors.push(syn::Error::new(lit.span(), v.to_string()));
    }
}

fn choice_attr(variant: &Variant) -> syn::Result<Option<&Attribute>> {
    let mut attrs = variant.attrs.iter().filter(|a| a.path().is_ident("choice"));
    let first = attrs.next();
    if let Some(second) = attrs.next() {
        return Err(syn::Error::new_spanned(
            second,
            "a variant takes one #[choice(..)] attribute",
        ));
    }
    Ok(first)
}

fn check_variant(variant: &Variant, errors: &mut Vec<syn::Error>) -> Option<Checked> {
    if !matches!(variant.fields, Fields::Unit) {
        errors.push(error(
            variant,
            RuleId::MOD_07,
            format!("Choices variant `{}` has fields", variant.ident),
            HELP,
        ));
        return None;
    }
    let attr = match choice_attr(variant) {
        Ok(Some(attr)) => attr,
        Ok(None) => {
            errors.push(syn::Error::new_spanned(
                variant,
                format!("variant `{}` needs #[choice(\"Label\")]", variant.ident),
            ));
            return None;
        }
        Err(e) => {
            errors.push(e);
            return None;
        }
    };
    let choice: Choice = match attr.parse_args() {
        Ok(c) => c,
        Err(e) => {
            errors.push(e);
            return None;
        }
    };
    check_text(&choice.label, "label", 1, errors);
    if let Some(v) = &choice.value {
        check_text(v, "value", 1, errors);
    }
    if let Some(d) = &choice.description {
        check_text(d, "description", 0, errors);
    }
    let value = choice
        .value
        .as_ref()
        .map_or_else(|| variant.ident.unraw().to_string(), LitStr::value);
    if choice.value.is_none() {
        let len = discord_len(&value);
        let max = Limits::default().modal_option_text_len;
        if len > max {
            let v = Violation::new(
                RuleId::MOD_07,
                "value",
                Bound::Between { min: 1, max },
                Actual::Count(len),
            );
            errors.push(syn::Error::new(variant.ident.span(), v.to_string()));
        }
    }
    Some(Checked {
        ident: variant.ident.clone(),
        label: choice.label.value(),
        value,
        description: choice.description.as_ref().map(LitStr::value),
    })
}

/// Expands `#[derive(Choices)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(error(
            name,
            RuleId::MOD_07,
            "derive(Choices) takes a unit enum",
            HELP,
        ));
    };
    let mut errors = Vec::new();
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        errors.push(error(
            &input.generics,
            RuleId::MOD_07,
            "a Choices enum cannot be generic",
            "remove the type, lifetime and const parameters and the `where` clause",
        ));
    }
    if data.variants.is_empty() {
        errors.push(error(
            name,
            RuleId::MOD_07,
            "a Choices enum needs at least one variant",
            HELP,
        ));
    }
    let mut checked: Vec<Checked> = Vec::new();
    for variant in &data.variants {
        let Some(c) = check_variant(variant, &mut errors) else {
            continue;
        };
        if checked.iter().any(|o| o.value == c.value) {
            errors.push(syn::Error::new_spanned(
                variant,
                format!(
                    "duplicate choice value `{}`: every variant needs its own value",
                    c.value
                ),
            ));
        }
        checked.push(c);
    }
    combine(errors)?;

    let count = Literal::usize_unsuffixed(checked.len());
    let idents: Vec<&Ident> = checked.iter().map(|c| &c.ident).collect();
    let labels = checked.iter().map(|c| &c.label);
    let values: Vec<&String> = checked.iter().map(|c| &c.value).collect();
    let descriptions = checked.iter().map(|c| option(c.description.as_ref()));
    let ui = quote!(::noctis::__private::ui);
    Ok(quote! {
        impl #ui::Choices for #name {
            type Count = #ui::Arity<#count>;
            const ALL: &'static [Self] = &[#(Self::#idents),*];

            fn label(&self) -> &'static ::core::primitive::str {
                match self { #(Self::#idents => #labels,)* }
            }

            fn value(&self) -> &'static ::core::primitive::str {
                match self { #(Self::#idents => #values,)* }
            }

            fn description(&self) -> ::core::option::Option<&'static ::core::primitive::str> {
                match self { #(Self::#idents => #descriptions,)* }
            }

            fn from_value(value: &::core::primitive::str) -> ::core::option::Option<Self> {
                match value {
                    #(#values => ::core::option::Option::Some(Self::#idents),)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: TokenStream) -> String {
        let input: DeriveInput = syn::parse2(input).unwrap_or_else(|e| panic!("{e}"));
        match expand(&input) {
            Ok(t) => t.to_string(),
            Err(e) => e.into_compile_error().to_string(),
        }
    }

    #[test]
    fn mod_07_choice_texts() {
        let ok = "a".repeat(100);
        let out = run(quote!(
            enum E {
                #[choice(#ok, value = #ok, description = #ok)]
                A,
            }
        ));
        assert!(out.contains("Arity < 1 >"), "{out}");
        let over = "a".repeat(101);
        let out = run(quote!(
            enum E {
                #[choice(#over)]
                A,
            }
        ));
        assert!(
            out.contains("[MOD-07] label: limit between 1 and 100, actual 101"),
            "{out}"
        );
        let out = run(quote!(
            enum E {
                #[choice("")]
                A,
            }
        ));
        assert!(
            out.contains("[MOD-07] label: limit between 1 and 100, actual 0"),
            "{out}"
        );
        let out = run(quote!(
            enum E {
                #[choice("L", value = "")]
                A,
            }
        ));
        assert!(out.contains("[MOD-07] value"), "{out}");
        let out = run(quote!(
            enum E {
                #[choice("L", description = #over)]
                A,
            }
        ));
        assert!(
            out.contains("[MOD-07] description: limit at most 100, actual 101"),
            "{out}"
        );
    }

    #[test]
    fn mod_07_enum_shape() {
        assert!(
            run(quote!(
                enum E {}
            ))
            .contains("[MOD-07] a Choices enum needs at least one variant")
        );
        assert!(
            run(quote!(
                enum E {
                    #[choice("L")]
                    A(u8),
                }
            ))
            .contains("[MOD-07] Choices variant `A` has fields")
        );
        assert!(
            run(quote!(
                struct S;
            ))
            .contains("[MOD-07] derive(Choices) takes a unit enum")
        );
        assert!(
            run(quote!(
                enum E<T> {
                    #[choice("L")]
                    A,
                }
            ))
            .contains("[MOD-07] a Choices enum cannot be generic")
        );
    }

    #[test]
    fn duplicate_values_and_raw_idents() {
        let out = run(quote!(
            enum E {
                #[choice("A", value = "x")]
                A,
                #[choice("B", value = "x")]
                B,
            }
        ));
        assert!(
            out.contains("duplicate choice value `x`: every variant needs its own value"),
            "{out}"
        );
        assert!(!out.contains("[MOD-07]"));
        let out = run(quote!(
            enum E {
                #[choice("T")]
                r#type,
            }
        ));
        assert!(out.contains("r#type => \"type\""), "{out}");
    }

    #[test]
    fn malformed_attributes() {
        assert!(
            run(quote!(
                enum E {
                    A,
                }
            ))
            .contains("variant `A` needs #[choice(\\\"Label\\\")]")
        );
        assert!(
            run(quote!(
                enum E {
                    #[choice(x = "y")]
                    A,
                }
            ))
            .contains("expected string literal")
        );
        assert!(
            run(quote!(
                enum E {
                    #[choice("L", foo = "y")]
                    A,
                }
            ))
            .contains("unknown #[choice] argument `foo`")
        );
    }
}
