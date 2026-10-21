//! The kind attribute of the component and modal derives:
//! `#[button("id", <codec args>, defaults(..))]`, the select attributes and
//! `#[modal(..)]` (AD-6, CID-03), and the Route impls every one of them
//! emits (CID-01).

use noctis_rules::RuleId;
use noctis_rules::cid::{SCHEMA_MAX, validate_route_id};
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{format_ident, quote, quote_spanned};
use syn::{Attribute, Ident, Lit, LitStr, Meta};

use crate::common::{combine, error};

/// How a Route's body is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Body {
    /// Always inline (the default).
    Inline,
    /// Always by Ref (`by_ref`).
    ByRef,
    /// Inline when it fits, by Ref otherwise (`overflow = spill`).
    Spill,
}

/// The codec arguments of a valid kind attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteArgs {
    /// The route id's value.
    pub(crate) id: String,
    /// The body kind.
    pub(crate) body: Body,
    /// The schema version, `0` when absent.
    pub(crate) schema: u8,
    /// A select nonce: every select kind carries one.
    pub(crate) select: bool,
    /// `signed`.
    pub(crate) signed: bool,
}

/// The runtime arguments of AD-6 that Epic 7 adds.
const RUNTIME_ARGS: [&str; 8] = [
    "ttl",
    "one_shot",
    "owner",
    "permissions",
    "guard",
    "rate_limit",
    "response",
    "auto_defer",
];

const ARGS_HELP: &str = "the codec arguments are `by_ref`, `overflow = spill`, `schema = n` and `signed`; the component's look goes in `defaults(..)`";

/// A kind attribute: its name and derive, and whether its Route carries a
/// select nonce.
#[derive(Clone, Copy)]
pub(crate) struct AttrKind {
    /// The attribute name, such as `button`.
    pub(crate) attr: &'static str,
    /// The derive name, such as `Button`.
    pub(crate) derive: &'static str,
    /// Every select kind carries the select nonce.
    pub(crate) select: bool,
}

impl AttrKind {
    fn usage(self) -> String {
        format!(
            "write #[{}(\"shop.buy\", defaults(..))], optionally with codec arguments such as #[{}(\"shop.buy\", schema = 1, defaults(..))]",
            self.attr, self.attr
        )
    }
}

/// A parsed kind attribute: the codec arguments and the raw `defaults(..)`
/// block (its `defaults` ident and inner tokens), if any.
pub(crate) struct KindAttr {
    pub(crate) args: RouteArgs,
    pub(crate) defaults: Option<(Ident, TokenStream)>,
    pub(crate) attr: Attribute,
}

/// One comma-separated argument: its tokens, without the comma.
type Segment = Vec<TokenTree>;

/// Splits a token stream on top-level commas.
fn split_commas(tokens: TokenStream) -> Vec<Segment> {
    let mut segments = vec![Vec::new()];
    for tt in tokens {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => segments.push(Vec::new()),
            _ => {
                if let Some(last) = segments.last_mut() {
                    last.push(tt);
                }
            }
        }
    }
    segments
}

fn stream(segment: &[TokenTree]) -> TokenStream {
    segment.iter().cloned().collect()
}

fn cid_03(
    tokens: impl quote::ToTokens,
    what: impl core::fmt::Display,
    help: impl core::fmt::Display,
) -> syn::Error {
    error(tokens, RuleId::CID_03, what, help)
}

/// Finds and parses the one kind attribute of `ident` (CID-03).
pub(crate) fn parse_kind_attr(
    attrs: &[Attribute],
    ident: &Ident,
    kind: AttrKind,
) -> syn::Result<KindAttr> {
    let name = kind.attr;
    let mut found = attrs.iter().filter(|a| a.path().is_ident(name));
    let Some(attr) = found.next() else {
        return Err(cid_03(
            ident,
            format!(
                "#[derive({})] needs a #[{name}(\"id\", defaults(..))] attribute",
                kind.derive
            ),
            kind.usage(),
        ));
    };
    let mut errors = Vec::new();
    for extra in found {
        errors.push(cid_03(
            extra,
            format!("a struct takes exactly one #[{name}] attribute"),
            format!("merge the arguments into one #[{name}(\"id\", args…)]"),
        ));
    }
    let parsed = match &attr.meta {
        Meta::List(list) => parse_list(attr, list.tokens.clone(), kind),
        Meta::Path(_) | Meta::NameValue(_) => Err(cid_03(
            attr,
            format!("#[{name}] takes a list: the id, then codec arguments and `defaults(..)`"),
            kind.usage(),
        )),
    };
    match parsed {
        Ok((args, defaults)) => {
            combine(errors)?;
            Ok(KindAttr {
                args,
                defaults,
                attr: attr.clone(),
            })
        }
        Err(mut e) => {
            for other in errors {
                e.combine(other);
            }
            Err(e)
        }
    }
}

