//! `#[user_command]` and `#[message_command]`: check a context-menu name at
//! expansion with `noctis_rules::command::validate_context_menu_name` and turn
//! an `async fn(ctx: CommandCtx, target: ResolvedUser | ResolvedMessage)`
//! into a descriptor function returning a `ContextMenuHandler` (FR-12, AD-26;
//! CTX-01, CTX-02, CTX-03).

use noctis_rules::Limits;
use noctis_rules::command::validate_context_menu_name;
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, LitStr, ReturnType, Type};

use crate::command::{arg_name, check_shape, segments, span_of};
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

    fn constructor(self) -> &'static str {
        match self {
            Self::User => "__new_user",
            Self::Message => "__new_message",
        }
    }

    fn signature(self) -> String {
        format!(
            "`{}` handlers take `(ctx: CommandCtx, target: {})`",
            self.attr(),
            self.target()
        )
    }
}

/// Arguments later stories add.
const PLANNED: [&str; 6] = [
    "key",
    "ephemeral",
    "opens_modal",
    "rate_limit",
    "flags",
    "apps",
];

const EXPECTED: &str =
    "expected `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds`";

const NO_DESCRIPTION: &str = "[CTX-02] context-menu commands have no description";

/// The parsed arguments.
struct Args {
    name: LitStr,
    meta: MetaArgs,
}

fn parse_args(menu: Menu, args: TokenStream) -> syn::Result<Args> {
    let attr = menu.attr();
    let missing = format!(
        "`{attr}` needs the menu name as its first argument: `{}(\"{}\")]`",
        attr.trim_end_matches(']'),
        menu.example()
    );
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
        match name.as_str() {
            "description" => return Err(syn::Error::new(span, NO_DESCRIPTION)),
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

/// Exactly `(ctx, target: <Target>)`; more parameters are CTX-02 options.
fn check_params(menu: Menu, item: &ItemFn) -> syn::Result<()> {
    let sig = &item.sig;
    let message = menu.signature();
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new_spanned(receiver, message));
    }
    if sig.inputs.len() > 2 && sig.inputs.iter().all(|a| matches!(a, FnArg::Typed(_))) {
        return Err(syn::Error::new(
            sig.inputs.span(),
            format!("[CTX-02] context-menu commands take no options: {message}"),
        ));
    }
    let Some(FnArg::Typed(target)) = sig.inputs.iter().nth(1).filter(|_| sig.inputs.len() == 2)
    else {
        let span = if sig.inputs.is_empty() {
            sig.ident.span()
        } else {
            sig.inputs.span()
        };
        return Err(syn::Error::new(span, message));
    };
    let ok = match &*target.ty {
        Type::Path(path) => {
            path.qself.is_none()
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == menu.target() && s.arguments.is_none())
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(&target.ty, message))
    }
}

