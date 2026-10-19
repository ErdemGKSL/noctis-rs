use noctis::prelude::*;

#[derive(Route)]
#[route("a", by_ref, signed)]
struct RefSigned {
    s: String,
}

fn main() {}
