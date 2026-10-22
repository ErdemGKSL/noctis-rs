//! Reads the Tailwind tables and the lookup algorithm out of takumi-core's
//! sources with `syn`. Every unexpected shape is an error naming the file and
//! the item; nothing is guessed.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, File, ImplItem, Item, LitStr, Pat, Stmt, Token, Type};

use super::GrammarError;

/// takumi-core's `map.rs`, relative to the crate root.
pub(crate) const MAP_RS: &str = "src/style/tw/map.rs";
/// takumi-core's `namespace.rs`, relative to the crate root.
pub(crate) const NAMESPACE_RS: &str = "src/style/tw/namespace.rs";
/// takumi-core's `tw/mod.rs`, relative to the crate root.
pub(crate) const MOD_RS: &str = "src/style/tw/mod.rs";

/// The functions of `tw/mod.rs` whose token strings are fingerprinted: the
/// lookup algorithm the matcher copies. `Type::fn` names an inherent method.
pub(crate) const ALGORITHM_ITEMS: &[&str] = &[
    "split_variant",
    "TailwindValue::parse",
    "Breakpoint::parse",
    "TailwindProperty::parse",
    "TailwindProperty::parse_prefix_suffix",
    "TailwindProperty::try_neg",
    "var_expression",
    "parse_opacity_modifier",
    "is_ident",
    "is_ident_byte",
    "extract_arbitrary_value",
    "macro_rules! try_neg",
];

/// One `property_parsers!` row: `Variant(Arg) => ParseType [namespaces]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParserDef {
    pub(crate) variant: String,
    pub(crate) parse_type: String,
    /// The bracketed namespace override, if the row spells one.
    pub(crate) namespaces: Option<Vec<String>>,
}

/// One `PREFIX_PARSERS` parser: `PropertyParser::<variant>(TailwindProperty::<property>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrefixParser {
    pub(crate) variant: String,
    pub(crate) property: String,
}

/// Everything the generator reads from takumi-core.
#[derive(Debug, Clone, Default)]
pub(crate) struct Extracted {
    /// `FIXED_PROPERTIES` keys.
    pub(crate) fixed: BTreeSet<String>,
    /// `PREFIX_PARSERS`: prefix -> parsers in source order.
    pub(crate) prefixes: BTreeMap<String, Vec<PrefixParser>>,
    /// `property_parsers!` rows plus the hard-coded `GradientPosition`, in order.
    pub(crate) parsers: Vec<ParserDef>,
    /// `VAR_TARGETS`: prefix -> namespaces in source order.
    pub(crate) var_targets: BTreeMap<String, Vec<String>>,
    /// `Namespace::prefix`: variant -> CSS prefix, in source order.
    pub(crate) namespaces: Vec<(String, String)>,
    /// `try_neg!` lists: `try_negative`, `unary`, `grid`, merged.
    pub(crate) negatable: BTreeSet<String>,
    /// `Breakpoint::parse` built-in keys, in source order.
    pub(crate) breakpoints: Vec<String>,
    /// Fingerprinted item -> blake3 of its token string.
    pub(crate) fingerprints: BTreeMap<String, String>,
}

fn err(file: &str, item: &str, message: impl Into<String>) -> GrammarError {
    GrammarError::new(file, item, message)
}

fn parse(file: &str, text: &str) -> Result<File, GrammarError> {
    syn::parse_file(text).map_err(|e| err(file, "file", format!("does not parse: {e}")))
}

/// The first 16 hex digits of the blake3 hash of `tokens`' string form.
pub(crate) fn fingerprint(tokens: &impl ToTokens) -> String {
    let text = tokens.to_token_stream().to_string();
    blake3::hash(text.as_bytes()).to_hex()[..16].to_owned()
}

