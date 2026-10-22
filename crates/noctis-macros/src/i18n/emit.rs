//! The tokens `i18n!` expands to: `Strings`, `keys`, `catalog()`, the hidden
//! `__noctis_i18n` module and the LOC-04 warnings.

use std::path::PathBuf;

use noctis_rules::Locale;
use proc_macro2::{Literal, Span, TokenStream};
use quote::{format_ident, quote};
use syn::Ident;

use super::model::{Leaf, Model, Node, Params};

/// Rust's strict, reserved and edition keywords: such a segment is emitted
/// as a raw identifier (`r#type`).
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "unsized", "use",
    "virtual", "where", "while", "yield",
];

/// The identifier of a segment or parameter, raw for a keyword.
pub(crate) fn ident(name: &str) -> Ident {
    if KEYWORDS.contains(&name) {
        Ident::new_raw(name, Span::call_site())
    } else {
        Ident::new(name, Span::call_site())
    }
}

fn i18n() -> TokenStream {
    quote!(::noctis::__private::i18n)
}

fn locale_path(locale: Locale) -> TokenStream {
    let variant = format_ident!("{}", format!("{locale:?}"));
    quote!(::noctis::__private::rules::Locale::#variant)
}

/// `` `en-US`: "text" ``.
fn shown_text(default: Locale, value: &str) -> String {
    format!("`{default}`: \"{}\"", value.escape_debug())
}

/// The emitter's shared state.
struct Emit<'a> {
    model: &'a Model,
    /// `&'static [&'static str; K]`.
    row_ty: TokenStream,
    /// The node structs, in definition order.
    structs: Vec<TokenStream>,
}

