// TW-01: unknown Tailwind classes in literals are compile errors. trybuild
// compiles fixtures without a noctis-theme.css, so theme tokens fail too.
use noctis::image::{Classes, Node, tw, view};

fn main() {
    let _: Classes = tw!("flx");
    let _: Classes = tw!("bg-gradient-to-r");
    let _: Classes = tw!("bg-brand");
    let _: Node = view! { <div tw="flex w-FULL"></div> };
}
