//! `#[derive(Template)]`: `impl noctis::image::Template` from
//! `#[template(id, version, size, format[, animation(..)])]` (AD-22, ATT-07).
//!
//! Every key is checked at expansion: a wrong key, form, variant, duplicate,
//! missing key or out-of-range value is a compile error spanned on the
//! offending token. The derive also rejects generic structs, `HashMap` /
//! `HashSet` fields (Template data is part of the render cache key and must
//! serialize deterministically) and `#[asset]` fields (Story 10.8).

use proc_macro2::{Literal, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::ext::IdentExt;
use syn::parse::ParseStream;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, ExprStruct, GenericArgument, Ident, Lit, LitInt,
    LitStr, Member, PathArguments, Token, Type, parenthesized,
};

use crate::common::combine;

/// The longest Template id.
const MAX_ID_LEN: usize = 64;

const KEYS: &str = "`id`, `version`, `size`, `format` and `animation(..)`";
const STILL_FORMATS: &str =
    "`Png`, `Jpeg { quality: 1..=100 }`, `Webp { quality: 1..=100 }` or `WebpLossless`";
const ANIMATED_FORMATS: &str = "`Webp { quality: 1..=100 }`, `Gif` or `Apng`";

/// A still output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Still {
    Png,
    Jpeg(u8),
    Webp(u8),
    WebpLossless,
}

/// An animated output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Animated {
    Webp(u8),
    Gif,
    Apng,
}

/// `animation(duration_ms = .., fps = ..[, format = ..])`; `format`
/// defaults to `Webp { quality: 80 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Animation {
    pub(crate) duration_ms: u32,
    pub(crate) fps: u16,
    pub(crate) format: Animated,
}

/// The checked `#[template(..)]` settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) id: String,
    pub(crate) version: u32,
    pub(crate) size: (u32, u32),
    pub(crate) format: Still,
    pub(crate) animation: Option<Animation>,
}

/// The derive's entry point.
pub(crate) fn derive(input: &DeriveInput) -> TokenStream {
    expand(input).unwrap_or_else(syn::Error::into_compile_error)
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let data = match &input.data {
        Data::Struct(data) => data,
        Data::Enum(data) => {
            return Err(syn::Error::new_spanned(
                data.enum_token,
                "`#[derive(Template)]` needs a struct",
            ));
        }
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "`#[derive(Template)]` needs a struct",
            ));
        }
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "`#[derive(Template)]` needs a struct without generic or lifetime parameters: the \
             renderer decodes a Template by its type alone",
        ));
    }
    let mut errors = Vec::new();
    let attrs: Vec<&Attribute> = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("template"))
        .collect();
    let settings = match attrs.as_slice() {
        [] => {
            errors.push(syn::Error::new_spanned(
                &input.ident,
                "missing `#[template(id = \"..\", version = 1, size = (w, h), format = Png)]`",
            ));
            None
        }
        [attr, rest @ ..] => {
            for extra in rest {
                errors.push(syn::Error::new_spanned(
                    extra,
                    "`#[template(..)]` appears twice; give every key in one attribute",
                ));
            }
            match attr.parse_args_with(|input: ParseStream<'_>| parse_settings(input, attr)) {
                Ok(Ok(settings)) => Some(settings),
                Ok(Err(errs)) => {
                    errors.extend(errs);
                    None
                }
                Err(err) => {
                    errors.push(err);
                    None
                }
            }
        }
    };
    for field in &data.fields {
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("asset")) {
            errors.push(syn::Error::new_spanned(
                attr,
                "`#[asset]` remote images are planned (Story 10.8)",
            ));
        }
        scan_type(&field.ty, &mut errors);
    }
    combine(errors)?;
    let Some(settings) = settings else {
        return Err(syn::Error::new(Span::call_site(), "unreachable: settings"));
    };
    Ok(emit(&input.ident, &settings))
}

