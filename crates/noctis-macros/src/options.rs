//! `#[derive(Options)]`: a struct with named fields as a chat-input
//! command's typed options (FR-10, FR-77, AD-3, AD-4, AD-16).
//!
//! Each field's `#[option(..)]` literals are lowered into one
//! `noctis_rules::command::OptionDecl` per possible option type and checked
//! with `validate_options`:
//!
//! - a violation that every option type gives (with the same text) does not
//!   depend on the field's Rust type and is a compile error at expansion;
//! - every other violation becomes its own free `const _` item that panics
//!   with the violation's text when the field's `CommandOption::KIND` is one
//!   of the types that give it (`E0080 evaluation panicked: …`).
//!
//! The array rules (CMD-13, OPT-01, CMD-06) come from one `validate_options`
//! over every field; OPT-08 and OPT-10 need the field type's choices and get
//! dedicated `const _` items.

use noctis_rules::command::{
    CHANNEL_TYPES, NumericValue, OptionDecl, OptionType, channel_type, validate_options,
};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Literal, Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, ExprPath, Field, Fields, GenericArgument, Ident,
    Lit, LitStr, Meta, PathArguments, Token, Type, UnOp,
};

use crate::common::combine;

const ARGS: &str = "expected `description`, `rename`, `min_value`, `max_value`, `min_length`, `max_length`, `channel_types`, `autocomplete`, `file_types` or `validate`";

/// A `min_value` / `max_value` literal.
#[derive(Clone, Copy)]
enum BoundLit {
    Int(i64),
    Float(f64),
}

impl BoundLit {
    fn numeric(self) -> NumericValue {
        match self {
            Self::Int(v) => NumericValue::Int(v),
            Self::Float(v) => NumericValue::Float(v),
        }
    }

    fn tokens(self) -> TokenStream {
        let rules = quote!(::noctis::__private::rules::command::NumericValue);
        match self {
            Self::Int(v) => {
                let lit = Literal::i64_unsuffixed(v);
                quote!(#rules::Int(#lit))
            }
            Self::Float(v) => {
                let lit = Literal::f64_unsuffixed(v.abs());
                if v.is_sign_negative() {
                    quote!(#rules::Float(-#lit))
                } else {
                    quote!(#rules::Float(#lit))
                }
            }
        }
    }
}

/// One field's parsed `#[option(..)]` arguments.
#[derive(Default)]
struct Args {
    description: Option<LitStr>,
    rename: Option<LitStr>,
    min_value: Option<BoundLit>,
    max_value: Option<BoundLit>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    channel_types: Option<Vec<u8>>,
    autocomplete: Option<ExprPath>,
    file_types: Option<Vec<String>>,
    validate: Option<ExprPath>,
}

/// A parsed field.
struct OptField<'a> {
    ident: &'a Ident,
    /// The option type (`T` of `Option<T>` for an optional field).
    ty: &'a Type,
    required: bool,
    name: String,
    attr: Option<&'a Attribute>,
    args: Args,
}

impl OptField<'_> {
    /// The span of the field's `#[option]` attribute, or of its ident.
    fn attr_span(&self) -> Span {
        self.attr.map_or_else(|| self.ident.span(), Spanned::span)
    }

    fn decl<'d>(
        &'d self,
        kind: OptionType,
        description: &'d str,
        file_types: &'d [&'d str],
    ) -> OptionDecl<'d> {
        let a = &self.args;
        let mut decl = OptionDecl::new(&self.name, kind, description)
            .required(self.required)
            .autocomplete(a.autocomplete.is_some())
            .file_types(file_types);
        if let Some(v) = a.min_value {
            decl = decl.min_value(v.numeric());
        }
        if let Some(v) = a.max_value {
            decl = decl.max_value(v.numeric());
        }
        if let Some(v) = a.min_length {
            decl = decl.min_length(v);
        }
        if let Some(v) = a.max_length {
            decl = decl.max_length(v);
        }
        if let Some(v) = &a.channel_types {
            decl = decl.channel_types(v);
        }
        decl
    }

    fn description(&self) -> String {
        self.args
            .description
            .as_ref()
            .map(LitStr::value)
            .unwrap_or_default()
    }

    /// The label violation paths start with: the name, or `options[i]`.
    fn label(&self, index: usize) -> String {
        if self.name.is_empty() {
            format!("options[{index}]")
        } else {
            self.name.clone()
        }
    }
}

/// `T` when `ty` is `Option<T>` (last path segment `Option`, one type
/// argument).
fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let last = path.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.iter().collect::<Vec<_>>().as_slice() {
        [GenericArgument::Type(inner)] => Some(inner),
        _ => None,
    }
}

