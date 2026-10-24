//! The render cache key (AD-15, FR-36) and the cache entry codec.
//!
//! A [`RenderKey`] is the blake3 digest of everything that decides a
//! render's bytes: the key schema, the Noctis and takumi versions, the
//! Template id and version, the Theme digest, the job's postcard data, the
//! App's asset pairs (fonts, static images, the remote-image CDN and
//! fallback), the size, the format, the animation and the effective encode
//! settings. Every variable-length field is framed by its `u64` LE byte
//! length; integers are fixed-width LE.
//!
//! An entry in the App's `RenderCache` is a 22-byte header (`NRC1`, the
//! output format, the encode settings) followed by the image bytes.

use std::fmt;

use bytes::{BufMut, Bytes, BytesMut};
use noctis_ports::{
    AnimatedFormat, AnimationSpec, EncodeSettings, Format, OutputFormat, RenderJob, RenderOutput,
    Size,
};

/// The key layout's version; bump it whenever the layout changes.
pub(crate) const KEY_SCHEMA: &[u8] = b"noctis.render.v1";
/// The Noctis version: every crate shares one (AD-2).
pub(crate) const NOCTIS_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The pinned takumi versions (`Cargo.lock`; the pin test keeps it honest).
pub(crate) const TAKUMI_VERSION: &str = "takumi=2.17.0;takumi-core=0.31.0";

/// The magic every cache entry starts with.
const ENTRY_MAGIC: &[u8; 4] = b"NRC1";
/// The length of an entry's header.
const HEADER_LEN: usize = 22;

/// A render's cache key: the blake3 digest of its [`KeyInputs`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RenderKey(pub(crate) [u8; 32]);

impl RenderKey {
    /// The key's 64 lowercase hex characters.
    pub(crate) fn hex(&self) -> String {
        blake3::Hash::from_bytes(self.0).to_hex().to_string()
    }

    /// The key in the App's `RenderCache`: `render:v1:<64 lowercase hex>`.
    pub(crate) fn store_key(&self) -> String {
        format!("render:v1:{}", self.hex())
    }
}

impl fmt::Debug for RenderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RenderKey({})", self.hex())
    }
}

/// The App-level assets that change a render's bytes, as `(id, version)`
/// pairs sorted by id bytewise.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AssetPairs(Vec<(String, [u8; 32])>);

impl AssetPairs {
    /// `pairs`, sorted by id.
    pub(crate) fn new(mut pairs: Vec<(String, [u8; 32])>) -> Self {
        pairs.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        Self(pairs)
    }
}

/// Everything a [`RenderKey`] is computed from. The version fields default
/// to the constants; tests vary them.
#[derive(Debug, Clone)]
pub(crate) struct KeyInputs<'a> {
    pub(crate) schema: &'a [u8],
    pub(crate) noctis_version: &'a str,
    pub(crate) takumi_version: &'a str,
    pub(crate) template_id: &'a str,
    pub(crate) template_version: u32,
    pub(crate) theme: [u8; 32],
    pub(crate) data: &'a [u8],
    pub(crate) assets: &'a AssetPairs,
    pub(crate) size: Size,
    pub(crate) format: Format,
    pub(crate) animation: Option<AnimationSpec>,
    /// `min(job.upload_budget, size_target)`.
    pub(crate) budget: u64,
    pub(crate) still_fallback: bool,
}

impl<'a> KeyInputs<'a> {
    /// The inputs of `job` on a Renderer with `theme`, `assets`,
    /// `size_target` and `still_fallback`, at the current versions.
    pub(crate) fn of(
        job: &'a RenderJob,
        theme: [u8; 32],
        assets: &'a AssetPairs,
        size_target: u64,
        still_fallback: bool,
    ) -> Self {
        Self {
            schema: KEY_SCHEMA,
            noctis_version: NOCTIS_VERSION,
            takumi_version: TAKUMI_VERSION,
            template_id: job.template_id,
            template_version: job.template_version,
            theme,
            data: &job.data,
            assets,
            size: job.size,
            format: job.format,
            animation: job.animation,
            budget: job.upload_budget.min(size_target),
            still_fallback,
        }
    }

