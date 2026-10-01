//! Platform clipboard capture for Context Drop.
//!
//! This crate is the platform abstraction layer (spec §14). Domain logic lives
//! in `context-drop-core`; **no** core/domain code lives here. The desktop app
//! maps the `RawCapture` produced here onto core's `append_snapshot`.
//!
//! Design:
//! - Cross-platform text + image capture via `arboard`.
//! - Native APIs where `arboard` is insufficient: file lists (Finder/Explorer)
//!   and cheap change detection (macOS change count, Windows sequence number).
//! - Images are normalized to PNG.
//! - Change detection uses native tokens where available, else a content hash,
//!   so the capture loop never busy-polls content it already has.
//!
//! Only the pure logic (classification, PNG normalization, snapshot hashing)
//! is unit-tested in this environment; the platform read paths are implemented
//! and documented as runtime-verified per-OS at the app layer.

use std::path::PathBuf;

pub mod kind;
pub use kind::ClipItemKind;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Errors from clipboard access.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("clipboard backend error: {0}")]
    Backend(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image decode/encode error: {0}")]
    Image(String),
    #[error("clipboard capture is not supported on this platform build")]
    Unsupported,
}

pub type CaptureResult<T> = Result<T, CaptureError>;

/// An opaque monotonic-ish token identifying the current clipboard state.
/// Two reads with the same token can be assumed unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeToken(pub u64);

/// Where an item's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// In-memory bytes (text, normalized image).
    Inline(Vec<u8>),
    /// A copied file on disk. Bytes are read at append time (bounded by the
    /// configured per-item limit), so huge files never load into memory here.
    /// `mtime_ms` is the file's last-modified time (epoch ms) and participates
    /// in the snapshot hash, so re-copying an edited file — even one whose size
    /// is unchanged — is treated as new content, not a consecutive duplicate.
    FileRef {
        path: PathBuf,
        byte_size: u64,
        mtime_ms: i64,
    },
}

impl Payload {
    /// The size that will count toward limits.
    pub fn declared_size(&self) -> u64 {
        match self {
            Payload::Inline(b) => b.len() as u64,
            Payload::FileRef { byte_size, .. } => *byte_size,
        }
    }
}

/// One captured item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawItem {
    pub kind: ClipItemKind,
    pub mime_type: String,
    pub ext: String,
    pub payload: Payload,
}

/// A single clipboard change event: one or more items sharing one snapshot hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCapture {
    pub snapshot_sha256: String,
    pub items: Vec<RawItem>,
}

impl RawCapture {
    /// Build a capture from items, computing the deterministic snapshot hash.
    pub fn from_items(items: Vec<RawItem>) -> RawCapture {
        let snapshot_sha256 = compute_snapshot_hash(&items);
        RawCapture {
            snapshot_sha256,
            items,
        }
    }
}

/// A platform clipboard reader.
pub trait ClipboardProvider {
    /// A cheap token that changes when the clipboard changes.
    fn change_token(&mut self) -> CaptureResult<ChangeToken>;
    /// Read the current clipboard as a normalized capture, or `None` if empty
    /// or holding only unsupported content.
    fn read(&mut self) -> CaptureResult<Option<RawCapture>>;
}

/// Watches a provider and yields a capture only when the clipboard changed
/// since the last poll — the building block of the desktop capture loop.
pub struct Watcher<P: ClipboardProvider> {
    provider: P,
    last: Option<ChangeToken>,
}

impl<P: ClipboardProvider> Watcher<P> {
    pub fn new(provider: P) -> Self {
        Watcher {
            provider,
            last: None,
        }
    }

