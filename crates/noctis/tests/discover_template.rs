//! Story 18.2 (FR-81): a Template registered through `module = ".."` on
//! `#[template(..)]` instead of `Module::template::<T>()`.

#![cfg(feature = "image")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::image::{Frame, Node, Template, TemplateView, view};
use noctis::{App, BuildViolation, Module};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "cards.badge", version = 1, size = (64, 64), format = Png, module = "cards")]
struct Badge {
    label: String,
}

impl TemplateView for Badge {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div tw="flex w-full h-full">{&self.label}</div> }
    }
}

/// The discovered Template lands in the created `cards` Module: adding it by
/// hand to another Module too registers its id twice.
#[tokio::test]
async fn discovered_template_is_registered() {
    let app = App::builder().discover().build().unwrap();
    assert!(app.modules().ids().any(|id| id == "cards"));
    assert_eq!(Badge::ID, "cards.badge");

    let err = App::builder()
        .module(Module::new("other").template::<Badge>())
        .discover()
        .build()
        .unwrap_err();
    assert!(
        err.violations().iter().any(|v| matches!(
            v,
            BuildViolation::DuplicateTemplate {
                id: "cards.badge",
                first_module: "other",
                second_module: "cards",
                ..
            }
        )),
        "{err}"
    );
}