impl Emit<'_> {
    /// The leaf accessor of `leaf` reading `self.__row[index]`.
    fn method(&self, name: &str, index: usize, leaf: &Leaf) -> TokenStream {
        let i18n = i18n();
        let method = ident(name);
        let doc = shown_text(self.model.default, &leaf.value);
        let index = Literal::usize_unsuffixed(index);
        let names = match &leaf.params {
            Params::Named(names) if names.is_empty() => {
                return quote! {
                    #[doc = #doc]
                    #[must_use]
                    pub fn #method(&self) -> &'static str {
                        self.__row[#index]
                    }
                };
            }
            other => other.names(),
        };
        let params: Vec<Ident> = match &leaf.params {
            Params::Named(_) => names.iter().map(|n| ident(n)).collect(),
            Params::Positional(n) => (0..*n).map(|i| format_ident!("arg{i}")).collect(),
        };
        let allow = (params.len() >= 7).then(|| quote!(#[allow(clippy::too_many_arguments)]));
        quote! {
            #[doc = #doc]
            #allow
            #[must_use]
            pub fn #method(&self, #(#params: impl ::core::fmt::Display),*) -> ::std::string::String {
                #i18n::format(
                    self.__row[#index],
                    &[#((#names, &#params as &dyn ::core::fmt::Display)),*],
                )
            }
        }
    }

    /// Defines the struct of `node` at `path` and returns its name.
    fn node(&mut self, path: &str, node: &Node) -> Ident {
        let name = format_ident!("N{}", self.structs.len());
        self.structs.push(TokenStream::new());
        let slot = self.structs.len() - 1;
        let row_ty = self.row_ty.clone();
        let (fields, inits) = self.children(path, node, &quote!());
        let methods = self.methods(node);
        let doc = format!("The strings under `{path}.*`.");
        self.structs[slot] = quote! {
            #[doc = #doc]
            #[derive(::core::clone::Clone, ::core::marker::Copy, ::core::fmt::Debug)]
            pub struct #name {
                #(#fields,)*
                #[doc(hidden)]
                pub __row: #row_ty,
            }

            impl #name {
                #[doc(hidden)]
                pub const fn __new(row: #row_ty) -> Self {
                    Self { #(#inits,)* __row: row }
                }

                #(#methods)*
            }
        };
        name
    }

    /// The fields and initializers of `node`'s child maps; `prefix` is the
    /// path of `__noctis_i18n` from where they are written.
    fn children(
        &mut self,
        path: &str,
        node: &Node,
        prefix: &TokenStream,
    ) -> (Vec<TokenStream>, Vec<TokenStream>) {
        let mut fields = Vec::new();
        let mut inits = Vec::new();
        for (segment, child) in &node.children {
            let full = if path.is_empty() {
                segment.clone()
            } else {
                format!("{path}.{segment}")
            };
            let ty = self.node(&full, child);
            let field = ident(segment);
            let doc = format!("`{full}.*`");
            fields.push(quote!(#[doc = #doc] pub #field: #prefix #ty));
            inits.push(quote!(#field: #prefix #ty::__new(row)));
        }
        (fields, inits)
    }

    fn methods(&self, node: &Node) -> Vec<TokenStream> {
        node.leaves
            .iter()
            .map(|(segment, &index)| self.method(segment, index, &self.model.content[index]))
            .collect()
    }
}

/// The docs of `Strings`, `keys` and `catalog`.
fn docs(model: &Model, dir: &str) -> (String, String, String) {
    let strings = format!(
        "The typed strings of `{dir}/`: `Strings::of(locale)` picks the locale's file, its sibling's, else `{}.yaml`; fields are the root maps, methods the root keys.",
        model.default
    );
    let keys = format!(
        "The `MessageKey` of every content key of `{dir}/{}.yaml` (dots become `_`, uppercased).",
        model.default
    );
    let catalog = format!(
        "The catalog of `{dir}/` for `AppBuilder::locales`: every key of every file, the `noctis.*` overrides included."
    );
    (strings, keys, catalog)
}

/// Every generated item of a checked model.
pub(crate) fn expand(model: &Model, dir: &str, read: &[PathBuf]) -> TokenStream {
    let i18n = i18n();
    let rules = quote!(::noctis::__private::rules);
    let k = Literal::usize_unsuffixed(model.content.len());
    let row_ty = quote!(&'static [&'static str; #k]);
    let mut emit = Emit {
        model,
        row_ty: row_ty.clone(),
        structs: Vec::new(),
    };
    // The root's children are defined inside `__noctis_i18n`, so their
    // paths there are relative to it; `Strings` names them through it.
    let (fields, inits) = emit.children("", &model.tree, &quote!(__noctis_i18n::));
    let methods = emit.methods(&model.tree);
    let structs = std::mem::take(&mut emit.structs);

    let rows = model.rows.iter().map(|row| quote!([#(#row),*]));
    let f = Literal::usize_unsuffixed(model.rows.len());
    let arms = Locale::ALL.iter().zip(&model.row_of).map(|(locale, row)| {
        let path = locale_path(*locale);
        let row = Literal::usize_unsuffixed(*row);
        quote!(#path => &ROWS[#row])
    });
    let files = model.files.iter().map(|file| {
        let locale = locale_path(file.locale);
        let entries = file.entries.iter().map(|(k, v)| quote!((#k, #v)));
        quote!((#locale, &[#(#entries),*]))
    });
    let placeholders = model
        .placeholders
        .iter()
        .map(|(key, names)| quote!((#key, &[#(#names),*])));
    let default = locale_path(model.default);
    let consts = model.content.iter().map(|leaf| {
        let name = Ident::new(&leaf.const_name, Span::call_site());
        let key = &leaf.key;
        let doc = format!("`{key}` ({})", shown_text(model.default, &leaf.value));
        quote! {
            #[doc = #doc]
            pub const #name: #rules::MessageKey = #rules::MessageKey::new(#key);
        }
    });
    let includes = read.iter().map(|path| {
        let path = path.to_string_lossy().into_owned();
        quote!(
            const _: &[u8] = ::core::include_bytes!(#path);
        )
    });
    let warnings = model.warnings.iter().map(|msg| warning(msg));
    let (strings_doc, keys_doc, catalog_doc) = docs(model, dir);
    quote! {
        #[doc = #strings_doc]
        #[derive(::core::clone::Clone, ::core::marker::Copy, ::core::fmt::Debug)]
        pub struct Strings {
            #(#fields,)*
            #[allow(dead_code)]
            __row: #row_ty,
        }

        impl Strings {
            /// The strings of `locale`: its file, else its sibling's file,
            /// else the default locale's; a key missing in the chosen file
            /// falls back the same way.
            #[must_use]
            pub fn of(locale: #rules::Locale) -> Self {
                let row = __noctis_i18n::row(locale);
                Self { #(#inits,)* __row: row }
            }

            #(#methods)*
        }

        #[doc = #keys_doc]
        pub mod keys {
            #(#consts)*
        }

        #[doc = #catalog_doc]
        #[must_use]
        pub fn catalog() -> #i18n::Catalog {
            __noctis_i18n::CATALOG
        }

        #[doc(hidden)]
        #[allow(missing_docs, clippy::all, clippy::pedantic)]
        pub mod __noctis_i18n {
            #(#structs)*

            /// The resolved rows, one per file.
            pub static ROWS: [[&str; #k]; #f] = [#(#rows),*];

            /// The row of `locale`: its file's, its sibling's, else the default's.
            pub fn row(locale: #rules::Locale) -> #row_ty {
                match locale {
                    #(#arms,)*
                }
            }

            /// Every file's entries.
            const FILES: &[(#rules::Locale, &[(&str, &str)])] = &[#(#files),*];

            /// The placeholder names of every content and `noctis.*` key.
            const PLACEHOLDERS: &[(&str, &[&str])] = &[#(#placeholders),*];

            /// The catalog.
            pub static CATALOG: #i18n::Catalog = #i18n::Catalog::__new(
                ::core::concat!(::core::env!("CARGO_PKG_NAME"), "/", #dir),
                #default,
                FILES,
                PLACEHOLDERS,
            );

            #(#includes)*
        }

        #(#warnings)*
    }
}

/// A `deprecated`-lint build warning spanned on the call site.
pub(crate) fn warning(message: &str) -> TokenStream {
    quote! {
        const _: () = {
            #[deprecated(note = #message)]
            const W: () = ();
            W
        };
    }
}

/// The `include_bytes!` consts of a failed expansion, so edits rebuild.
pub(crate) fn includes_only(read: &[PathBuf]) -> TokenStream {
    let includes = read.iter().map(|path| {
        let path = path.to_string_lossy().into_owned();
        quote!(
            const _: &[u8] = ::core::include_bytes!(#path);
        )
    });
    quote!(#(#includes)*)
}

#[cfg(test)]
mod tests {
    use super::super::model::compile;
    use super::*;

    fn tokens(list: &[(&str, &str)]) -> String {
        let files: Vec<(String, String)> = list
            .iter()
            .map(|(n, t)| ((*n).to_owned(), (*t).to_owned()))
            .collect();
        let model = compile(&files, Locale::EnUs, "locales").unwrap_or_else(|e| panic!("{e:?}"));
        expand(&model, "locales", &[PathBuf::from("/x/en-US.yaml")]).to_string()
    }

    #[test]
    fn reserved_families_get_no_accessors_or_consts() {
        let t = tokens(&[(
            "en-US.yaml",
            "cmd: {setup: {description: d}}\ncmdword: {setup: s}\nctxcmd: {show: {name: n}}\nnoctis: {expired: e}\ncommon: {refresh: Refresh}",
        )]);
        assert!(t.contains("pub const COMMON_REFRESH"), "{t}");
        for absent in [
            "CMD_",
            "CMDWORD_",
            "CTXCMD_",
            "NOCTIS_",
            "fn setup",
            "fn expired",
            "pub cmd",
        ] {
            assert!(!t.contains(absent), "{absent}: {t}");
        }
        assert!(t.contains("\"noctis.expired\" , \"e\""), "{t}");
        assert!(t.contains("include_bytes ! (\"/x/en-US.yaml\")"), "{t}");
        assert!(t.contains("CARGO_PKG_NAME"), "{t}");
    }

    #[test]
    fn keywords_are_raw_identifiers() {
        let t = tokens(&[("en-US.yaml", "type: x\nprofile: {match: \"{type}\"}")]);
        assert!(t.contains("pub fn r#type (& self) -> & 'static str"), "{t}");
        assert!(t.contains("pub profile : __noctis_i18n :: N0"), "{t}");
        assert!(
            t.contains("pub fn r#match (& self , r#type : impl :: core :: fmt :: Display)"),
            "{t}"
        );
        assert!(t.contains("pub const TYPE"), "{t}");
        assert!(t.contains("pub const PROFILE_MATCH"), "{t}");
    }

    #[test]
    fn parameters_and_rows() {
        let t = tokens(&[
            (
                "en-US.yaml",
                "greet: \"Hi {name}, {name}! You have {n}\"\np: \"{1} of {0}\"",
            ),
            ("tr.yaml", "greet: \"Selam {name} {n}\""),
        ]);
        assert!(t.contains("pub fn greet (& self , name : impl :: core :: fmt :: Display , n : impl :: core :: fmt :: Display) -> :: std :: string :: String"), "{t}");
        assert!(
            t.contains("(\"name\" , & name as & dyn :: core :: fmt :: Display)"),
            "{t}"
        );
        assert!(t.contains("pub fn p (& self , arg0 : impl :: core :: fmt :: Display , arg1 : impl :: core :: fmt :: Display)"), "{t}");
        assert!(t.contains("(\"0\" , & arg0 as & dyn"), "{t}");
        assert!(t.contains("pub static ROWS : [[& str ; 2] ; 2]"), "{t}");
        assert!(t.contains("Locale :: Tr => & ROWS [1]"), "{t}");
        assert!(t.contains("Locale :: De => & ROWS [0]"), "{t}");
    }

    #[test]
    fn loc_04_warning_tokens() {
        let t = tokens(&[("en-US.yaml", "a: A\nb: B"), ("tr.yaml", "")]);
        assert!(t.contains("# [deprecated (note = \"[LOC-04] `locales/tr.yaml` lacks 2 key(s) of the default locale `en-US.yaml`: `a`, `b`; they fall back at runtime\")] const W : () = () ; W"), "{t}");
        let t = tokens(&[("en-US.yaml", "a: A"), ("tr.yaml", "a: B")]);
        assert!(!t.contains("deprecated"), "{t}");
    }

    #[test]
    fn leaf_docs_show_the_default_text() {
        let t = tokens(&[("en-US.yaml", "q: 'say \"hi\"'")]);
        assert!(t.contains(r#""`en-US`: \"say \\\"hi\\\"\"""#), "{t}");
        assert!(t.contains(r#""`q` (`en-US`: \"say \\\"hi\\\"\")""#), "{t}");
    }
}
