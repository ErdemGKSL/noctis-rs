// `#[template(..)]`: missing attribute, missing key, duplicate key, unknown key.
use noctis::image::Template;
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
struct NoAttribute;

#[derive(Template, Serialize, Deserialize)]
#[template(id = "a", version = 1, format = Png)]
struct NoSize;

#[derive(Template, Serialize, Deserialize)]
#[template(id = "a", id = "b", version = 1, size = (1, 1), format = Png)]
struct IdTwice;

#[derive(Template, Serialize, Deserialize)]
#[template(id = "a", version = 1, size = (1, 1), format = Png, colour = 1)]
struct UnknownKey;

fn main() {}
