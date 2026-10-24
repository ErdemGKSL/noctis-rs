//! Proc macros: the kind derives (`SlashCommand`, `UserCommand`,
//! `MessageCommand`, `EventHandler`, `Button`, the five select derives,
//! `Modal`), the `#[handler]`, `#[autocomplete(field)]`, `#[on_stale]` and
//! `#[on_expired]` method attributes,
//! the `Payload`/`PayloadEnum`/`Choices` derives, `components!`, `i18n!`,
//! the literal macros and, with the `image` feature, `#[derive(Template)]`,
//! `view!`, `tw!` and `theme!`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! Every interaction is one struct (maintainer decisions §7): a kind derive
//! names the kind, its one kind attribute carries the id and the routing
//! arguments (and, for components and modals, the mandatory `defaults(..)`),
//! and `impl X { #[handler] async fn handle(self, ctx, ..) }` is the handler.
//! The withdrawn free-function attributes and the `Options`/`Route` derives
//! are stubs that fail with the struct form (`withdrawn`).
//!
//! Generated code names runtime items only through
//! `::noctis::__private::{codec, ui, core, rules, model, ports, image}` and std items through `::core`/`::std`
//! (AD-2). Errors are `[RULE-ID] what is wrong` plus a `= help:` line (or a
//! `noctis_rules::Violation`'s `Display`), spanned on the offending token.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod autocomplete;
mod choices;
mod command;
mod common;
mod component;
mod components;
mod defaults;
mod duration;
mod event;
mod flags;
mod guard;
mod handler;
mod hook;
mod i18n;
mod kind;
mod literal;
mod menu;
mod meta;
mod modal;
mod options;
mod payload;
mod payload_enum;
mod route;
#[cfg(feature = "image")]
mod template;
#[cfg(feature = "image")]
mod theme;
#[cfg(feature = "image")]
mod tw;
#[cfg(feature = "image")]
mod view;
mod withdrawn;

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

/// Derives a modal from a struct: `#[modal("id", <codec args>,
/// defaults(title = "…"))]`; fields without an input attribute are the
/// Payload (in the `custom_id`, CID-01), every other field is one input with
/// its literal texts and counts checked at expansion (MOD-01, MOD-02,
/// MOD-04..07, CV2-04, CV2-09). Generates `build`, `blank`, `ModalForm` and
/// the kind scaffolding; see `noctis::prelude::Modal`.
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
    modal::expand_or_fallback(&input).into()
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

/// An alt text literal, checked at expansion against ATT-06 (at most 1024
/// characters by `discord_len`); expands to a `noctis::ui::AltText`.
#[proc_macro]
pub fn alt(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::ALT, &input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A webhook name literal, checked at expansion against WH-04 (1 to 80
/// characters by `discord_len`, no `clyde` or `discord` in any ASCII case);
/// expands to a `noctis::ui::WebhookName`.
#[proc_macro]
pub fn webhook_name(input: TokenStream) -> TokenStream {
    literal::expand_text(&literal::WEBHOOK_NAME, &input.into())
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

/// `i18n!(default = "en-US", dir = "locales")`: typed content strings from
/// the YAML locale files of `dir` (relative to the crate root), checked at
/// expansion (LOC-01, LOC-04); see `noctis::i18n!`.
#[proc_macro]
pub fn i18n(input: TokenStream) -> TokenStream {
    i18n::expand(input.into()).into()
}

/// `#[derive(SlashCommand)]`: a slash Command struct whose named fields are
/// its options. Takes `#[command("path or pattern", description = "…", ..)]`
/// (CMD-01..05, CMD-10..13, CMD-15, CMD-16, META-01..03) and
/// `#[option(..)]` per field (OPT-*); a Pattern with a group generates
/// `<Struct>Path`. See `noctis::prelude::SlashCommand`.
#[proc_macro_derive(SlashCommand, attributes(command, option))]
pub fn derive_slash_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    command::derive(&input).into()
}

/// `#[derive(UserCommand)]` on a unit struct with `#[user_command("Name",
/// ..)]` (CTX-01..03); its handler takes `target: ResolvedUser`.
#[proc_macro_derive(UserCommand, attributes(user_command))]
pub fn derive_user_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    menu::derive_or_fallback(menu::Menu::User, &input).into()
}

/// `#[derive(MessageCommand)]` on a unit struct with
/// `#[message_command("Name", ..)]`; its handler takes `target:
/// ResolvedMessage`.
#[proc_macro_derive(MessageCommand, attributes(message_command))]
pub fn derive_message_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    menu::derive_or_fallback(menu::Menu::Message, &input).into()
}

