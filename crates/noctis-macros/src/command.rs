//! `#[derive(SlashCommand)]` with its `#[command("path or pattern", ..)]`
//! attribute: checks the Command Path or Pattern at expansion with
//! `noctis_rules::command`, generates `<Struct>Path` for a Pattern with a
//! group, the options half (`crate::options`) over the struct's named
//! fields, and the kind scaffolding `#[handler]` binds through (FR-7, FR-8,
//! FR-10, FR-75, FR-77, AD-8, AD-16, AD-26).

use noctis_rules::command::{
    self as rules, CommandDecl, LocalizedText, validate_pattern_spacing, validate_tree,
};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Attribute, DeriveInput, Ident, LitInt, LitStr, Meta, Type};
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::common::combine;
use crate::meta::{self, MetaArgs};

mod locale;

use self::locale::{LocaleArgs, description_key};
pub(crate) use self::locale::{LocaleFile, OptionText, not_found, option_description};

const MISSING_PATTERN: &str = "`#[command]` needs a Command Path or Pattern as its first argument: `#[command(\"ping\", description = \"…\")]`";
const NOT_A_LITERAL: &str = "`#[command]` needs its Command Path or Pattern as a string literal; check runtime-built patterns with `noctis::rules::command::{validate_pattern_spacing, expand, validate_tree}`";
const DESCRIPTION_LITERAL: &str = "`description` takes a string literal";
const CAP_LITERAL: &str =
    "`max_expansions` takes an integer literal, such as `max_expansions = 30`";
const CAP_ZERO: &str = "`max_expansions` must be at least 1";

/// `#[command]` arguments that later stories add.
const PLANNED: [&str; 3] = ["rate_limit", "flags", "apps"];

const EXPECTED: &str = "expected `description`, `max_expansions`, `response`, `auto_defer`, `permissions`, `contexts`, `integration_types`, `nsfw`, `guilds`, `locales` or `default_locale`";

/// The hidden flag of Noctis's own trybuild fixtures: `locales` is relative
/// to the invoking source file.
const FROM_SOURCE_FILE: &str = "__locales_from_source_file";

const RESPONSE_UPDATE: &str = "[INT-04] a Command cannot `update` its message: write `response = public` or `response = ephemeral`";

const RESPONSE_VALUE: &str = "`response` takes `public` or `ephemeral`";

const AUTO_DEFER_VALUE: &str = "`auto_defer` takes `true` or `false`";

/// The parsed `#[command(..)]` arguments.
struct Args {
    pattern: LitStr,
    description: Option<LitStr>,
    max_expansions: Option<usize>,
    handler: HandlerArgs,
    meta: MetaArgs,
    locales: LocaleArgs,
}

/// The handler arguments every Command macro shares: `response` and
/// `auto_defer` (AD-11).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HandlerArgs {
    /// `response = ephemeral`.
    pub(crate) ephemeral: bool,
    /// `auto_defer = false`.
    pub(crate) manual: bool,
}

impl HandlerArgs {
    /// Parses `segment` when it is a handler argument (`response` or
    /// `auto_defer`); `None` for any other argument.
    pub(crate) fn parse(&mut self, name: &str, segment: &[TokenTree]) -> Option<syn::Result<()>> {
        match name {
            "response" => Some(parse_response(segment).map(|e| self.ephemeral = e)),
            "auto_defer" => Some(parse_auto_defer(segment).map(|a| self.manual = !a)),
            _ => None,
        }
    }

    /// The declared `Ephemerality` as a path.
    pub(crate) fn ephemerality(self) -> TokenStream {
        if self.ephemeral {
            quote!(::noctis::__private::core::Ephemerality::Ephemeral)
        } else {
            quote!(::noctis::__private::core::Ephemerality::Public)
        }
    }
}

/// The error of a withdrawn argument (the bare `ephemeral` and modal flags) of
/// `attr`, naming its replacement; `None` for any other argument.
pub(crate) fn withdrawn(attr: &str, name: &str, span: Span) -> Option<syn::Error> {
    let replacement = match name {
        "ephemeral" => "write `response = ephemeral`",
        "opens_modal" => "write `auto_defer = false` and take `CommandResponder<Manual>`",
        _ => return None,
    };
    Some(syn::Error::new(
        span,
        format!("unknown `{attr}` argument `{name}`: {replacement}"),
    ))
}

/// Splits `args` on top-level commas, dropping one trailing empty segment.
pub(crate) fn segments(args: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut segments: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tt in args {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => segments.push(Vec::new()),
            _ => {
                if let Some(last) = segments.last_mut() {
                    last.push(tt);
                }
            }
        }
    }
    if segments.len() > 1 && segments.last().is_some_and(Vec::is_empty) {
        segments.pop();
    }
    segments
}

pub(crate) fn span_of(segment: &[TokenTree]) -> Span {
    segment
        .first()
        .map_or_else(Span::call_site, TokenTree::span)
}

/// The name of a `name`, `name = …` or `name(…)` argument.
pub(crate) fn arg_name(segment: &[TokenTree]) -> Option<String> {
    match segment.first() {
        Some(TokenTree::Ident(ident)) => Some(ident.to_string()),
        _ => None,
    }
}

/// The tokens after `name =`, or `None` without `=`.
fn value_of(segment: &[TokenTree]) -> Option<TokenStream> {
    match segment {
        [_, TokenTree::Punct(eq), rest @ ..] if eq.as_char() == '=' => {
            Some(rest.iter().cloned().collect())
        }
        _ => None,
    }
}

/// Parses `response = public | ephemeral`; whether it is `ephemeral`.
fn parse_response(segment: &[TokenTree]) -> syn::Result<bool> {
    match segment {
        [_, TokenTree::Punct(eq), TokenTree::Ident(value)] if eq.as_char() == '=' => {
            match value.to_string().as_str() {
                "public" => Ok(false),
                "ephemeral" => Ok(true),
                "update" => Err(syn::Error::new(value.span(), RESPONSE_UPDATE)),
                _ => Err(syn::Error::new(value.span(), RESPONSE_VALUE)),
            }
        }
        _ => Err(syn::Error::new(span_of(segment), RESPONSE_VALUE)),
    }
}

