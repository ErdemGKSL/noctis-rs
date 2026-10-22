//! The mandatory `defaults(..)` block of the component and modal kind
//! attributes (FR-59, AD-19; CV2-08, CV2-09, MOD-01): its keys per kind,
//! literal-only values checked against Discord's limits at expansion, and
//! the `ComponentRoute::DEFAULTS` constant it lowers to.
//!
//! Literal versus localized text is decided by syntax, never by a cargo
//! feature: a string literal is hard-coded text, checked here; a path
//! (`keys::SAVE`) for `label`, `placeholder` or `title` is a locale key,
//! lowered to `DefaultText::Key` with its span kept (a path that is not a
//! `MessageKey` is rustc's `E0308` at the path) and validated by core's
//! Responder after it resolves the key.
//!
//! Omitted keys are not sent, so Discord's `min_values = 1`,
//! `max_values = 1` and `disabled = false` apply.

use noctis_rules::command::CHANNEL_TYPES;
use noctis_rules::component::{validate_button_label, validate_select_placeholder};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, ExprLit, Ident, Lit, LitInt, LitStr, MetaNameValue, Path, Token};

use crate::common::combine;
use crate::kind::panic_lit;

/// The kind whose `defaults(..)` is parsed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DefaultsKind {
    Button,
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect,
    Modal,
}

impl DefaultsKind {
    /// The kind attribute's name.
    pub(crate) fn attr(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::StringSelect => "string_select",
            Self::UserSelect => "user_select",
            Self::RoleSelect => "role_select",
            Self::MentionableSelect => "mentionable_select",
            Self::ChannelSelect => "channel_select",
            Self::Modal => "modal",
        }
    }

    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Button => &["style", "label", "emoji", "disabled"],
            Self::StringSelect => &[
                "placeholder",
                "min_values",
                "max_values",
                "disabled",
                "options",
            ],
            Self::ChannelSelect => &[
                "placeholder",
                "min_values",
                "max_values",
                "disabled",
                "channel_types",
            ],
            Self::UserSelect | Self::RoleSelect | Self::MentionableSelect => {
                &["placeholder", "min_values", "max_values", "disabled"]
            }
            Self::Modal => &["title"],
        }
    }

    /// `` `a`, `b` or `c` ``.
    fn key_list(self) -> String {
        let keys: Vec<String> = self.keys().iter().map(|k| format!("`{k}`")).collect();
        match keys.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
            None => String::new(),
        }
    }

    /// The error of a missing block or a missing required key.
    pub(crate) fn needs(self) -> String {
        match self {
            Self::Button => {
                "`#[button]` needs `defaults(..)` with `style` and `label` or `emoji`".to_owned()
            }
            Self::Modal => "`#[modal]` needs `defaults(title = \"…\")`".to_owned(),
            select => format!(
                "`#[{}]` needs `defaults(..)` with `placeholder`",
                select.attr()
            ),
        }
    }
}

/// A Button's `emoji` literal.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EmojiLit {
    Unicode(String),
    Custom {
        animated: bool,
        name: String,
        id: u64,
    },
}

/// A `label`, `placeholder` or `title` value: a string literal or a locale
/// key path.
#[derive(Clone)]
pub(crate) enum TextValue {
    /// A string literal, checked at expansion.
    Literal(LitStr),
    /// A locale key path (`keys::SAVE`), after `Expr::Group` unwrapping.
    Key(Expr),
}

impl TextValue {
    /// The literal, when the value is one.
    pub(crate) fn literal(&self) -> Option<&LitStr> {
        match self {
            Self::Literal(lit) => Some(lit),
            Self::Key(_) => None,
        }
    }

    /// The span of the value.
    pub(crate) fn span(&self) -> proc_macro2::Span {
        match self {
            Self::Literal(lit) => lit.span(),
            Self::Key(path) => path.span(),
        }
    }

