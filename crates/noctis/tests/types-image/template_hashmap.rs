// Template data is part of the render cache key: HashMap/HashSet fields fail.
use std::collections::{HashMap, HashSet};

use noctis::image::{Frame, Node, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "card", version = 1, size = (10, 10), format = Png)]
struct Card {
    a: HashMap<String, u8>,
    b: Vec<HashMap<u8, u8>>,
    c: Option<std::collections::HashSet<u8>>,
    d: HashSet<u8>,
}

impl TemplateView for Card {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div/> }
    }
}

fn main() {}