/// Parses `auto_defer = true | false`, a bool literal.
fn parse_auto_defer(segment: &[TokenTree]) -> syn::Result<bool> {
    match segment {
        [_, TokenTree::Punct(eq), TokenTree::Ident(value)] if eq.as_char() == '=' => {
            match value.to_string().as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(syn::Error::new(value.span(), AUTO_DEFER_VALUE)),
            }
        }
        _ => Err(syn::Error::new(span_of(segment), AUTO_DEFER_VALUE)),
    }
}

/// Whether `ty` is a responder parameter: a path whose last segment is
/// `CommandResponder`, `ComponentResponder`, `ModalResponder` or
/// `Responder`.
pub(crate) fn is_responder(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            path.qself.is_none()
                && path.path.segments.last().is_some_and(|s| {
                    [
                        "CommandResponder",
                        "ComponentResponder",
                        "ModalResponder",
                        "Responder",
                    ]
                    .iter()
                    .any(|name| s.ident == name)
                })
        }
        _ => false,
    }
}

/// Parses `"pattern"[, description = "…"][, max_expansions = n]` and the
/// metadata arguments.
fn parse_args(args: TokenStream) -> syn::Result<Args> {
    let segments = segments(args);
    let mut iter = segments.into_iter();
    let first = iter.next().unwrap_or_default();
    let pattern = if first.is_empty() {
        return Err(syn::Error::new(Span::call_site(), MISSING_PATTERN));
    } else if matches!(first.get(1), Some(TokenTree::Punct(p)) if p.as_char() == '=') {
        return Err(syn::Error::new(span_of(&first), MISSING_PATTERN));
    } else {
        let tokens: TokenStream = first.iter().cloned().collect();
        syn::parse2::<LitStr>(tokens)
            .map_err(|_| syn::Error::new(span_of(&first), NOT_A_LITERAL))?
    };
    let mut parsed = Args {
        pattern,
        description: None,
        max_expansions: None,
        handler: HandlerArgs::default(),
        meta: MetaArgs::default(),
        locales: LocaleArgs::default(),
    };
    let mut seen: Vec<String> = Vec::new();
    for segment in iter {
        let span = span_of(&segment);
        let Some(name) = arg_name(&segment) else {
            let tokens: TokenStream = segment.iter().cloned().collect();
            return Err(syn::Error::new(
                span,
                format!("unknown `#[command]` argument `{tokens}`: {EXPECTED}"),
            ));
        };
        if let Some(error) = withdrawn("#[command]", &name, span) {
            return Err(error);
        }
        match name.as_str() {
            "locales" | "default_locale" | FROM_SOURCE_FILE if !cfg!(feature = "i18n") => {
                return Err(syn::Error::new(
                    span,
                    format!("`{name}` needs the `i18n` feature of `noctis`"),
                ));
            }
            "description" | "max_expansions" | "response" | "auto_defer" | "locales"
            | "default_locale" | FROM_SOURCE_FILE => {}
            other if meta::is_meta(other) => {}
            planned if PLANNED.contains(&planned) => {
                return Err(syn::Error::new(
                    span,
                    format!("`#[command]` argument `{name}` is planned and not supported yet"),
                ));
            }
            _ => {
                return Err(syn::Error::new(
                    span,
                    format!("unknown `#[command]` argument `{name}`: {EXPECTED}"),
                ));
            }
        }
        if seen.contains(&name) {
            return Err(syn::Error::new(
                span,
                format!("duplicate `#[command]` argument `{name}`"),
            ));
        }
        seen.push(name.clone());
        if let Some(result) = parsed.meta.parse(&name, &segment) {
            result?;
            continue;
        }
        if let Some(result) = parsed.handler.parse(&name, &segment) {
            result?;
            continue;
        }
        if matches!(
            name.as_str(),
            "locales" | "default_locale" | FROM_SOURCE_FILE
        ) {
            parse_locale_arg(&mut parsed.locales, &name, &segment)?;
            continue;
        }
        let value = value_of(&segment);
        if name == "description" {
            let lit = value
                .and_then(|v| syn::parse2::<LitStr>(v).ok())
                .ok_or_else(|| syn::Error::new(span, DESCRIPTION_LITERAL))?;
            parsed.description = Some(lit);
        } else {
            let lit = value
                .and_then(|v| syn::parse2::<LitInt>(v).ok())
                .ok_or_else(|| syn::Error::new(span, CAP_LITERAL))?;
            let cap = lit
                .base10_parse::<usize>()
                .map_err(|_| syn::Error::new(lit.span(), CAP_LITERAL))?;
            if cap == 0 {
                return Err(syn::Error::new(lit.span(), CAP_ZERO));
            }
            parsed.max_expansions = Some(cap);
        }
    }
    Ok(parsed)
}

/// Parses one of `locales = "<dir>"`, `default_locale = "<code>"` and the
/// hidden bare flag into `locales`.
fn parse_locale_arg(
    locales: &mut LocaleArgs,
    name: &str,
    segment: &[TokenTree],
) -> syn::Result<()> {
    let span = span_of(segment);
    if name == FROM_SOURCE_FILE {
        if segment.len() != 1 {
            return Err(syn::Error::new(span, format!("`{name}` is a bare flag")));
        }
        locales.from_source_file = true;
        return Ok(());
    }
    let lit = value_of(segment)
        .and_then(|v| syn::parse2::<LitStr>(v).ok())
        .ok_or_else(|| syn::Error::new(span, format!("`{name}` takes a string literal")))?;
    if name == "locales" {
        locales.dir = Some(lit);
    } else {
        let locale = lit
            .value()
            .parse::<noctis_rules::Locale>()
            .map_err(|e| syn::Error::new(lit.span(), e.to_string()))?;
        locales.default = Some((locale, lit.span()));
    }
    Ok(())
}