    /// The `DefaultText` expression.
    pub(crate) fn tokens(&self) -> TokenStream {
        let ui = ui();
        match self {
            Self::Literal(lit) => quote!(#ui::__macro::DefaultText::Literal(#lit)),
            Self::Key(path) => quote!(#ui::__macro::DefaultText::Key(#path)),
        }
    }
}

/// The parsed `defaults(..)` of one kind.
pub(crate) enum Defaults {
    Button {
        style: Ident,
        label: Option<TextValue>,
        emoji: Option<EmojiLit>,
        disabled: bool,
    },
    Select {
        placeholder: TextValue,
        min_values: Option<u8>,
        max_values: Option<u8>,
        disabled: bool,
        options: Option<Path>,
        channel_types: Vec<Ident>,
    },
    Modal {
        title: TextValue,
    },
}

const STYLES: [&str; 4] = ["Primary", "Secondary", "Success", "Danger"];

const EMOJI_FORM: &str = "custom emoji literals are `<:name:id>` or `<a:name:id>`";

fn string_lit(expr: &Expr) -> Option<&LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Some(s),
        Expr::Group(g) => string_lit(&g.expr),
        _ => None,
    }
}

fn int_lit(expr: &Expr) -> Option<&LitInt> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => Some(i),
        Expr::Group(g) => int_lit(&g.expr),
        _ => None,
    }
}

fn bool_lit(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => Some(b.value),
        Expr::Group(g) => bool_lit(&g.expr),
        _ => None,
    }
}

fn literal_error(expr: &Expr, key: &str, what: &str) -> syn::Error {
    syn::Error::new_spanned(
        expr,
        format!("`defaults` values are literals: `{key}` takes {what}"),
    )
}

/// The path of `expr` after `Expr::Group` unwrapping.
fn key_path(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Path(_) => Some(expr),
        Expr::Group(g) => key_path(&g.expr),
        _ => None,
    }
}

/// A `label`, `placeholder` or `title`: a literal or a locale key path.
fn text_value(expr: &Expr, key: &str) -> syn::Result<TextValue> {
    if let Some(path) = key_path(expr) {
        return Ok(TextValue::Key(path.clone()));
    }
    literal_text(expr, key).map(TextValue::Literal)
}

/// A text that must be a string literal (`emoji`).
fn literal_text(expr: &Expr, key: &str) -> syn::Result<LitStr> {
    string_lit(expr)
        .cloned()
        .ok_or_else(|| literal_error(expr, key, "a string literal"))
}

fn count(expr: &Expr, key: &str) -> syn::Result<(usize, LitInt)> {
    let lit = int_lit(expr).ok_or_else(|| literal_error(expr, key, "an integer literal"))?;
    let n = lit
        .base10_parse::<usize>()
        .map_err(|_| literal_error(expr, key, "an integer literal"))?;
    Ok((n, lit.clone()))
}

/// Parses `<:name:id>` / `<a:name:id>`, or keeps any other text as a
/// Unicode emoji.
pub(crate) fn parse_emoji(text: &str) -> Result<EmojiLit, &'static str> {
    if text.is_empty() {
        return Err("`emoji` is empty");
    }
    let Some(inner) = text.strip_prefix('<') else {
        return Ok(EmojiLit::Unicode(text.to_owned()));
    };
    let inner = inner.strip_suffix('>').ok_or(EMOJI_FORM)?;
    let (animated, rest) = match inner.strip_prefix("a:") {
        Some(rest) => (true, rest),
        None => (false, inner.strip_prefix(':').ok_or(EMOJI_FORM)?),
    };
    let (name, id) = rest.split_once(':').ok_or(EMOJI_FORM)?;
    if name.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(EMOJI_FORM);
    }
    match id.parse::<u64>() {
        Ok(id) if id > 0 => Ok(EmojiLit::Custom {
            animated,
            name: name.to_owned(),
            id,
        }),
        _ => Err(EMOJI_FORM),
    }
}