/// Reads the three Tailwind files plus every `impl TailwindPropertyParser`
/// in `sources` (path relative to the crate root -> text).
pub(crate) fn extract(sources: &BTreeMap<String, String>) -> Result<Extracted, GrammarError> {
    let source = |path: &str| {
        sources
            .get(path)
            .ok_or_else(|| err(path, "file", "is missing from the takumi-core sources"))
    };
    let mut out = Extracted::default();
    extract_map(&parse(MAP_RS, source(MAP_RS)?)?, &mut out)?;
    out.namespaces = extract_namespaces(&parse(NAMESPACE_RS, source(NAMESPACE_RS)?)?)?;
    let mod_rs = parse(MOD_RS, source(MOD_RS)?)?;
    out.negatable = extract_try_neg(&mod_rs)?;
    out.breakpoints = extract_breakpoints(&mod_rs)?;
    for name in ALGORITHM_ITEMS {
        let item = find_algorithm_item(&mod_rs, name)
            .ok_or_else(|| err(MOD_RS, name, "item is missing"))?;
        out.fingerprints.insert((*name).to_owned(), item);
    }
    for (path, text) in sources {
        for (ty, hash) in parser_impls(&parse(path, text)?.items) {
            let key = format!("impl TailwindPropertyParser for {ty}");
            if out.fingerprints.insert(key.clone(), hash).is_some() {
                return Err(err(path, &key, "is implemented twice"));
            }
        }
    }
    Ok(out)
}