fn emit(ident: &Ident, settings: &Settings) -> TokenStream {
    let ports = quote!(::noctis::__private::ports);
    let id = LitStr::new(&settings.id, Span::call_site());
    let version = Literal::u32_unsuffixed(settings.version);
    let (width, height) = (
        Literal::u32_unsuffixed(settings.size.0),
        Literal::u32_unsuffixed(settings.size.1),
    );
    let format = match settings.format {
        Still::Png => quote!(#ports::Format::Png),
        Still::Jpeg(q) => {
            let q = Literal::u8_unsuffixed(q);
            quote!(#ports::Format::Jpeg { quality: #q })
        }
        Still::Webp(q) => {
            let q = Literal::u8_unsuffixed(q);
            quote!(#ports::Format::Webp { quality: #q })
        }
        Still::WebpLossless => quote!(#ports::Format::WebpLossless),
    };
    // `Gif`/`Apng` compile only with `image-gif`: the gate trait is
    // implemented only then, and its `on_unimplemented` names the feature.
    let gif_gate = match settings.animation {
        Some(Animation {
            format: Animated::Gif | Animated::Apng,
            ..
        }) => quote! {
            const _: () = ::noctis::__private::image::require_gif_output::<
                ::noctis::__private::image::GifOutputFormat,
            >();
        },
        _ => TokenStream::new(),
    };
    let animation = match settings.animation {
        None => quote!(::core::option::Option::None),
        Some(a) => {
            let ms = Literal::u64_unsuffixed(u64::from(a.duration_ms));
            let fps = Literal::u16_unsuffixed(a.fps);
            let format = match a.format {
                Animated::Webp(q) => {
                    let q = Literal::u8_unsuffixed(q);
                    quote!(#ports::AnimatedFormat::Webp { quality: #q })
                }
                Animated::Gif => quote!(#ports::AnimatedFormat::Gif),
                Animated::Apng => quote!(#ports::AnimatedFormat::Apng),
            };
            quote!(::core::option::Option::Some(#ports::AnimationSpec::new(
                ::core::time::Duration::from_millis(#ms),
                #fps,
                #format,
            )))
        }
    };
    quote! {
        impl #ports::Template for #ident {
            const ID: &'static str = #id;
            const VERSION: u32 = #version;
            const SIZE: #ports::Size = #ports::Size::new(#width, #height);
            const FORMAT: #ports::Format = #format;
            const ANIMATION: ::core::option::Option<#ports::AnimationSpec> = #animation;

            fn view() -> #ports::ErasedView {
                ::noctis::__private::image::erased_view::<Self>()
            }
        }
        #gif_gate
    }
}

/// Parses the attribute's arguments. Syntax errors are the outer `Err`;
/// key and value errors are collected in the inner one.
fn parse_settings(
    input: ParseStream<'_>,
    attr: &Attribute,
) -> syn::Result<Result<Settings, Vec<syn::Error>>> {
    let mut errors = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let (mut id, mut version, mut size, mut format, mut animation) = (None, None, None, None, None);
    while !input.is_empty() {
        let key = Ident::parse_any(input)?;
        let name = key.to_string();
        if seen.contains(&name) {
            errors.push(syn::Error::new(
                key.span(),
                format!("duplicate key `{name}` in `#[template(..)]`"),
            ));
        }
        seen.push(name.clone());
        match name.as_str() {
            "animation" => {
                if !input.peek(syn::token::Paren) {
                    return Err(syn::Error::new(
                        key.span(),
                        "`animation` takes `(duration_ms = <ms>, fps = <n>, format = ..)`",
                    ));
                }
                let content;
                parenthesized!(content in input);
                match parse_animation(&content, &key)? {
                    Ok(a) => animation = Some(a),
                    Err(errs) => errors.extend(errs),
                }
            }
            "id" | "version" | "size" | "format" => {
                input.parse::<Token![=]>().map_err(|_| {
                    syn::Error::new(key.span(), format!("expected `=` after `{name}`"))
                })?;
                let value: Expr = input.parse()?;
                let checked = match name.as_str() {
                    "id" => check_id(&value).map(|v| id = Some(v)),
                    "version" => check_version(&value).map(|v| version = Some(v)),
                    "size" => check_size(&value).map(|v| size = Some(v)),
                    _ => still_format(&value).map(|v| format = Some(v)),
                };
                if let Err(err) = checked {
                    errors.push(err);
                }
            }
            _ => {
                // Skip the value so the next key still parses.
                skip_value(input)?;
                errors.push(syn::Error::new(
                    key.span(),
                    format!("unknown key `{name}` in `#[template(..)]`: it takes {KEYS}"),
                ));
            }
        }
        if input.is_empty() {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    let missing = [
        ("id", id.is_none(), "id = \"<a-z0-9._->\""),
        ("version", version.is_none(), "version = <u32>"),
        ("size", size.is_none(), "size = (<width>, <height>)"),
        ("format", format.is_none(), "format = Png"),
    ];
    for (key, absent, form) in missing {
        if absent && !seen.iter().any(|s| s == key) {
            errors.push(syn::Error::new_spanned(
                attr,
                format!("`#[template(..)]` is missing `{form}`"),
            ));
        }
    }
    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    match (id, version, size, format) {
        (Some(id), Some(version), Some(size), Some(format)) => Ok(Ok(Settings {
            id,
            version,
            size,
            format,
            animation,
        })),
        _ => Ok(Err(vec![syn::Error::new_spanned(
            attr,
            "`#[template(..)]` is incomplete",
        )])),
    }
}

/// Skips `= <expr>` or `(..)` after an unknown key.
fn skip_value(input: ParseStream<'_>) -> syn::Result<()> {
    if input.peek(Token![=]) {
        input.parse::<Token![=]>()?;
        input.parse::<Expr>()?;
    } else if input.peek(syn::token::Paren) {
        let _content;
        parenthesized!(_content in input);
    }
    Ok(())
}

fn parse_animation(
    input: ParseStream<'_>,
    key: &Ident,
) -> syn::Result<Result<Animation, Vec<syn::Error>>> {
    let mut errors = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let (mut duration, mut fps, mut format) = (None, None, None);
    while !input.is_empty() {
        let k = Ident::parse_any(input)?;
        let name = k.to_string();
        if seen.contains(&name) {
            errors.push(syn::Error::new(
                k.span(),
                format!("duplicate key `{name}` in `animation(..)`"),
            ));
        }
        seen.push(name.clone());
        input
            .parse::<Token![=]>()
            .map_err(|_| syn::Error::new(k.span(), format!("expected `=` after `{name}`")))?;
        let value: Expr = input.parse()?;
        let checked = match name.as_str() {
            "duration_ms" => int(
                &value,
                "`duration_ms`",
                &["", "u32"],
                1,
                u128::from(u32::MAX),
            )
            .map(|v| duration = Some(narrow::<u32>(v))),
            "fps" => int(&value, "`fps`", &["", "u16"], 1, 90)
                .map(|v| fps = Some((narrow::<u16>(v), value.clone()))),
            "format" => animated_format(&value).map(|v| format = Some(v)),
            _ => Err(syn::Error::new(
                k.span(),
                format!(
                    "unknown key `{name}` in `animation(..)`: it takes `duration_ms`, `fps` and `format`"
                ),
            )),
        };
        if let Err(err) = checked {
            errors.push(err);
        }
        if input.is_empty() {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    for (name, form) in [
        ("duration_ms", "duration_ms = <milliseconds>"),
        ("fps", "fps = <frames per second>"),
    ] {
        if !seen.iter().any(|s| s == name) {
            errors.push(syn::Error::new(
                key.span(),
                format!("`animation(..)` is missing `{form}`"),
            ));
        }
    }
    if let (Some((fps, value)), Some(Animated::Gif)) = (&fps, format)
        && *fps > 50
    {
        errors.push(syn::Error::new_spanned(
            value,
            format!("`fps` for `Gif` must be 1..=50, found `{fps}`"),
        ));
    }
    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    match (duration, fps, format) {
        // `format` defaults to `AnimatedFormat::DEFAULT` (animated WebP, 80).
        (Some(duration_ms), Some((fps, _)), format) => Ok(Ok(Animation {
            duration_ms,
            fps,
            format: format.unwrap_or(Animated::Webp(80)),
        })),
        _ => Ok(Err(vec![syn::Error::new(
            key.span(),
            "`animation(..)` is incomplete",
        )])),
    }
}

/// A value already checked to fit `T`.
fn narrow<T: TryFrom<u128> + Default>(value: u128) -> T {
    T::try_from(value).unwrap_or_default()
}

/// How a value is shown in an error.
fn shown(expr: &Expr) -> String {
    match expr {
        Expr::Lit(ExprLit { lit, .. }) => lit.to_token_stream().to_string(),
        Expr::Group(group) => shown(&group.expr),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            format!("-{}", shown(&unary.expr))
        }
        other => other.to_token_stream().to_string(),
    }
}

fn lit_int(expr: &Expr) -> Option<&LitInt> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(int), ..
        }) => Some(int),
        Expr::Group(group) => lit_int(&group.expr),
        _ => None,
    }
}

/// An integer literal with one of `suffixes`, within `min..=max`; `what`
/// names the value in the error (`` `fps` ``).
fn int(expr: &Expr, what: &str, suffixes: &[&str], min: u128, max: u128) -> syn::Result<u128> {
    let range = format!("{min}..={max}");
    let fail = || {
        syn::Error::new_spanned(
            expr,
            format!(
                "{what} takes an integer literal {range}{}, found `{}`",
                suffixes
                    .iter()
                    .find(|s| !s.is_empty())
                    .map(|s| format!(" (`{s}`)"))
                    .unwrap_or_default(),
                shown(expr)
            ),
        )
    };
    let lit = lit_int(expr).ok_or_else(fail)?;
    if !suffixes.contains(&lit.suffix()) {
        return Err(fail());
    }
    let value: u128 = lit.base10_digits().parse().map_err(|_| fail())?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(fail())
    }
}

/// `[a-z0-9._-]{1,64}`.
pub(crate) fn is_template_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn check_id(expr: &Expr) -> syn::Result<String> {
    let value = match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Some(s.value()),
        _ => None,
    };
    match value {
        Some(id) if is_template_id(&id) => Ok(id),
        _ => Err(syn::Error::new_spanned(
            expr,
            format!(
                "`id` takes a string literal of 1 to 64 characters from `a-z`, `0-9`, `.`, `_` \
                 and `-`, found `{}`",
                shown(expr)
            ),
        )),
    }
}

fn check_version(expr: &Expr) -> syn::Result<u32> {
    int(expr, "`version`", &["", "u32"], 0, u128::from(u32::MAX)).map(narrow::<u32>)
}

fn check_size(expr: &Expr) -> syn::Result<(u32, u32)> {
    let form = || {
        syn::Error::new_spanned(
            expr,
            format!(
                "`size` takes `(width, height)`: two integer literals 1..={}, found `{}`",
                u32::MAX,
                shown(expr)
            ),
        )
    };
    let Expr::Tuple(tuple) = expr else {
        return Err(form());
    };
    let sides: Vec<&Expr> = tuple.elems.iter().collect();
    let [w, h] = sides.as_slice() else {
        return Err(form());
    };
    let max = u128::from(u32::MAX);
    let w = int(w, "the `size` width", &["", "u32"], 1, max)?;
    let h = int(h, "the `size` height", &["", "u32"], 1, max)?;
    Ok((narrow(w), narrow(h)))
}

/// The variant name and `quality` of a format expression.
fn format_parts(expr: &Expr) -> Option<(String, Option<&Expr>, &Expr)> {
    match expr {
        Expr::Path(path) if path.qself.is_none() => {
            let ident = path.path.get_ident()?;
            Some((ident.unraw().to_string(), None, expr))
        }
        Expr::Struct(ExprStruct {
            qself: None,
            path,
            fields,
            rest: None,
            dot2_token: None,
            ..
        }) => {
            let ident = path.get_ident()?;
            let [field] = fields.iter().collect::<Vec<_>>()[..] else {
                return Some((ident.to_string(), None, expr));
            };
            match &field.member {
                Member::Named(name) if name == "quality" => {
                    Some((ident.to_string(), Some(&field.expr), expr))
                }
                _ => Some((ident.to_string(), None, expr)),
            }
        }
        Expr::Group(group) => format_parts(&group.expr),
        _ => None,
    }
}

fn quality(expr: &Expr) -> syn::Result<u8> {
    int(expr, "`quality`", &["", "u8"], 1, 100).map(narrow::<u8>)
}

fn still_format(expr: &Expr) -> syn::Result<Still> {
    let unknown = || {
        syn::Error::new_spanned(
            expr,
            format!("`format` takes {STILL_FORMATS}, found `{}`", shown(expr)),
        )
    };
    let (name, q, whole) = format_parts(expr).ok_or_else(unknown)?;
    let has_fields = matches!(whole, Expr::Struct(_));
    match (name.as_str(), q) {
        ("Png", None) if !has_fields => Ok(Still::Png),
        ("WebpLossless", None) if !has_fields => Ok(Still::WebpLossless),
        ("Jpeg", Some(q)) => Ok(Still::Jpeg(quality(q)?)),
        ("Webp", Some(q)) => Ok(Still::Webp(quality(q)?)),
        ("Gif" | "Apng", _) => Err(syn::Error::new_spanned(
            expr,
            format!(
                "`{name}` is not a still format: `format` takes {STILL_FORMATS}; put an \
                 animated format in `animation(duration_ms = .., fps = .., format = {name})`"
            ),
        )),
        _ => Err(unknown()),
    }
}

fn animated_format(expr: &Expr) -> syn::Result<Animated> {
    let unknown = || {
        syn::Error::new_spanned(
            expr,
            format!(
                "`animation(format = ..)` takes {ANIMATED_FORMATS}, found `{}`",
                shown(expr)
            ),
        )
    };
    let (name, q, whole) = format_parts(expr).ok_or_else(unknown)?;
    let has_fields = matches!(whole, Expr::Struct(_));
    match (name.as_str(), q) {
        ("Webp", Some(q)) => Ok(Animated::Webp(quality(q)?)),
        ("Gif", None) if !has_fields => Ok(Animated::Gif),
        ("Apng", None) if !has_fields => Ok(Animated::Apng),
        _ => Err(unknown()),
    }
}

/// Pushes an error for every `HashMap` / `HashSet` path segment in `ty`.
fn scan_type(ty: &Type, errors: &mut Vec<syn::Error>) {
    match ty {
        Type::Path(path) => {
            if let Some(qself) = &path.qself {
                scan_type(&qself.ty, errors);
            }
            for segment in &path.path.segments {
                if segment.ident == "HashMap" || segment.ident == "HashSet" {
                    errors.push(syn::Error::new(
                        segment.ident.span(),
                        format!(
                            "Template data must serialize deterministically (it is part of the \
                             render cache key): `{}` iterates in random order; use `BTreeMap`, \
                             `BTreeSet` or `Vec`",
                            segment.ident
                        ),
                    ));
                }
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    for arg in &args.args {
                        match arg {
                            GenericArgument::Type(ty) => scan_type(ty, errors),
                            GenericArgument::AssocType(assoc) => scan_type(&assoc.ty, errors),
                            _ => {}
                        }
                    }
                }
            }
        }
        Type::Reference(r) => scan_type(&r.elem, errors),
        Type::Array(a) => scan_type(&a.elem, errors),
        Type::Slice(s) => scan_type(&s.elem, errors),
        Type::Ptr(p) => scan_type(&p.elem, errors),
        Type::Paren(p) => scan_type(&p.elem, errors),
        Type::Group(g) => scan_type(&g.elem, errors),
        Type::Tuple(t) => t.elems.iter().for_each(|ty| scan_type(ty, errors)),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
