//! Windows clipboard provider.
//!
//! - text + image: `arboard` (image normalized to PNG in the shared logic).
//! - change detection: `GetClipboardSequenceNumber` via `clipboard-win`.
//! - file lists (Explorer single/multiple copy): CF_HDROP via `clipboard-win`
//!   (`arboard` cannot provide these — spec §14).
//!
//! This module compiles only on Windows. It is exercised by CI on
//! `windows-latest`; it is not runtime-verified in the macOS build environment.

use std::path::PathBuf;

use arboard::Clipboard;

use crate::{
    file_item, image_item_from_rgba, text_item, CaptureError, CaptureResult, ChangeToken,
    ClipboardProvider, RawCapture,
};

pub struct WindowsClipboardProvider {
    clipboard: Clipboard,
}

impl WindowsClipboardProvider {
    pub fn new() -> CaptureResult<Self> {
        let clipboard = Clipboard::new().map_err(|e| CaptureError::Backend(e.to_string()))?;
        Ok(WindowsClipboardProvider { clipboard })
    }
}

impl ClipboardProvider for WindowsClipboardProvider {
    fn change_token(&mut self) -> CaptureResult<ChangeToken> {
        // `clipboard_win::raw::seq_num()` wraps GetClipboardSequenceNumber and
        // returns Option<NonZeroU32> (no open required).
        let seq = clipboard_win::raw::seq_num().map(|n| n.get()).unwrap_or(0);
        Ok(ChangeToken(seq as u64))
    }

    fn read(&mut self) -> CaptureResult<Option<RawCapture>> {
        // Priority: copied files (Explorer, CF_HDROP) > image > text.
        if let Ok(files) = clipboard_win::get_clipboard(clipboard_win::formats::FileList) {
            let files: Vec<String> = files;
            if !files.is_empty() {
                let items = files
                    .into_iter()
                    .map(|p| file_item(PathBuf::from(p)))
                    .collect();
                return Ok(Some(RawCapture::from_items(items)));
            }
        }

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
