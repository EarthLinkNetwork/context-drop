//! macOS clipboard provider.
//!
//! - text + image: `arboard` (image normalized to PNG in the shared logic).
//! - change detection: `NSPasteboard.changeCount` (cheap, native).
//! - file lists (Finder single/multiple copy): `NSPasteboard` file-URL items
//!   read via `objc2` — `arboard` cannot provide these (spec §14).

use std::path::PathBuf;

use arboard::Clipboard;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
use objc2_foundation::{NSString, NSURL};

use crate::{
    file_item, image_item_from_rgba, text_item, CaptureError, CaptureResult, ChangeToken,
    ClipboardProvider, RawCapture,
};

pub struct MacosClipboardProvider {
    clipboard: Clipboard,
}

impl MacosClipboardProvider {
    pub fn new() -> CaptureResult<Self> {
        let clipboard = Clipboard::new().map_err(|e| CaptureError::Backend(e.to_string()))?;
        Ok(MacosClipboardProvider { clipboard })
    }
}

impl ClipboardProvider for MacosClipboardProvider {
    fn change_token(&mut self) -> CaptureResult<ChangeToken> {
        // changeCount increments on every clipboard write, system-wide.
        let count = NSPasteboard::generalPasteboard().changeCount();
        Ok(ChangeToken(count as u64))
    }

    fn read(&mut self) -> CaptureResult<Option<RawCapture>> {
        // Priority: copied files (Finder) > image > text.
        let files = read_file_urls();
        if !files.is_empty() {
            let items = files.into_iter().map(file_item).collect();
            return Ok(Some(RawCapture::from_items(items)));
        }

        match self.clipboard.get_image() {
            Ok(img) => {
                let item = image_item_from_rgba(img.width, img.height, &img.bytes)?;
                return Ok(Some(RawCapture::from_items(vec![item])));
            }
            // A transient "clipboard busy" must surface as an error so the
            // watcher retries (rather than skipping the event forever).
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

/// Read file URLs from the general pasteboard (single or multiple Finder files).
fn read_file_urls() -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        let Some(items) = pb.pasteboardItems() else {
            return out;
        };
        for item in items.iter() {
            if let Some(s) = item.stringForType(NSPasteboardTypeFileURL) {
                let url_str = s.to_string();
                if let Some(url) = NSURL::URLWithString(&NSString::from_str(&url_str)) {
                    if let Some(path) = url.path() {
                        out.push(PathBuf::from(path.to_string()));
                        continue;
                    }
                }
                // Fallback: strip a leading file:// scheme if URL parsing failed.
                if let Some(stripped) = url_str.strip_prefix("file://") {
                    out.push(PathBuf::from(percent_decode(stripped)));
                }
            }
        }
    }
    out
}

/// Minimal percent-decoding for file URL paths.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
