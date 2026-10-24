//! Story 10.2: `#[derive(Template)]`, the `ViewFn` decoder, `view!`, `tw!`,
//! `theme!` and `Module::template` through the facade. The macros validate
//! against this crate's `noctis-theme.css`.

#![cfg(feature = "image")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::time::Duration;

use noctis::image::unstable::ViewFn;
use noctis::image::{
    AnimatedFormat, AnimationSpec, Classes, Format, Frame, Node, Size, Style, Tag, Template,
    TemplateDescriptor, TemplateView, Theme, div, span, text, theme, tw, view,
};
use noctis::ports::{RenderError, RenderJob, ValueError};
use noctis::{App, BuildViolation, Module};
use serde::{Deserialize, Serialize};

/// The project template's profile card without the planned remote
/// `avatar` (Story 10.8).
#[derive(Template, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]
struct ProfileCard {
    name: String,
    subtitle: String,
    level_label: String,
    xp_label: String,
    progress: f32,
}

impl TemplateView for ProfileCard {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl">
                <div tw="flex flex-col gap-3 flex-1">
                    <span tw="text-5xl font-bold text-white">{&self.name}</span>
                    <span tw="text-xl text-muted">{&self.subtitle}</span>
                    <div tw="flex justify-between text-2xl text-white">
                        <span>{&self.level_label}</span>
                        <span tw="text-muted">{&self.xp_label}</span>
                    </div>
                    <div tw="flex w-full h-6 rounded-full bg-slate-700">
                        <div
                            tw="h-full rounded-full bg-linear-to-r from-indigo-500 to-violet-400"
                            style={Style::new().width_pct(self.progress * 100.0)}
                        />
                    </div>
                </div>
            </div>
        }
    }
}

fn card() -> ProfileCard {
    ProfileCard {
        name: "Ada".to_owned(),
        subtitle: "Level 7".to_owned(),
        level_label: "Lv 7".to_owned(),
        xp_label: "1200 / 1500 XP".to_owned(),
        progress: 0.8,
    }
}

/// The card's tree, written with the builders.
fn card_by_hand(c: &ProfileCard) -> Node {
    div(tw!(
        "flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl"
    ))
    .child(
        div(tw!("flex flex-col gap-3 flex-1"))
            .child(span(tw!("text-5xl font-bold text-white")).child(&c.name))
            .child(span(tw!("text-xl text-muted")).child(&c.subtitle))
            .child(
                div(tw!("flex justify-between text-2xl text-white"))
                    .child(Node::element(Tag::Span).child(&c.level_label))
                    .child(span(tw!("text-muted")).child(&c.xp_label)),
            )
            .child(
                div(tw!("flex w-full h-6 rounded-full bg-slate-700")).child(
                    div(tw!(
                        "h-full rounded-full bg-linear-to-r from-indigo-500 to-violet-400"
                    ))
                    .style(Style::new().width_pct(c.progress * 100.0)),
                ),
            ),
    )
}

#[test]
fn derive_consts_and_descriptor() {
    assert_eq!(ProfileCard::ID, "profile.card");
    assert_eq!(ProfileCard::VERSION, 1);
    assert_eq!(ProfileCard::SIZE, Size::new(934, 282));
    assert_eq!(ProfileCard::FORMAT, Format::Png);
    assert_eq!(ProfileCard::ANIMATION, None);
    let descriptor = TemplateDescriptor::of::<ProfileCard>();
    assert_eq!((descriptor.id, descriptor.version), ("profile.card", 1));
    assert!(descriptor.view.downcast_ref::<ViewFn>().is_some());
}

#[derive(Template, Serialize, Deserialize)]
#[template(
    version = 3u32,
    animation(duration_ms = 2000, fps = 20, format = Webp { quality: 80 }),
    format = Jpeg { quality: 90 },
    size = (64, 32),
    id = "anim.counter"
)]
struct Counter {
    target: u32,
}

impl TemplateView for Counter {
    fn view(&self, frame: Frame) -> Node {
        view! { <p>{frame.index}</p> }
    }
}

#[test]
fn derive_animation() {
    assert_eq!(
        Counter::ANIMATION,
        Some(AnimationSpec::new(
            Duration::from_secs(2),
            20,
            AnimatedFormat::Webp { quality: 80 }
        ))
    );
    assert_eq!(Counter::FORMAT, Format::Jpeg { quality: 90 });
    assert_eq!((Counter::ID, Counter::VERSION), ("anim.counter", 3));
}

fn decoder<T: Template>() -> ViewFn {
    *TemplateDescriptor::of::<T>()
        .view
        .downcast_ref::<ViewFn>()
        .expect("a derived Template's view is a ViewFn")
}

#[test]
fn decoder_round_trips() {
    let value = card();
    let job = RenderJob::of(&value).unwrap();
    let node = decoder::<ProfileCard>()
        .call(&job.data, Frame::STILL)
        .unwrap();
    assert_eq!(node, value.view(Frame::STILL));
}

#[test]
fn decoder_passes_the_frame() {
    let job = RenderJob::of(&Counter { target: 9 }).unwrap();
    let node = decoder::<Counter>()
        .call(
            &job.data,
            Frame {
                index: 3,
                count: 10,
            },
        )
        .unwrap();
    assert_eq!(node, Node::element(Tag::P).child(Node::text("3")));
}