    /// Poll once. Returns `Some(capture)` when the clipboard changed and holds
    /// supported content; `None` when unchanged or unsupported.
    pub fn poll(&mut self) -> CaptureResult<Option<RawCapture>> {
        let token = self.provider.change_token()?;
        if self.last == Some(token) {
            return Ok(None);
        }
        // Only acknowledge the token after a SUCCESSFUL read, so a transient read
        // failure is retried on the next poll instead of being skipped forever.
        match self.provider.read() {
            Ok(capture) => {
                self.last = Some(token);
                Ok(capture)
            }
            Err(e) => Err(e),
        }
    }

    /// Reset change tracking so the next poll re-reads the current clipboard
    /// (i.e. it WILL capture whatever is on the clipboard right now).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Prime change tracking to the CURRENT clipboard state WITHOUT capturing it,
    /// so only content copied AFTER this point is captured. Used at capture start
    /// so a stale pre-existing clipboard item is not swept into the packet.
    /// Falls back to `reset()` if the current change token cannot be read.
    pub fn prime(&mut self) {
        match self.provider.change_token() {
            Ok(token) => self.last = Some(token),
            Err(_) => self.last = None,
        }
    }
}

/// Create the system clipboard provider for this platform.
#[cfg(target_os = "macos")]
pub fn system_provider() -> CaptureResult<macos::MacosClipboardProvider> {
    macos::MacosClipboardProvider::new()
}

#[cfg(target_os = "windows")]
pub fn system_provider() -> CaptureResult<windows::WindowsClipboardProvider> {
    windows::WindowsClipboardProvider::new()
}

#[cfg(target_os = "linux")]
pub fn system_provider() -> CaptureResult<linux::LinuxClipboardProvider> {
    linux::LinuxClipboardProvider::new()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn system_provider() -> CaptureResult<NullClipboardProvider> {
    Ok(NullClipboardProvider)
}

/// A no-op provider for unsupported platform builds.
pub struct NullClipboardProvider;
impl ClipboardProvider for NullClipboardProvider {
    fn change_token(&mut self) -> CaptureResult<ChangeToken> {
        Ok(ChangeToken(0))
    }
    fn read(&mut self) -> CaptureResult<Option<RawCapture>> {
        Err(CaptureError::Unsupported)
    }
}

// ---- Pure logic (unit-tested) ------------------------------------------

/// Deterministic hash over a snapshot's items, used for consecutive-dedupe.
/// For file references we hash path + size (not content) so repeated copies of
/// the same files dedupe cheaply without reading them.
pub fn compute_snapshot_hash(items: &[RawItem]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for item in items {
        h.update([kind_tag(item.kind)]);
        match &item.payload {
            Payload::Inline(bytes) => {
                h.update(b"inline:");
                h.update((bytes.len() as u64).to_le_bytes());
                h.update(bytes);
            }
            Payload::FileRef {
                path,
                byte_size,
                mtime_ms,
            } => {
                h.update(b"file:");
                h.update(byte_size.to_le_bytes());
                h.update(mtime_ms.to_le_bytes());
                h.update(path.to_string_lossy().as_bytes());
            }
        }
        h.update([0u8]); // separator
    }
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn kind_tag(k: ClipItemKind) -> u8 {
    match k {
        ClipItemKind::Text => b't',
        ClipItemKind::Json => b'j',
        ClipItemKind::Html => b'h',
        ClipItemKind::Url => b'u',
        ClipItemKind::Image => b'i',
        ClipItemKind::File => b'f',
        ClipItemKind::Unknown => b'?',
    }
}

/// Classify a plain-text clipboard value into a finer kind.
pub fn classify_text(text: &str) -> ClipItemKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ClipItemKind::Text;
    }
    // A single-line URL.
    if is_probable_url(trimmed) {
        return ClipItemKind::Url;
    }
    // JSON: strict parse (only if it starts like JSON, to avoid parsing bare
    // numbers/strings as JSON documents).
    let first = trimmed.as_bytes()[0];
    if (first == b'{' || first == b'[')
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return ClipItemKind::Json;
    }
    // HTML fragment/document heuristic.
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.contains("</") && lower.contains('>') && lower.starts_with('<')
    {
        return ClipItemKind::Html;
    }
    ClipItemKind::Text
}

