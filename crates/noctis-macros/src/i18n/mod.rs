//! `i18n!(default = "<code>", dir = "<path>")`: typed content strings from
//! YAML locale files (FR-42; AD-16; LOC-01, LOC-04).
//!
//! `args` parses the arguments, `load` lists and reads `dir` (the only
//! I/O), `model::compile` runs every check and resolves the rows, and
//! `emit` writes the items. Every error is a `syn::Error` spanned on the
//! `dir` (or `default`) literal, else the call site.

mod args;
mod emit;
mod load;
mod model;

pub(crate) use load::{root, shown};
pub(crate) use model::{COMMAND_KEY_WORDS, file_entries};

use proc_macro2::TokenStream;

use crate::common::combine;

/// Expands one invocation.
pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let args = match args::parse(input) {
        Ok(args) => args,
        Err(e) => return e.into_compile_error(),
    };
    let loaded = load::load(&args);
    let dir = shown(&args.dir);
    let span = args.span();
    let mut errors: Vec<syn::Error> = loaded
        .errors
        .iter()
        .map(|e| syn::Error::new(span, e))
        .collect();
    let default_file = format!("{}.yaml", args.default.as_str());
    if !errors.is_empty() && !loaded.files.iter().any(|(name, _)| *name == default_file) {
        return failed(errors, &loaded.read);
    }
    let model = model::compile(&loaded.files, args.default, dir);
    let model = match model {
        Ok(model) if errors.is_empty() => model,
        Ok(_) => {
            return failed(errors, &loaded.read);
        }
        Err(problems) => {
            let default_span = args.default_span.unwrap_or(span);
            errors.extend(problems.iter().map(|e| {
                let span = if e.starts_with("i18n!: the default locale") {
                    default_span
                } else {
                    span
                };
                syn::Error::new(span, e)
            }));
            return failed(errors, &loaded.read);
        }
    };
    emit::expand(&model, &args.dir, &loaded.read)
}

/// The errors plus the `include_bytes!` of every file read.
fn failed(errors: Vec<syn::Error>, read: &[std::path::PathBuf]) -> TokenStream {
    let includes = emit::includes_only(read);
    let errors = combine(errors)
        .err()
        .map(syn::Error::into_compile_error)
        .unwrap_or_default();
    quote::quote!(#errors #includes)
}
