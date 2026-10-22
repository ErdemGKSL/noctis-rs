use noctis::prelude::*;

mod keys {
    pub const P: &str = "pick.who";
}

#[derive(UserSelectMenu)]
#[user_select("pick.who", defaults(placeholder = keys::P))]
struct Pick;

fn main() {}
