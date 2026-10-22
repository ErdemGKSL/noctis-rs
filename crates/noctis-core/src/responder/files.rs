//! The attachments of one send (FR-30, AD-22; ATT-01..05, MSG-08).
//!
//! A send uploads or keeps only the attachments its tree references
//! ([`V2Message::used_attachments`](noctis_ui::V2Message::used_attachments)),
//! in name order. On an edit, a name the target message already holds
//! (its [`KnownFiles`]) becomes the kept entry `{"id": "<snowflake>"}`;
//! every other name is a new entry `{"id": n, "filename": name}` uploaded
//! as the multipart part `files[n]` (ATT-04). Names the target holds but
//! the tree no longer uses are left out, so Discord drops them (MSG-08).
//!
//! The Upload Budget (ATT-01..03) is checked here, once, after the entries
//! and the payload JSON exist and before any request: a violation is
//! [`ResponderError::Invalid`] in every mode, and nothing is sent.

use bytes::Bytes;
use noctis_rules::Limits;
use noctis_rules::attachment::{AttachmentDecl, validate_attachments};
use noctis_ui::UiError;
use serde_json::{Map, Value, json};
use twilight_http::request::Form;
use twilight_model::id::Id;
use twilight_model::id::marker::AttachmentMarker;

use super::{OutgoingMessage, ResponderError};

/// The attachments a message already holds on Discord, by filename, in the
/// order Discord listed them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KnownFiles(Vec<(String, Id<AttachmentMarker>)>);

impl KnownFiles {
    /// No known attachment.
    pub(crate) const EMPTY: Self = Self(Vec::new());

    /// The `attachments[]` of a message object. An entry whose `id` is not
    /// a non-zero snowflake string, or whose `filename` is not a string, is
    /// skipped; of two entries with one filename the first wins.
    pub(crate) fn from_message(message: &Value) -> Self {
        Self::from_list(message.get("attachments"))
    }

    /// An `attachments` array (anything else is empty).
    pub(crate) fn from_list(list: Option<&Value>) -> Self {
        let mut known = Self::EMPTY;
        let Some(list) = list.and_then(Value::as_array) else {
            return known;
        };
        for entry in list {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<u64>().ok())
                .and_then(Id::new_checked);
            let filename = entry.get("filename").and_then(Value::as_str);
            if let (Some(id), Some(filename)) = (id, filename)
                && known.get(filename).is_none()
            {
                known.0.push((filename.to_owned(), id));
            }
        }
        known
    }

    /// The id of the attachment named `filename`.
    pub(crate) fn get(&self, filename: &str) -> Option<Id<AttachmentMarker>> {
        self.0
            .iter()
            .find_map(|(name, id)| (name == filename).then_some(*id))
    }
}

/// The `attachment_size_limit` of a raw interaction: a positive JSON
/// integer (saturating at `usize::MAX`), anything else `None`.
pub(crate) fn file_limit(raw: &[u8]) -> Option<usize> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        attachment_size_limit: Option<Value>,
    }
    let limit = serde_json::from_slice::<Raw>(raw)
        .ok()?
        .attachment_size_limit?
        .as_u64()
        .filter(|&n| n > 0)?;
    Some(usize::try_from(limit).unwrap_or(usize::MAX))
}

/// The attachments of one send, planned from the message's used ones.
pub(crate) struct Files<'m> {
    /// The `attachments` entries, in name order.
    entries: Vec<Value>,
    /// `(name, size)` of every used attachment, in name order; a kept one
    /// has size 0 (it is not uploaded again).
    decls: Vec<(&'m str, usize)>,
    /// The new uploads, in `files[n]` order: `(filename, bytes)`.
    uploads: Vec<(&'m str, &'m Bytes)>,
    /// The kept entries.
    kept: KnownFiles,
}

impl<'m> Files<'m> {
    /// The attachments `msg` uses; `known` is the edit target's attachments
    /// (`None` for a new message).
    pub(crate) fn plan(msg: &'m OutgoingMessage, known: Option<&KnownFiles>) -> Self {
        let mut files = Self {
            entries: Vec::new(),
            decls: Vec::new(),
            uploads: Vec::new(),
            kept: KnownFiles::EMPTY,
        };
        let OutgoingMessage::V2(m) = msg else {
            return files;
        };
        let mut used = m.used_attachments().peekable();
        for member in m.attachment_set().iter() {
            if used.peek().is_some_and(|u| u.name() == member.name()) {
                used.next();
            } else {
                tracing::debug!(
                    target: "noctis::responder",
                    name = member.name(),
                    "attachment not referenced by the tree: not sent"
                );
                continue;
            }
            let name = member.name();
            if let Some(id) = known.and_then(|k| k.get(name)) {
                files.entries.push(json!({ "id": id.to_string() }));
                files.decls.push((name, 0));
                files.kept.0.push((name.to_owned(), id));
                continue;
            }
            let mut entry = Map::new();
            entry.insert("id".to_owned(), json!(files.uploads.len()));
            entry.insert("filename".to_owned(), json!(name));
            if let Some(alt) = member.alt_text() {
                entry.insert("description".to_owned(), json!(alt));
            }
            files.entries.push(Value::Object(entry));
            files.decls.push((name, member.bytes().len()));
            files.uploads.push((name, member.bytes()));
        }
        files
    }

