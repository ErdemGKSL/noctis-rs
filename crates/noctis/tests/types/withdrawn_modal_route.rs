use noctis::prelude::*;

#[derive(Modal)]
#[modal(route = Save, title = "Bio")] // withdrawn form
struct BioForm {
    #[text_input("Bio")]
    bio: String,
}

fn main() {}
