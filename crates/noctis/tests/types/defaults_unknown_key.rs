use noctis::prelude::*;

// Story 6.11 Matrix 35-37, 39 and 42: unknown, duplicate and non-literal
// keys, styles, emoji and channel types.
#[derive(Button)]
#[button("a", defaults(style = Primary, colour = Red))]
struct Colour;

#[derive(UserSelectMenu)]
#[user_select("b", defaults(placeholder = "p", options = Kinds))]
struct UserOptions;

#[derive(RoleSelectMenu)]
#[role_select("c", defaults(placeholder = "p", channel_types = [GuildText]))]
struct RoleChannels;

#[derive(StringSelectMenu)]
#[string_select("d", defaults(placeholder = "p", label = "L"))]
struct SelectLabel;

#[derive(Button)]
#[button("e", defaults(style = Primary, label = "a", label = "b"))]
struct Twice;

const LABEL: &str = "x";

#[derive(Button)]
#[button("f", defaults(style = Primary, label = LABEL))]
struct NotLiteral;

#[derive(UserSelectMenu)]
#[user_select("g", defaults(placeholder = "p", disabled = "yes"))]
struct NotBool;

#[derive(Button)]
#[button("h", defaults(style = Link, label = "Docs"))]
struct LinkStyle;

#[derive(Button)]
#[button("i", defaults(style = Blurple, label = "B"))]
struct Blurple;

#[derive(Button)]
#[button("j", defaults(style = Primary, emoji = "<:x:abc>"))]
struct BadEmoji;

#[derive(ChannelSelectMenu)]
#[channel_select("k", defaults(placeholder = "p", channel_types = [Bogus]))]
struct BadChannel;

fn main() {}
