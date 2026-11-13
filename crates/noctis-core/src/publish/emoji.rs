//! The emoji sync of a Publish (FR-82, AD-33; EMO-01, EMO-02, EMO-04):
//! after the Commands, list the application's emojis and create every
//! declared name that is missing, one request at a time through the App's
//! HTTP client and its rate limiter.
//!
//! Changed art is found by a blake3 digest of the uploaded bytes kept in
//! the App's `Kv` under `noctis:emoji:<name>`, with the emoji's id: Discord
//! returns no hash and re-encodes the image. An emoji with no stored digest
//! (or one stored for another id) is adopted as unchanged. Every check and
//! every render runs before the first write, so a failed plan writes
//! nothing; a sync that fails half way is safe to repeat, because the next
//! list finds what was created.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use twilight_http::routing::Route;

use super::{PublishError, send};
use crate::App;
use crate::emoji::{DeclaredEmoji, EmojiImage, parse_emoji_list};

/// The `Kv` key prefix of the stored digests.
const KEY_PREFIX: &str = "noctis:emoji:";

/// What the emoji sync of a Publish did, or would do in a dry run, by
/// emoji name: [`PublishReport::emojis`](super::PublishReport::emojis).
///
/// Declared names keep declaration order; deleted names are sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmojiReport {
    created: Vec<String>,
    unchanged: Vec<String>,
    changed: Vec<String>,
    replaced: Vec<String>,
    deleted: Vec<String>,
    wrote: bool,
}

impl EmojiReport {
    /// The declared names the application lacked: created (or, in a dry
    /// run, to create).
    pub fn created(&self) -> &[String] {
        &self.created
    }

    /// The declared names the application holds with the same art (or
    /// with no stored digest yet: adopted).
    pub fn unchanged(&self) -> &[String] {
        &self.unchanged
    }

    /// The declared names whose art changed and that were kept, because
    /// [`EmojiSync::replace_changed`](crate::emoji::EmojiSync::replace_changed)
    /// is off.
    pub fn changed(&self) -> &[String] {
        &self.changed
    }

    /// The declared names whose art changed and that were deleted and
    /// re-created (a new id).
    pub fn replaced(&self) -> &[String] {
        &self.replaced
    }

    /// The application's emojis no set declares that were deleted
    /// ([`EmojiSync::delete_undeclared`](crate::emoji::EmojiSync::delete_undeclared)).
    pub fn deleted(&self) -> &[String] {
        &self.deleted
    }

    /// Whether any emoji was created or deleted.
    pub fn wrote(&self) -> bool {
        self.wrote
    }

    /// Whether the sync has (or, in a dry run, would have) anything to
    /// write.
    pub fn has_writes(&self) -> bool {
        !(self.created.is_empty() && self.replaced.is_empty() && self.deleted.is_empty())
    }
}

impl fmt::Display for EmojiReport {
    /// One line per name: `+ create`, `~ replace`, `~ changed (kept)`,
    /// `- delete`, then the unchanged count.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for name in &self.created {
            writeln!(f, "+ create emoji `{name}`")?;
        }
        for name in &self.replaced {
            writeln!(f, "~ replace emoji `{name}` (new art; its id changes)")?;
        }
        for name in &self.changed {
            writeln!(
                f,
                "~ changed emoji `{name}` (kept: EmojiSync::replace_changed is off)"
            )?;
        }
        for name in &self.deleted {
            writeln!(f, "- delete emoji `{name}` (not declared)")?;
        }
        write!(f, "{} emoji(s) unchanged", self.unchanged.len())
    }
}

/// The digest Publish keeps per uploaded emoji.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Stored {
    id: u64,
    digest: [u8; 32],
}

/// One declared emoji's upload: its bytes, media type and digest.
struct Prepared {
    name: &'static str,
    bytes: Cow<'static, [u8]>,
    mime: &'static str,
    digest: [u8; 32],
}

impl Prepared {
    /// The `image` data URI of the create request.
    fn data_uri(&self) -> String {
        let mut uri = format!("data:{};base64,", self.mime);
        base64_into(&self.bytes, &mut uri);
        uri
    }
}

/// Fails before any request when a drawn emoji is declared and the App has
/// no Renderer.
pub(super) fn check_renderer(app: &App) -> Result<(), PublishError> {
    if app.shared().renderer.is_none()
        && let Some(emoji) = app.emojis().declarations().first_rendered()
    {
        return Err(PublishError::EmojiNeedsRenderer { name: emoji.name() });
    }
    Ok(())
}

