//! `#[derive(UserCommand)]` / `#[derive(MessageCommand)]` with their
//! `#[user_command("Name", ..)]` / `#[message_command("Name", ..)]`
//! attribute: check a context-menu name at expansion with
//! `noctis_rules::command::validate_context_menu_name` and declare the unit
//! struct's menu for `#[handler]` (FR-12, AD-26; CTX-01, CTX-02, CTX-03).

use noctis_rules::Limits;
use noctis_rules::command::validate_context_menu_name;
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, LitStr};

use crate::command::{HandlerArgs, arg_name, segments, span_of, withdrawn};
use crate::common::combine;
use crate::meta::{self, MetaArgs};

/// Which context-menu macro is expanding.
#[derive(Clone, Copy)]
pub(crate) enum Menu {
    User,
    Message,
}

impl Menu {
    fn attr(self) -> &'static str {
        match self {
            Self::User => "#[user_command]",
            Self::Message => "#[message_command]",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Self::User => "Show profile",
            Self::Message => "Quote",
        }
    }

    fn target(self) -> &'static str {
        match self {
            Self::User => "ResolvedUser",
            Self::Message => "ResolvedMessage",
        }
    }

    fn derive(self) -> &'static str {
        match self {
            Self::User => "UserCommand",
            Self::Message => "MessageCommand",
        }
    }
}

/// Arguments later stories add.
const PLANNED: [&str; 4] = ["key", "rate_limit", "flags", "apps"];

const EXPECTED: &str = "expected `response`, `auto_defer`, `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds`";

const NO_DESCRIPTION: &str = "[CTX-02] context-menu commands have no description";

/// The parsed arguments.
struct Args {
    name: LitStr,
    handler: HandlerArgs,
    meta: MetaArgs,
}

/// The error of a menu attribute without its name.
fn missing(menu: Menu) -> String {
    let attr = menu.attr();
    format!(
        "`{attr}` needs the menu name as its first argument: `{}(\"{}\")]`",
        attr.trim_end_matches(']'),
        menu.example()
    )
}

fn parse_args(menu: Menu, args: TokenStream) -> syn::Result<Args> {
    let attr = menu.attr();
    let missing = missing(menu);
    let not_literal = format!("`{attr}` needs its menu name as a string literal");
    let mut iter = segments(args).into_iter();
    let first = iter.next().unwrap_or_default();
    if first.is_empty() {
        return Err(syn::Error::new(Span::call_site(), missing));
    }
    if matches!(first.get(1), Some(TokenTree::Punct(p)) if p.as_char() == '=') {
        return Err(syn::Error::new(span_of(&first), missing));
    }
    let tokens: TokenStream = first.iter().cloned().collect();
    let name =
        syn::parse2::<LitStr>(tokens).map_err(|_| syn::Error::new(span_of(&first), not_literal))?;
    let mut parsed = Args {
        name,
        handler: HandlerArgs::default(),
        meta: MetaArgs::default(),
    };
    let mut seen: Vec<String> = Vec::new();
    for segment in iter {
        let span = span_of(&segment);
        let Some(name) = arg_name(&segment) else {
            let tokens: TokenStream = segment.iter().cloned().collect();
            return Err(syn::Error::new(
                span,
                format!("unknown `{attr}` argument `{tokens}`: {EXPECTED}"),
            ));
        };
        if let Some(error) = withdrawn(attr, &name, span) {
            return Err(error);
        }
        match name.as_str() {
            "description" => return Err(syn::Error::new(span, NO_DESCRIPTION)),
            "response" | "auto_defer" => {}
            other if meta::is_meta(other) => {}
            planned if PLANNED.contains(&planned) => {
                return Err(syn::Error::new(
                    span,
                    format!("`{attr}` argument `{name}` is planned and not supported yet"),
                ));
            }
            _ => {
                return Err(syn::Error::new(
                    span,
                    format!("unknown `{attr}` argument `{name}`: {EXPECTED}"),
                ));
            }
        }
        if seen.contains(&name) {
            return Err(syn::Error::new(
                span,
                format!("duplicate `{attr}` argument `{name}`"),
            ));
        }
        seen.push(name.clone());
        if let Some(result) = parsed.handler.parse(&name, &segment) {
            result?;
            continue;
        }
        if let Some(result) = parsed.meta.parse(&name, &segment) {
            result?;
        }
    }
    Ok(parsed)
}

/// CTX-01 on the name literal, then META-03, as one error.
fn check_name(args: &Args) -> syn::Result<()> {
    let mut errors: Vec<syn::Error> =
        validate_context_menu_name(&args.name.value(), &Limits::default())
            .iter()
            .map(|v| syn::Error::new(args.name.span(), v.to_string()))
            .collect();
    errors.extend(args.meta.meta03());
    combine(errors)
}

