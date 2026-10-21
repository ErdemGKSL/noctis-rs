//! Select freshness (FR-25): the nonces an interaction already carries and
//! the draw that avoids them.
//!
//! A client keys a select's local state by its `custom_id`, so a re-render
//! whose select ids repeat the nonce the client holds would keep the user's
//! old local selection instead of the render's declared defaults. Each send
//! therefore draws its nonce outside a small avoid-set: the nonces found in
//! the raw interaction (the clicked select's `data` and the source
//! `message`'s components) and every nonce this run already drew.

use noctis_codec::{MAX_ID_LEN, SelectNonce};

use super::ResponderError;

/// How many nonces a [`NonceSet`] keeps.
const CAPACITY: usize = 8;

/// A fixed-capacity set of select nonces: a ring that overwrites its oldest
/// entry when full, so it never allocates.
///
/// Every select of one send shares one nonce, so an interaction carries one
/// or two in practice; eight is generous.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NonceSet {
    slots: [Option<SelectNonce>; CAPACITY],
    /// The slot the next new nonce goes to.
    next: usize,
}

impl NonceSet {
    /// The empty set.
    pub(crate) const EMPTY: Self = Self {
        slots: [None; CAPACITY],
        next: 0,
    };

    /// Whether `nonce` is in the set.
    pub(crate) fn contains(&self, nonce: SelectNonce) -> bool {
        self.slots.contains(&Some(nonce))
    }

    /// Adds `nonce`; a nonce already present is not stored twice, and a
    /// full set drops its oldest entry.
    pub(crate) fn insert(&mut self, nonce: SelectNonce) {
        if self.contains(nonce) {
            return;
        }
        if let Some(slot) = self.slots.get_mut(self.next) {
            *slot = Some(nonce);
        }
        self.next = (self.next + 1) % CAPACITY;
    }

    /// The nonces in the set, in no particular order.
    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = SelectNonce> + '_ {
        self.slots.iter().flatten().copied()
    }
}

/// The `"custom_id"` key as it appears in raw JSON.
const KEY: &[u8] = b"\"custom_id\"";

/// The select nonces of every Noctis `custom_id` in a raw interaction.
///
/// It scans the bytes for each `"custom_id"` key (an unescaped quote in JSON
/// only ever delimits a string, so this cannot match inside a string
/// value), reads the string after its `:`, and decodes it with
/// `noctis_codec::decode`. Ids that are not Noctis ids, malformed or
/// without `N` are ignored; malformed JSON gives what was found before it.
/// Nothing is allocated.
pub(crate) fn nonces_in(raw: &[u8]) -> NonceSet {
    let mut set = NonceSet::default();
    let mut rest = raw;
    while let Some(at) = find(rest, KEY) {
        rest = rest.get(at + KEY.len()..).unwrap_or_default();
        let Some(after_colon) = skip_ws(rest)
            .strip_prefix(b":")
            .map(skip_ws)
            .and_then(|r| r.strip_prefix(b"\""))
        else {
            continue;
        };
        let mut buf = [0_u8; MAX_ID_LEN];
        let (value, tail) = read_string(after_colon, &mut buf);
        rest = tail;
        if let Some(nonce) = value
            .and_then(|id| noctis_codec::decode(id).ok())
            .and_then(|decoded| decoded.nonce())
        {
            set.insert(nonce);
        }
    }
    set
}

/// The first index of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// `bytes` without its leading JSON whitespace.
fn skip_ws(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        .unwrap_or(bytes.len());
    bytes.get(start..).unwrap_or_default()
}

/// Reads a JSON string body (after its opening quote) into `buf`.
///
/// Returns the unescaped text when it is at most `MAX_ID_LEN` ASCII bytes
/// with only `\"`, `\\` and `\/` escapes (the only ones a Noctis id can
/// need), and the bytes after the closing quote; an unterminated string
/// gives `None` and nothing after it.
fn read_string<'r, 'b>(
    bytes: &'r [u8],
    buf: &'b mut [u8; MAX_ID_LEN],
) -> (Option<&'b str>, &'r [u8]) {
    let mut len = 0;
    let mut valid = true;
    let mut iter = bytes.iter().enumerate();
    while let Some((i, &b)) = iter.next() {
        let c = match b {
            b'"' => {
                let tail = bytes.get(i + 1..).unwrap_or_default();
                let text = buf
                    .get(..len)
                    .filter(|_| valid)
                    .and_then(|t| core::str::from_utf8(t).ok());
                return (text, tail);
            }
            b'\\' => match iter.next() {
                Some((_, &e @ (b'"' | b'\\' | b'/'))) => e,
                Some(_) => {
                    valid = false;
                    continue;
                }
                None => break,
            },
            _ => b,
        };
        match buf.get_mut(len) {
            Some(slot) if c.is_ascii() => {
                *slot = c;
                len += 1;
            }
            _ => valid = false,
        }
    }
    (None, &[])
}