/// Adds the macro's help to a violation that has none (see the spec's
/// "Always" list): the `?` removal for CMD-11/CMD-12, the `description`
/// argument for a missing CMD-04, and the cap for an over-cap CMD-15.
fn add_help(violation: &mut Violation, pattern: &str, limits: &Limits) {
    if violation.help.is_some() {
        return;
    }
    let help = match violation.rule {
        RuleId::CMD_11 | RuleId::CMD_12 if pattern.contains('?') => {
            Some(format!("remove the `?`: \"{}\"", pattern.replace('?', "")))
        }
        RuleId::CMD_04 if violation.notes.iter().any(|n| n == "missing description") => {
            Some(format!(
                "add `description = \"…\"` with 1 to {} characters (doc comments are rustdoc only)",
                limits.command_description_len
            ))
        }
        RuleId::CMD_15 => match (&violation.limit, &violation.actual) {
            (Bound::AtMost(_), Actual::Count(n)) => Some(format!(
                "raise the cap with `max_expansions = {n}` or split the pattern"
            )),
            _ => None,
        },
        _ => None,
    };
    violation.help = help;
}

/// The result of steps 3-4: the paths (when the pattern expands), each
/// path's description and every violation, with the macro's help added.
struct Checked {
    paths: Option<Vec<String>>,
    descriptions: Vec<String>,
    violations: Vec<Violation>,
}

/// Steps 3-4: the spacing check, `expand`, the description of each path
/// (the literal or `cmd.<path>.description` of `file`), the reserved words
/// (with a `file`) and `validate_tree`.
fn check_pattern(args: &Args, file: Option<&LocaleFile>) -> Checked {
    let defaults = Limits::default();
    let mut cap = Limits::default();
    if let Some(n) = args.max_expansions {
        cap.pattern_expansions = n;
    }
    let pattern = args.pattern.value();
    let literal = args.description.as_ref().map(LitStr::value);
    let mut violations = validate_pattern_spacing(&pattern, &defaults);
    let mut descriptions = Vec::new();
    let paths = match rules::expand(&pattern, &cap) {
        Ok(paths) => {
            if let Some(file) = file {
                violations.extend(locale::reserved_words(&paths, file));
            }
            let (texts, conflicts) = locale::descriptions(&paths, literal.as_deref(), file);
            violations.extend(conflicts);
            let decls: Vec<CommandDecl<'_>> = paths
                .iter()
                .zip(&texts)
                .map(|(path, text)| {
                    let decl = CommandDecl::handler("", path);
                    match text {
                        Some(d) => decl.description(LocalizedText::new(d)),
                        None => decl,
                    }
                })
                .collect();
            if let Err(tree) = validate_tree(&decls, &defaults) {
                violations.extend(tree);
            }
            descriptions = texts.into_iter().map(Option::unwrap_or_default).collect();
            Some(paths)
        }
        Err(errors) => {
            violations.extend(errors);
            None
        }
    };
    for v in &mut violations {
        if let Some(file) = file
            && v.rule == RuleId::CMD_04
            && v.notes.iter().any(|n| n == "missing description")
        {
            v.notes.push(not_found(&description_key(&v.path), file));
        }
        add_help(v, &pattern, &defaults);
    }
    Checked {
        paths,
        descriptions,
        violations,
    }
}

/// The pattern violations (on the description literal for CMD-04 when one
/// is given, else on the pattern literal), then META-03 (on the argument
/// name), as one error.
fn report(args: &Args, checked: &Checked) -> syn::Result<()> {
    let mut errors: Vec<syn::Error> = checked
        .violations
        .iter()
        .map(|v| {
            let span = match (&args.description, v.rule) {
                (Some(d), RuleId::CMD_04) => d.span(),
                _ => args.pattern.span(),
            };
            syn::Error::new(span, v.to_string())
        })
        .collect();
    errors.extend(args.meta.meta03());
    combine(errors)?;
    if checked.paths.is_none() {
        return Err(syn::Error::new(
            args.pattern.span(),
            "`#[command]` pattern check failed",
        ));
    }
    Ok(())
}

/// One word in `UpperCamelCase`: split on `seps`, each non-empty segment's
/// first char uppercased (full mapping), concatenated.
fn camel(word: &str, seps: &[char]) -> String {
    word.split(seps)
        .filter(|s| !s.is_empty())
        .flat_map(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .into_iter()
                .flat_map(char::to_uppercase)
                .chain(chars)
        })
        .collect()
}

/// Makes `raw` an identifier: drops non-`XID_Continue` chars, `Bare` when
/// empty, `_` before a non-`XID_Start` first char, `Self_` for `Self`.
fn identifier(raw: &str) -> String {
    let kept: String = raw.chars().filter(|&c| is_xid_continue(c)).collect();
    match kept.chars().next() {
        None => "Bare".to_owned(),
        Some(_) if kept == "Self" => "Self_".to_owned(),
        Some(c) if !is_xid_start(c) => format!("_{kept}"),
        Some(_) => kept,
    }
}

/// The name of the path enum of the struct `name` (`r#` stripped):
/// `<Struct>Path`.
fn enum_name(name: &str) -> String {
    let name = name.strip_prefix("r#").unwrap_or(name);
    identifier(&format!("{}Path", camel(name, &['_'])))
}

/// The variant name of an expansion from its chosen alternatives.
fn variant_name(alternatives: &[&str]) -> String {
    let joined: String = alternatives
        .iter()
        .map(|word| camel(word, &['-', '_']))
        .collect();
    identifier(&joined)
}

/// The chosen alternatives of every expansion of a well-spaced, valid
/// pattern, in `expand`'s odometer order (absent optional groups add none).
fn alternatives(pattern: &str) -> Vec<Vec<&str>> {
    let mut partial: Vec<Vec<&str>> = vec![Vec::new()];
    for token in pattern.split(' ') {
        let Some(group) = token.strip_prefix('(') else {
            continue;
        };
        let optional = group.ends_with('?');
        let group = group.trim_end_matches('?').trim_end_matches(')');
        let choices: Vec<Option<&str>> = group
            .split('|')
            .map(Some)
            .chain(optional.then_some(None))
            .collect();
        partial = partial
            .into_iter()
            .flat_map(|prefix| {
                choices.iter().map(move |choice| {
                    let mut next = prefix.clone();
                    next.extend(*choice);
                    next
                })
            })
            .collect();
    }
    partial
}

