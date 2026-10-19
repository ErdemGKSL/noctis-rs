use noctis::prelude::*;

#[derive(Route)]
#[route("a", ttl = "1m")]
struct Timed;

fn main() {}
