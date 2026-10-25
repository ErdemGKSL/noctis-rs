use noctis::prelude::*;

#[derive(Hook)]
struct Up;

fn main() {
    let _ = Module::new("ops").add::<Up>();
}
