//! `#[derive(Route)]` and its `#[route("id", args…)]` attribute (AD-6).

use noctis_rules::RuleId;
use noctis_rules::cid::{SCHEMA_MAX, validate_route_id};
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{format_ident, quote, quote_spanned};
use syn::{Attribute, DeriveInput, Ident, Lit, LitStr, Meta};

use crate::common::{combine, error};
use crate::payload;

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

/// The codec arguments of a valid `#[route]` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteArgs {
    /// The route id's value.
    pub(crate) id: String,
    /// The body kind.
    pub(crate) body: Body,
    /// The schema version, `0` when absent.
    pub(crate) schema: u8,
    /// `view`.
    pub(crate) view: bool,
    /// `select`.
    pub(crate) select: bool,
    /// `signed`.
    pub(crate) signed: bool,
}

/// The runtime arguments of AD-6 that later releases add.
const RUNTIME_ARGS: [&str; 6] = [
    "ttl",
    "one_shot",
    "owner",
    "permissions",
    "guard",
    "opens_modal",
];

const ARGS_HELP: &str = "the codec arguments are `by_ref`, `overflow = spill`, `schema = n`, `view`, `select` and `signed`";

const USAGE_HELP: &str = "write #[route(\"shop.buy\")], optionally followed by codec arguments such as #[route(\"shop.buy\", schema = 1)]";

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

