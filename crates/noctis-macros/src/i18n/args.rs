//! The `i18n!(default = "<code>", dir = "<path>")` arguments.

use noctis_rules::Locale;
use proc_macro2::{Span, TokenStream};
use syn::ext::IdentExt;
use syn::parse::{ParseStream, Parser};
use syn::{Expr, ExprLit, Ident, Lit, LitStr, Token};

use crate::common::combine;

/// The hidden flag of Noctis's own trybuild fixtures.
const FROM_SOURCE_FILE: &str = "__dir_from_source_file";

/// The checked arguments.
pub(crate) struct Args {
    /// The default locale (`en-US` when omitted).
    pub(crate) default: Locale,
    /// The `default` literal, when given.
    pub(crate) default_span: Option<Span>,
    /// The directory as written (`locales` when omitted).
    pub(crate) dir: String,
    /// The `dir` literal, when given.
    pub(crate) dir_span: Option<Span>,
    /// `__dir_from_source_file`: `dir` is relative to the invoking source
    /// file instead of the crate root.
    pub(crate) from_source_file: bool,
}

impl Args {
    /// The span of a file error: the `dir` literal, else the call site.
    pub(crate) fn span(&self) -> Span {
        self.dir_span.unwrap_or_else(Span::call_site)
    }
}

fn string_lit(expr: &Expr) -> Option<&LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Some(s),
        Expr::Group(g) => string_lit(&g.expr),
        _ => None,
    }
}

/// The raw `name = "literal"` pairs, every error collected.
#[derive(Default)]
struct Raw {
    default: Option<LitStr>,
    dir: Option<LitStr>,
    from_source_file: bool,
}

const SHAPE: &str = "`i18n!` takes `default = \"…\"` and `dir = \"…\"`";

fn parse_raw(input: ParseStream<'_>) -> syn::Result<Raw> {
    let mut raw = Raw::default();
    let mut errors = Vec::new();
    while !input.is_empty() {
        if !input.peek(Ident::peek_any) {
            return Err(input.error(SHAPE));
        }
        let name = Ident::parse_any(input)?;
        let key = name.to_string();
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let value: Expr = input.parse()?;
            let slot = match key.as_str() {
                "default" => Some(&mut raw.default),
                "dir" => Some(&mut raw.dir),
                _ => None,
            };
            match slot {
                None => errors.push(syn::Error::new_spanned(
                    &name,
                    format!("unknown `i18n!` argument `{key}`: expected `default` or `dir`"),
                )),
                Some(Some(_)) => {
                    errors.push(syn::Error::new_spanned(&name, format!("duplicate `{key}`")));
                }
                Some(slot) => match string_lit(&value) {
                    Some(lit) => *slot = Some(lit.clone()),
                    None => errors.push(syn::Error::new_spanned(
                        &value,
                        format!("`{key}` takes a string literal"),
                    )),
                },
            }
        } else if key == FROM_SOURCE_FILE {
            if raw.from_source_file {
                errors.push(syn::Error::new_spanned(&name, format!("duplicate `{key}`")));
            }
            raw.from_source_file = true;
        } else if key == "default" || key == "dir" {
            errors.push(syn::Error::new_spanned(
                &name,
                format!("`{key}` takes a string literal"),
            ));
        } else {
            errors.push(syn::Error::new_spanned(
                &name,
                format!("unknown `i18n!` argument `{key}`: expected `default` or `dir`"),
            ));
        }
        if input.is_empty() {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    combine(errors)?;
    Ok(raw)
}

/// Whether `dir` is absolute on any platform (`/x`, `\x`, `C:…`).
fn is_absolute(dir: &str) -> bool {
    let bytes = dir.as_bytes();
    dir.starts_with('/')
        || dir.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || std::path::Path::new(dir).is_absolute()
}

/// Parses and checks the arguments.
pub(crate) fn parse(input: TokenStream) -> syn::Result<Args> {
    let raw = parse_raw.parse2(input)?;
    let mut errors = Vec::new();
    let default = match &raw.default {
        None => Some(Locale::EnUs),
        Some(lit) => match lit.value().parse::<Locale>() {
            Ok(locale) => Some(locale),
            Err(e) => {
                errors.push(syn::Error::new(lit.span(), e.to_string()));
                None
            }
        },
    };
    let dir = raw
        .dir
        .as_ref()
        .map_or_else(|| "locales".to_owned(), LitStr::value);
    if let Some(lit) = &raw.dir
        && (dir.is_empty() || is_absolute(&dir))
    {
        errors.push(syn::Error::new(
            lit.span(),
            "`dir` must be a non-empty path relative to the crate root",
        ));
    }
    combine(errors)?;
    Ok(Args {
        default: default.unwrap_or(Locale::EnUs),
        default_span: raw.default.as_ref().map(LitStr::span),
        dir,
        dir_span: raw.dir.as_ref().map(LitStr::span),
        from_source_file: raw.from_source_file,
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    fn err(tokens: TokenStream) -> String {
        match parse(tokens) {
            Ok(_) => "ok".to_owned(),
            Err(e) => e
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Matrix 2.
    #[test]
    fn any_order_and_trailing_comma() {
        let args = parse(quote!(dir = "l", default = "tr",)).ok().unwrap();
        assert_eq!((args.default, args.dir.as_str()), (Locale::Tr, "l"));
        let args = parse(quote!()).ok().unwrap();
        assert_eq!((args.default, args.dir.as_str()), (Locale::EnUs, "locales"));
        assert!(!args.from_source_file);
        let args = parse(quote!(__dir_from_source_file, dir = "x"))
            .ok()
            .unwrap();
        assert!(args.from_source_file);
    }

    /// Matrix 3-7.
    #[test]
    fn argument_errors() {
        assert_eq!(
            err(quote!(path = "x")),
            "unknown `i18n!` argument `path`: expected `default` or `dir`"
        );
        assert_eq!(err(quote!(dir = "a", dir = "b")), "duplicate `dir`");
        assert_eq!(err(quote!("x")), SHAPE);
        assert_eq!(err(quote!(dir = 3)), "`dir` takes a string literal");
        assert_eq!(
            err(quote!(default = tr)),
            "`default` takes a string literal"
        );
        for dir in ["", "/abs", "C:\\x"] {
            assert_eq!(
                err(quote!(dir = #dir)),
                "`dir` must be a non-empty path relative to the crate root",
                "{dir}"
            );
        }
        assert_eq!(
            err(quote!(default = "en")),
            "[LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US`"
        );
        assert_eq!(
            err(quote!(default = "xx")),
            "[LOC-01] `xx` is not a Discord locale code"
        );
    }
}
