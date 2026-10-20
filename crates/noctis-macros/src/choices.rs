//! `#[derive(Choices)]`: a unit enum as a modal option set and a STRING
//! command option with choices, whose labels and values are checked at
//! expansion as OPT-09 (the limit MOD-07 and CV2-09 share) and descriptions
//! as MOD-07, with its variant count at the type level (`Count = Arity<N>`).

use noctis_rules::command::{
    Choice as RuleChoice, ChoiceValue, OptionDecl, OptionType, validate_options,
};
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

/// OPT-09 for one variant's label and value, from `validate_options` on a
/// synthetic STRING option, re-pathed to the variant and spanned on the
/// label, or on the value literal (the ident for a derived value).
fn check_opt_09(variant: &Variant, choice: &Choice, value: &str, errors: &mut Vec<syn::Error>) {
    let label = choice.label.value();
    let choices = [RuleChoice::new(&label, ChoiceValue::Str(value))];
    let decl = OptionDecl::new("choice", OptionType::String, "d").choices(&choices);
    let Err(violations) = validate_options(&[decl], &Limits::default()) else {
        return;
    };
    for mut v in violations.into_iter().filter(|v| v.rule == RuleId::OPT_09) {
        v.path = variant.ident.unraw().to_string();
        let span = if v.notes.iter().any(|n| n == "in the choice value") {
            choice
                .value
                .as_ref()
                .map_or_else(|| variant.ident.span(), LitStr::span)
        } else {
            choice.label.span()
        };
        let v = v.with_note("the same limit applies to modal and select options (MOD-07, CV2-09)");
        errors.push(syn::Error::new(span, v.to_string()));
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
    if let Some(d) = &choice.description {
        check_text(d, "description", 0, errors);
    }
    let value = choice
        .value
        .as_ref()
        .map_or_else(|| variant.ident.unraw().to_string(), LitStr::value);
    if let Some(v) = choice.value.as_ref().filter(|v| v.value().is_empty()) {
        check_text(v, "value", 1, errors);
    }
    check_opt_09(variant, &choice, &value, errors);
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
    let core = quote!(::noctis::__private::core);
    let rules = quote!(::noctis::__private::rules::command);
    let labels2 = checked.iter().map(|c| &c.label);
    Ok(quote! {
        impl #core::CommandOption for #name {
            const KIND: #rules::OptionType = #rules::OptionType::String;
            const CHOICES: &'static [#rules::Choice<'static>] = &[
                #(#rules::Choice::new(#labels2, #rules::ChoiceValue::Str(#values)),)*
            ];

            fn decode(
                value: &#core::OptionValue,
                input: &#core::CommandInput,
                option: &::core::primitive::str,
            ) -> ::core::result::Result<Self, #core::OptionError> {
                let _ = input;
                #core::__options::choice::<Self>(value, option)
            }
        }


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

    const SHARED: &str =
        "\n  = note: the same limit applies to modal and select options (MOD-07, CV2-09)";

    #[test]
    fn opt_09_choice_texts() {
        let ok = "a".repeat(100);
        let out = run(quote!(
            enum E {
                #[choice(#ok, value = #ok, description = #ok)]
                A,
            }
        ));
        assert!(out.contains("Arity < 1 >"), "{out}");
        let smile = "😀".repeat(50);
        assert!(
            !run(quote!(
                enum E {
                    #[choice(#smile)]
                    A,
                }
            ))
            .contains("compile_error")
        );
        let over = "a".repeat(101);
        let out = run(quote!(
            enum E {
                #[choice(#over)]
                A,
            }
        ));
        let expected = format!(
            "[OPT-09] A: limit between 1 and 100, actual 101\n  = note: in the choice name{SHARED}"
        );
        assert!(out.contains(&format!("{expected:?}")), "{out}");
        assert!(!out.contains("[MOD-07]"), "{out}");
        let out = run(quote!(
            enum E {
                #[choice("")]
                A,
            }
        ));
        assert!(
            out.contains("[OPT-09] A: limit between 1 and 100, actual 0"),
            "{out}"
        );
        let out = run(quote!(
            enum E {
                #[choice("L", value = #over)]
                A,
            }
        ));
        let expected = format!(
            "[OPT-09] A: limit at most 100, actual 101\n  = note: in the choice value{SHARED}"
        );
        assert!(out.contains(&format!("{expected:?}")), "{out}");
        let long = Ident::new(
            &format!("A{}", "a".repeat(100)),
            proc_macro2::Span::call_site(),
        );
        let out = run(quote!(
            enum E {
                #[choice("L")]
                #long,
            }
        ));
        assert!(
            out.contains(&format!("[OPT-09] {long}: limit at most 100, actual 101")),
            "{out}"
        );
        let out = run(quote!(
            enum E {
                #[choice("L", value = "")]
                A,
            }
        ));
        assert!(
            out.contains("[MOD-07] value: limit between 1 and 100, actual 0"),
            "{out}"
        );
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
    fn implements_command_option() {
        let out = run(quote!(
            enum Severity {
                #[choice("Low")]
                Low,
                #[choice("High", value = "hi")]
                High,
            }
        ));
        assert!(out.contains("CommandOption for Severity"), "{out}");
        assert!(out.contains("OptionType :: String ;"), "{out}");
        assert!(
            out.contains("Choice :: new (\"Low\" , :: noctis :: __private :: rules :: command :: ChoiceValue :: Str (\"Low\")) ,"),
            "{out}"
        );
        assert!(out.contains("ChoiceValue :: Str (\"hi\")"), "{out}");
        assert!(
            out.contains("__options :: choice :: < Self > (value , option)"),
            "{out}"
        );
        assert!(out.contains("Choices for Severity"), "{out}");
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