/// `#[derive(EventHandler)]` on a unit struct, with an optional
/// `#[event(ordered, droppable)]`.
#[proc_macro_derive(EventHandler, attributes(event))]
pub fn derive_event_handler(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    event::derive_or_fallback(&input).into()
}

macro_rules! component_derives {
    ($($name:ident, $attr:ident, $fn:ident, $variant:ident, $doc:literal;)*) => {$(
        #[doc = $doc]
        #[proc_macro_derive($name, attributes($attr))]
        pub fn $fn(input: TokenStream) -> TokenStream {
            let input = parse_macro_input!(input as DeriveInput);
            component::derive_or_fallback(component::Component::$variant, &input).into()
        }
    )*};
}

component_derives! {
    Button, button, derive_button, Button,
        "`#[derive(Button)]` with `#[button(\"id\", <codec args>, defaults(style = .., label = ..))]`: an interactive button Route (CID-01, CID-03, CID-04, CV2-08).";
    StringSelectMenu, string_select, derive_string_select, StringSelect,
        "`#[derive(StringSelectMenu)]` with `#[string_select(\"id\", defaults(placeholder = .., options = ..))]` (CV2-09).";
    UserSelectMenu, user_select, derive_user_select, UserSelect,
        "`#[derive(UserSelectMenu)]` with `#[user_select(\"id\", defaults(placeholder = ..))]` (CV2-09).";
    RoleSelectMenu, role_select, derive_role_select, RoleSelect,
        "`#[derive(RoleSelectMenu)]` with `#[role_select(\"id\", defaults(placeholder = ..))]` (CV2-09).";
    MentionableSelectMenu, mentionable_select, derive_mentionable_select, MentionableSelect,
        "`#[derive(MentionableSelectMenu)]` with `#[mentionable_select(\"id\", defaults(placeholder = ..))]` (CV2-09).";
    ChannelSelectMenu, channel_select, derive_channel_select, ChannelSelect,
        "`#[derive(ChannelSelectMenu)]` with `#[channel_select(\"id\", defaults(placeholder = .., channel_types = [..]))]` (CV2-09).";
}

/// `#[handler]`: the one `async` method (receiver `self` by value) of a
/// kind-derived struct that handles its interaction; emits the public
/// `fn descriptor()` beside it.
#[proc_macro_attribute]
pub fn handler(args: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand_or_fallback(args.into(), item.into()).into()
}

/// `#[autocomplete(field)]`: the suggestion method of the option `field`
/// flagged `#[option(.., autocomplete)]` (OPT-11, INT-06).
#[proc_macro_attribute]
pub fn autocomplete(args: TokenStream, item: TokenStream) -> TokenStream {
    autocomplete::expand_or_fallback(args.into(), item.into()).into()
}

/// `#[on_stale]`: the method of a component or modal struct that answers
/// a stale `custom_id` (another `schema`, body kind or flag set of the
/// Route): `async fn <any>(ctx[, r: <Kind>Responder<..>])`, no `self`.
#[proc_macro_attribute]
pub fn on_stale(args: TokenStream, item: TokenStream) -> TokenStream {
    hook::expand_or_fallback(hook::Hook::Stale, args.into(), item.into()).into()
}