    /// Puts the `attachments` array into the message object `body`: always
    /// on an edit (an empty one drops every attachment), on a new message
    /// only when a file is used.
    pub(crate) fn place(&self, body: &mut Value, edit: bool) {
        if !edit && self.entries.is_empty() {
            return;
        }
        if let Value::Object(map) = body {
            map.insert("attachments".to_owned(), Value::Array(self.entries.clone()));
        }
    }

    /// Serializes `payload` (the whole request JSON) and checks the Upload
    /// Budget against `file_limit` (the interaction's
    /// `attachment_size_limit`, else `Limits::attachment_bytes`).
    ///
    /// # Errors
    ///
    /// [`ResponderError::Encode`] when `payload` does not serialize;
    /// [`ResponderError::Invalid`] with every ATT-01..03 violation.
    pub(crate) fn finish(
        self,
        payload: &Value,
        file_limit: Option<usize>,
        limits: &Limits,
    ) -> Result<Body, ResponderError> {
        let json = serde_json::to_vec(payload).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        let decls: Vec<AttachmentDecl<'_>> = self
            .decls
            .iter()
            .map(|&(name, size)| AttachmentDecl::new(name, size))
            .collect();
        validate_attachments(&decls, json.len(), file_limit, limits)
            .map_err(|violations| ResponderError::Invalid(UiError::Limits { violations }))?;
        let payload = if self.uploads.is_empty() {
            Payload::Json(json)
        } else {
            let mut form = Form::new().json_part(b"payload_json", &json);
            for (n, (filename, bytes)) in self.uploads.iter().enumerate() {
                form = form.file_part(format!("files[{n}]").as_bytes(), filename.as_bytes(), bytes);
            }
            Payload::Form(form)
        };
        Ok(Body {
            payload,
            kept: self.kept,
        })
    }
}

/// A request body: JSON, or multipart with uploads.
#[derive(Debug)]
pub(crate) enum Payload {
    /// `application/json`.
    Json(Vec<u8>),
    /// `multipart/form-data`: `payload_json`, then `files[n]`.
    Form(Form),
}

/// A built send: its body and the attachments it keeps.
#[derive(Debug)]
pub(crate) struct Body {
    pub(crate) payload: Payload,
    /// The kept entries: what the target is known to hold after the send
    /// when the response does not say (new uploads stay unknown, so a
    /// later edit uploads them again).
    pub(crate) kept: KnownFiles,
}

impl Body {
    /// Whether the body uploads a file.
    pub(crate) fn uploads(&self) -> bool {
        matches!(self.payload, Payload::Form(_))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

    use noctis_rules::{Bound, LimitMode, RuleId};
    use noctis_ui::{
        Attachment, AttachmentSet, Components, MediaGallery, MediaItem, TextDisplay, V2Message,
    };

    use super::*;

    fn known(pairs: &[(&str, u64)]) -> KnownFiles {
        KnownFiles(
            pairs
                .iter()
                .map(|&(name, id)| (name.to_owned(), Id::new(id)))
                .collect(),
        )
    }

    /// A message using one file of `size` bytes per entry of `sizes`.
    fn files_msg(sizes: &[usize]) -> OutgoingMessage {
        let mut set = AttachmentSet::new();
        let refs: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(i, &size)| {
                let mut bytes = vec![0_u8; size];
                if let Some(first) = bytes.first_mut() {
                    *first = u8::try_from(i).unwrap();
                }
                let digest = [u8::try_from(i).unwrap(); 32];
                set.add(Attachment::with_digest(bytes, digest, "bin").unwrap())
            })
            .collect();
        let items: Vec<MediaItem> = refs
            .into_iter()
            .map(|r| MediaItem::new(r).alt("x"))
            .collect();
        let gallery = MediaGallery::try_new(items, &Limits::default()).unwrap_or_else(|_| {
            MediaGallery::new([MediaItem::new(noctis_ui::Media::url("https://x"))])
        });
        let body = Components::new().push(TextDisplay::new("t"));
        let body = if sizes.is_empty() {
            body
        } else {
            body.push(gallery)
        };
        V2Message::new(body).attachments(set).into()
    }

