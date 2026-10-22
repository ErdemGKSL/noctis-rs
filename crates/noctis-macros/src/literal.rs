//! `label!`, `placeholder!`, `content!`, `alt!` and `rgb!`: one literal,
//! measured at expansion with the `noctis-rules` function the runtime
//! `try_new` / `TryFrom` path calls (CV2-08, CV2-09, MSG-01, ATT-06, CV2-13).

use noctis_rules::attachment::validate_attachment_description;
use noctis_rules::component::{
    validate_accent_color, validate_button_label, validate_select_placeholder,
};
use noctis_rules::message::validate_content;
use noctis_rules::{Actual, Limits, RuleId, Violation};
use proc_macro2::{Literal, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{LitInt, LitStr};

use crate::common::{combine, error};

/// A text literal macro: its rule, check and target type.
pub(crate) struct TextMacro {
    name: &'static str,
    rule: RuleId,
    ty: &'static str,
    check: fn(&str, &Limits) -> Vec<Violation>,
}

/// `label!`: a button label (CV2-08).
pub(crate) const LABEL: TextMacro = TextMacro {
    name: "label",
    rule: RuleId::CV2_08,
    ty: "Label",
    check: validate_button_label,
};

/// `placeholder!`: a select placeholder (CV2-09).
pub(crate) const PLACEHOLDER: TextMacro = TextMacro {
    name: "placeholder",
    rule: RuleId::CV2_09,
    ty: "Placeholder",
    check: validate_select_placeholder,
};

/// `content!`: legacy message content (MSG-01).
pub(crate) const CONTENT: TextMacro = TextMacro {
    name: "content",
    rule: RuleId::MSG_01,
    ty: "Content",
    check: validate_content,
};

/// `alt!`: alt text (ATT-06).
pub(crate) const ALT: TextMacro = TextMacro {
    name: "alt",
    rule: RuleId::ATT_06,
    ty: "AltText",
    check: validate_attachment_description,
};

/// Spans a misuse error on the arguments, or on the call site when there are
/// none.
fn misuse(input: &TokenStream, rule: RuleId, what: String, help: String) -> syn::Error {
    if input.is_empty() {
        syn::Error::new(
            Span::call_site(),
            format!("[{rule}] {what}\n  = help: {help}"),
        )
    } else {
        error(input, rule, what, help)
    }
}

/// Expands a text literal macro into `<Type>::__from_checked(lit)`.
pub(crate) fn expand_text(m: &TextMacro, input: &TokenStream) -> syn::Result<TokenStream> {
    let lit: LitStr = syn::parse2(input.clone()).map_err(|_| {
        misuse(
            input,
            m.rule,
            format!("{}! takes one string literal", m.name),
            format!(
                "for text known only at runtime use `{}::try_new(text, &limits)`",
                m.ty
            ),
        )
    })?;
    let errors = (m.check)(&lit.value(), &Limits::default())
        .into_iter()
        .map(|v| syn::Error::new(lit.span(), v.to_string()))
        .collect();
    combine(errors)?;
    let ty = syn::Ident::new(m.ty, Span::call_site());
    Ok(quote!(::noctis::__private::ui::#ty::__from_checked(#lit)))
}

/// The CV2-13 violation of an integer literal that does not fit `u32`: the
/// bound of [`validate_accent_color`] with the literal's text as the actual.
pub(crate) fn accent_overflow(text: String) -> Violation {
    let mut v = validate_accent_color(u32::MAX, &Limits::default())
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            Violation::new(
                RuleId::CV2_13,
                "",
                noctis_rules::Bound::Describe("a colour that fits 24 bits"),
                Actual::Count(0),
            )
        });
    v.actual = Actual::Text(text);
    v
}

/// Checks an integer colour literal; on success returns the value.
pub(crate) fn check_accent(lit: &LitInt) -> Result<u32, Box<Violation>> {
    match lit.base10_parse::<u32>() {
        Ok(value) => match validate_accent_color(value, &Limits::default())
            .into_iter()
            .next()
        {
            None => Ok(value),
            Some(v) => Err(Box::new(v)),
        },
        Err(_) => Err(Box::new(accent_overflow(lit.to_token_stream().to_string()))),
    }
}

/// `rgb!(0xRRGGBB)` into `Rgb::__from_checked(0x…u32)`.
pub(crate) fn expand_rgb(input: &TokenStream) -> syn::Result<TokenStream> {
    let lit: LitInt = syn::parse2(input.clone())
        .ok()
        .filter(|l: &LitInt| !l.base10_digits().starts_with('-'))
        .ok_or_else(|| {
            misuse(
                input,
                RuleId::CV2_13,
                "rgb! takes one non-negative integer literal".to_owned(),
                "for a colour known only at runtime use `Rgb::try_from(u32)`".to_owned(),
            )
        })?;
    let value = check_accent(&lit).map_err(|v| syn::Error::new(lit.span(), v.to_string()))?;
    let value = u32_literal(value);
    Ok(quote!(::noctis::__private::ui::Rgb::__from_checked(#value)))
}

/// The checked colour as a `u32` literal with a macro (call-site) span, so
/// lints about how a literal is written look only at the author's tokens.
pub(crate) fn u32_literal(value: u32) -> TokenStream {
    Literal::u32_suffixed(value).to_token_stream()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(m: &TextMacro, input: &TokenStream) -> String {
        match expand_text(m, input) {
            Ok(t) => t.to_string(),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn cv2_08_label_bounds() {
        let ok = "a".repeat(80);
        assert!(text(&LABEL, &quote!(#ok)).contains("__from_checked"));
        let over = "a".repeat(81);
        assert_eq!(
            text(&LABEL, &quote!(#over)),
            "[CV2-08] limit at most 80, actual 81"
        );
        let emoji = "😀".repeat(41);
        assert_eq!(
            text(&LABEL, &quote!(#emoji)),
            "[CV2-08] limit at most 80, actual 82"
        );
        assert!(text(&LABEL, &quote!("")).contains("__from_checked"));
        for bad in [
            quote!(name),
            quote!(1),
            quote!(b"x"),
            quote!(),
            quote!("a", "b"),
        ] {
            assert!(
                text(&LABEL, &bad).starts_with("[CV2-08] label! takes one string literal"),
                "misuse"
            );
        }
    }

    #[test]
    fn msg_01_content_and_cv2_09_placeholder() {
        let over = "a".repeat(2001);
        assert_eq!(
            text(&CONTENT, &quote!(#over)),
            "[MSG-01] content: limit at most 2000, actual 2001"
        );
        let over = "a".repeat(151);
        assert_eq!(
            text(&PLACEHOLDER, &quote!(#over)),
            "[CV2-09] limit at most 150, actual 151"
        );
    }

    #[test]
    fn att_06_alt_bounds() {
        for ok in ["a".repeat(1024), "😀".repeat(512), String::new()] {
            assert!(text(&ALT, &quote!(#ok)).contains("AltText :: __from_checked"));
        }
        let over = "a".repeat(1025);
        assert_eq!(
            text(&ALT, &quote!(#over)),
            "[ATT-06] limit at most 1024, actual 1025"
        );
        let emoji = "😀".repeat(513);
        assert_eq!(
            text(&ALT, &quote!(#emoji)),
            "[ATT-06] limit at most 1024, actual 1026"
        );
        for bad in [quote!(x), quote!()] {
            let err = text(&ALT, &bad);
            assert!(
                err.starts_with("[ATT-06] alt! takes one string literal"),
                "{err}"
            );
            assert!(err.contains("AltText::try_new"), "{err}");
        }
    }

    #[test]
    fn cv2_13_rgb() {
        let ok = |t: TokenStream| expand_rgb(&t).map(|t| t.to_string());
        assert_eq!(
            ok(quote!(0xFFFFFF)).ok().as_deref(),
            Some(":: noctis :: __private :: ui :: Rgb :: __from_checked (16777215u32)")
        );
        assert!(ok(quote!(16777215u32)).is_ok());
        assert!(ok(quote!(0x5865_F2)).is_ok());
        let err = |t: TokenStream| expand_rgb(&t).map(|_| ()).unwrap_err().to_string();
        assert_eq!(
            err(quote!(0x1000000)),
            "[CV2-13] limit a colour from 0x000000 to 0xFFFFFF, actual `0x1000000`"
        );
        assert_eq!(
            err(quote!(99999999999999999999)),
            "[CV2-13] limit a colour from 0x000000 to 0xFFFFFF, actual `99999999999999999999`"
        );
        for bad in [quote!(-1), quote!(1.0), quote!("x"), quote!(v), quote!()] {
            assert!(err(bad).starts_with("[CV2-13] rgb! takes one non-negative integer literal"));
        }
    }
}