/// `map.rs`: `FIXED_PROPERTIES`, `PREFIX_PARSERS`, `VAR_TARGETS`, the
/// `property_parsers!` invocation and the `GradientPosition` arm of its
/// `macro_rules!` body.
pub(crate) fn extract_map(file: &File, out: &mut Extracted) -> Result<(), GrammarError> {
    let fixed = phf_static(file, "FIXED_PROPERTIES")?;
    out.fixed = fixed.into_iter().flat_map(|(keys, _)| keys).collect();

    for (keys, expr) in phf_static(file, "PREFIX_PARSERS")? {
        let parsers = array_elems(&expr)
            .ok_or_else(|| {
                err(
                    MAP_RS,
                    "PREFIX_PARSERS",
                    format!("`{}`: not `&[..]`", keys[0]),
                )
            })?
            .iter()
            .map(|elem| {
                prefix_parser(elem).ok_or_else(|| {
                    err(
                        MAP_RS,
                        "PREFIX_PARSERS",
                        format!(
                            "`{}`: expected `PropertyParser::V(TailwindProperty::C)`, found `{}`",
                            keys[0],
                            elem.to_token_stream()
                        ),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for key in keys {
            out.prefixes.insert(key, parsers.clone());
        }
    }

    for (keys, expr) in phf_static(file, "VAR_TARGETS")? {
        let namespaces = array_elems(&expr)
            .ok_or_else(|| err(MAP_RS, "VAR_TARGETS", format!("`{}`: not `&[..]`", keys[0])))?
            .iter()
            .map(|elem| {
                var_target_namespace(elem).ok_or_else(|| {
                    err(
                        MAP_RS,
                        "VAR_TARGETS",
                        format!(
                            "`{}`: expected `(Namespace::X, &[..])`, found `{}`",
                            keys[0],
                            elem.to_token_stream()
                        ),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for key in keys {
            out.var_targets.insert(key, namespaces.clone());
        }
    }

    out.parsers = property_parsers_invocation(file)?;
    let (variant, parse_type) = hard_coded_parser(file)?;
    out.parsers.push(ParserDef {
        variant,
        parse_type,
        namespaces: None,
    });
    let macro_body = file
        .items
        .iter()
        .find(|item| matches!(item, Item::Macro(m) if m.ident.as_ref().is_some_and(|i| i == "property_parsers")));
    if let Some(item) = macro_body {
        out.fingerprints.insert(
            "macro_rules! property_parsers".to_owned(),
            fingerprint(item),
        );
    }
    Ok(())
}

/// The `phf_map!` entries of `static <name>`: keys (with `|` alternatives) and value.
fn phf_static(file: &File, name: &str) -> Result<Vec<(Vec<String>, Expr)>, GrammarError> {
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Static(s) if s.ident == name => Some(s),
            _ => None,
        })
        .ok_or_else(|| err(MAP_RS, name, "static is missing"))?;
    let Expr::Macro(mac) = &*item.expr else {
        return Err(err(MAP_RS, name, "is not a `phf_map!` invocation"));
    };
    if mac
        .mac
        .path
        .segments
        .last()
        .is_none_or(|s| s.ident != "phf_map")
    {
        return Err(err(MAP_RS, name, "is not a `phf_map!` invocation"));
    }
    let parser = |input: ParseStream<'_>| {
        let mut entries = Vec::new();
        while !input.is_empty() {
            let mut keys = vec![input.parse::<LitStr>()?.value()];
            while input.peek(Token![|]) {
                input.parse::<Token![|]>()?;
                keys.push(input.parse::<LitStr>()?.value());
            }
            input.parse::<Token![=>]>()?;
            let value: Expr = input.parse()?;
            entries.push((keys, value));
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(entries)
    };
    parser
        .parse2(mac.mac.tokens.clone())
        .map_err(|e| err(MAP_RS, name, format!("unexpected entry shape: {e}")))
}

/// The elements of `&[a, b, ..]`.
fn array_elems(expr: &Expr) -> Option<Vec<Expr>> {
    let Expr::Reference(reference) = expr else {
        return None;
    };
    let Expr::Array(array) = &*reference.expr else {
        return None;
    };
    Some(array.elems.iter().cloned().collect())
}

/// The last segment of a two-segment path `Head::Last`.
fn path_tail(expr: &Expr, head: &str) -> Option<String> {
    let Expr::Path(path) = expr else {
        return None;
    };
    let segments: Vec<_> = path.path.segments.iter().collect();
    match segments.as_slice() {
        [first, last] if first.ident == head && last.arguments.is_none() => {
            Some(last.ident.to_string())
        }
        _ => None,
    }
}

fn prefix_parser(expr: &Expr) -> Option<PrefixParser> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let variant = path_tail(&call.func, "PropertyParser")?;
    let [arg] = call.args.iter().collect::<Vec<_>>()[..] else {
        return None;
    };
    Some(PrefixParser {
        variant,
        property: path_tail(arg, "TailwindProperty")?,
    })
}

fn var_target_namespace(expr: &Expr) -> Option<String> {
    let Expr::Tuple(tuple) = expr else {
        return None;
    };
    let [namespace, longhands] = tuple.elems.iter().collect::<Vec<_>>()[..] else {
        return None;
    };
    array_elems(longhands)?;
    path_tail(namespace, "Namespace")
}

/// The rows of the `property_parsers! { .. }` invocation.
fn property_parsers_invocation(file: &File) -> Result<Vec<ParserDef>, GrammarError> {
    const ITEM: &str = "property_parsers!";
    let mac = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Macro(m) if m.ident.is_none() && m.mac.path.is_ident("property_parsers") => {
                Some(&m.mac)
            }
            _ => None,
        })
        .ok_or_else(|| err(MAP_RS, ITEM, "invocation is missing"))?;
    let parser = |input: ParseStream<'_>| {
        let mut rows = Vec::new();
        while !input.is_empty() {
            let variant: syn::Ident = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            let _arg: Type = content.parse()?;
            input.parse::<Token![=>]>()?;
            let parse_type: Type = input.parse()?;
            let namespaces = if input.peek(syn::token::Bracket) {
                let content;
                syn::bracketed!(content in input);
                let list = Punctuated::<Expr, Token![,]>::parse_terminated(&content)?;
                Some(list.into_iter().collect::<Vec<_>>())
            } else {
                None
            };
            rows.push((variant, parse_type, namespaces));
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(rows)
    };
    let rows = parser
        .parse2(mac.tokens.clone())
        .map_err(|e| err(MAP_RS, ITEM, format!("unexpected row shape: {e}")))?;
    rows.into_iter()
        .map(|(variant, parse_type, namespaces)| {
            let namespaces = namespaces
                .map(|list| {
                    list.iter()
                        .map(|expr| {
                            path_tail(expr, "Namespace").ok_or_else(|| {
                                err(
                                    MAP_RS,
                                    ITEM,
                                    format!(
                                        "`{variant}`: namespace `{}` is not `Namespace::X`",
                                        expr.to_token_stream()
                                    ),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            Ok(ParserDef {
                variant: variant.to_string(),
                parse_type: type_name(&parse_type).ok_or_else(|| {
                    err(
                        MAP_RS,
                        ITEM,
                        format!("`{variant}`: parse type is not a plain type name"),
                    )
                })?,
                namespaces,
            })
        })
        .collect()
}

fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    (path.qself.is_none() && path.path.segments.len() == 1)
        .then(|| path.path.segments.first())
        .flatten()
        .filter(|s| s.arguments.is_none())
        .map(|s| s.ident.to_string())
}

/// The `Self::<V>(f) => { <T>::parse_tw_with_arbitrary .. }` arm of the
/// `property_parsers` `macro_rules!` body, outside every `$( .. )` repetition.
fn hard_coded_parser(file: &File) -> Result<(String, String), GrammarError> {
    const ITEM: &str = "macro_rules! property_parsers";
    let mac = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Macro(m)
                if m.mac.path.is_ident("macro_rules")
                    && m.ident.as_ref().is_some_and(|i| i == "property_parsers") =>
            {
                Some(&m.mac)
            }
            _ => None,
        })
        .ok_or_else(|| err(MAP_RS, ITEM, "definition is missing"))?;
    let mut found = Vec::new();
    find_hard_coded_arms(mac.tokens.clone(), &mut found);
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(err(
            MAP_RS,
            ITEM,
            "no `Self::V(f) => { T::parse_tw_with_arbitrary .. }` arm outside the repetitions",
        )),
        _ => Err(err(MAP_RS, ITEM, "more than one hard-coded parser arm")),
    }
}

fn find_hard_coded_arms(tokens: TokenStream, out: &mut Vec<(String, String)>) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Group(group) = token {
            let repetition = index
                .checked_sub(1)
                .and_then(|i| tokens.get(i))
                .is_some_and(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == '$'));
            if !repetition {
                find_hard_coded_arms(group.stream(), out);
            }
        }
        if let Some(arm) = hard_coded_arm_at(&tokens[index..]) {
            out.push(arm);
        }
    }
}

/// Matches `Self :: V ( f ) => { T :: parse_tw_with_arbitrary ..` at the start.
fn hard_coded_arm_at(tokens: &[TokenTree]) -> Option<(String, String)> {
    let [
        TokenTree::Ident(this),
        TokenTree::Punct(c1),
        TokenTree::Punct(c2),
        TokenTree::Ident(variant),
        TokenTree::Group(args),
        TokenTree::Punct(eq),
        TokenTree::Punct(gt),
        TokenTree::Group(body),
        ..,
    ] = tokens
    else {
        return None;
    };
    let path_sep = c1.as_char() == ':' && c2.as_char() == ':';
    let arrow = eq.as_char() == '=' && gt.as_char() == '>';
    let args_ok = args.delimiter() == Delimiter::Parenthesis && args.stream().to_string() == "f";
    if this != "Self" || !path_sep || !arrow || !args_ok || body.delimiter() != Delimiter::Brace {
        return None;
    }
    let body: Vec<TokenTree> = body.stream().into_iter().collect();
    let [
        TokenTree::Ident(ty),
        TokenTree::Punct(d1),
        TokenTree::Punct(d2),
        TokenTree::Ident(method),
        ..,
    ] = body.as_slice()
    else {
        return None;
    };
    (d1.as_char() == ':' && d2.as_char() == ':' && method == "parse_tw_with_arbitrary")
        .then(|| (variant.to_string(), ty.to_string()))
}

/// `namespace.rs`: the `Namespace::prefix` match, checked against the enum.
pub(crate) fn extract_namespaces(file: &File) -> Result<Vec<(String, String)>, GrammarError> {
    const ITEM: &str = "Namespace::prefix";
    let variants: Vec<String> = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Enum(e) if e.ident == "Namespace" => {
                Some(e.variants.iter().map(|v| v.ident.to_string()).collect())
            }
            _ => None,
        })
        .ok_or_else(|| err(NAMESPACE_RS, "enum Namespace", "is missing"))?;
    let method = inherent_fns(&file.items, "Namespace")
        .into_iter()
        .find(|f| f.sig.ident == "prefix")
        .ok_or_else(|| err(NAMESPACE_RS, ITEM, "is missing"))?;
    let Some(Stmt::Expr(Expr::Match(matched), None)) = method.block.stmts.last() else {
        return Err(err(NAMESPACE_RS, ITEM, "body is not a single `match`"));
    };
    let mut out = Vec::new();
    for arm in &matched.arms {
        let Pat::Path(pat) = &arm.pat else {
            return Err(err(NAMESPACE_RS, ITEM, "arm pattern is not `Namespace::X`"));
        };
        let variant = path_tail(
            &Expr::Path(syn::ExprPath {
                attrs: Vec::new(),
                qself: None,
                path: pat.path.clone(),
            }),
            "Namespace",
        )
        .ok_or_else(|| err(NAMESPACE_RS, ITEM, "arm pattern is not `Namespace::X`"))?;
        let Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(prefix),
            ..
        }) = &*arm.body
        else {
            return Err(err(
                NAMESPACE_RS,
                ITEM,
                format!("`{variant}`: body is not a string literal"),
            ));
        };
        out.push((variant, prefix.value()));
    }
    let arms: Vec<&String> = out.iter().map(|(v, _)| v).collect();
    if arms != variants.iter().collect::<Vec<_>>() {
        return Err(err(
            NAMESPACE_RS,
            ITEM,
            format!("arms {arms:?} do not match the enum variants {variants:?}"),
        ));
    }
    Ok(out)
}

/// Every `fn` of inherent `impl <ty>` blocks.
fn inherent_fns<'a>(items: &'a [Item], ty: &str) -> Vec<&'a syn::ImplItemFn> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Impl(imp)
                if imp.trait_.is_none() && type_name(&imp.self_ty).is_some_and(|n| n == ty) =>
            {
                Some(imp)
            }
            _ => None,
        })
        .flat_map(|imp| {
            imp.items.iter().filter_map(|item| match item {
                ImplItem::Fn(f) => Some(f),
                _ => None,
            })
        })
        .collect()
}