/// Step 5: one variant name per path (same order); a collision is an error.
fn variants(pattern: &str, paths: &[String]) -> Result<Vec<String>, String> {
    let chosen = alternatives(pattern);
    if chosen.len() != paths.len() {
        return Err(format!(
            "`#[command]` could not name the paths of `{pattern}`; please report this"
        ));
    }
    let names: Vec<String> = chosen.iter().map(|alts| variant_name(alts)).collect();
    for (i, name) in names.iter().enumerate() {
        if let Some(j) = names[..i].iter().position(|n| n == name) {
            return Err(format!(
                "`#[command]` path variants collide: `{}` and `{}` both name `{name}`; rename one alternative",
                paths[j], paths[i]
            ));
        }
    }
    Ok(names)
}

fn ident(name: &str, span: Span) -> syn::Result<Ident> {
    let mut ident: Ident = syn::parse_str(name).map_err(|_| {
        syn::Error::new(span, format!("`#[command]` cannot use `{name}` as a name"))
    })?;
    ident.set_span(span);
    Ok(ident)
}

/// The generated `<Struct>Path` enum and its impls.
fn path_enum(
    input: &DeriveInput,
    enum_ident: &Ident,
    struct_name: &str,
    pattern: &str,
    paths: &[String],
    variants: &[Ident],
) -> TokenStream {
    let vis = &input.vis;
    let cfgs = input.attrs.iter().filter(|a| a.path().is_ident("cfg"));
    let doc = format!(" The Command Paths of `{struct_name}`: `{pattern}`.");
    let variant_docs = paths.iter().map(|p| format!(" `/{p}`"));
    let count = paths.len();
    let cfgs2 = cfgs.clone();
    let cfgs3 = cfgs.clone();
    quote! {
        #(#cfgs)*
        #[doc = #doc]
        #[derive(::core::clone::Clone, ::core::marker::Copy, ::core::fmt::Debug, ::core::cmp::PartialEq, ::core::cmp::Eq, ::core::hash::Hash)]
        #[allow(non_camel_case_types, clippy::enum_variant_names)]
        #vis enum #enum_ident {
            #( #[doc = #variant_docs] #variants, )*
        }

        #(#cfgs2)*
        impl #enum_ident {
            /// Every path, in expansion order.
            pub const ALL: [Self; #count] = [#(Self::#variants),*];

            /// The Command Path, words joined by one space.
            #[must_use]
            pub const fn as_str(self) -> &'static ::core::primitive::str {
                match self {
                    #(Self::#variants => #paths,)*
                }
            }
        }

        #(#cfgs3)*
        impl ::noctis::__private::core::CommandPath for #enum_ident {
            const PATHS: &'static [&'static ::core::primitive::str] = &[#(#paths),*];

            fn from_path(path: &::core::primitive::str) -> ::core::option::Option<Self> {
                match path {
                    #(#paths => ::core::option::Option::Some(Self::#variants),)*
                    _ => ::core::option::Option::None,
                }
            }

            fn as_str(self) -> &'static ::core::primitive::str {
                Self::as_str(self)
            }
        }
    }
}

/// The one `#[name(..)]` attribute of a kind derive, as its argument
/// tokens; `missing` when there is none, `MISSING_PATTERN`-like `empty`
/// when it has no list.
pub(crate) fn kind_attr<'a>(
    attrs: &'a [Attribute],
    name: &str,
    ident: &Ident,
    missing: &str,
    empty: &str,
) -> syn::Result<(&'a Attribute, TokenStream)> {
    let mut found = attrs.iter().filter(|a| a.path().is_ident(name));
    let Some(attr) = found.next() else {
        return Err(syn::Error::new_spanned(ident, missing));
    };
    if let Some(second) = found.next() {
        return Err(syn::Error::new_spanned(
            second,
            format!("a struct takes one #[{name}(..)] attribute"),
        ));
    }
    match &attr.meta {
        Meta::List(list) if !list.tokens.is_empty() => Ok((attr, list.tokens.clone())),
        _ => Err(syn::Error::new_spanned(attr, empty)),
    }
}

/// The placeholder impls a failed `#[derive(SlashCommand)]` emits beside its
/// errors (see `kind::fallback`): `options` when the options half expanded,
/// else a stub `Options` impl with an empty `<Struct>Partial`.
fn fallback_items(input: &DeriveInput, options: Option<TokenStream>) -> TokenStream {
    let core = quote!(::noctis::__private::core);
    let ident = &input.ident;
    let options = options.unwrap_or_else(|| {
        let vis = &input.vis;
        let partial = Ident::new(&format!("{}Partial", crate::common::plain(ident)), ident.span());
        quote! {
            #[doc(hidden)]
            #[derive(::core::default::Default)]
            #vis struct #partial;

            impl #core::PartialOptions for #partial {
                type Options = #ident;

                fn __from_input(_: &#core::CommandInput) -> Self {
                    Self
                }
            }

            impl #core::Options for #ident {
                const OPTIONS: &'static [::noctis::__private::rules::command::OptionDecl<'static>] = &[];

                type Partial = #partial;

                fn from_input(
                    _: &#core::CommandInput,
                ) -> ::core::result::Result<Self, #core::OptionError> {
                    ::core::unreachable!("the derive failed to compile")
                }
            }
        }
    });
    quote! {
        #options

        impl #core::__SlashCommand for #ident {
            const PATTERN: &'static ::core::primitive::str = "";
            const PATHS: &'static [&'static ::core::primitive::str] = &[];
            const DESCRIPTIONS: &'static [&'static ::core::primitive::str] = &[];
            const METADATA: #core::CommandMetadata = #core::CommandMetadata::new();
            const EPHEMERALITY: #core::Ephemerality = #core::Ephemerality::Public;
        }

        impl #core::__SinglePath for #ident {}
    }
}