/// Expands `#[user_command(args)] item` or `#[message_command(args)] item`.
pub(crate) fn expand(menu: Menu, args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = parse_args(menu, args)?;
    let item: ItemFn = syn::parse2(item)?;
    check_shape(&item, menu.attr())?;
    check_name(&args)?;
    check_params(menu, &item)?;

    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        ..
    } = &item;
    let name = &sig.ident;
    let fn_name = name.to_string();
    let fn_name = fn_name.strip_prefix("r#").unwrap_or(&fn_name).to_owned();
    let mut inner = sig.clone();
    inner.ident = Ident::new("__noctis_menu", name.span());
    let inner = inner.into_token_stream();
    let output_span = match &sig.output {
        ReturnType::Default => sig.ident.span(),
        ReturnType::Type(_, ty) => ty.span(),
    };
    let constructor = Ident::new(menu.constructor(), output_span);
    let new = quote_spanned! {output_span=>
        ::noctis::__private::core::ContextMenuHandler::#constructor
    };
    let menu_name = args.name.value();
    let metadata = if args.meta.is_empty() {
        quote!(::noctis::__private::core::CommandMetadata::new())
    } else {
        args.meta.tokens()
    };
    Ok(quote! {
        #(#attrs)*
        #vis fn #name() -> ::noctis::__private::core::ContextMenuHandler {
            #inner #block
            #new(::core::concat!(::core::module_path!(), "::", #fn_name), #menu_name, #metadata, __noctis_menu)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str =
        "async fn show(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<()> { Ok(()) }";
    const QUOTE: &str =
        "async fn quote(ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<()> { Ok(()) }";
    const USER_SIG: &str =
        "`#[user_command]` handlers take `(ctx: CommandCtx, target: ResolvedUser)`";

    fn item(src: &str) -> TokenStream {
        src.parse().unwrap()
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

    /// Matrix 29, 30 and 44.
    #[test]
    fn expands_both_kinds() {
        let out = expand(
            Menu::User,
            quote!("Show profile"),
            item(&format!("/// docs\n#[cfg(test)]\npub(crate) {SHOW}")),
        )
        .unwrap()
        .to_string();
        assert!(
            out.contains(
                "pub (crate) fn show () -> :: noctis :: __private :: core :: ContextMenuHandler"
            ),
            "{out}"
        );
        assert!(out.contains("# [doc = \" docs\"]"), "{out}");
        assert!(out.contains("# [cfg (test)]"), "{out}");
        assert!(out.contains("ContextMenuHandler :: __new_user"), "{out}");
        assert!(
            out.contains("\"Show profile\" , :: noctis :: __private :: core :: CommandMetadata :: new () , __noctis_menu"),
            "{out}"
        );
        let out = expand(Menu::Message, quote!(r"Quote"), item(QUOTE))
            .unwrap()
            .to_string();
        assert!(out.contains("__new_message"), "{out}");
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

    /// Matrix 39, 41 and 42: signatures.
    #[test]
    fn signature_errors() {
        let msg_sig =
            "`#[message_command]` handlers take `(ctx: CommandCtx, target: ResolvedMessage)`";
        for (menu, src, expected) in [
            (
                Menu::User,
                "async fn f(ctx: CommandCtx, target: ResolvedUser, opts: O) -> HandlerResult<()> { Ok(()) }",
                format!("[CTX-02] context-menu commands take no options: {USER_SIG}"),
            ),
            (
                Menu::User,
                "async fn f() -> HandlerResult<()> { Ok(()) }",
                USER_SIG.to_owned(),
            ),
            (
                Menu::User,
                "async fn f(ctx: CommandCtx) -> HandlerResult<()> { Ok(()) }",
                USER_SIG.to_owned(),
            ),
            (
                Menu::User,
                "async fn f(&self, t: ResolvedUser) -> HandlerResult<()> { Ok(()) }",
                USER_SIG.to_owned(),
            ),
            (
                Menu::User,
                "async fn f(ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<()> { Ok(()) }",
                USER_SIG.to_owned(),
            ),
            (
                Menu::User,
                "async fn f(ctx: CommandCtx, target: Id<UserMarker>) -> HandlerResult<()> { Ok(()) }",
                USER_SIG.to_owned(),
            ),
            (
                Menu::Message,
                "async fn f(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<()> { Ok(()) }",
                msg_sig.to_owned(),
            ),
            (
                Menu::User,
                "fn f(ctx: CommandCtx, t: ResolvedUser) -> HandlerResult<()> { Ok(()) }",
                "`#[user_command]` needs an `async fn`".to_owned(),
            ),
            (
                Menu::Message,
                "const async fn f(ctx: CommandCtx, t: ResolvedMessage) -> HandlerResult<()> { Ok(()) }",
                "`#[message_command]` handlers must be a plain `async fn`".to_owned(),
            ),
            (
                Menu::User,
                "async unsafe fn f(ctx: CommandCtx, t: ResolvedUser) -> HandlerResult<()> { Ok(()) }",
                "`#[user_command]` handlers must be a plain `async fn`".to_owned(),
            ),
            (
                Menu::User,
                "async extern \"C\" fn f(ctx: CommandCtx, t: ResolvedUser) -> HandlerResult<()> { Ok(()) }",
                "`#[user_command]` handlers must be a plain `async fn`".to_owned(),
            ),
            (
                Menu::User,
                "async fn f<T>(ctx: CommandCtx, t: ResolvedUser) -> HandlerResult<()> { Ok(()) }",
                "`#[user_command]` handlers cannot be generic".to_owned(),
            ),
        ] {
            assert_eq!(error(menu, quote!("X"), src), expected, "{src}");
        }
        assert!(
            expand(
                Menu::User,
                quote!("X"),
                item("async fn f(c: CommandCtx, t: noctis::ui::ResolvedUser) -> HandlerResult<()> { Ok(()) }")
            )
            .is_ok()
        );
    }
}
