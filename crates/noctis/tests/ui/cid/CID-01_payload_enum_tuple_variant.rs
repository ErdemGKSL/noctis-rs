use noctis::prelude::*;

#[derive(PayloadEnum)]
enum Kind {
    A,
    B(u8),
}

fn main() {}