fn is_probable_url(s: &str) -> bool {
    if s.contains(char::is_whitespace) {
        return false;
    }
    for scheme in ["https://", "http://", "ftp://", "file://"] {
        if s.len() > scheme.len() && s.to_ascii_lowercase().starts_with(scheme) {
            return true;
        }
    }
    false
}

/// The file extension used to store an item of a given kind.
pub fn ext_for_kind(kind: ClipItemKind) -> &'static str {
    match kind {
        ClipItemKind::Text => "txt",
        ClipItemKind::Json => "json",
        ClipItemKind::Html => "html",
        ClipItemKind::Url => "txt",
        ClipItemKind::Image => "png",
        ClipItemKind::File => "bin",
        ClipItemKind::Unknown => "bin",
    }
}

/// The MIME type used for an item of a given kind.
pub fn mime_for_kind(kind: ClipItemKind) -> &'static str {
    match kind {
        ClipItemKind::Text => "text/plain",
        ClipItemKind::Json => "application/json",
        ClipItemKind::Html => "text/html",
        ClipItemKind::Url => "text/uri-list",
        ClipItemKind::Image => "image/png",
        ClipItemKind::File => "application/octet-stream",
        ClipItemKind::Unknown => "application/octet-stream",
    }
}

/// Build a text-like item, classifying the text.
pub fn text_item(text: &str) -> RawItem {
    let kind = classify_text(text);
    RawItem {
        kind,
        mime_type: mime_for_kind(kind).to_string(),
        ext: ext_for_kind(kind).to_string(),
        payload: Payload::Inline(text.as_bytes().to_vec()),
    }
}

/// Normalize raw RGBA pixels into PNG bytes.
pub fn rgba_to_png(width: usize, height: usize, rgba: &[u8]) -> CaptureResult<Vec<u8>> {
    use image::{ExtendedColorType, ImageEncoder};
    let expected = width.saturating_mul(height).saturating_mul(4);
    if rgba.len() < expected || width == 0 || height == 0 {
        return Err(CaptureError::Image(format!(
            "invalid rgba buffer: {}x{} needs {} bytes, got {}",
            width,
            height,
            expected,
            rgba.len()
        )));
    }
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(
            &rgba[..expected],
            width as u32,
            height as u32,
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| CaptureError::Image(e.to_string()))?;
    Ok(out)
}

/// Re-encode already-encoded image bytes (PNG/JPEG/TIFF/…) to normalized PNG.
pub fn encoded_image_to_png(bytes: &[u8]) -> CaptureResult<Vec<u8>> {
    let img = image::load_from_memory(bytes).map_err(|e| CaptureError::Image(e.to_string()))?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| CaptureError::Image(e.to_string()))?;
    Ok(out.into_inner())
}

/// Build an image item from raw RGBA, normalized to PNG.
pub fn image_item_from_rgba(width: usize, height: usize, rgba: &[u8]) -> CaptureResult<RawItem> {
    let png = rgba_to_png(width, height, rgba)?;
    Ok(RawItem {
        kind: ClipItemKind::Image,
        mime_type: "image/png".to_string(),
        ext: "png".to_string(),
        payload: Payload::Inline(png),
    })
}

/// Build a small PNG thumbnail (fit within `max_dim` px, aspect preserved) of an
/// encoded image, returned as a `data:image/png;base64,…` URL for direct `<img>`
/// display in the desktop UI. Returns `None` if the bytes are not a decodable
/// image. This is a UI convenience (the user previewing their OWN captured
/// content locally) — it is unrelated to the packet→subagent data flow.
pub fn thumbnail_data_url(image_bytes: &[u8], max_dim: u32) -> Option<String> {
    let img = image::load_from_memory(image_bytes).ok()?;
    let thumb = img.thumbnail(max_dim.max(1), max_dim.max(1));
    let mut png = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let mut out = String::from("data:image/png;base64,");
    base64_encode_into(&png.into_inner(), &mut out);
    Some(out)
}