/// Syncs the declared emojis of `app` into the application `application`;
/// `None` when the App declares none (nothing is sent).
pub(super) async fn sync(
    app: &App,
    application: u64,
    dry_run: bool,
) -> Result<Option<EmojiReport>, PublishError> {
    let emojis = app.emojis();
    let declarations = emojis.declarations();
    if declarations.is_empty() {
        return Ok(None);
    }
    emojis.note_application(application);
    let http = app.http();
    let shared = app.shared();
    let policy = emojis.sync_policy();
    let label = format!("GET /applications/{application}/emojis");
    let body = send(
        http,
        &Route::GetApplicationEmojis {
            application_id: application,
        },
        &label,
        None,
    )
    .await?;
    let remote = parse_emoji_list(&body).map_err(|reason| PublishError::InvalidResponse {
        route: label,
        reason,
    })?;

    // Every image and digest before the first write.
    let mut prepared = Vec::with_capacity(declarations.len());
    for emoji in declarations.all() {
        prepared.push(prepare(shared, emoji).await?);
    }

    let kv = crate::Kv::new(Arc::clone(&shared.kv));
    let mut report = EmojiReport::default();
    let mut creates: Vec<&Prepared> = Vec::new();
    let mut replaces: Vec<(&Prepared, u64)> = Vec::new();
    let mut adopted: Vec<(&Prepared, u64)> = Vec::new();
    for item in &prepared {
        let Some(listed) = remote.get(item.name) else {
            report.created.push(item.name.to_owned());
            creates.push(item);
            continue;
        };
        let id = listed.id.get();
        let stored = match kv.get::<Stored>(&key(item.name)).await {
            Ok(stored) => stored.filter(|s| s.id == id),
            Err(error) => {
                tracing::warn!(
                    target: "noctis::publish",
                    emoji = item.name,
                    %error,
                    "could not read the stored emoji digest; the emoji counts as unchanged"
                );
                None
            }
        };
        match stored {
            Some(stored) if stored.digest != item.digest => {
                if policy.replaces_changed() {
                    report.replaced.push(item.name.to_owned());
                    replaces.push((item, id));
                } else {
                    report.changed.push(item.name.to_owned());
                }
            }
            Some(_) => report.unchanged.push(item.name.to_owned()),
            None => {
                report.unchanged.push(item.name.to_owned());
                adopted.push((item, id));
            }
        }
    }
    let mut deletes: Vec<(&str, u64)> = if policy.deletes_undeclared() {
        remote
            .iter()
            .filter(|(name, _)| declarations.get(name).is_none())
            .map(|(name, listed)| (name.as_str(), listed.id.get()))
            .collect()
    } else {
        Vec::new()
    };
    deletes.sort_unstable();
    report.deleted = deletes.iter().map(|(name, _)| (*name).to_owned()).collect();

    // EMO-01: the application after the sync, before the first write.
    let max = shared.limits.application_emojis;
    let count = remote.len() - deletes.len() + creates.len();
    if count > max {
        return Err(PublishError::TooManyEmojis { count, max });
    }

    if dry_run {
        tracing::info!(target: "noctis::publish", plan = %report, "dry run: emoji sync planned; nothing written");
        return Ok(Some(report));
    }
    let writes = Writes {
        adopted,
        deletes,
        replaces,
        creates,
    };
    apply(http, &kv, application, writes, &mut report).await?;
    Ok(Some(report))
}