fn string_lit(name: &str, expr: &Expr) -> syn::Result<LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s.clone()),
        other => Err(syn::Error::new_spanned(
            other,
            format!("`{name}` takes a string literal"),
        )),
    }
}

fn length_lit(name: &str, expr: &Expr) -> syn::Result<usize> {
    let err = || {
        syn::Error::new_spanned(
            expr,
            format!("`{name}` takes an unsuffixed integer literal"),
        )
    };
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) if i.suffix().is_empty() => i.base10_parse().map_err(|_| err()),
        _ => Err(err()),
    }
}

fn bound_lit(name: &str, expr: &Expr) -> syn::Result<BoundLit> {
    let err = || {
        syn::Error::new_spanned(
            expr,
            format!("`{name}` takes an unsuffixed integer or float literal, optionally negated"),
        )
    };
    let (negative, lit) = match expr {
        Expr::Unary(u) if matches!(u.op, UnOp::Neg(_)) => (true, &*u.expr),
        other => (false, other),
    };
    let Expr::Lit(ExprLit { lit, .. }) = lit else {
        return Err(err());
    };
    match lit {
        Lit::Int(i) if i.suffix().is_empty() => {
            let digits: i128 = i.base10_digits().parse().map_err(|_| {
                syn::Error::new_spanned(expr, format!("`{name}` is outside the i64 range"))
            })?;
            let value = if negative { -digits } else { digits };
            i64::try_from(value).map(BoundLit::Int).map_err(|_| {
                syn::Error::new_spanned(expr, format!("`{name}` is outside the i64 range"))
            })
        }
        Lit::Float(f) if f.suffix().is_empty() => {
            let value: f64 = f.base10_parse().map_err(|_| err())?;
            Ok(BoundLit::Float(if negative { -value } else { value }))
        }
        _ => Err(err()),
    }
}

fn path_arg(name: &str, expr: &Expr) -> syn::Result<ExprPath> {
    match expr {
        Expr::Path(p) => Ok(p.clone()),
        other => Err(syn::Error::new_spanned(
            other,
            format!("`{name}` takes a function path"),
        )),
    }
}

fn file_types_lit(expr: &Expr) -> syn::Result<Vec<String>> {
    let err = || syn::Error::new_spanned(expr, "`file_types` takes an array of string literals");
    let Expr::Array(array) = expr else {
        return Err(err());
    };
    array
        .elems
        .iter()
        .map(|e| match e {
            Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) => Ok(s.value()),
            _ => Err(err()),
        })
        .collect()
}

fn channel_types_lit(expr: &Expr) -> syn::Result<Vec<u8>> {
    let Expr::Array(array) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            "`channel_types` lists ChannelType variants such as `[GuildText]`",
        ));
    };
    if array.elems.is_empty() {
        return Err(syn::Error::new_spanned(
            expr,
            "`channel_types` lists at least one channel type",
        ));
    }
    let names: Vec<&str> = CHANNEL_TYPES.iter().map(|(n, _)| *n).collect();
    array
        .elems
        .iter()
        .map(|e| {
            let ident = match e {
                Expr::Path(p) if p.qself.is_none() => p.path.segments.last().map(|s| &s.ident),
                _ => None,
            };
            let Some(ident) = ident else {
                return Err(syn::Error::new_spanned(
                    e,
                    "`channel_types` lists ChannelType variants such as `[GuildText]`",
                ));
            };
            channel_type(&ident.to_string()).ok_or_else(|| {
                syn::Error::new_spanned(
                    e,
                    format!(
                        "unknown channel type `{ident}`; expected one of {}",
                        names.join(", ")
                    ),
                )
            })
        })
        .collect()
}

