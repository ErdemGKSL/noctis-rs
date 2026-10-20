//! Proc macros: `command!`, user/message commands, the `Route`/`Payload`/`Options`/`Choices`/`Modal` derives, `components!`, `view!`/`tw!` and `i18n!`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! Implemented so far: the `Route`, `Payload` and `PayloadEnum` derives, the
//! literal macros `label!`, `placeholder!`, `content!` and `rgb!`,
//! `components!`, the `Modal`, `Choices` and `Options` derives, and the
//! `#[event]`, `#[command]`, `#[user_command]`, `#[message_command]` and
//! `#[autocomplete]` attributes.
//!
//! Generated code names runtime items only through
//! `::noctis::__private::{codec, ui, core, rules, model}` and std items through `::core`/`::std`
//! (AD-2). Errors are `[RULE-ID] what is wrong` plus a `= help:` line (or a
//! `noctis_rules::Violation`'s `Display`), spanned on the offending token.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod autocomplete;
mod choices;
mod command;
mod common;
mod components;
mod event;
mod literal;
mod menu;
mod meta;
mod modal;
mod options;
mod payload;
mod payload_enum;
mod route;

/// Derives `Route` and `Payload` for a struct and proves at compile time
/// that every `custom_id` of the Route fits 100 characters (CID-01).
///
/// Takes one `#[route("id", args…)]` attribute; see `noctis::prelude::Route`.
#[proc_macro_derive(Route, attributes(route))]
pub fn derive_route(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    route::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Payload` and `PayloadDecode` for a struct, for a hand-written
/// `Route`.
#[proc_macro_derive(Payload)]
pub fn derive_payload(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    payload::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Field` and `FieldDecode` for a unit enum of at most 62 variants,
/// encoded as the declaration index in one base62 digit.
#[proc_macro_derive(PayloadEnum)]
pub fn derive_payload_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    payload_enum::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `ModalForm` for a struct with named fields: one field per
/// top-level modal component, every literal text and count checked at
/// expansion (MOD-01, MOD-02, MOD-04..07, CV2-04, CV2-09, CID-01).
///
/// Takes `#[modal(route = Route, title = "…")]` and one component attribute
/// per field; see `noctis::prelude::Modal`.
#[proc_macro_derive(
    Modal,
    attributes(
        modal,
        text_input,
        string_select,
        user_select,
        role_select,
        mentionable_select,
        channel_select,
        file_upload,
        radio_group,
        checkbox_group,
        checkbox,
        text_display
    )
)]
pub fn derive_modal(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    modal::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Options` for a struct with named fields: each field one
/// chat-input command option, its `#[option(..)]` literals checked at
/// expansion with `noctis_rules::command::validate_options` (CMD-01..04,
/// CMD-06, CMD-13, OPT-01, OPT-03..11, OPT-13); see `noctis::Options`.
#[proc_macro_derive(Options, attributes(option))]
pub fn derive_options(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    options::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Choices` and `CommandOption` (a STRING option with choices) for
/// an enum of unit variants, each with
/// `#[choice("Label", value = "v", description = "d")]`, checked at
/// expansion (OPT-09 for labels and values, MOD-07 for descriptions).
#[proc_macro_derive(Choices, attributes(choice))]
pub fn derive_choices(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    choices::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A button label literal, checked at expansion against CV2-08 (at most 80
/// characters by `discord_len`); expands to a `noctis::ui::Label`.
#[proc_macro]
pub fn label(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::LABEL, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A select placeholder literal, checked at expansion against CV2-09 (at
/// most 150 characters); expands to a `noctis::ui::Placeholder`.
#[proc_macro]
pub fn placeholder(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::PLACEHOLDER, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Legacy message content literal, checked at expansion against MSG-01 (at
/// most 2000 characters); expands to a `noctis::ui::Content`.
#[proc_macro]
pub fn content(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::CONTENT, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A `0xRRGGBB` colour literal, checked at expansion against CV2-13;
/// expands to a `noctis::ui::Rgb`.
#[proc_macro]
pub fn rgb(input: TokenStream) -> TokenStream {
    literal::expand_rgb(&input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A literal Components V2 tree, checked at expansion with
/// `noctis_rules::component::validate_components` (and CID-02); expands to
/// the typed builders, yielding a `noctis::ui::Components`.
#[proc_macro]
pub fn components(input: TokenStream) -> TokenStream {
    components::components(input.into()).into()
}

/// Turns `async fn name(ctx: EventCtx, event: <payload>) -> HandlerResult<()>`
/// into `fn name() -> EventHandler`, a descriptor for `Module::add`.
///
/// Takes `ordered` (run before the concurrent handlers, in per-guild order)
/// and `droppable` (the event may be dropped under load), in any order.
#[proc_macro_attribute]
pub fn event(args: TokenStream, item: TokenStream) -> TokenStream {
    event::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Checks a Command Path or Pattern at expansion (CMD-01..05, CMD-10..13,
/// CMD-15, CMD-16) and turns `async fn name(ctx: CommandCtx[, path:
/// <Fn>Path]) -> HandlerResult<()>` into `fn name() -> CommandHandler`, a
/// descriptor for `Module::add`.
///
/// Takes the path or pattern literal first, then `description = "…"`,
/// `max_expansions = n` and the metadata arguments `permissions`,
/// `contexts`, `integration_types`, `nsfw` and `guilds` (META-01..03,
/// CMD-07); a pattern with a group also generates the `<Fn>Path` enum. See
/// `noctis::command`.
#[proc_macro_attribute]
pub fn command(args: TokenStream, item: TokenStream) -> TokenStream {
    command::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Checks a `USER` context-menu name at expansion (CTX-01) and turns
/// `async fn name(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<()>`
/// into `fn name() -> ContextMenuHandler`, a descriptor for `Module::add`.
///
/// Takes the menu name literal first, then the metadata arguments. See
/// `noctis::user_command`.
#[proc_macro_attribute]
pub fn user_command(args: TokenStream, item: TokenStream) -> TokenStream {
    menu::expand(menu::Menu::User, args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Like [`user_command`] for a `MESSAGE` context-menu command, whose handler
/// takes `target: ResolvedMessage`. See `noctis::message_command`.
#[proc_macro_attribute]
pub fn message_command(args: TokenStream, item: TokenStream) -> TokenStream {
    menu::expand(menu::Menu::Message, args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[autocomplete]`: an autocomplete handler descriptor; see `noctis`.
#[proc_macro_attribute]
pub fn autocomplete(args: TokenStream, item: TokenStream) -> TokenStream {
    autocomplete::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
