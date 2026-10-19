use noctis::prelude::*;

#[derive(Route)]
#[route("g")]
struct Generic<T> {
    t: T,
}

fn main() {}
