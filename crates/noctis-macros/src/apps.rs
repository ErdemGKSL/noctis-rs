//! The `apps = ["main", ..]` argument of `#[command]`, `#[user_command]`
//! and `#[message_command]` (FR-47): a non-empty list of string literals,
//! each a valid App id (`noctis_rules::app_id::check`, `[a-z0-9-]{1,32}`),
//! none repeated. Checked at expansion, so a bad id is a compile error.

use noctis_rules::app_id;
use proc_macro2::{TokenStream, TokenTree};
use quote::quote;
use syn::LitStr;

use crate::flags::string_list;

/// The error of a value that is not a bracketed list of string literals.
pub(crate) const NOT_A_LIST: &str =
    "`apps` takes a list of string literals: write `apps = [\"main\"]`";
/// The error of `apps = []`.
pub(crate) const EMPTY: &str =
    "`apps` needs at least one App id: write `apps = [\"main\"]` or remove it";

/// The App ids a registration is restricted to, in source order; empty
/// when the attribute has no `apps` argument (unrestricted).
#[derive(Clone, Default)]
pub(crate) struct Apps(Vec<LitStr>);

impl Apps {
    /// The id values, in source order.
    fn values(&self) -> impl Iterator<Item = String> + '_ {
        self.0.iter().map(LitStr::value)
    }

    /// Parses the whole `apps = [..]` argument `segment`.
    pub(crate) fn parse(segment: &[TokenTree]) -> syn::Result<Self> {
        let mut apps: Vec<LitStr> = Vec::new();
        for lit in string_list(segment, NOT_A_LIST, EMPTY)? {
            let value = lit.value();
            if let Err(reason) = app_id::check(&value) {
                return Err(syn::Error::new(
                    lit.span(),
                    format!("invalid App id `{value}` in `apps`: {reason}"),
                ));
            }
            if apps.iter().any(|a| a.value() == value) {
                return Err(syn::Error::new(
                    lit.span(),
                    format!("`{value}` is listed twice in `apps`"),
                ));
            }
            apps.push(lit);
        }
        Ok(Self(apps))
    }

    /// Whether no App id is listed (unrestricted).
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `&["a", "b"]` slice expression.
    pub(crate) fn slice(&self) -> TokenStream {
        let apps = &self.0;
        quote!(&[#(#apps),*])
    }
}

impl PartialEq for Apps {
    fn eq(&self, other: &Self) -> bool {
        self.values().eq(other.values())
    }
}

impl Eq for Apps {}

impl core::fmt::Debug for Apps {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.values()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags::tests::segment;

    fn error(tokens: TokenStream) -> String {
        match Apps::parse(&segment(tokens)) {
            Ok(_) => panic!("parsed"),
            Err(e) => e.to_string(),
        }
    }

    /// The `apps` rows of the Story 15.2 matrix on the parser.
    #[test]
    fn matrix() {
        let ok = Apps::parse(&segment(quote!(apps = ["main", "acme"]))).unwrap();
        assert_eq!(
            ok.slice().to_string(),
            quote!(&["main", "acme"]).to_string()
        );
        assert!(!ok.is_empty());
        assert!(Apps::default().is_empty());
        let max = "a".repeat(32);
        assert!(Apps::parse(&segment(quote!(apps = [#max]))).is_ok());

        assert_eq!(error(quote!(apps = [])), EMPTY);
        for bad in [
            quote!(apps = "main"),
            quote!(apps = [main]),
            quote!(apps = [1]),
            quote!(apps),
            quote!(apps("main")),
        ] {
            assert_eq!(error(bad), NOT_A_LIST);
        }
        let long = "a".repeat(33);
        assert_eq!(
            error(quote!(apps = [#long])),
            format!(
                "invalid App id `{long}` in `apps`: App id has 33 characters: shorten it to at \
                 most 32 matching [a-z0-9-]{{1,32}}"
            )
        );
        assert_eq!(
            error(quote!(apps = [""])),
            "invalid App id `` in `apps`: App id is empty: use 1-32 characters matching \
             [a-z0-9-]{1,32}, such as `my-bot`"
        );
        assert_eq!(
            error(quote!(apps = ["Main"])),
            "invalid App id `Main` in `apps`: App id has `M` at index 0: use only a-z, 0-9 and \
             - ([a-z0-9-]{1,32})"
        );
        assert_eq!(
            error(quote!(apps = ["bot-ü"])),
            "invalid App id `bot-ü` in `apps`: App id has `ü` at index 4: use only a-z, 0-9 and \
             - ([a-z0-9-]{1,32})"
        );
        assert_eq!(
            error(quote!(apps = ["a", "a"])),
            "`a` is listed twice in `apps`"
        );
    }
}
