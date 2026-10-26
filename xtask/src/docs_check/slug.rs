//! GitHub heading anchors: the slug of a heading's rendered text, made unique
//! per file the way GitHub (github-slugger) does it.

use std::collections::HashMap;

/// The GitHub slug of a heading's rendered text (A4, A5): lowercase
/// (Unicode), every character that is not alphanumeric (Unicode), a space,
/// `-` or `_` dropped, then every space turned into `-`.
pub(super) fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Hands out the anchors of one file in heading order: a repeated slug gets
/// `-1`, `-2`, … (A6).
#[derive(Debug, Default)]
pub(super) struct Slugger {
    occurrences: HashMap<String, u32>,
}

impl Slugger {
    /// The unique anchor of the next heading whose rendered text is `text`.
    pub(super) fn next(&mut self, text: &str) -> String {
        let original = slug(text);
        let mut result = original.clone();
        while self.occurrences.contains_key(&result) {
            let count = self.occurrences.entry(original.clone()).or_insert(0);
            *count += 1;
            result = format!("{original}-{count}");
        }
        self.occurrences.insert(result.clone(), 0);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A4, A5 and the catalog's own headings.
    #[test]
    fn slugs_follow_github() {
        for (heading, expected) in [
            (
                "1. The custom_id layout and fixed-length Refs",
                "1-the-custom_id-layout-and-fixed-length-refs",
            ),
            (
                "3. Command Pattern expansion and structural validation",
                "3-command-pattern-expansion-and-structural-validation",
            ),
            ("9. Components V2 (messages)", "9-components-v2-messages"),
            ("Çalışma — notları!", "çalışma--notları"),
            ("tescord / dbi → Noctis", "tescord--dbi--noctis"),
            ("NOCTIS_PAYLOAD_KEY", "noctis_payload_key"),
            ("Title", "title"),
        ] {
            assert_eq!(slug(heading), expected, "{heading}");
        }
    }

    /// A6: repeated headings get numbered suffixes, and a literal `-1`
    /// heading does not collide with a generated one.
    #[test]
    fn duplicates_get_suffixes() {
        let mut slugger = Slugger::default();
        let got: Vec<String> = ["Setup", "Setup", "Setup-1", "Setup"]
            .into_iter()
            .map(|h| slugger.next(h))
            .collect();
        assert_eq!(got, ["setup", "setup-1", "setup-1-1", "setup-2"]);
    }
}