/// Expands `#[derive(SlashCommand)]`: on any error, the errors and the
/// placeholder impls (`kind::fallback`).
pub(crate) fn derive(input: &DeriveInput) -> TokenStream {
    let fields = match crate::common::kind_fields(input, "SlashCommand") {
        Ok(fields) => fields,
        Err(e) => return e.into_compile_error(),
    };
    let fail = |options: Option<TokenStream>, errors: TokenStream| {
        let items = fallback_items(input, options);
        crate::kind::fallback(&input.ident, crate::kind::Family::Slash, &items, &errors)
    };
    let args = kind_attr(
        &input.attrs,
        "command",
        &input.ident,
        "`#[derive(SlashCommand)]` needs `#[command(\"path\", description = \"…\")]`",
        MISSING_PATTERN,
    )
    .and_then(|(_, tokens)| parse_args(tokens));
    let args = match args {
        Ok(args) => args,
        Err(e) => return fail(None, e.into_compile_error()),
    };
    let file = match locale::load(&args.locales) {
        Ok(file) => file,
        Err(e) => return fail(None, e.into_compile_error()),
    };
    let checked = check_pattern(&args, file.as_ref());
    let reported = report(&args, &checked);
    let option_locale = file
        .as_ref()
        .zip(checked.paths.as_deref())
        .map(|(file, paths)| crate::options::OptionLocale { file, paths });
    let options = crate::options::expand(input, &fields, option_locale);
    let includes = file
        .as_ref()
        .and_then(|f| f.path.as_ref())
        .and_then(|p| p.to_str())
        .map(|p| {
            quote!(
                const _: &[::core::primitive::u8] = ::core::include_bytes!(#p);
            )
        })
        .unwrap_or_default();
    let (paths, options) = match (reported, options) {
        (Ok(()), Ok(o)) => (checked.paths.unwrap_or_default(), o),
        (Err(e), Ok(o)) => {
            let e = e.into_compile_error();
            return fail(Some(o), quote!(#e #includes));
        }
        (Ok(()), Err(errors)) => return fail(None, quote!(#errors #includes)),
        (Err(e), Err(errors)) => {
            let e = e.into_compile_error();
            return fail(None, quote!(#e #errors #includes));
        }
    };
    match expand(input, &args, &paths, &checked.descriptions, &options) {
        Ok(tokens) => quote!(#tokens #includes),
        Err(e) => fail(Some(options), e.into_compile_error()),
    }
}

/// The items of a checked `#[derive(SlashCommand)]`.
fn expand(
    input: &DeriveInput,
    args: &Args,
    paths: &[String],
    descriptions: &[String],
    options: &TokenStream,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let pattern = args.pattern.value();
    let struct_name = crate::common::plain(ident);
    let core = quote!(::noctis::__private::core);
    let path_impls = if pattern.contains('(') {
        let names = variants(&pattern, paths)
            .map_err(|message| syn::Error::new(args.pattern.span(), message))?;
        let enum_ident = self::ident(&enum_name(&struct_name), ident.span())?;
        let variants = names
            .iter()
            .map(|n| self::ident(n, args.pattern.span()))
            .collect::<syn::Result<Vec<_>>>()?;
        let generated = path_enum(input, &enum_ident, &struct_name, &pattern, paths, &variants);
        quote! {
            #generated

            #[automatically_derived]
            impl #core::__PatternPaths for #ident {
                type Path = #enum_ident;
            }
        }
    } else {
        quote! {
            #[automatically_derived]
            impl #core::__SinglePath for #ident {}
        }
    };
    let metadata = if args.meta.is_empty() {
        quote!(#core::CommandMetadata::new())
    } else {
        args.meta.tokens()
    };
    let ephemerality = args.handler.ephemerality();
    let scaffolding =
        crate::kind::scaffolding(ident, crate::kind::Family::Slash, args.handler.manual);
    Ok(quote! {
        #options

        #path_impls

        #[automatically_derived]
        impl #core::__SlashCommand for #ident {
            const PATTERN: &'static ::core::primitive::str = #pattern;
            const PATHS: &'static [&'static ::core::primitive::str] = &[#(#paths),*];
            const DESCRIPTIONS: &'static [&'static ::core::primitive::str] = &[#(#descriptions),*];
            const METADATA: #core::CommandMetadata = #metadata;
            const EPHEMERALITY: #core::Ephemerality = #ephemerality;
        }

        #scaffolding
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(src: &str) -> TokenStream {
        src.parse().unwrap()
    }

    /// `#[derive(SlashCommand)] #[command(args)]` on a unit struct named
    /// after the handler `item` of the old free-function cases.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the test cases hand over `quote!` values"
    )]
    fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
        let f: syn::ItemFn = syn::parse2(item).unwrap();
        let name = Ident::new(
            &camel(&crate::common::plain(&f.sig.ident), &['_']),
            Span::call_site(),
        );
        let input: DeriveInput = syn::parse_quote!(#[command(#args)] struct #name;);
        let out = derive(&input);
        let text = out.to_string();
        match text.find("compile_error") {
            Some(_) => Err(errors(&text)),
            None => Ok(out),
        }
    }

    /// The `compile_error!` messages of a failed expansion, as one error.
    fn errors(text: &str) -> syn::Error {
        let file: syn::File = syn::parse_str(text).unwrap();
        let mut found: Option<syn::Error> = None;
        for item in file.items {
            if let syn::Item::Macro(m) = item
                && m.mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "compile_error")
            {
                let lit: LitStr = m.mac.parse_body().unwrap();
                let e = syn::Error::new(Span::call_site(), lit.value());
                match &mut found {
                    Some(f) => f.combine(e),
                    None => found = Some(e),
                }
            }
        }
        found.unwrap()
    }

    fn run(src: TokenStream) -> String {
        let input: DeriveInput = syn::parse2(src).unwrap();
        let out = derive(&input).to_string();
        if out.contains("compile_error") {
            errors(&out).to_string()
        } else {
            out
        }
    }

    /// Story 6.11 Matrix 1: a unit command.
    #[test]
    fn expands_a_unit_struct() {
        let out = run(quote! {
            /// docs
            #[command("ping", description = "d")]
            pub struct Ping;
        });
        assert!(
            out.contains("impl :: noctis :: __private :: core :: Options for Ping"),
            "{out}"
        );
        assert!(out.contains("Ok (Self { })"), "{out}");
        assert!(out.contains("__SinglePath for Ping"), "{out}");
        assert!(
            out.contains(
                "const PATHS : & 'static [& 'static :: core :: primitive :: str] = & [\"ping\"]"
            ),
            "{out}"
        );
        assert!(
            out.contains(
                "const DESCRIPTIONS : & 'static [& 'static :: core :: primitive :: str] = & [\"d\"]"
            ),
            "{out}"
        );
        assert!(out.contains("Ephemerality :: Public"), "{out}");
        assert!(out.contains("CommandMetadata :: new ()"), "{out}");
        assert!(
            out.contains("type Defer = :: noctis :: __private :: core :: AutoDefer"),
            "{out}"
        );
        assert!(out.contains("\"::\" , \"Ping\""), "{out}");
        assert!(!out.contains("enum"), "{out}");
    }

    /// Story 6.11 Matrix 2 and 3: options and a Pattern with a group.
    #[test]
    fn expands_options_and_a_pattern() {
        let out = run(quote! {
            #[command("config (set|get) (prefix|welcome)", description = "Configure", response = ephemeral, auto_defer = false)]
            pub(crate) struct Config {
                #[option(description = "u")]
                user: ResolvedUser,
                #[option(description = "r")]
                reason: Option<String>,
            }
        });
        assert!(
            out.contains(
                "pub (crate) enum ConfigPath { # [doc = \" `/config set prefix`\"] SetPrefix ,"
            ),
            "{out}"
        );
        assert!(out.contains("GetWelcome"), "{out}");
        assert!(out.contains("type Path = ConfigPath"), "{out}");
        assert!(out.contains("pub (crate) struct ConfigPartial"), "{out}");
        assert!(out.contains("Ephemerality :: Ephemeral"), "{out}");
        assert!(
            out.contains("type Defer = :: noctis :: __private :: core :: Manual"),
            "{out}"
        );
        assert!(
            out.contains("\"config (set|get) (prefix|welcome)\""),
            "{out}"
        );
    }

    #[test]
    fn cfg_attributes_reach_the_enum() {
        let out = run(quote! {
            #[cfg(test)]
            #[command("x (a|b)", description = "d")]
            struct X;
        });
        assert_eq!(out.matches("# [cfg (test)]").count(), 3, "{out}");
    }

    /// Story 6.11 Matrix 5 and 6.
    #[test]
    fn struct_and_attribute_errors() {
        assert_eq!(
            run(quote!(
                struct Ping;
            )),
            "`#[derive(SlashCommand)]` needs `#[command(\"path\", description = \"…\")]`"
        );
        assert_eq!(
            run(quote!(
                #[command]
                struct Ping;
            )),
            MISSING_PATTERN
        );
        for shape in [
            quote!(
                enum E {
                    A,
                }
            ),
            quote!(
                struct T(u8);
            ),
            quote!(
                struct G<T> {
                    t: T,
                }
            ),
        ] {
            let out = run(quote!(#[command("p", description = "d")] #shape));
            assert_eq!(
                out, "`#[derive(SlashCommand)]` needs a struct with named fields or a unit struct",
                "{shape}"
            );
        }
        assert!(
            run(quote!(
                #[command("ping", descripción = "d")]
                struct P;
            ))
            .starts_with("unknown `#[command]` argument `descripción`")
        );
    }

    /// Story 6.3 Matrix 1, 2 and 28: the declared metadata.
    #[test]
    fn metadata_tokens() {
        let out = run(quote!(#[command("ping", description = "d", permissions = MANAGE_GUILD | BAN_MEMBERS, contexts = [Guild], nsfw = true)] struct P;))
            .replace(' ', "");
        assert!(
            out.contains("constMETADATA:::noctis::__private::core::CommandMetadata={constM"),
            "{out}"
        );
        assert!(out.contains("from_bits_retain(36u64)"), "{out}");
    }

    const PING: &str = "async fn ping(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }";
    const CONFIG: &str =
        "async fn config(ctx: CommandCtx, path: ConfigPath) -> HandlerResult<()> { Ok(()) }";

    fn error(args: TokenStream, src: &str) -> String {
        expand(args, item(src)).unwrap_err().to_string()
    }

    /// Every message of a combined error.
    fn messages(args: TokenStream, src: &str) -> Vec<String> {
        expand(args, item(src))
            .unwrap_err()
            .into_iter()
            .map(|e| e.to_string())
            .collect()
    }

    /// Matrix 11: no `?` help without a `?`.
    #[test]
    fn cmd_11_no_question_mark_help_without_question_mark() {
        let limits = Limits::default();
        for rule in [RuleId::CMD_11, RuleId::CMD_12] {
            let mut v = Violation::new(rule, "config", Bound::Describe("x"), Actual::Count(0));
            add_help(&mut v, "config set", &limits);
            assert_eq!(v.help, None);
            add_help(&mut v, "config (set)?", &limits);
            assert_eq!(v.help.as_deref(), Some("remove the `?`: \"config (set)\""));
        }
    }

    /// Matrix 20: an empty description.
    #[test]
    fn cmd_04_empty_description() {
        let errs = messages(quote!("ping", description = ""), PING);
        assert_eq!(
            errs,
            [
                "[CMD-04] ping: limit between 1 and 100, actual 0\n  = note: in the default description"
            ]
        );
    }

    /// Matrix 19 (help text) and 24: one CMD-04 per handler path.
    #[test]
    fn cmd_04_missing_description_per_expansion() {
        let errs = messages(quote!("config (set|get) (prefix|welcome)"), CONFIG);
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert_eq!(
            errs[0],
            "[CMD-04] config set prefix: limit between 1 and 100, actual 0\n  = note: missing description\n  \
             = help: add `description = \"…\"` with 1 to 100 characters (doc comments are rustdoc only)"
        );
    }

    /// Matrix 23: UTF-16 length.
    #[test]
    fn cmd_04_unicode_over() {
        let desc = "😀".repeat(51);
        let errs = messages(quote!("ping", description = #desc), PING);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("[CMD-04] ping: limit between 1 and 100, actual 102"));
    }

    /// Matrix 17: 33 × `ğ`.
    #[test]
    fn cmd_02_unicode_word_33() {
        let word = "ğ".repeat(33);
        let errs = messages(quote!(#word, description = "d"), PING);
        assert_eq!(
            errs,
            [format!(
                "[CMD-02] {word}: limit between 1 and 32, actual 33"
            )]
        );
    }

    /// Matrix 27: an empty pattern is CMD-10 only.
    #[test]
    fn cmd_10_empty_pattern() {
        let errs = messages(quote!("", description = "d"), PING);
        assert_eq!(errs, ["[CMD-10] limit between 1 and 3, actual 0"]);
    }

    /// Matrix 36: spacing, then tree violations, all in one error.
    #[test]
    fn cmd_02_several_rules_at_once() {
        let errs = messages(quote!("Config  it's", description = "d"), PING);
        let rules: Vec<_> = errs.iter().map(|e| &e[..8]).collect();
        assert_eq!(rules, ["[CMD-02]", "[CMD-03]", "[CMD-01]"], "{errs:?}");
    }

    #[test]
    fn cmd_15_over_cap_help() {
        let alts: Vec<String> = (1..=26).map(|i| format!("a{i}")).collect();
        let pattern = format!("b ({})", alts.join("|"));
        let errs = messages(quote!(#pattern, description = "d"), CONFIG);
        assert_eq!(
            errs,
            [format!(
                "[CMD-15] {pattern}: limit at most 25, actual 26\n  \
                 = help: raise the cap with `max_expansions = 26` or split the pattern"
            )]
        );
    }

    /// Matrix 37-40: argument errors.
    #[test]
    fn argument_errors() {
        let cases: [(TokenStream, &str); 21] = [
            (quote!(), MISSING_PATTERN),
            (quote!(PATTERN), NOT_A_LITERAL),
            (quote!(description = "d"), MISSING_PATTERN),
            (quote!(b"x"), NOT_A_LITERAL),
            (quote!("ping", max_expansions = 0), CAP_ZERO),
            (quote!("ping", max_expansions = -1), CAP_LITERAL),
            (quote!("ping", max_expansions = "3"), CAP_LITERAL),
            (quote!("ping", max_expansions = N), CAP_LITERAL),
            (quote!("ping", max_expansions = 3.0), CAP_LITERAL),
            (quote!("ping", max_expansions), CAP_LITERAL),
            (
                quote!("ping", description = "a", description = "b"),
                "duplicate `#[command]` argument `description`",
            ),
            (
                quote!("ping", foo = 1),
                "unknown `#[command]` argument `foo`: expected `description`, `max_expansions`, `response`, `auto_defer`, `permissions`, `contexts`, `integration_types`, `nsfw`, `guilds`, `locales` or `default_locale`",
            ),
            (
                quote!("ping", ephemeral),
                "unknown `#[command]` argument `ephemeral`: write `response = ephemeral`",
            ),
            (
                quote!("ping", ephemeral = true),
                "unknown `#[command]` argument `ephemeral`: write `response = ephemeral`",
            ),
            (
                quote!("ping", opens_modal),
                "unknown `#[command]` argument `opens_modal`: write `auto_defer = false` and take `CommandResponder<Manual>`",
            ),
            (
                quote!("ping", response = public, response = ephemeral),
                "duplicate `#[command]` argument `response`",
            ),
            (
                quote!("ping", auto_defer = true, auto_defer = true),
                "duplicate `#[command]` argument `auto_defer`",
            ),
            (
                quote!("ping", default_member_permissions = X),
                "unknown `#[command]` argument `default_member_permissions`: expected `description`, `max_expansions`, `response`, `auto_defer`, `permissions`, `contexts`, `integration_types`, `nsfw`, `guilds`, `locales` or `default_locale`",
            ),
            (
                quote!("ping", rate_limit(user, 3)),
                "`#[command]` argument `rate_limit` is planned and not supported yet",
            ),
            (
                quote!("ping", description = concat!("a")),
                DESCRIPTION_LITERAL,
            ),
            (quote!("ping", description), DESCRIPTION_LITERAL),
        ];
        for (args, expected) in cases {
            assert_eq!(error(args.clone(), PING), expected, "{args}");
        }
    }

    /// Matrix 40: raw strings.
    #[test]
    fn raw_strings_are_accepted() {
        let args: TokenStream = "r\"ping\", description = r#\"a \"b\"\"#".parse().unwrap();
        let out = expand(args, item(PING)).unwrap().to_string();
        assert!(out.contains("\"a \\\"b\\\"\""), "{out}");
        let parsed = parse_args(quote!("ping", max_expansions = 30)).unwrap();
        assert_eq!(parsed.max_expansions, Some(30));
    }

    /// Matrix 20 and 23 (6.3): META-03 after the pattern violations, in one
    /// error.
    #[test]
    fn meta_03_joins_the_pattern_violations() {
        let errs = messages(
            quote!("ping", description = "d", guilds = [42], contexts = [Guild]),
            PING,
        );
        assert_eq!(
            errs,
            [
                "[META-03] contexts: limit only on global commands, actual `[Guild]`\n  = note: scope: guild 42"
            ]
        );
        let errs = messages(quote!("Ping", guilds = [1], contexts = [Guild]), PING);
        let rules: Vec<_> = errs.iter().map(|e| &e[..9]).collect();
        assert_eq!(rules, ["[CMD-03] ", "[CMD-04] ", "[META-03]"], "{errs:?}");
        assert!(
            expand(
                quote!(
                    "ping",
                    description = "d",
                    guilds = [42],
                    permissions = MANAGE_GUILD,
                    nsfw = true
                ),
                item(PING)
            )
            .is_ok()
        );
    }

    /// Matrix 25-27 (6.3): argument-level errors.
    #[test]
    fn metadata_argument_errors() {
        for (args, expected) in [
            (quote!("ping", nsfw = true, nsfw = false), "duplicate `#[command]` argument `nsfw`".to_owned()),
            (quote!("ping", dm_permission = false), "[META-05] `dm_permission` is deprecated by Discord and not supported; use `contexts = [Guild]` to keep a command out of DMs".to_owned()),
            (quote!("ping", nsfw = 1), "[CMD-07] `nsfw` takes `true` or `false`".to_owned()),
            (quote!("ping", permissions(set = A)), "[META-02] `permissions` applies to the base command of every path this handler declares; Discord keeps it only on the base command, so it has no per-path form".to_owned()),
        ] {
            assert_eq!(error(args.clone(), PING), expected, "{args}");
        }
        for planned in ["rate_limit", "flags", "apps"] {
            let ident = quote::format_ident!("{planned}");
            assert_eq!(
                error(quote!("ping", #ident), PING),
                format!("`#[command]` argument `{planned}` is planned and not supported yet")
            );
        }
    }

    /// Story 11.4 Matrix M16, M17: the locale arguments.
    #[test]
    fn locale_argument_errors() {
        if cfg!(feature = "i18n") {
            for (args, expected) in [
                (
                    quote!("ping", description = "d", locales = "a", locales = "b"),
                    "duplicate `#[command]` argument `locales`".to_owned(),
                ),
                (
                    quote!("ping", description = "d", locales = 5),
                    "`locales` takes a string literal".to_owned(),
                ),
                (
                    quote!("ping", description = "d", default_locale = "en"),
                    "[LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US`"
                        .to_owned(),
                ),
            ] {
                assert_eq!(error(args.clone(), PING), expected, "{args}");
            }
        } else {
            for name in ["locales", "default_locale"] {
                let ident = quote::format_ident!("{name}");
                assert_eq!(
                    error(quote!("ping", description = "d", #ident = "x"), PING),
                    format!("`{name}` needs the `i18n` feature of `noctis`")
                );
            }
        }
    }

    /// Story 6.10 Matrix 14-17: `response` and `auto_defer` values.
    #[test]
    fn response_and_auto_defer_values() {
        for (args, ephemeral) in [
            (quote!("ping", description = "d", response = public), false),
            (
                quote!("ping", description = "d", response = ephemeral),
                true,
            ),
            (quote!("ping", description = "d"), false),
        ] {
            let out = expand(args, item(PING)).unwrap().to_string();
            assert_eq!(
                out.contains("Ephemerality :: Ephemeral"),
                ephemeral,
                "{out}"
            );
        }
        assert_eq!(
            error(quote!("ping", response = update), PING),
            RESPONSE_UPDATE
        );
        for args in [
            quote!("ping", response = "ephemeral"),
            quote!("ping", response),
            quote!("ping", response = loud),
        ] {
            assert_eq!(error(args, PING), RESPONSE_VALUE);
        }
        for args in [
            quote!("ping", auto_defer = 1),
            quote!("ping", auto_defer = "false"),
            quote!("ping", auto_defer),
        ] {
            assert_eq!(error(args, PING), AUTO_DEFER_VALUE);
        }
        assert!(
            expand(
                quote!("ping", description = "d", auto_defer = true),
                item(PING)
            )
            .is_ok()
        );
    }

    /// Matrix 45: variant naming.
    #[test]
    fn variant_names() {
        for (word, expected) in [
            ("set-prefix", "SetPrefix"),
            ("a_b", "AB"),
            ("ğüncelle", "Ğüncelle"),
            ("straße", "Straße"),
            ("ßa", "SSa"),
            ("2fa", "_2fa"),
            ("self", "Self_"),
            ("汉字", "汉字"),
            ("ʼa", "ʼa"),
        ] {
            assert_eq!(variant_name(&[word]), expected, "{word}");
        }
        assert_eq!(variant_name(&[]), "Bare");
        assert_eq!(variant_name(&["set", "prefix"]), "SetPrefix");
        let out = expand(
            quote!("x (self|2fa)", description = "d"),
            item("async fn x(c: CommandCtx, p: XPath) -> HandlerResult<()> { Ok(()) }"),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("Self_"), "{out}");
        assert!(out.contains("# [allow (non_camel_case_types , clippy :: enum_variant_names)]"));
    }

    /// Matrix 8: an absent optional group gives `Bare`.
    #[test]
    fn optional_group_variants() {
        let paths = expand_paths("(a|b)? c");
        assert_eq!(variants("(a|b)? c", &paths).unwrap(), ["A", "B", "Bare"]);
    }

    fn expand_paths(pattern: &str) -> Vec<String> {
        rules::expand(pattern, &Limits::default()).unwrap()
    }

    /// Matrix 46: variant collision.
    #[test]
    fn variant_collision() {
        assert_eq!(
            error(
                quote!("x (a-b|a_b)", description = "d"),
                "async fn x(c: CommandCtx, p: XPath) -> HandlerResult<()> { Ok(()) }"
            ),
            "`#[command]` path variants collide: `x a-b` and `x a_b` both name `AB`; rename one alternative"
        );
    }

    /// Matrix 47: enum names.
    #[test]
    fn enum_names() {
        for (name, expected) in [
            ("Config", "ConfigPath"),
            ("SetPrefix", "SetPrefixPath"),
            ("r#Type", "TypePath"),
            ("Grüße", "GrüßePath"),
            ("_1", "_1Path"),
        ] {
            assert_eq!(enum_name(name), expected, "{name}");
        }
        let out = run(quote!(
            #[command("t (a|b)", description = "d")]
            struct Type;
        ));
        assert!(out.contains("enum TypePath"), "{out}");
        assert!(out.contains("\"Type\""), "{out}");
    }

    /// Matrix M2: the expansion tracks the default-locale file it read
    /// with `include_bytes!`.
    #[cfg(feature = "i18n")]
    #[test]
    fn includes_the_read_locale_file() {
        let out = run(quote! {
            #[command("mod ban", locales = "../noctis/tests/fixtures/i18n/cmd")]
            struct Ban;
        });
        // The macro resolves `locales` against the runtime
        // `CARGO_MANIFEST_DIR`, which can differ from the compile-time one
        // when the test binary is built in another checkout.
        let dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
        let file = std::fs::canonicalize(
            std::path::Path::new(&dir).join("../noctis/tests/fixtures/i18n/cmd/en-US.yaml"),
        )
        .unwrap();
        let expected = format!("include_bytes ! ({:?})", file.to_str().unwrap());
        assert!(out.contains(&expected), "{expected} in {out}");
    }
}