/// Expands the derive; on an error, the error and the placeholder impls
/// (`kind::fallback`).
pub(crate) fn derive_or_fallback(menu: Menu, input: &DeriveInput) -> TokenStream {
    let result = derive(menu, input);
    crate::kind::or_fallback(input, crate::kind::Family::Menu, result, || {
        let core = quote!(::noctis::__private::core);
        let ident = &input.ident;
        let target = Ident::new(menu.target(), Span::call_site());
        quote! {
            impl #core::__ContextMenu for #ident {
                type Target = ::noctis::__private::ui::#target;
                const MENU_NAME: &'static ::core::primitive::str = "";
                const METADATA: #core::CommandMetadata = #core::CommandMetadata::new();
                const EPHEMERALITY: #core::Ephemerality = #core::Ephemerality::Public;

                fn __new() -> Self {
                    ::core::unreachable!("the derive failed to compile")
                }
            }
        }
    })
}

/// Expands `#[derive(UserCommand)]` or `#[derive(MessageCommand)]`.
pub(crate) fn derive(menu: Menu, input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = crate::common::kind_fields(input, menu.derive())?;
    let ident = &input.ident;
    if !fields.is_empty()
        || !matches!(&input.data, Data::Struct(d) if matches!(d.fields, Fields::Unit))
    {
        return Err(syn::Error::new_spanned(
            ident,
            format!(
                "[CTX-02] context-menu commands have no options: make `{}` a unit struct",
                crate::common::plain(ident)
            ),
        ));
    }
    let attr = menu.attr();
    let name = attr.trim_start_matches("#[").trim_end_matches(']');
    let (_, tokens) = crate::command::kind_attr(
        &input.attrs,
        name,
        ident,
        &format!(
            "`#[derive({})]` needs `{}(\"{}\")]`",
            menu.derive(),
            attr.trim_end_matches(']'),
            menu.example()
        ),
        &missing(menu),
    )?;
    let args = parse_args(menu, tokens)?;
    check_name(&args)?;
    let core = quote!(::noctis::__private::core);
    let target = Ident::new(menu.target(), Span::call_site());
    let menu_name = args.name.value();
    let metadata = if args.meta.is_empty() {
        quote!(#core::CommandMetadata::new())
    } else {
        args.meta.tokens()
    };
    let ephemerality = args.handler.ephemerality();
    let scaffolding =
        crate::kind::scaffolding(ident, crate::kind::Family::Menu, args.handler.manual);
    Ok(quote! {
        #[automatically_derived]
        impl #core::__ContextMenu for #ident {
            type Target = ::noctis::__private::ui::#target;
            const MENU_NAME: &'static ::core::primitive::str = #menu_name;
            const METADATA: #core::CommandMetadata = #metadata;
            const EPHEMERALITY: #core::Ephemerality = #ephemerality;

            fn __new() -> Self {
                Self
            }
        }

        #scaffolding
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str =
        "async fn show(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<()> { Ok(()) }";
    const QUOTE: &str =
        "async fn quote(ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<()> { Ok(()) }";

    fn item(src: &str) -> TokenStream {
        src.parse().unwrap()
    }

    /// The menu derive with `args` on a unit struct (the old cases name a
    /// handler `item`, which only picks the kind).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the test cases hand over `quote!` values"
    )]
    fn expand(menu: Menu, args: TokenStream, _item: TokenStream) -> syn::Result<TokenStream> {
        let input: DeriveInput = match menu {
            Menu::User => syn::parse_quote!(
                #[user_command(#args)]
                struct S;
            ),
            Menu::Message => syn::parse_quote!(
                #[message_command(#args)]
                struct S;
            ),
        };
        derive(menu, &input)
    }

    /// Story 6.11 Matrix 6 and 11.
    #[test]
    fn expands_both_kinds() {
        let out = expand(
            Menu::User,
            quote!("Report", response = ephemeral, auto_defer = false),
            item(SHOW),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains("type Target = :: noctis :: __private :: ui :: ResolvedUser"),
            "{out}"
        );
        assert!(
            out.contains("const MENU_NAME : & 'static :: core :: primitive :: str = \"Report\""),
            "{out}"
        );
        assert!(out.contains("Ephemerality :: Ephemeral"), "{out}");
        assert!(
            out.contains("type Defer = :: noctis :: __private :: core :: Manual"),
            "{out}"
        );
        assert!(out.contains("__MenuKind"), "{out}");
        let out = expand(Menu::Message, quote!("Quote"), item(QUOTE))
            .unwrap()
            .to_string();
        assert!(out.contains("ResolvedMessage"), "{out}");
        assert!(out.contains("Ephemerality :: Public"), "{out}");
        let fields: DeriveInput = syn::parse_quote!(
            #[user_command("X")]
            struct Report {
                a: u8,
            }
        );
        assert_eq!(
            derive(Menu::User, &fields).unwrap_err().to_string(),
            "[CTX-02] context-menu commands have no options: make `Report` a unit struct"
        );
        let none: DeriveInput = syn::parse_quote!(
            struct Report;
        );
        assert_eq!(
            derive(Menu::Message, &none).unwrap_err().to_string(),
            "`#[derive(MessageCommand)]` needs `#[message_command(\"Quote\")]`"
        );
    }

    fn error(menu: Menu, args: TokenStream, src: &str) -> String {
        expand(menu, args, item(src)).unwrap_err().to_string()
    }

    fn messages(menu: Menu, args: TokenStream, src: &str) -> Vec<String> {
        expand(menu, args, item(src))
            .unwrap_err()
            .into_iter()
            .map(|e| e.to_string())
            .collect()
    }

    /// Matrix 36: metadata on menus.
    #[test]
    fn metadata_on_menus() {
        let out = expand(
            Menu::User,
            quote!(
                "Show profile",
                permissions = KICK_MEMBERS,
                contexts = [Guild],
                integration_types = [GuildInstall],
                nsfw = false
            ),
            item(SHOW),
        )
        .unwrap()
        .to_string()
        .replace(' ', "");
        assert!(out.contains("from_bits_retain(2u64)"), "{out}");
        assert!(out.contains(".nsfw(false)"), "{out}");
        assert!(out.contains("IntegrationType::GuildInstall"), "{out}");
    }

    /// Matrix 31-35: CTX-01, alone and with META-03.
    #[test]
    fn ctx_01_name_length() {
        for name in [
            "a".repeat(32),
            "😀".repeat(16),
            "x".to_owned(),
            "Show Profile".to_owned(),
            "It's".to_owned(),
            "Report 🚩".to_owned(),
            " padded ".to_owned(),
        ] {
            assert!(
                expand(Menu::User, quote!(#name), item(SHOW)).is_ok(),
                "{name}"
            );
        }
        for (name, actual) in [
            ("a".repeat(33), 33),
            ("😀".repeat(17), 34),
            (String::new(), 0),
        ] {
            assert_eq!(
                messages(Menu::User, quote!(#name), SHOW),
                [format!("[CTX-01] limit between 1 and 32, actual {actual}")]
            );
        }
        let long = "a".repeat(33);
        let errs = messages(
            Menu::Message,
            quote!(#long, guilds = [1], contexts = [Guild]),
            QUOTE,
        );
        assert_eq!(
            errs,
            [
                "[CTX-01] limit between 1 and 32, actual 33".to_owned(),
                "[META-03] contexts: limit only on global commands, actual `[Guild]`\n  = note: scope: guild 1".to_owned(),
            ]
        );
    }

    /// Matrix 37, 38 and 40: argument errors.
    #[test]
    fn argument_errors() {
        let missing = "`#[user_command]` needs the menu name as its first argument: `#[user_command(\"Show profile\")]`";
        let not_literal = "`#[user_command]` needs its menu name as a string literal";
        for (args, expected) in [
            (quote!(), missing.to_owned()),
            (quote!(name = "x"), missing.to_owned()),
            (quote!(NAME), not_literal.to_owned()),
            (quote!(b"x"), not_literal.to_owned()),
            (quote!("X", description = "d"), NO_DESCRIPTION.to_owned()),
            (
                quote!("X", max_expansions = 2),
                format!("unknown `#[user_command]` argument `max_expansions`: {EXPECTED}"),
            ),
            (
                quote!("X", foo),
                format!("unknown `#[user_command]` argument `foo`: {EXPECTED}"),
            ),
            (
                quote!("X", nsfw = true, nsfw = true),
                "duplicate `#[user_command]` argument `nsfw`".to_owned(),
            ),
            (
                quote!("X", dm_permission = false),
                "[META-05] `dm_permission` is deprecated by Discord and not supported; use `contexts = [Guild]` to keep a command out of DMs".to_owned(),
            ),
        ] {
            assert_eq!(error(Menu::User, args.clone(), SHOW), expected, "{args}");
        }
        for planned in PLANNED {
            let ident = quote::format_ident!("{planned}");
            assert_eq!(
                error(Menu::User, quote!("X", #ident = "show_profile"), SHOW),
                format!("`#[user_command]` argument `{planned}` is planned and not supported yet")
            );
        }
        assert_eq!(
            error(Menu::Message, quote!(), QUOTE),
            "`#[message_command]` needs the menu name as its first argument: `#[message_command(\"Quote\")]`"
        );
    }
}
