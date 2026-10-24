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
use crate::duration::{DurationError, parse_secs};
use crate::flags::Flags;

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

/// The declared `response` of a component or `#[modal]` attribute (AD-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Response {
    /// `response = update` (the default).
    Update,
    /// `response = ephemeral`.
    Ephemeral,
    /// `response = public`.
    Public,
}

impl Response {
    /// The `__Response` variant the derives emit.
    pub(crate) fn variant(self) -> TokenStream {
        match self {
            Self::Update => quote!(Update),
            Self::Ephemeral => quote!(Ephemeral),
            Self::Public => quote!(Public),
        }
    }
}

/// The codec and runtime arguments of a valid kind attribute.
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
    /// `response = ..` (`Update` when omitted).
    pub(crate) response: Response,
    /// `auto_defer = ..` (`true` when omitted).
    pub(crate) auto_defer: bool,
    /// `ttl = ".."` and `one_shot` (by-Ref Routes only).
    pub(crate) ref_args: RefArgs,
    /// The Route Guards: `owner`, `permissions` and `guard` (AD-23 stage 8).
    pub(crate) guards: GuardArgs,
    /// `flags = [..]`: the Boot Flags (FR-73).
    pub(crate) flags: Flags,
}

/// The Route Guard arguments of a kind attribute (FR-19, AD-23 stage 8);
/// none of them changes the encoding.
#[derive(Clone, Default)]
pub(crate) struct GuardArgs {
    /// `owner = field`: the Payload field of type `Id<UserMarker>`.
    pub(crate) owner: Option<Ident>,
    /// `permissions = A | B`, as permission bits.
    pub(crate) permissions: Option<u64>,
    /// `guard = path::to::fn`.
    pub(crate) guard: Option<syn::Path>,
}

impl GuardArgs {
    /// Whether any Guard is declared.
    pub(crate) fn any(&self) -> bool {
        self.owner.is_some() || self.permissions.is_some() || self.guard.is_some()
    }
}

