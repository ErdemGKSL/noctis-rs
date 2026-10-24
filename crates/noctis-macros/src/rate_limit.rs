//! The `rate_limit(<scope>, <n>, per = "<dur>"[, manual])` argument of the
//! Command, context-menu, component and modal attributes (FR-46, INT-14):
//! parsed here once for all of them and emitted as the kind trait's
//! `RATE_LIMITS` const.

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Lit, LitInt, LitStr};

use crate::duration::{DurationError, parse_secs};

/// Any malformed `rate_limit` argument.
pub(crate) const SHAPE: &str =
    "[INT-14] `rate_limit` takes `(scope, count, per = \"…\"[, manual])`";
/// A count outside `1..=u32::MAX` or not an integer literal.
pub(crate) const COUNT: &str =
    "[INT-14] the rate-limit count must be an integer literal in 1..=4294967295";
/// A `per` value outside the duration grammar.
pub(crate) const PER: &str =
    "[INT-14] `per` takes a duration such as \"10s\", \"1m\" or \"1h30m\" (units d, h, m, s)";
/// No `per = ".."`.
pub(crate) const MISSING_PER: &str =
    "[INT-14] `rate_limit` needs a window: `rate_limit(user, 3, per = \"10s\")`";
/// `message` on a slash or user command.
pub(crate) const NO_MESSAGE: &str = "[INT-14] scope `message` needs a message: use it on #[message_command], a component or a modal";

/// The longest window: one day.
const MAX_WINDOW_SECS: u64 = 86_400;

/// A rate-limit scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    User,
    Member,
    Channel,
    Guild,
    Message,
}

impl Scope {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "user" => Self::User,
            "member" => Self::Member,
            "channel" => Self::Channel,
            "guild" => Self::Guild,
            "message" => Self::Message,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Member => "member",
            Self::Channel => "channel",
            Self::Guild => "guild",
            Self::Message => "message",
        }
    }

    fn variant(self) -> TokenStream {
        match self {
            Self::User => quote!(User),
            Self::Member => quote!(Member),
            Self::Channel => quote!(Channel),
            Self::Guild => quote!(Guild),
            Self::Message => quote!(Message),
        }
    }
}

/// One declared limit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Decl {
    pub(crate) scope: Scope,
    pub(crate) max_uses: u32,
    pub(crate) window_secs: u32,
    pub(crate) manual: bool,
    span: Span,
}

impl PartialEq for Decl {
    fn eq(&self, other: &Self) -> bool {
        (self.scope, self.max_uses, self.window_secs, self.manual)
            == (other.scope, other.max_uses, other.window_secs, other.manual)
    }
}

impl Eq for Decl {}

/// The declared limits of one attribute, in declared order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RateLimits {
    pub(crate) decls: Vec<Decl>,
}

fn error(span: Span, message: impl core::fmt::Display) -> syn::Error {
    syn::Error::new(span, message.to_string())
}

fn span_of(tokens: &[TokenTree], fallback: Span) -> Span {
    tokens.first().map_or(fallback, TokenTree::span)
}

/// Splits `tokens` on top-level commas, dropping one trailing empty segment.
fn split(tokens: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut out: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tt in tokens {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => out.push(Vec::new()),
            _ => {
                if let Some(last) = out.last_mut() {
                    last.push(tt);
                }
            }
        }
    }
    if out.len() > 1 && out.last().is_some_and(Vec::is_empty) {
        out.pop();
    }
    if out.len() == 1 && out.first().is_some_and(Vec::is_empty) {
        out.clear();
    }
    out
}

/// Whether `arg` is `per = …`.
fn is_per(arg: &[TokenTree]) -> bool {
    matches!(arg, [TokenTree::Ident(name), TokenTree::Punct(eq), ..] if name == "per" && eq.as_char() == '=')
}

/// The scope argument.
fn parse_scope(arg: &[TokenTree], span: Span) -> syn::Result<Scope> {
    let text = match arg {
        [TokenTree::Ident(ident)] => ident.to_string(),
        [TokenTree::Literal(lit)]
            if matches!(
                syn::parse2::<Lit>(TokenTree::Literal(lit.clone()).into()),
                Ok(Lit::Str(_))
            ) =>
        {
            lit.to_string()
        }
        _ => return Err(error(span_of(arg, span), SHAPE)),
    };
    Scope::parse(&text).ok_or_else(|| {
        error(
            span_of(arg, span),
            format!("[INT-14] unknown rate-limit scope `{text}`: expected `user`, `member`, `channel`, `guild` or `message`"),
        )
    })
}

/// The count argument.
fn parse_count(arg: &[TokenTree], span: Span) -> syn::Result<u32> {
    let span = span_of(arg, span);
    let [TokenTree::Literal(lit)] = arg else {
        return Err(error(span, COUNT));
    };
    let lit = syn::parse2::<LitInt>(TokenTree::Literal(lit.clone()).into())
        .map_err(|_| error(span, COUNT))?;
    if !lit.suffix().is_empty() {
        return Err(error(span, COUNT));
    }
    lit.base10_parse::<u64>()
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n >= 1)
        .ok_or_else(|| error(span, COUNT))
}

