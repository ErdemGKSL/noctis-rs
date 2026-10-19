use noctis::codec::BoundedStr;
use noctis::prelude::*;

#[derive(Route)]
#[route("a")]
struct TooLong {
    t: BoundedStr<47>,
    n: u16,
}

fn main() {}