/// The fingerprint of an [`ALGORITHM_ITEMS`] entry, if the item exists.
fn find_algorithm_item(file: &File, name: &str) -> Option<String> {
    if let Some(macro_name) = name.strip_prefix("macro_rules! ") {
        return file.items.iter().find_map(|item| match item {
            Item::Macro(m) if m.ident.as_ref().is_some_and(|i| i == macro_name) => {
                Some(fingerprint(m))
            }
            _ => None,
        });
    }
    match name.split_once("::") {
        Some((ty, method)) => inherent_fns(&file.items, ty)
            .into_iter()
            .find(|f| f.sig.ident == method)
            .map(fingerprint),
        None => file.items.iter().find_map(|item| match item {
            Item::Fn(f) if f.sig.ident == name => Some(fingerprint(f)),
            _ => None,
        }),
    }
}

/// `impl TailwindPropertyParser for <T>` blocks, recursively through inline modules.
fn parser_impls(items: &[Item]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Impl(imp) => {
                let is_parser = imp.trait_.as_ref().is_some_and(|(path, _)| {
                    path.segments
                        .last()
                        .is_some_and(|s| s.ident == "TailwindPropertyParser")
                });
                if is_parser {
                    out.push((imp.self_ty.to_token_stream().to_string(), fingerprint(imp)));
                }
            }
            Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    out.extend(parser_impls(items));
                }
            }
            _ => {}
        }
    }
    out
}

