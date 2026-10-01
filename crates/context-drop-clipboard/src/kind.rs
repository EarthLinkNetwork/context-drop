//! Captured item kinds. The string tokens intentionally match
//! `context_drop_core::ItemKind` so the desktop can map between them by name
//! without this crate depending on the core domain.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipItemKind {
    Text,
    Json,
    Html,
    Url,
    Image,
    File,
    Unknown,
}

impl ClipItemKind {
    /// The stable token, matching `context_drop_core::ItemKind::as_str`.
    pub fn as_str(self) -> &'static str {
        match self {
            ClipItemKind::Text => "text",
            ClipItemKind::Json => "json",
            ClipItemKind::Html => "html",
            ClipItemKind::Url => "url",
            ClipItemKind::Image => "image",
            ClipItemKind::File => "file",
            ClipItemKind::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_core_convention() {
        assert_eq!(ClipItemKind::Image.as_str(), "image");
        assert_eq!(ClipItemKind::Json.as_str(), "json");
        assert_eq!(ClipItemKind::File.as_str(), "file");
    }
}
