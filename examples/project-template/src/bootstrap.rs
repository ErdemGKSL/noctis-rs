//! First-run setup. The bot is a single self-contained binary: locale files are
//! compiled in by `i18n!`, the theme by `theme!()`, the default font by the
//! `default-font` feature, and `.env.example` by this module. Nothing next to the
//! executable is needed except the `.env` it writes on first run.
//!
//! Memory: `.env.example` is a `&'static str` in the binary's read-only data. It is
//! read once, on first run, and never copied to the heap; those pages are
//! file-backed, so the OS drops them from memory once they go cold. (Static data
//! can't be zeroized: it isn't writable memory.) Everything this module *builds*
//! from it lives in `Zeroizing` buffers, which are wiped when they drop. The
//! secrets it generates never outlive the write.
//!
//! Locale strings are the opposite: they are used on every interaction, so they
//! stay in memory as compiled, typed accessors.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use zeroize::Zeroizing;

/// `.env.example`, embedded at compile time.
static ENV_EXAMPLE: &str = include_str!("../.env.example");

/// The variable that receives a freshly generated key when `.env` is created.
const PAYLOAD_KEY_VAR: &str = "NOCTIS_PAYLOAD_KEY";

/// What [`load_or_create_env`] did.
#[derive(Debug, PartialEq, Eq)]
#[must_use]
pub enum EnvFile {
    /// `.env` existed and its variables were loaded into the process environment.
    Loaded,
    /// `.env` didn't exist and was written from the embedded `.env.example`, with a
    /// fresh `NOCTIS_PAYLOAD_KEY`. The bot should stop so the owner can fill in the
    /// token.
    Created,
}

/// Loads `path` into the process environment, or creates it from the embedded
/// `.env.example` when it doesn't exist.
///
/// Variables already set in the environment win over the file, so deployments that
/// export variables directly never need a `.env`.
///
/// # Errors
///
/// Returns an error if `.env` exists but can't be read or parsed, or if it can't be
/// created (for example in a read-only directory), or if the OS random source fails.
pub fn load_or_create_env(path: &Path) -> anyhow::Result<EnvFile> {
    match dotenvy::from_path(path) {
        Ok(()) => return Ok(EnvFile::Loaded),
        Err(dotenvy::Error::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    let contents = render_env(ENV_EXAMPLE)?;
    let mut file = create_private(path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok(EnvFile::Created)
    // `contents` (and the key inside it) is zeroized here.
}

/// Lowercase hex digits.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Copies `template`, filling an empty `NOCTIS_PAYLOAD_KEY=` line with 32 random
/// bytes as 64 lowercase hex digits (the format `PayloadKey::from_hex` reads).
/// Every intermediate buffer is zeroized on drop.
fn render_env(template: &str) -> anyhow::Result<Zeroizing<String>> {
    let mut raw_key = Zeroizing::new([0u8; 32]);
    getrandom::fill(raw_key.as_mut_slice())
        .map_err(|e| anyhow::anyhow!("OS random source failed: {e}"))?;
    let mut key = Zeroizing::new(String::with_capacity(64));
    for byte in raw_key.iter() {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }

    let mut out = Zeroizing::new(String::with_capacity(template.len() + key.len()));
    for line in template.lines() {
        match line
            .strip_prefix(PAYLOAD_KEY_VAR)
            .and_then(|rest| rest.strip_prefix('='))
        {
            Some(value) if value.trim().is_empty() => {
                out.push_str(PAYLOAD_KEY_VAR);
                out.push('=');
                out.push_str(&key);
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    Ok(out)
}

/// Creates `path` exclusively (never overwrites), readable only by its owner where
/// the platform supports it.
fn create_private(path: &Path) -> io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_only_an_empty_payload_key() {
        let out = render_env("A=1\nNOCTIS_PAYLOAD_KEY=\n# NOCTIS_PAYLOAD_KEY=\nB=2\n").unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "A=1");
        let key = lines[1].strip_prefix("NOCTIS_PAYLOAD_KEY=").unwrap();
        assert_eq!(key.len(), 64);
        assert!(
            key.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert!(noctis::codec::PayloadKey::from_hex(key).is_ok());
        assert_eq!(lines[2], "# NOCTIS_PAYLOAD_KEY=");
        assert_eq!(lines[3], "B=2");
    }

    #[test]
    fn keeps_a_preset_payload_key() {
        let out = render_env("NOCTIS_PAYLOAD_KEY=already-set\n").unwrap();
        assert_eq!(out.as_str(), "NOCTIS_PAYLOAD_KEY=already-set\n");
    }

    #[test]
    fn two_renders_get_different_keys() {
        let a = render_env("NOCTIS_PAYLOAD_KEY=\n").unwrap();
        let b = render_env("NOCTIS_PAYLOAD_KEY=\n").unwrap();
        assert_ne!(*a, *b);
    }

    #[test]
    fn creates_then_loads_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("noctis-bootstrap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        let _ = std::fs::remove_file(&path);

        assert_eq!(load_or_create_env(&path).unwrap(), EnvFile::Created);
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("NOCTIS_TOKEN="));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        assert_eq!(load_or_create_env(&path).unwrap(), EnvFile::Loaded);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
