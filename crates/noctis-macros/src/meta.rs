//! The metadata arguments `#[command]`, `#[user_command]` and
//! `#[message_command]` share: `permissions`, `contexts`,
//! `integration_types`, `nsfw` and `guilds`, lowered into a `const`
//! `CommandMetadata` (FR-11, AD-26; META-01, META-02, META-03, META-05,
//! CMD-07).

use noctis_rules::command::{
    CommandMeta, IntegrationType, InteractionContext, Scope, permission, validate_meta,
};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};
use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use quote::{format_ident, quote};
use syn::LitInt;

/// The metadata argument names, in field order.
pub(crate) const NAMES: [&str; 5] = [
    "permissions",
    "contexts",
    "integration_types",
    "nsfw",
    "guilds",
];

pub(crate) const PERMISSIONS_SYNTAX: &str = "[META-01] `permissions` takes permission names joined by `|`, such as `permissions = MANAGE_GUILD | BAN_MEMBERS`";
const NSFW_BOOL: &str = "[CMD-07] `nsfw` takes `true` or `false`";
const GUILDS_LIST: &str =
    "`guilds` takes a list of guild id integer literals, such as `guilds = [123456789012345678]`";
const GUILDS_EMPTY: &str = "`guilds` lists at least one guild id";
const GUILD_ZERO: &str = "guild ids are never 0";
const DM_PERMISSION: &str = "[META-05] `dm_permission` is deprecated by Discord and not supported; use `contexts = [Guild]` to keep a command out of DMs";

/// Whether `name` is handled by [`MetaArgs::parse`] (a metadata argument or
/// the rejected `dm_permission`).
pub(crate) fn is_meta(name: &str) -> bool {
    NAMES.contains(&name) || name == "dm_permission"
}

/// The parsed metadata arguments; each keeps the span of its name.
#[derive(Default)]
pub(crate) struct MetaArgs {
    permissions: Option<u64>,
    contexts: Option<(Span, Vec<InteractionContext>)>,
    integration_types: Option<(Span, Vec<IntegrationType>)>,
    nsfw: Option<bool>,
    guilds: Option<Vec<u64>>,
}

impl MetaArgs {
    /// Whether no metadata argument was given.
    pub(crate) fn is_empty(&self) -> bool {
        self.permissions.is_none()
            && self.contexts.is_none()
            && self.integration_types.is_none()
            && self.nsfw.is_none()
            && self.guilds.is_none()
    }

