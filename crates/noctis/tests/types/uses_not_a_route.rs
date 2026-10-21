use noctis::prelude::*;

// Story 7.1 Matrix 10: `Module::uses::<R>()` needs a handled Route.
fn main() {
    let _ = Module::new("dash").uses::<String>();
}
