//! [`MessageBody`]: the JSON of one send or edit, borrowed from a message.

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use super::flags::IS_COMPONENTS_V2;
use super::{LegacyMessage, V2Message};
use crate::wire::WiredSeq;

/// The JSON body of one send or edit, borrowed from a [`V2Message`] or a
/// [`LegacyMessage`]; core's Responder serializes it.
///
/// It holds `flags`, `components` and `allowed_mentions` (plus `content` for
/// a legacy message, and the legacy clears of a legacy-to-V2 edit); never
/// `attachments` or `files`, which the Responder adds. Serializing a body
/// with an unresolved Route `custom_id` is an error.
///
/// ```
/// use noctis_ui::{CallbackFlags, Components, TextDisplay, V2Message};
///
/// let msg = V2Message::new(Components::new().push(TextDisplay::new("hi")));
/// let json = serde_json::to_value(msg.callback_body(CallbackFlags::EPHEMERAL)).unwrap();
/// assert_eq!(json["flags"], 32832);
/// ```
#[must_use = "a body does nothing until it is serialized"]
#[derive(Debug, Clone)]
pub struct MessageBody<'a>(Inner<'a>);

#[derive(Debug, Clone)]
enum Inner<'a> {
    V2 {
        msg: &'a V2Message,
        flags: u64,
        clear: Clear,
    },
    Legacy {
        msg: &'a LegacyMessage,
        flags: Option<u64>,
    },
}

/// The legacy fields a V2 body sends empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Clear {
    /// None: a send, or an edit of a V2 message.
    Nothing,
    /// `content`, `embeds` and `poll`: an edit of a message that may be
    /// legacy (CV2-02, WH-03). Never `sticker_ids`: stickers cannot be
    /// edited, and Edit Message rejects the key with 50080 even when empty.
    Legacy,
}

impl<'a> MessageBody<'a> {
    pub(super) fn v2(msg: &'a V2Message, flags: u64, clear: Clear) -> Self {
        Self(Inner::V2 {
            msg,
            flags: flags | IS_COMPONENTS_V2,
            clear,
        })
    }

    pub(super) fn legacy(msg: &'a LegacyMessage, flags: Option<u64>) -> Self {
        Self(Inner::Legacy { msg, flags })
    }
}

/// An always-empty JSON array.
const NO_ITEMS: [(); 0] = [];

impl Serialize for MessageBody<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match &self.0 {
            Inner::V2 { msg, flags, clear } => {
                map.serialize_entry("flags", flags)?;
                if *clear == Clear::Legacy {
                    map.serialize_entry("content", &())?;
                    map.serialize_entry("embeds", &NO_ITEMS)?;
                    map.serialize_entry("poll", &())?;
                }
                map.serialize_entry("components", msg.components())?;
                map.serialize_entry("allowed_mentions", msg.allowed_mentions())?;
            }
            Inner::Legacy { msg, flags } => {
                crate::wire::opt(&mut map, "flags", flags.as_ref())?;
                map.serialize_entry("content", msg.content_text())?;
                map.serialize_entry("components", &WiredSeq(&msg.rows, None))?;
                map.serialize_entry("allowed_mentions", msg.allowed_mentions())?;
            }
        }
        map.end()
    }
}
