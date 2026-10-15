//! The app's global shortcuts: which action each one triggers, which of them
//! may be registered, and which action a key press routes to.
//!
//! Pure so it can be tested without the OS hotkey registry: the desktop app
//! passes in how an accelerator string parses (`key_of`) and whether the OS
//! accepted a registration.

use crate::settings::Settings;

/// What a global shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    ToggleCapture,
    ToggleWindow,
    ToggleAlwaysOnTop,
}

impl ShortcutAction {
    /// Registration order: when two actions share a key combination the
    /// earlier one keeps it (Capture first, as before v0.1.9).
    pub const ALL: [ShortcutAction; 3] = [
        ShortcutAction::ToggleCapture,
        ShortcutAction::ToggleWindow,
        ShortcutAction::ToggleAlwaysOnTop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShortcutAction::ToggleCapture => "Start / Stop Capture",
            ShortcutAction::ToggleWindow => "Show / hide window",
            ShortcutAction::ToggleAlwaysOnTop => "Always on Top",
        }
    }

    /// Position in `ALL` (and in any per-action list built in that order).
    pub fn index(self) -> usize {
        match self {
            ShortcutAction::ToggleCapture => 0,
            ShortcutAction::ToggleWindow => 1,
            ShortcutAction::ToggleAlwaysOnTop => 2,
        }
    }

    /// This action's accelerator in a list built in `ALL` order ("" if absent).
    pub fn accelerator_in(self, config: &[String]) -> &str {
        config.get(self.index()).map(String::as_str).unwrap_or("")
    }

    /// The accelerator configured for this action ("" = off).
    pub fn accelerator(self, s: &Settings) -> &str {
        match self {
            ShortcutAction::ToggleCapture => &s.global_shortcut,
            ShortcutAction::ToggleWindow => &s.show_window_shortcut,
            ShortcutAction::ToggleAlwaysOnTop => &s.on_top_shortcut,
        }
    }
}

/// What to do with one action's shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Empty: the shortcut is turned off (not an error).
    Off,
    /// Not a valid accelerator.
    Invalid,
    /// Same key combination as an earlier action's shortcut; refused so it
    /// can never steal that action's presses.
    Duplicate(ShortcutAction),
    /// Register this key (its parsed id).
    Register(u32),
}

/// Decide, for every action in `ShortcutAction::ALL` order, what to register.
/// `key_of` parses an accelerator into its key id (None when invalid).
pub fn plan(
    settings: &Settings,
    key_of: impl Fn(&str) -> Option<u32>,
) -> Vec<(ShortcutAction, Slot)> {
    let mut taken: Vec<(u32, ShortcutAction)> = Vec::new();
    ShortcutAction::ALL
        .iter()
        .map(|&action| {
            let acc = action.accelerator(settings).trim();
            let slot = if acc.is_empty() {
                Slot::Off
            } else {
                match key_of(acc) {
                    None => Slot::Invalid,
                    Some(id) => match taken.iter().find(|(t, _)| *t == id) {
                        Some((_, owner)) => Slot::Duplicate(*owner),
                        None => {
                            taken.push((id, action));
                            Slot::Register(id)
                        }
                    },
                }
            };
            (action, slot)
        })
        .collect()
}

/// The final duplicate check for a save: if any action in `changed` would end
/// up with the same key combination as another action in `settings` (the
/// settings as they would be saved), return (changed action, other action).
/// The backend runs this under its settings lock, so it holds whatever the UI
/// believed about saves still in flight or failed.
pub fn duplicate_after_change(
    settings: &Settings,
    changed: &[ShortcutAction],
    key_of: impl Fn(&str) -> Option<u32>,
) -> Option<(ShortcutAction, ShortcutAction)> {
    let id = |a: ShortcutAction| {
        let acc = a.accelerator(settings).trim();
        if acc.is_empty() {
            None
        } else {
            key_of(acc)
        }
    };
    for &a in changed {
        let Some(mine) = id(a) else { continue };
        for other in ShortcutAction::ALL {
            if other != a && id(other) == Some(mine) {
                return Some((a, other));
            }
        }
    }
    None
}

