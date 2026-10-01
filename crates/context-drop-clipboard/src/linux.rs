//! Linux clipboard provider (experimental).
//!
//! - text + image: `arboard` (X11/Wayland; normalized to PNG in shared logic).
//! - change detection: content-hash polling (no cheap native sequence number
//!   is portable across X11/Wayland), so this reads content each tick. This is
//!   acceptable for the experimental Linux adapter.
//! - file lists: not implemented on Linux yet (Finder/Explorer-style file copy
//!   has no single portable mechanism). Documented as experimental.
//!
//! Linux support is experimental and must never block macOS/Windows (spec §2).

use arboard::Clipboard;
use sha2::{Digest, Sha256};

use crate::{
    image_item_from_rgba, text_item, CaptureError, CaptureResult, ChangeToken, ClipboardProvider,
    RawCapture,
};

pub struct LinuxClipboardProvider {
    clipboard: Clipboard,
}

impl LinuxClipboardProvider {
    pub fn new() -> CaptureResult<Self> {
        let clipboard = Clipboard::new().map_err(|e| CaptureError::Backend(e.to_string()))?;
        Ok(LinuxClipboardProvider { clipboard })
    }

    /// Cheap-ish digest of the current clipboard content for change detection.
    fn content_token(&mut self) -> u64 {
        let mut h = Sha256::new();
        if let Ok(text) = self.clipboard.get_text() {
            h.update(b"t");
            h.update(text.as_bytes());
        }
        if let Ok(img) = self.clipboard.get_image() {
            h.update(b"i");
            h.update((img.width as u64).to_le_bytes());
            h.update((img.height as u64).to_le_bytes());
            // Hash the FULL pixel buffer: a bounded prefix misses screenshots
            // that differ only below the first rows (experimental Linux path).
            h.update(&img.bytes);
        }
        let out = h.finalize();
        let mut token = [0u8; 8];
        token.copy_from_slice(&out[..8]);
        u64::from_le_bytes(token)
    }
}

impl ClipboardProvider for LinuxClipboardProvider {
    fn change_token(&mut self) -> CaptureResult<ChangeToken> {
        Ok(ChangeToken(self.content_token()))
    }

    fn read(&mut self) -> CaptureResult<Option<RawCapture>> {
        // Image before text (a screenshot is more specific than any text alt).
        match self.clipboard.get_image() {
            Ok(img) => {
                let item = image_item_from_rgba(img.width, img.height, &img.bytes)?;
                return Ok(Some(RawCapture::from_items(vec![item])));
            }
            Err(arboard::Error::ClipboardOccupied) => {
                return Err(CaptureError::Backend("clipboard occupied".into()))
            }
            Err(_) => {}
        }
        match self.clipboard.get_text() {
            Ok(text) if !text.trim().is_empty() => {
                return Ok(Some(RawCapture::from_items(vec![text_item(&text)])))
            }
            Ok(_) => {}
            Err(arboard::Error::ClipboardOccupied) => {
                return Err(CaptureError::Backend("clipboard occupied".into()))
            }
            Err(_) => {}
        }
        Ok(None)
    }
}
