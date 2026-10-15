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

/// Default shortcut that shows / hides the window. Distinct from the capture
/// toggle; an empty value turns it off.
pub fn default_show_window_shortcut() -> String {
    "CommandOrControl+Alt+D".to_string()
}

/// Which screen the window opens on when it is shown (tray click or shortcut).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowPlacement {
    /// The screen the mouse pointer is on.
    #[default]
    Cursor,
    /// A specific display chosen in Settings (`window_display`).
    Display,
    /// Wherever the window was last (restored after a restart).
    Last,
}

impl WindowPlacement {
    pub fn as_str(self) -> &'static str {
        match self {
            WindowPlacement::Cursor => "cursor",
            WindowPlacement::Display => "display",
            WindowPlacement::Last => "last",
        }
    }

    pub fn parse(s: &str) -> Option<WindowPlacement> {
        match s {
            "cursor" => Some(WindowPlacement::Cursor),
            "display" => Some(WindowPlacement::Display),
            "last" => Some(WindowPlacement::Last),
            _ => None,
        }
    }
}

/// The user-configurable application settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Toggles Capture ON/OFF.
    pub global_shortcut: String,
    /// Shows / hides the window. Empty = off.
    pub show_window_shortcut: String,
    pub window_placement: WindowPlacement,
    /// Key of the display chosen for `WindowPlacement::Display`
    /// (see `placement::display_key`). Empty = none chosen yet.
    pub window_display: String,
    /// Keep the window above other apps' windows.
    pub always_on_top: bool,
    /// Show the window on every desktop (Space), like the Dock's
    /// "Assign To: All Desktops".
    pub show_on_all_desktops: bool,
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
            show_window_shortcut: default_show_window_shortcut(),
            window_placement: WindowPlacement::Cursor,
            window_display: String::new(),
            always_on_top: false,
            show_on_all_desktops: false,
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
        self.global_shortcut = self.global_shortcut.trim().to_string();
        self.show_window_shortcut = self.show_window_shortcut.trim().to_string();
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
        assert_ne!(s.show_window_shortcut, s.global_shortcut);
        assert_eq!(s.window_placement, WindowPlacement::Cursor);
        assert!(!s.always_on_top);
        assert!(!s.show_on_all_desktops);
    }

    // Tier A: settings decoding of the window fields.
    #[test]
    fn placement_parse_roundtrips_and_rejects_unknown() {
        for p in [
            WindowPlacement::Cursor,
            WindowPlacement::Display,
            WindowPlacement::Last,
        ] {
            assert_eq!(WindowPlacement::parse(p.as_str()), Some(p));
        }
        assert_eq!(WindowPlacement::parse("Cursor"), None);
        assert_eq!(WindowPlacement::parse(""), None);
    }

    // Tier A: settings decoding of the window fields.
    #[test]
    fn deserializes_settings_missing_new_fields_with_defaults() {
        // A payload from before the window settings existed still loads.
        let json = r#"{"globalShortcut":"CommandOrControl+Shift+9","packetTtlHours":24,
            "maxItemBytes":1,"maxPacketBytes":2,"autoCleanup":true,"shortAliasInstalled":false}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.show_window_shortcut, default_show_window_shortcut());
        assert_eq!(s.window_placement, WindowPlacement::Cursor);
    }

    #[test]
    fn sanitize_fixes_bad_values() {
        let s = Settings {
            global_shortcut: "  ".into(),
            show_window_shortcut: "  ".into(),
            packet_ttl_hours: 0,
            max_item_bytes: -1,
            max_packet_bytes: 0,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.global_shortcut, default_global_shortcut());
        // An empty show-window shortcut means "off" and stays empty.
        assert_eq!(s.show_window_shortcut, "");
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
        assert!(json.contains("showWindowShortcut"));
        assert!(json.contains(r#""windowPlacement":"cursor""#));
        assert!(json.contains("alwaysOnTop"));
        assert!(json.contains("showOnAllDesktops"));
    }
}
