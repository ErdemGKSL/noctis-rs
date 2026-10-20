//! `#[command]`: checks a Command Path or Pattern at expansion with
//! `noctis_rules::command` and turns an `async fn(ctx: CommandCtx[, path:
//! <Fn>Path][, options: O])` into a descriptor function returning a
//! `CommandHandler` (FR-7, FR-8, FR-10, FR-75, FR-77, AD-8, AD-16, AD-26).

use noctis_rules::command::{
    self as rules, CommandDecl, LocalizedText, validate_pattern_spacing, validate_tree,
};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, LitInt, LitStr, ReturnType, Safety, Type};
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::common::combine;
use crate::meta::{self, MetaArgs};

const MISSING_PATTERN: &str = "`#[command]` needs a Command Path or Pattern as its first argument: `#[command(\"ping\", description = \"…\")]`";
const NOT_A_LITERAL: &str = "`#[command]` needs its Command Path or Pattern as a string literal; check runtime-built patterns with `noctis::rules::command::{validate_pattern_spacing, expand, validate_tree}`";
const DESCRIPTION_LITERAL: &str = "`description` takes a string literal";
const CAP_LITERAL: &str =
    "`max_expansions` takes an integer literal, such as `max_expansions = 30`";
const CAP_ZERO: &str = "`max_expansions` must be at least 1";

/// `#[command]` arguments that later stories add.
const PLANNED: [&str; 4] = ["rate_limit", "flags", "apps", "locales"];

const EXPECTED: &str = "expected `description`, `max_expansions`, `ephemeral`, `opens_modal`, `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds`";

const OPENS_MODAL_RESPONDER: &str =
    "[INT-05] `opens_modal` handlers take `r: CommandResponder<OpensModal>` to send the modal";

