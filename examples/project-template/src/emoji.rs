//! The bot's application emojis: one `#[derive(AppEmojis)]` set, one variant per icon.
//!
//! Each variant is an emoji named in `snake_case` (`StOnline` is `st_online`) whose
//! image is `assets/emoji/<name>.png`, embedded at compile time; `#[emoji(white)]` also
//! declares `<name>_white` from `<name>_white.png`, the variant for `Primary`, `Success`
//! and `Danger` buttons. A bad name, a missing file, a file that is not PNG, JPEG or GIF,
//! or one over 256 KiB fails `cargo check` (`[EMO-02]`..`[EMO-04]`).

use noctis::prelude::*;

/// The icons, registered in `configure` with `.emojis::<Icons>()`.
#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "assets/emoji")]
pub enum Icons {
    /// `check` and `check_white`.
    #[emoji(white)]
    Check,
    /// `cross` and `cross_white`.
    #[emoji(white)]
    Cross,
    /// `st_online`, a status dot for text.
    StOnline,
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
        assert_eq!(names, ["check", "cross", "st_online"]);
        assert_eq!(Icons::Check.declaration().white_name(), Some("check_white"));
        assert_eq!(Icons::StOnline.declaration().white_name(), None);
    }
}
