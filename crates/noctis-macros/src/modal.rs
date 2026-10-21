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
use crate::defaults::{Defaults, DefaultsKind};
use crate::route::{AttrKind, RouteArgs, parse_kind_attr, route_impls};

/// The help of a localized or non-literal text.
const LOCALIZED: &str = "localized modal text is [planned, Epic 11]; use a literal";

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

/// A text argument that must be a literal.
fn text_arg(expr: &Expr, rule: RuleId, what: &str) -> syn::Result<LitStr> {
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
    title: LitStr,
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
        if let Some(def) = parse_field(ident, &field.ty, attr, kind, &mut errors) {
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
    let mut violations = validate(&attr.title.value(), defs, &limits);
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

/// The top-level component expression of a field.
fn component(def: &FieldDef) -> TokenStream {
    let ui = ui();
    let ty = &def.ty;
    let sp = ty.span();
    let key = LitStr::new(&def.key, def.ident.span());
    let text = &def.text;
    let spec = spec(def);
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
            quote_spanned!(sp=> #ui::__macro::text_input::<#ty>(#key, #style, #spec))
        }
        Kind::StringSelect => quote_spanned!(sp=> #ui::__macro::string_select::<#ty>(#key, #spec)),
        Kind::UserSelect => quote_spanned!(sp=> #ui::__macro::user_select::<#ty>(#key, #spec)),
        Kind::RoleSelect => quote_spanned!(sp=> #ui::__macro::role_select::<#ty>(#key, #spec)),
        Kind::MentionableSelect => {
            quote_spanned!(sp=> #ui::__macro::mentionable_select::<#ty>(#key, #spec))
        }
        Kind::ChannelSelect => {
            let types = &def.channel_types;
            quote_spanned!(sp=> #ui::__macro::channel_select::<#ty>(
                #key, #spec, &[#(#ui::__macro::ChannelType::#types),*]
            ))
        }
        Kind::FileUpload => {
            let types = &def.file_types;
            quote_spanned!(sp=> #ui::__macro::file_upload::<#ty>(#key, #spec, &[#(#types),*]))
        }
        Kind::RadioGroup => quote_spanned!(sp=> #ui::__macro::radio_group::<#ty>(#key)),
        Kind::CheckboxGroup => {
            quote_spanned!(sp=> #ui::__macro::checkbox_group::<#ty>(#key, #spec))
        }
        Kind::Checkbox => quote_spanned!(sp=> #ui::__macro::checkbox::<#ty>(#key)),
    };
    let label = quote!(#ui::ModalLabel::new(#text, #input));
    match &def.description {
        Some(d) => quote!(#ui::ModalLabel::description(#label, #d)),
        None => label,
    }
}

/// The decode expression of a field, reading the submission `s`.
fn decode(def: &FieldDef, s: &Ident) -> TokenStream {
    let ui = ui();
    let ty = &def.ty;
    let sp = ty.span();
    let key = LitStr::new(&def.key, def.ident.span());
    let spec = spec(def);
    let field_trait = match def.kind {
        Kind::TextDisplay => {
            return quote_spanned!(sp=> <#ty as #ui::__macro::TextDisplayField>::decode());
        }
        Kind::Checkbox => {
            return quote_spanned!(sp=> <#ty as #ui::__macro::CheckboxField>::decode(#s, #key)?);
        }
        Kind::TextInput => quote!(TextInputField),
        Kind::StringSelect => quote!(StringSelectField),
        Kind::UserSelect => quote!(UserSelectField),
        Kind::RoleSelect => quote!(RoleSelectField),
        Kind::MentionableSelect => quote!(MentionableSelectField),
        Kind::ChannelSelect => quote!(ChannelSelectField),
        Kind::FileUpload => quote!(FileUploadField),
        Kind::RadioGroup => quote!(RadioGroupField),
        Kind::CheckboxGroup => quote!(CheckboxGroupField),
    };
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
        }
    })
}

/// Expands `#[derive(Modal)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let (attr, defs, payload) = parse(input)?;
    let name = &input.ident;
    combine(check(name, &attr, &defs))?;

    let ui = ui();
    let core = quote!(::noctis::__private::core);
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
    let title = &attr.title;
    let id = &attr.args.id;
    let m = Ident::new("__noctis_modal", Span::mixed_site());
    let s = Ident::new("__noctis_submission", Span::mixed_site());
    let p = Ident::new("__noctis_payload", Span::mixed_site());
    let components = defs.iter().map(component);
    let idents = defs.iter().map(|d| &d.ident);
    let decodes = defs.iter().map(|d| decode(d, &s));
    let blank_doc = format!(
        " The modal with empty inputs, for these Payload fields (`{name}`'s fields without an input attribute, in order)."
    );
    let scaffolding = crate::kind::scaffolding(name, crate::kind::Family::Modal, false);
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

        #[automatically_derived]
        impl #core::__Modal for #name {
            const ROUTE: &'static ::core::primitive::str = #id;
        }

        impl #name {
            /// The modal of this value: its declared title, its Payload
            /// fields in the `custom_id`, empty inputs (pre-fill arrives
            /// with Story 9.4).
            #[must_use]
            pub fn build(self) -> #ui::Modal {
                let Self { #(#p_idents,)* .. } = self;
                Self::__noctis_modal(#payload_ident { #(#p_idents),* })
            }

            #[doc = #blank_doc]
            #[must_use]
            pub fn blank(#(#p_idents: #p_types),*) -> #ui::Modal {
                Self::__noctis_modal(#payload_ident { #(#p_idents),* })
            }

            fn __noctis_modal(#p: #payload_ident) -> #ui::Modal {
                let #m = #ui::__macro::build_modal::<Self>(
                    #p,
                    <Self as #ui::ComponentRoute>::DEFAULTS.title,
                );
                #(let #m = #ui::Modal::push(#m, #components);)*
                #m
            }
        }

        #[automatically_derived]
        impl #ui::ModalForm for #name {
            /// Decodes the Payload fields from the `custom_id`, then the inputs.
            /// A modal declared `by_ref`, `overflow = spill` (once its Payload
            /// overflows) or `signed` decodes only through component dispatch
            /// (Epic 7); decoded directly it returns `SubmitError::Malformed`
            /// until then.
            fn from_submission(
                #s: &#ui::ModalSubmission,
            ) -> ::core::result::Result<Self, #ui::SubmitError> {
                let #p = #ui::__macro::decode_modal_payload::<Self>(#s)?;
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
        assert_eq!(attr.title.value(), "T");
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
        let out = run(quote!(#[modal("f", defaults(title = TITLE))] #body));
        assert!(
            out.contains("`defaults` values are literals: `title` takes a string literal"),
            "{out}"
        );
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
}