    /// The budget result of `msg` as a fresh message.
    fn budget(msg: &OutgoingMessage, file_limit: Option<usize>) -> Result<Body, ResponderError> {
        Files::plan(msg, None).finish(&json!({}), file_limit, &Limits::default())
    }

    fn violations(result: Result<Body, ResponderError>) -> Vec<noctis_rules::Violation> {
        match result {
            Err(ResponderError::Invalid(UiError::Limits { violations })) => violations,
            other => panic!("expected a Limits error, got {other:?}"),
        }
    }

    #[test]
    fn msg_08_known_files_from_message() {
        let message = json!({ "attachments": [
            { "id": "11", "filename": "a.png" },
            { "id": "0", "filename": "zero.png" },
            { "id": 12, "filename": "number.png" },
            { "id": "13" },
            { "id": "14", "filename": "a.png" },
            { "id": "15", "filename": "c.png" },
        ] });
        assert_eq!(
            KnownFiles::from_message(&message),
            known(&[("a.png", 11), ("c.png", 15)])
        );
        assert_eq!(KnownFiles::from_message(&json!({})), KnownFiles::EMPTY);
        assert_eq!(
            KnownFiles::from_message(&json!({ "attachments": "x" })),
            KnownFiles::EMPTY
        );
        assert_eq!(KnownFiles::from_message(&Value::Null), KnownFiles::EMPTY);
    }

    #[test]
    fn att_04_new_files_are_numbered_parts() {
        let msg = files_msg(&[1, 2]);
        let files = Files::plan(&msg, None);
        let names: Vec<&str> = files.uploads.iter().map(|(n, _)| *n).collect();
        assert_eq!(files.entries.len(), 2);
        assert_eq!(files.entries[0]["id"], 0);
        assert_eq!(files.entries[1]["id"], 1);
        assert_eq!(files.entries[0]["filename"], names[0]);
        assert_eq!(files.entries[1]["filename"], names[1]);
        let mut body = json!({});
        files.place(&mut body, false);
        assert_eq!(body["attachments"].as_array().unwrap().len(), 2);
        let built = files.finish(&body, None, &Limits::default()).unwrap();
        assert!(built.uploads());
        let Payload::Form(form) = built.payload else {
            panic!("multipart");
        };
        let raw = form.build();
        let text = String::from_utf8_lossy(&raw);
        let payload = text.find("name=\"payload_json\"").unwrap();
        let first = text.find("name=\"files[0]\"").unwrap();
        let second = text.find("name=\"files[1]\"").unwrap();
        assert!(payload < first && first < second, "{text}");
        assert!(text.contains(&format!("filename=\"{}\"", names[0])));
    }

    #[test]
    fn msg_08_edit_keeps_known_and_uploads_the_rest() {
        let msg = files_msg(&[1, 2]);
        let OutgoingMessage::V2(m) = &msg else {
            unreachable!()
        };
        let names: Vec<String> = m.used_attachments().map(|a| a.name().to_owned()).collect();
        let target = known(&[(&names[0], 11), ("gone.png", 12)]);
        let files = Files::plan(&msg, Some(&target));
        assert_eq!(
            files.entries,
            [
                json!({ "id": "11" }),
                json!({ "id": 0, "filename": names[1] }),
            ]
        );
        assert_eq!(files.kept, known(&[(&names[0], 11)]));
        // Every name known: JSON, no upload.
        let all = known(&[(&names[0], 11), (&names[1], 13)]);
        let built = Files::plan(&msg, Some(&all))
            .finish(&json!({}), None, &Limits::default())
            .unwrap();
        assert!(!built.uploads());
    }

    #[test]
    fn msg_08_edit_without_files_sends_an_empty_list() {
        let msg = files_msg(&[]);
        let mut body = json!({});
        Files::plan(&msg, Some(&known(&[("a.png", 11)]))).place(&mut body, true);
        assert_eq!(body["attachments"], json!([]));
        let mut create = json!({});
        Files::plan(&msg, None).place(&mut create, false);
        assert!(create.get("attachments").is_none());
    }

