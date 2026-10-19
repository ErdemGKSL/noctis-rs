//! Message boundary passes (MSG-*).

use noctis::rules::Limits;
use noctis::rules::message::{AllowedMentions, MentionIds, validate_mentions};

#[test]
fn msg_03_typed_mentions_validate() {
    let roles = [1_u64; 100];
    let mentions = AllowedMentions::new()
        .users(MentionIds::All)
        .roles(MentionIds::Only(&roles));
    assert!(validate_mentions(&mentions, &Limits::default()).is_empty());
}