/// The parsed `#[command(..)]` arguments.
struct Args {
    pattern: LitStr,
    description: Option<LitStr>,
    max_expansions: Option<usize>,
    ephemeral: bool,
    opens_modal: bool,
    meta: MetaArgs,
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

/// Checks a bare `ephemeral` flag: it takes no value.
pub(crate) fn parse_ephemeral(segment: &[TokenTree]) -> syn::Result<bool> {
    parse_flag(segment, "ephemeral")
}

/// Checks the bare flag `name`: it takes no value.
fn parse_flag(segment: &[TokenTree], name: &str) -> syn::Result<bool> {
    if segment.len() == 1 {
        Ok(true)
    } else {
        Err(syn::Error::new(
            span_of(segment),
            format!("`{name}` takes no value"),
        ))
    }
}

/// The `.__with_ephemerality(..)` call `ephemeral` appends.
pub(crate) fn ephemerality_call() -> TokenStream {
    quote! {
        .__with_ephemerality(::noctis::__private::core::Ephemerality::Ephemeral)
    }
}

/// Whether `ty` is a responder parameter: a path whose last segment is
/// `CommandResponder` or `Responder`.
pub(crate) fn is_responder(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            path.qself.is_none()
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "CommandResponder" || s.ident == "Responder")
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
        ephemeral: false,
        opens_modal: false,
        meta: MetaArgs::default(),
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
        match name.as_str() {
            "description" | "max_expansions" | "ephemeral" | "opens_modal" => {}
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
        if name == "ephemeral" {
            parsed.ephemeral = parse_ephemeral(&segment)?;
            continue;
        }
        if name == "opens_modal" {
            parsed.opens_modal = parse_flag(&segment, "opens_modal")?;
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

/// Checks the signature shape: a plain, non-generic `async fn`; `attr` is
/// the macro (`#[command]`) the messages name.
pub(crate) fn check_shape(item: &ItemFn, attr: &str) -> syn::Result<()> {
    let plain = format!("`{attr}` handlers must be a plain `async fn`");
    let plain = plain.as_str();
    let sig = &item.sig;
    if let Some(constness) = &sig.constness {
        return Err(syn::Error::new_spanned(constness, plain));
    }
    if let Safety::Unsafe(unsafety) = &sig.safety {
        return Err(syn::Error::new_spanned(unsafety, plain));
    }
    if let Safety::Safe(safety) = &sig.safety {
        return Err(syn::Error::new_spanned(safety, plain));
    }
    if let Some(abi) = &sig.abi {
        return Err(syn::Error::new_spanned(abi, plain));
    }
    if item.modifiers.require_empty().is_err() {
        return Err(syn::Error::new(sig.fn_token.span(), plain));
    }
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            sig.fn_token,
            format!("`{attr}` needs an `async fn`"),
        ));
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            format!("`{attr}` handlers cannot be generic"),
        ));
    }
    if let Some(variadic) = &sig.variadic {
        return Err(syn::Error::new_spanned(variadic, plain));
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

/// Steps 3-4: the spacing check, `expand` and `validate_tree`; every
/// violation, with the macro's help added.
fn check_pattern(args: &Args) -> Result<Vec<String>, Vec<Violation>> {
    let defaults = Limits::default();
    let mut cap = Limits::default();
    if let Some(n) = args.max_expansions {
        cap.pattern_expansions = n;
    }
    let pattern = args.pattern.value();
    let description = args.description.as_ref().map(LitStr::value);
    let mut violations = validate_pattern_spacing(&pattern, &defaults);
    let paths = match rules::expand(&pattern, &cap) {
        Ok(paths) => {
            let decls: Vec<CommandDecl<'_>> = paths
                .iter()
                .map(|path| {
                    let decl = CommandDecl::handler("", path);
                    match &description {
                        Some(d) => decl.description(LocalizedText::new(d)),
                        None => decl,
                    }
                })
                .collect();
            if let Err(tree) = validate_tree(&decls, &defaults) {
                violations.extend(tree);
            }
            Some(paths)
        }
        Err(errors) => {
            violations.extend(errors);
            None
        }
    };
    match paths {
        Some(paths) if violations.is_empty() => Ok(paths),
        _ => {
            for v in &mut violations {
                add_help(v, &pattern, &defaults);
            }
            Err(violations)
        }
    }
}

/// The pattern violations (on the description literal for CMD-04 when one
/// is given, else on the pattern literal), then META-03 (on the argument
/// name), as one error; `Ok` with the paths when there is none.
fn report(args: &Args, checked: Result<Vec<String>, Vec<Violation>>) -> syn::Result<Vec<String>> {
    let (paths, violations) = match checked {
        Ok(paths) => (Some(paths), Vec::new()),
        Err(violations) => (None, violations),
    };
    let mut errors: Vec<syn::Error> = violations
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
    paths.ok_or_else(|| syn::Error::new(args.pattern.span(), "`#[command]` pattern check failed"))
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

/// The name of the path enum of the handler fn `fn_name` (`r#` stripped).
fn enum_name(fn_name: &str) -> String {
    let name = fn_name.strip_prefix("r#").unwrap_or(fn_name);
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

/// What `check_params` found after the fixed parameters.
struct Params {
    /// The type of the `CommandResponder` parameter after the fixed ones.
    responder: Option<Type>,
    /// The trailing options parameter's type.
    options: Option<Type>,
}

/// Step 6: the parameter forms `(ctx[, path][, r: CommandResponder][,
/// options])`: the path parameter's type, then an optional responder, then
/// an optional options struct.
fn check_params(item: &ItemFn, pattern: &str, path_enum: Option<&str>) -> syn::Result<Params> {
    let sig = &item.sig;
    let (fixed, message) = match path_enum {
        None => (
            1,
            format!(
                "`#[command({pattern:?})]` handlers take `(ctx: CommandCtx[, r: CommandResponder][, options: O])` with `O: Options`, in this order"
            ),
        ),
        Some(name) => (
            2,
            format!(
                "`#[command({pattern:?})]` handlers take `(ctx: CommandCtx, path: {name}[, r: CommandResponder][, options: O])` with `O: Options`, in this order"
            ),
        ),
    };
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new_spanned(receiver, message));
    }
    let len = sig.inputs.len();
    let typed: Vec<&syn::PatType> = sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(t),
            FnArg::Receiver(_) => None,
        })
        .collect();
    if len < fixed || len > fixed + 2 || typed.len() != len {
        let span = if sig.inputs.is_empty() {
            sig.ident.span()
        } else {
            sig.inputs.span()
        };
        return Err(syn::Error::new(span, message));
    }
    if let (Some(name), Some(param)) = (path_enum, typed.get(1)) {
        let ok = match &*param.ty {
            Type::Path(path) => {
                path.qself.is_none()
                    && path
                        .path
                        .segments
                        .last()
                        .is_some_and(|s| s.ident == name && s.arguments.is_none())
            }
            _ => false,
        };
        if !ok {
            return Err(syn::Error::new_spanned(
                &param.ty,
                format!(
                    "the second parameter must be `{name}`, the enum `#[command]` generates for this pattern"
                ),
            ));
        }
    }
    match &typed[fixed..] {
        [] => Ok(Params {
            responder: None,
            options: None,
        }),
        [one] if is_responder(&one.ty) => Ok(Params {
            responder: Some((*one.ty).clone()),
            options: None,
        }),
        [one] => Ok(Params {
            responder: None,
            options: Some((*one.ty).clone()),
        }),
        [r, o] if is_responder(&r.ty) && !is_responder(&o.ty) => Ok(Params {
            responder: Some((*r.ty).clone()),
            options: Some((*o.ty).clone()),
        }),
        [_, o] => Err(syn::Error::new_spanned(&o.ty, message)),
        _ => Err(syn::Error::new(sig.inputs.span(), message)),
    }
}

