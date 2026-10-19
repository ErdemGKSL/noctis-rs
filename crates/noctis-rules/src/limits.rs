//! The configurable [`Limits`] value and its [`LimitMode`].

use crate::RuleId;

/// What happens when dynamic text is over its limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LimitMode {
    /// Fail before sending: the validator returns a [`Violation`](crate::Violation)
    /// and nothing reaches Discord. The default.
    #[default]
    Strict,
    /// Truncate over-long display-text length fields with `…` (via
    /// [`truncate`](crate::len::truncate)) and log a warning.
    ///
    /// Only display-text lengths are truncated. Counts, byte sizes, patterns,
    /// URLs, values and ids always fail, as do the structural rules CV2-03,
    /// CV2-05, CV2-06, CID-01 and CID-02 (see [`Limits::truncates`]). It does not
    /// affect autocomplete either: INT-07 always truncates choices to 25.
    Lenient,
}

macro_rules! limits {
    ($($field:ident = $default:literal, $rule:literal, $doc:literal;)*) => {
        /// Every numeric Discord limit Noctis enforces, plus the [`LimitMode`].
        ///
        /// Each field is a maximum and names its rule id from the Discord Rules
        /// Catalog. [`Limits::default`] holds Discord's values in
        /// [`LimitMode::Strict`].
        ///
        /// Macros check static input against the defaults. Lowering a field at
        /// runtime makes the runtime validator re-check every tree, static ones
        /// included; raising one never relaxes a compile-time check (AD-3).
        ///
        /// The struct is `#[non_exhaustive]`, so build it by mutating a default
        /// instead of writing a struct literal:
        ///
        /// ```
        /// use noctis_rules::{LimitMode, Limits, RuleId};
        ///
        /// let mut l = Limits::default();
        /// l.mode = LimitMode::Lenient;
        /// l.components = 30;
        /// assert!(l.truncates(RuleId::CV2_04));
        /// assert!(!l.truncates(RuleId::CV2_03));
        /// ```
        #[non_exhaustive]
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Limits {
            /// Strict (fail) or Lenient (truncate text) handling of dynamic text.
            pub mode: LimitMode,
            $(
                #[doc = concat!("`", $rule, "`: ", $doc)]
                #[doc = ""]
                #[doc = concat!("Default: `", stringify!($default), "`.")]
                pub $field: usize,
            )*
        }

        impl Default for Limits {
            fn default() -> Self {
                Self {
                    mode: LimitMode::Strict,
                    $($field: $default,)*
                }
            }
        }

        /// Every limit field with the rule id it documents and its default.
        #[cfg_attr(not(test), expect(dead_code, reason = "read by the unit tests only for now"))]
        pub(crate) const FIELDS: &[(&str, RuleId, usize)] = &[
            $((stringify!($field), match RuleId::lookup($rule) {
                Some(rule) => rule,
                None => panic!("Limits field names an unknown rule id"),
            }, $default),)*
        ];
    };
}