/// The CV2-09 count violation of `key = n`.
fn count_violation(key: &str, n: usize, min: usize) -> Option<Violation> {
    let max = Limits::default().select_values;
    (n < min || n > max).then(|| {
        Violation::new(
            RuleId::CV2_09,
            format!("defaults.{key}"),
            Bound::Between { min, max },
            Actual::Count(n),
        )
    })
}

/// The parsed key-values, every error collected.
#[derive(Default)]
struct Raw {
    style: Option<Ident>,
    label: Option<TextValue>,
    emoji: Option<EmojiLit>,
    disabled: Option<bool>,
    placeholder: Option<TextValue>,
    min_values: Option<(usize, LitInt)>,
    max_values: Option<(usize, LitInt)>,
    options: Option<Path>,
    channel_types: Option<Vec<Ident>>,
    title: Option<TextValue>,
}

fn parse_style(expr: &Expr) -> syn::Result<Ident> {
    let ident = match expr {
        Expr::Path(p) if p.qself.is_none() => p.path.segments.last().map(|s| s.ident.clone()),
        _ => None,
    };
    let message = "`style` takes `Primary`, `Secondary`, `Success` or `Danger`";
    let ident = ident.ok_or_else(|| syn::Error::new_spanned(expr, message))?;
    let name = ident.to_string();
    if name == "Link" || name == "Premium" {
        return Err(syn::Error::new_spanned(
            expr,
            "Link and Premium buttons have no handler: build them with `LinkButton::new(url)` / `PremiumButton`",
        ));
    }
    if !STYLES.contains(&name.as_str()) {
        return Err(syn::Error::new_spanned(expr, message));
    }
    Ok(ident)
}

fn parse_channel_types(expr: &Expr) -> syn::Result<Vec<Ident>> {
    let Expr::Array(array) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            "`channel_types` takes a list of channel types: `channel_types = [GuildText]`",
        ));
    };
    if array.elems.is_empty() {
        return Err(syn::Error::new_spanned(
            expr,
            "`channel_types` lists at least one type",
        ));
    }
    let names: Vec<&str> = CHANNEL_TYPES.iter().map(|(n, _)| *n).collect();
    let mut out: Vec<Ident> = Vec::new();
    for elem in &array.elems {
        let ident = match elem {
            Expr::Path(p) if p.qself.is_none() => p.path.segments.last().map(|s| s.ident.clone()),
            _ => None,
        };
        let Some(ident) = ident.filter(|i| names.contains(&i.to_string().as_str())) else {
            return Err(syn::Error::new_spanned(
                elem,
                format!(
                    "unknown channel type `{}`; expected one of {}",
                    elem.to_token_stream(),
                    names.join(", ")
                ),
            ));
        };
        if out.contains(&ident) {
            return Err(syn::Error::new_spanned(
                elem,
                format!("duplicate channel type `{ident}`"),
            ));
        }
        out.push(ident);
    }
    Ok(out)
}