/// The `try_neg!(self; try_negative: ..; unary: ..; grid: ..)` lists of
/// `TailwindProperty::try_neg`.
pub(crate) fn extract_try_neg(file: &File) -> Result<BTreeSet<String>, GrammarError> {
    const ITEM: &str = "try_neg!";
    let method = inherent_fns(&file.items, "TailwindProperty")
        .into_iter()
        .find(|f| f.sig.ident == "try_neg")
        .ok_or_else(|| err(MOD_RS, "TailwindProperty::try_neg", "is missing"))?;
    let body = find_macro_body(method.block.to_token_stream(), "try_neg")
        .ok_or_else(|| err(MOD_RS, ITEM, "invocation is missing"))?;
    let mut segments: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for token in body {
        match &token {
            TokenTree::Punct(p) if p.as_char() == ';' => segments.push(Vec::new()),
            _ => segments.last_mut().map_or((), |s| s.push(token)),
        }
    }
    let lists = segments.get(1..).unwrap_or_default();
    let mut labels = Vec::new();
    let mut out = BTreeSet::new();
    for segment in lists {
        let [TokenTree::Ident(label), TokenTree::Punct(colon), rest @ ..] = segment.as_slice()
        else {
            return Err(err(MOD_RS, ITEM, "a list is not `label: A, B, ..`"));
        };
        if colon.as_char() != ':' {
            return Err(err(
                MOD_RS,
                ITEM,
                format!("`{label}` is not followed by `:`"),
            ));
        }
        labels.push(label.to_string());
        for token in rest {
            match token {
                TokenTree::Ident(ident) => {
                    out.insert(ident.to_string());
                }
                TokenTree::Punct(p) if p.as_char() == ',' => {}
                other => {
                    return Err(err(
                        MOD_RS,
                        ITEM,
                        format!("`{label}`: unexpected token `{other}`"),
                    ));
                }
            }
        }
    }
    if labels != ["try_negative", "unary", "grid"] {
        return Err(err(
            MOD_RS,
            ITEM,
            format!("lists are {labels:?}, expected [\"try_negative\", \"unary\", \"grid\"]"),
        ));
    }
    Ok(out)
}