/// Minimal standard-base64 encoder (no external dep). Appends to `out`.
fn base64_encode_into(data: &[u8], out: &mut String) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
}

/// Build a file-reference item from a path (bytes read later, within limits).
pub fn file_item(path: PathBuf) -> RawItem {
    let meta = std::fs::metadata(&path).ok();
    let byte_size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime_ms = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_string();
    RawItem {
        kind: ClipItemKind::File,
        mime_type: "application/octet-stream".to_string(),
        ext,
        payload: Payload::FileRef {
            path,
            byte_size,
            mtime_ms,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_plain_text() {
        assert_eq!(classify_text("just some words"), ClipItemKind::Text);
        assert_eq!(classify_text("  こんにちは、世界  "), ClipItemKind::Text);
        assert_eq!(classify_text(""), ClipItemKind::Text);
    }

    #[test]
    fn classify_json() {
        assert_eq!(classify_text(r#"{"a":1,"b":[2,3]}"#), ClipItemKind::Json);
        assert_eq!(classify_text("[1, 2, 3]"), ClipItemKind::Json);
        // Not JSON: looks like a brace but invalid.
        assert_eq!(classify_text("{ not json"), ClipItemKind::Text);
        // A bare number is not treated as a JSON document.
        assert_eq!(classify_text("42"), ClipItemKind::Text);
    }

    #[test]
    fn classify_url_only_single_line() {
        assert_eq!(
            classify_text("https://example.com/x?y=1"),
            ClipItemKind::Url
        );
        assert_eq!(classify_text("http://localhost:3000"), ClipItemKind::Url);
        // URL followed by text is not a bare URL.
        assert_eq!(
            classify_text("see https://example.com now"),
            ClipItemKind::Text
        );
    }

    #[test]
    fn classify_html() {
        assert_eq!(
            classify_text("<!DOCTYPE html><html></html>"),
            ClipItemKind::Html
        );
        assert_eq!(
            classify_text("<div class=\"x\">hi</div>"),
            ClipItemKind::Html
        );
    }

    #[test]
    fn snapshot_hash_is_deterministic_and_content_sensitive() {
        let a = vec![text_item("hello")];
        let b = vec![text_item("hello")];
        let c = vec![text_item("world")];
        assert_eq!(compute_snapshot_hash(&a), compute_snapshot_hash(&b));
        assert_ne!(compute_snapshot_hash(&a), compute_snapshot_hash(&c));
    }

    #[test]
    fn file_snapshot_hash_changes_when_mtime_changes() {
        // Re-copying an edited file (same path, same size, newer mtime) must NOT
        // be treated as a consecutive duplicate.
        let mk = |mtime: i64| RawItem {
            kind: ClipItemKind::File,
            mime_type: "application/octet-stream".into(),
            ext: "log".into(),
            payload: Payload::FileRef {
                path: std::path::PathBuf::from("/tmp/app.log"),
                byte_size: 4096,
                mtime_ms: mtime,
            },
        };
        let older = vec![mk(1_000)];
        let same = vec![mk(1_000)];
        let edited = vec![mk(2_000)];
        assert_eq!(compute_snapshot_hash(&older), compute_snapshot_hash(&same));
        assert_ne!(
            compute_snapshot_hash(&older),
            compute_snapshot_hash(&edited),
            "an edited (newer mtime) file must hash differently"
        );
    }

    #[test]
    fn snapshot_hash_distinguishes_multi_item_order() {
        let ab = vec![text_item("a"), text_item("b")];
        let ba = vec![text_item("b"), text_item("a")];
        assert_ne!(compute_snapshot_hash(&ab), compute_snapshot_hash(&ba));
    }

    #[test]
    fn rgba_to_png_roundtrips_dimensions() {
        // A 2x2 opaque red image.
        let rgba = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let png = rgba_to_png(2, 2, &rgba).unwrap();
        // PNG signature.
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // Decodes back to 2x2.
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn rgba_to_png_rejects_bad_buffer() {
        assert!(rgba_to_png(2, 2, &[0, 0, 0]).is_err());
        assert!(rgba_to_png(0, 0, &[]).is_err());
    }

    #[test]
    fn encoded_image_normalizes_to_png() {
        // Start from a PNG we generate, then normalize again.
        let rgba = vec![0, 255, 0, 255];
        let png = rgba_to_png(1, 1, &rgba).unwrap();
        let normalized = encoded_image_to_png(&png).unwrap();
        assert_eq!(
            &normalized[..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }

    #[test]
    fn base64_encodes_known_vectors() {
        let enc = |s: &str| {
            let mut out = String::new();
            base64_encode_into(s.as_bytes(), &mut out);
            out
        };
        // RFC 4648 test vectors.
        assert_eq!(enc(""), "");
        assert_eq!(enc("f"), "Zg==");
        assert_eq!(enc("fo"), "Zm8=");
        assert_eq!(enc("foo"), "Zm9v");
        assert_eq!(enc("foob"), "Zm9vYg==");
        assert_eq!(enc("fooba"), "Zm9vYmE=");
        assert_eq!(enc("foobar"), "Zm9vYmFy");
    }

    #[test]
    fn thumbnail_produces_a_png_data_url_and_rejects_non_images() {
        // A real (tiny) PNG thumbnails to a PNG data URL.
        let png = rgba_to_png(4, 2, &[0u8; 4 * 2 * 4]).unwrap();
        let url = thumbnail_data_url(&png, 8).expect("valid image thumbnails");
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(url.len() > "data:image/png;base64,".len());
        // Non-image bytes yield None rather than a broken URL.
        assert!(thumbnail_data_url(b"not an image", 8).is_none());
    }

    #[test]
    fn watcher_yields_once_per_change() {
        // A fake provider whose token and content we control.
        struct Fake {
            token: u64,
            content: String,
        }
        impl ClipboardProvider for Fake {
            fn change_token(&mut self) -> CaptureResult<ChangeToken> {
                Ok(ChangeToken(self.token))
            }
            fn read(&mut self) -> CaptureResult<Option<RawCapture>> {
                Ok(Some(RawCapture::from_items(vec![text_item(&self.content)])))
            }
        }
        let mut w = Watcher::new(Fake {
            token: 1,
            content: "a".into(),
        });
        assert!(w.poll().unwrap().is_some(), "first poll reads");
        assert!(
            w.poll().unwrap().is_none(),
            "unchanged token yields nothing"
        );
        w.provider.token = 2;
        w.provider.content = "b".into();
        assert!(w.poll().unwrap().is_some(), "changed token reads again");
    }

    #[test]
    fn prime_skips_preexisting_clipboard() {
        struct Fake {
            token: u64,
            content: String,
        }
        impl ClipboardProvider for Fake {
            fn change_token(&mut self) -> CaptureResult<ChangeToken> {
                Ok(ChangeToken(self.token))
            }
            fn read(&mut self) -> CaptureResult<Option<RawCapture>> {
                Ok(Some(RawCapture::from_items(vec![text_item(&self.content)])))
            }
        }
        let mut w = Watcher::new(Fake {
            token: 7,
            content: "already on the clipboard before Start".into(),
        });
        // Prime: record the current state WITHOUT capturing it.
        w.prime();
        assert!(
            w.poll().unwrap().is_none(),
            "a pre-existing clipboard item is NOT captured after prime"
        );
        // Something copied AFTER start (token changes) IS captured.
        w.provider.token = 8;
        w.provider.content = "copied after Start".into();
        assert!(
            w.poll().unwrap().is_some(),
            "content copied after Start is captured"
        );
    }
}