/// Parses one `key = value` into `raw`.
fn parse_pair(raw: &mut Raw, kind: DefaultsKind, key: &str, expr: &Expr) -> syn::Result<()> {
    let limits = Limits::default();
    match key {
        "style" => raw.style = Some(parse_style(expr)?),
        "label" => {
            let value = text_value(expr, key)?;
            let TextValue::Literal(lit) = &value else {
                raw.label = Some(value);
                return Ok(());
            };
            if lit.value().is_empty() {
                return Err(syn::Error::new_spanned(
                    lit,
                    "`label` is empty: omit it and give `emoji`",
                ));
            }
            let errors = validate_button_label(&lit.value(), &limits)
                .into_iter()
                .map(|mut v| {
                    "defaults.label".clone_into(&mut v.path);
                    syn::Error::new_spanned(lit, v.to_string())
                })
                .collect();
            combine(errors)?;
            raw.label = Some(value);
        }
        "emoji" => {
            let lit = literal_text(expr, key)?;
            let emoji = parse_emoji(&lit.value()).map_err(|e| syn::Error::new_spanned(&lit, e))?;
            raw.emoji = Some(emoji);
        }
        "disabled" => {
            raw.disabled = Some(bool_lit(expr).ok_or_else(|| {
                syn::Error::new_spanned(expr, "`disabled` takes `true` or `false`")
            })?);
        }
        "placeholder" => {
            let value = text_value(expr, key)?;
            if let TextValue::Literal(lit) = &value {
                let errors = validate_select_placeholder(&lit.value(), &limits)
                    .into_iter()
                    .map(|mut v| {
                        "defaults.placeholder".clone_into(&mut v.path);
                        syn::Error::new_spanned(lit, v.to_string())
                    })
                    .collect();
                combine(errors)?;
            }
            raw.placeholder = Some(value);
        }
        "min_values" | "max_values" => {
            let (n, lit) = count(expr, key)?;
            let min = usize::from(key == "max_values");
            if let Some(v) = count_violation(key, n, min) {
                return Err(syn::Error::new_spanned(&lit, v.to_string()));
            }
            if key == "min_values" {
                raw.min_values = Some((n, lit));
            } else {
                raw.max_values = Some((n, lit));
            }
        }
        "options" => match expr {
            Expr::Path(p) if p.qself.is_none() => raw.options = Some(p.path.clone()),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "`options` takes a `#[derive(Choices)]` enum: `options = Colour`",
                ));
            }
        },
        "channel_types" => raw.channel_types = Some(parse_channel_types(expr)?),
        _ => {
            debug_assert_eq!(kind, DefaultsKind::Modal);
            raw.title = Some(text_value(expr, key)?);
        }
    }
    Ok(())
}

/// Parses the `defaults(..)` block of `kind`; `block` is its `defaults`
/// ident and inner tokens, `attr` the attribute it was missing from.
pub(crate) fn parse(
    kind: DefaultsKind,
    block: Option<&(Ident, TokenStream)>,
    attr: &syn::Attribute,
) -> syn::Result<Defaults> {
    let Some((ident, tokens)) = block else {
        return Err(syn::Error::new_spanned(attr, kind.needs()));
    };
    let pairs = syn::parse::Parser::parse2(
        Punctuated::<MetaNameValue, Token![,]>::parse_terminated,
        tokens.clone(),
    )?;
    if pairs.is_empty() {
        return Err(syn::Error::new_spanned(ident, kind.needs()));
    }
    let mut raw = Raw::default();
    let mut seen: Vec<String> = Vec::new();
    let mut errors = Vec::new();
    for pair in &pairs {
        let key = pair.path.get_ident().map_or_else(
            || pair.path.to_token_stream().to_string(),
            ToString::to_string,
        );
        if !kind.keys().contains(&key.as_str()) {
            errors.push(syn::Error::new_spanned(
                &pair.path,
                format!(
                    "unknown `defaults` key `{key}` for `#[{}]`: expected {}",
                    kind.attr(),
                    kind.key_list()
                ),
            ));
            continue;
        }
        if seen.contains(&key) {
            errors.push(syn::Error::new_spanned(
                &pair.path,
                format!("duplicate `defaults` key `{key}`"),
            ));
            continue;
        }
        seen.push(key.clone());
        if let Err(e) = parse_pair(&mut raw, kind, &key, &pair.value) {
            errors.push(e);
        }
    }
    combine(errors)?;
    finish(kind, raw, ident)
}