fn source_value(err: &RenderError) -> Option<ValueError> {
    let RenderError::Failed(source) = err else {
        return None;
    };
    source.downcast_ref::<ValueError>().copied()
}

#[test]
fn decoder_rejects_bad_data() {
    let err = decoder::<ProfileCard>()
        .call(&[], Frame::STILL)
        .unwrap_err();
    assert_eq!(source_value(&err), Some(ValueError::Malformed));
    let mut data = RenderJob::of(&card()).unwrap().data.to_vec();
    data.push(0);
    let err = decoder::<ProfileCard>()
        .call(&data, Frame::STILL)
        .unwrap_err();
    assert_eq!(source_value(&err), Some(ValueError::Trailing { extra: 1 }));
}

#[test]
fn view_equals_the_builder_tree() {
    let c = card();
    assert_eq!(c.view(Frame::STILL), card_by_hand(&c));
}

#[test]
fn user_text_is_text() {
    let name = "<b tw=\"hidden\">".to_owned();
    let node = view! { <div>{&name}</div> };
    assert_eq!(
        node,
        Node::element(Tag::Div).child(Node::text(name.clone()))
    );
}

#[test]
fn view_children_forms() {
    let pick = |n: u8| {
        view! {
            <div>
                if n == 0 { "zero" } else if n == 1 { <span>"one"</span> } else { {n} "+" }
            </div>
        }
    };
    assert_eq!(pick(0), div(Classes::default()).child("zero"));
    assert_eq!(
        pick(1),
        div(Classes::default()).child(Node::element(Tag::Span).child("one"))
    );
    assert_eq!(pick(5), div(Classes::default()).child("5").child("+"));

    let badges = ["a", "b"];
    let none: Option<Node> = None;
    let nodes = vec![Node::text("x"), Node::text("y")];
    let tree = view! {
        <h1 tw={tw!("flex").and(Classes::dynamic("gap-2"))} style={Style::new().gap_px(2)}>
            for (i, badge) in badges.iter().enumerate() { <h2>{*badge}</h2> {i} }
            {none}
            {nodes.clone()}
            {42u8}
            {1.5f32}
            if false { "never" }
            <h3/>
        </h1>
    };
    let expected = Node::element(Tag::H1)
        .classes(tw!("flex").and(Classes::dynamic("gap-2")))
        .style(Style::new().gap_px(2))
        .child(Node::element(Tag::H2).child("a"))
        .child("0")
        .child(Node::element(Tag::H2).child("b"))
        .child("1")
        .child(nodes)
        .child("42")
        .child("1.5")
        .child(Node::element(Tag::H3));
    assert_eq!(tree, expected);
}

#[test]
fn tw_literals() {
    const CARD: Classes = tw!("flex p-4 bg-brand sm:!p-2");
    assert_eq!(CARD.as_str(), "flex p-4 bg-brand sm:!p-2");
    assert_eq!(tw!("").as_str(), "");
    assert_eq!(tw!("  \t").as_str(), "  \t");
    // The project template's avatar `<img>` classes validate too.
    assert!(
        !tw!("w-40 h-40 rounded-full border-4 border-brand")
            .as_str()
            .is_empty()
    );
    let joined = tw!("p-4").and(Classes::dynamic("x"));
    assert_eq!(joined.as_str(), "p-4 x");
    assert_eq!(
        text("hi", tw!("font-display text-brand-soft")),
        Node::text("hi").classes(tw!("font-display text-brand-soft"))
    );
}

#[test]
fn theme_macro_embeds_the_crate_theme() {
    let embedded: Theme = theme!();
    let parsed = Theme::parse(include_str!("../noctis-theme.css")).unwrap();
    assert_eq!(embedded, parsed);
    assert_eq!(
        embedded
            .keyframes()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["rise"]
    );
    assert_eq!(theme!("../noctis-theme.css"), parsed);
}

#[derive(Template, Serialize, Deserialize)]
#[template(id = "profile.card", version = 2, size = (10, 10), format = WebpLossless)]
struct OtherCard;

impl TemplateView for OtherCard {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div/> }
    }
}

#[tokio::test]
async fn duplicate_template_ids_fail_the_build() {
    let err = App::builder()
        .module(Module::new("a").template::<ProfileCard>())
        .module(Module::new("b").template::<OtherCard>())
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicateTemplate {
            id: "profile.card",
            first_module: "a",
            first_type: std::any::type_name::<ProfileCard>(),
            second_module: "b",
            second_type: std::any::type_name::<OtherCard>(),
        }]
    );
    let text = err.violations()[0].to_string();
    assert!(
        text.contains("`template::ProfileCard` (Module `a`)"),
        "{text}"
    );
}

#[tokio::test]
async fn same_template_twice_is_one_violation() {
    let err = App::builder()
        .module(
            Module::new("a")
                .template::<ProfileCard>()
                .template::<ProfileCard>(),
        )
        .build()
        .unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [BuildViolation::DuplicateTemplate {
                id: "profile.card",
                ..
            }]
        ),
        "{err}"
    );
}

#[tokio::test]
async fn distinct_templates_build() {
    App::builder()
        .module(
            Module::new("a")
                .template::<ProfileCard>()
                .module(Module::new("a/b").template::<Counter>()),
        )
        .build()
        .unwrap();
}
