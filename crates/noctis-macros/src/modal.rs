//! `#[derive(Modal)]` with `#[modal("id", <codec args>, defaults(title =
//! "…"))]`: a struct as a modal and its own Route (AD-6, AD-19, FR-29,
//! FR-58). Fields without an input attribute are the Payload, encoded in
//! the `custom_id` and length-proven like a component Route (CID-01);
//! every field with an input attribute is one top-level component. Every
//! literal text and count of the inputs is checked at expansion by
//! lowering the form into `noctis_rules::modal` descriptors and running
//! `validate_modal`, then CID-01 per input key and CV2-04 on the Text
//! Displays (MOD-01, MOD-02, MOD-04..07, CV2-09). Choices-backed option sets
//! lower to sentinel options of the largest legal count, so only literal
//! facts are judged; their real counts are trait bounds on the field type.

use std::collections::HashMap;

use noctis_rules::component::{self as rc, SelectKind};
use noctis_rules::len::discord_len;
use noctis_rules::modal::{self as rm, validate_input_custom_id, validate_modal};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Literal, Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Ident, Lit, LitInt, LitStr, Token, Type};

use crate::common::{combine, error, option};
use crate::defaults::{Defaults, DefaultsKind, TextValue};
use crate::route::{AttrKind, RouteArgs, parse_kind_attr, route_impls};

/// The help of a localized or non-literal input attribute text.
const LOCALIZED: &str =
    "input attribute texts are string literals; localize them per render with `InputOverrides`";

/// The options a Radio Group lowers to: the largest legal count.
static RADIO_SENTINEL: [rm::ModalOption<'static>; 10] = [rm::ModalOption::new("x", "x"); 10];
/// The options a String Select lowers to: the largest legal count.
static SELECT_SENTINEL: [rc::SelectOption<'static>; 25] = [rc::SelectOption::new("x", "x"); 25];

/// A field's component kind: its attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    TextInput,
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect,
    FileUpload,
    RadioGroup,
    CheckboxGroup,
    Checkbox,
    TextDisplay,
}

impl Kind {
    const ALL: [Self; 11] = [
        Self::TextInput,
        Self::StringSelect,
        Self::UserSelect,
        Self::RoleSelect,
        Self::MentionableSelect,
        Self::ChannelSelect,
        Self::FileUpload,
        Self::RadioGroup,
        Self::CheckboxGroup,
        Self::Checkbox,
        Self::TextDisplay,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::TextInput => "text_input",
            Self::StringSelect => "string_select",
            Self::UserSelect => "user_select",
            Self::RoleSelect => "role_select",
            Self::MentionableSelect => "mentionable_select",
            Self::ChannelSelect => "channel_select",
            Self::FileUpload => "file_upload",
            Self::RadioGroup => "radio_group",
            Self::CheckboxGroup => "checkbox_group",
            Self::Checkbox => "checkbox",
            Self::TextDisplay => "text_display",
        }
    }

    fn of(attr: &Attribute) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| attr.path().is_ident(k.name()))
    }

    /// The named arguments the attribute takes.
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::TextInput => &[
                "description",
                "style",
                "min_length",
                "max_length",
                "placeholder",
            ],
            Self::StringSelect | Self::UserSelect | Self::RoleSelect | Self::MentionableSelect => {
                &["description", "placeholder", "min_values", "max_values"]
            }
            Self::ChannelSelect => &[
                "description",
                "placeholder",
                "min_values",
                "max_values",
                "channel_types",
            ],
            Self::FileUpload => &["description", "min_values", "max_values", "file_types"],
            Self::CheckboxGroup => &["description", "min_values", "max_values"],
            Self::RadioGroup | Self::Checkbox => &["description"],
            Self::TextDisplay => &[],
        }
    }

    /// The path segment `noctis_rules::modal` names the input by.
    fn rules_path(self) -> &'static str {
        match self {
            Self::TextInput => "text_input",
            Self::StringSelect
            | Self::UserSelect
            | Self::RoleSelect
            | Self::MentionableSelect
            | Self::ChannelSelect => "select",
            Self::FileUpload => "file_upload",
            Self::RadioGroup => "radio_group",
            Self::CheckboxGroup => "checkbox_group",
            Self::Checkbox => "checkbox",
            Self::TextDisplay => "text",
        }
    }

    /// The rule of the placeholder.
    fn placeholder_rule(self) -> RuleId {
        if self == Self::TextInput {
            RuleId::MOD_05
        } else {
            RuleId::CV2_09
        }
    }
}

/// One attribute argument, before interpretation.
enum RawArg {
    /// A positional expression (the Label text or Text Display content).
    Positional(Expr),
    /// `name = expr`.
    Named(Ident, Expr),
    /// `name(..)`, such as `title(key = "…")`.
    Call(Ident),
}

impl Parse for RawArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(Ident) && input.peek2(Token![=]) && !input.peek2(Token![==]) {
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            return Ok(Self::Named(name, input.parse()?));
        }
        if input.peek(Ident) && input.peek2(syn::token::Paren) {
            let name: Ident = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            let _: TokenStream = content.parse()?;
            return Ok(Self::Call(name));
        }
        Ok(Self::Positional(input.parse()?))
    }
}

fn parse_args(attr: &Attribute) -> syn::Result<Vec<RawArg>> {
    if matches!(attr.meta, syn::Meta::Path(_)) {
        return Ok(Vec::new());
    }
    attr.parse_args_with(Punctuated::<RawArg, Token![,]>::parse_terminated)
        .map(|p| p.into_iter().collect())
}

fn lit_str(expr: &Expr) -> Option<&LitStr> {
    match expr {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Str(s), ..
        }) => Some(s),
        Expr::Group(g) => lit_str(&g.expr),
        _ => None,
    }
}

fn lit_int(expr: &Expr) -> Option<&LitInt> {
    match expr {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => Some(i),
        Expr::Group(g) => lit_int(&g.expr),
        _ => None,
    }
}

fn is_path(expr: &Expr) -> bool {
    match expr {
        Expr::Path(_) => true,
        Expr::Group(g) => is_path(&g.expr),
        _ => false,
    }
}

/// A text argument that must be a literal; a path is a locale key path,
/// which input attributes do not take (localize per render instead).
fn text_arg(expr: &Expr, rule: RuleId, what: &str) -> syn::Result<LitStr> {
    if is_path(expr) {
        return Err(syn::Error::new_spanned(
            expr,
            format!("[{rule}] {what} is a locale key path: {LOCALIZED}"),
        ));
    }
    lit_str(expr).cloned().ok_or_else(|| {
        error(
            expr,
            rule,
            format!("{what} must be a string literal"),
            LOCALIZED,
        )
    })
}

fn localized(tokens: impl ToTokens, rule: RuleId, what: &str) -> syn::Error {
    error(
        tokens,
        rule,
        format!("{what} must be a string literal"),
        LOCALIZED,
    )
}

/// The parsed `#[modal(..)]` attribute.
struct ModalAttr {
    args: RouteArgs,
    title: TextValue,
}

/// The help every withdrawn `#[modal(route = ..)]` form shows.
const STRUCT_FORM: &str = "#[derive(Modal)] #[modal(\"bio\", defaults(title = \"Bio\"))] struct BioForm { user: Id<UserMarker>, #[text_input(\"Bio\")] bio: String } impl BioForm { #[handler] async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> { .. } }";

const MODAL_KIND: AttrKind = AttrKind {
    attr: "modal",
    derive: "Modal",
    select: false,
};

fn parse_modal_attr(input: &DeriveInput) -> syn::Result<ModalAttr> {
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("modal")) {
        if let syn::Meta::List(list) = &attr.meta {
            let withdrawn = crate::command::segments(list.tokens.clone())
                .iter()
                .any(|segment| {
                    matches!(
                        segment.as_slice(),
                        [proc_macro2::TokenTree::Ident(name), proc_macro2::TokenTree::Punct(eq), ..]
                            if (name == "route" || name == "title") && eq.as_char() == '='
                    )
                });
            if withdrawn {
                return Err(syn::Error::new_spanned(
                    attr,
                    format!(
                        "`#[modal(route = …, title = \"…\")]` is withdrawn: a modal struct is its own Route, with its id first and its title in `defaults(..)`\n  = help: {STRUCT_FORM}"
                    ),
                ));
            }
        }
    }
    let attr = parse_kind_attr(&input.attrs, &input.ident, MODAL_KIND)?;
    let Defaults::Modal { title } =
        crate::defaults::parse(DefaultsKind::Modal, attr.defaults.as_ref(), &attr.attr)?
    else {
        return Err(syn::Error::new_spanned(
            &attr.attr,
            DefaultsKind::Modal.needs(),
        ));
    };
    Ok(ModalAttr {
        args: attr.args,
        title,
    })
}

