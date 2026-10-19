//! The runtime Limit validator through the facade only (`noctis::ui`,
//! `noctis::rules`): a path-precise error, a duplicate `custom_id` and a
//! Lenient truncation (FR-24, CV2-08, CID-02).

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use noctis::prelude::*;
use noctis::rules::{Actual, Bound, LimitMode, Limits, RuleId, Violation};
use noctis::ui::{
    ActionRow, Components, Container, CostReport, CreateFlags, LimitWarning, Section, TextDisplay,
    UiError, V2Message, cost, validate,
};

#[derive(Route)]
#[route("v.buy")]
struct Buy {
    item: u64,
}

impl ::noctis::__private::ui::Handled for Buy {}

fn violations(r: Result<CostReport, UiError>) -> Vec<Violation> {
    match r {
        Err(UiError::Limits { violations }) => violations,
        other => panic!("expected UiError::Limits, got {other:?}"),
    }
}

#[test]
fn cv2_08_path_precise_error() {
    let mut msg = V2Message::new(
        Components::new().push(
            Container::new()
                .push(TextDisplay::new("a"))
                .push(TextDisplay::new("b"))
                .push(Section::new(
                    [TextDisplay::new("c")],
                    Buy { item: 1 }.button().label("a".repeat(81)),
                )),
        ),
    );
    let errs = violations(validate(&mut msg, &Limits::default()));
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].path, "container[0].section[2].accessory.label");
    assert_eq!(
        errs[0].to_string(),
        "[CV2-08] container[0].section[2].accessory.label: limit at most 80, actual 81"
    );
    assert_eq!(cost(&msg, &Limits::default()).components, 6);
}

#[test]
fn cid_02_duplicate_custom_id() {
    let mut msg = V2Message::new(Components::new().push(ActionRow::buttons([
        Buy { item: 1 }.button(),
        Buy { item: 1 }.button(),
    ])));
    let errs = violations(validate(&mut msg, &Limits::default()));
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].rule, RuleId::CID_02);
    assert_eq!(errs[0].path, "row[0].button[1].custom_id");
    assert_eq!(
        errs[0].limit,
        Bound::Describe("a custom_id unique within the message")
    );
    assert_eq!(errs[0].actual, Actual::Text("0v.buy:1".to_owned()));
    assert_eq!(errs[0].notes, ["first used at row[0].button[0].custom_id"]);
}

#[test]
fn cv2_08_lenient_truncates() {
    let mut limits = Limits::default();
    limits.mode = LimitMode::Lenient;
    let mut msg = V2Message::new(Components::new().push(ActionRow::buttons([
        Buy { item: 1 }.button().label("a".repeat(81)),
    ])));
    let report = validate(&mut msg, &limits).expect("truncated to fit");
    assert_eq!(
        report.warnings,
        [LimitWarning::Truncated(Violation::new(
            RuleId::CV2_08,
            "row[0].button[0].label",
            Bound::AtMost(80),
            Actual::Count(81),
        ))]
    );
    let json = serde_json::to_value(msg.create_body(CreateFlags::empty())).expect("serializes");
    assert_eq!(
        json["components"][0]["components"][0]["label"],
        format!("{}…", "a".repeat(79))
    );
}