/// The required keys and the cross-key checks.
fn finish(kind: DefaultsKind, raw: Raw, ident: &Ident) -> syn::Result<Defaults> {
    let needs = || syn::Error::new_spanned(ident, kind.needs());
    match kind {
        DefaultsKind::Button => {
            let style = raw.style.ok_or_else(needs)?;
            if raw.label.is_none() && raw.emoji.is_none() {
                return Err(needs());
            }
            Ok(Defaults::Button {
                style,
                label: raw.label,
                emoji: raw.emoji,
                disabled: raw.disabled.unwrap_or(false),
            })
        }
        DefaultsKind::Modal => Ok(Defaults::Modal {
            title: raw.title.ok_or_else(needs)?,
        }),
        _ => {
            let placeholder = raw.placeholder.ok_or_else(needs)?;
            if let (Some((min, lit)), Some((max, _))) = (&raw.min_values, &raw.max_values)
                && min > max
            {
                let v = Violation::new(
                    RuleId::CV2_09,
                    "defaults.min_values",
                    Bound::Describe("at most `max_values`"),
                    Actual::Text(format!("{min} > {max}")),
                );
                return Err(syn::Error::new_spanned(lit, v.to_string()));
            }
            let narrow = |v: Option<(usize, LitInt)>| v.and_then(|(n, _)| u8::try_from(n).ok());
            Ok(Defaults::Select {
                placeholder,
                min_values: narrow(raw.min_values),
                max_values: narrow(raw.max_values),
                disabled: raw.disabled.unwrap_or(false),
                options: raw.options,
                channel_types: raw.channel_types.unwrap_or_default(),
            })
        }
    }
}

fn ui() -> TokenStream {
    quote!(::noctis::__private::ui)
}

fn opt_u8(v: Option<u8>) -> TokenStream {
    crate::common::option(v.map(Literal::u8_unsuffixed))
}

impl Defaults {
    /// The `SelectDefaults` expression of a select.
    fn select_tokens(&self) -> TokenStream {
        let ui = ui();
        let Self::Select {
            placeholder,
            min_values,
            max_values,
            disabled,
            ..
        } = self
        else {
            return TokenStream::new();
        };
        let (min, max) = (opt_u8(*min_values), opt_u8(*max_values));
        let placeholder = placeholder.tokens();
        quote! {
            #ui::__macro::SelectDefaults {
                placeholder: ::core::option::Option::Some(#placeholder),
                min_values: #min,
                max_values: #max,
                disabled: #disabled,
            }
        }
    }

