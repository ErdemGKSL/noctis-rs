use noctis::prelude::*;

#[derive(Choices, Clone, Copy)]
enum Severity {
    #[choice("Low")]
    Low,
}

fn suggest() {}

#[derive(Options)]
struct O {
    #[option(description = "d", autocomplete = suggest)]
    a: Severity,
}

fn main() {}
