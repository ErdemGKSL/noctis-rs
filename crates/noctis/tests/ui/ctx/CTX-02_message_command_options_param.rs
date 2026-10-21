use noctis::prelude::*;

#[derive(MessageCommand)]
#[message_command("Report")]
struct Report {
    reason: Option<String>, // [CTX-02]
}

fn main() {}
