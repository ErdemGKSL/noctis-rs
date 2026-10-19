use noctis::prelude::*;

#[derive(Route)]
#[route("shop.buy")]
struct Buy {
    item: u64,
}

fn main() {
    let _ = Buy { item: 1 }.button();
}