/// The parsed state of the arguments after the id.
#[derive(Default)]
struct Seen {
    by_ref: Option<Ident>,
    spill: Option<TokenStream>,
    schema: Option<u8>,
    signed: Option<Ident>,
    defaults: Option<(Ident, TokenStream)>,
    names: Vec<String>,
}

/// Parses the tokens inside the kind attribute.
fn parse_list(
    attr: &Attribute,
    tokens: TokenStream,
    kind: AttrKind,
) -> syn::Result<(RouteArgs, Option<(Ident, TokenStream)>)> {
    let mut segments = split_commas(tokens);
    // A trailing comma leaves one empty segment at the end.
    if segments.len() > 1 && segments.last().is_some_and(Vec::is_empty) {
        segments.pop();
    }
    let mut errors = Vec::new();
    let mut segments = segments.into_iter();
    let id = parse_id(
        attr,
        &segments.next().unwrap_or_default(),
        kind,
        &mut errors,
    );
    let mut seen = Seen::default();
    for segment in segments {
        parse_arg(attr, &segment, kind, &mut seen, &mut errors);
    }
    if let (Some(by_ref), Some(spill)) = (&seen.by_ref, &seen.spill) {
        errors.push(cid_03(
            spill,
            "`by_ref` and `overflow = spill` cannot be combined",
            format!("`by_ref` always stores the body by Ref; remove `overflow = spill` (or `{by_ref}` to keep short bodies inline)"),
        ));
    }
    if let (Some(_), Some(signed)) = (&seen.by_ref, &seen.signed) {
        errors.push(cid_03(
            signed,
            "a `by_ref` route cannot be `signed`",
            "remove `signed`: a Ref is already unguessable, and only inline bodies carry a tag",
        ));
    }
    combine(errors)?;
    let body = match (&seen.by_ref, &seen.spill) {
        (Some(_), _) => Body::ByRef,
        (None, Some(_)) => Body::Spill,
        (None, None) => Body::Inline,
    };
    Ok((
        RouteArgs {
            id: id.unwrap_or_default(),
            body,
            schema: seen.schema.unwrap_or(0),
            select: kind.select,
            signed: seen.signed.is_some(),
        },
        seen.defaults,
    ))
}

/// Parses and validates the route id segment.
fn parse_id(
    attr: &Attribute,
    segment: &[TokenTree],
    kind: AttrKind,
    errors: &mut Vec<syn::Error>,
) -> Option<String> {
    let usage = kind.usage();
    let [TokenTree::Literal(literal)] = segment else {
        let what = if segment.is_empty() {
            cid_03(
                attr,
                format!("#[{}] is missing the route id", kind.attr),
                usage,
            )
        } else {
            cid_03(
                stream(segment),
                "the route id must be a string literal",
                usage,
            )
        };
        errors.push(what);
        return None;
    };
    let Ok(Lit::Str(lit)) = syn::parse2::<Lit>(TokenTree::Literal(literal.clone()).into()) else {
        errors.push(cid_03(
            literal,
            "the route id must be a string literal",
            usage,
        ));
        return None;
    };
    let id = lit.value();
    let violations = validate_route_id(&id);
    if violations.is_empty() {
        return Some(id);
    }
    for v in violations {
        errors.push(syn::Error::new_spanned(&lit, v.to_string()));
    }
    None
}

