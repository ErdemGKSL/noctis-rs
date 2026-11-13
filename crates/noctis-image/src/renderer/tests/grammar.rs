//! Story 20.10: theme colours as gradient stops, and the Noctis-lowered
//! `z-*` and `tabular-nums` utilities, drawn.

use super::*;

/// `bg-red-500` (Tailwind v4's `oklch` red as takumi draws it).
const RED: [u8; 4] = [251, 44, 54, 0xFF];

async fn draw<T: Template>(template: &T) -> (u32, u32, Vec<u8>) {
    let out = renderer()
        .render(RenderJob::of(template).unwrap())
        .await
        .unwrap();
    decode_png(&out.bytes)
}

fn near(px: [u8; 4], want: [u8; 4]) -> bool {
    px.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 2)
}

#[tokio::test]
async fn a_theme_colour_gradient_stop_draws_in_the_theme_colour() {
    let (w, _, rgba) = draw(&StopGradient {}).await;
    // `from-brand` and `to-brand` at the ends, `via-brand/50` half
    // transparent in the middle: none of them transparent.
    for x in [0, 31] {
        let px = pixel(&rgba, w, x, 4);
        assert!(
            near([px[0], px[1], px[2], 0xFF], BRAND) && px[3] > 240,
            "x {x}: {px:?}"
        );
    }
    let middle = pixel(&rgba, w, 16, 4);
    assert!(
        (100..=200).contains(&middle[3]) && middle[2] > middle[0],
        "the 50% stop is the brand colour, half transparent: {middle:?}"
    );
}

#[tokio::test]
async fn z_10_paints_on_top_of_a_later_z_0_and_style_z_index_wins() {
    let (w, _, rgba) = draw(&Overlap { styled: false }).await;
    assert_eq!(pixel(&rgba, w, 4, 4), RED, "z-10 is on top");
    let (w, _, rgba) = draw(&Overlap { styled: true }).await;
    assert_eq!(
        pixel(&rgba, w, 4, 4),
        BRAND,
        "Style::z_index(-1) beats z-10"
    );
}

/// The red widths of the two rows (`1111`, `0000`, `text-2xl`, so 32 px
/// high each) of a [`Digits`] image; the glyphs are transparent.
fn row_widths(w: u32, rgba: &[u8]) -> [usize; 2] {
    [4, 36].map(|y| (0..w).filter(|&x| pixel(rgba, w, x, y) == RED).count())
}

#[tokio::test]
async fn tabular_nums_gives_the_digits_one_advance_width() {
    let (w, _, rgba) = draw(&Digits { tabular: false }).await;
    let [ones, zeros] = row_widths(w, &rgba);
    assert!(
        ones > 0 && ones + 4 < zeros,
        "Inter's proportional `1` is narrower than its `0`: {ones} vs {zeros}"
    );
    let (w, _, rgba) = draw(&Digits { tabular: true }).await;
    let [ones, zeros] = row_widths(w, &rgba);
    assert!(ones > 0, "the rows draw");
    assert_eq!(ones, zeros, "tabular-nums: one advance width");
}
