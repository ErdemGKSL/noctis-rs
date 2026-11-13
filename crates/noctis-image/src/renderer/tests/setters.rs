//! Story 20.7: the typed `Style` setters draw, win over a class of the same
//! property, and compose their transforms in the documented order
//! (translate, then rotate, then scale, around the centre).

use super::*;

/// `bg-red-500` (Tailwind v4's `oklch` red as takumi draws it).
const RED: [u8; 4] = [251, 44, 54, 0xFF];
const GREEN: [u8; 4] = [0, 0xFF, 0, 0xFF];
const CLEAR: u8 = 0;

/// The decoded pixels of `template` rendered with the shared renderer.
async fn draw<T: Template>(template: &T) -> (u32, Vec<u8>) {
    let out = renderer()
        .render(RenderJob::of(template).unwrap())
        .await
        .unwrap();
    let (w, _, rgba) = decode_png(&out.bytes);
    (w, rgba)
}

fn is_red(px: [u8; 4]) -> bool {
    px == RED
}

fn is_clear(px: [u8; 4]) -> bool {
    px[3] == CLEAR
}

#[tokio::test]
async fn rounded_px_wins_over_rounded_lg() {
    let (w, control) = draw(&Rounded { styled: false }).await;
    assert!(
        pixel(&control, w, 1, 1)[3] < 32,
        "rounded-lg cuts the corner"
    );
    let (w, styled) = draw(&Rounded { styled: true }).await;
    assert!(
        is_red(pixel(&styled, w, 1, 1)),
        "a 3 px radius keeps (1, 1)"
    );
    assert!(pixel(&styled, w, 0, 0)[3] < 255, "and still rounds (0, 0)");
}

#[tokio::test]
async fn left_px_wins_over_left_4() {
    let (w, control) = draw(&Inset { styled: false }).await;
    assert!(is_red(pixel(&control, w, 20, 1)));
    assert!(is_clear(pixel(&control, w, 44, 1)));
    let (w, styled) = draw(&Inset { styled: true }).await;
    assert!(is_clear(pixel(&styled, w, 20, 1)));
    assert!(is_red(pixel(&styled, w, 41, 1)));
    assert!(is_red(pixel(&styled, w, 47, 7)));
    assert!(is_clear(pixel(&styled, w, 48, 1)));
}

#[tokio::test]
async fn pixel_and_percent_insets_place_an_absolute_node() {
    // left 50% = 10, right 2 px, top -2 px, bottom 25% = 5: x 10..18, y 0..15.
    let (w, rgba) = draw(&Insets {}).await;
    for (x, y) in [(10, 0), (17, 0), (10, 14), (17, 14)] {
        assert!(is_red(pixel(&rgba, w, x, y)), "({x}, {y})");
    }
    for (x, y) in [(9, 5), (18, 5), (12, 15)] {
        assert!(is_clear(pixel(&rgba, w, x, y)), "({x}, {y})");
    }
}

#[tokio::test]
async fn border_px_draws_solid_and_wins_over_border_4() {
    for class in [false, true] {
        let (w, rgba) = draw(&Bordered { class }).await;
        assert_eq!(pixel(&rgba, w, 0, 0), GREEN, "class {class}");
        assert_eq!(pixel(&rgba, w, 1, 4), GREEN, "class {class}");
        assert!(is_red(pixel(&rgba, w, 2, 2)), "class {class}");
        assert!(is_red(pixel(&rgba, w, 4, 4)), "class {class}");
    }
}

#[tokio::test]
async fn z_index_draws_an_earlier_sibling_on_top() {
    let (w, control) = draw(&Stacked { styled: false }).await;
    assert_eq!(pixel(&control, w, 4, 4), BRAND);
    let (w, styled) = draw(&Stacked { styled: true }).await;
    assert!(is_red(pixel(&styled, w, 4, 4)));
}

#[tokio::test]
async fn scale_wins_over_its_class_and_composes_after_translate_and_rotate() {
    // An 8x4 bar centred on (16, 16), scaled 2x (not the class's 0.5x) to
    // 16x8 and moved 4 px right, unscaled: x 12..28, y 12..20.
    let (w, flat) = draw(&Scaled { rotate: false }).await;
    for (x, y) in [(12, 12), (27, 19), (13, 16)] {
        assert!(is_red(pixel(&flat, w, x, y)), "({x}, {y})");
    }
    for (x, y) in [(11, 16), (28, 16), (20, 11), (20, 20)] {
        assert!(is_clear(pixel(&flat, w, x, y)), "({x}, {y})");
    }
    // Rotated 90 degrees about its own centre, then moved: x 16..24, y 8..24.
    let (w, turned) = draw(&Scaled { rotate: true }).await;
    for (x, y) in [(16, 8), (23, 23), (20, 16)] {
        assert!(is_red(pixel(&turned, w, x, y)), "({x}, {y})");
    }
    for (x, y) in [(13, 16), (26, 16), (20, 7), (20, 24)] {
        assert!(is_clear(pixel(&turned, w, x, y)), "({x}, {y})");
    }
}