/// Parses one `#[option(..)]` attribute.
fn parse_args(attr: &Attribute) -> syn::Result<Args> {
    let metas = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let mut args = Args::default();
    let mut seen: Vec<String> = Vec::new();
    for meta in metas {
        let Some(name) = meta.path().get_ident().map(ToString::to_string) else {
            return Err(syn::Error::new_spanned(
                meta.path(),
                format!("unknown `#[option]` argument: {ARGS}"),
            ));
        };
        let known = [
            "description",
            "rename",
            "min_value",
            "max_value",
            "min_length",
            "max_length",
            "channel_types",
            "autocomplete",
            "file_types",
            "validate",
        ];
        if !known.contains(&name.as_str()) {
            let message = match name.as_str() {
                "min" => "unknown `#[option]` argument `min`: use `min_value`".to_owned(),
                "max" => "unknown `#[option]` argument `max`: use `max_value`".to_owned(),
                other => format!("unknown `#[option]` argument `{other}`: {ARGS}"),
            };
            return Err(syn::Error::new_spanned(meta.path(), message));
        }
        if seen.contains(&name) {
            return Err(syn::Error::new_spanned(
                meta.path(),
                format!("duplicate `#[option]` argument `{name}`"),
            ));
        }
        seen.push(name.clone());
        let Meta::NameValue(nv) = &meta else {
            return Err(syn::Error::new_spanned(
                &meta,
                format!("`{name}` takes a value: `{name} = …`"),
            ));
        };
        let expr = &nv.value;
        match name.as_str() {
            "description" => args.description = Some(string_lit(&name, expr)?),
            "rename" => args.rename = Some(string_lit(&name, expr)?),
            "min_value" => args.min_value = Some(bound_lit(&name, expr)?),
            "max_value" => args.max_value = Some(bound_lit(&name, expr)?),
            "min_length" => args.min_length = Some(length_lit(&name, expr)?),
            "max_length" => args.max_length = Some(length_lit(&name, expr)?),
            "channel_types" => args.channel_types = Some(channel_types_lit(expr)?),
            "autocomplete" => args.autocomplete = Some(path_arg(&name, expr)?),
            "file_types" => args.file_types = Some(file_types_lit(expr)?),
            _ => args.validate = Some(path_arg(&name, expr)?),
        }
    }
    Ok(args)
}

fn parse_field(field: &Field) -> syn::Result<OptField<'_>> {
    let mut attrs = field.attrs.iter().filter(|a| a.path().is_ident("option"));
    let attr = attrs.next();
    if let Some(second) = attrs.next() {
        return Err(syn::Error::new_spanned(
            second,
            "a field takes one #[option(..)] attribute",
        ));
    }
    let args = attr.map(parse_args).transpose()?.unwrap_or_default();
    let Some(ident) = field.ident.as_ref() else {
        return Err(syn::Error::new_spanned(
            field,
            "`derive(Options)` takes a struct with named fields",
        ));
    };
    let (ty, required) = match option_inner(&field.ty) {
        Some(inner) => (inner, false),
        None => (&field.ty, true),
    };
    let name = args
        .rename
        .as_ref()
        .map_or_else(|| ident.unraw().to_string(), LitStr::value);
    Ok(OptField {
        ident,
        ty,
        required,
        name,
        attr,
        args,
    })
}

/// A type-dependent check: a violation text and the option types giving it.
struct ConstCheck {
    span: Span,
    kinds: Vec<OptionType>,
    text: String,
}

/// The per-field checks: expansion errors and type-dependent const checks.
fn check_field(
    index: usize,
    field: &OptField<'_>,
    errors: &mut Vec<syn::Error>,
    consts: &mut Vec<ConstCheck>,
) {
    let limits = Limits::default();
    let file_types: Vec<&str> = field
        .args
        .file_types
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let missing = field.args.description.is_none();
    let description = field.description();
    let mut seen: Vec<(String, Violation, Vec<OptionType>)> = Vec::new();
    for kind in OptionType::ALL {
        let decl = field.decl(kind, &description, &file_types);
        let Err(violations) = validate_options(&[decl], &limits) else {
            continue;
        };
        for mut v in violations {
            if field.name.is_empty()
                && let Some(rest) = v.path.strip_prefix("options[0]")
            {
                v.path = format!("options[{index}]{rest}");
            }
            if missing && v.rule == RuleId::CMD_04 {
                v = v.with_help(format!(
                    "add `#[option(description = \"…\")]` with 1 to {} characters (doc comments are rustdoc only)",
                    limits.command_description_len
                ));
            }
            let text = v.to_string();
            match seen.iter_mut().find(|(t, _, _)| *t == text) {
                Some((_, _, kinds)) => kinds.push(kind),
                None => seen.push((text, v, vec![kind])),
            }
        }
    }
    for (text, v, kinds) in seen {
        if kinds.len() == OptionType::ALL.len() {
            let span = match v.rule {
                RuleId::CMD_04 => field
                    .args
                    .description
                    .as_ref()
                    .map_or_else(|| field.ident.span(), LitStr::span),
                RuleId::CMD_01 | RuleId::CMD_02 | RuleId::CMD_03 => field
                    .args
                    .rename
                    .as_ref()
                    .map_or_else(|| field.ident.span(), LitStr::span),
                _ => field.attr_span(),
            };
            errors.push(syn::Error::new(span, text));
        } else {
            consts.push(ConstCheck {
                span: field.attr_span(),
                kinds,
                text,
            });
        }
    }
}