/// The generated `<Fn>Path` enum and its impls.
fn path_enum(
    item: &ItemFn,
    enum_ident: &Ident,
    fn_name: &str,
    pattern: &str,
    paths: &[String],
    variants: &[Ident],
) -> TokenStream {
    let vis = &item.vis;
    let cfgs = item.attrs.iter().filter(|a| a.path().is_ident("cfg"));
    let doc = format!(" The Command Paths of `{fn_name}`: `{pattern}`.");
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
            pub const fn as_str(self) -> &'static str {
                match self {
                    #(Self::#variants => #paths,)*
                }
            }
        }

        #(#cfgs3)*
        impl ::noctis::__private::core::CommandPath for #enum_ident {
            const PATHS: &'static [&'static str] = &[#(#paths),*];

            fn from_path(path: &str) -> ::core::option::Option<Self> {
                match path {
                    #(#paths => ::core::option::Option::Some(Self::#variants),)*
                    _ => ::core::option::Option::None,
                }
            }

            fn as_str(self) -> &'static str {
                Self::as_str(self)
            }
        }
    }
}

/// The closure adapting `__noctis_command` to the constructor's uniform
/// shape `(ctx, [path,] responder, [options])`; a handler without a
/// responder parameter drops it.
fn adapter(path: bool, responder: bool, options: bool, span: Span) -> TokenStream {
    let ident = |name: &str| Ident::new(name, span);
    let mut params = vec![ident("__noctis_ctx")];
    let mut args = vec![ident("__noctis_ctx")];
    if path {
        params.push(ident("__noctis_path"));
        args.push(ident("__noctis_path"));
    }
    if responder {
        params.push(ident("__noctis_responder"));
        args.push(ident("__noctis_responder"));
    } else {
        params.push(ident("_"));
    }
    if options {
        params.push(ident("__noctis_options"));
        args.push(ident("__noctis_options"));
    }
    let inner = ident("__noctis_command");
    if responder {
        // Every parameter is passed through: the handler itself fits.
        return quote_spanned! {span=> #inner };
    }
    quote_spanned! {span=>
        |#(#params),*| #inner(#(#args),*)
    }
}

/// INT-05: the assertion that `opens_modal` and the responder's modal
/// marker match, and the marker the constructor's turbofish passes
/// (inferred from the responder parameter, `NoModal` without one).
fn modal_check(
    item: &ItemFn,
    responder: Option<&Type>,
    opens_modal: bool,
) -> syn::Result<(TokenStream, TokenStream)> {
    let assertion = match (responder, opens_modal) {
        (None, true) => {
            return Err(syn::Error::new_spanned(&item.sig, OPENS_MODAL_RESPONDER));
        }
        (None, false) => TokenStream::new(),
        (Some(ty), true) => quote_spanned! {ty.span()=>
            ::noctis::__private::core::__assert_opens_modal::<#ty>();
        },
        (Some(ty), false) => quote_spanned! {ty.span()=>
            ::noctis::__private::core::__assert_no_modal::<#ty>();
        },
    };
    let marker = if responder.is_some() {
        quote! { _ }
    } else {
        quote! { ::noctis::__private::core::NoModal }
    };
    Ok((assertion, marker))
}

/// The `CommandHandler` constructor path with its turbofish: `__new*` for
/// a pattern with a group (`path_enum`), `__new_plain*` without, and the
/// `*_with_options` form when the handler takes an options struct.
fn constructor(
    path_enum: Option<&Ident>,
    options: Option<&Type>,
    marker: &TokenStream,
    span: Span,
) -> TokenStream {
    match (path_enum, options) {
        (Some(e), None) => quote_spanned! {span=>
            ::noctis::__private::core::CommandHandler::__new::<#e, #marker, _, _, _>
        },
        (Some(e), Some(ty)) => quote_spanned! {span=>
            ::noctis::__private::core::CommandHandler::__new_with_options::<#e, #ty, #marker, _, _, _>
        },
        (None, None) => quote_spanned! {span=>
            ::noctis::__private::core::CommandHandler::__new_plain::<#marker, _, _, _>
        },
        (None, Some(ty)) => quote_spanned! {span=>
            ::noctis::__private::core::CommandHandler::__new_plain_with_options::<#ty, #marker, _, _, _>
        },
    }
}

/// Expands `#[command(args)] item`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = parse_args(args)?;
    let item: ItemFn = syn::parse2(item)?;
    check_shape(&item, "#[command]")?;
    let paths = report(&args, check_pattern(&args))?;
    let pattern = args.pattern.value();
    let name = item.sig.ident.clone();
    let fn_name = name.to_string();
    let fn_name = fn_name.strip_prefix("r#").unwrap_or(&fn_name).to_owned();
    let has_group = pattern.contains('(');
    let enum_ident = if has_group {
        let names = variants(&pattern, &paths)
            .map_err(|message| syn::Error::new(args.pattern.span(), message))?;
        let enum_ident = ident(&enum_name(&fn_name), name.span())?;
        let variants = names
            .iter()
            .map(|n| ident(n, args.pattern.span()))
            .collect::<syn::Result<Vec<_>>>()?;
        Some((enum_ident, variants))
    } else {
        None
    };
    let Params {
        responder: responder_ty,
        options,
    } = check_params(
        &item,
        &pattern,
        enum_ident.as_ref().map(|(e, _)| e.to_string()).as_deref(),
    )?;
    let (assertion, marker) = modal_check(&item, responder_ty.as_ref(), args.opens_modal)?;
    let responder = responder_ty.is_some();

    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        ..
    } = &item;
    let mut inner = sig.clone();
    inner.ident = Ident::new("__noctis_command", name.span());
    let output_span = match &sig.output {
        ReturnType::Default => sig.ident.span(),
        ReturnType::Type(_, ty) => ty.span(),
    };
    let description = args
        .description
        .as_ref()
        .map(LitStr::value)
        .unwrap_or_default();
    let inner = inner.into_token_stream();
    let adapter = adapter(
        enum_ident.is_some(),
        responder,
        options.is_some(),
        output_span,
    );
    let (generated, call) = if let Some((enum_ident, variants)) = &enum_ident {
        let generated = path_enum(&item, enum_ident, &fn_name, &pattern, &paths, variants);
        let new = constructor(Some(enum_ident), options.as_ref(), &marker, output_span);
        (
            generated,
            quote_spanned! {output_span=> #new(::core::concat!(::core::module_path!(), "::", #fn_name), #pattern, #description, #adapter) },
        )
    } else {
        let new = constructor(None, options.as_ref(), &marker, output_span);
        (
            TokenStream::new(),
            quote_spanned! {output_span=> #new(::core::concat!(::core::module_path!(), "::", #fn_name), &[#(#paths),*], #description, #adapter) },
        )
    };
    let call = if args.meta.is_empty() {
        call
    } else {
        let metadata = args.meta.tokens();
        quote! { #call.__with_metadata(#metadata) }
    };
    let call = if args.ephemeral {
        let ephemeral = ephemerality_call();
        quote! { #call #ephemeral }
    } else {
        call
    };
    Ok(quote! {
        #generated

        #(#attrs)*
        #vis fn #name() -> ::noctis::__private::core::CommandHandler {
            // A handler needs no `.await` to be `async` (the macro requires it).
            #[allow(clippy::unused_async)]
            #inner #block
            #assertion
            #call
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(src: &str) -> TokenStream {
        src.parse().unwrap()
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

    #[test]
    fn expands_a_plain_path() {
        let out = expand(
            quote!("ping", description = "d"),
            item(&format!("/// docs\npub {PING}")),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains("pub fn ping () -> :: noctis :: __private :: core :: CommandHandler"),
            "{out}"
        );
        assert!(out.contains("__new_plain"), "{out}");
        assert!(
            out.contains(
                "& [\"ping\"] , \"d\" , | __noctis_ctx , _ | __noctis_command (__noctis_ctx)"
            ),
            "{out}"
        );
        assert!(!out.contains("enum"), "{out}");
        assert!(!out.contains("__with_ephemerality"), "{out}");
        assert!(out.contains("doc"), "{out}");
    }

    #[test]
    fn expands_a_pattern() {
        let out = expand(
            quote!(
                "config (set|get) (prefix|welcome)",
                description = "Configure"
            ),
            item(&format!("pub(crate) {CONFIG}")),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains(
                "pub (crate) enum ConfigPath { # [doc = \" `/config set prefix`\"] SetPrefix ,"
            ),
            "{out}"
        );
        assert!(out.contains("GetWelcome"), "{out}");
        assert!(
            out.contains(
                "__new :: < ConfigPath , :: noctis :: __private :: core :: NoModal , _ , _ , _ >"
            ),
            "{out}"
        );
        assert!(
            out.contains("| __noctis_ctx , __noctis_path , _ | __noctis_command (__noctis_ctx , __noctis_path)"),
            "{out}"
        );
        assert!(
            out.contains("\"config (set|get) (prefix|welcome)\" , \"Configure\""),
            "{out}"
        );
    }

    #[test]
    fn cfg_attributes_reach_the_enum() {
        let out = expand(
            quote!("x (a|b)", description = "d"),
            item(
                "#[cfg(test)] async fn x(c: CommandCtx, p: XPath) -> HandlerResult<()> { Ok(()) }",
            ),
        )
        .unwrap()
        .to_string();
        assert_eq!(out.matches("# [cfg (test)]").count(), 4, "{out}");
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
                "unknown `#[command]` argument `foo`: expected `description`, `max_expansions`, `ephemeral`, `opens_modal`, `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds`",
            ),
            (
                quote!("ping", ephemeral = true),
                "`ephemeral` takes no value",
            ),
            (quote!("ping", ephemeral(x)), "`ephemeral` takes no value"),
            (
                quote!("ping", ephemeral, ephemeral),
                "duplicate `#[command]` argument `ephemeral`",
            ),
            (
                quote!("ping", opens_modal = true),
                "`opens_modal` takes no value",
            ),
            (
                quote!("ping", opens_modal, opens_modal),
                "duplicate `#[command]` argument `opens_modal`",
            ),
            (
                quote!("ping", default_member_permissions = X),
                "unknown `#[command]` argument `default_member_permissions`: expected `description`, `max_expansions`, `ephemeral`, `opens_modal`, `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds`",
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

    /// Matrix 41: signature shape.
    #[test]
    fn signature_errors() {
        let args = || quote!("ping", description = "d");
        for (src, expected) in [
            (
                "fn ping(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                "`#[command]` needs an `async fn`",
            ),
            (
                "const async fn ping(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                PLAIN,
            ),
            (
                "async unsafe fn ping(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                PLAIN,
            ),
            (
                "async extern \"C\" fn ping(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                PLAIN,
            ),
            (
                "async fn ping<T>(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                "`#[command]` handlers cannot be generic",
            ),
            (
                "async fn ping(ctx: CommandCtx) -> HandlerResult<()> where u8: Copy { Ok(()) }",
                "`#[command]` handlers cannot be generic",
            ),
            (
                "async fn ping(&self) -> HandlerResult<()> { Ok(()) }",
                PLAIN_PARAMS,
            ),
        ] {
            assert_eq!(error(args(), src), expected, "{src}");
        }
    }

    const PLAIN: &str = "`#[command]` handlers must be a plain `async fn`";

    /// Matrix 1, 2 and 28 (6.3): `__with_metadata` only with a metadata
    /// argument.
    #[test]
    fn metadata_is_appended_only_when_given() {
        let out = expand(quote!("ping", description = "d"), item(PING))
            .unwrap()
            .to_string();
        assert!(!out.contains("__with_metadata"), "{out}");
        let out = expand(
            quote!(
                "ping",
                description = "d",
                permissions = MANAGE_GUILD | BAN_MEMBERS,
                contexts = [Guild],
                nsfw = true
            ),
            item(PING),
        )
        .unwrap()
        .to_string()
        .replace(' ', "");
        assert!(
            out.contains("__noctis_command(__noctis_ctx)).__with_metadata({constM:::noctis::__private::core::CommandMetadata"),
            "{out}"
        );
        assert!(out.contains("from_bits_retain(36u64)"), "{out}");
        let out = expand(
            quote!(
                "config (set|get)",
                description = "d",
                permissions = MANAGE_GUILD
            ),
            item(CONFIG),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("__with_metadata"), "{out}");
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
        for planned in ["rate_limit", "flags", "apps", "locales"] {
            let ident = quote::format_ident!("{planned}");
            assert_eq!(
                error(quote!("ping", #ident), PING),
                format!("`#[command]` argument `{planned}` is planned and not supported yet")
            );
        }
    }

    const PLAIN_PARAMS: &str = "`#[command(\"ping\")]` handlers take `(ctx: CommandCtx[, r: CommandResponder][, options: O])` with `O: Options`, in this order";

    /// Matrix 42 and 43 (6.1) and 78 (6.2): parameter count and the path
    /// parameter.
    #[test]
    fn parameter_errors() {
        const PATTERN_PARAMS: &str = "`#[command(\"config (set|get)\")]` handlers take `(ctx: CommandCtx, path: ConfigPath[, r: CommandResponder][, options: O])` with `O: Options`, in this order";
        let ping = || quote!("ping", description = "d");
        let config = || quote!("config (set|get)", description = "d");
        for (args, src, expected) in [
            (
                ping(),
                "async fn ping() -> HandlerResult<()> { Ok(()) }",
                PLAIN_PARAMS,
            ),
            (
                ping(),
                "async fn ping(c: CommandCtx, x: W, y: u8) -> HandlerResult<()> { Ok(()) }",
                PLAIN_PARAMS,
            ),
            (
                ping(),
                "async fn ping(c: CommandCtx, o: W, r: CommandResponder) -> HandlerResult<()> { Ok(()) }",
                PLAIN_PARAMS,
            ),
            (
                ping(),
                "async fn ping(c: CommandCtx, r: CommandResponder, s: Responder<Command>) -> HandlerResult<()> { Ok(()) }",
                PLAIN_PARAMS,
            ),
            (
                ping(),
                "async fn ping(c: CommandCtx, r: CommandResponder, o: W, x: u8) -> HandlerResult<()> { Ok(()) }",
                PLAIN_PARAMS,
            ),
            (
                config(),
                "async fn config(c: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                PATTERN_PARAMS,
            ),
            (
                config(),
                "async fn config(c: CommandCtx, p: ConfigPath, x: W, y: u8) -> HandlerResult<()> { Ok(()) }",
                PATTERN_PARAMS,
            ),
            (
                config(),
                "async fn config(c: CommandCtx, p: String) -> HandlerResult<()> { Ok(()) }",
                "the second parameter must be `ConfigPath`, the enum `#[command]` generates for this pattern",
            ),
        ] {
            assert_eq!(error(args, src), expected, "{src}");
        }
    }

    /// Matrix 75-77: the options parameter picks the `*_with_options`
    /// constructor.
    #[test]
    fn options_parameter() {
        let out = expand(
            quote!("warn", description = "d"),
            item("async fn warn(c: CommandCtx, o: W) -> HandlerResult<()> { Ok(()) }"),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains(
                "__new_plain_with_options :: < W , :: noctis :: __private :: core :: NoModal , _ , _ , _ >"
            ),
            "{out}"
        );
        assert!(
            out.contains("| __noctis_ctx , _ , __noctis_options | __noctis_command (__noctis_ctx , __noctis_options)"),
            "{out}"
        );
        let out = expand(
            quote!("mod (ban|kick)", description = "d"),
            item("async fn r#mod(c: CommandCtx, p: ModPath, o: W) -> HandlerResult<()> { Ok(()) }"),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains(
                "__new_with_options :: < ModPath , W , :: noctis :: __private :: core :: NoModal , _ , _ , _ >"
            ),
            "{out}"
        );
        let out = expand(quote!("ping", description = "d"), item(PING))
            .unwrap()
            .to_string();
        assert!(!out.contains("with_options"), "{out}");
    }

    /// Story 6.5 Matrix 41 and 44: `ephemeral` and the responder parameter.
    #[test]
    fn ephemeral_and_responder() {
        let out = expand(quote!("ping", description = "d", ephemeral), item(PING))
            .unwrap()
            .to_string()
            .replace(' ', "");
        assert!(
            out.contains(
                ".__with_ephemerality(::noctis::__private::core::Ephemerality::Ephemeral)"
            ),
            "{out}"
        );
        for (pattern, src, adapter) in [
            (
                quote!("ping", description = "d"),
                "async fn ping(c: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> { todo!() }",
                "\"d\",__noctis_command)",
            ),
            (
                quote!("ping", description = "d"),
                "async fn ping(c: CommandCtx, r: noctis::Responder<Command>, o: W) -> HandlerResult<Responded> { todo!() }",
                "__new_plain_with_options::<W,_,_,_,_>(::core::concat!(::core::module_path!(),\"::\",\"ping\"),&[\"ping\"],\"d\",__noctis_command)",
            ),
            (
                quote!("config (set|get)", description = "d"),
                "async fn config(c: CommandCtx, p: ConfigPath, r: CommandResponder, o: W) -> HandlerResult<Responded> { todo!() }",
                "\"d\",__noctis_command)",
            ),
        ] {
            let out = expand(pattern, item(src))
                .unwrap()
                .to_string()
                .replace(' ', "");
            assert!(out.contains(adapter), "{out}");
        }
    }

    /// Story 6.6 Matrix 22: `opens_modal` and the responder's modal
    /// marker are asserted to match (INT-05).
    #[test]
    fn opens_modal_assertions() {
        let modal = || quote!("feedback", description = "d", opens_modal);
        let out = expand(
            modal(),
            item("async fn feedback(c: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded> { todo!() }"),
        )
        .unwrap()
        .to_string()
        .replace(' ', "");
        assert!(
            out.contains(
                "::noctis::__private::core::__assert_opens_modal::<CommandResponder<OpensModal>>();"
            ),
            "{out}"
        );
        assert!(out.contains("__new_plain::<_,_,_,_>"), "{out}");
        let out = expand(
            quote!("ping", description = "d"),
            item("async fn ping(c: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> { todo!() }"),
        )
        .unwrap()
        .to_string()
        .replace(' ', "");
        assert!(
            out.contains("::noctis::__private::core::__assert_no_modal::<CommandResponder>();"),
            "{out}"
        );
        let out = expand(quote!("ping", description = "d"), item(PING))
            .unwrap()
            .to_string()
            .replace(' ', "");
        assert!(!out.contains("__assert"), "{out}");
        assert!(
            out.contains("__new_plain::<::noctis::__private::core::NoModal,_,_,_>"),
            "{out}"
        );
        assert_eq!(
            error(
                modal(),
                "async fn feedback(c: CommandCtx) -> HandlerResult<Responded> { todo!() }"
            ),
            OPENS_MODAL_RESPONDER
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

    /// A qualified `<Fn>Path` parameter type is accepted.
    #[test]
    fn accepts_a_qualified_path_type() {
        for ty in ["crate::m::ConfigPath", "self::ConfigPath"] {
            let src = format!(
                "async fn config(c: CommandCtx, p: {ty}) -> HandlerResult<()> {{ Ok(()) }}"
            );
            assert!(
                expand(quote!("config (set|get)", description = "d"), item(&src)).is_ok(),
                "{ty}"
            );
        }
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
        for (fn_name, expected) in [
            ("config", "ConfigPath"),
            ("set_prefix", "SetPrefixPath"),
            ("r#type", "TypePath"),
            ("grüße", "GrüßePath"),
            ("_1", "_1Path"),
        ] {
            assert_eq!(enum_name(fn_name), expected, "{fn_name}");
        }
        let out = expand(
            quote!("t (a|b)", description = "d"),
            item("async fn r#type(c: CommandCtx, p: TypePath) -> HandlerResult<()> { Ok(()) }"),
        )
        .unwrap()
        .to_string();
        assert!(out.contains("enum TypePath"), "{out}");
        assert!(out.contains("\"type\""), "{out}");
    }
}