/// The window of `per = "<dur>"`, in seconds.
fn parse_per(arg: &[TokenTree], span: Span) -> syn::Result<u32> {
    let [_, _, value @ ..] = arg else {
        return Err(error(span_of(arg, span), SHAPE));
    };
    let value_span = span_of(value, span_of(arg, span));
    let lit = match value {
        [TokenTree::Literal(lit)] => {
            syn::parse2::<LitStr>(TokenTree::Literal(lit.clone()).into()).ok()
        }
        _ => None,
    }
    .ok_or_else(|| error(value_span, PER))?;
    let text = lit.value();
    let out_of_range = || {
        error(
            lit.span(),
            format!("[INT-14] the rate-limit window must be 1s..=1d; found \"{text}\""),
        )
    };
    match parse_secs(&text) {
        Err(DurationError::Syntax) => Err(error(lit.span(), PER)),
        Err(DurationError::OutOfRange) => Err(out_of_range()),
        Ok(secs) if secs > MAX_WINDOW_SECS => Err(out_of_range()),
        Ok(secs) => u32::try_from(secs).map_err(|_| out_of_range()),
    }
}

impl RateLimits {
    /// Parses one `rate_limit(..)` argument segment (its first token is the
    /// `rate_limit` ident) and appends it.
    pub(crate) fn parse(&mut self, segment: &[TokenTree]) -> syn::Result<()> {
        let span = span_of(segment, Span::call_site());
        let [_, TokenTree::Group(group)] = segment else {
            return Err(error(span, SHAPE));
        };
        if group.delimiter() != Delimiter::Parenthesis {
            return Err(error(span, SHAPE));
        }
        let args = split(group.stream());
        let inner = group.span();
        if args.len() < 2 || args.iter().any(Vec::is_empty) {
            return Err(error(inner, SHAPE));
        }
        if !args.iter().skip(2).any(|a| is_per(a)) {
            if args.len() == 2 {
                return Err(error(inner, MISSING_PER));
            }
            if args
                .iter()
                .skip(2)
                .all(|a| matches!(a.as_slice(), [TokenTree::Ident(i)] if i == "manual"))
            {
                return Err(error(inner, MISSING_PER));
            }
            return Err(error(inner, SHAPE));
        }
        let scope = parse_scope(&args[0], inner)?;
        let max_uses = parse_count(&args[1], inner)?;
        if !is_per(&args[2]) || args.len() > 4 {
            return Err(error(inner, SHAPE));
        }
        let window_secs = parse_per(&args[2], inner)?;
        let manual = match args.get(3).map(Vec::as_slice) {
            None => false,
            Some([TokenTree::Ident(ident)]) if ident == "manual" => true,
            Some(other) => return Err(error(span_of(other, inner), SHAPE)),
        };
        if self.decls.iter().any(|d| d.scope == scope) {
            return Err(error(
                span_of(&args[0], inner),
                format!(
                    "[INT-14] a second `rate_limit` for scope `{}`: declare each scope once",
                    scope.as_str()
                ),
            ));
        }
        self.decls.push(Decl {
            scope,
            max_uses,
            window_secs,
            manual,
            span: span_of(&args[0], inner),
        });
        Ok(())
    }

    /// The INT-14 error of a `message` scope on a slash or user command.
    pub(crate) fn reject_message(&self) -> syn::Result<()> {
        match self.decls.iter().find(|d| d.scope == Scope::Message) {
            Some(decl) => Err(error(decl.span, NO_MESSAGE)),
            None => Ok(()),
        }
    }