/// A parsed field.
struct FieldDef {
    ident: Ident,
    vis: syn::Visibility,
    key: String,
    ty: Type,
    kind: Kind,
    attr: Span,
    text: LitStr,
    description: Option<LitStr>,
    paragraph: bool,
    min: Option<(usize, LitInt)>,
    max: Option<(usize, LitInt)>,
    placeholder: Option<LitStr>,
    channel_types: Vec<Ident>,
    file_types: Vec<LitStr>,
    file_types_span: Option<Span>,
}

impl FieldDef {
    fn is_vec(&self) -> bool {
        is_vec(&self.ty)
    }
}

fn is_vec(ty: &Type) -> bool {
    match ty {
        Type::Path(p) if p.qself.is_none() => {
            p.path.segments.last().is_some_and(|s| s.ident == "Vec")
        }
        Type::Group(g) => is_vec(&g.elem),
        _ => false,
    }
}

fn int_arg(name: &Ident, expr: &Expr) -> syn::Result<(usize, LitInt)> {
    let lit = lit_int(expr).ok_or_else(|| {
        syn::Error::new_spanned(
            expr,
            format!("`{name}` takes a non-negative integer literal"),
        )
    })?;
    Ok((lit.base10_parse()?, lit.clone()))
}

fn array(name: &Ident, expr: &Expr) -> syn::Result<Vec<Expr>> {
    match expr {
        Expr::Array(a) => Ok(a.elems.iter().cloned().collect()),
        other => Err(syn::Error::new_spanned(
            other,
            format!("`{name}` takes a list `[…]`"),
        )),
    }
}

/// The first argument: the Label text or Text Display content literal.
fn first_text(arg: Option<RawArg>, attr: &Attribute, kind: Kind) -> syn::Result<LitStr> {
    let (rule, what) = if kind == Kind::TextDisplay {
        (RuleId::CV2_04, "the Text Display content")
    } else {
        (RuleId::MOD_04, "the Label text")
    };
    match arg {
        Some(RawArg::Positional(expr)) => text_arg(&expr, rule, what),
        Some(RawArg::Named(name, _) | RawArg::Call(name)) if name == "key" => {
            Err(localized(&name, rule, what))
        }
        _ => Err(syn::Error::new_spanned(
            attr,
            format!(
                "#[{}] takes {what} literal as its first argument",
                kind.name()
            ),
        )),
    }
}

/// Parses one field's component attribute.
fn parse_field(
    ident: &Ident,
    ty: &Type,
    attr: &Attribute,
    kind: Kind,
    errors: &mut Vec<syn::Error>,
) -> Option<FieldDef> {
    let args = match parse_args(attr) {
        Ok(a) => a,
        Err(e) => {
            errors.push(e);
            return None;
        }
    };
    let mut args = args.into_iter();
    let text = match first_text(args.next(), attr, kind) {
        Ok(t) => t,
        Err(e) => {
            errors.push(e);
            return None;
        }
    };
    let mut def = FieldDef {
        ident: ident.clone(),
        vis: syn::Visibility::Inherited,
        key: ident.unraw().to_string(),
        ty: ty.clone(),
        kind,
        attr: attr.path().span(),
        text,
        description: None,
        paragraph: false,
        min: None,
        max: None,
        placeholder: None,
        channel_types: Vec::new(),
        file_types: Vec::new(),
        file_types_span: None,
    };
    let before = errors.len();
    for arg in args {
        let (name, expr) = match arg {
            RawArg::Named(name, expr) => (name, expr),
            RawArg::Call(name) if name == "description" => {
                errors.push(localized(&name, RuleId::MOD_04, "the Label description"));
                continue;
            }
            RawArg::Call(name) if name == "placeholder" => {
                errors.push(localized(&name, kind.placeholder_rule(), "the placeholder"));
                continue;
            }
            RawArg::Call(name) => {
                errors.push(syn::Error::new_spanned(
                    &name,
                    format!("unknown #[{}] argument `{name}`", kind.name()),
                ));
                continue;
            }
            RawArg::Positional(expr) => {
                errors.push(syn::Error::new_spanned(
                    expr,
                    format!("#[{}] takes one positional text", kind.name()),
                ));
                continue;
            }
        };
        let n = name.to_string();
        if !kind.args().contains(&n.as_str()) {
            let expected = if kind.args().is_empty() {
                "it takes no other argument".to_owned()
            } else {
                format!("expected one of `{}`", kind.args().join("`, `"))
            };
            errors.push(syn::Error::new_spanned(
                &name,
                format!("unknown #[{}] argument `{name}`; {expected}", kind.name()),
            ));
            continue;
        }
        let result = apply_arg(&mut def, kind, &name, &expr);
        if let Err(e) = result {
            errors.push(e);
        }
        if (n == "min_values" || n == "max_values") && !def.is_vec() {
            errors.push(syn::Error::new_spanned(
                &name,
                format!("`{n}` is only for `Vec` fields; a single-valued field takes one value"),
            ));
        }
    }
    (errors.len() == before).then_some(def)
}

/// Applies one named argument (already checked against [`Kind::args`]).
fn apply_arg(def: &mut FieldDef, kind: Kind, name: &Ident, expr: &Expr) -> syn::Result<()> {
    match name.to_string().as_str() {
        "description" => text_arg(expr, RuleId::MOD_04, "the Label description")
            .map(|t| def.description = Some(t)),
        "placeholder" => text_arg(expr, kind.placeholder_rule(), "the placeholder")
            .map(|t| def.placeholder = Some(t)),
        "style" => match expr {
            Expr::Path(p) if p.path.is_ident("short") => Ok(()),
            Expr::Path(p) if p.path.is_ident("paragraph") => {
                def.paragraph = true;
                Ok(())
            }
            other => Err(syn::Error::new_spanned(
                other,
                "`style` is `short` or `paragraph`",
            )),
        },
        "min_length" | "min_values" => int_arg(name, expr).map(|v| def.min = Some(v)),
        "max_length" | "max_values" => int_arg(name, expr).map(|v| def.max = Some(v)),
        "channel_types" => array(name, expr).and_then(|elems| {
            for e in elems {
                match e {
                    Expr::Path(p) if p.qself.is_none() => {
                        if let Some(seg) = p.path.segments.last() {
                            def.channel_types.push(seg.ident.clone());
                        }
                    }
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "`channel_types` lists ChannelType variants such as `GuildText`",
                        ));
                    }
                }
            }
            Ok(())
        }),
        "file_types" => array(name, expr).and_then(|elems| {
            def.file_types_span = Some(expr.span());
            for e in &elems {
                let lit = lit_str(e).ok_or_else(|| {
                    syn::Error::new_spanned(e, "`file_types` lists string literals")
                })?;
                def.file_types.push(lit.clone());
            }
            Ok(())
        }),
        _ => Ok(()),
    }
}

/// The associated consts the derive's traits define on the struct that a
/// bot author names as `X::<CONST>`; an input's `X::<FIELD>` handle may not
/// shadow one. `__Kind::NAME` is left out on purpose: it is internal, only
/// ever read as `<X as __Kind>::NAME`, and `name` is a common input field.
const RESERVED_CONSTS: [&str; 8] = [
    "ID", "SHAPE", "SCHEMA", "DEFAULTS", "RUNTIME", "ROUTE", "RESPONSE", "GUARDED",
];

/// The `X::<FIELD>` handle name of an input: its unraw name in upper case.
fn screaming(key: &str) -> String {
    key.to_uppercase()
}

/// An input named `title` (the overrides' own field) or one whose handle
/// would shadow a derived associated const is an error.
fn check_reserved(def: &FieldDef) -> syn::Result<()> {
    // A Text Display has no overrides field, and neither it nor a File
    // Upload gets an `X::<FIELD>` handle.
    if def.key == "title" && def.kind != Kind::TextDisplay {
        return Err(syn::Error::new_spanned(
            &def.ident,
            format!(
                "input field `{}` cannot be named `title`: `<Modal>Overrides` has its own `title` field for the modal's title; rename the field",
                def.key
            ),
        ));
    }
    let handle = screaming(&def.key);
    if prefills(def.kind) && RESERVED_CONSTS.contains(&handle.as_str()) {
        return Err(syn::Error::new_spanned(
            &def.ident,
            format!(
                "input field `{}` cannot be named so: its `.value(..)` handle `{handle}` would shadow the associated const `{handle}` the derive's traits define; rename the field",
                def.key
            ),
        ));
    }
    Ok(())
}

