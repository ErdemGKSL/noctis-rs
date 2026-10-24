use noctis::gateway::MemberRequest;
use noctis::twilight::model::id::Id;

fn main() {
    let guild = Id::new(1);
    let _ = MemberRequest::users(guild, [Id::new(2); 101]); // [GW-07]
    let _ = MemberRequest::users(guild, []); // [GW-07]
}