    /// `const RATE_LIMITS: &'static [RateLimit] = &[..];`, or nothing when
    /// no limit is declared (the trait's empty default).
    pub(crate) fn const_item(&self) -> TokenStream {
        if self.decls.is_empty() {
            return TokenStream::new();
        }
        let core = quote!(::noctis::__private::core);
        let entries = self.decls.iter().map(|d| {
            let scope = d.scope.variant();
            let (max, secs, manual) = (d.max_uses, d.window_secs, d.manual);
            quote!(#core::RateLimit::__declare(#core::RateScope::#scope, #max, #secs, #manual))
        });
        quote! {
            const RATE_LIMITS: &'static [#core::RateLimit] = &[#(#entries),*];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(src: &str) -> Result<RateLimits, String> {
        let tokens: TokenStream = src.parse().unwrap();
        let mut limits = RateLimits::default();
        for segment in split(tokens) {
            limits.parse(&segment).map_err(|e| e.to_string())?;
        }
        Ok(limits)
    }

    fn one(src: &str) -> Decl {
        let limits = parse_all(src).unwrap();
        assert_eq!(limits.decls.len(), 1, "{src}");
        limits.decls[0]
    }

    fn err(src: &str) -> String {
        parse_all(src).unwrap_err()
    }

    const WINDOW: &str = "[INT-14] the rate-limit window must be 1s..=1d; found";

    /// G1-G4.
    #[test]
    fn int_14_accepted_forms() {
        let d = one(r#"rate_limit(user, 3, per = "10s")"#);
        assert_eq!(
            (d.scope, d.max_uses, d.window_secs, d.manual),
            (Scope::User, 3, 10, false)
        );
        let two =
            parse_all(r#"rate_limit(user, 3, per = "10s"), rate_limit(guild, 100, per = "1m")"#)
                .unwrap();
        let scopes: Vec<_> = two
            .decls
            .iter()
            .map(|d| (d.scope, d.max_uses, d.window_secs))
            .collect();
        assert_eq!(scopes, [(Scope::User, 3, 10), (Scope::Guild, 100, 60)]);
        let d = one(r#"rate_limit(member, 1, per = "1d", manual)"#);
        assert_eq!(
            (d.scope, d.max_uses, d.window_secs, d.manual),
            (Scope::Member, 1, 86_400, true)
        );
        assert_eq!(one(r#"rate_limit(channel, 1, per = "1s")"#).window_secs, 1);
        assert_eq!(
            one(r#"rate_limit(message, 4294967295, per = "1h30m")"#).max_uses,
            u32::MAX
        );
        assert_eq!(one(r#"rate_limit(user, 3, per = "10s",)"#).max_uses, 3);
    }

    /// G5.
    #[test]
    fn int_14_count_errors() {
        for n in ["0", "4294967296", "-1", "3.0", "\"3\"", "3u8", "x"] {
            assert_eq!(
                err(&format!(r#"rate_limit(user, {n}, per = "1s")"#)),
                COUNT,
                "{n}"
            );
        }
    }

    /// G6, G7.
    #[test]
    fn int_14_window_range_errors() {
        for w in ["2d", "86401s", "1d1s", "0s", "366d"] {
            assert_eq!(
                err(&format!(r#"rate_limit(user, 3, per = "{w}")"#)),
                format!("{WINDOW} \"{w}\""),
                "{w}"
            );
        }
    }

    /// G8.
    #[test]
    fn int_14_window_syntax_errors() {
        for w in [
            r#""""#,
            r#""10""#,
            r#""10S""#,
            r#""1m1h""#,
            r#""1.5m""#,
            r#""10 s""#,
            "\"\u{0661}\u{0660}s\"",
            "10",
        ] {
            assert_eq!(err(&format!("rate_limit(user, 3, per = {w})")), PER, "{w}");
        }
    }

    /// G9-G12.
    #[test]
    fn int_14_shape_errors() {
        assert_eq!(err("rate_limit(user, 3)"), MISSING_PER);
        assert_eq!(err("rate_limit(user, 3, manual)"), MISSING_PER);
        for scope in ["users", "User", "\"user\""] {
            assert_eq!(
                err(&format!(r#"rate_limit({scope}, 3, per = "1s")"#)),
                format!(
                    "[INT-14] unknown rate-limit scope `{scope}`: expected `user`, `member`, `channel`, `guild` or `message`"
                )
            );
        }
        assert_eq!(
            err(r#"rate_limit(user, 3, per = "1s"), rate_limit(user, 5, per = "1m")"#),
            "[INT-14] a second `rate_limit` for scope `user`: declare each scope once"
        );
        for src in [
            r#"rate_limit(user, 3, per = "1s", manual, manual)"#,
            r#"rate_limit(user, 3, per = "1s", auto)"#,
            r#"rate_limit(user, 3, per = "1s", per = "2s")"#,
            r#"rate_limit(3, user, per = "1s")"#,
            "rate_limit()",
            "rate_limit = 3",
            "rate_limit",
            r#"rate_limit[user, 3, per = "1s"]"#,
            r#"rate_limit(user, 3, manual, per = "1s")"#,
        ] {
            assert_eq!(err(src), SHAPE, "{src}");
        }
    }

    /// G13.
    #[test]
    fn int_14_message_scope_rejection() {
        let limits = parse_all(r#"rate_limit(message, 3, per = "1s")"#).unwrap();
        assert_eq!(limits.reject_message().unwrap_err().to_string(), NO_MESSAGE);
        let limits = parse_all(r#"rate_limit(user, 3, per = "1s")"#).unwrap();
        assert!(limits.reject_message().is_ok());
    }

    #[test]
    fn int_14_const_item() {
        assert!(RateLimits::default().const_item().is_empty());
        let limits = parse_all(
            r#"rate_limit(user, 3, per = "10s"), rate_limit(guild, 1, per = "1m", manual)"#,
        )
        .unwrap();
        let out = limits.const_item().to_string().replace(' ', "");
        assert!(
            out.contains("RateLimit::__declare(::noctis::__private::core::RateScope::User,3u32,10u32,false),::noctis::__private::core::RateLimit::__declare(::noctis::__private::core::RateScope::Guild,1u32,60u32,true)"),
            "{out}"
        );
    }
}