limits! {
    command_name_len = 32, "CMD-02", "characters per command name word.";
    command_description_len = 100, "CMD-04", "characters in a command description.";
    command_size = 8000, "CMD-05", "characters per command, options and longest localizations included.";
    command_path_words = 3, "CMD-10", "words in a command path (command, group, subcommand).";
    command_options = 25, "CMD-13", "options, subcommands or groups per level.";
    pattern_expansions = 25, "CMD-15", "expansions of one command name pattern.";
    context_menu_name_len = 32, "CTX-01", "characters in a context-menu command name.";
    chat_input_commands = 100, "REG-01", "chat-input commands per scope (also REG-02).";
    user_commands = 15, "REG-01", "user commands per scope (also REG-02).";
    message_commands = 15, "REG-01", "message commands per scope (also REG-02).";
    entry_point_commands = 1, "REG-01", "entry-point commands per scope (also REG-02).";
    option_length = 6000, "OPT-06", "`min_length` / `max_length` of a string option.";
    option_choices = 25, "OPT-08", "choices per option.";
    choice_name_len = 100, "OPT-09", "characters in a choice name.";
    choice_value_len = 100, "OPT-09", "characters in a string choice value.";
    option_file_types = 10, "OPT-13", "`file_types` entries of an attachment option.";
    autocomplete_choices = 25, "INT-07", "autocomplete choices (always truncated, in every mode).";
    user_install_followups = 5, "INT-11", "follow-ups for a user-installed app outside the guild.";
    custom_id_len = 100, "CID-01", "characters in a `custom_id` (also MOD-08).";
    components = 40, "CV2-03", "components per message, nested ones included.";
    text_total_len = 4000, "CV2-04", "characters across all Text Displays of a message.";
    row_buttons = 5, "CV2-06", "buttons per Action Row.";
    section_texts = 3, "CV2-07", "Text Displays per Section.";
    button_label_len = 80, "CV2-08", "characters in a button label.";
    link_url_len = 512, "CV2-08", "characters in a link button URL.";
    select_placeholder_len = 150, "CV2-09", "characters in a select placeholder.";
    select_options = 25, "CV2-09", "options per String Select.";
    select_values = 25, "CV2-09", "`max_values` of a select.";
    select_option_label_len = 100, "CV2-09", "characters in a select option label.";
    select_option_value_len = 100, "CV2-09", "characters in a select option value.";
    select_option_description_len = 100, "CV2-09", "characters in a select option description.";
    gallery_items = 10, "CV2-10", "items per Media Gallery.";
    gallery_description_len = 1024, "CV2-10", "characters in a Media Gallery item description.";
    thumbnail_description_len = 1024, "CV2-11", "characters in a Thumbnail description.";
    legacy_action_rows = 5, "CV2-15", "Action Rows in a legacy (non-V2) message.";
    media_url_len = 2048, "CV2-17", "characters in a Media Gallery item or Thumbnail URL.";
    modal_title_len = 45, "MOD-01", "characters in a modal title.";
    modal_components = 5, "MOD-02", "top-level components of a modal.";
    label_len = 45, "MOD-04", "characters in a Label.";
    label_description_len = 100, "MOD-04", "characters in a Label description.";
    text_input_len = 4000, "MOD-05", "characters in a Text Input value (`min_length` / `max_length`).";
    text_input_placeholder_len = 100, "MOD-05", "characters in a Text Input placeholder.";
    file_upload_values = 10, "MOD-06", "files per File Upload (`max_values`).";
    file_upload_file_types = 10, "MOD-06", "`file_types` entries of a File Upload.";
    radio_options = 10, "MOD-07", "options per Radio Group.";
    checkbox_group_options = 10, "MOD-07", "options per Checkbox Group.";
    checkbox_group_values = 10, "MOD-07", "`max_values` of a Checkbox Group.";
    modal_option_text_len = 100, "MOD-07", "characters in a Radio or Checkbox Group option label, value or description.";
    content_len = 2000, "MSG-01", "characters in legacy message `content`.";
    mention_users = 100, "MSG-04", "user ids in allowed mentions.";
    mention_roles = 100, "MSG-04", "role ids in allowed mentions.";
    nonce_len = 25, "MSG-06", "characters in a message nonce.";
    sticker_ids = 3, "MSG-06", "stickers per message.";
    attachment_bytes = 20_971_520, "ATT-01", "bytes per file (20 MiB; an interaction's `attachment_size_limit` wins).";
    request_bytes = 26_214_400, "ATT-02", "bytes per request (25 MiB).";
    attachments = 10, "ATT-03", "attachments per message.";
    attachment_description_len = 1024, "ATT-06", "characters in attachment alt text.";
    webhook_name_len = 80, "WH-04", "characters in a webhook name.";
}

