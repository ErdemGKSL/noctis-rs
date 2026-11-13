//! The bot's application emojis: one `#[derive(AppEmojis)]` set, one variant per icon.
//!
//! Each variant is an emoji named in `snake_case` (`StOnline` is `st_online`) whose
//! image is `assets/emoji/<name>.png`, embedded at compile time; `#[emoji(white)]` also
//! declares `<name>_white` from `<name>_white.png`, the variant for `Primary`, `Success`
//! and `Danger` buttons. A bad name, a missing file, a file that is not PNG, JPEG or GIF,
//! or one over 256 KiB fails `cargo check` (`[EMO-02]`..`[EMO-04]`).
//!
//! `#[emoji(render)]` reads no file: the set's `art` function returns a Template, and
//! Publish renders it through the App's Renderer as a PNG and uploads it. Publish
//! (`--publish`, `--publish-guild`, `NOCTIS_PUBLISHER=true`) uploads every missing
//! name; every process then looks the ids up with `ctx.emojis()`.

use noctis::image::{Frame, Node, Style, Template, TemplateView, view};
use noctis::prelude::*;
use serde::{Deserialize, Serialize};

/// The icons, registered in `configure` with `.emojis::<Icons>()`.
#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "assets/emoji", art = icon_art)]
pub enum Icons {
    /// `check` and `check_white`.
    #[emoji(white)]
    Check,
    /// `cross` and `cross_white`.
    #[emoji(white)]
    Cross,
    /// `st_online`, a status dot for text.
    StOnline,
    /// `level_up`, drawn by [`LevelUpGlyph`] at Publish: the level-up line and the
    /// leaderboard heading. Text only, so it has no white variant.
    #[emoji(render)]
    LevelUp,
}

/// The art of the drawn icons (`#[emoji(render)]`), called at Publish once per drawn
/// name: `EmojiTone::Colour` for `<name>`, and `EmojiTone::White` for `<name>_white`
/// when the variant also has `white`. `LevelUp` is the only drawn icon and has no white
/// variant; give each further drawn icon its own Template or a `glyph` field.
fn icon_art(_icon: Icons, _tone: EmojiTone) -> EmojiArt {
    EmojiArt::template(LevelUpGlyph { tint: 0x58_65F2 })
}

/// A 128 × 128 glyph in the vector style of the set: a disc in the brand colour with a
/// white chevron pointing up. The root has no background, so the PNG is transparent.
/// Drawn icons need no `module`: the App registers every art Template at `build()`.
#[derive(Template, Serialize, Deserialize, Debug, Clone, Copy)]
#[template(id = "emoji.level_up", version = 1, size = (128, 128), format = Png)]
pub struct LevelUpGlyph {
    pub tint: u32,
}

impl TemplateView for LevelUpGlyph {
    fn view(&self, _frame: Frame) -> Node {
        // Two borders of a box turned 45°: a chevron; `pt-6` keeps it optically centred.
        view! {
            <div tw="flex items-center justify-center w-full h-full">
                <div tw="flex items-center justify-center w-28 h-28 pt-6 rounded-full"
                     style={Style::new().bg_rgb(self.tint)}>
                    <div tw="w-12 h-12 border-t-[16px] border-l-[16px] border-white rounded-sm"
                         style={Style::new().rotate_deg(45.0)} />
                </div>
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_declare_snake_case_names_and_white_pairs() {
        let names: Vec<_> = Icons::DECLARATIONS
            .iter()
            .map(noctis::emoji::EmojiDecl::name)
            .collect();
        assert_eq!(names, ["check", "cross", "st_online", "level_up"]);
        assert_eq!(Icons::Check.declaration().white_name(), Some("check_white"));
        assert_eq!(Icons::StOnline.declaration().white_name(), None);
        assert!(matches!(
            Icons::LevelUp.declaration().image(),
            noctis::emoji::EmojiImage::Render(_)
        ));
    }
}