/// Whether `ty` has an empty value `X::blank(..)` can fill in: it is
/// syntactically `Option<_>`, `Vec<_>`, `String`, `bool` or `()`. A type
/// alias counts as not empty-able.
fn is_blankable(ty: &Type) -> bool {
    match ty {
        Type::Tuple(t) => t.elems.is_empty(),
        Type::Group(g) => is_blankable(&g.elem),
        Type::Paren(p) => is_blankable(&p.elem),
        Type::Path(p) if p.qself.is_none() => p.path.segments.last().is_some_and(|s| {
            let name = s.ident.to_string();
            match name.as_str() {
                "Option" | "Vec" => {
                    matches!(s.arguments, syn::PathArguments::AngleBracketed(_))
                }
                "String" | "bool" => s.arguments.is_none(),
                _ => false,
            }
        }),
        _ => false,
    }
}

/// A Payload field: in the `custom_id`, not an input.
struct PayloadField {
    ident: Ident,
    ty: Type,
}

/// The struct, its inputs and its Payload fields, or every error found.
fn parse(input: &DeriveInput) -> syn::Result<(ModalAttr, Vec<FieldDef>, Vec<PayloadField>)> {
    let name = &input.ident;
    let shape_help = "a modal is a struct with named fields: the Payload fields, then one field per top-level component";
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            Fields::Unnamed(_) | Fields::Unit => {
                return Err(error(
                    name,
                    RuleId::MOD_02,
                    "`#[derive(Modal)]` needs a struct with named fields",
                    shape_help,
                ));
            }
        },
        _ => {
            return Err(error(
                name,
                RuleId::MOD_02,
                "`#[derive(Modal)]` needs a struct with named fields",
                shape_help,
            ));
        }
    };
    let mut errors = Vec::new();
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        errors.push(error(
            &input.generics,
            RuleId::MOD_02,
            "a modal form cannot be generic",
            "remove the type, lifetime and const parameters and the `where` clause",
        ));
    }
    let attr = parse_modal_attr(input);
    let mut defs = Vec::new();
    let mut payload = Vec::new();
    for field in fields {
        let Some(ident) = &field.ident else {
            continue;
        };
        let mut kinds = field
            .attrs
            .iter()
            .filter_map(|a| Kind::of(a).map(|k| (a, k)));
        let Some((attr, kind)) = kinds.next() else {
            payload.push(PayloadField {
                ident: ident.clone(),
                ty: field.ty.clone(),
            });
            continue;
        };
        if let Some((second, _)) = kinds.next() {
            errors.push(error(
                second,
                RuleId::MOD_03,
                format!("field `{ident}` has two component attributes"),
                "one field is one component: keep exactly one component attribute",
            ));
            continue;
        }
        if let Some(mut def) = parse_field(ident, &field.ty, attr, kind, &mut errors) {
            def.vis = field.vis.clone();
            if let Err(e) = check_reserved(&def) {
                errors.push(e);
            }
            defs.push(def);
        }
    }
    let attr = match attr {
        Ok(a) => Some(a),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    combine(errors)?;
    match attr {
        Some(attr) => Ok((attr, defs, payload)),
        None => Err(syn::Error::new_spanned(name, "missing #[modal]")),
    }
}

/// Where a violation path points.
#[derive(Default)]
struct Spans(HashMap<String, Span>);

impl Spans {
    fn insert(&mut self, path: impl Into<String>, span: Span) {
        self.0.insert(path.into(), span);
    }

    /// The span of `path`, or of its longest recorded prefix.
    fn of(&self, path: &str) -> Span {
        self.key_of(path)
            .and_then(|k| self.0.get(k).copied())
            .unwrap_or_else(Span::call_site)
    }

    /// The longest recorded prefix of `path` (itself included), if any.
    fn key_of<'p>(&self, path: &'p str) -> Option<&'p str> {
        let mut path = path;
        loop {
            if self.0.contains_key(path) {
                return Some(path);
            }
            match path.rfind(['.', '[']) {
                Some(i) if i > 0 => path = &path[..i],
                _ => return None,
            }
        }
    }
}

/// The component path of field `i`: `label[i]` or `text[i]`.
fn base(i: usize, def: &FieldDef) -> String {
    if def.kind == Kind::TextDisplay {
        format!("text[{i}]")
    } else {
        format!("label[{i}]")
    }
}

fn record_spans(name: &Ident, attr: &ModalAttr, defs: &[FieldDef]) -> Spans {
    let mut spans = Spans::default();
    spans.insert("title", attr.title.span());
    let count_span = defs.get(5).map_or_else(|| name.span(), |d| d.ident.span());
    spans.insert("components", count_span);
    for (i, def) in defs.iter().enumerate() {
        let base = base(i, def);
        spans.insert(base.clone(), def.attr);
        spans.insert(format!("{base}.label"), def.text.span());
        if let Some(d) = &def.description {
            spans.insert(format!("{base}.description"), d.span());
        }
        let kp = format!("{base}.{}", def.kind.rules_path());
        spans.insert(kp.clone(), def.attr);
        let (min, max) = if def.kind == Kind::TextInput {
            ("min_length", "max_length")
        } else {
            ("min_values", "max_values")
        };
        if let Some((_, lit)) = &def.min {
            spans.insert(format!("{kp}.{min}"), lit.span());
        }
        if let Some((_, lit)) = &def.max {
            spans.insert(format!("{kp}.{max}"), lit.span());
        }
        if let Some(p) = &def.placeholder {
            spans.insert(format!("{kp}.placeholder"), p.span());
        }
        if let Some(sp) = def.file_types_span {
            spans.insert(format!("{kp}.file_types"), sp);
        }
        for (k, lit) in def.file_types.iter().enumerate() {
            spans.insert(format!("{kp}.file_types[{k}]"), lit.span());
        }
        spans.insert(format!("{kp}.options"), def.ty.span());
        spans.insert(format!("{kp}.custom_id"), def.ident.span());
    }
    spans
}

/// The `validate_modal` violations of the lowered form.
fn validate(title: &str, defs: &[FieldDef], limits: &Limits) -> Vec<Violation> {
    let texts: Vec<String> = defs.iter().map(|d| d.text.value()).collect();
    let descriptions: Vec<Option<String>> = defs
        .iter()
        .map(|d| d.description.as_ref().map(LitStr::value))
        .collect();
    let placeholders: Vec<Option<String>> = defs
        .iter()
        .map(|d| d.placeholder.as_ref().map(LitStr::value))
        .collect();
    let file_types: Vec<Vec<String>> = defs
        .iter()
        .map(|d| d.file_types.iter().map(LitStr::value).collect())
        .collect();
    let file_type_refs: Vec<Vec<&str>> = file_types
        .iter()
        .map(|v| v.iter().map(String::as_str).collect())
        .collect();
    let components: Vec<rm::ModalComponent<'_>> = defs
        .iter()
        .enumerate()
        .map(|(i, def)| {
            if def.kind == Kind::TextDisplay {
                return rm::ModalComponent::Text(&texts[i]);
            }
            let child = lower_child(def, placeholders[i].as_deref(), &file_type_refs[i]);
            let label = rm::Label::new(&texts[i], child);
            rm::ModalComponent::Label(match &descriptions[i] {
                Some(d) => label.description(d),
                None => label,
            })
        })
        .collect();
    validate_modal(&rm::Modal::new(title, &components), limits)
        .err()
        .unwrap_or_default()
}

/// The `noctis_rules::modal` descriptor of a field's input.
fn lower_child<'a>(
    def: &FieldDef,
    placeholder: Option<&'a str>,
    file_types: &'a [&'a str],
) -> rm::LabelChild<'a> {
    let min = def.min.as_ref().map(|(n, _)| *n);
    let max = def.max.as_ref().map(|(n, _)| *n);
    let required = min != Some(0);
    match def.kind {
        Kind::TextInput => {
            let style = if def.paragraph {
                rm::TextInputStyle::Paragraph
            } else {
                rm::TextInputStyle::Short
            };
            let mut t = rm::TextInput::new(style);
            if let Some(n) = min {
                t = t.min_length(n);
            }
            if let Some(n) = max {
                t = t.max_length(n);
            }
            if let Some(p) = placeholder {
                t = t.placeholder(p);
            }
            rm::LabelChild::TextInput(t)
        }
        Kind::StringSelect
        | Kind::UserSelect
        | Kind::RoleSelect
        | Kind::MentionableSelect
        | Kind::ChannelSelect => {
            let kind = match def.kind {
                Kind::StringSelect => SelectKind::String(&SELECT_SENTINEL),
                Kind::UserSelect => SelectKind::User,
                Kind::RoleSelect => SelectKind::Role,
                Kind::MentionableSelect => SelectKind::Mentionable,
                _ => SelectKind::Channel,
            };
            let mut s = rc::Select::new(kind);
            if let Some(p) = placeholder {
                s = s.placeholder(p);
            }
            if let Some(n) = min {
                s = s.min_values(n);
            }
            // A `Vec` field without a declared max gets the largest legal one.
            match max {
                Some(n) => s = s.max_values(n),
                None if def.is_vec() => s = s.max_values(SELECT_SENTINEL.len()),
                None => {}
            }
            rm::LabelChild::Select(rm::ModalSelect::new(s).required(required))
        }
        Kind::FileUpload => {
            let mut f = rm::FileUpload::new()
                .file_types(file_types)
                .required(required);
            if let Some(n) = min {
                f = f.min_values(n);
            }
            match max {
                Some(n) => f = f.max_values(n),
                None if def.is_vec() => f = f.max_values(Limits::default().file_upload_values),
                None => {}
            }
            rm::LabelChild::FileUpload(f)
        }
        Kind::RadioGroup => rm::LabelChild::RadioGroup(rm::RadioGroup::new(&RADIO_SENTINEL)),
        Kind::CheckboxGroup => {
            let mut g = rm::CheckboxGroup::new(&RADIO_SENTINEL).required(required);
            if let Some(n) = min {
                g = g.min_values(n);
            }
            if let Some(n) = max {
                g = g.max_values(n);
            }
            rm::LabelChild::CheckboxGroup(g)
        }
        Kind::Checkbox | Kind::TextDisplay => rm::LabelChild::Checkbox(rm::Checkbox::new()),
    }
}

