// TW-01: an animation the theme does not declare is a compile error naming
// the missing declaration. trybuild compiles fixtures without a
// noctis-theme.css, so neither `--animate-wiggle` nor `@keyframes wiggle`
// exists; built-in animations need no theme.
use noctis::image::{Classes, tw};

fn main() {
    let _: Classes = tw!("animate-spin animate-[spin_2s_linear]");
    let _: Classes = tw!("animate-wiggle");
    let _: Classes = tw!("animate-[wiggle_1s]");
}