    #[test]
    fn att_01_file_size_boundary() {
        assert!(budget(&files_msg(&[20_971_520]), None).is_ok());
        let errs = violations(budget(&files_msg(&[20_971_521]), None));
        assert_eq!(errs.len(), 1);
        assert_eq!(
            (errs[0].rule, errs[0].path.as_str()),
            (RuleId::ATT_01, "attachments[0].size")
        );
        assert!(
            errs[0].notes[0].starts_with("file `"),
            "{:?}",
            errs[0].notes
        );
    }

    #[test]
    fn att_01_interaction_limit_lower_and_higher() {
        let errs = violations(budget(&files_msg(&[10_485_761]), Some(10_485_760)));
        assert_eq!(errs[0].rule, RuleId::ATT_01);
        assert_eq!(errs[0].limit, Bound::AtMost(10_485_760));
        let errs = violations(budget(&files_msg(&[31_457_280]), Some(52_428_800)));
        assert_eq!(errs.len(), 1);
        assert_eq!(
            (errs[0].rule, errs[0].path.as_str()),
            (RuleId::ATT_02, "request")
        );
    }

    #[test]
    fn att_01_kept_files_are_exempt() {
        let msg = files_msg(&[10_485_760, 10_485_760, 10_485_760]);
        let OutgoingMessage::V2(m) = &msg else {
            unreachable!()
        };
        let target = KnownFiles(
            m.used_attachments()
                .zip(1_u64..)
                .map(|(a, id)| (a.name().to_owned(), Id::new(id)))
                .collect(),
        );
        let built = Files::plan(&msg, Some(&target))
            .finish(&json!({}), None, &Limits::default())
            .unwrap();
        assert!(!built.uploads());
    }

    #[test]
    fn att_02_request_size_boundary() {
        // `{}` is 2 bytes of JSON.
        assert!(budget(&files_msg(&[13_107_200, 13_107_198]), None).is_ok());
        let errs = violations(budget(&files_msg(&[13_107_200, 13_107_199]), None));
        assert_eq!(
            (errs[0].rule, errs[0].path.as_str()),
            (RuleId::ATT_02, "request")
        );
    }

    #[test]
    fn att_03_file_count() {
        let mut set = AttachmentSet::new();
        let refs: Vec<_> = (0_u8..11)
            .map(|i| set.add(Attachment::with_digest(vec![i], [i; 32], "png").unwrap()))
            .collect();
        let gallery = |refs: &[noctis_ui::AttachmentRef]| {
            MediaGallery::try_new(
                refs.iter().map(|r| MediaItem::new(r.clone()).alt("x")),
                &Limits::default(),
            )
            .unwrap()
        };
        let ten =
            V2Message::new(Components::new().push(gallery(&refs[..10]))).attachments(set.clone());
        assert!(budget(&ten.into(), None).is_ok());
        let eleven = V2Message::new(
            Components::new()
                .push(gallery(&refs[..10]))
                .push(gallery(&refs[10..])),
        )
        .attachments(set);
        let msg: OutgoingMessage = eleven.into();
        let OutgoingMessage::V2(m) = &msg else {
            unreachable!()
        };
        // Kept and new mixed: the count is of used attachments.
        let first = m.used_attachments().next().unwrap().name().to_owned();
        let target = known(&[(&first, 9)]);
        let errs = violations(Files::plan(&msg, Some(&target)).finish(
            &json!({}),
            None,
            &Limits::default(),
        ));
        assert_eq!(
            (errs[0].rule, errs[0].path.as_str()),
            (RuleId::ATT_03, "attachments")
        );
    }

    #[test]
    fn att_01_lenient_never_drops_a_file() {
        let mut limits = Limits::default();
        limits.mode = LimitMode::Lenient;
        let msg = files_msg(&[22_020_096]);
        let errs = violations(Files::plan(&msg, None).finish(&json!({}), None, &limits));
        assert_eq!(errs[0].rule, RuleId::ATT_01);
    }

    #[test]
    fn att_01_file_limit_parse() {
        let parse =
            |v: &str| file_limit(format!(r#"{{"type":3,"attachment_size_limit":{v}}}"#).as_bytes());
        assert_eq!(file_limit(br#"{"type":3}"#), None);
        assert_eq!(parse("0"), None);
        assert_eq!(parse("-1"), None);
        assert_eq!(parse("1.5"), None);
        assert_eq!(parse(r#""5""#), None);
        assert_eq!(parse("null"), None);
        assert_eq!(parse("10485760"), Some(10_485_760));
        assert_eq!(
            parse(&u64::MAX.to_string()),
            Some(usize::try_from(u64::MAX).unwrap_or(usize::MAX))
        );
        assert_eq!(file_limit(b"not json"), None);
    }
}