/// `#[on_expired]`: the method of a component or modal struct that answers
/// a missing Ref or, without `#[on_stale]`, a stale `custom_id`:
/// `async fn <any>(ctx, expired: Expired[, r: <Kind>Responder<..>])`, no
/// `self`.
#[proc_macro_attribute]
pub fn on_expired(args: TokenStream, item: TokenStream) -> TokenStream {
    hook::expand_or_fallback(hook::Hook::Expired, args.into(), item.into()).into()
}

/// `#[derive(Template)]` with `#[template(id = "..", version = N, size = (w,
/// h), format = ..[, animation(duration_ms = .., fps = ..[, format = ..])])]`:
/// implements `noctis::image::Template` (AD-22). Keys, forms and ranges are
/// checked at expansion (an animated format in `format` is ATT-07); the
/// animated `format` defaults to `Webp { quality: 80 }`, and `Gif`/`Apng`
/// compile only with the `image-gif` feature. Generic structs,
/// `HashMap`/`HashSet` fields and `#[asset]` fields are errors.
#[cfg(feature = "image")]
#[proc_macro_derive(Template, attributes(template, asset))]
pub fn derive_template(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    template::derive(&input).into()
}

/// `view! { <div tw="..">..</div> }`: one root element lowered to the
/// `noctis::image::Node` builders; every `tw = "…"` literal is checked
/// against the Tailwind grammar and the crate's `noctis-theme.css` (TW-01).
#[cfg(feature = "image")]
#[proc_macro]
pub fn view(input: TokenStream) -> TokenStream {
    view::expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `tw!("classes")`: a checked `noctis::image::Classes` const expression,
/// validated against the Tailwind grammar and the crate's
/// `noctis-theme.css` (TW-01).
#[cfg(feature = "image")]
#[proc_macro]
pub fn tw(input: TokenStream) -> TokenStream {
    tw::expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `theme!()` / `theme!("path")`: embeds the crate's `noctis-theme.css` (or
/// the file at `path`, relative to the calling file) as a
/// `noctis::image::Theme`, validated at expansion (TW-02).
#[cfg(feature = "image")]
#[proc_macro]
pub fn theme(input: TokenStream) -> TokenStream {
    theme::expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Withdrawn: `#[command]` on a free function. Fails with the
/// `#[derive(SlashCommand)]` form.
#[proc_macro_attribute]
pub fn command(_: TokenStream, item: TokenStream) -> TokenStream {
    withdrawn::attribute(withdrawn::Attr::Command, &item.into()).into()
}

/// Withdrawn: `#[user_command]` on a free function.
#[proc_macro_attribute]
pub fn user_command(_: TokenStream, item: TokenStream) -> TokenStream {
    withdrawn::attribute(withdrawn::Attr::UserCommand, &item.into()).into()
}

/// Withdrawn: `#[message_command]` on a free function.
#[proc_macro_attribute]
pub fn message_command(_: TokenStream, item: TokenStream) -> TokenStream {
    withdrawn::attribute(withdrawn::Attr::MessageCommand, &item.into()).into()
}

/// Withdrawn: `#[event]` on a free function.
#[proc_macro_attribute]
pub fn event(_: TokenStream, item: TokenStream) -> TokenStream {
    withdrawn::attribute(withdrawn::Attr::Event, &item.into()).into()
}

/// The withdrawn `#[derive(Options)]`.
#[proc_macro_derive(Options, attributes(option))]
pub fn derive_options(input: TokenStream) -> TokenStream {
    withdrawn::derive(withdrawn::Derive::Options, &input.into()).into()
}

/// The withdrawn `#[derive(Route)]`.
#[proc_macro_derive(Route, attributes(route))]
pub fn derive_route(input: TokenStream) -> TokenStream {
    withdrawn::derive(withdrawn::Derive::Route, &input.into()).into()
}
