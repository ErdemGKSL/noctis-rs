use noctis::prelude::*;

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

fn main() {
    let _ = Buy { item: 1 }.build();
}