    /// Parses the argument `segment` named `name`; `None` when `name` is
    /// not a metadata argument. Duplicates are the caller's check.
    pub(crate) fn parse(&mut self, name: &str, segment: &[TokenTree]) -> Option<syn::Result<()>> {
        let span = segment
            .first()
            .map_or_else(Span::call_site, TokenTree::span);
        if name == "dm_permission" {
            return Some(Err(syn::Error::new(span, DM_PERMISSION)));
        }
        if !NAMES.contains(&name) {
            return None;
        }
        if matches!(segment.get(1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
        {
            return Some(Err(syn::Error::new(
                span,
                format!(
                    "[META-02] `{name}` applies to the base command of every path this handler declares; Discord keeps it only on the base command, so it has no per-path form"
                ),
            )));
        }
        let value: Option<Vec<TokenTree>> = match segment {
            [_, TokenTree::Punct(eq), rest @ ..] if eq.as_char() == '=' => Some(rest.to_vec()),
            _ => None,
        };
        Some(self.parse_value(name, span, value.as_deref()))
    }

    fn parse_value(
        &mut self,
        name: &str,
        span: Span,
        value: Option<&[TokenTree]>,
    ) -> syn::Result<()> {
        match name {
            "permissions" => {
                self.permissions = Some(permissions(span, value.unwrap_or_default())?);
            }
            "contexts" => {
                let list = names(
                    span,
                    value,
                    &Kind {
                        arg: "contexts",
                        what: "context",
                        example: "contexts = [Guild, BotDm, PrivateChannel]",
                    },
                    &InteractionContext::ALL,
                    InteractionContext::name,
                )?;
                self.contexts = Some((span, list));
            }
            "integration_types" => {
                let list = names(
                    span,
                    value,
                    &Kind {
                        arg: "integration_types",
                        what: "integration type",
                        example: "integration_types = [GuildInstall, UserInstall]",
                    },
                    &IntegrationType::ALL,
                    IntegrationType::name,
                )?;
                self.integration_types = Some((span, list));
            }
            "nsfw" => {
                let tokens: TokenStream = value.unwrap_or_default().iter().cloned().collect();
                let lit = syn::parse2::<syn::LitBool>(tokens)
                    .map_err(|_| syn::Error::new(span, NSFW_BOOL))?;
                self.nsfw = Some(lit.value);
            }
            _ => self.guilds = Some(guilds(span, value)?),
        }
        Ok(())
    }

    /// META-03: `contexts` and `integration_types` on a guild-scoped
    /// declaration (`guilds = [..]`), each spanned on its argument name.
    pub(crate) fn meta03(&self) -> Vec<syn::Error> {
        let Some(first) = self.guilds.as_ref().and_then(|g| g.first()) else {
            return Vec::new();
        };
        let mut meta = CommandMeta::new();
        if let Some((_, contexts)) = &self.contexts {
            meta = meta.contexts(contexts);
        }
        if let Some((_, types)) = &self.integration_types {
            meta = meta.integration_types(types);
        }
        validate_meta(&meta, Scope::Guild(*first), &Limits::default())
            .into_iter()
            .map(|v| {
                let span = match v.path.as_str() {
                    "contexts" => self.contexts.as_ref().map(|(s, _)| *s),
                    _ => self.integration_types.as_ref().map(|(s, _)| *s),
                };
                syn::Error::new(span.unwrap_or_else(Span::call_site), v.to_string())
            })
            .collect()
    }

    /// The `const` block building the `CommandMetadata`, setters in field
    /// order.
    pub(crate) fn tokens(&self) -> TokenStream {
        let core = quote!(::noctis::__private::core);
        let mut setters = TokenStream::new();
        if let Some(bits) = self.permissions {
            setters.extend(quote! {
                .default_member_permissions(#core::Permissions::from_bits_retain(#bits))
            });
        }
        if let Some((_, contexts)) = &self.contexts {
            let items = contexts.iter().map(|c| format_ident!("{}", c.name()));
            setters.extend(quote! {
                .contexts(&[#(::noctis::__private::rules::command::InteractionContext::#items),*])
            });
        }
        if let Some((_, types)) = &self.integration_types {
            let items = types.iter().map(|t| format_ident!("{}", t.name()));
            setters.extend(quote! {
                .integration_types(&[#(::noctis::__private::rules::command::IntegrationType::#items),*])
            });
        }
        if let Some(nsfw) = self.nsfw {
            setters.extend(quote!(.nsfw(#nsfw)));
        }
        if let Some(guilds) = &self.guilds {
            setters.extend(quote! {
                .guilds(&[#(::noctis::__private::model::id::Id::new(#guilds)),*])
            });
        }
        quote! {
            {
                const M: #core::CommandMetadata = #core::CommandMetadata::new() #setters;
                M
            }
        }
    }
}

/// The last identifier of a path (`a::b::C`, `::C`, `C`) starting at
/// `tokens[*i]`; advances `i` past it.
fn path_last(tokens: &[TokenTree], i: &mut usize) -> Option<proc_macro2::Ident> {
    let colons = |i: usize| {
        matches!((tokens.get(i), tokens.get(i + 1)),
            (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
                if a.as_char() == ':' && b.as_char() == ':')
    };
    if colons(*i) {
        *i += 2;
    }
    loop {
        let Some(TokenTree::Ident(ident)) = tokens.get(*i) else {
            return None;
        };
        *i += 1;
        if colons(*i) {
            *i += 2;
        } else {
            return Some(ident.clone());
        }
    }
}

/// `A | B | C` (names or paths) as a permission bit set: the syntax first,
/// then each name (META-01).
pub(crate) fn permissions(span: Span, tokens: &[TokenTree]) -> syn::Result<u64> {
    let syntax = || syn::Error::new(span, PERMISSIONS_SYNTAX);
    let mut idents = Vec::new();
    let mut i = 0;
    loop {
        idents.push(path_last(tokens, &mut i).ok_or_else(syntax)?);
        match tokens.get(i) {
            None => break,
            Some(TokenTree::Punct(p)) if p.as_char() == '|' && i + 1 < tokens.len() => i += 1,
            Some(_) => return Err(syntax()),
        }
    }
    let mut bits = 0;
    for ident in idents {
        let name = ident.to_string();
        let Some(bit) = permission(&name) else {
            let v = Violation::new(
                RuleId::META_01,
                "permissions",
                Bound::Describe("a permission name of noctis::Permissions"),
                Actual::Text(name.clone()),
            );
            let upper = name.to_ascii_uppercase();
            let v = if upper != name && permission(&upper).is_some() {
                v.with_help(format!("use `{upper}`"))
            } else {
                v.with_help("see noctis::rules::command::PERMISSIONS for every name")
            };
            return Err(syn::Error::new(ident.span(), v.to_string()));
        };
        bits |= bit;
    }
    Ok(bits)
}

/// The wording of one list argument.
struct Kind {
    arg: &'static str,
    what: &'static str,
    example: &'static str,
}

/// `[A, B]`: names from `all`, at least one, no duplicates, declared order.
fn names<T: Copy + PartialEq>(
    span: Span,
    value: Option<&[TokenTree]>,
    kind: &Kind,
    all: &[T],
    name_of: fn(T) -> &'static str,
) -> syn::Result<Vec<T>> {
    let not_list = || {
        syn::Error::new(
            span,
            format!("`{}` takes a list such as `{}`", kind.arg, kind.example),
        )
    };
    let items = list(value).ok_or_else(not_list)?;
    if items.is_empty() {
        return Err(syn::Error::new(
            span,
            format!("`{}` lists at least one {}", kind.arg, kind.what),
        ));
    }
    let mut out = Vec::new();
    for item in items {
        let mut i = 0;
        let ident = path_last(&item, &mut i)
            .filter(|_| i == item.len())
            .ok_or_else(not_list)?;
        let name = ident.to_string();
        let Some(value) = all.iter().copied().find(|v| name_of(*v) == name) else {
            let expected: Vec<&str> = all.iter().map(|v| name_of(*v)).collect();
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "unknown {} `{name}`; expected one of {}",
                    kind.what,
                    expected.join(", ")
                ),
            ));
        };
        if out.contains(&value) {
            return Err(syn::Error::new(
                ident.span(),
                format!("duplicate {} `{name}`", kind.what),
            ));
        }
        out.push(value);
    }
    Ok(out)
}

/// The comma-separated items of a `[..]` value (a trailing comma allowed),
/// or `None` when the value is not one bracketed list.
fn list(value: Option<&[TokenTree]>) -> Option<Vec<Vec<TokenTree>>> {
    let [TokenTree::Group(group)] = value? else {
        return None;
    };
    if group.delimiter() != Delimiter::Bracket {
        return None;
    }
    let mut items: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tt in group.stream() {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => items.push(Vec::new()),
            _ => items.last_mut()?.push(tt),
        }
    }
    if items.last().is_some_and(Vec::is_empty) {
        items.pop();
    }
    if items.iter().any(Vec::is_empty) {
        return None;
    }
    Some(items)
}

/// `[1, 2]`: non-zero, distinct `u64` literals (unsuffixed or `u64`).
fn guilds(span: Span, value: Option<&[TokenTree]>) -> syn::Result<Vec<u64>> {
    let not_list = || syn::Error::new(span, GUILDS_LIST);
    let items = list(value).ok_or_else(not_list)?;
    if items.is_empty() {
        return Err(syn::Error::new(span, GUILDS_EMPTY));
    }
    let mut out = Vec::new();
    for item in items {
        let tokens: TokenStream = item.into_iter().collect();
        let lit = syn::parse2::<LitInt>(tokens).map_err(|_| not_list())?;
        if !matches!(lit.suffix(), "" | "u64") {
            return Err(not_list());
        }
        let id = lit.base10_parse::<u64>().map_err(|_| not_list())?;
        if id == 0 {
            return Err(syn::Error::new(lit.span(), GUILD_ZERO));
        }
        if out.contains(&id) {
            return Err(syn::Error::new(
                lit.span(),
                format!("duplicate guild id `{id}`"),
            ));
        }
        out.push(id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use noctis_rules::command::PERMISSIONS;

    use super::*;

    fn all_permission_names() -> String {
        PERMISSIONS
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn segment(tokens: TokenStream) -> Vec<TokenTree> {
        tokens.into_iter().collect()
    }

    fn parse(tokens: TokenStream) -> syn::Result<MetaArgs> {
        let seg = segment(tokens);
        let name = match seg.first() {
            Some(TokenTree::Ident(i)) => i.to_string(),
            _ => String::new(),
        };
        let mut args = MetaArgs::default();
        args.parse(&name, &seg)
            .unwrap_or_else(|| panic!("{name} is not metadata"))?;
        Ok(args)
    }

    fn err(tokens: TokenStream) -> String {
        match parse(tokens) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    fn compact(tokens: &TokenStream) -> String {
        tokens.to_string().replace(' ', "")
    }

    /// Matrix 1 and 3-6: permissions.
    #[test]
    fn meta_01_permissions() {
        assert_eq!(
            parse(quote!(permissions = ADMINISTRATOR))
                .unwrap()
                .permissions,
            Some(8)
        );
        assert_eq!(
            parse(quote!(permissions = MANAGE_GUILD | BAN_MEMBERS))
                .unwrap()
                .permissions,
            Some(36)
        );
        assert_eq!(
            parse(quote!(
                permissions = Permissions::MANAGE_GUILD | noctis::Permissions::KICK_MEMBERS
            ))
            .unwrap()
            .permissions,
            Some(34)
        );
        assert_eq!(
            parse(quote!(permissions = ::noctis::Permissions::MANAGE_GUILD))
                .unwrap()
                .permissions,
            Some(32)
        );
        assert_eq!(
            parse(quote!(permissions = MANAGE_GUILD | MANAGE_GUILD))
                .unwrap()
                .permissions,
            Some(32)
        );
        let all: TokenStream = all_permission_names().parse().unwrap();
        let expected = PERMISSIONS.iter().fold(0, |acc, (_, b)| acc | b);
        assert_eq!(
            parse(quote!(permissions = #all)).unwrap().permissions,
            Some(expected)
        );
    }

    /// Matrix 7-9: unknown names.
    #[test]
    fn meta_01_unknown_names() {
        assert_eq!(
            err(quote!(permissions = MANAGE_SERVER)),
            "[META-01] permissions: limit a permission name of noctis::Permissions, actual `MANAGE_SERVER`\n  = help: see noctis::rules::command::PERMISSIONS for every name"
        );
        assert_eq!(
            err(quote!(permissions = BAN_MEMBERS | manage_guild)),
            "[META-01] permissions: limit a permission name of noctis::Permissions, actual `manage_guild`\n  = help: use `MANAGE_GUILD`"
        );
        assert!(err(quote!(permissions = MANAGE_EMOJIS_AND_STICKERS)).ends_with("for every name"));
    }

    /// Matrix 10: syntax.
    #[test]
    fn meta_01_syntax() {
        for tokens in [
            quote!(permissions = 8),
            quote!(permissions = "MANAGE_GUILD"),
            quote!(permissions = A + B),
            quote!(permissions = A || B),
            quote!(permissions = | A),
            quote!(permissions = A |),
            quote!(permissions =),
            quote!(permissions),
            quote!(permissions = A::<u8>),
        ] {
            assert_eq!(err(tokens.clone()), PERMISSIONS_SYNTAX, "{tokens}");
        }
    }

    /// Matrix 11: the per-path form.
    #[test]
    fn meta_02_per_path_form() {
        for name in NAMES {
            let ident = format_ident!("{name}");
            assert_eq!(
                err(quote!(#ident(set = MANAGE_GUILD))),
                format!(
                    "[META-02] `{name}` applies to the base command of every path this handler declares; Discord keeps it only on the base command, so it has no per-path form"
                )
            );
        }
    }

    /// Matrix 12-14: contexts and integration types.
    #[test]
    fn meta_03_lists() {
        let args = parse(quote!(contexts = [Guild, BotDm, PrivateChannel,])).unwrap();
        assert_eq!(
            args.contexts.map(|(_, c)| c),
            Some(vec![
                InteractionContext::Guild,
                InteractionContext::BotDm,
                InteractionContext::PrivateChannel
            ])
        );
        let args = parse(quote!(contexts = [InteractionContext::BotDm])).unwrap();
        assert_eq!(
            args.contexts.map(|(_, c)| c),
            Some(vec![InteractionContext::BotDm])
        );
        let args = parse(quote!(integration_types = [UserInstall, GuildInstall])).unwrap();
        assert_eq!(
            args.integration_types.map(|(_, t)| t),
            Some(vec![
                IntegrationType::UserInstall,
                IntegrationType::GuildInstall
            ])
        );
        for (tokens, expected) in [
            (
                quote!(contexts = Guild),
                "`contexts` takes a list such as `contexts = [Guild, BotDm, PrivateChannel]`",
            ),
            (
                quote!(contexts),
                "`contexts` takes a list such as `contexts = [Guild, BotDm, PrivateChannel]`",
            ),
            (
                quote!(contexts = [1]),
                "`contexts` takes a list such as `contexts = [Guild, BotDm, PrivateChannel]`",
            ),
            (
                quote!(contexts = [Guild,, BotDm]),
                "`contexts` takes a list such as `contexts = [Guild, BotDm, PrivateChannel]`",
            ),
            (
                quote!(contexts = []),
                "`contexts` lists at least one context",
            ),
            (
                quote!(contexts = [Gild]),
                "unknown context `Gild`; expected one of Guild, BotDm, PrivateChannel",
            ),
            (
                quote!(contexts = [Guild, Guild]),
                "duplicate context `Guild`",
            ),
            (
                quote!(integration_types = GuildInstall),
                "`integration_types` takes a list such as `integration_types = [GuildInstall, UserInstall]`",
            ),
            (
                quote!(integration_types = []),
                "`integration_types` lists at least one integration type",
            ),
            (
                quote!(integration_types = [Guild]),
                "unknown integration type `Guild`; expected one of GuildInstall, UserInstall",
            ),
            (
                quote!(integration_types = [UserInstall, UserInstall]),
                "duplicate integration type `UserInstall`",
            ),
        ] {
            assert_eq!(err(tokens.clone()), expected, "{tokens}");
        }
    }

    /// Matrix 15 and 16: nsfw.
    #[test]
    fn cmd_07_nsfw() {
        assert_eq!(parse(quote!(nsfw = true)).unwrap().nsfw, Some(true));
        assert_eq!(parse(quote!(nsfw = false)).unwrap().nsfw, Some(false));
        for tokens in [
            quote!(nsfw = 1),
            quote!(nsfw = "true"),
            quote!(nsfw),
            quote!(nsfw = yes),
        ] {
            assert_eq!(err(tokens.clone()), NSFW_BOOL, "{tokens}");
        }
    }

    /// Matrix 17-19: guilds.
    #[test]
    fn meta_03_guilds() {
        let args = parse(quote!(guilds = [123456789012345678, 42u64])).unwrap();
        assert_eq!(args.guilds, Some(vec![123_456_789_012_345_678, 42]));
        let max: TokenStream = "guilds = [18446744073709551615]".parse().unwrap();
        assert_eq!(parse(max).unwrap().guilds, Some(vec![u64::MAX]));
        let overflow: TokenStream = "guilds = [18446744073709551616]".parse().unwrap();
        for (tokens, expected) in [
            (quote!(guilds = 1), GUILDS_LIST),
            (quote!(guilds), GUILDS_LIST),
            (quote!(guilds = []), GUILDS_EMPTY),
            (quote!(guilds = [0]), GUILD_ZERO),
            (quote!(guilds = [-1]), GUILDS_LIST),
            (overflow, GUILDS_LIST),
            (quote!(guilds = [DEV]), GUILDS_LIST),
            (quote!(guilds = [1u8]), GUILDS_LIST),
            (quote!(guilds = [1, 1]), "duplicate guild id `1`"),
        ] {
            assert_eq!(err(tokens.clone()), expected, "{tokens}");
        }
    }

    /// Matrix 24: `dm_permission`.
    #[test]
    fn meta_05_dm_permission() {
        for tokens in [
            quote!(dm_permission = false),
            quote!(dm_permission),
            quote!(dm_permission(x)),
        ] {
            assert_eq!(err(tokens.clone()), DM_PERMISSION, "{tokens}");
        }
        assert!(is_meta("dm_permission"));
        assert!(!is_meta("description"));
    }

    fn parse_all(segments: &[TokenStream]) -> MetaArgs {
        let mut args = MetaArgs::default();
        for tokens in segments {
            let seg = segment(tokens.clone());
            let Some(TokenTree::Ident(name)) = seg.first() else {
                panic!("{tokens}");
            };
            args.parse(&name.to_string(), &seg)
                .unwrap_or_else(|| panic!("{tokens}"))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        args
    }

    /// Matrix 20-22: META-03.
    #[test]
    fn meta_03_guild_scope() {
        let args = parse_all(&[quote!(guilds = [42]), quote!(contexts = [Guild])]);
        let errs: Vec<String> = args.meta03().iter().map(ToString::to_string).collect();
        assert_eq!(
            errs,
            [
                "[META-03] contexts: limit only on global commands, actual `[Guild]`\n  = note: scope: guild 42"
            ]
        );
        let args = parse_all(&[
            quote!(integration_types = [UserInstall]),
            quote!(guilds = [7, 8]),
            quote!(contexts = [BotDm, Guild]),
        ]);
        let errs: Vec<String> = args.meta03().iter().map(ToString::to_string).collect();
        assert_eq!(
            errs,
            [
                "[META-03] contexts: limit only on global commands, actual `[Guild, BotDm]`\n  = note: scope: guild 7",
                "[META-03] integration_types: limit only on global commands, actual `[UserInstall]`\n  = note: scope: guild 7",
            ]
        );
        let args = parse_all(&[
            quote!(guilds = [42]),
            quote!(permissions = MANAGE_GUILD),
            quote!(nsfw = true),
        ]);
        assert!(args.meta03().is_empty());
        let args = parse_all(&[quote!(contexts = [Guild])]);
        assert!(args.meta03().is_empty());
    }

    /// Matrix 2 and the generated `const` block, setters in field order.
    #[test]
    fn tokens_in_field_order() {
        assert!(MetaArgs::default().is_empty());
        let args = parse_all(&[
            quote!(guilds = [42]),
            quote!(nsfw = false),
            quote!(permissions = MANAGE_GUILD | ADMINISTRATOR),
        ]);
        assert!(!args.is_empty());
        assert_eq!(
            compact(&args.tokens()),
            "{constM:::noctis::__private::core::CommandMetadata=::noctis::__private::core::CommandMetadata::new()\
             .default_member_permissions(::noctis::__private::core::Permissions::from_bits_retain(40u64))\
             .nsfw(false)\
             .guilds(&[::noctis::__private::model::id::Id::new(42u64)]);M}"
        );
        let args = parse_all(&[
            quote!(integration_types = [GuildInstall]),
            quote!(contexts = [Guild, BotDm]),
        ]);
        assert_eq!(
            compact(&args.tokens()),
            "{constM:::noctis::__private::core::CommandMetadata=::noctis::__private::core::CommandMetadata::new()\
             .contexts(&[::noctis::__private::rules::command::InteractionContext::Guild,::noctis::__private::rules::command::InteractionContext::BotDm])\
             .integration_types(&[::noctis::__private::rules::command::IntegrationType::GuildInstall]);M}"
        );
    }
}
