//! The `flags = ["beta", ..]` argument every kind attribute shares (FR-73,
//! AD-7): a non-empty list of string literals, each a valid Boot Flag
//! (`noctis_rules::boot_flag::check`), none repeated. Checked at expansion,
//! so a bad flag is a compile error (maintainer decisions §4).

use noctis_rules::boot_flag::{self, BootFlagError};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::quote;
use syn::LitStr;

use crate::command::{segments, span_of};

/// The error of a value that is not a bracketed list of string literals.
pub(crate) const NOT_A_LIST: &str =
    "`flags` takes a list of string literals: write `flags = [\"beta\"]`";
/// The error of `flags = []`.
pub(crate) const EMPTY: &str =
    "`flags` needs at least one Boot Flag: write `flags = [\"beta\"]` or remove it";

/// The Boot Flags a registration declares, in source order; empty when the
/// attribute has no `flags` argument.
#[derive(Clone, Default)]
pub(crate) struct Flags(Vec<LitStr>);

impl Flags {
    /// The flag values, in source order.
    fn values(&self) -> impl Iterator<Item = String> + '_ {
        self.0.iter().map(LitStr::value)
    }
}

impl PartialEq for Flags {
    fn eq(&self, other: &Self) -> bool {
        self.values().eq(other.values())
    }
}

impl Eq for Flags {}

impl core::fmt::Debug for Flags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.values()).finish()
    }
}

impl Flags {
    /// Parses the whole `flags = [..]` argument `segment`.
    pub(crate) fn parse(segment: &[TokenTree]) -> syn::Result<Self> {
        let mut flags: Vec<LitStr> = Vec::new();
        for lit in string_list(segment, NOT_A_LIST, EMPTY)? {
            let value = lit.value();
            let reason = match boot_flag::check(&value) {
                Err(reason) => Some(reason),
                Ok(()) if flags.iter().any(|f| f.value() == value) => {
                    Some(BootFlagError::Duplicate)
                }
                Ok(()) => None,
            };
            if let Some(reason) = reason {
                return Err(syn::Error::new(
                    lit.span(),
                    format!("invalid Boot Flag `{value}` in `flags`: {reason}"),
                ));
            }
            flags.push(lit);
        }
        Ok(Self(flags))
    }

    /// Whether no flag is declared.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `&["a", "b"]` slice expression.
    pub(crate) fn slice(&self) -> TokenStream {
        let flags = &self.0;
        quote!(&[#(#flags),*])
    }
}

/// The string literals of a `name = ["a", ..]` argument `segment`, in
/// source order: `not_a_list` when the value is not a bracketed list of
/// string literals, `empty` for `[]` (shared by `flags` and `apps`).
pub(crate) fn string_list(
    segment: &[TokenTree],
    not_a_list: &str,
    empty: &str,
) -> syn::Result<Vec<LitStr>> {
    let span = span_of(segment);
    let group = match segment {
        [_, TokenTree::Punct(eq), TokenTree::Group(group)]
            if eq.as_char() == '=' && group.delimiter() == Delimiter::Bracket =>
        {
            group
        }
        _ => return Err(syn::Error::new(span, not_a_list)),
    };
    let items = segments(group.stream());
    if items.iter().all(Vec::is_empty) {
        return Err(syn::Error::new(group.span(), empty));
    }
    items
        .iter()
        .map(|item| {
            let tokens: TokenStream = item.iter().cloned().collect();
            syn::parse2::<LitStr>(tokens).map_err(|_| syn::Error::new(span_of(item), not_a_list))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The `flags = ..` segment of `tokens`.
    pub(crate) fn segment(tokens: TokenStream) -> Vec<TokenTree> {
        tokens.into_iter().collect()
    }

    fn error(tokens: TokenStream) -> String {
        match Flags::parse(&segment(tokens)) {
            Ok(_) => panic!("parsed"),
            Err(e) => e.to_string(),
        }
    }

    /// M1-M8 on the shared parser.
    #[test]
    fn matrix() {
        let ok = Flags::parse(&segment(quote!(flags = ["beta", "eco"]))).unwrap();
        assert_eq!(ok.slice().to_string(), quote!(&["beta", "eco"]).to_string());
        assert!(!ok.is_empty());
        assert!(Flags::default().is_empty());
        let trailing = Flags::parse(&segment(quote!(flags = ["beta",]))).unwrap();
        assert_eq!(trailing.0.len(), 1);

        assert_eq!(error(quote!(flags = [])), EMPTY);
        for bad in [
            quote!(flags = "beta"),
            quote!(flags = [beta]),
            quote!(flags = [1]),
            quote!(flags),
            quote!(flags("beta")),
            quote!(flags = ("beta")),
        ] {
            assert_eq!(error(bad), NOT_A_LIST);
        }
        assert_eq!(
            error(quote!(flags = ["Beta"])),
            "invalid Boot Flag `Beta` in `flags`: `B` at index 0 is not allowed in a Boot Flag: \
             use only a-z, 0-9, `_` and `-`"
        );
        assert_eq!(
            error(quote!(flags = ["all"])),
            "invalid Boot Flag `all` in `flags`: `all` is reserved: write `all` or `none` alone, \
             never in a list or as a declared flag"
        );
        assert_eq!(
            error(quote!(flags = ["a", "a"])),
            "invalid Boot Flag `a` in `flags`: a Boot Flag is given twice: keep one"
        );
        let long = "a".repeat(33);
        assert!(
            error(quote!(flags = [#long]))
                .contains("the Boot Flag has 33 characters: shorten it to at most 32")
        );
    }
}
