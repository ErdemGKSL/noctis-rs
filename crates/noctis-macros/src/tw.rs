//! `tw!` and the compile-time half of TW-01: literal class strings are
//! validated with `noctis_image::tw::validate` against the crate's theme.

use noctis_image::tw::{
    Suggestion, Theme, ThemeNamespace, ThemeTokens, UnknownClass, Utility, Variant, classify,
    validate,
};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::LitStr;

use crate::common::combine;
use crate::theme::crate_theme;

/// One `[TW-01]` error per unknown class of `lit`, spanned on the literal.
pub(crate) fn check_literal(lit: &LitStr, theme: &Theme) -> Vec<syn::Error> {
    match validate(&lit.value(), theme) {
        Ok(()) => Vec::new(),
        Err(invalid) => invalid
            .classes()
            .iter()
            .map(|class| syn::Error::new(lit.span(), message(class, theme)))
            .collect(),
    }
}

/// `[TW-01] <class line>`, plus the theme declarations that would make a
/// theme utility (or custom variant) draw.
fn message(class: &UnknownClass, theme: &Theme) -> String {
    let mut missing = Vec::new();
    if !matches!(class.suggestion(), Some(Suggestion::Rename(_)))
        && let Some(parsed) = classify(class.class())
    {
        if let Some(Variant::Custom(token)) = parsed.variant
            && !theme.contains(ThemeNamespace::Breakpoint, token)
        {
            missing.push(format!(
                "`{}{token}`",
                ThemeNamespace::Breakpoint.css_prefix()
            ));
        }
        if let Utility::Theme { namespaces, token } = parsed.utility
            && !namespaces.iter().any(|ns| theme.contains(*ns, token))
            && let Some(first) = namespaces.first()
        {
            missing.push(format!("`{}{token}`", first.css_prefix()));
        }
    }
    if missing.is_empty() {
        format!("[TW-01] {class}")
    } else {
        format!(
            "[TW-01] {class}; or declare {} in noctis-theme.css",
            missing.join(" and ")
        )
    }
}

/// `tw!("classes")`: a checked `Classes` const expression.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let lit: LitStr = syn::parse2(input.clone()).map_err(|_| {
        let span = input
            .into_iter()
            .next()
            .map_or_else(Span::call_site, |t| t.span());
        syn::Error::new(
            span,
            "[TW-01] tw! expected a string literal of Tailwind classes, such as \
             `tw!(\"flex p-4\")`\n  = help: for classes chosen at run time use \
             `Classes::dynamic(..)` (never with user text)",
        )
    })?;
    let theme = crate_theme(lit.span())?;
    combine(check_literal(&lit, &theme.theme))?;
    let tracking = theme.tracking();
    Ok(quote! {{
        #tracking
        ::noctis::__private::image::Classes::__literal(#lit)
    }})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(classes: &str, theme: &Theme) -> Vec<String> {
        check_literal(&LitStr::new(classes, Span::call_site()), theme)
            .into_iter()
            .map(|e| e.to_string())
            .collect()
    }

    #[test]
    fn tw_01_valid_and_blank_literals() {
        for classes in ["flex p-4 sm:!p-2", "", "  \t"] {
            assert!(errors(classes, &Theme::empty()).is_empty(), "{classes:?}");
        }
    }

    #[test]
    fn tw_01_one_error_per_unknown_class() {
        assert_eq!(
            errors("flx bad flex bad", &Theme::empty()),
            [
                "[TW-01] unknown Tailwind class `flx`",
                "[TW-01] unknown Tailwind class `bad`"
            ]
        );
    }

    #[test]
    fn tw_01_v3_rename_and_theme_hint() {
        assert_eq!(
            errors("bg-gradient-to-r", &Theme::empty()),
            [
                "[TW-01] unknown Tailwind class `bg-gradient-to-r`; use `bg-linear-to-r` (Tailwind v4)"
            ]
        );
        assert_eq!(
            errors("bg-nope", &Theme::empty()),
            [
                "[TW-01] unknown Tailwind class `bg-nope`; or declare `--color-nope` in noctis-theme.css"
            ]
        );
        assert_eq!(
            errors("hover:flex", &Theme::empty()),
            [
                "[TW-01] unknown Tailwind class `hover:flex`; or declare `--breakpoint-hover` in noctis-theme.css"
            ]
        );
    }

    #[test]
    fn tw_01_theme_tokens_and_narrowing() {
        let theme = Theme::parse(
            "@theme { --color-brand: #000; --animate-rise: rise 1s; }\n@keyframes rise {}",
        )
        .unwrap();
        assert!(errors("bg-brand text-brand/50", &theme).is_empty());
        assert_eq!(
            errors("from-brand animate-rise", &theme),
            [
                "[TW-01] unknown Tailwind class `from-brand`",
                "[TW-01] unknown Tailwind class `animate-rise`"
            ]
        );
    }
}