/// Every expansion check, in report order: `validate_modal`, CID-01 per
/// input key, then CV2-04 on the Text Displays.
fn check(name: &Ident, attr: &ModalAttr, defs: &[FieldDef]) -> Vec<syn::Error> {
    let limits = Limits::default();
    let spans = record_spans(name, attr, defs);
    // A key title is validated by core after it resolves the key; a stand-in
    // keeps the other checks.
    let title = attr
        .title
        .literal()
        .map_or_else(|| "x".to_owned(), LitStr::value);
    let mut violations = validate(&title, defs, &limits);
    for (i, def) in defs.iter().enumerate() {
        if def.kind == Kind::TextDisplay {
            continue;
        }
        let kp = format!("{}.{}", base(i, def), def.kind.rules_path());
        violations.extend(
            validate_input_custom_id(&def.key, &limits)
                .into_iter()
                .map(|mut v| {
                    v.path = format!("{kp}.custom_id");
                    v
                }),
        );
    }
    let mut errors: Vec<syn::Error> = violations
        .into_iter()
        .map(|v| syn::Error::new(spans.of(&v.path), v.to_string()))
        .collect();
    let mut total = 0;
    let mut crossing = None;
    for def in defs.iter().filter(|d| d.kind == Kind::TextDisplay) {
        total += discord_len(&def.text.value());
        if total > limits.text_total_len && crossing.is_none() {
            crossing = Some(def.text.span());
        }
    }
    if let Some(span) = crossing {
        let v = Violation::new(
            RuleId::CV2_04,
            "components",
            Bound::AtMost(limits.text_total_len),
            Actual::Count(total),
        );
        errors.push(syn::Error::new(span, v.to_string()));
    }
    errors
}

fn opt_int(v: Option<&(usize, LitInt)>) -> TokenStream {
    option(v.map(|(n, lit)| {
        let mut l = Literal::usize_unsuffixed(*n);
        l.set_span(lit.span());
        l
    }))
}