    /// The blake3 digest of the fields, in the AD-15 order.
    pub(crate) fn key(&self) -> RenderKey {
        let mut h = blake3::Hasher::new();
        framed(&mut h, self.schema);
        framed(&mut h, self.noctis_version.as_bytes());
        framed(&mut h, self.takumi_version.as_bytes());
        framed(&mut h, self.template_id.as_bytes());
        h.update(&self.template_version.to_le_bytes());
        h.update(&self.theme);
        framed(&mut h, self.data);
        h.update(&len_u64(self.assets.0.len()).to_le_bytes());
        for (id, version) in &self.assets.0 {
            framed(&mut h, id.as_bytes());
            h.update(version);
        }
        h.update(&self.size.width().to_le_bytes());
        h.update(&self.size.height().to_le_bytes());
        let (tag, quality) = match self.format {
            Format::Png => (0, 0),
            Format::Jpeg { quality } => (1, quality),
            Format::Webp { quality } => (2, quality),
            Format::WebpLossless => (3, 0),
        };
        h.update(&[tag, quality]);
        match self.animation {
            None => {
                h.update(&[0]);
            }
            Some(spec) => {
                h.update(&[1]);
                h.update(&spec.duration.as_nanos().to_le_bytes());
                h.update(&spec.fps.to_le_bytes());
                let (tag, quality) = match spec.format {
                    AnimatedFormat::Webp { quality } => (0, quality),
                    AnimatedFormat::Gif => (1, 0),
                    AnimatedFormat::Apng => (2, 0),
                };
                h.update(&[tag, quality]);
            }
        }
        h.update(&self.budget.to_le_bytes());
        h.update(&[u8::from(self.still_fallback)]);
        RenderKey(*h.finalize().as_bytes())
    }
}

/// Feeds `bytes` framed by its `u64` LE length.
fn framed(h: &mut blake3::Hasher, bytes: &[u8]) {
    h.update(&len_u64(bytes.len()).to_le_bytes());
    h.update(bytes);
}

/// `len` as a `u64` (lossless on every supported target).
fn len_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// The blake3 digest of `bytes`, as an asset pair version.
pub(crate) fn version_of(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

/// The tag of `format` in an entry header.
fn format_tag(format: OutputFormat) -> u8 {
    match format {
        OutputFormat::Png => 0,
        OutputFormat::Jpeg => 1,
        OutputFormat::Webp => 2,
        OutputFormat::Gif => 3,
        OutputFormat::Apng => 4,
    }
}

/// The format of an entry header's `tag`.
fn tag_format(tag: u8) -> Option<OutputFormat> {
    Some(match tag {
        0 => OutputFormat::Png,
        1 => OutputFormat::Jpeg,
        2 => OutputFormat::Webp,
        3 => OutputFormat::Gif,
        4 => OutputFormat::Apng,
        _ => return None,
    })
}

/// The cache entry of `output`: the header, then its bytes.
pub(crate) fn encode_entry(output: &RenderOutput) -> Bytes {
    let s = &output.settings_used;
    let mut entry = BytesMut::with_capacity(HEADER_LEN + output.bytes.len());
    entry.put_slice(ENTRY_MAGIC);
    entry.put_u8(format_tag(output.format));
    entry.put_u32_le(s.size.width());
    entry.put_u32_le(s.size.height());
    entry.put_u8(u8::from(s.quality.is_some()));
    entry.put_u8(s.quality.unwrap_or(0));
    entry.put_u8(u8::from(s.fps.is_some()));
    entry.put_u16_le(s.fps.unwrap_or(0));
    entry.put_u32_le(s.frames);
    entry.put_slice(&output.bytes);
    entry.freeze()
}

/// The output `entry` holds, with `key` as its digest; `None` when it is
/// not a valid entry. The image bytes share `entry`'s buffer.
pub(crate) fn decode_entry(entry: &Bytes, key: RenderKey) -> Option<RenderOutput> {
    if entry.len() <= HEADER_LEN || &entry[..4] != ENTRY_MAGIC {
        return None;
    }
    let h = &entry[..HEADER_LEN];
    let u32_at = |at: usize| u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]]);
    let present = |byte: u8| match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    let format = tag_format(h[4])?;
    let size = Size::try_new(u32_at(5), u32_at(9))?;
    let quality = present(h[13])?.then_some(h[14]);
    let fps = present(h[15])?.then(|| u16::from_le_bytes([h[16], h[17]]));
    let frames = u32_at(18);
    if frames == 0 {
        return None;
    }
    Some(RenderOutput {
        bytes: entry.slice(HEADER_LEN..),
        digest: key.0,
        format,
        settings_used: EncodeSettings {
            size,
            quality,
            fps,
            frames,
        },
    })
}

#[cfg(test)]
mod tests;