/// The writes of a planned sync, in the order they are sent.
struct Writes<'a> {
    /// Listed emojis with no stored digest: store one.
    adopted: Vec<(&'a Prepared, u64)>,
    /// Undeclared emojis to delete.
    deletes: Vec<(&'a str, u64)>,
    /// Changed emojis to delete and re-create.
    replaces: Vec<(&'a Prepared, u64)>,
    /// Missing names to create.
    creates: Vec<&'a Prepared>,
}

/// Sends `writes` one request at a time: deletes first (they make room),
/// then replacements, then creates.
async fn apply(
    http: &twilight_http::Client,
    kv: &crate::Kv,
    application: u64,
    writes: Writes<'_>,
    report: &mut EmojiReport,
) -> Result<(), PublishError> {
    for (item, id) in writes.adopted {
        store(kv, item.name, id, item.digest).await;
    }
    for (name, id) in writes.deletes {
        delete(http, application, name, id).await?;
        report.wrote = true;
        if let Err(error) = kv.delete(&key(name)).await {
            tracing::debug!(target: "noctis::publish", emoji = name, %error, "stored digest not deleted");
        }
    }
    for (item, id) in writes.replaces {
        delete(http, application, item.name, id).await?;
        report.wrote = true;
        create(http, kv, application, item).await?;
    }
    for item in writes.creates {
        create(http, kv, application, item).await?;
        report.wrote = true;
    }
    if report.has_writes() {
        tracing::info!(target: "noctis::publish", sync = %report, "emojis synced");
    } else {
        tracing::info!(target: "noctis::publish", "emojis unchanged; nothing written");
    }
    Ok(())
}

/// The bytes, media type and digest of one declared emoji: its embedded
/// file, or its art rendered through the App's Renderer.
async fn prepare(
    shared: &Arc<crate::handler::AppShared>,
    emoji: &DeclaredEmoji,
) -> Result<Prepared, PublishError> {
    let name = emoji.name();
    let (bytes, mime): (Cow<'static, [u8]>, _) = match *emoji.image() {
        EmojiImage::File { bytes, format } => (Cow::Borrowed(bytes), format.mime()),
        EmojiImage::Render(art) => {
            let report = crate::render::render_emoji(shared, art(emoji.tone()))
                .await
                .map_err(|reason| PublishError::EmojiArt { name, reason })?;
            let mime = match report.format {
                noctis_ports::OutputFormat::Png => "image/png",
                noctis_ports::OutputFormat::Gif => "image/gif",
                other => {
                    return Err(PublishError::EmojiArt {
                        name,
                        reason: format!(
                            "[EMO-04] the Renderer returned {}, but an emoji is uploaded as PNG, \
                             JPEG or GIF",
                            other.extension()
                        ),
                    });
                }
            };
            (Cow::Owned(report.attachment.bytes().to_vec()), mime)
        }
    };
    let digest = *blake3::hash(&bytes).as_bytes();
    Ok(Prepared {
        name,
        bytes,
        mime,
        digest,
    })
}

/// The `Kv` key of `name`'s stored digest.
fn key(name: &str) -> String {
    format!("{KEY_PREFIX}{name}")
}

/// Stores `name`'s digest; a failure only logs (the next Publish adopts
/// the emoji again).
async fn store(kv: &crate::Kv, name: &str, id: u64, digest: [u8; 32]) {
    if let Err(error) = kv.put(&key(name), &Stored { id, digest }, None).await {
        tracing::warn!(
            target: "noctis::publish",
            emoji = name,
            %error,
            "could not store the emoji digest; a later art change is not detected"
        );
    }
}

/// `POST /applications/{id}/emojis` for `item`, then stores its digest. A
/// 400 is [`PublishError::EmojiRejected`].
async fn create(
    http: &twilight_http::Client,
    kv: &crate::Kv,
    application: u64,
    item: &Prepared,
) -> Result<(), PublishError> {
    let label = format!("POST /applications/{application}/emojis");
    let body = serde_json::to_vec(&serde_json::json!({
        "name": item.name,
        "image": item.data_uri(),
    }))
    .map_err(|_| PublishError::Transport {
        route: label.clone(),
    })?;
    let route = Route::AddApplicationEmoji {
        application_id: application,
    };
    let answer = match send(http, &route, &label, Some(body)).await {
        Ok(answer) => answer,
        Err(PublishError::Rejected {
            status: 400, code, ..
        }) => {
            return Err(PublishError::EmojiRejected {
                name: item.name,
                status: 400,
                code,
            });
        }
        Err(error) => return Err(error),
    };
    tracing::info!(target: "noctis::publish", emoji = item.name, "emoji created");
    if let Some(id) = created_id(&answer) {
        store(kv, item.name, id, item.digest).await;
    } else {
        tracing::warn!(
            target: "noctis::publish",
            emoji = item.name,
            "the created emoji has no id in the answer; its digest is not stored"
        );
    }
    Ok(())
}

/// `DELETE /applications/{id}/emojis/{emoji}`.
async fn delete(
    http: &twilight_http::Client,
    application: u64,
    name: &str,
    emoji: u64,
) -> Result<(), PublishError> {
    let label = format!("DELETE /applications/{application}/emojis/{emoji}");
    let route = Route::DeleteApplicationEmoji {
        application_id: application,
        emoji_id: emoji,
    };
    send(http, &route, &label, None).await?;
    tracing::info!(target: "noctis::publish", emoji = name, "emoji deleted");
    Ok(())
}

/// The `id` of a created emoji object.
fn created_id(body: &[u8]) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    match value.get("id")? {
        serde_json::Value::String(id) => id.parse().ok(),
        serde_json::Value::Number(id) => id.as_u64(),
        _ => None,
    }
    .filter(|&id| id != 0)
}

/// Appends standard, padded base64 of `bytes` to `out`.
fn base64_into(bytes: &[u8], out: &mut String) {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.reserve(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> shift) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(bytes: &[u8]) -> String {
        let mut out = String::new();
        base64_into(bytes, &mut out);
        out
    }

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(b64(input.as_bytes()), expected, "{input}");
        }
        assert_eq!(b64(&[0xFF, 0xFE, 0xFD]), "//79");
    }

    #[test]
    fn created_id_reads_strings_and_numbers() {
        assert_eq!(created_id(br#"{"id": "42"}"#), Some(42));
        assert_eq!(created_id(br#"{"id": 43}"#), Some(43));
        assert_eq!(created_id(br#"{"id": "0"}"#), None);
        assert_eq!(created_id(b"nope"), None);
    }

    #[test]
    fn report_display_lists_every_change() {
        let report = EmojiReport {
            created: vec!["check".into()],
            unchanged: vec!["cross".into()],
            changed: vec!["spin".into()],
            replaced: vec!["dot".into()],
            deleted: vec!["old".into()],
            wrote: true,
        };
        assert_eq!(
            report.to_string(),
            "+ create emoji `check`\n~ replace emoji `dot` (new art; its id changes)\n~ changed \
             emoji `spin` (kept: EmojiSync::replace_changed is off)\n- delete emoji `old` (not \
             declared)\n1 emoji(s) unchanged"
        );
        assert!(report.has_writes() && report.wrote());
        assert!(!EmojiReport::default().has_writes());
    }
}
