//! The Theme file the image macros read (TW-02) and `theme!`.
//!
//! `tw!` and `view!` read `$CARGO_MANIFEST_DIR/noctis-theme.css`: a missing
//! file is an empty theme, an unreadable or invalid one is a compile error at
//! the call site. Every expansion that read a file emits
//! `const _: &[u8] = include_bytes!("<absolute path>");`, so editing the file
//! rebuilds the crate. `#[derive(Template)]` has no classes and reads no
//! theme.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use noctis_image::tw::Theme;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::LitStr;

/// The theme file name at the crate root.
const THEME_FILE: &str = "noctis-theme.css";

/// A crate's theme and the file it came from (`None`: no file, empty theme).
pub(crate) struct CrateTheme {
    pub(crate) theme: Theme,
    path: Option<PathBuf>,
}

impl CrateTheme {
    /// The `include_bytes!` item that makes a theme edit rebuild the crate;
    /// empty without a file.
    pub(crate) fn tracking(&self) -> TokenStream {
        self.path.as_deref().map(track).unwrap_or_default()
    }
}

/// `$CARGO_MANIFEST_DIR/noctis-theme.css`.
fn manifest_theme_path(span: Span) -> syn::Result<PathBuf> {
    let dir = std::env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
        syn::Error::new(
            span,
            "[TW-02] CARGO_MANIFEST_DIR is not set: build with cargo so the macros find noctis-theme.css",
        )
    })?;
    Ok(absolute(Path::new(&dir).join(THEME_FILE)))
}

/// The theme `tw!` and `view!` validate against.
pub(crate) fn crate_theme(span: Span) -> syn::Result<CrateTheme> {
    let path = manifest_theme_path(span)?;
    match read(&path, span)? {
        None => Ok(CrateTheme {
            theme: Theme::empty(),
            path: None,
        }),
        Some(source) => Ok(CrateTheme {
            theme: parse(&source, &path, span)?,
            path: Some(path),
        }),
    }
}

/// `theme!()` / `theme!("path")`.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let span = Span::call_site();
    let path = if input.is_empty() {
        manifest_theme_path(span)?
    } else {
        let lit: LitStr = syn::parse2(input).map_err(|err| {
            syn::Error::new(
                err.span(),
                "[TW-02] theme! takes no argument (the crate's noctis-theme.css) or one string \
                 literal path, relative to the calling file",
            )
        })?;
        relative_to_caller(&lit.value())
    };
    let Some(source) = read(&path, span)? else {
        let shown = path.display();
        return Err(syn::Error::new(
            span,
            if path.file_name().is_some_and(|name| name == THEME_FILE) {
                format!("[TW-02] no noctis-theme.css at {shown}")
            } else {
                format!("[TW-02] theme file not found at {shown}")
            },
        ));
    };
    parse(&source, &path, span)?;
    let lit = path_lit(&path);
    let tracking = track(&path);
    Ok(quote! {{
        #tracking
        ::noctis::__private::image::tw::Theme::__embedded(::core::include_str!(#lit))
    }})
}

/// `const _: &[u8] = include_bytes!("<path>");`.
fn track(path: &Path) -> TokenStream {
    let lit = path_lit(path);
    quote!(
        const _: &[u8] = ::core::include_bytes!(#lit);
    )
}

fn path_lit(path: &Path) -> LitStr {
    LitStr::new(&path.to_string_lossy(), Span::call_site())
}

/// `rel` resolved like `include_str!`: against the calling file's directory,
/// or the crate root when the compiler does not name a local file; an
/// absolute path is used as is.
fn relative_to_caller(rel: &str) -> PathBuf {
    let rel = Path::new(rel);
    if rel.is_absolute() {
        return absolute(rel.to_path_buf());
    }
    let base = proc_macro::Span::call_site()
        .local_file()
        .and_then(|file| file.parent().map(Path::to_path_buf))
        .or_else(|| std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from))
        .unwrap_or_default();
    absolute(base.join(rel))
}

/// `path` made absolute (against the compiler's working directory) with `.`
/// and `..` resolved lexically.
fn absolute(path: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().map_or(path.clone(), |cwd| cwd.join(&path))
    };
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The file's text, or `None` when it does not exist.
fn read(path: &Path, span: Span) -> syn::Result<Option<String>> {
    let shown = path.display();
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| syn::Error::new(span, format!("[TW-02] {shown} is not UTF-8"))),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(syn::Error::new(
            span,
            format!("[TW-02] cannot read {shown}: {err}"),
        )),
    }
}

fn parse(source: &str, path: &Path, span: Span) -> syn::Result<Theme> {
    Theme::parse(source)
        .map_err(|err| syn::Error::new(span, format!("{err} (in {})", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_resolves_dots_lexically() {
        let root = if cfg!(windows) { "C:\\a" } else { "/a" };
        let path = Path::new(root).join("b/./c/../../d.css");
        assert_eq!(absolute(path), Path::new(root).join("d.css"));
    }

    #[test]
    fn parse_errors_carry_the_path() {
        let err = parse(
            "@keyframes spin {}",
            Path::new("/x/noctis-theme.css"),
            Span::call_site(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.starts_with("[TW-02] 1:12: `spin`"), "{err}");
        assert!(err.ends_with(" (in /x/noctis-theme.css)"), "{err}");
    }

    #[test]
    fn tracking_includes_the_absolute_path() {
        let path = PathBuf::from("/x/noctis-theme.css");
        let expected = quote!(
            const _: &[u8] = ::core::include_bytes!("/x/noctis-theme.css");
        )
        .to_string();
        assert_eq!(track(&path).to_string(), expected);
        let with = CrateTheme {
            theme: Theme::empty(),
            path: Some(path),
        };
        assert_eq!(with.tracking().to_string(), expected);
        let without = CrateTheme {
            theme: Theme::empty(),
            path: None,
        };
        assert!(without.tracking().is_empty());
    }

    #[test]
    fn missing_file_reads_as_none() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-theme.css");
        assert!(read(&path, Span::call_site()).unwrap().is_none());
    }
}