/// Parses one argument after the id into `seen`.
fn parse_arg(
    attr: &Attribute,
    segment: &[TokenTree],
    kind: AttrKind,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    let a = kind.attr;
    let Some((TokenTree::Ident(name), rest)) = segment.split_first() else {
        let tokens = if segment.is_empty() {
            quote!(#attr)
        } else {
            stream(segment)
        };
        errors.push(cid_03(
            tokens,
            format!("expected a #[{a}] argument name"),
            ARGS_HELP,
        ));
        return;
    };
    let key = name.to_string();
    if RUNTIME_ARGS.contains(&key.as_str()) {
        errors.push(cid_03(
            name,
            format!("`{key}` is a runtime argument of #[{a}] that this Noctis release does not support yet"),
            "remove it for now; runtime arguments arrive with their release",
        ));
        return;
    }
    if key == "defaults" {
        match rest {
            [TokenTree::Group(group)]
                if group.delimiter() == proc_macro2::Delimiter::Parenthesis =>
            {
                if seen.defaults.is_some() {
                    errors.push(cid_03(
                        name,
                        format!("#[{a}] argument `defaults` is given twice"),
                        "merge the keys into one `defaults(..)`",
                    ));
                } else {
                    seen.defaults = Some((name.clone(), group.stream()));
                }
            }
            _ => errors.push(cid_03(
                stream(segment),
                "`defaults` takes a list: `defaults(key = value, ..)`",
                "write `defaults(..)` with the kind's keys",
            )),
        }
        return;
    }
    let value: Option<&[TokenTree]> = match rest {
        [] => None,
        [TokenTree::Punct(eq), value @ ..] if eq.as_char() == '=' => Some(value),
        _ => {
            errors.push(cid_03(
                stream(segment),
                format!("malformed #[{a}] argument `{key}`"),
                ARGS_HELP,
            ));
            return;
        }
    };
    if key == "select" {
        errors.push(cid_03(
            name,
            format!("unknown #[{a}] argument `select`"),
            "every select kind carries the select nonce: remove `select`",
        ));
        return;
    }
    if !["by_ref", "overflow", "schema", "signed"].contains(&key.as_str()) {
        errors.push(cid_03(
            name,
            format!("unknown #[{a}] argument `{key}`"),
            ARGS_HELP,
        ));
        return;
    }
    if seen.names.contains(&key) {
        errors.push(cid_03(
            name,
            format!("#[{a}] argument `{key}` is given twice"),
            "remove the duplicate",
        ));
        return;
    }
    seen.names.push(key.clone());
    match (key.as_str(), value) {
        ("by_ref" | "signed", Some(_)) => errors.push(cid_03(
            stream(segment),
            format!("`{key}` is a flag and takes no value"),
            format!("write `{key}` alone"),
        )),
        ("by_ref", None) => seen.by_ref = Some(name.clone()),
        ("signed", None) => seen.signed = Some(name.clone()),
        ("overflow", value) => parse_overflow(name, value, segment, kind, seen, errors),
        (_, value) => parse_schema(name, value, seen, errors),
    }
}

fn parse_overflow(
    name: &Ident,
    value: Option<&[TokenTree]>,
    segment: &[TokenTree],
    kind: AttrKind,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    match value {
        Some([TokenTree::Ident(policy)]) if policy == "spill" => {
            seen.spill = Some(stream(segment));
        }
        Some([TokenTree::Ident(policy)]) if policy == "error" => errors.push(cid_03(
            policy,
            format!("`overflow = error` is not a #[{}] argument", kind.attr),
            "the plain inline Route (no `overflow`) is the error policy: leave `overflow` out",
        )),
        Some(value) if !value.is_empty() => errors.push(cid_03(
            stream(value),
            "the only `overflow` policy is `spill`",
            "write `overflow = spill`, or leave `overflow` out for an inline Route",
        )),
        _ => errors.push(cid_03(
            name,
            "`overflow` needs a value",
            "write `overflow = spill`",
        )),
    }
}

fn parse_schema(
    name: &Ident,
    value: Option<&[TokenTree]>,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    let help = format!("write `schema = n` with an integer literal n in 1..={SCHEMA_MAX}");
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        errors.push(cid_03(name, "`schema` needs a value", help));
        return;
    };
    let lit = match value {
        [TokenTree::Literal(l)] => syn::parse2::<Lit>(TokenTree::Literal(l.clone()).into()).ok(),
        _ => None,
    };
    let Some(Lit::Int(int)) = lit else {
        errors.push(cid_03(
            stream(value),
            "`schema` must be an integer literal",
            help,
        ));
        return;
    };
    let parsed = match int.suffix() {
        "" | "u8" => int.base10_parse::<u8>().ok(),
        _ => None,
    };
    match parsed {
        Some(0) => errors.push(cid_03(
            &int,
            "`schema = 0` is the default schema",
            "leave it out for schema 0",
        )),
        Some(n) if n <= SCHEMA_MAX => seen.schema = Some(n),
        _ => errors.push(cid_03(
            &int,
            format!("`schema` must be in 1..={SCHEMA_MAX} (one base62 digit), found `{int}`"),
            help,
        )),
    }
}