fn spec(def: &FieldDef) -> TokenStream {
    let ui = ui();
    let min = opt_int(def.min.as_ref());
    let max = opt_int(def.max.as_ref());
    let placeholder = option(def.placeholder.as_ref());
    quote!(#ui::__macro::Spec { min: #min, max: #max, placeholder: #placeholder })
}

fn ui() -> TokenStream {
    quote!(::noctis::__private::ui)
}

/// The field trait of a kind, the one whose `prefill` and `decode` the
/// expansion calls (`None` for a Text Display).
fn field_trait(kind: Kind) -> Option<TokenStream> {
    Some(match kind {
        Kind::TextDisplay => return None,
        Kind::TextInput => quote!(TextInputField),
        Kind::StringSelect => quote!(StringSelectField),
        Kind::UserSelect => quote!(UserSelectField),
        Kind::RoleSelect => quote!(RoleSelectField),
        Kind::MentionableSelect => quote!(MentionableSelectField),
        Kind::ChannelSelect => quote!(ChannelSelectField),
        Kind::FileUpload => quote!(FileUploadField),
        Kind::RadioGroup => quote!(RadioGroupField),
        Kind::CheckboxGroup => quote!(CheckboxGroupField),
        Kind::Checkbox => quote!(CheckboxField),
    })
}

/// Whether the field's value pre-fills its input (and it has an
/// `X::<FIELD>` handle): every input but a File Upload and a Text Display.
fn prefills(kind: Kind) -> bool {
    !matches!(kind, Kind::FileUpload | Kind::TextDisplay)
}

/// The top-level component expression of a field, pre-filled from the
/// render's prefill `p` and with the render's overrides `o` applied.
fn component(def: &FieldDef, p: &Ident, o: &Ident) -> TokenStream {
    let ui = ui();
    let ty = &def.ty;
    let sp = ty.span();
    let ident = &def.ident;
    let key = LitStr::new(&def.key, def.ident.span());
    let text = &def.text;
    let spec = spec(def);
    let prefill = quote!(::core::option::Option::as_ref(&#p.#ident));
    let input = match def.kind {
        Kind::TextDisplay => {
            return quote_spanned!(sp=> #ui::__macro::text_display::<#ty>(#text));
        }
        Kind::TextInput => {
            let style = if def.paragraph {
                quote!(#ui::TextInputStyle::Paragraph)
            } else {
                quote!(#ui::TextInputStyle::Short)
            };
            quote_spanned!(sp=> #ui::__macro::text_input::<#ty>(#key, #style, #spec, #prefill))
        }
        Kind::StringSelect => {
            quote_spanned!(sp=> #ui::__macro::string_select::<#ty>(#key, #spec, #prefill))
        }
        Kind::UserSelect => {
            quote_spanned!(sp=> #ui::__macro::user_select::<#ty>(#key, #spec, #prefill))
        }
        Kind::RoleSelect => {
            quote_spanned!(sp=> #ui::__macro::role_select::<#ty>(#key, #spec, #prefill))
        }
        Kind::MentionableSelect => {
            quote_spanned!(sp=> #ui::__macro::mentionable_select::<#ty>(#key, #spec, #prefill))
        }
        Kind::ChannelSelect => {
            let types = &def.channel_types;
            quote_spanned!(sp=> #ui::__macro::channel_select::<#ty>(
                #key, #spec, &[#(#ui::__macro::ChannelType::#types),*], #prefill
            ))
        }
        Kind::FileUpload => {
            let types = &def.file_types;
            quote_spanned!(sp=> #ui::__macro::file_upload::<#ty>(#key, #spec, &[#(#types),*]))
        }
        Kind::RadioGroup => {
            quote_spanned!(sp=> #ui::__macro::radio_group::<#ty>(#key, #prefill))
        }
        Kind::CheckboxGroup => {
            quote_spanned!(sp=> #ui::__macro::checkbox_group::<#ty>(#key, #spec, #prefill))
        }
        Kind::Checkbox => quote_spanned!(sp=> #ui::__macro::checkbox::<#ty>(#key, #prefill)),
    };
    let label = quote!(#ui::ModalLabel::new(#text, #input));
    let label = match &def.description {
        Some(d) => quote!(#ui::ModalLabel::description(#label, #d)),
        None => label,
    };
    quote!(#ui::__macro::input_label(#label, &#o.#ident))
}

/// The decode expression of a field, reading the submission `s`.
fn decode(def: &FieldDef, s: &Ident) -> TokenStream {
    let ui = ui();
    let ty = &def.ty;
    let sp = ty.span();
    let key = LitStr::new(&def.key, def.ident.span());
    let spec = spec(def);
    let Some(field_trait) = field_trait(def.kind) else {
        return quote_spanned!(sp=> <#ty as #ui::__macro::TextDisplayField>::decode());
    };
    if def.kind == Kind::Checkbox {
        return quote_spanned!(sp=> <#ty as #ui::__macro::CheckboxField>::decode(#s, #key)?);
    }
    quote_spanned!(sp=> <#ty as #ui::__macro::#field_trait>::decode(#s, #key, &#spec)?)
}

/// Expands the derive; on an error, the error and the placeholder impls
/// (`kind::fallback`).
pub(crate) fn expand_or_fallback(input: &DeriveInput) -> TokenStream {
    let result = expand(input);
    crate::kind::or_fallback(input, crate::kind::Family::Modal, result, || {
        let ident = &input.ident;
        let route = crate::kind::fallback_route(ident);
        let ui = quote!(::noctis::__private::ui);
        quote! {
            impl ::noctis::__private::core::__Modal for #ident {
                const ROUTE: &'static ::core::primitive::str = "";
                const RESPONSE: ::noctis::__private::core::__Response =
                    ::noctis::__private::core::__Response::Update;
            }

            impl ::noctis::__private::core::__Guards<::noctis::__private::core::ModalSubmitCtx>
                for #ident
            {
            }

            #route

            impl #ui::ModalForm for #ident {
                fn from_submission(
                    _: &#ui::ModalSubmission,
                ) -> ::core::result::Result<Self, #ui::SubmitError> {
                    ::core::result::Result::Err(#ui::SubmitError::Malformed {
                        reason: ::std::string::String::new(),
                    })
                }
            }

            impl #ui::__macro::ModalFromPayload for #ident {
                fn from_payload(
                    _: <Self as ::noctis::__private::codec::Route>::Payload,
                    _: &#ui::ModalSubmission,
                ) -> ::core::result::Result<Self, #ui::SubmitError> {
                    ::core::result::Result::Err(#ui::SubmitError::Malformed {
                        reason: ::std::string::String::new(),
                    })
                }
            }
        }
    })
}

/// The `__Modal` impl: the Route id and the declared `response` (AD-11).
fn modal_impl(name: &Ident, args: &RouteArgs) -> TokenStream {
    let core = quote!(::noctis::__private::core);
    let id = &args.id;
    let response = args.response.variant();
    quote! {
        #[automatically_derived]
        impl #core::__Modal for #name {
            const ROUTE: &'static ::core::primitive::str = #id;
            const RESPONSE: #core::__Response = #core::__Response::#response;
        }
    }
}

/// The Payload fields: names and types, in order.
struct PayloadIdents<'a> {
    idents: &'a [&'a Ident],
    types: &'a [&'a Type],
}

/// The hygienic locals of the generated render code.
struct Idents<'a> {
    modal: &'a Ident,
    prefill: &'a Ident,
    overrides: &'a Ident,
}

/// The generated `blank`: `Self` when every input has an empty value,
/// else a `ModalBuilder` with empty inputs.
fn blank_fn(
    name: &Ident,
    defs: &[FieldDef],
    payload_ident: &Ident,
    payload: &PayloadIdents<'_>,
) -> TokenStream {
    let ui = ui();
    let p_idents = payload.idents;
    let p_types = payload.types;
    let blankable = defs.iter().all(|d| is_blankable(&d.ty));
    let blank_doc_tail = if blankable {
        "Every input is empty (`None`, `vec![]`, `String::new()`, `false`), so `X { field: value, ..X::blank(..) }.build()` pre-fills only what it names."
    } else {
        "An input has no empty value (a required `Choices` enum or `Resolved*` field), so this is the builder with empty inputs; set one with `.value(X::FIELD, v)`."
    };
    let blank_doc = format!(
        " The modal with empty inputs, for these Payload fields (`{name}`'s fields without an input attribute, in order). {blank_doc_tail}"
    );
    if blankable {
        let all_idents = defs.iter().map(|d| &d.ident);
        quote! {
            #[doc = #blank_doc]
            #[must_use]
            pub fn blank(#(#p_idents: #p_types),*) -> Self {
                Self {
                    #(#p_idents,)*
                    #(#all_idents: ::core::default::Default::default(),)*
                }
            }
        }
    } else {
        quote! {
            #[doc = #blank_doc]
            pub fn blank(#(#p_idents: #p_types),*) -> #ui::ModalBuilder<Self> {
                #ui::ModalBuilder::__new(
                    #payload_ident { #(#p_idents),* },
                    ::core::default::Default::default(),
                )
            }
        }
    }
}

/// The `X::<FIELD>` handles of the pre-filled inputs.
fn handle_consts(name: &Ident, filled: &[&FieldDef], prefill_ident: &Ident) -> TokenStream {
    let ui = ui();
    let handles = filled.iter().map(|d| {
        let ident = &d.ident;
        let ty = &d.ty;
        let fvis = &d.vis;
        let tr = field_trait(d.kind);
        let handle = Ident::new(&screaming(&d.key), d.ident.span());
        let doc = format!(
            " The typed handle of the `{}` input, for `ModalBuilder::value`: `.value({name}::{handle}, v)` replaces its pre-fill.",
            d.key
        );
        quote_spanned! {ty.span()=>
            #[doc = #doc]
            // A form that never pre-fills through `.value(..)` leaves it unused.
            #[allow(dead_code)]
            #fvis const #handle: #ui::InputField<Self, #ty> = #ui::InputField::__new(
                |__noctis_p: &mut #prefill_ident, __noctis_v: #ty| {
                    __noctis_p.#ident = ::core::option::Option::Some(
                        <#ty as #ui::__macro::#tr>::prefill(__noctis_v),
                    );
                },
            );
        }
    });
    quote!(#(#handles)*)
}

/// The generated `build`: every input field's value pre-fills its input.
fn build_fn(
    defs: &[FieldDef],
    payload_ident: &Ident,
    p_idents: &[&Ident],
    prefill_ident: &Ident,
    pf: &Ident,
) -> TokenStream {
    let ui = ui();
    let field_locals: Vec<Ident> = (0..defs.len())
        .map(|i| Ident::new(&format!("__noctis_field_{i}"), Span::mixed_site()))
        .collect();
    let destructure = defs.iter().zip(&field_locals).map(|(d, local)| {
        let ident = &d.ident;
        if prefills(d.kind) {
            quote!(#ident: #local)
        } else {
            quote!(#ident: _)
        }
    });
    let fills = defs.iter().zip(&field_locals).filter(|(d, _)| prefills(d.kind)).map(|(d, local)| {
        let ident = &d.ident;
        let ty = &d.ty;
        let tr = field_trait(d.kind);
        quote_spanned!(ty.span()=> #pf.#ident = ::core::option::Option::Some(<#ty as #ui::__macro::#tr>::prefill(#local));)
    });

    quote! {
            /// The modal of this value: its declared title, its Payload
            /// fields in the `custom_id` and every input pre-filled from its
            /// field (`None`, `""`, `vec![]` and `false` send nothing). The
            /// builder takes `.title(..)`, `.value(X::FIELD, v)` and
            /// `.overrides(..)` for this render and converts into a `Modal`.
            pub fn build(self) -> #ui::ModalBuilder<Self> {
                let Self { #(#p_idents,)* #(#destructure,)* } = self;
                #[allow(unused_mut)]
                let mut #pf = <#prefill_ident as ::core::default::Default>::default();
                #(#fills)*
                #ui::ModalBuilder::__new(#payload_ident { #(#p_idents),* }, #pf)
            }
    }
}

/// The per-render surface: the hidden prefill struct, `<X>Overrides`,
/// `ModalRender`, `build`, `blank`, the `X::<FIELD>` handles and
/// `From<X> for Modal`.
fn render_impls(
    name: &Ident,
    vis: &syn::Visibility,
    defs: &[FieldDef],
    payload_ident: &Ident,
    payload: &PayloadIdents<'_>,
    locals: &Idents<'_>,
) -> TokenStream {
    let ui = ui();
    let &Idents {
        modal: m,
        prefill: pf,
        overrides: ov,
    } = locals;
    let prefill_ident = format_ident!("__NoctisModalPrefill{}", name.unraw());
    let overrides_ident = format_ident!("{}Overrides", name.unraw());
    let filled: Vec<&FieldDef> = defs.iter().filter(|d| prefills(d.kind)).collect();
    let filled_idents: Vec<&Ident> = filled.iter().map(|d| &d.ident).collect();
    let labels: Vec<&FieldDef> = defs
        .iter()
        .filter(|d| d.kind != Kind::TextDisplay)
        .collect();
    let label_idents: Vec<&Ident> = labels.iter().map(|d| &d.ident).collect();
    let label_docs = labels.iter().map(|d| {
        format!(
            " The Label text, description and placeholder of the `{}` input.",
            d.key
        )
    });
    let label_locals: Vec<Ident> = (0..labels.len())
        .map(|i| Ident::new(&format!("__noctis_input_{i}"), Span::mixed_site()))
        .collect();
    let title_local = Ident::new("__noctis_title", Span::mixed_site());
    let into = Ident::new("__noctis_into", Span::mixed_site());
    let from = Ident::new("__noctis_from", Span::mixed_site());
    let components = defs.iter().map(|d| component(d, pf, ov));

    let build = build_fn(defs, payload_ident, payload.idents, &prefill_ident, pf);
    let blank = blank_fn(name, defs, payload_ident, payload);
    let handles = handle_consts(name, &filled, &prefill_ident);
    let overrides_doc = format!(
        " The per-render presentation overrides of `{name}` (title, and per Label input its text, description and placeholder), applied with `ModalBuilder::overrides`; they never change a value."
    );

    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code)]
        #[derive(::core::default::Default, ::core::clone::Clone, ::core::fmt::Debug)]
        #vis struct #prefill_ident {
            #(#filled_idents: ::core::option::Option<#ui::__macro::Prefill>,)*
        }

        #[doc = #overrides_doc]
        #[derive(::core::fmt::Debug, ::core::clone::Clone, ::core::default::Default)]
        #vis struct #overrides_ident {
            /// The modal's title (a literal or a locale key, ≤ 45, MOD-01).
            pub title: ::core::option::Option<#ui::Text>,
            #(
                #[doc = #label_docs]
                pub #label_idents: #ui::InputOverrides,
            )*
        }

        #[automatically_derived]
        impl #ui::__macro::ModalRender for #name {
            type Prefill = #prefill_ident;
            type Overrides = #overrides_ident;

            fn title_mut(
                #ov: &mut Self::Overrides,
            ) -> &mut ::core::option::Option<#ui::Text> {
                &mut #ov.title
            }

            fn merge(#into: &mut Self::Overrides, #from: Self::Overrides) {
                let #overrides_ident { title: #title_local, #(#label_idents: #label_locals,)* } = #from;
                if ::core::option::Option::is_some(&#title_local) {
                    #into.title = #title_local;
                }
                #(#ui::InputOverrides::__merge(&mut #into.#label_idents, #label_locals);)*
            }

            #[allow(unused_variables)]
            fn render(
                #m: #ui::Modal,
                #pf: &Self::Prefill,
                #ov: &Self::Overrides,
            ) -> #ui::Modal {
                #(let #m = #ui::Modal::push(#m, #components);)*
                #m
            }
        }

        impl #name {
            #build

            #blank

            #handles
        }

        #[automatically_derived]
        impl ::core::convert::From<#name> for #ui::Modal {
            fn from(value: #name) -> Self {
                ::core::convert::From::from(#name::build(value))
            }
        }
    }
}