/// One select nonce for a send, drawn uniformly in `0..=SelectNonce::MAX`
/// outside `avoid` (rejection sampling over two bytes from `fill`, the OS
/// CSPRNG in production).
///
/// # Errors
///
/// [`ResponderError::Encode`] when `fill` fails.
pub(crate) fn draw_nonce(
    avoid: &NonceSet,
    mut fill: impl FnMut(&mut [u8; 2]) -> Result<(), getrandom::Error>,
) -> Result<SelectNonce, ResponderError> {
    let span = u32::from(SelectNonce::MAX) + 1;
    // The largest multiple of `span` that fits in 16 bits.
    let limit = (u32::from(u16::MAX) + 1) / span * span;
    loop {
        let mut bytes = [0_u8; 2];
        fill(&mut bytes).map_err(|error| ResponderError::Encode {
            reason: format!("no select nonce could be drawn: {error}"),
        })?;
        let value = u32::from(u16::from_le_bytes(bytes));
        if value < limit
            && let Ok(n) = u16::try_from(value % span)
            && let Some(nonce) = SelectNonce::new(n)
            && !avoid.contains(nonce)
        {
            return Ok(nonce);
        }
    }
}

/// [`draw_nonce`] from the OS CSPRNG.
pub(crate) fn fill_os(bytes: &mut [u8; 2]) -> Result<(), getrandom::Error> {
    getrandom::fill(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(value: u16) -> SelectNonce {
        SelectNonce::new(value).expect("in range")
    }

    /// A select id of Route `pick` (Payload `page: 1`) carrying `nonce`;
    /// layout `F route ":" N body`, `F` = 4 (the `N` flag).
    fn select_id(nonce: SelectNonce) -> String {
        format!("4pick:{nonce}1")
    }

    fn set(raw: &str) -> Vec<u16> {
        let mut v: Vec<u16> = nonces_in(raw.as_bytes())
            .iter()
            .map(SelectNonce::get)
            .collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn select_id_layout_decodes() {
        let id = select_id(n(371));
        let decoded = noctis_codec::decode(&id).expect("a Noctis id");
        assert_eq!(decoded.nonce(), Some(n(371)));
    }

    #[test]
    fn n1_nothing_to_find() {
        assert!(set("").is_empty());
        assert!(set("not json").is_empty());
        assert!(set(r#"{"data":{"name":"x"}}"#).is_empty());
    }

    #[test]
    fn n2_select_data() {
        let raw = format!(r#"{{"data":{{"custom_id":"{}"}}}}"#, select_id(n(10)));
        assert_eq!(set(&raw), [10]);
    }

    #[test]
    fn n3_spaced_key() {
        let raw = format!("{{\"custom_id\" :\n \"{}\"}}", select_id(n(11)));
        assert_eq!(set(&raw), [11]);
    }

    #[test]
    fn n4_button_data_and_message_select() {
        let raw = format!(
            r#"{{"data":{{"custom_id":"0shop.buy:1"}},"message":{{"components":[{{"type":1,"components":[{{"type":3,"custom_id":"{}"}}]}}]}}}}"#,
            select_id(n(12))
        );
        assert_eq!(set(&raw), [12]);
    }

    #[test]
    fn n5_foreign_and_malformed_ids() {
        let raw = r#"{"a":{"custom_id":"legacy-button"},"b":{"custom_id":"9??"},"c":{"custom_id":"0x:1"},"d":{"custom_id":""}}"#;
        assert!(set(raw).is_empty());
    }

    #[test]
    fn n6_truncated_input() {
        assert!(set(r#"{"custom_id":"4pick:ab"#).is_empty());
        assert!(set(r#"{"custom_id""#).is_empty());
        assert!(set(r#"{"custom_id":"#).is_empty());
        assert!(set(r#"{"custom_id":4"#).is_empty());
        assert!(set(r#"{"custom_id":"4pick\"#).is_empty());
    }

    #[test]
    fn n7_escaped_key_in_content() {
        let raw = format!(r#"{{"content":"\"custom_id\":\"{}\""}}"#, select_id(n(13)));
        assert!(set(&raw).is_empty());
    }

    #[test]
    fn n8_same_nonce_once() {
        let id = select_id(n(14));
        let raw = format!(r#"[{{"custom_id":"{id}"}},{{"custom_id":"{id}"}}]"#);
        let found = nonces_in(raw.as_bytes());
        assert_eq!(found.iter().count(), 1);
    }

    #[test]
    fn n9_ring_keeps_the_last_eight() {
        let raw: Vec<String> = (100..110)
            .map(|v| format!(r#"{{"custom_id":"{}"}}"#, select_id(n(v))))
            .collect();
        assert_eq!(set(&raw.join(",")), (102..110).collect::<Vec<_>>());
    }

    #[test]
    fn n10_modal_submit() {
        let raw = format!(
            r#"{{"type":5,"data":{{"custom_id":"0mod.report:","components":[{{"type":18,"component":{{"type":4,"custom_id":"reason","value":"x"}}}}]}},"message":{{"components":[{{"type":1,"components":[{{"type":3,"custom_id":"{}"}}]}}]}}}}"#,
            select_id(n(15))
        );
        assert_eq!(set(&raw), [15]);
    }

    #[test]
    fn json_escapes_in_an_id_are_read() {
        let raw = r#"{"custom_id":"4pick:\/ab"}"#;
        // `\/` unescapes to `/`; the id is read (whether it decodes is the
        // codec's business) and nothing panics.
        let _ = nonces_in(raw.as_bytes());
        // Other escapes never occur in a Noctis id: the id is skipped.
        assert!(set(r#"{"custom_id":"4pick:\tab1"}"#).is_empty());
        let escaped_quote = format!(
            r#"{{"custom_id":"4pick:ab\"x"}},{{"custom_id":"{}"}}"#,
            select_id(n(16))
        );
        // The escaped quote does not end the first id, which unescapes to
        // `4pick:ab"x` and carries nonce `ab`; the scan then finds the second.
        let ab = noctis_codec::decode("4pick:ab1")
            .ok()
            .and_then(|d| d.nonce())
            .expect("nonce");
        let mut expected = vec![16, ab.get()];
        expected.sort_unstable();
        assert_eq!(set(&escaped_quote), expected);
    }

    /// A `fill` that yields `values` (little-endian) in order.
    fn scripted(values: &[u16]) -> impl FnMut(&mut [u8; 2]) -> Result<(), getrandom::Error> + '_ {
        let mut it = values.iter();
        move |bytes| {
            *bytes = it.next().expect("enough draws").to_le_bytes();
            Ok(())
        }
    }

    #[test]
    fn d1_rejects_values_over_the_limit() {
        let nonce = draw_nonce(&NonceSet::default(), scripted(&[u16::MAX, 7]));
        assert_eq!(nonce.map(SelectNonce::get).ok(), Some(7));
    }

    #[test]
    fn d2_skips_avoided_nonces() {
        let mut avoid = NonceSet::default();
        avoid.insert(n(5));
        let nonce = draw_nonce(&avoid, scripted(&[5, 3844 + 5, 9]));
        assert_eq!(nonce.map(SelectNonce::get).ok(), Some(9));
    }

    #[test]
    fn d3_fill_error() {
        let err = draw_nonce(&NonceSet::default(), |_| Err(getrandom::Error::UNSUPPORTED));
        match err {
            Err(ResponderError::Encode { reason }) => {
                assert!(
                    reason.starts_with("no select nonce could be drawn: "),
                    "{reason}"
                );
            }
            other => panic!("expected Encode, got {other:?}"),
        }
    }

    #[test]
    fn d4_empty_avoid_takes_the_first_valid_draw() {
        assert_eq!(
            draw_nonce(&NonceSet::default(), scripted(&[3843]))
                .map(SelectNonce::get)
                .ok(),
            Some(3843)
        );
        assert_eq!(
            draw_nonce(&NonceSet::default(), scripted(&[3844]))
                .map(SelectNonce::get)
                .ok(),
            Some(0)
        );
        let nonce = draw_nonce(&NonceSet::default(), fill_os).expect("the OS CSPRNG works");
        assert!(nonce.get() <= SelectNonce::MAX);
    }
}