/// The string keys of `Breakpoint::parse`'s `match_ignore_ascii_case!`.
pub(crate) fn extract_breakpoints(file: &File) -> Result<Vec<String>, GrammarError> {
    const ITEM: &str = "Breakpoint::parse";
    let method = inherent_fns(&file.items, "Breakpoint")
        .into_iter()
        .find(|f| f.sig.ident == "parse")
        .ok_or_else(|| err(MOD_RS, ITEM, "is missing"))?;
    let body = find_macro_body(method.block.to_token_stream(), "match_ignore_ascii_case")
        .ok_or_else(|| err(MOD_RS, ITEM, "has no `match_ignore_ascii_case!`"))?;
    let keys: Vec<String> = body
        .windows(3)
        .filter_map(|w| match w {
            [
                TokenTree::Literal(lit),
                TokenTree::Punct(eq),
                TokenTree::Punct(gt),
            ] if eq.as_char() == '=' && gt.as_char() == '>' => {
                syn::parse2::<LitStr>(lit.to_token_stream())
                    .ok()
                    .map(|s| s.value())
            }
            _ => None,
        })
        .collect();
    if keys.is_empty() {
        return Err(err(MOD_RS, ITEM, "has no string keys"));
    }
    Ok(keys)
}

/// The tokens inside the first `<name>! ( .. )` / `{ .. }` invocation, searched recursively.
fn find_macro_body(tokens: TokenStream, name: &str) -> Option<Vec<TokenTree>> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let [
            TokenTree::Ident(ident),
            TokenTree::Punct(bang),
            TokenTree::Group(group),
            ..,
        ] = &tokens[index..]
            && ident == name
            && bang.as_char() == '!'
        {
            return Some(group.stream().into_iter().collect());
        }
        if let TokenTree::Group(group) = token
            && let Some(found) = find_macro_body(group.stream(), name)
        {
            return Some(found);
        }
    }
    None
}