/// CMD-13, OPT-01 and CMD-06 over every field.
fn check_array(name: &Ident, fields: &[OptField<'_>], errors: &mut Vec<syn::Error>) {
    let decls: Vec<OptionDecl<'_>> = fields
        .iter()
        .map(|f| OptionDecl::new(&f.name, OptionType::String, "d").required(f.required))
        .collect();
    let Err(violations) = validate_options(&decls, &Limits::default()) else {
        return;
    };
    let mut first_optional = false;
    let mut late_required = fields.iter().enumerate().filter_map(|(i, f)| {
        if !f.required {
            first_optional = true;
        }
        (first_optional && f.required).then_some(i)
    });
    for v in violations {
        let span = match v.rule {
            RuleId::CMD_13 => name.span(),
            RuleId::OPT_01 => match late_required.next() {
                Some(i) => fields[i].ident.span(),
                None => name.span(),
            },
            RuleId::CMD_06 => v
                .notes
                .first()
                .and_then(|n| n.strip_prefix("declared at positions "))
                .and_then(|list| list.split(", ").nth(1))
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| fields.get(i))
                .map_or_else(|| name.span(), |f| f.ident.span()),
            _ => continue,
        };
        errors.push(syn::Error::new(span, v.to_string()));
    }
}

/// `OptionType::K` as a path.
fn kind_path(kind: OptionType) -> TokenStream {
    let variant = Ident::new(&format!("{kind:?}"), Span::call_site());
    quote!(::noctis::__private::rules::command::OptionType::#variant)
}

/// A string literal usable as a `panic!` format string.
fn panic_lit(text: &str) -> Literal {
    Literal::string(&text.replace('{', "{{").replace('}', "}}"))
}

fn const_checks(fields: &[OptField<'_>], consts: &[(usize, ConstCheck)]) -> TokenStream {
    let opt = quote!(::noctis::__private::core::CommandOption);
    let mut out = TokenStream::new();
    for (i, check) in consts {
        let ty = fields[*i].ty;
        let kinds = check.kinds.iter().map(|k| kind_path(*k));
        let text = panic_lit(&check.text);
        out.extend(quote_spanned! {check.span=>
            const _: () = match <#ty as #opt>::KIND {
                #(#kinds)|* => ::core::panic!(#text),
                _ => {}
            };
        });
    }
    let limits = Limits::default();
    for (index, field) in fields.iter().enumerate() {
        let ty = field.ty;
        let span = field.attr_span();
        let p = field.label(index);
        let over = Violation::new(
            RuleId::OPT_08,
            format!("{p}.choices"),
            Bound::AtMost(limits.option_choices),
            Actual::Text(format!("more than {}", limits.option_choices)),
        )
        .with_note(format!(
            "`{}` has more variants than a command option takes",
            ty.to_token_stream().to_string().replace(' ', "")
        ));
        let max = limits.option_choices;
        let over = panic_lit(&over.to_string());
        out.extend(quote_spanned! {span=>
            const _: () = if <#ty as #opt>::CHOICES.len() > #max {
                ::core::panic!(#over)
            };
        });
        if let Some(path) = &field.args.autocomplete {
            let both = Violation::new(
                RuleId::OPT_10,
                p.as_str(),
                Bound::Describe("choices or autocomplete, not both"),
                Actual::Text(p.clone()),
            );
            let both = panic_lit(&both.to_string());
            out.extend(quote_spanned! {span=>
                const _: () = if !<#ty as #opt>::CHOICES.is_empty() {
                    ::core::panic!(#both)
                };
            });
            out.extend(quote_spanned! {path.span()=>
                const _: () = {
                    let _ = #path;
                };
            });
        }
    }
    out
}

fn decl_tokens(field: &OptField<'_>) -> TokenStream {
    let opt = quote!(::noctis::__private::core::CommandOption);
    let rules = quote!(::noctis::__private::rules::command);
    let ty = field.ty;
    let name = &field.name;
    let description = field.description();
    let required = field.required;
    let a = &field.args;
    let mut decl = quote! {
        #rules::OptionDecl::new(#name, <#ty as #opt>::KIND, #description)
            .required(#required)
            .choices(<#ty as #opt>::CHOICES)
    };
    if let Some(v) = a.min_value {
        let v = v.tokens();
        decl.extend(quote!(.min_value(#v)));
    }
    if let Some(v) = a.max_value {
        let v = v.tokens();
        decl.extend(quote!(.max_value(#v)));
    }
    if let Some(v) = a.min_length {
        decl.extend(quote!(.min_length(#v)));
    }
    if let Some(v) = a.max_length {
        decl.extend(quote!(.max_length(#v)));
    }
    if let Some(v) = &a.channel_types {
        decl.extend(quote!(.channel_types(&[#(#v),*])));
    }
    if a.autocomplete.is_some() {
        decl.extend(quote!(.autocomplete(true)));
    }
    if let Some(v) = &a.file_types {
        decl.extend(quote!(.file_types(&[#(#v),*])));
    }
    decl
}

fn validator_tokens(field: &OptField<'_>) -> Option<TokenStream> {
    let path = field.args.validate.as_ref()?;
    let core = quote!(::noctis::__private::core);
    let ty = field.ty;
    let ident = field.ident;
    let name = &field.name;
    let f = quote_spanned! {path.span()=>
        let __noctis_validate: fn(&#ty, &#core::CommandCtx) -> ::core::result::Result<(), ::noctis::__private::rules::MessageKey> = #path;
    };
    let call = quote! {
        __noctis_validate(__noctis_value, ctx)
            .map_err(|key| #core::InvalidOption::__new(#name, key))?;
    };
    Some(if field.required {
        quote! {{
            #f
            let __noctis_value = &self.#ident;
            #call
        }}
    } else {
        quote! {{
            #f
            if let ::core::option::Option::Some(__noctis_value) = self.#ident.as_ref() {
                #call
            }
        }}
    })
}

/// Expands `#[derive(Options)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => return Err(not_named(name)),
        },
        _ => return Err(not_named(name)),
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "an Options struct cannot be generic",
        ));
    }
    let mut errors = Vec::new();
    let mut parsed = Vec::new();
    let mut all_parsed = true;
    for field in fields {
        match parse_field(field) {
            Ok(f) => parsed.push(f),
            Err(e) => {
                all_parsed = false;
                errors.push(e);
            }
        }
    }
    let mut consts = Vec::new();
    for (index, field) in parsed.iter().enumerate() {
        let mut field_consts = Vec::new();
        check_field(index, field, &mut errors, &mut field_consts);
        consts.extend(field_consts.into_iter().map(|c| (index, c)));
    }
    if all_parsed {
        check_array(name, &parsed, &mut errors);
    }
    let checks = const_checks(&parsed, &consts);
    if let Err(e) = combine(errors) {
        let e = e.into_compile_error();
        return Ok(quote!(#e #checks));
    }

    let core = quote!(::noctis::__private::core);
    let decls = parsed.iter().map(decl_tokens);
    let inits = parsed.iter().map(|f| {
        let ident = f.ident;
        let ty = f.ty;
        let name = &f.name;
        let helper = if f.required {
            quote!(required)
        } else {
            quote!(optional)
        };
        quote!(#ident: #core::__options::#helper::<#ty>(input, #name)?)
    });
    let validators = parsed.iter().filter_map(validator_tokens);
    Ok(quote! {
        #checks

        #[automatically_derived]
        impl #core::Options for #name {
            const OPTIONS: &'static [::noctis::__private::rules::command::OptionDecl<'static>] = &[
                #(#decls),*
            ];

            fn from_input(
                input: &#core::CommandInput,
            ) -> ::core::result::Result<Self, #core::OptionError> {
                let _ = input;
                ::core::result::Result::Ok(Self { #(#inits),* })
            }

            fn validate(
                &self,
                ctx: &#core::CommandCtx,
            ) -> ::core::result::Result<(), #core::InvalidOption> {
                let _ = ctx;
                #(#validators)*
                ::core::result::Result::Ok(())
            }
        }
    })
}

fn not_named(name: &Ident) -> syn::Error {
    syn::Error::new_spanned(name, "`derive(Options)` takes a struct with named fields")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: TokenStream) -> String {
        let input: DeriveInput = syn::parse2(input).unwrap_or_else(|e| panic!("{e}"));
        match expand(&input) {
            Ok(t) => t.to_string(),
            Err(e) => e.into_compile_error().to_string(),
        }
    }

    /// Every expansion-time error message.
    fn errors(input: TokenStream) -> Vec<String> {
        let input: DeriveInput = syn::parse2(input).unwrap_or_else(|e| panic!("{e}"));
        let mut parsed = Vec::new();
        let mut errors = Vec::new();
        let Data::Struct(data) = &input.data else {
            return expand(&input)
                .err()
                .into_iter()
                .flatten()
                .map(|e| e.to_string())
                .collect();
        };
        let Fields::Named(named) = &data.fields else {
            return expand(&input)
                .err()
                .into_iter()
                .flatten()
                .map(|e| e.to_string())
                .collect();
        };
        if !input.generics.params.is_empty() {
            return expand(&input)
                .err()
                .into_iter()
                .flatten()
                .map(|e| e.to_string())
                .collect();
        }
        let mut all = true;
        for f in &named.named {
            match parse_field(f) {
                Ok(f) => parsed.push(f),
                Err(e) => {
                    all = false;
                    errors.push(e);
                }
            }
        }
        let mut consts = Vec::new();
        for (i, f) in parsed.iter().enumerate() {
            check_field(i, f, &mut errors, &mut consts);
        }
        if all {
            check_array(&input.ident, &parsed, &mut errors);
        }
        errors.iter().map(ToString::to_string).collect()
    }

    /// Every `const _` check text of one field.
    fn const_texts(input: TokenStream) -> Vec<(Vec<OptionType>, String)> {
        let input: DeriveInput = syn::parse2(input).unwrap_or_else(|e| panic!("{e}"));
        let Data::Struct(data) = &input.data else {
            panic!("not a struct");
        };
        let mut out = Vec::new();
        for (i, f) in data.fields.iter().enumerate() {
            let f = parse_field(f).unwrap_or_else(|e| panic!("{e}"));
            let mut consts = Vec::new();
            check_field(i, &f, &mut Vec::new(), &mut consts);
            out.extend(consts.into_iter().map(|c| (c.kinds, c.text)));
        }
        out
    }

    #[test]
    fn happy_struct() {
        let out = run(quote! {
            struct W {
                #[option(description = "Who")]
                member: ResolvedUser,
                #[option(description = "How long", min_value = 1, max_value = 30)]
                days: Option<i64>,
                #[option(description = "Why", validate = check)]
                reason: Option<String>,
            }
        });
        assert!(
            out.contains("impl :: noctis :: __private :: core :: Options for W"),
            "{out}"
        );
        assert!(
            out.contains("OptionDecl :: new (\"member\" , < ResolvedUser as"),
            "{out}"
        );
        assert!(out.contains(". required (true)"), "{out}");
        assert!(
            out.contains("OptionDecl :: new (\"days\" , < i64 as"),
            "{out}"
        );
        assert!(out.contains(". min_value (:: noctis :: __private :: rules :: command :: NumericValue :: Int (1))"), "{out}");
        assert!(
            out.contains("required :: < ResolvedUser > (input , \"member\")"),
            "{out}"
        );
        assert!(
            out.contains("optional :: < i64 > (input , \"days\")"),
            "{out}"
        );
        assert!(out.contains("= check ;"), "{out}");
        assert!(!out.contains("compile_error"), "{out}");
    }

    #[test]
    fn empty_struct() {
        let out = run(quote!(
            struct E {}
        ));
        assert!(out.contains("OptionDecl < 'static >] = & []"), "{out}");
    }

    #[test]
    fn shape_errors() {
        for input in [
            quote!(
                struct T(i64);
            ),
            quote!(
                struct U;
            ),
            quote!(
                enum E {
                    A,
                }
            ),
            quote!(
                union X {
                    a: i64,
                }
            ),
        ] {
            assert!(run(input).contains("`derive(Options)` takes a struct with named fields"));
        }
        for input in [
            quote!(
                struct O<T> {
                    a: T,
                }
            ),
            quote!(
                struct O<'a> {
                    a: &'a str,
                }
            ),
            quote!(
                struct O
                where
                    u8: Copy,
                {
                    a: i64,
                }
            ),
        ] {
            assert!(run(input).contains("an Options struct cannot be generic"));
        }
    }

    #[test]
    fn argument_errors() {
        let cases: Vec<(TokenStream, String)> = vec![
            (
                quote!(foo = 1),
                format!("unknown `#[option]` argument `foo`: {ARGS}"),
            ),
            (
                quote!(description = "a", description = "b"),
                "duplicate `#[option]` argument `description`".into(),
            ),
            (
                quote!(min = 1),
                "unknown `#[option]` argument `min`: use `min_value`".into(),
            ),
            (
                quote!(max = 1),
                "unknown `#[option]` argument `max`: use `max_value`".into(),
            ),
            (
                quote!(description = concat!("a")),
                "`description` takes a string literal".into(),
            ),
            (quote!(rename = 1), "`rename` takes a string literal".into()),
            (
                quote!(min_length = -1),
                "`min_length` takes an unsuffixed integer literal".into(),
            ),
            (
                quote!(max_length = 1u8),
                "`max_length` takes an unsuffixed integer literal".into(),
            ),
            (
                quote!(min_value = "1"),
                "`min_value` takes an unsuffixed integer or float literal, optionally negated"
                    .into(),
            ),
            (
                quote!(max_value = 1i64),
                "`max_value` takes an unsuffixed integer or float literal, optionally negated"
                    .into(),
            ),
            (
                quote!(file_types = "x"),
                "`file_types` takes an array of string literals".into(),
            ),
            (
                quote!(channel_types = GuildText),
                "`channel_types` lists ChannelType variants such as `[GuildText]`".into(),
            ),
            (
                quote!(min_value = 9223372036854775808),
                "`min_value` is outside the i64 range".into(),
            ),
            (
                quote!(channel_types = [GuildTxt]),
                format!(
                    "unknown channel type `GuildTxt`; expected one of {}",
                    CHANNEL_TYPES
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            (
                quote!(channel_types = []),
                "`channel_types` lists at least one channel type".into(),
            ),
        ];
        for (args, expected) in cases {
            let errs = errors(quote! {
                struct S {
                    #[option(#args)]
                    a: i64,
                }
            });
            assert_eq!(errs, [expected], "{args}");
        }
        let errs = errors(quote! {
            struct S {
                #[option(description = "d")]
                #[option(description = "d")]
                a: i64,
            }
        });
        assert_eq!(errs, ["a field takes one #[option(..)] attribute"]);
        assert!(
            errors(quote! {
                struct S {
                    #[option(min_value = -9223372036854775808, description = "d")]
                    a: i64,
                }
            })
            .is_empty()
        );
    }

    #[test]
    fn raw_idents_and_rename() {
        let out = run(quote! {
            struct S {
                #[option(description = "d")]
                r#type: i64,
                #[option(description = "d", rename = "silent")]
                quiet: bool,
            }
        });
        assert!(out.contains("OptionDecl :: new (\"type\""), "{out}");
        assert!(out.contains("r#type : :: noctis"), "{out}");
        assert!(out.contains("OptionDecl :: new (\"silent\""), "{out}");
    }

    const MISSING_HELP: &str = "\n  = help: add `#[option(description = \"…\")]` with 1 to 100 characters (doc comments are rustdoc only)";

    #[test]
    fn cmd_04_descriptions() {
        assert_eq!(
            errors(quote!(
                struct S {
                    a: i64,
                }
            )),
            [format!(
                "[CMD-04] a: limit between 1 and 100, actual 0\n  = note: in the default description{MISSING_HELP}"
            )]
        );
        let cases = [
            (String::new(), Some(0)),
            ("a".repeat(101), Some(101)),
            ("a".repeat(100), None),
            ("😀".repeat(50), None),
            ("😀".repeat(51), Some(102)),
        ];
        for (desc, actual) in cases {
            let errs = errors(quote! {
                struct S {
                    #[option(description = #desc)]
                    a: i64,
                }
            });
            match actual {
                None => assert!(errs.is_empty(), "{errs:?}"),
                Some(n) => assert_eq!(
                    errs,
                    [format!(
                        "[CMD-04] a: limit between 1 and 100, actual {n}\n  = note: in the default description"
                    )]
                ),
            }
        }
    }

    fn with_name(name: &str) -> Vec<String> {
        errors(quote! {
            struct S {
                #[option(description = "d")]
                x: i64,
                #[option(description = "d", rename = #name)]
                a: i64,
            }
        })
    }

    #[test]
    fn cmd_01_cmd_02_cmd_03_names() {
        assert!(with_name("Days")[0].starts_with("[CMD-03] Days"));
        assert!(with_name("it's")[0].starts_with("[CMD-01]"));
        assert!(with_name(&"a".repeat(33))[0].starts_with("[CMD-02]"));
        assert!(with_name(&"a".repeat(33))[0].contains("actual 33"));
        assert!(with_name(&"a".repeat(32)).is_empty());
        assert_eq!(
            with_name(""),
            ["[CMD-02] options[1]: limit between 1 and 32, actual 0"]
        );
    }

    #[test]
    fn opt_01_required_after_optional() {
        let errs = errors(quote! {
            struct S {
                #[option(description = "d")]
                a: Option<i64>,
                #[option(description = "d")]
                b: String,
            }
        });
        assert_eq!(
            errs,
            [
                "[OPT-01] b: limit required options before optional ones, actual `b`\n  = note: after optional `a`"
            ]
        );
        let errs = errors(quote! {
            struct S {
                #[option(description = "d")]
                a: Option<i64>,
                #[option(description = "d")]
                b: i64,
                #[option(description = "d")]
                c: i64,
            }
        });
        assert_eq!(errs.len(), 2);
        assert!(errs[1].starts_with("[OPT-01] c"));
    }

    #[test]
    fn cmd_06_duplicate_names() {
        let errs = errors(quote! {
            struct S {
                #[option(description = "d")]
                a: i64,
                #[option(description = "d", rename = "a")]
                b: i64,
            }
        });
        assert_eq!(
            errs,
            [
                "[CMD-06] a: limit unique option names, actual `a`\n  = note: declared at positions 0, 1"
            ]
        );
    }

    #[test]
    fn cmd_13_option_count() {
        let fields = |n: usize| {
            let idents = (0..n).map(|i| Ident::new(&format!("o{i}"), Span::call_site()));
            quote!(struct S { #(#[option(description = "d")] #idents: i64,)* })
        };
        assert!(errors(fields(25)).is_empty());
        assert_eq!(
            errors(fields(26)),
            ["[CMD-13] options: limit at most 25, actual 26"]
        );
    }

    #[test]
    fn several_at_once() {
        let errs = errors(quote! {
            struct S {
                #[option(description = "d")]
                a: Option<i64>,
                #[option(rename = "Days")]
                b: i64,
            }
        });
        let rules: Vec<&str> = errs.iter().map(|e| &e[..8]).collect();
        assert_eq!(rules, ["[CMD-03]", "[CMD-04]", "[OPT-01]"], "{errs:?}");
    }

    #[test]
    fn opt_13_entries_are_expansion_errors() {
        let errs = errors(quote! {
            struct S {
                #[option(description = "d", file_types = ["pdf", ".tar.gz"])]
                a: ResolvedAttachment,
            }
        });
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(
            errs[0].starts_with("[OPT-13] a.file_types[0]: "),
            "{errs:?}"
        );
        assert!(errs[0].ends_with("actual `pdf`"), "{errs:?}");
        let eleven: Vec<String> = (0..11).map(|i| format!(".e{i}")).collect();
        let errs = errors(quote! {
            struct S {
                #[option(description = "d", file_types = [#(#eleven),*])]
                a: ResolvedAttachment,
            }
        });
        assert_eq!(errs, ["[OPT-13] a.file_types: limit at most 10, actual 11"]);
        assert!(
            errors(quote! {
                struct S {
                    #[option(description = "d", file_types = ["image", ".PNG", ".7z"])]
                    a: ResolvedAttachment,
                }
            })
            .is_empty()
        );
    }

    #[test]
    fn type_dependent_rules_are_const_arms() {
        let texts = const_texts(quote! {
            struct S {
                #[option(description = "d", min_value = 10, max_value = 5)]
                a: i64,
            }
        });
        assert!(
            texts
                .iter()
                .any(|(k, t)| k == &[OptionType::Integer, OptionType::Number]
                    && t == "[OPT-05] a: limit min_value ≤ max_value, actual `10 > 5`"),
            "{texts:?}"
        );
        assert!(texts.iter().any(|(k, t)| k == &[OptionType::String]
            && t == "[OPT-05] a.min_value: limit min_value / max_value only on INTEGER or NUMBER options, actual `STRING`"), "{texts:?}");

        let texts = const_texts(quote! {
            struct S {
                #[option(description = "d", max_length = 6001)]
                a: String,
            }
        });
        assert!(
            texts.iter().any(|(k, t)| k == &[OptionType::String]
                && t == "[OPT-06] a.max_length: limit between 1 and 6000, actual 6001"),
            "{texts:?}"
        );
        let texts = const_texts(quote! {
            struct S {
                #[option(description = "d", channel_types = [GuildText, ChannelType::GuildVoice], autocomplete = f)]
                a: String,
            }
        });
        assert!(
            texts
                .iter()
                .any(|(_, t)| t.starts_with("[OPT-07] a.channel_types")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|(k, t)| k.contains(&OptionType::Boolean)
            && t == "[OPT-11] a.autocomplete: limit autocomplete only on STRING, INTEGER or NUMBER options, actual `BOOLEAN`"), "{texts:?}");
        let texts = const_texts(quote! {
            struct S {
                #[option(description = "d", max_value = 1.5)]
                a: i64,
            }
        });
        assert!(texts.iter().any(|(k, t)| k == &[OptionType::Integer]
            && t == "[OPT-05] a.max_value: limit an integer bound on an INTEGER option, actual `1.5`"), "{texts:?}");
    }

    #[test]
    fn const_items_are_generated() {
        let out = run(quote! {
            struct S {
                #[option(description = "d", min_length = 1, autocomplete = suggest)]
                a: i64,
            }
        });
        assert!(out.contains("const _ : () = match < i64 as :: noctis :: __private :: core :: CommandOption > :: KIND"), "{out}");
        assert!(
            out.contains(":: core :: panic ! (\"[OPT-06] a.min_length"),
            "{out}"
        );
        assert!(out.contains("CHOICES . len () > 25usize"), "{out}");
        assert!(out.contains("[OPT-08] a.choices: limit at most 25, actual `more than 25`\\n  = note: `i64` has more variants than a command option takes"), "{out}");
        assert!(
            out.contains("[OPT-10] a: limit choices or autocomplete, not both, actual `a`"),
            "{out}"
        );
        assert!(out.contains("let _ = suggest ;"), "{out}");
        assert!(out.contains(". autocomplete (true)"), "{out}");
    }

    #[test]
    fn errors_keep_const_checks_and_drop_the_impl() {
        let out = run(quote! {
            struct S {
                a: i64,
            }
        });
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("const _"), "{out}");
        assert!(!out.contains("impl"), "{out}");
    }

    #[test]
    fn panic_text_escapes_braces() {
        assert_eq!(panic_lit("a {b}").to_string(), "\"a {{b}}\"");
    }
}