impl Limits {
    /// Whether the mode permits truncating text over the limit of `rule`.
    ///
    /// True only in [`LimitMode::Lenient`] and only for non-structural rules
    /// ([`RuleId::is_structural`]). It answers only whether the mode allows it:
    /// a validator truncates display-text length fields only, and counts, byte
    /// sizes, patterns, URLs, values and ids always fail, whatever this returns.
    ///
    /// It does not govern INT-07: the 25-choice autocomplete truncation is
    /// unconditional, in both modes, even though this returns `false` for
    /// `INT-07` in [`LimitMode::Strict`].
    pub const fn truncates(&self, rule: RuleId) -> bool {
        matches!(self.mode, LimitMode::Lenient) && !rule.is_structural()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn defaults_match_the_catalog() {
        let l = Limits::default();
        assert_eq!(l.mode, LimitMode::Strict);
        let expected: [(usize, usize); 58] = [
            (l.command_name_len, 32),
            (l.command_description_len, 100),
            (l.command_size, 8000),
            (l.command_path_words, 3),
            (l.command_options, 25),
            (l.pattern_expansions, 25),
            (l.context_menu_name_len, 32),
            (l.chat_input_commands, 100),
            (l.user_commands, 15),
            (l.message_commands, 15),
            (l.entry_point_commands, 1),
            (l.option_length, 6000),
            (l.option_choices, 25),
            (l.choice_name_len, 100),
            (l.choice_value_len, 100),
            (l.option_file_types, 10),
            (l.autocomplete_choices, 25),
            (l.user_install_followups, 5),
            (l.custom_id_len, 100),
            (l.components, 40),
            (l.text_total_len, 4000),
            (l.row_buttons, 5),
            (l.section_texts, 3),
            (l.button_label_len, 80),
            (l.link_url_len, 512),
            (l.select_placeholder_len, 150),
            (l.select_options, 25),
            (l.select_values, 25),
            (l.select_option_label_len, 100),
            (l.select_option_value_len, 100),
            (l.select_option_description_len, 100),
            (l.gallery_items, 10),
            (l.gallery_description_len, 1024),
            (l.thumbnail_description_len, 1024),
            (l.legacy_action_rows, 5),
            (l.media_url_len, 2048),
            (l.modal_title_len, 45),
            (l.modal_components, 5),
            (l.label_len, 45),
            (l.label_description_len, 100),
            (l.text_input_len, 4000),
            (l.text_input_placeholder_len, 100),
            (l.file_upload_values, 10),
            (l.file_upload_file_types, 10),
            (l.radio_options, 10),
            (l.checkbox_group_options, 10),
            (l.checkbox_group_values, 10),
            (l.modal_option_text_len, 100),
            (l.content_len, 2000),
            (l.mention_users, 100),
            (l.mention_roles, 100),
            (l.nonce_len, 25),
            (l.sticker_ids, 3),
            (l.attachment_bytes, 20_971_520),
            (l.request_bytes, 26_214_400),
            (l.attachments, 10),
            (l.attachment_description_len, 1024),
            (l.webhook_name_len, 80),
        ];
        for (i, (actual, want)) in expected.into_iter().enumerate() {
            assert_eq!(actual, want, "field {} ({})", i, FIELDS[i].0);
            assert_eq!(FIELDS[i].2, actual, "FIELDS default of {}", FIELDS[i].0);
        }
    }

    const CHEATSHEET: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md"
    ));

    #[test]
    fn cheatsheet_limits_table_matches_fields() {
        let start = CHEATSHEET
            .find("## Limits value")
            .expect("Limits value section");
        let section = &CHEATSHEET[start..];
        let end = section.find("## Rules types").expect("Rules types section");
        let rows: Vec<(String, String, usize)> = section[..end]
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .map(|rest| {
                let cells: Vec<&str> = rest.split('|').map(str::trim).collect();
                let field = cells[0].trim_end_matches('`').to_owned();
                let default = cells[2].replace(',', "").parse().expect("numeric default");
                (field, cells[1].to_owned(), default)
            })
            .collect();
        let expected: Vec<(String, String, usize)> = FIELDS
            .iter()
            .map(|(name, rule, default)| ((*name).to_owned(), rule.as_str().to_owned(), *default))
            .collect();
        assert_eq!(
            rows, expected,
            "limits-cheatsheet.md Limits table differs from Limits"
        );
    }

    #[test]
    fn field_table_has_58_unique_fields_with_catalog_ids() {
        assert_eq!(FIELDS.len(), 58);
        let names: HashSet<_> = FIELDS.iter().map(|(name, _, _)| name).collect();
        assert_eq!(names.len(), 58);
        for (name, rule, _) in FIELDS {
            assert!(RuleId::ALL.contains(rule), "{name}: {rule}");
        }
    }

    #[test]
    fn field_rule_ids() {
        let rule_of = |field: &str| {
            FIELDS
                .iter()
                .find(|(name, _, _)| *name == field)
                .map(|(_, rule, _)| *rule)
        };
        assert_eq!(rule_of("components"), Some(RuleId::CV2_03));
        assert_eq!(rule_of("media_url_len"), Some(RuleId::CV2_17));
        assert_eq!(rule_of("custom_id_len"), Some(RuleId::CID_01));
        assert_eq!(rule_of("webhook_name_len"), Some(RuleId::WH_04));
    }

    #[test]
    fn strict_never_truncates() {
        let l = Limits::default();
        for rule in RuleId::ALL {
            assert!(!l.truncates(*rule), "{rule}");
        }
    }

    #[expect(
        clippy::field_reassign_with_default,
        reason = "mirrors the mutation pattern callers outside the crate must use"
    )]
    fn lenient() -> Limits {
        let mut l = Limits::default();
        l.mode = LimitMode::Lenient;
        l
    }

    #[test]
    fn lenient_never_applies_to_cv2_03() {
        assert!(!lenient().truncates(RuleId::CV2_03));
    }

    #[test]
    fn lenient_never_applies_to_cv2_05() {
        assert!(!lenient().truncates(RuleId::CV2_05));
    }

    #[test]
    fn lenient_never_applies_to_cv2_06() {
        assert!(!lenient().truncates(RuleId::CV2_06));
    }

    #[test]
    fn lenient_never_applies_to_cid_01() {
        assert!(!lenient().truncates(RuleId::CID_01));
    }

    #[test]
    fn lenient_never_applies_to_cid_02() {
        assert!(!lenient().truncates(RuleId::CID_02));
    }

    #[test]
    fn lenient_truncates_text_rules() {
        assert!(lenient().truncates(RuleId::CV2_04));
        assert!(lenient().truncates(RuleId::MSG_01));
    }

    #[test]
    #[expect(
        clippy::field_reassign_with_default,
        reason = "mirrors the mutation pattern callers outside the crate must use"
    )]
    fn lowered_override_changes_only_that_field() {
        let mut l = Limits::default();
        l.components = 30;
        assert_eq!(l.components, 30);
        assert_ne!(l, Limits::default());
        l.components = Limits::default().components;
        assert_eq!(l, Limits::default());
    }
}