/// The action a pressed key triggers: only keys that were actually registered
/// route anywhere (a refused or failed shortcut never does).
pub fn route(registered: &[(ShortcutAction, u32)], pressed: u32) -> Option<ShortcutAction> {
    registered
        .iter()
        .find(|(_, id)| *id == pressed)
        .map(|(action, _)| *action)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake parser: accelerators are compared case-insensitively, with
    /// "Cmd" and "CommandOrControl" meaning the same modifier (as on macOS).
    fn key_of(s: &str) -> Option<u32> {
        if s.contains("??") {
            return None;
        }
        let norm = s.to_ascii_lowercase().replace("commandorcontrol", "cmd");
        Some(
            norm.bytes()
                .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)),
        )
    }

    fn settings(capture: &str, show: &str, on_top: &str) -> Settings {
        Settings {
            global_shortcut: capture.into(),
            show_window_shortcut: show.into(),
            on_top_shortcut: on_top.into(),
            ..Settings::default()
        }
    }

    // Tier A: the defaults are three distinct, registrable shortcuts.
    #[test]
    fn defaults_register_all_three() {
        let p = plan(&Settings::default(), key_of);
        assert_eq!(p.len(), 3);
        assert!(
            p.iter().all(|(_, s)| matches!(s, Slot::Register(_))),
            "{p:?}"
        );
    }

    // Tier A: a later shortcut equal to an earlier one is refused and names
    // the owner; the earlier one keeps the key.
    #[test]
    fn duplicate_is_refused_and_earlier_action_keeps_the_key() {
        let p = plan(
            &settings("Cmd+Shift+9", "CommandOrControl+Shift+9", "Cmd+Shift+8"),
            key_of,
        );
        assert!(matches!(
            p[0],
            (ShortcutAction::ToggleCapture, Slot::Register(_))
        ));
        assert_eq!(
            p[1],
            (
                ShortcutAction::ToggleWindow,
                Slot::Duplicate(ShortcutAction::ToggleCapture)
            )
        );
        assert!(matches!(
            p[2],
            (ShortcutAction::ToggleAlwaysOnTop, Slot::Register(_))
        ));
    }

    // Tier A: empty means off (not invalid, not a duplicate of another off).
    #[test]
    fn empty_shortcuts_are_off_and_never_collide() {
        let p = plan(&settings("", "  ", ""), key_of);
        assert!(p.iter().all(|(_, s)| *s == Slot::Off), "{p:?}");
    }

    // Tier A: an unparsable value is reported, and does not block a later one.
    #[test]
    fn invalid_is_reported_separately() {
        let p = plan(&settings("??", "Cmd+Shift+0", "Cmd+Shift+8"), key_of);
        assert_eq!(p[0].1, Slot::Invalid);
        assert!(matches!(p[1].1, Slot::Register(_)));
    }

    // Tier A: the save-time check refuses a changed shortcut that equals any
    // other one (in any spelling), and ignores off ones and unchanged rows.
    #[test]
    fn save_time_duplicate_check() {
        let s = settings("Cmd+Shift+9", "CommandOrControl+Shift+9", "");
        assert_eq!(
            duplicate_after_change(&s, &[ShortcutAction::ToggleCapture], key_of),
            Some((ShortcutAction::ToggleCapture, ShortcutAction::ToggleWindow))
        );
        let ok = settings("Cmd+Shift+9", "Cmd+Shift+0", "");
        assert_eq!(
            duplicate_after_change(&ok, &[ShortcutAction::ToggleAlwaysOnTop], key_of),
            None
        );
        // An existing duplicate elsewhere does not block saving an unrelated row.
        let old_dup = settings("Cmd+Shift+9", "Cmd+Shift+9", "Cmd+Shift+8");
        assert_eq!(
            duplicate_after_change(&old_dup, &[ShortcutAction::ToggleAlwaysOnTop], key_of),
            None
        );
    }

    // Tier A: routing only by registered ids — a refused duplicate never
    // routes, so the capture key always toggles capture.
    #[test]
    fn route_uses_only_registered_ids() {
        let s = settings("Cmd+Shift+9", "Cmd+Shift+9", "Cmd+Shift+8");
        let registered: Vec<(ShortcutAction, u32)> = plan(&s, key_of)
            .into_iter()
            .filter_map(|(a, slot)| match slot {
                Slot::Register(id) => Some((a, id)),
                _ => None,
            })
            .collect();
        let nine = key_of("Cmd+Shift+9").unwrap();
        let eight = key_of("Cmd+Shift+8").unwrap();
        assert_eq!(
            route(&registered, nine),
            Some(ShortcutAction::ToggleCapture)
        );
        assert_eq!(
            route(&registered, eight),
            Some(ShortcutAction::ToggleAlwaysOnTop)
        );
        assert_eq!(route(&registered, key_of("Cmd+Shift+7").unwrap()), None);
    }
}
