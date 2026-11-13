//! `#[derive(AppEmojis)]`: a fieldless enum as a declared set of
//! application emojis (FR-82, AD-33).
//!
//! Each variant is one emoji named in `snake_case` (`#[emoji(name = "..")]`
//! overrides it) whose image is `<dir>/<name>.png|gif|jpg`, `dir` relative
//! to `CARGO_MANIFEST_DIR` (`#[emoji(file = "..")]`, relative to `dir`,
//! overrides it). The file is read, sniffed and measured at expansion
//! (EMO-04, EMO-02) and embedded with `include_bytes!`, so an edit
//! rebuilds the crate. `#[emoji(white)]` adds `<name>_white` from
//! `<stem>_white.<ext>`; `#[emoji(render)]` reads no file and draws the
//! image at Publish with the set's `#[emojis(art = path)]` function.

use std::path::{Path, PathBuf};

use noctis_rules::emoji::{EmojiFormat, validate_emoji_image, validate_emoji_name};
use noctis_rules::{Limits, RuleId};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Path as SynPath, Variant};

use crate::common::{combine, error, plain};

/// The default `dir`.
const DEFAULT_DIR: &str = "assets/emoji";
/// The shape of the attributes, for errors.
const HELP: &str = "an AppEmojis set is a fieldless enum: #[emojis(dir = \"assets/emoji\", art = path::to::fn)] on the enum, #[emoji(name = \"..\", file = \"..\", white, render)] on a variant";

/// The enum's `#[emojis(..)]`.
struct SetArgs {
    dir: String,
    art: Option<SynPath>,
    from_source_file: bool,
}

/// One variant's `#[emoji(..)]`.
#[derive(Default)]
struct VariantArgs {
    name: Option<LitStr>,
    file: Option<LitStr>,
    white: bool,
    render: Option<Span>,
}

fn parse_set(input: &DeriveInput) -> syn::Result<SetArgs> {
    let mut args = SetArgs {
        dir: DEFAULT_DIR.to_owned(),
        art: None,
        from_source_file: false,
    };
    let mut seen_dir = false;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("emojis")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("dir") {
                if std::mem::replace(&mut seen_dir, true) {
                    return Err(meta.error("`dir` is given twice"));
                }
                args.dir = meta.value()?.parse::<LitStr>()?.value();
                Ok(())
            } else if meta.path.is_ident("art") {
                if args.art.is_some() {
                    return Err(meta.error("`art` is given twice"));
                }
                args.art = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("__dir_from_source_file") {
                args.from_source_file = true;
                Ok(())
            } else {
                Err(meta.error(format!(
                    "unknown #[emojis] argument; expected `dir` or `art`\n  = help: {HELP}"
                )))
            }
        })?;
    }
    Ok(args)
}

