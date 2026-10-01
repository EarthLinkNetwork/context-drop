//! Application settings and their defaults.
//!
//! Settings are persisted in the SQLite `settings` table (key/value). The
//! database is the single authority; the desktop UI edits a copy and saves it
//! back. Field names serialize as camelCase to match the TypeScript frontend.

use serde::{Deserialize, Serialize};

/// 25 MiB — a generous default per-item ceiling.
pub const DEFAULT_MAX_ITEM_BYTES: i64 = 25 * 1024 * 1024;
/// 200 MiB — default total packet ceiling.
pub const DEFAULT_MAX_PACKET_BYTES: i64 = 200 * 1024 * 1024;
/// Default packet TTL.
pub const DEFAULT_TTL_HOURS: u32 = 24;

/// A low-conflict default shortcut. Uses Tauri's accelerator syntax and avoids
/// the well-known OS shortcuts (Spotlight, screenshots). Always user-editable.
pub fn default_global_shortcut() -> String {
    "CommandOrControl+Shift+9".to_string()
}

/// The user-configurable application settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub global_shortcut: String,
    pub packet_ttl_hours: u32,
    pub max_item_bytes: i64,
    pub max_packet_bytes: i64,
    pub auto_cleanup: bool,
    /// Whether the `/cd` short alias skill has been installed (best-effort flag).
    pub short_alias_installed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            global_shortcut: default_global_shortcut(),
            packet_ttl_hours: DEFAULT_TTL_HOURS,
            max_item_bytes: DEFAULT_MAX_ITEM_BYTES,
            max_packet_bytes: DEFAULT_MAX_PACKET_BYTES,
            auto_cleanup: true,
            short_alias_installed: false,
        }
    }
}

impl Settings {
    /// Clamp obviously invalid values to safe defaults (never crash on bad input).
    pub fn sanitized(mut self) -> Settings {
        if self.global_shortcut.trim().is_empty() {
            self.global_shortcut = default_global_shortcut();
        }
        if self.packet_ttl_hours == 0 {
            self.packet_ttl_hours = DEFAULT_TTL_HOURS;
        }
        if self.max_item_bytes <= 0 {
            self.max_item_bytes = DEFAULT_MAX_ITEM_BYTES;
        }
        if self.max_packet_bytes <= 0 {
            self.max_packet_bytes = DEFAULT_MAX_PACKET_BYTES;
        }
        // A single item can never be larger than the whole packet budget.
        if self.max_item_bytes > self.max_packet_bytes {
            self.max_item_bytes = self.max_packet_bytes;
        }
        self
    }

    /// The size limits derived from these settings.
    pub fn limits(&self) -> Limits {
        Limits {
            max_item_bytes: self.max_item_bytes,
            max_packet_bytes: self.max_packet_bytes,
        }
    }
}

/// Size limits applied when appending items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_item_bytes: i64,
    pub max_packet_bytes: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Settings::default().limits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_reasonable() {
        let s = Settings::default();
        assert_eq!(s.packet_ttl_hours, 24);
        assert_eq!(s.max_item_bytes, 25 * 1024 * 1024);
        assert_eq!(s.max_packet_bytes, 200 * 1024 * 1024);
        assert!(s.auto_cleanup);
        assert!(!s.short_alias_installed);
        assert!(!s.global_shortcut.is_empty());
    }

    #[test]
    fn sanitize_fixes_bad_values() {
        let s = Settings {
            global_shortcut: "  ".into(),
            packet_ttl_hours: 0,
            max_item_bytes: -1,
            max_packet_bytes: 0,
            auto_cleanup: true,
            short_alias_installed: false,
        }
        .sanitized();
        assert_eq!(s.global_shortcut, default_global_shortcut());
        assert_eq!(s.packet_ttl_hours, 24);
        assert!(s.max_item_bytes > 0);
        assert!(s.max_packet_bytes > 0);
        assert!(s.max_item_bytes <= s.max_packet_bytes);
    }

    #[test]
    fn serializes_camel_case() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(json.contains("globalShortcut"));
        assert!(json.contains("packetTtlHours"));
        assert!(json.contains("maxItemBytes"));
        assert!(json.contains("shortAliasInstalled"));
    }
}