/// Finds and parses the one `#[route(...)]` attribute of `ident` (CID-03).
pub(crate) fn parse_route_attr(attrs: &[Attribute], ident: &Ident) -> syn::Result<RouteArgs> {
    let mut found = attrs.iter().filter(|a| a.path().is_ident("route"));
    let Some(attr) = found.next() else {
        return Err(cid_03(
            ident,
            "#[derive(Route)] needs a #[route(\"id\")] attribute",
            USAGE_HELP,
        ));
    };
    let mut errors = Vec::new();
    for extra in found {
        errors.push(cid_03(
            extra,
            "a Route takes exactly one #[route] attribute",
            "merge the arguments into one #[route(\"id\", args…)]",
        ));
    }
    let parsed = match &attr.meta {
        Meta::List(list) => parse_list(attr, list.tokens.clone()),
        Meta::Path(_) | Meta::NameValue(_) => Err(cid_03(
            attr,
            "#[route] takes a list: the route id, then codec arguments",
            USAGE_HELP,
        )),
    };
    match parsed {
        Ok(args) => {
            combine(errors)?;
            Ok(args)
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
    view: bool,
    select: bool,
    signed: Option<Ident>,
    names: Vec<String>,
}

/// Parses the tokens inside `#[route(...)]`.
fn parse_list(attr: &Attribute, tokens: TokenStream) -> syn::Result<RouteArgs> {
    let mut segments = split_commas(tokens);
    // A trailing comma leaves one empty segment at the end.
    if segments.len() > 1 && segments.last().is_some_and(Vec::is_empty) {
        segments.pop();
    }
    let mut errors = Vec::new();
    let mut segments = segments.into_iter();
    let id = parse_id(attr, &segments.next().unwrap_or_default(), &mut errors);
    let mut seen = Seen::default();
    for segment in segments {
        parse_arg(attr, &segment, &mut seen, &mut errors);
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
    Ok(RouteArgs {
        id: id.unwrap_or_default(),
        body,
        schema: seen.schema.unwrap_or(0),
        view: seen.view,
        select: seen.select,
        signed: seen.signed.is_some(),
    })
}

/// Parses and validates the route id segment.
fn parse_id(
    attr: &Attribute,
    segment: &[TokenTree],
    errors: &mut Vec<syn::Error>,
) -> Option<String> {
    let [TokenTree::Literal(literal)] = segment else {
        let what = if segment.is_empty() {
            cid_03(attr, "#[route] is missing the route id", USAGE_HELP)
        } else {
            cid_03(
                stream(segment),
                "the route id must be a string literal",
                USAGE_HELP,
            )
        };
        errors.push(what);
        return None;
    };
    let Ok(Lit::Str(lit)) = syn::parse2::<Lit>(TokenTree::Literal(literal.clone()).into()) else {
        errors.push(cid_03(
            literal,
            "the route id must be a string literal",
            USAGE_HELP,
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
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    let Some((TokenTree::Ident(name), rest)) = segment.split_first() else {
        let tokens = if segment.is_empty() {
            quote!(#attr)
        } else {
            stream(segment)
        };
        errors.push(cid_03(
            tokens,
            "expected a #[route] argument name",
            ARGS_HELP,
        ));
        return;
    };
    let key = name.to_string();
    let value: Option<&[TokenTree]> = match rest {
        [] => None,
        [TokenTree::Punct(eq), value @ ..] if eq.as_char() == '=' => Some(value),
        _ => {
            errors.push(cid_03(
                stream(segment),
                format!("malformed #[route] argument `{key}`"),
                ARGS_HELP,
            ));
            return;
        }
    };
    if RUNTIME_ARGS.contains(&key.as_str()) {
        errors.push(cid_03(
            name,
            format!("`{key}` is a runtime argument of #[route] that this Noctis release does not support yet"),
            "remove it for now; runtime arguments arrive with their release",
        ));
        return;
    }
    if !["by_ref", "overflow", "schema", "view", "select", "signed"].contains(&key.as_str()) {
        errors.push(cid_03(
            name,
            format!("unknown #[route] argument `{key}`"),
            ARGS_HELP,
        ));
        return;
    }
    if seen.names.contains(&key) {
        errors.push(cid_03(
            name,
            format!("#[route] argument `{key}` is given twice"),
            "remove the duplicate",
        ));
        return;
    }
    seen.names.push(key.clone());
    match (key.as_str(), value) {
        ("by_ref" | "view" | "select" | "signed", Some(_)) => errors.push(cid_03(
            stream(segment),
            format!("`{key}` is a flag and takes no value"),
            format!("write `{key}` alone"),
        )),
        ("by_ref", None) => seen.by_ref = Some(name.clone()),
        ("view", None) => seen.view = true,
        ("select", None) => seen.select = true,
        ("signed", None) => seen.signed = Some(name.clone()),
        ("overflow", value) => parse_overflow(name, value, segment, seen, errors),
        (_, value) => parse_schema(name, value, seen, errors),
    }
}

fn parse_overflow(
    name: &Ident,
    value: Option<&[TokenTree]>,
    segment: &[TokenTree],
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    match value {
        Some([TokenTree::Ident(policy)]) if policy == "spill" => {
            seen.spill = Some(stream(segment));
        }
        Some([TokenTree::Ident(policy)]) if policy == "error" => errors.push(cid_03(
            policy,
            "`overflow = error` is not a #[route] argument",
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

/// Expands `#[derive(Route)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = payload::check_struct(input, "a Route");
    let args = parse_route_attr(&input.attrs, &input.ident);
    let (fields, args) = match (fields, args) {
        (Ok(f), Ok(a)) => (f, a),
        (Err(mut e), Err(a)) => {
            e.combine(a);
            return Err(e);
        }
        (Err(e), Ok(_)) | (Ok(_), Err(e)) => return Err(e),
    };
    let ident = &input.ident;
    let codec = quote!(::noctis::__private::codec);
    let id = LitStr::new(&args.id, Span::call_site());
    let base = match args.body {
        Body::Inline => quote!(INLINE),
        Body::ByRef => quote!(BY_REF),
        Body::Spill => quote!(SPILL),
    };
    let view = args.view.then(|| quote!(.view()));
    let select = args.select.then(|| quote!(.select()));
    let signed = args.signed.then(|| quote!(.signed()));
    let schema = args.schema;
    let payload = payload::impls(input, fields);
    let proof = quote_spanned! {ident.span()=>
        const _: () = ::noctis::__private::codec::assert_fits::<#ident>();
    };
    let test = format_ident!("__noctis_route_width_{}", ident);
    Ok(quote! {
        #[automatically_derived]
        impl #codec::Route for #ident {
            const ID: #codec::RouteId = #codec::RouteId::new(#id);
            const SHAPE: #codec::RouteShape = #codec::RouteShape::#base #view #select #signed;
            const SCHEMA: ::core::primitive::u8 = #schema;
            type Payload = Self;
        }

        #payload

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
    })
}

#[cfg(test)]
mod tests {
    use syn::{DeriveInput, parse_quote};

    use super::{Body, RouteArgs, expand, parse_route_attr};

    fn parse(input: &DeriveInput) -> Result<RouteArgs, String> {
        parse_route_attr(&input.attrs, &input.ident).map_err(|e| {
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
            view: false,
            select: false,
            signed: false,
        }
    }

    /// `#[route]` declarations the derive rejects, and the start of the first
    /// error message.
    const REJECTED: &[(&str, &str)] = &[
        (
            "struct R;",
            "[CID-03] #[derive(Route)] needs a #[route(\"id\")] attribute",
        ),
        (
            "#[route(\"a\")] #[route(\"b\")] struct R;",
            "[CID-03] a Route takes exactly one #[route] attribute",
        ),
        ("#[route] struct R;", "[CID-03] #[route] takes a list"),
        (
            "#[route = \"a\"] struct R;",
            "[CID-03] #[route] takes a list",
        ),
        (
            "#[route()] struct R;",
            "[CID-03] #[route] is missing the route id",
        ),
        (
            "#[route(a)] struct R;",
            "[CID-03] the route id must be a string literal",
        ),
        (
            "#[route(b\"a\")] struct R;",
            "[CID-03] the route id must be a string literal",
        ),
        (
            "#[route(\"Shop\")] struct R;",
            "[CID-03] route id: limit [a-z0-9._-], actual `Shop`",
        ),
        (
            "#[route(\"\")] struct R;",
            "[CID-03] route id: limit between 1 and 24, actual 0",
        ),
        (
            "#[route(\"abcdefghijklmnopqrstuvwxy\")] struct R;",
            "[CID-03] route id: limit between 1 and 24, actual 25",
        ),
        (
            "#[route(\"_x\")] struct R;",
            "[CID-03] route id: limit an id that does not start with `_`",
        ),
        (
            "#[route(\"é\")] struct R;",
            "[CID-03] route id: limit [a-z0-9._-], actual `é`",
        ),
        (
            "#[route(\"a\", colour)] struct R;",
            "[CID-03] unknown #[route] argument `colour`",
        ),
        (
            "#[route(\"a\", ttl = \"1m\")] struct R;",
            "[CID-03] `ttl` is a runtime argument of #[route] that this Noctis release does not support yet",
        ),
        (
            "#[route(\"a\", one_shot)] struct R;",
            "[CID-03] `one_shot` is a runtime argument",
        ),
        (
            "#[route(\"a\", owner = user)] struct R;",
            "[CID-03] `owner` is a runtime argument",
        ),
        (
            "#[route(\"a\", permissions = X)] struct R;",
            "[CID-03] `permissions` is a runtime argument",
        ),
        (
            "#[route(\"a\", guard = f)] struct R;",
            "[CID-03] `guard` is a runtime argument",
        ),
        (
            "#[route(\"a\", opens_modal)] struct R;",
            "[CID-03] `opens_modal` is a runtime argument",
        ),
        (
            "#[route(\"a\", view, view)] struct R;",
            "[CID-03] #[route] argument `view` is given twice",
        ),
        (
            "#[route(\"a\", view = true)] struct R;",
            "[CID-03] `view` is a flag and takes no value",
        ),
        (
            "#[route(\"a\", by_ref())] struct R;",
            "[CID-03] malformed #[route] argument `by_ref`",
        ),
        (
            "#[route(\"a\", overflow = error)] struct R;",
            "[CID-03] `overflow = error` is not a #[route] argument",
        ),
        (
            "#[route(\"a\", overflow = inline)] struct R;",
            "[CID-03] the only `overflow` policy is `spill`",
        ),
        (
            "#[route(\"a\", overflow)] struct R;",
            "[CID-03] `overflow` needs a value",
        ),
        (
            "#[route(\"a\", schema = 0)] struct R;",
            "[CID-03] `schema = 0` is the default schema",
        ),
        (
            "#[route(\"a\", schema = 62)] struct R;",
            "[CID-03] `schema` must be in 1..=61",
        ),
        (
            "#[route(\"a\", schema = -1)] struct R;",
            "[CID-03] `schema` must be an integer literal",
        ),
        (
            "#[route(\"a\", schema = \"3\")] struct R;",
            "[CID-03] `schema` must be an integer literal",
        ),
        (
            "#[route(\"a\", schema)] struct R;",
            "[CID-03] `schema` needs a value",
        ),
        (
            "#[route(\"a\", schema = 3, schema = 4)] struct R;",
            "[CID-03] #[route] argument `schema` is given twice",
        ),
        (
            "#[route(\"a\", by_ref, overflow = spill)] struct R;",
            "[CID-03] `by_ref` and `overflow = spill` cannot be combined",
        ),
        (
            "#[route(\"a\", by_ref, signed)] struct R;",
            "[CID-03] a `by_ref` route cannot be `signed`",
        ),
    ];

    #[test]
    fn cid_03_route_attribute_matrix() {
        // Accepted.
        assert_eq!(
            parse(&parse_quote!(
                #[route("a")]
                struct R;
            )),
            Ok(args("a"))
        );
        assert_eq!(
            parse(&parse_quote!(
                #[route(r"a")]
                struct R;
            )),
            Ok(args("a"))
        );
        assert_eq!(
            parse(&parse_quote!(
                #[route("shop\x2Ebuy")]
                struct R;
            )),
            Ok(args("shop.buy"))
        );
        assert_eq!(
            parse(&parse_quote!(
                #[route("a", schema = 61u8)]
                struct R;
            ))
            .map(|a| a.schema),
            Ok(61)
        );
        assert_eq!(
            parse(&parse_quote!(
                #[route("a", schema = 1)]
                struct R;
            ))
            .map(|a| a.schema),
            Ok(1)
        );
        assert_eq!(
            parse(&parse_quote!(
                #[route("a", signed, schema = 61, select, view, overflow = spill)]
                struct R;
            )),
            Ok(RouteArgs {
                id: "a".to_owned(),
                body: Body::Spill,
                schema: 61,
                view: true,
                select: true,
                signed: true,
            })
        );
        assert_eq!(
            parse(&parse_quote!(
                #[route("w", by_ref)]
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

        let m = parse(&parse_quote!(
            #[route("a", overflow = error)]
            struct R;
        ))
        .unwrap_err();
        assert!(
            m.contains("the plain inline Route (no `overflow`) is the error policy"),
            "{m}"
        );
        let m = parse(&parse_quote!(
            #[route("a", schema = 0)]
            struct R;
        ))
        .unwrap_err();
        assert!(m.contains("leave it out for schema 0"), "{m}");
        // Every applicable id violation is reported.
        let m = parse(&parse_quote!(
            #[route("_X")]
            struct R;
        ))
        .unwrap_err();
        assert_eq!(m.matches("[CID-03]").count(), 2, "{m}");
        let m = parse(&parse_quote!(
            #[route("Shop")]
            struct R;
        ))
        .unwrap_err();
        assert!(m.contains("= note: `S` at byte 0 is not allowed"), "{m}");
    }

    #[test]
    fn cid_01_structure_errors() {
        let message = |input: DeriveInput| expand(&input).map(|_| ()).unwrap_err().to_string();
        for input in [
            parse_quote!(
                #[route("a")]
                struct R<T>(T);
            ),
            parse_quote!(
                #[route("a")]
                struct R<'a>(&'a str);
            ),
            parse_quote!(
                #[route("a")]
                struct R<const N: usize>;
            ),
            parse_quote!(
                #[route("a")]
                struct R
                where
                    u8: Copy;
            ),
        ] {
            let m = message(input);
            assert!(
                m.starts_with("[CID-01] a Route cannot be generic\n  = help: "),
                "{m}"
            );
        }
        let m = message(parse_quote!(
            #[route("a")]
            enum R {
                A,
            }
        ));
        assert!(
            m.starts_with("[CID-01] a Route must be a struct, not an enum"),
            "{m}"
        );
        let m = message(parse_quote!(#[route("a")] union R { a: u8 }));
        assert!(
            m.starts_with("[CID-01] a Route must be a struct, not a union"),
            "{m}"
        );
        assert!(
            expand(&parse_quote!(
                #[route("a")]
                struct R {
                    a: u8,
                }
            ))
            .is_ok()
        );
    }

    #[test]
    fn expand_emits_the_width_test() {
        let out = expand(&parse_quote!(
            #[route("a")]
            struct R {
                a: u8,
            }
        ))
        .unwrap()
        .to_string();
        assert!(out.contains("cfg (test)"), "{out}");
        assert!(out.contains(":: core :: prelude :: v1 :: test"), "{out}");
        assert!(out.contains("fn __noctis_route_width_R"), "{out}");
    }
}
