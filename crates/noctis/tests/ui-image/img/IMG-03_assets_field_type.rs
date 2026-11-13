// IMG-03: an `#[asset]` field of a `#[derive(Assets)]` struct follows the
// Template's rule: a remote image or another `Assets` struct.
use noctis::image::Assets;
use serde::{Deserialize, Serialize};

#[derive(Assets, Serialize, Deserialize)]
struct Row {
    #[asset]
    avatar: String,
}

#[derive(Assets)]
enum Kind {
    A,
}

fn main() {}