/// The Route impls of `ident` with `args`: `Route` (Payload `payload`),
/// `SelectRoute` for a select, `DeclaredRuntime`, the CID-01 proof and its
/// width test. The Payload impls are the caller's.
pub(crate) fn route_impls(ident: &Ident, args: &RouteArgs, payload: &TokenStream) -> TokenStream {
    let codec = quote!(::noctis::__private::codec);
    let id = LitStr::new(&args.id, Span::call_site());
    let base = match args.body {
        Body::Inline => quote!(INLINE),
        Body::ByRef => quote!(BY_REF),
        Body::Spill => quote!(SPILL),
    };
    let select = args.select.then(|| quote!(.select()));
    let signed = args.signed.then(|| quote!(.signed()));
    let select_impl = args.select.then(|| {
        quote! {
            #[automatically_derived]
            impl #codec::SelectRoute for #ident {}
        }
    });
    let schema = args.schema;
    let proof = quote_spanned! {ident.span()=>
        const _: () = ::noctis::__private::codec::assert_fits::<#ident>();
    };
    let test = format_ident!("__noctis_route_width_{}", ident);
    quote! {
        #[automatically_derived]
        impl #codec::Route for #ident {
            const ID: #codec::RouteId = #codec::RouteId::new(#id);
            const SHAPE: #codec::RouteShape = #codec::RouteShape::#base #select #signed;
            const SCHEMA: ::core::primitive::u8 = #schema;
            type Payload = #payload;
        }

        #select_impl

        #[automatically_derived]
        impl #codec::DeclaredRuntime for #ident {
            const RUNTIME: #codec::RouteRuntime = #codec::RouteRuntime::EMPTY;
        }

        #proof

        #[cfg(test)]
        #[::core::prelude::v1::test]
        #[allow(non_snake_case, clippy::print_stdout)]
        fn #test() {
            let width = #codec::route_width::<#ident>();
            ::std::println!("{width}");
            ::core::assert!(width.fits(), "{width}");
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::{DeriveInput, parse_quote};

    use super::{AttrKind, Body, RouteArgs, parse_kind_attr, route_impls};

    const BUTTON: AttrKind = AttrKind {
        attr: "button",
        derive: "Button",
        select: false,
    };

    fn parse(input: &DeriveInput) -> Result<RouteArgs, String> {
        parse_kind_attr(&input.attrs, &input.ident, BUTTON)
            .map(|k| k.args)
            .map_err(|e| {
                e.into_iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            })
    }

    fn args(id: &str) -> RouteArgs {
        RouteArgs {
            id: id.to_owned(),
            body: Body::Inline,
            schema: 0,
            select: false,
            signed: false,
        }
    }

    /// `#[button]` declarations the derive rejects, and the start of the first
    /// error message.
    const REJECTED: &[(&str, &str)] = &[
        (
            "struct R;",
            "[CID-03] #[derive(Button)] needs a #[button(\"id\", defaults(..))] attribute",
        ),
        (
            "#[button(\"a\")] #[button(\"b\")] struct R;",
            "[CID-03] a struct takes exactly one #[button] attribute",
        ),
        ("#[button] struct R;", "[CID-03] #[button] takes a list"),
        (
            "#[button = \"a\"] struct R;",
            "[CID-03] #[button] takes a list",
        ),
        (
            "#[button()] struct R;",
            "[CID-03] #[button] is missing the route id",
        ),
        (
            "#[button(a)] struct R;",
            "[CID-03] the route id must be a string literal",
        ),
        (
            "#[button(b\"a\")] struct R;",
            "[CID-03] the route id must be a string literal",
        ),
        (
            "#[button(\"Shop\")] struct R;",
            "[CID-03] route id: limit [a-z0-9._-], actual `Shop`",
        ),
        (
            "#[button(\"\")] struct R;",
            "[CID-03] route id: limit between 1 and 24, actual 0",
        ),
        (
            "#[button(\"abcdefghijklmnopqrstuvwxy\")] struct R;",
            "[CID-03] route id: limit between 1 and 24, actual 25",
        ),
        (
            "#[button(\"_x\")] struct R;",
            "[CID-03] route id: limit an id that does not start with `_`",
        ),
        (
            "#[button(\"é\")] struct R;",
            "[CID-03] route id: limit [a-z0-9._-], actual `é`",
        ),
        (
            "#[button(\"a\", colour)] struct R;",
            "[CID-03] unknown #[button] argument `colour`",
        ),
        (
            "#[button(\"a\", ttl = \"1m\")] struct R;",
            "[CID-03] `ttl` is a runtime argument of #[button] that this Noctis release does not support yet",
        ),
        (
            "#[button(\"a\", one_shot)] struct R;",
            "[CID-03] `one_shot` is a runtime argument",
        ),
        (
            "#[button(\"a\", owner = user)] struct R;",
            "[CID-03] `owner` is a runtime argument",
        ),
        (
            "#[button(\"a\", permissions = X)] struct R;",
            "[CID-03] `permissions` is a runtime argument",
        ),
        (
            "#[button(\"a\", guard = f)] struct R;",
            "[CID-03] `guard` is a runtime argument",
        ),
        (
            "#[button(\"a\", opens_modal)] struct R;",
            "[CID-03] unknown #[button] argument `opens_modal`",
        ),
        (
            "#[button(\"a\", view)] struct R;",
            "[CID-03] unknown #[button] argument `view`",
        ),
        (
            "#[button(\"a\", view = true)] struct R;",
            "[CID-03] unknown #[button] argument `view`",
        ),
        (
            "#[button(\"a\", vïew)] struct R;",
            "[CID-03] unknown #[button] argument `vïew`",
        ),
        (
            "#[button(\"a\", select)] struct R;",
            "[CID-03] unknown #[button] argument `select`",
        ),
        (
            "#[button(\"a\", signed, signed)] struct R;",
            "[CID-03] #[button] argument `signed` is given twice",
        ),
        (
            "#[button(\"a\", signed = true)] struct R;",
            "[CID-03] `signed` is a flag and takes no value",
        ),
        (
            "#[button(\"a\", rate_limit(user, 3))] struct R;",
            "[CID-03] `rate_limit` is a runtime argument",
        ),
        (
            "#[button(\"a\", response = update)] struct R;",
            "[CID-03] `response` is a runtime argument",
        ),
        (
            "#[button(\"a\", auto_defer = false)] struct R;",
            "[CID-03] `auto_defer` is a runtime argument",
        ),
        (
            "#[button(\"a\", defaults = 1)] struct R;",
            "[CID-03] `defaults` takes a list",
        ),
        (
            "#[button(\"a\", by_ref())] struct R;",
            "[CID-03] malformed #[button] argument `by_ref`",
        ),
        (
            "#[button(\"a\", overflow = error)] struct R;",
            "[CID-03] `overflow = error` is not a #[button] argument",
        ),
        (
            "#[button(\"a\", overflow = inline)] struct R;",
            "[CID-03] the only `overflow` policy is `spill`",
        ),
        (
            "#[button(\"a\", overflow)] struct R;",
            "[CID-03] `overflow` needs a value",
        ),
        (
            "#[button(\"a\", schema = 0)] struct R;",
            "[CID-03] `schema = 0` is the default schema",
        ),
        (
            "#[button(\"a\", schema = 62)] struct R;",
            "[CID-03] `schema` must be in 1..=61",
        ),
        (
            "#[button(\"a\", schema = -1)] struct R;",
            "[CID-03] `schema` must be an integer literal",
        ),
        (
            "#[button(\"a\", schema = \"3\")] struct R;",
            "[CID-03] `schema` must be an integer literal",
        ),
        (
            "#[button(\"a\", schema)] struct R;",
            "[CID-03] `schema` needs a value",
        ),
        (
            "#[button(\"a\", schema = 3, schema = 4)] struct R;",
            "[CID-03] #[button] argument `schema` is given twice",
        ),
        (
            "#[button(\"a\", by_ref, overflow = spill)] struct R;",
            "[CID-03] `by_ref` and `overflow = spill` cannot be combined",
        ),
        (
            "#[button(\"a\", by_ref, signed)] struct R;",
            "[CID-03] a `by_ref` route cannot be `signed`",
        ),
    ];

    #[test]
    fn cid_03_route_attribute_matrix() {
        // Accepted.
        assert_eq!(
            parse(&parse_quote!(
                #[button("a")]
                struct R;
            )),
            Ok(args("a"))
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button(r"a")]
                struct R;
            )),
            Ok(args("a"))
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button("shop\x2Ebuy")]
                struct R;
            )),
            Ok(args("shop.buy"))
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button("a", schema = 61u8)]
                struct R;
            ))
            .map(|a| a.schema),
            Ok(61)
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button("a", schema = 1)]
                struct R;
            ))
            .map(|a| a.schema),
            Ok(1)
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button("a", signed, schema = 61, overflow = spill, defaults(style = Primary))]
                struct R;
            )),
            Ok(RouteArgs {
                id: "a".to_owned(),
                body: Body::Spill,
                schema: 61,
                select: false,
                signed: true,
            })
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button("w", by_ref)]
                struct R;
            ))
            .map(|a| a.body),
            Ok(Body::ByRef)
        );

        // Rejected, with the start of the first message.
        for (source, start) in REJECTED {
            let input: DeriveInput = syn::parse_str(source).unwrap();
            let message = parse(&input).expect_err(start);
            assert!(message.starts_with(start), "{start}\n=> {message}");
            assert!(message.contains("\n  = help: "), "{message}");
        }

        let input: DeriveInput = syn::parse_str("#[button(\"a\", view)] struct R;").unwrap();
        assert_eq!(
            parse(&input).expect_err("view is withdrawn"),
            "[CID-03] unknown #[button] argument `view`\n  = help: the codec arguments are `by_ref`, `overflow = spill`, `schema = n` and `signed`; the component's look goes in `defaults(..)`"
        );

        let m = parse(&parse_quote!(
            #[button("a", overflow = error)]
            struct R;
        ))
        .unwrap_err();
        assert!(
            m.contains("the plain inline Route (no `overflow`) is the error policy"),
            "{m}"
        );
        let m = parse(&parse_quote!(
            #[button("a", schema = 0)]
            struct R;
        ))
        .unwrap_err();
        assert!(m.contains("leave it out for schema 0"), "{m}");
        // Every applicable id violation is reported.
        let m = parse(&parse_quote!(
            #[button("_X")]
            struct R;
        ))
        .unwrap_err();
        assert_eq!(m.matches("[CID-03]").count(), 2, "{m}");
        let m = parse(&parse_quote!(
            #[button("Shop")]
            struct R;
        ))
        .unwrap_err();
        assert!(m.contains("= note: `S` at byte 0 is not allowed"), "{m}");
    }

    #[test]
    fn select_kinds_carry_the_nonce() {
        let input: DeriveInput = parse_quote!(
            #[string_select("a", defaults(placeholder = "p"))]
            struct R;
        );
        let kind = AttrKind {
            attr: "string_select",
            derive: "StringSelectMenu",
            select: true,
        };
        let parsed = parse_kind_attr(&input.attrs, &input.ident, kind).unwrap();
        assert!(parsed.args.select);
        let (ident, tokens) = parsed.defaults.unwrap();
        assert_eq!(
            (ident.to_string(), tokens.to_string()),
            ("defaults".to_owned(), "placeholder = \"p\"".to_owned())
        );
    }

    #[test]
    fn route_impls_emit_the_width_test_and_select_route() {
        let ident: syn::Ident = parse_quote!(R);
        let mut a = args("a");
        let plain = route_impls(&ident, &a, &quote::quote!(Self)).to_string();
        assert!(plain.contains("cfg (test)"), "{plain}");
        assert!(plain.contains("fn __noctis_route_width_R"), "{plain}");
        assert!(!plain.contains("SelectRoute"), "{plain}");
        a.select = true;
        let select = route_impls(&ident, &a, &quote::quote!(P)).to_string();
        assert!(
            select.contains("impl :: noctis :: __private :: codec :: SelectRoute for R { }"),
            "{select}"
        );
        assert!(select.contains("type Payload = P ;"), "{select}");
    }
}
