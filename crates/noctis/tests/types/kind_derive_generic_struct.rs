use noctis::prelude::*;

// Story 6.11 Matrix 6: a kind derive takes a non-generic struct.
#[derive(Button)]
#[button("g", defaults(style = Primary, label = "G"))]
struct Generic<T> {
    t: T,
}

fn main() {}