fn parse_variant(variant: &Variant) -> syn::Result<VariantArgs> {
    let mut args = VariantArgs::default();
    for attr in variant.attrs.iter().filter(|a| a.path().is_ident("emoji")) {
        attr.parse_nested_meta(|meta| {
            let twice = || meta.error("this argument is given twice");
            if meta.path.is_ident("name") {
                if args.name.is_some() {
                    return Err(twice());
                }
                args.name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("file") {
                if args.file.is_some() {
                    return Err(twice());
                }
                args.file = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("white") {
                if std::mem::replace(&mut args.white, true) {
                    return Err(twice());
                }
            } else if meta.path.is_ident("render") {
                if args.render.replace(meta.path.span()).is_some() {
                    return Err(twice());
                }
            } else {
                return Err(meta.error(format!(
                    "unknown #[emoji] argument; expected `name`, `file`, `white` or `render`\n  = help: {HELP}"
                )));
            }
            Ok(())
        })?;
    }
    if let (Some(span), Some(file)) = (args.render, &args.file) {
        let mut e = syn::Error::new(span, "`render` draws the image: remove `file`");
        e.combine(syn::Error::new_spanned(file, "`file` is set here"));
        return Err(e);
    }
    Ok(args)
}

/// `CamelCase` to `snake_case`: `StOnline` → `st_online`, `HTTPCode` →
/// `http_code`, `Check2` → `check2`.
pub(crate) fn snake_case(ident: &str) -> String {
    let chars: Vec<char> = ident.chars().collect();
    let mut out = String::with_capacity(ident.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev != '_'
                && (prev.is_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_uppercase() && next_lower))
            {
                out.push('_');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// The directory relative paths start from.
fn root(from_source_file: bool) -> Result<PathBuf, String> {
    if from_source_file {
        proc_macro::Span::call_site()
            .local_file()
            .and_then(|file| file.parent().map(Path::to_path_buf))
            .ok_or_else(|| "the invoking source file is unknown".to_owned())
    } else {
        std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "`CARGO_MANIFEST_DIR` is not set: build with cargo".to_owned())
    }
}

/// One file read and checked.
struct Image {
    path: PathBuf,
    format: EmojiFormat,
}

impl Image {
    fn tokens(&self) -> TokenStream {
        let path = LitStr::new(&self.path.to_string_lossy(), Span::call_site());
        let format = match self.format {
            EmojiFormat::Png => quote!(Png),
            EmojiFormat::Jpeg => quote!(Jpeg),
            EmojiFormat::Gif => quote!(Gif),
        };
        quote!(::noctis::__private::core::emoji::EmojiImage::File {
            bytes: ::core::include_bytes!(#path),
            format: ::noctis::__private::core::emoji::EmojiFormat::#format,
        })
    }
}

/// Where a variant's files are looked up.
struct Files<'a> {
    base: &'a Path,
    /// `dir` as written, for messages.
    shown: &'a str,
    limits: &'a Limits,
}

impl Files<'_> {
    /// Reads `<base>/<rel>` and checks it as the image of emoji `name`.
    fn read(&self, at: &Variant, name: &str, rel: &str) -> syn::Result<Image> {
        let path = self.base.join(rel);
        let shown = shown(self.shown, rel);
        let bytes = std::fs::read(&path).map_err(|e| {
            error(
                at,
                RuleId::EMO_04,
                format!("emoji `{name}`: cannot read `{shown}`: {}", io_reason(&e)),
                "add the file (PNG, JPEG or GIF) or point #[emoji(file = \"..\")] at it",
            )
        })?;
        let format = validate_emoji_image(name, &bytes, self.limits)
            .map_err(|v| syn::Error::new_spanned(at, format!("{v}\n  = note: file `{shown}`")))?;
        Ok(Image { path, format })
    }

    /// The image of emoji `name` without `file`: exactly one of
    /// `<name>.png`, `.gif`, `.jpg`.
    fn find(&self, at: &Variant, name: &str) -> syn::Result<Image> {
        let found: Vec<String> = EmojiFormat::EXTENSIONS
            .iter()
            .map(|ext| format!("{name}.{ext}"))
            .filter(|file| self.base.join(file).is_file())
            .collect();
        match found.as_slice() {
            [file] => self.read(at, name, file),
            [] => Err(error(
                at,
                RuleId::EMO_04,
                format!(
                    "emoji `{name}`: no image file: looked for `{}`",
                    shown(self.shown, &format!("{name}.png|gif|jpg"))
                ),
                "add the file (PNG, JPEG or GIF) or point #[emoji(file = \"..\")] at it",
            )),
            several => Err(error(
                at,
                RuleId::EMO_04,
                format!(
                    "emoji `{name}`: several image files: {}",
                    several
                        .iter()
                        .map(|f| format!("`{}`", shown(self.shown, f)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                "keep one, or choose one with #[emoji(file = \"..\")]",
            )),
        }
    }
}

/// `dir/rel` as the user wrote `dir`.
fn shown(dir: &str, rel: &str) -> String {
    let dir = dir.trim_end_matches('/');
    if dir.is_empty() || dir == "." {
        rel.to_owned()
    } else {
        format!("{dir}/{rel}")
    }
}

fn io_reason(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        _ => "the file cannot be read",
    }
}

/// `<stem>_white.<ext>` of a file name.
fn white_file(file: &str) -> String {
    let path = Path::new(file);
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let white = match path.extension() {
        Some(ext) => format!("{stem}_white.{}", ext.to_string_lossy()),
        None => format!("{stem}_white"),
    };
    match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => format!("{}/{white}", parent.to_string_lossy()),
        None => white,
    }
}

/// A variant's checked declaration tokens.
struct Checked {
    ident: Ident,
    decl: TokenStream,
}

pub(crate) fn derive(input: &DeriveInput) -> TokenStream {
    expand(input).unwrap_or_else(syn::Error::into_compile_error)
}

#[expect(clippy::too_many_lines, reason = "one pass over the variants")]
fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            ident,
            format!("AppEmojis needs a fieldless enum\n  = help: {HELP}"),
        ));
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "an AppEmojis set cannot be generic",
        ));
    }
    let set = parse_set(input)?;
    let limits = Limits::default();
    let base = root(set.from_source_file)
        .map_err(|e| syn::Error::new_spanned(ident, format!("AppEmojis: {e}")))?
        .join(&set.dir);
    let files = Files {
        base: &base,
        shown: &set.dir,
        limits: &limits,
    };
    let mut errors = Vec::new();
    let mut checked = Vec::new();
    let mut names: Vec<(String, &Variant)> = Vec::new();
    let mut any_render = false;
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            errors.push(syn::Error::new_spanned(
                variant,
                format!("an AppEmojis variant has no fields\n  = help: {HELP}"),
            ));
            continue;
        }
        if let Some((_, d)) = &variant.discriminant {
            errors.push(syn::Error::new_spanned(
                d,
                "an AppEmojis variant has no discriminant",
            ));
            continue;
        }
        let args = match parse_variant(variant) {
            Ok(args) => args,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let name = args
            .name
            .as_ref()
            .map_or_else(|| snake_case(&plain(&variant.ident)), LitStr::value);
        if let Err(v) = validate_emoji_name(&name) {
            let at: &dyn ToTokens = match &args.name {
                Some(lit) => lit,
                None => variant,
            };
            errors.push(syn::Error::new_spanned(at, v.to_string()));
            continue;
        }
        let white_name = format!("{name}_white");
        let mut declared = vec![name.clone()];
        if args.white {
            if let Err(v) = validate_emoji_name(&white_name) {
                errors.push(syn::Error::new_spanned(
                    variant,
                    format!("{v}\n  = note: the white variant of `{name}`; shorten the name"),
                ));
                continue;
            }
            declared.push(white_name.clone());
        }
        let mut duplicate = false;
        for n in &declared {
            if let Some((_, first)) = names.iter().find(|(seen, _)| seen == n) {
                errors.push(error(
                    variant,
                    RuleId::EMO_03,
                    format!(
                        "emoji `{n}` is declared twice in this set (also by `{}`)",
                        plain(&first.ident)
                    ),
                    "an application emoji name is unique: rename one with #[emoji(name = \"..\")]",
                ));
                duplicate = true;
            }
        }
        if duplicate {
            continue;
        }
        names.extend(declared.into_iter().map(|n| (n, variant)));
        let v_ident = &variant.ident;
        let (image, white) = if args.render.is_some() {
            any_render = true;
            let Some(art) = &set.art else {
                errors.push(syn::Error::new(
                    args.render.unwrap_or_else(Span::call_site),
                    "`#[emoji(render)]` needs the set's art function: add #[emojis(art = path::to::fn)] (a `fn(Self, EmojiTone) -> EmojiArt`) to the enum",
                ));
                continue;
            };
            let render = |tone: TokenStream| {
                quote!(::noctis::__private::core::emoji::EmojiImage::Render({
                    fn __noctis_art(
                        _: ::noctis::__private::core::emoji::EmojiTone,
                    ) -> ::noctis::__private::core::emoji::EmojiArt {
                        let art: fn(
                            #ident,
                            ::noctis::__private::core::emoji::EmojiTone,
                        ) -> ::noctis::__private::core::emoji::EmojiArt = #art;
                        art(#ident::#v_ident, ::noctis::__private::core::emoji::EmojiTone::#tone)
                    }
                    __noctis_art
                }))
            };
            let white = args.white.then(|| render(quote!(White)));
            (render(quote!(Colour)), white)
        } else {
            let colour = match &args.file {
                Some(file) => files.read(variant, &name, &file.value()),
                None => files.find(variant, &name),
            };
            let colour = match colour {
                Ok(image) => image,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let white = if args.white {
                let image = match &args.file {
                    Some(file) => files.read(variant, &white_name, &white_file(&file.value())),
                    None => files.find(variant, &white_name),
                };
                match image {
                    Ok(image) => Some(image.tokens()),
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            } else {
                None
            };
            (colour.tokens(), white)
        };
        let name_lit = LitStr::new(&name, Span::call_site());
        let mut decl =
            quote!(::noctis::__private::core::emoji::EmojiDecl::__new(#name_lit, #image));
        if let Some(white) = white {
            let white_lit = LitStr::new(&white_name, Span::call_site());
            decl = quote!(#decl.__white(#white_lit, #white));
        }
        checked.push(Checked {
            ident: v_ident.clone(),
            decl,
        });
    }
    if let (Some(art), false) = (&set.art, any_render)
        && errors.is_empty()
    {
        errors.push(syn::Error::new_spanned(
            art,
            "`art` is set but no variant is `#[emoji(render)]`: remove `art`, or mark the drawn variants with #[emoji(render)]",
        ));
    }
    combine(errors)?;
    let decls = checked.iter().map(|c| &c.decl);
    let arms = checked.iter().enumerate().map(|(i, c)| {
        let v = &c.ident;
        quote!(Self::#v => &<Self as ::noctis::__private::core::emoji::AppEmojis>::DECLARATIONS[#i],)
    });
    let body = if checked.is_empty() {
        quote!(match self {})
    } else {
        quote!(match self { #(#arms)* })
    };
    Ok(quote! {
        #[automatically_derived]
        impl ::noctis::__private::core::emoji::AppEmojis for #ident {
            const DECLARATIONS: &'static [::noctis::__private::core::emoji::EmojiDecl] = &[#(#decls),*];

            fn declaration(self) -> &'static ::noctis::__private::core::emoji::EmojiDecl {
                #body
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_names() {
        for (ident, want) in [
            ("Check", "check"),
            ("StOnline", "st_online"),
            ("HTTPCode", "http_code"),
            ("Check2", "check2"),
            ("Bar2Left", "bar2_left"),
            ("Ic_Home", "ic_home"),
            ("ABC", "abc"),
        ] {
            assert_eq!(snake_case(ident), want, "{ident}");
        }
    }

    #[test]
    fn white_file_names() {
        assert_eq!(white_file("check.png"), "check_white.png");
        assert_eq!(white_file("ui/ok.gif"), "ui/ok_white.gif");
        assert_eq!(white_file("noext"), "noext_white");
    }

    #[test]
    fn shown_joins_dir() {
        assert_eq!(shown("assets/emoji/", "a.png"), "assets/emoji/a.png");
        assert_eq!(shown(".", "a.png"), "a.png");
    }
}
