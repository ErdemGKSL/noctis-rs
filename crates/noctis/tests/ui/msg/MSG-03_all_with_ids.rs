use noctis::rules::message::MentionIds;

fn main() {
    let _ = MentionIds::All(&[1_u64]); // [MSG-03]
}
