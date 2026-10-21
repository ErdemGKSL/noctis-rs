use noctis::prelude::*;

// Story 6.11 Matrix 33 and 34: `defaults(..)` is mandatory, with the kind's
// required keys.
#[derive(Button)]
#[button("a")]
struct NoBlock;

#[derive(Button)]
#[button("b", defaults())]
struct Empty;

#[derive(Button)]
#[button("c", defaults(label = "C"))]
struct NoStyle;

#[derive(Button)]
#[button("d", defaults(style = Primary))]
struct NoText;

#[derive(UserSelectMenu)]
#[user_select("e", defaults(min_values = 1))]
struct NoPlaceholder;

#[derive(Modal)]
#[modal("f")]
struct NoTitle {
    #[text_input("Text")]
    text: String,
}

fn main() {}