/// Expands `#[derive(Modal)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let (attr, defs, payload) = parse(input)?;
    let name = &input.ident;
    combine(check(name, &attr, &defs))?;
    let all: Vec<&Ident> = match &input.data {
        Data::Struct(s) => s.fields.iter().filter_map(|f| f.ident.as_ref()).collect(),
        _ => Vec::new(),
    };
    let inputs: Vec<&Ident> = defs.iter().map(|d| &d.ident).collect();
    crate::guard::check_owner(&attr.args.guards, name, &all, &inputs)?;
    let guards = crate::guard::guards_impl(name, &attr.args.guards, crate::guard::GuardCtx::Modal);

    let ui = ui();
    let vis = &input.vis;
    let payload_ident = format_ident!("__NoctisModalPayload{}", name.unraw());
    let p_idents: Vec<&Ident> = payload.iter().map(|p| &p.ident).collect();
    let p_types: Vec<&Type> = payload.iter().map(|p| &p.ty).collect();
    let payload_input: DeriveInput = syn::parse_quote! {
        struct #payload_ident { #(#p_idents: #p_types),* }
    };
    let payload_impls = crate::payload::impls(
        &payload_input,
        match &payload_input.data {
            Data::Struct(s) => &s.fields,
            _ => unreachable!("parsed as a struct"),
        },
    );
    let route = route_impls(name, &attr.args, &quote!(#payload_ident));
    let title = attr.title.tokens();
    let m = Ident::new("__noctis_modal", Span::mixed_site());
    let s = Ident::new("__noctis_submission", Span::mixed_site());
    let p = Ident::new("__noctis_payload", Span::mixed_site());
    let pf = Ident::new("__noctis_prefill", Span::mixed_site());
    let ov = Ident::new("__noctis_overrides", Span::mixed_site());
    let idents = defs.iter().map(|d| &d.ident);
    let decodes = defs.iter().map(|d| decode(d, &s));
    let modal_impl = modal_impl(name, &attr.args);
    let scaffolding = crate::kind::scaffolding(
        name,
        crate::kind::Family::Modal,
        !attr.args.auto_defer,
        &attr.args.flags,
    );
    let render = render_impls(
        name,
        vis,
        &defs,
        &payload_ident,
        &PayloadIdents {
            idents: &p_idents,
            types: &p_types,
        },
        &Idents {
            modal: &m,
            prefill: &pf,
            overrides: &ov,
        },
    );
    Ok(quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #payload_ident { #(#p_idents: #p_types),* }

        #payload_impls

        #route

        #[automatically_derived]
        impl #ui::Handled for #name {}

        #[automatically_derived]
        impl #ui::ComponentRoute for #name {
            type Defaults = #ui::__macro::ModalDefaults;
            const DEFAULTS: Self::Defaults = #ui::__macro::ModalDefaults { title: #title };
        }

        #modal_impl

        #guards

        #render

        #[automatically_derived]
        impl #ui::ModalForm for #name {
            /// Decodes the Payload fields from the `custom_id`, then the inputs.
            /// A `signed` modal decoded directly returns
            /// `SubmitError::Malformed`: its tag is verified only through
            /// component dispatch, which decodes the `custom_id` with the key
            /// ring and hands the verified Payload to `from_payload`. A
            /// `by_ref` or spilled Payload is likewise resolved only there.
            fn from_submission(
                #s: &#ui::ModalSubmission,
            ) -> ::core::result::Result<Self, #ui::SubmitError> {
                let #p = #ui::__macro::decode_modal_payload::<Self>(#s)?;
                <Self as #ui::__macro::ModalFromPayload>::from_payload(#p, #s)
            }
        }

        #[automatically_derived]
        impl #ui::__macro::ModalFromPayload for #name {
            fn from_payload(
                #p: <Self as ::noctis::__private::codec::Route>::Payload,
                #s: &#ui::ModalSubmission,
            ) -> ::core::result::Result<Self, #ui::SubmitError> {
                ::core::result::Result::Ok(Self {
                    #(#p_idents: #p.#p_idents,)*
                    #(#idents: #decodes,)*
                })
            }
        }

        #scaffolding
    })
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

    fn defs(input: TokenStream) -> (ModalAttr, Vec<FieldDef>) {
        let input: DeriveInput = syn::parse2(input).unwrap_or_else(|e| panic!("{e}"));
        let (attr, defs, _) = parse(&input).unwrap_or_else(|e| panic!("{e}"));
        (attr, defs)
    }

    /// Route Guard Matrix 10 and 34: a modal's `owner` must be a Payload
    /// field; a guarded modal emits its `__Guards<ModalSubmitCtx>`.
    #[test]
    fn modal_owner_guard() {
        let out = run(quote! {
            #[modal("f", owner = bio, defaults(title = "T"))]
            struct F {
                user: Id<UserMarker>,
                #[text_input("Bio")]
                bio: String,
            }
        });
        assert!(
            out.contains("[CID-03] `owner` must name a Payload field; `bio` is a modal input"),
            "{out}"
        );
        let out = run(quote! {
            #[modal("f", owner = usr, defaults(title = "T"))]
            struct F {
                user: Id<UserMarker>,
                #[text_input("Bio")]
                bio: String,
            }
        });
        assert!(
            out.contains("names no field of `F`; its fields are: user, bio"),
            "{out}"
        );
        let out = run(quote! {
            #[modal("f", owner = user, defaults(title = "T"))]
            struct F {
                user: Id<UserMarker>,
                #[text_input("Bio")]
                bio: String,
            }
        });
        assert!(
            out.contains("__Guards < :: noctis :: __private :: core :: ModalSubmitCtx > for F"),
            "{out}"
        );
        assert!(out.contains("const GUARDED"), "{out}");
        assert!(out.contains("__owner_id (& self . user)"), "{out}");
    }

    /// Story 7.8 rows 1-4 and 7: `response` and `auto_defer` on `#[modal]`
    /// emit `__Modal::RESPONSE` and `__Kind::Defer`; the width line does
    /// not change.
    #[test]
    fn modal_response_args() {
        let expand = |args: TokenStream| {
            run(quote! {
                #[modal("f", #args)]
                struct F {
                    user: u64,
                    #[text_input("Bio")]
                    bio: String,
                }
            })
            .replace(' ', "")
        };
        let response = |v: &str| {
            format!(
                "constRESPONSE:::noctis::__private::core::__Response=::noctis::__private::core::__Response::{v};"
            )
        };
        let auto = "typeDefer=::noctis::__private::core::AutoDefer;";
        let manual = "typeDefer=::noctis::__private::core::Manual;";
        let default = expand(quote!(defaults(title = "T")));
        assert!(default.contains(&response("Update")), "{default}");
        assert!(default.contains(auto), "{default}");
        for (arg, v) in [
            (quote!(response = update), "Update"),
            (quote!(response = ephemeral), "Ephemeral"),
            (quote!(response = public), "Public"),
        ] {
            let out = expand(quote!(#arg, defaults(title = "T")));
            assert!(out.contains(&response(v)), "{out}");
            assert!(out.contains(auto), "{out}");
        }
        let out = expand(quote!(auto_defer = false, defaults(title = "T")));
        assert!(out.contains(manual), "{out}");
        assert!(out.contains(&response("Update")), "{out}");
        for args in [
            quote!(auto_defer = true, response = public, defaults(title = "T")),
            quote!(defaults(title = "T"), response = public, auto_defer = true),
        ] {
            let out = expand(args);
            assert!(out.contains(&response("Public")), "{out}");
            assert!(out.contains(auto), "{out}");
            // Only `RESPONSE` changes: the Route, its width line and the
            // rest of the expansion stay the same.
            assert_eq!(
                out.replace(&response("Public"), &response("Update")),
                default
            );
        }
        for (arg, key) in [
            (quote!(response = update, response = public), "response"),
            (quote!(auto_defer = true, auto_defer = false), "auto_defer"),
        ] {
            let out = run(quote! {
                #[modal("f", #arg, defaults(title = "T"))]
                struct F {
                    #[text_input("Bio")]
                    bio: String,
                }
            });
            assert!(
                out.contains(&format!("#[modal] argument `{key}` is given twice")),
                "{out}"
            );
        }
        // The fallback of a broken derive declares `Update`.
        let broken: DeriveInput = syn::parse2(quote! {
            #[modal("f", response = edit, defaults(title = "T"))]
            struct F {
                #[text_input("Bio")]
                bio: String,
            }
        })
        .unwrap();
        let out = expand_or_fallback(&broken).to_string();
        assert!(
            out.contains("`response` takes `update`, `ephemeral` or `public`"),
            "{out}"
        );
        assert!(out.replace(' ', "").contains(&response("Update")), "{out}");
    }

    /// The derive implements `ModalFromPayload` (the inputs, from a decoded
    /// Payload) and its `from_submission` delegates to it; the fallback
    /// expansion of a broken derive implements it too.
    #[test]
    fn modal_from_payload_seam() {
        let body = quote! {
            #[modal("f", signed, defaults(title = "T"))]
            struct F {
                item: u64,
                #[text_input("Bio")]
                bio: String,
            }
        };
        let out = run(body.clone()).replace(' ', "");
        assert!(
            out.contains("impl::noctis::__private::ui::__macro::ModalFromPayloadforF"),
            "{out}"
        );
        assert!(
            out.contains(
                "<Selfas::noctis::__private::ui::__macro::ModalFromPayload>::from_payload("
            ),
            "{out}"
        );
        let broken: DeriveInput = syn::parse2(quote! {
            #[modal("f", defaults(title = ""))]
            struct F { item: u64 }
        })
        .unwrap();
        let fallback = expand_or_fallback(&broken).to_string().replace(' ', "");
        assert!(fallback.contains("ModalFromPayloadforF"), "{fallback}");
        assert!(fallback.contains("[MOD-01]"), "{fallback}");
    }

    #[test]
    fn attribute_parser_reads_every_argument() {
        let (attr, defs) = defs(quote! {
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[text_input("Bio", description = "d", style = paragraph, min_length = 2, max_length = 30, placeholder = "p")]
                r#type: Option<String>,
                #[channel_select("C", channel_types = [GuildText, ChannelType::GuildVoice], min_values = 0, max_values = 2)]
                chans: Vec<ResolvedChannel>,
                #[file_upload("F", file_types = ["image", ".pdf"])]
                files: Vec<ResolvedAttachment>,
                #[text_display("Hello")]
                _intro: (),
            }
        });
        assert_eq!(attr.args.id, "f");
        assert_eq!(
            attr.title.literal().map(LitStr::value).as_deref(),
            Some("T")
        );
        let bio = &defs[0];
        assert_eq!((bio.key.as_str(), bio.kind), ("type", Kind::TextInput));
        assert!(bio.paragraph);
        assert_eq!(bio.min.as_ref().map(|m| m.0), Some(2));
        assert_eq!(bio.max.as_ref().map(|m| m.0), Some(30));
        assert_eq!(
            bio.placeholder.as_ref().map(LitStr::value).as_deref(),
            Some("p")
        );
        assert_eq!(
            bio.description.as_ref().map(LitStr::value).as_deref(),
            Some("d")
        );
        let chans = &defs[1];
        assert!(chans.is_vec());
        assert_eq!(chans.channel_types.len(), 2);
        assert_eq!(chans.channel_types[1], "GuildVoice");
        assert_eq!(defs[2].file_types.len(), 2);
        assert_eq!(defs[3].kind, Kind::TextDisplay);
        assert_eq!(defs[3].text.value(), "Hello");
    }

    #[test]
    fn spans_fall_back_to_the_longest_prefix() {
        let mut spans = Spans::default();
        let a = Span::call_site();
        spans.insert("label[0]", a);
        spans.insert("label[0].text_input.max_length", a);
        assert!(spans.0.contains_key("label[0]"));
        assert_eq!(
            spans.key_of("label[0].text_input.min_length"),
            Some("label[0]")
        );
        assert_eq!(
            spans.key_of("label[0].text_input.max_length"),
            Some("label[0].text_input.max_length")
        );
        assert_eq!(spans.key_of("nothing"), None);
        let _ = spans.of("nothing");
        let (attr, defs) = defs(quote! {
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[file_upload("F", file_types = ["image", "exe"])]
                files: Vec<ResolvedAttachment>,
            }
        });
        let name = Ident::new("F", Span::call_site());
        let recorded = record_spans(&name, &attr, &defs);
        for path in [
            "title",
            "components",
            "label[0]",
            "label[0].label",
            "label[0].file_upload",
            "label[0].file_upload.file_types",
            "label[0].file_upload.file_types[1]",
            "label[0].file_upload.custom_id",
        ] {
            assert!(recorded.0.contains_key(path), "{path}");
        }
    }

    #[test]
    fn mod_01_title() {
        let ok = "a".repeat(45);
        let body = quote!(
            struct F {
                #[checkbox("x")]
                ok: bool,
            }
        );
        assert!(run(quote!(#[modal("f", defaults(title = #ok))] #body)).contains("impl"));
        let over = "a".repeat(46);
        assert!(
            run(quote!(#[modal("f", defaults(title = #over))] #body))
                .contains("[MOD-01] title: limit between 1 and 45, actual 46")
        );
        assert!(run(quote!(#[modal("f", defaults(title = ""))] #body)).contains("[MOD-01] title"));
        let emoji = "😀".repeat(23);
        assert!(run(quote!(#[modal("f", defaults(title = #emoji))] #body)).contains("actual 46"));
        // Story 11.1 Matrix 68: a path title is a locale key.
        let out = run(quote!(#[modal("f", defaults(title = TITLE))] #body));
        assert!(out.contains("DefaultText :: Key (TITLE)"), "{out}");
        assert!(run(quote!(#[modal("f")] #body)).contains("`#[modal]` needs `defaults(title ="));
        // Story 6.11 Matrix 53: the withdrawn form.
        for withdrawn in [
            quote!(#[modal(route = R, title = "T")]),
            quote!(#[modal(title = "T")]),
        ] {
            let out = run(quote!(#withdrawn #body));
            assert!(out.contains("is withdrawn"), "{out}");
            assert!(out.contains("= help: #[derive(Modal)]"), "{out}");
        }
    }

    #[test]
    fn mod_02_struct_shape_and_attributes() {
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {}
        ));
        assert!(
            out.contains("[MOD-02] components: limit between 1 and 5, actual 0"),
            "{out}"
        );
        assert!(
            run(quote!(
                #[modal("f", defaults(title = "T"))]
                struct F(u8);
            ))
            .contains("[MOD-02]")
        );
        assert!(
            run(quote!(
                #[modal("f", defaults(title = "T"))]
                enum F {
                    A,
                }
            ))
            .contains("[MOD-02]")
        );
        assert!(
            run(quote!(
                #[modal("f", defaults(title = "T"))]
                struct F<T> {
                    #[checkbox("x")]
                    a: T,
                }
            ))
            .contains("[MOD-02] a modal form cannot be generic")
        );
        // A field without an input attribute is Payload: no input counts.
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                a: bool,
            }
        ));
        assert!(
            out.contains("[MOD-02] components: limit between 1 and 5, actual 0"),
            "{out}"
        );
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[checkbox("x")]
                #[checkbox("y")]
                a: bool,
            }
        ));
        assert!(
            out.contains("[MOD-03] field `a` has two component attributes"),
            "{out}"
        );
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[radio_group("x", min_values = 1)]
                a: C,
            }
        ));
        assert!(
            out.contains("unknown #[radio_group] argument `min_values`"),
            "{out}"
        );
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[string_select("x", min_values = 1)]
                a: C,
            }
        ));
        assert!(
            out.contains("`min_values` is only for `Vec` fields"),
            "{out}"
        );
        assert!(!out.contains("[MOD"), "{out}");
    }

    #[test]
    fn mod_04_label_and_cid_01_key() {
        let over = "a".repeat(46);
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[checkbox(#over)]
                a: bool,
            }
        ));
        assert!(
            out.contains("[MOD-04] label[0].label: limit between 1 and 45, actual 46"),
            "{out}"
        );
        let d = "a".repeat(101);
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[checkbox("x", description = #d)]
                a: bool,
            }
        ));
        assert!(out.contains("[MOD-04] label[0].description"), "{out}");
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[checkbox(key = "k")]
                a: bool,
            }
        ));
        assert!(
            out.contains("[MOD-04] the Label text must be a string literal"),
            "{out}"
        );
        let long = Ident::new(&"k".repeat(101), Span::call_site());
        let out = run(
            quote!(#[modal("f", defaults(title = "T"))] struct F { #[checkbox("x")] #long: bool }),
        );
        assert!(
            out.contains(
                "[CID-01] label[0].checkbox.custom_id: limit between 1 and 100, actual 101"
            ),
            "{out}"
        );
    }

    #[test]
    fn mod_05_to_07_and_cv2_checks() {
        let f = |attr: TokenStream, ty: TokenStream| {
            run(quote!(#[modal("f", defaults(title = "T"))] struct F { #attr a: #ty }))
        };
        assert!(
            f(
                quote!(#[text_input("x", min_length = 10, max_length = 5)]),
                quote!(String)
            )
            .contains("[MOD-05] label[0].text_input: limit min_length ≤ max_length")
        );
        assert!(
            f(
                quote!(#[text_input("x", max_length = 4001)]),
                quote!(String)
            )
            .contains("[MOD-05]")
        );
        assert!(f(quote!(#[text_input("x", max_length = 0)]), quote!(String)).contains("[MOD-05]"));
        let p = "a".repeat(101);
        assert!(
            f(quote!(#[text_input("x", placeholder = #p)]), quote!(String))
                .contains("[MOD-05] label[0].text_input.placeholder")
        );
        assert!(
            f(quote!(#[file_upload("x", max_values = 11)]), quote!(Vec<A>)).contains("[MOD-06]")
        );
        assert!(
            f(
                quote!(#[file_upload("x", file_types = ["exe"])]),
                quote!(Vec<A>)
            )
            .contains("[MOD-06] label[0].file_upload.file_types[0]")
        );
        assert!(
            f(
                quote!(#[checkbox_group("x", min_values = 11)]),
                quote!(Vec<C>)
            )
            .contains("[MOD-07]")
        );
        let p = "a".repeat(151);
        assert!(
            f(quote!(#[string_select("x", placeholder = #p)]), quote!(C))
                .contains("[CV2-09] label[0].select.placeholder")
        );
        assert!(f(quote!(#[user_select("x", min_values = 3)]), quote!(Vec<U>)).contains("impl"));
        assert!(
            f(
                quote!(#[checkbox_group("x", min_values = 0)]),
                quote!(Vec<C>)
            )
            .contains("impl")
        );
        let t = "a".repeat(4001);
        let out = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[text_display(#t)]
                a: (),
            }
        ));
        assert!(
            out.contains("[CV2-04] components: limit at most 4000, actual 4001"),
            "{out}"
        );
    }

    /// Story 9.3 M18: an input named `title` or one whose handle shadows a
    /// derived associated const is a compile error naming the field.
    #[test]
    fn m18_reserved_input_names() {
        let title = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[text_input("Title")]
                title: String,
            }
        ));
        assert!(
            title.contains("input field `title` cannot be named `title`"),
            "{title}"
        );
        for (field, handle) in [("id", "ID"), ("defaults", "DEFAULTS"), ("route", "ROUTE")] {
            let ident = Ident::new(field, Span::call_site());
            let out = run(quote!(
                #[modal("f", defaults(title = "T"))]
                struct F {
                    #[text_input("X")]
                    #ident: String,
                }
            ));
            assert!(out.contains(&format!("input field `{field}`")), "{out}");
            assert!(out.contains(&format!("`{handle}`")), "{out}");
        }
        // `name` stays allowed: `__Kind::NAME` is only read fully qualified.
        let name = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[text_input("Name")]
                name: String,
            }
        ));
        assert!(name.contains("const NAME"), "{name}");
        // Kinds without a handle (and Text Display without an overrides
        // field) may use those names.
        let free = run(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                #[text_display("Heading")]
                title: (),
                #[text_display("Intro")]
                id: (),
                #[file_upload("F")]
                route: Vec<A>,
            }
        ));
        assert!(!free.contains("cannot be named"), "{free}");
    }

    /// The return type of the generated `blank`.
    fn blank_return(input: TokenStream) -> String {
        let out = run(input);
        let start = out
            .find("pub fn blank")
            .unwrap_or_else(|| panic!("no blank: {out}"));
        let rest = &out[start..];
        let arrow = rest.find("->").unwrap_or_else(|| panic!("{rest}"));
        let body = rest.find('{').unwrap_or_else(|| panic!("{rest}"));
        rest[arrow + 2..body].trim().to_owned()
    }

    /// Story 9.3 M7/M8: `blank` is `Self` only when every input field has
    /// an empty value (`Option`, `Vec`, `String`, `bool`, `()`).
    #[test]
    fn blankable_classification() {
        let blankable = blank_return(quote!(
            #[modal("f", defaults(title = "T"))]
            struct F {
                user: u64,
                #[text_display("Hi")]
                intro: (),
                #[text_input("A")]
                a: Option<String>,
                #[text_input("B")]
                b: String,
                #[checkbox("C")]
                c: bool,
                #[checkbox_group("D")]
                d: Vec<Topic>,
            }
        ));
        assert_eq!(blankable, "Self");
        for required in [
            quote!(Severity),
            quote!(ResolvedRole),
            quote!(ResolvedAttachment),
        ] {
            let kind = if required.to_string() == "Severity" {
                quote!(radio_group)
            } else if required.to_string() == "ResolvedRole" {
                quote!(role_select)
            } else {
                quote!(file_upload)
            };
            let ret = blank_return(quote!(
                #[modal("f", defaults(title = "T"))]
                struct F {
                    #[text_input("A")]
                    a: Option<String>,
                    #[#kind("R")]
                    r: #required,
                }
            ));
            assert!(ret.contains("ModalBuilder < Self >"), "{required}: {ret}");
        }
        assert!(is_blankable(&syn::parse_quote!(::std::option::Option<u8>)));
        assert!(!is_blankable(&syn::parse_quote!(Option)));
        assert!(!is_blankable(&syn::parse_quote!(u8)));
    }

    /// Story 13.2 M1-M8 on `#[modal]`.
    #[test]
    fn boot_flags() {
        let out = run(quote! {
            #[modal("bio", flags = ["beta", "eco"], defaults(title = "T"))]
            struct Bio {
                #[text_input("Bio")]
                bio: String,
            }
        });
        assert!(
            out.contains("__add_flagged (& [\"beta\" , \"eco\"]"),
            "{out}"
        );
        for (flags, expected) in [
            (quote!(flags = []), crate::flags::EMPTY),
            (quote!(flags = "beta"), crate::flags::NOT_A_LIST),
            (quote!(flags), crate::flags::NOT_A_LIST),
            (
                quote!(flags = ["Beta"]),
                "invalid Boot Flag `Beta` in `flags`: `B` at index 0 is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-`",
            ),
            (
                quote!(flags = ["none"]),
                "invalid Boot Flag `none` in `flags`: `none` is reserved: write `all` or `none` alone, never in a list or as a declared flag",
            ),
            (
                quote!(flags = ["a", "a"]),
                "invalid Boot Flag `a` in `flags`: a Boot Flag is given twice: keep one",
            ),
        ] {
            let out = run(quote! {
                #[modal("bio", #flags, defaults(title = "T"))]
                struct Bio {
                    #[text_input("Bio")]
                    bio: String,
                }
            });
            assert!(
                out.contains(&expected.replace('"', "\\\"")),
                "{flags}: {out}"
            );
        }
        let long = "a".repeat(33);
        let out = run(quote! {
            #[modal("bio", flags = [#long], defaults(title = "T"))]
            struct Bio {
                #[text_input("Bio")]
                bio: String,
            }
        });
        assert!(
            out.contains("the Boot Flag has 33 characters: shorten it to at most 32"),
            "{out}"
        );
    }
}