impl PartialEq for GuardArgs {
    fn eq(&self, other: &Self) -> bool {
        let path = |p: &Option<syn::Path>| p.as_ref().map(|p| quote!(#p).to_string());
        self.owner == other.owner
            && self.permissions == other.permissions
            && path(&self.guard) == path(&other.guard)
    }
}

impl Eq for GuardArgs {}

impl core::fmt::Debug for GuardArgs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let guard = self.guard.as_ref().map(|p| quote!(#p).to_string());
        f.debug_struct("GuardArgs")
            .field("owner", &self.owner)
            .field("permissions", &self.permissions)
            .field("guard", &guard)
            .finish()
    }
}

/// The Ref arguments of a by-Ref Route (AD-6, AD-18).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RefArgs {
    /// `ttl = ".."` in seconds.
    pub(crate) ttl_secs: Option<u64>,
    /// `one_shot`.
    pub(crate) one_shot: bool,
}

/// The argument names after the id, besides `defaults(..)`.
const ARG_NAMES: [&str; 12] = [
    "by_ref",
    "overflow",
    "schema",
    "signed",
    "response",
    "auto_defer",
    "ttl",
    "one_shot",
    "owner",
    "permissions",
    "guard",
    "flags",
];

/// The runtime arguments of AD-6 that a later story adds (`rate_limit` in
/// 13.4).
const RUNTIME_ARGS: [&str; 1] = ["rate_limit"];

const TTL_HELP: &str = "write ttl = \"15m\" (units d, h, m, s, such as \"1h30m\")";

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
    response: Option<Response>,
    auto_defer: Option<bool>,
    ttl: Option<(Ident, u64)>,
    one_shot: Option<Ident>,
    guards: GuardArgs,
    flags: Flags,
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
    if seen.by_ref.is_none() && seen.spill.is_none() {
        let names = [
            seen.ttl.as_ref().map(|(name, _)| name),
            seen.one_shot.as_ref(),
        ];
        for name in names.into_iter().flatten() {
            errors.push(cid_03(
                name,
                format!("`{name}` needs a by-Ref Route"),
                "add by_ref or overflow = spill: only a Ref has a TTL or is consumed on use",
            ));
        }
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
            response: seen.response.unwrap_or(Response::Update),
            auto_defer: seen.auto_defer.unwrap_or(true),
            ref_args: RefArgs {
                ttl_secs: seen.ttl.map(|(_, secs)| secs),
                one_shot: seen.one_shot.is_some(),
            },
            guards: seen.guards,
            flags: seen.flags,
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
    if let Some(error) = unsupported_runtime_arg(name, &key, a) {
        errors.push(error);
        return;
    }
    if key == "defaults" {
        parse_defaults(name, rest, segment, a, seen, errors);
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
    if !ARG_NAMES.contains(&key.as_str()) {
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
        ("by_ref" | "signed" | "one_shot", Some(_)) => errors.push(cid_03(
            stream(segment),
            format!("`{key}` is a flag and takes no value"),
            format!("write `{key}` alone"),
        )),
        ("by_ref", None) => seen.by_ref = Some(name.clone()),
        ("signed", None) => seen.signed = Some(name.clone()),
        ("one_shot", None) => seen.one_shot = Some(name.clone()),
        ("ttl", value) => parse_ttl(name, value, seen, errors),
        ("overflow", value) => parse_overflow(name, value, segment, kind, seen, errors),
        ("response", value) => parse_response(name, value, seen, errors),
        ("auto_defer", value) => parse_auto_defer(name, value, a, seen, errors),
        ("owner", value) => parse_owner(name, value, seen, errors),
        ("permissions", value) => {
            match crate::meta::permissions(name.span(), value.unwrap_or_default()) {
                Ok(bits) => seen.guards.permissions = Some(bits),
                Err(error) => errors.push(error),
            }
        }
        ("guard", value) => parse_guard(name, value, a, seen, errors),
        ("flags", _) => match Flags::parse(segment) {
            Ok(flags) => seen.flags = flags,
            Err(error) => errors.push(error),
        },
        (_, value) => parse_schema(name, value, seen, errors),
    }
}

/// `defaults(..)`: its raw tokens, once.
fn parse_defaults(
    name: &Ident,
    rest: &[TokenTree],
    segment: &[TokenTree],
    a: &str,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    match rest {
        [TokenTree::Group(group)] if group.delimiter() == proc_macro2::Delimiter::Parenthesis => {
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
}

/// The "not supported yet" error of a runtime argument this release does
/// not take on `#[attr]` (only `rate_limit`, Story 13.4).
fn unsupported_runtime_arg(name: &Ident, key: &str, attr: &str) -> Option<syn::Error> {
    RUNTIME_ARGS.contains(&key).then(|| {
        cid_03(
            name,
            format!("`{key}` is a runtime argument of #[{attr}] that this Noctis release does not support yet"),
            "remove it for now; runtime arguments arrive with their release",
        )
    })
}

/// `response = update | ephemeral | public` (a bare identifier).
fn parse_response(
    name: &Ident,
    value: Option<&[TokenTree]>,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    let parsed = match value {
        Some([TokenTree::Ident(v)]) if v == "update" => Some(Response::Update),
        Some([TokenTree::Ident(v)]) if v == "ephemeral" => Some(Response::Ephemeral),
        Some([TokenTree::Ident(v)]) if v == "public" => Some(Response::Public),
        _ => None,
    };
    if let Some(response) = parsed {
        seen.response = Some(response);
    } else {
        errors.push(cid_03(
            value_tokens(name, value),
            "`response` takes `update`, `ephemeral` or `public`",
            "write `response = update` (the default: answer by updating the clicked message), `response = ephemeral` or `response = public` (a new message)",
        ));
    }
}

/// `owner = field` (a bare identifier).
fn parse_owner(
    name: &Ident,
    value: Option<&[TokenTree]>,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    if let Some([TokenTree::Ident(field)]) = value {
        seen.guards.owner = Some(field.clone());
    } else {
        errors.push(cid_03(
            value_tokens(name, value),
            "`owner` takes a field name such as `owner = user`",
            "name the Payload field of type `Id<UserMarker>` whose user alone may use this Route",
        ));
    }
}

/// `guard = path::to::fn` (a path without generic arguments).
fn parse_guard(
    name: &Ident,
    value: Option<&[TokenTree]>,
    attr: &str,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    use syn::parse::Parser as _;
    let path = value
        .filter(|v| !v.is_empty())
        .and_then(|v| syn::Path::parse_mod_style.parse2(stream(v)).ok());
    if let Some(path) = path {
        seen.guards.guard = Some(path);
    } else {
        let ctx = if attr == "modal" {
            "ModalSubmitCtx"
        } else {
            "ComponentCtx"
        };
        errors.push(cid_03(
            value_tokens(name, value),
            "`guard` takes a function path such as `guard = auth::is_staff`",
            format!(
                "name an `async fn(ctx: &{ctx}, this: &Self) -> HandlerResult<Access>`, without generic arguments"
            ),
        ));
    }
}

/// `auto_defer = true | false` (a bool literal); the help names the
/// `Manual` responder of `#[attr]`'s kind.
fn parse_auto_defer(
    name: &Ident,
    value: Option<&[TokenTree]>,
    attr: &str,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    let lit = match value {
        Some([TokenTree::Ident(v)]) if v == "true" => Some(true),
        Some([TokenTree::Ident(v)]) if v == "false" => Some(false),
        _ => None,
    };
    if let Some(auto_defer) = lit {
        seen.auto_defer = Some(auto_defer);
    } else {
        let responder = if attr == "modal" {
            "ModalResponder<Manual>"
        } else {
            "ComponentResponder<Manual>"
        };
        errors.push(cid_03(
            value_tokens(name, value),
            "`auto_defer` takes `true` or `false`",
            format!(
                "write `auto_defer = false` for a handler that answers within 3 s itself (it takes `{responder}`), or leave it out"
            ),
        ));
    }
}

/// The tokens an argument value error points at: the value, or the name
/// when there is none.
fn value_tokens(name: &Ident, value: Option<&[TokenTree]>) -> TokenStream {
    match value {
        Some(value) if !value.is_empty() => stream(value),
        _ => quote!(#name),
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

/// `ttl = "<duration>"` (a string literal, `crate::duration`'s grammar).
fn parse_ttl(
    name: &Ident,
    value: Option<&[TokenTree]>,
    seen: &mut Seen,
    errors: &mut Vec<syn::Error>,
) {
    let lit = match value {
        Some([TokenTree::Literal(l)]) => {
            match syn::parse2::<Lit>(TokenTree::Literal(l.clone()).into()) {
                Ok(Lit::Str(s)) => Some(s),
                _ => None,
            }
        }
        _ => None,
    };
    let Some(lit) = lit else {
        errors.push(cid_03(
            value_tokens(name, value),
            "`ttl` takes a duration string",
            TTL_HELP,
        ));
        return;
    };
    let text = lit.value();
    match parse_secs(&text) {
        Ok(secs) => seen.ttl = Some((name.clone(), secs)),
        Err(DurationError::Syntax) => errors.push(cid_03(
            &lit,
            format!("`ttl = {text:?}` is not a duration"),
            TTL_HELP,
        )),
        Err(DurationError::OutOfRange) => errors.push(cid_03(
            &lit,
            format!("`ttl` must be between 1s and 365d, found `{text}`"),
            TTL_HELP,
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
    let ttl = args
        .ref_args
        .ttl_secs
        .map(|secs| quote!(.with_ttl_secs(#secs)));
    let one_shot = args.ref_args.one_shot.then(|| quote!(.one_shot()));
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
            const RUNTIME: #codec::RouteRuntime = #codec::RouteRuntime::EMPTY #ttl #one_shot;
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
            response: super::Response::Update,
            auto_defer: true,
            ref_args: super::RefArgs::default(),
            guards: super::GuardArgs::default(),
            flags: crate::flags::Flags::default(),
        }
    }

    const OWNER_TEXT: &str = "[CID-03] `owner` takes a field name such as `owner = user`";
    const GUARD_TEXT: &str =
        "[CID-03] `guard` takes a function path such as `guard = auth::is_staff`";

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
            "#[button(\"a\", rate_limit(user, 5, per = \"1m\"))] struct R;",
            "[CID-03] `rate_limit` is a runtime argument of #[button] that this Noctis release does not support yet",
        ),
        ("#[button(\"a\", owner)] struct R;", OWNER_TEXT),
        ("#[button(\"a\", owner = \"user\")] struct R;", OWNER_TEXT),
        ("#[button(\"a\", owner = a.b)] struct R;", OWNER_TEXT),
        ("#[button(\"a\", owner = 1)] struct R;", OWNER_TEXT),
        ("#[button(\"a\", guard)] struct R;", GUARD_TEXT),
        ("#[button(\"a\", guard = \"f\")] struct R;", GUARD_TEXT),
        ("#[button(\"a\", guard = f::<T>)] struct R;", GUARD_TEXT),
        ("#[button(\"a\", guard = 1)] struct R;", GUARD_TEXT),
        (
            "#[button(\"a\", owner = a, owner = b)] struct R;",
            "[CID-03] #[button] argument `owner` is given twice",
        ),
        (
            "#[button(\"a\", guard = f, guard = g)] struct R;",
            "[CID-03] #[button] argument `guard` is given twice",
        ),
        (
            "#[button(\"a\", permissions = KICK_MEMBERS, permissions = BAN_MEMBERS)] struct R;",
            "[CID-03] #[button] argument `permissions` is given twice",
        ),
        (
            "#[button(\"a\", ttl = \"1m\")] struct R;",
            "[CID-03] `ttl` needs a by-Ref Route",
        ),
        (
            "#[button(\"a\", one_shot)] struct R;",
            "[CID-03] `one_shot` needs a by-Ref Route",
        ),
        (
            "#[button(\"a\", by_ref, ttl = 15)] struct R;",
            "[CID-03] `ttl` takes a duration string",
        ),
        (
            "#[button(\"a\", by_ref, ttl)] struct R;",
            "[CID-03] `ttl` takes a duration string",
        ),
        (
            "#[button(\"a\", by_ref, ttl = b\"1m\")] struct R;",
            "[CID-03] `ttl` takes a duration string",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"1m\" \"2m\")] struct R;",
            "[CID-03] `ttl` takes a duration string",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"15x\")] struct R;",
            "[CID-03] `ttl = \"15x\"` is not a duration",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"1m1h\")] struct R;",
            "[CID-03] `ttl = \"1m1h\"` is not a duration",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"0s\")] struct R;",
            "[CID-03] `ttl` must be between 1s and 365d, found `0s`",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"366d\")] struct R;",
            "[CID-03] `ttl` must be between 1s and 365d, found `366d`",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"99999999999999999999d\")] struct R;",
            "[CID-03] `ttl` must be between 1s and 365d",
        ),
        (
            "#[button(\"a\", by_ref, one_shot = true)] struct R;",
            "[CID-03] `one_shot` is a flag and takes no value",
        ),
        (
            "#[button(\"a\", by_ref, one_shot, one_shot)] struct R;",
            "[CID-03] #[button] argument `one_shot` is given twice",
        ),
        (
            "#[button(\"a\", by_ref, ttl = \"1m\", ttl = \"2m\")] struct R;",
            "[CID-03] #[button] argument `ttl` is given twice",
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
            "#[button(\"a\", response = edit)] struct R;",
            "[CID-03] `response` takes `update`, `ephemeral` or `public`",
        ),
        (
            "#[button(\"a\", response = \"update\")] struct R;",
            "[CID-03] `response` takes `update`, `ephemeral` or `public`",
        ),
        (
            "#[button(\"a\", response)] struct R;",
            "[CID-03] `response` takes `update`, `ephemeral` or `public`",
        ),
        (
            "#[button(\"a\", auto_defer = 1)] struct R;",
            "[CID-03] `auto_defer` takes `true` or `false`",
        ),
        (
            "#[button(\"a\", auto_defer = yes)] struct R;",
            "[CID-03] `auto_defer` takes `true` or `false`",
        ),
        (
            "#[button(\"a\", response = public, response = update)] struct R;",
            "[CID-03] #[button] argument `response` is given twice",
        ),
        (
            "#[button(\"a\", auto_defer = true, auto_defer = false)] struct R;",
            "[CID-03] #[button] argument `auto_defer` is given twice",
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

    /// Story 7.2 Matrix 52: `response` and `auto_defer`.
    #[test]
    fn response_and_auto_defer_parse() {
        for (response, want) in [
            (quote::quote!(update), super::Response::Update),
            (quote::quote!(ephemeral), super::Response::Ephemeral),
            (quote::quote!(public), super::Response::Public),
        ] {
            let input: DeriveInput = parse_quote!(
                #[button("a", response = #response, auto_defer = false)]
                struct R;
            );
            assert_eq!(
                parse(&input).map(|a| (a.response, a.auto_defer)),
                Ok((want, false))
            );
        }
        assert_eq!(
            parse(&parse_quote!(
                #[button("a", auto_defer = true)]
                struct R;
            ))
            .map(|a| (a.response, a.auto_defer)),
            Ok((super::Response::Update, true))
        );
    }

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
                body: Body::Spill,
                schema: 61,
                signed: true,
                ..args("a")
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

    const MODAL: AttrKind = AttrKind {
        attr: "modal",
        derive: "Modal",
        select: false,
    };

    fn parse_modal(input: &DeriveInput) -> Result<RouteArgs, String> {
        parse_kind_attr(&input.attrs, &input.ident, MODAL)
            .map(|k| k.args)
            .map_err(|e| {
                e.into_iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            })
    }

    /// Story 7.8 rows 1-4: `#[modal]` takes `response` and `auto_defer`
    /// with the component grammar.
    #[test]
    fn modal_response_and_auto_defer() {
        use super::Response;
        let declared = |input: &DeriveInput| parse_modal(input).map(|a| (a.response, a.auto_defer));
        assert_eq!(
            declared(&parse_quote!(
                #[modal("m", defaults(title = "T"))]
                struct R;
            )),
            Ok((Response::Update, true))
        );
        for (input, response) in [
            (
                parse_quote!(
                    #[modal("m", response = update, defaults(title = "T"))]
                    struct R;
                ),
                Response::Update,
            ),
            (
                parse_quote!(
                    #[modal("m", response = ephemeral, defaults(title = "T"))]
                    struct R;
                ),
                Response::Ephemeral,
            ),
            (
                parse_quote!(
                    #[modal("m", defaults(title = "T"), response = public)]
                    struct R;
                ),
                Response::Public,
            ),
        ] {
            assert_eq!(declared(&input), Ok((response, true)));
        }
        assert_eq!(
            declared(&parse_quote!(
                #[modal("m", auto_defer = false, defaults(title = "T"))]
                struct R;
            )),
            Ok((Response::Update, false))
        );
        // Any order, around `defaults`, the Guards and `by_ref`.
        for input in [
            parse_quote!(
                #[modal("m", auto_defer = true, response = public, defaults(title = "T"))]
                struct R;
            ),
            parse_quote!(
                #[modal("m", defaults(title = "T"), owner = user, response = public, auto_defer = true)]
                struct R;
            ),
            parse_quote!(
                #[modal("m", by_ref, response = public, guard = f, auto_defer = true, defaults(title = "T"))]
                struct R;
            ),
        ] {
            assert_eq!(declared(&input), Ok((Response::Public, true)));
        }
    }

    /// Story 7.8 rows 5-8: the `#[modal]` value, duplicate and
    /// still-rejected argument errors, with the kind's `Manual` responder in
    /// the `auto_defer` help.
    #[test]
    fn modal_response_and_auto_defer_errors() {
        const RESPONSE: &str = "[CID-03] `response` takes `update`, `ephemeral` or `public`";
        const AUTO_DEFER: &str = "[CID-03] `auto_defer` takes `true` or `false`";
        let rejected: [(&str, &str); 12] = [
            ("response = edit", RESPONSE),
            ("response = \"update\"", RESPONSE),
            ("response", RESPONSE),
            ("response = Update", RESPONSE),
            ("auto_defer = 0", AUTO_DEFER),
            ("auto_defer = \"false\"", AUTO_DEFER),
            ("auto_defer", AUTO_DEFER),
            ("auto_defer = no", AUTO_DEFER),
            (
                "response = update, response = public",
                "[CID-03] #[modal] argument `response` is given twice",
            ),
            (
                "auto_defer = true, auto_defer = false",
                "[CID-03] #[modal] argument `auto_defer` is given twice",
            ),
            (
                "rate_limit(user, 3)",
                "[CID-03] `rate_limit` is a runtime argument of #[modal] that this Noctis release does not support yet",
            ),
            ("response = edit, auto_defer = 1", RESPONSE),
        ];
        for (args, expected) in rejected {
            let input: DeriveInput = syn::parse_str(&format!(
                "#[modal(\"m\", {args}, defaults(title = \"T\"))] struct R;"
            ))
            .unwrap();
            let error = parse_modal(&input).unwrap_err();
            assert!(error.starts_with(expected), "{error}");
            if expected == AUTO_DEFER {
                assert!(
                    error.contains("it takes `ModalResponder<Manual>`"),
                    "{error}"
                );
            }
        }
        // The help names the kind's responder.
        let error = parse(&parse_quote!(
            #[button("a", auto_defer = 1)]
            struct R;
        ))
        .unwrap_err();
        assert!(
            error.contains("it takes `ComponentResponder<Manual>`"),
            "{error}"
        );
    }

    /// Matrix rows 1 and 7: accepted `ttl`/`one_shot`, on components and
    /// on `#[modal]`, and the emitted `RUNTIME`.
    #[test]
    fn ttl_and_one_shot_parse_and_emit() {
        let ttl =
            |input: &DeriveInput| parse(input).map(|a| (a.ref_args.ttl_secs, a.ref_args.one_shot));
        assert_eq!(
            ttl(&parse_quote!(
                #[button("a", by_ref, ttl = "15m", one_shot)]
                struct R;
            )),
            Ok((Some(900), true))
        );
        assert_eq!(
            ttl(&parse_quote!(
                #[button("a", overflow = spill, one_shot)]
                struct R;
            )),
            Ok((None, true))
        );
        assert_eq!(
            ttl(&parse_quote!(
                #[button("a", by_ref, ttl = r"1h30m")]
                struct R;
            )),
            Ok((Some(5_400), false))
        );
        let modal = AttrKind {
            attr: "modal",
            derive: "Modal",
            select: false,
        };
        let input: DeriveInput = parse_quote!(
            #[modal("m", by_ref, ttl = "1h", defaults(title = "t"))]
            struct R;
        );
        let parsed = parse_kind_attr(&input.attrs, &input.ident, modal).unwrap();
        assert_eq!(
            (parsed.args.ref_args.ttl_secs, parsed.args.ref_args.one_shot),
            (Some(3_600), false)
        );

        let ident: syn::Ident = parse_quote!(R);
        let mut a = args("a");
        let plain = route_impls(&ident, &a, &quote::quote!(Self)).to_string();
        assert!(
            plain.contains(
                "RouteRuntime = :: noctis :: __private :: codec :: RouteRuntime :: EMPTY ;"
            ),
            "{plain}"
        );
        a.body = Body::ByRef;
        a.ref_args = super::RefArgs {
            ttl_secs: Some(900),
            one_shot: true,
        };
        let emitted = route_impls(&ident, &a, &quote::quote!(Self)).to_string();
        assert!(
            emitted.contains("RouteRuntime :: EMPTY . with_ttl_secs (900u64) . one_shot () ;"),
            "{emitted}"
        );
    }

    /// Both inline errors are reported, each spanned on its name.
    #[test]
    fn ttl_and_one_shot_on_an_inline_route() {
        let m = parse(&parse_quote!(
            #[button("a", ttl = "1m", one_shot)]
            struct R;
        ))
        .unwrap_err();
        assert!(m.contains("[CID-03] `ttl` needs a by-Ref Route"), "{m}");
        assert!(
            m.contains("[CID-03] `one_shot` needs a by-Ref Route"),
            "{m}"
        );
        assert!(
            m.contains(
                "help: add by_ref or overflow = spill: only a Ref has a TTL or is consumed on use"
            ),
            "{m}"
        );
        let m = parse(&parse_quote!(
            #[button("a", by_ref, ttl = "15 m")]
            struct R;
        ))
        .unwrap_err();
        assert_eq!(
            m,
            "[CID-03] `ttl = \"15 m\"` is not a duration\n  = help: write ttl = \"15m\" (units d, h, m, s, such as \"1h30m\")"
        );
    }

    /// Route Guard Matrix 1-3 and 15: the accepted Guard arguments, in any
    /// position, leave the codec arguments alone.
    #[test]
    fn guard_arguments_parse() {
        let input: DeriveInput = parse_quote!(
            #[button("a", owner = user, defaults(style = Primary), schema = 2)]
            struct R;
        );
        let parsed = parse(&input).unwrap();
        assert_eq!(
            parsed.guards.owner.map(|o| o.to_string()).as_deref(),
            Some("user")
        );
        assert_eq!((parsed.schema, parsed.body), (2, Body::Inline));
        assert_eq!(parsed.guards.permissions, None);
        let bits = |input: &DeriveInput| parse(input).map(|a| a.guards.permissions);
        assert_eq!(
            bits(&parse_quote!(
                #[button("a", permissions = MANAGE_GUILD | Permissions::BAN_MEMBERS)]
                struct R;
            )),
            Ok(Some(32 | 4))
        );
        assert_eq!(
            bits(&parse_quote!(
                #[button("a", permissions = KICK_MEMBERS | KICK_MEMBERS)]
                struct R;
            )),
            Ok(Some(2))
        );
        let all = noctis_rules::command::PERMISSIONS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(" | ");
        let input: DeriveInput =
            syn::parse_str(&format!("#[button(\"a\", permissions = {all})] struct R;")).unwrap();
        let union = noctis_rules::command::PERMISSIONS
            .iter()
            .fold(0, |acc, (_, bit)| acc | bit);
        assert_eq!(bits(&input), Ok(Some(union)));
        let parsed = parse(&parse_quote!(
            #[button("a", guard = auth::is_staff, by_ref)]
            struct R;
        ))
        .unwrap();
        let guard = parsed.guards.guard.as_ref().unwrap();
        assert_eq!(quote::quote!(#guard).to_string(), "auth :: is_staff");
        assert_eq!(parsed.body, Body::ByRef);
        let parsed = parse(&parse_quote!(
            #[button("a", guard = ::crate_a::f)]
            struct R;
        ))
        .unwrap();
        assert!(parsed.guards.guard.is_some());
        // Matrix 15: nothing declared.
        assert!(
            !parse(&parse_quote!(
                #[button("a")]
                struct R;
            ))
            .unwrap()
            .guards
            .any()
        );
        // On `#[modal]` too.
        let modal = AttrKind {
            attr: "modal",
            derive: "Modal",
            select: false,
        };
        let input: DeriveInput = parse_quote!(
            #[modal("m", owner = user, permissions = ADMINISTRATOR, guard = f, defaults(title = "t"))]
            struct R;
        );
        let parsed = parse_kind_attr(&input.attrs, &input.ident, modal).unwrap();
        assert!(parsed.args.guards.any());
        assert_eq!(parsed.args.guards.permissions, Some(8));
    }

    /// Route Guard Matrix 5 and 6: the permission errors keep META-01's text.
    #[test]
    fn guard_permission_errors() {
        for value in ["", " = 8", " = \"X\"", " = A + B", " = A || B", " = A |"] {
            let input: DeriveInput =
                syn::parse_str(&format!("#[button(\"a\", permissions{value})] struct R;")).unwrap();
            assert_eq!(
                parse(&input).unwrap_err(),
                crate::meta::PERMISSIONS_SYNTAX,
                "{value}"
            );
        }
        let m = parse(&parse_quote!(
            #[button("a", permissions = MANAGE_SERVER)]
            struct R;
        ))
        .unwrap_err();
        assert!(m.starts_with("[META-01]"), "{m}");
        assert!(m.contains("see noctis::rules::command::PERMISSIONS"), "{m}");
        let m = parse(&parse_quote!(
            #[button("a", permissions = manage_guild)]
            struct R;
        ))
        .unwrap_err();
        assert!(m.contains("use `MANAGE_GUILD`"), "{m}");
    }

    /// Route Guard Matrix 7 and 11: the exact Guard argument errors.
    #[test]
    fn guard_argument_errors() {
        assert_eq!(
            parse(&parse_quote!(
                #[button("a", owner = a.b)]
                struct R;
            ))
            .unwrap_err(),
            "[CID-03] `owner` takes a field name such as `owner = user`\n  = help: name the Payload field of type `Id<UserMarker>` whose user alone may use this Route"
        );
        assert_eq!(
            parse(&parse_quote!(
                #[button("a", guard = 1)]
                struct R;
            ))
            .unwrap_err(),
            "[CID-03] `guard` takes a function path such as `guard = auth::is_staff`\n  = help: name an `async fn(ctx: &ComponentCtx, this: &Self) -> HandlerResult<Access>`, without generic arguments"
        );
    }
}