    /// The `(type, const expression)` of `ComponentRoute::DEFAULTS`.
    pub(crate) fn tokens(&self, kind: DefaultsKind) -> (TokenStream, TokenStream) {
        let ui = ui();
        match self {
            Self::Button {
                style,
                label,
                emoji,
                disabled,
            } => {
                let label = crate::common::option(label.as_ref().map(TextValue::tokens));
                let emoji = crate::common::option(emoji.as_ref().map(|e| match e {
                    EmojiLit::Unicode(text) => {
                        quote!(#ui::__macro::DefaultEmoji::Unicode(#text))
                    }
                    EmojiLit::Custom { animated, name, id } => {
                        let id = Literal::u64_unsuffixed(*id);
                        quote!(#ui::__macro::DefaultEmoji::Custom { id: #id, name: #name, animated: #animated })
                    }
                }));
                (
                    quote!(#ui::__macro::ButtonDefaults),
                    quote! {
                        #ui::__macro::ButtonDefaults {
                            style: #ui::ButtonStyle::#style,
                            label: #label,
                            emoji: #emoji,
                            disabled: #disabled,
                        }
                    },
                )
            }
            Self::Modal { title } => {
                let title = title.tokens();
                (
                    quote!(#ui::__macro::ModalDefaults),
                    quote!(#ui::__macro::ModalDefaults { title: #title }),
                )
            }
            Self::Select {
                options,
                channel_types,
                ..
            } => {
                let select = self.select_tokens();
                match kind {
                    DefaultsKind::StringSelect => {
                        let options = crate::common::option(options.as_ref().map(|p| {
                            quote_spanned! {p.span()=>
                                #ui::__macro::choices_options::<#p> as fn() -> ::std::vec::Vec<#ui::SelectOption>
                            }
                        }));
                        (
                            quote!(#ui::__macro::StringSelectDefaults),
                            quote! {
                                #ui::__macro::StringSelectDefaults { select: #select, options: #options }
                            },
                        )
                    }
                    DefaultsKind::ChannelSelect => (
                        quote!(#ui::__macro::ChannelSelectDefaults),
                        quote! {
                            #ui::__macro::ChannelSelectDefaults {
                                select: #select,
                                channel_types: &[#(#ui::__macro::ChannelType::#channel_types),*],
                            }
                        },
                    ),
                    _ => (quote!(#ui::__macro::SelectDefaults), select),
                }
            }
        }
    }

    /// The CV2-09 check of `options = C`: at most 25 choices (its `Choices`
    /// bound is the `choices_options` call's).
    pub(crate) fn checks(&self) -> TokenStream {
        let Self::Select {
            options: Some(path),
            ..
        } = self
        else {
            return TokenStream::new();
        };
        let ui = ui();
        let max = Limits::default().select_options;
        let v = Violation::new(
            RuleId::CV2_09,
            "defaults.options",
            Bound::AtMost(max),
            Actual::Text(format!("more than {max}")),
        )
        .with_note(format!(
            "`{}` has more variants than a String Select takes",
            path.to_token_stream().to_string().replace(' ', "")
        ));
        let text = panic_lit(&v.to_string());
        quote_spanned! {path.span()=>
            const _: () = if <#path as #ui::Choices>::ALL.len() > #max {
                ::core::panic!(#text)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::Span;

    use super::*;

    fn attr() -> syn::Attribute {
        syn::parse_quote!(#[button("a")])
    }

    fn run(kind: DefaultsKind, tokens: TokenStream) -> Result<Defaults, String> {
        let block = (Ident::new("defaults", Span::call_site()), tokens);
        parse(kind, Some(&block), &attr()).map_err(|e| {
            e.into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n---\n")
        })
    }

    fn err(kind: DefaultsKind, tokens: TokenStream) -> String {
        match run(kind, tokens) {
            Ok(_) => "ok".to_owned(),
            Err(e) => e,
        }
    }

    use DefaultsKind::{Button, ChannelSelect, Modal, RoleSelect, StringSelect, UserSelect};

    /// Story 11.1 Matrix 64, 68, 69: a path for `label`, `placeholder` or
    /// `title` lowers to `DefaultText::Key` with the path; other
    /// non-literals keep the generic error.
    #[test]
    fn key_paths_lower_to_keys() {
        let (_, t) = run(Button, quote!(style = Primary, label = keys::SAVE))
            .ok()
            .unwrap()
            .tokens(Button);
        let t = t.to_string();
        assert!(t.contains("label : :: core :: option :: Option :: Some (:: noctis :: __private :: ui :: __macro :: DefaultText :: Key (keys :: SAVE))"), "{t}");
        let (_, t) = run(UserSelect, quote!(placeholder = keys::P))
            .ok()
            .unwrap()
            .tokens(UserSelect);
        assert!(
            t.to_string().contains("DefaultText :: Key (keys :: P)"),
            "{t}"
        );
        let (_, t) = run(Modal, quote!(title = T)).ok().unwrap().tokens(Modal);
        assert!(
            t.to_string().contains(
                "title : :: noctis :: __private :: ui :: __macro :: DefaultText :: Key (T)"
            ),
            "{t}"
        );
        let (_, t) = run(Button, quote!(style = Primary, label = "Buy"))
            .ok()
            .unwrap()
            .tokens(Button);
        assert!(
            t.to_string().contains("DefaultText :: Literal (\"Buy\")"),
            "{t}"
        );
        let e = err(Button, quote!(style = Primary, label = concat!("a")));
        assert!(e.contains("`defaults` values are literals"), "{e}");
        let e = err(Button, quote!(style = Primary, emoji = keys::E));
        assert!(e.contains("`defaults` values are literals"), "{e}");
    }

    /// Story 6.11 Matrix 29.
    #[test]
    fn button_defaults() {
        let Ok(Defaults::Button { style, label, .. }) =
            run(Button, quote!(style = Primary, label = "Buy"))
        else {
            panic!("button");
        };
        assert_eq!(
            (
                style.to_string(),
                label
                    .as_ref()
                    .and_then(TextValue::literal)
                    .map(LitStr::value)
            ),
            ("Primary".into(), Some("Buy".into()))
        );
        assert_eq!(err(Button, quote!(style = Primary, emoji = "➡️")), "ok");
        let (_, t) = run(
            Button,
            quote!(style = Success, emoji = "<a:pt:123>", disabled = true),
        )
        .ok()
        .unwrap()
        .tokens(Button);
        let t = t.to_string();
        assert!(
            t.contains("id : 123 , name : \"pt\" , animated : true"),
            "{t}"
        );
        assert!(t.contains("disabled : true"), "{t}");
        assert!(t.contains("ButtonStyle :: Success"), "{t}");
    }

    /// Story 6.11 Matrix 33-39.
    #[test]
    fn button_errors() {
        let needs = Button.needs();
        assert_eq!(err(Button, quote!()), needs);
        assert_eq!(err(Button, quote!(label = "a")), needs);
        assert_eq!(err(Button, quote!(style = Primary)), needs);
        assert_eq!(
            err(Button, quote!(colour = Red)),
            "unknown `defaults` key `colour` for `#[button]`: expected `style`, `label`, `emoji` or `disabled`"
        );
        assert_eq!(
            err(Button, quote!(style = Primary, label = "a", label = "b")),
            "duplicate `defaults` key `label`"
        );
        assert_eq!(err(Button, quote!(style = Primary, label = LABEL)), "ok");
        assert_eq!(
            err(
                Button,
                quote!(style = Primary, label = "a", disabled = "yes")
            ),
            "`disabled` takes `true` or `false`"
        );
        for s in [quote!(Link), quote!(Premium)] {
            assert!(
                err(Button, quote!(style = #s, label = "a"))
                    .starts_with("Link and Premium buttons have no handler")
            );
        }
        assert_eq!(
            err(Button, quote!(style = Blurple, label = "a")),
            "`style` takes `Primary`, `Secondary`, `Success` or `Danger`"
        );
        let l80 = "x".repeat(80);
        let l81 = "x".repeat(81);
        assert_eq!(err(Button, quote!(style = Primary, label = #l80)), "ok");
        assert!(err(Button, quote!(style = Primary, label = #l81)).starts_with("[CV2-08]"));
        let e40 = "😀".repeat(40);
        let e41 = "😀".repeat(41);
        assert_eq!(err(Button, quote!(style = Primary, label = #e40)), "ok");
        assert!(err(Button, quote!(style = Primary, label = #e41)).starts_with("[CV2-08]"));
        assert_eq!(
            err(Button, quote!(style = Primary, label = "")),
            "`label` is empty: omit it and give `emoji`"
        );
        assert_eq!(err(Button, quote!(style = Primary, label = "Bü")), "ok");
        assert_eq!(
            err(Button, quote!(style = Primary, emoji = "")),
            "`emoji` is empty"
        );
        for e in ["<:x>", "<:x:abc>", "<:x:0>"] {
            assert_eq!(
                err(Button, quote!(style = Primary, emoji = #e)),
                EMOJI_FORM,
                "{e}"
            );
        }
    }

    #[test]
    fn emoji_literals() {
        assert_eq!(parse_emoji("➡️"), Ok(EmojiLit::Unicode("➡️".into())));
        assert_eq!(
            parse_emoji("<:pt:123>"),
            Ok(EmojiLit::Custom {
                animated: false,
                name: "pt".into(),
                id: 123
            })
        );
        assert_eq!(parse_emoji("<a:pt:1"), Err(EMOJI_FORM));
    }

    /// Story 6.11 Matrix 30, 35, 40-42.
    #[test]
    fn select_defaults() {
        assert_eq!(err(UserSelect, quote!(placeholder = "Pick")), "ok");
        assert_eq!(err(UserSelect, quote!(min_values = 1)), UserSelect.needs());
        assert_eq!(
            UserSelect.needs(),
            "`#[user_select]` needs `defaults(..)` with `placeholder`"
        );
        assert_eq!(
            err(UserSelect, quote!(placeholder = "p", options = Colour)),
            "unknown `defaults` key `options` for `#[user_select]`: expected `placeholder`, `min_values`, `max_values` or `disabled`"
        );
        assert!(
            err(
                RoleSelect,
                quote!(placeholder = "p", channel_types = [GuildText])
            )
            .starts_with("unknown `defaults` key `channel_types`")
        );
        assert!(err(UserSelect, quote!(label = "p")).starts_with("unknown `defaults` key `label`"));
        assert_eq!(
            err(UserSelect, quote!(placeholder = "p", min_values = n)),
            "`defaults` values are literals: `min_values` takes an integer literal"
        );
        let p150 = "x".repeat(150);
        let p151 = "x".repeat(151);
        assert_eq!(err(UserSelect, quote!(placeholder = #p150)), "ok");
        assert!(err(UserSelect, quote!(placeholder = #p151)).starts_with("[CV2-09]"));
        for (tokens, ok) in [
            (quote!(min_values = 0, max_values = 25), true),
            (quote!(min_values = 25), true),
            (quote!(min_values = 26), false),
            (quote!(max_values = 0), false),
            (quote!(max_values = 1), true),
            (quote!(max_values = 26), false),
            (quote!(min_values = 3, max_values = 2), false),
        ] {
            let e = err(UserSelect, quote!(placeholder = "p", #tokens));
            assert_eq!(e == "ok", ok, "{tokens}: {e}");
            if !ok {
                assert!(e.starts_with("[CV2-09]"), "{e}");
            }
        }
        let t = run(UserSelect, quote!(placeholder = "Pick"))
            .ok()
            .unwrap()
            .tokens(UserSelect)
            .1
            .to_string();
        assert!(
            t.contains("min_values : :: core :: option :: Option :: None"),
            "{t}"
        );
        assert!(t.contains("disabled : false"), "{t}");
        let s = run(StringSelect, quote!(placeholder = "p", options = Colour))
            .ok()
            .unwrap();
        assert!(
            s.tokens(StringSelect)
                .1
                .to_string()
                .contains("choices_options :: < Colour >")
        );
        assert!(s.checks().to_string().contains("[CV2-09]"));
        assert_eq!(
            err(
                ChannelSelect,
                quote!(placeholder = "p", channel_types = [GuildText])
            ),
            "ok"
        );
        let c = run(
            ChannelSelect,
            quote!(placeholder = "p", channel_types = [GuildText, GuildVoice]),
        )
        .ok()
        .unwrap()
        .tokens(ChannelSelect)
        .1
        .to_string();
        assert!(c.contains("ChannelType :: GuildText , :: noctis"), "{c}");
        assert_eq!(
            err(ChannelSelect, quote!(placeholder = "p", channel_types = [])),
            "`channel_types` lists at least one type"
        );
        assert!(
            err(
                ChannelSelect,
                quote!(placeholder = "p", channel_types = [Bogus])
            )
            .starts_with("unknown channel type `Bogus`; expected one of GuildText")
        );
        assert_eq!(
            err(
                ChannelSelect,
                quote!(placeholder = "p", channel_types = [GuildText, GuildText])
            ),
            "duplicate channel type `GuildText`"
        );
    }

    /// Story 6.11 Matrix 31, 33.
    #[test]
    fn modal_defaults() {
        assert_eq!(err(Modal, quote!(title = "Bio")), "ok");
        assert_eq!(
            err(Modal, quote!()),
            "`#[modal]` needs `defaults(title = \"…\")`"
        );
        assert!(err(Modal, quote!(label = "x")).contains("expected `title`"));
        assert_eq!(
            parse(Modal, None, &attr()).err().unwrap().to_string(),
            Modal.needs()
        );
    }
}
