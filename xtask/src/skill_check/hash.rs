//! The API hash check (H1-H4): `SKILL.md` has exactly one line containing
//! `**API hash:**`, and the first code span after that marker is the
//! snapshot's blake3 hash (64 lowercase hex digits).

use super::Problems;

const SUBJECT: &str = "SKILL.md";
const MARKER: &str = "**API hash:**";

/// Checks `skill_md` against the snapshot hash `expected`.
pub(super) fn check(skill_md: &str, expected: &str, problems: &mut Problems) {
    let lines: Vec<&str> = skill_md.lines().filter(|l| l.contains(MARKER)).collect();
    let [line] = lines.as_slice() else {
        problems.add(
            SUBJECT,
            format!("expected exactly one {MARKER} line, found {}", lines.len()),
        );
        return;
    };
    let after = line.split_once(MARKER).map_or("", |(_, rest)| rest);
    let span = after
        .split_once('`')
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(span, _)| span);
    match span {
        Some(found) if is_hash(found) => {
            if found != expected {
                problems.add(
                    SUBJECT,
                    format!(
                        "API hash {found} does not match xtask/public-api.txt ({expected}); update the skill for the API change, then the hash"
                    ),
                );
            }
        }
        _ => problems.add(
            SUBJECT,
            format!("API hash must be the snapshot's 64-hex blake3 hash ({expected})"),
        ),
    }
}

fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(skill: &str) -> Vec<String> {
        let mut problems = Problems::default();
        check(skill, &"a".repeat(64), &mut problems);
        problems.0.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn h1_match() {
        let a = "a".repeat(64);
        assert!(problems(&format!("x\n> **API hash:** `{a}` (blake3)\n")).is_empty());
    }

    #[test]
    fn h2_stale() {
        let b = "b".repeat(64);
        let got = problems(&format!("**API hash:** `{b}`"));
        assert_eq!(got.len(), 1);
        assert!(
            got[0].contains(&format!("API hash {b} does not match")),
            "{got:?}"
        );
    }

    #[test]
    fn h3_malformed() {
        for bad in [
            "`pending`".to_owned(),
            format!("`{}`", "a".repeat(63)),
            format!("`{}`", "A".repeat(64)),
            format!("`blake3:{}`", "a".repeat(64)),
            "no span".to_owned(),
        ] {
            let got = problems(&format!("**API hash:** {bad}"));
            assert_eq!(
                got,
                [format!(
                    "skill-check: SKILL.md: API hash must be the snapshot's 64-hex blake3 hash ({})",
                    "a".repeat(64)
                )],
                "{bad}"
            );
        }
    }

    #[test]
    fn h4_none_or_many() {
        assert_eq!(
            problems("nothing"),
            ["skill-check: SKILL.md: expected exactly one **API hash:** line, found 0"]
        );
        assert_eq!(problems("**API hash:** `x`\n**API hash:** `y`").len(), 1);
    }
}
