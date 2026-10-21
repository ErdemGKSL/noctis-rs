//! The withdrawn authoring forms (maintainer decisions §7, §8): free-function
//! `#[command]`, `#[user_command]`, `#[message_command]` and `#[event]`
//! handlers, `#[derive(Options)]` and `#[derive(Route)]`. Each stub fails
//! with a `= help:` showing the struct form. A kind derive's helper attribute
//! of the same name shadows the stub, so `#[command(..)]` after
//! `#[derive(SlashCommand)]` is the helper and never reaches this module.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;

/// A withdrawn attribute macro.
#[derive(Clone, Copy)]
pub(crate) enum Attr {
    Command,
    UserCommand,
    MessageCommand,
    Event,
}

impl Attr {
    fn name(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::UserCommand => "user_command",
            Self::MessageCommand => "message_command",
            Self::Event => "event",
        }
    }

    fn derive(self) -> &'static str {
        match self {
            Self::Command => "SlashCommand",
            Self::UserCommand => "UserCommand",
            Self::MessageCommand => "MessageCommand",
            Self::Event => "EventHandler",
        }
    }

    fn struct_form(self) -> &'static str {
        match self {
            Self::Command => {
                "#[derive(SlashCommand)] #[command(\"ping\", ..)] struct Ping; impl Ping { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { .. } }"
            }
            Self::UserCommand => {
                "#[derive(UserCommand)] #[user_command(\"Report\", ..)] struct Report; impl Report { #[handler] async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Responded> { .. } }"
            }
            Self::MessageCommand => {
                "#[derive(MessageCommand)] #[message_command(\"Quote\", ..)] struct Quote; impl Quote { #[handler] async fn handle(self, ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<Responded> { .. } }"
            }
            Self::Event => {
                "#[derive(EventHandler)] #[event(ordered)] struct AwardXp; impl AwardXp { #[handler] async fn handle(self, ctx: EventCtx, event: MessageCreate) -> HandlerResult<()> { .. } }"
            }
        }
    }
}

/// The message of a withdrawn attribute on `item`.
fn attr_message(attr: Attr, item: &TokenStream) -> (String, Span) {
    let name = attr.name();
    let derive = attr.derive();
    let help = attr.struct_form();
    match syn::parse2::<syn::Item>(item.clone()) {
        Ok(syn::Item::Struct(s)) => (
            format!(
                "`#[{name}(..)]` goes after `#[derive({derive})]`, which declares it\n  = help: {help}"
            ),
            s.ident.span(),
        ),
        Ok(syn::Item::Fn(f)) => (
            format!(
                "free-function `#[{name}]` handlers are withdrawn: one struct per interaction, with a `#[handler]` method\n  = help: {help}"
            ),
            f.sig.ident.span(),
        ),
        _ => (
            format!("`#[{name}]` is the attribute of `#[derive({derive})]`\n  = help: {help}"),
            item.span(),
        ),
    }
}

/// Expands a withdrawn attribute: its error, then the item unchanged.
pub(crate) fn attribute(attr: Attr, item: &TokenStream) -> TokenStream {
    let (message, span) = attr_message(attr, item);
    let error = syn::Error::new(span, message).into_compile_error();
    quote!(#error #item)
}

/// A withdrawn derive.
#[derive(Clone, Copy)]
pub(crate) enum Derive {
    Options,
    Route,
}

/// The error of a withdrawn derive.
pub(crate) fn derive(derive: Derive, input: &TokenStream) -> TokenStream {
    let (message, span) = match (derive, syn::parse2::<syn::DeriveInput>(input.clone())) {
        (d, Ok(i)) => (message(d), i.ident.span()),
        (d, Err(_)) => (message(d), Span::call_site()),
    };
    syn::Error::new(span, message).into_compile_error()
}

fn message(derive: Derive) -> String {
    match derive {
        Derive::Options => "`#[derive(Options)]` is withdrawn: a slash Command struct's named fields are its options\n  = help: #[derive(SlashCommand)] #[command(\"ban\", description = \"…\")] struct Ban { #[option(description = \"Who\")] user: ResolvedUser } impl Ban { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { .. } }".to_owned(),
        Derive::Route => "`#[derive(Route)]` and `#[route]` are withdrawn: derive the component kind, whose attribute carries the id, the codec arguments and `defaults(..)`\n  = help: #[derive(Button)] #[button(\"shop.buy\", defaults(style = Primary, label = \"Buy\"))] struct Buy { item: u64 } impl Buy { #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> { .. } }".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 6.11 Matrix 53.
    #[test]
    fn every_stub_shows_the_struct_form() {
        for attr in [
            Attr::Command,
            Attr::UserCommand,
            Attr::MessageCommand,
            Attr::Event,
        ] {
            let out = attribute(
                attr,
                &quote!(
                    async fn h(ctx: CommandCtx) {}
                ),
            )
            .to_string();
            assert!(out.contains("are withdrawn"), "{out}");
            assert!(out.contains("= help: #[derive("), "{out}");
            assert!(out.contains("async fn h"), "{out}");
            let out = attribute(
                attr,
                &quote!(
                    struct S;
                ),
            )
            .to_string();
            assert!(out.contains("goes after `#[derive("), "{out}");
        }
        for d in [Derive::Options, Derive::Route] {
            let out = derive(
                d,
                &quote!(
                    struct S;
                ),
            )
            .to_string();
            assert!(out.contains("= help: #[derive("), "{out}");
        }
    }
}
