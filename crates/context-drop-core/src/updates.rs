//! Decisions for the desktop app's update checks, kept pure so they can be
//! tested without Tauri: when an automatic check is due, and whether the
//! running app sits somewhere it can be replaced.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// An automatic check runs when this long has passed since the last
/// SUCCESSFUL one (wall clock, so a Mac's sleep does not stretch it).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// After a failed check (offline, GitHub down, no latest.json), the next try
/// waits this long, doubling with each further failure up to `CHECK_INTERVAL`
/// — so a check that keeps failing never runs more than once a day for long.
pub const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60 * 60);

/// When the automatic checks last ran. Kept in memory only: every launch
/// checks once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckClock {
    pub last_success: Option<SystemTime>,
    pub last_failure: Option<SystemTime>,
    /// Failures since the last success.
    pub failures: u32,
}

impl CheckClock {
    pub fn succeeded(&mut self, at: SystemTime) {
        self.last_success = Some(at);
        self.failures = 0;
    }

    pub fn failed(&mut self, at: SystemTime) {
        self.last_failure = Some(at);
        self.failures = self.failures.saturating_add(1);
    }

    /// How long to wait after the latest failure: 1 h, 2 h, 4 h, … ≤ 24 h.
    pub fn retry_delay(&self) -> Duration {
        let doublings = self.failures.saturating_sub(1).min(5);
        (RETRY_AFTER_FAILURE * 2u32.pow(doublings)).min(CHECK_INTERVAL)
    }

    /// Whether an automatic check should run at `now`.
    pub fn due(&self, enabled: bool, now: SystemTime) -> bool {
        if !enabled {
            return false;
        }
        let since = |t: SystemTime| now.duration_since(t).unwrap_or(Duration::ZERO);
        // After a failure newer than the last success, only the back-off
        // counts (a check that failed soon after a success is retried, not
        // left until a day after that success).
        if let Some(failed) = self.last_failure {
            if self.last_success.map_or(true, |ok| failed > ok) {
                return since(failed) >= self.retry_delay();
            }
        }
        match self.last_success {
            None => true,
            Some(ok) => since(ok) >= CHECK_INTERVAL,
        }
    }
}

/// Why the app at `exe` (the running executable) cannot be replaced by an
/// update, or `None` when it can. Only an installed macOS `.app` qualifies:
/// a dev build would have its whole `target/<profile>` directory replaced.
pub fn install_location_problem(exe: &Path) -> Option<&'static str> {
    let path = exe.to_string_lossy();
    if !path.contains(".app/Contents/MacOS/") {
        return Some(
            "Updates work only in the installed Context Drop app (not in a development build).",
        );
    }
    if path.contains("/AppTranslocation/") {
        return Some("Context Drop is running from a temporary location. Move it to the Applications folder, open it from there, and try again.");
    }
    if path.starts_with("/Volumes/") {
        return Some("Context Drop is running from the disk image. Move it to the Applications folder, open it from there, and try again.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + secs)
    }
    const HOUR: u64 = 60 * 60;

    // Tier A: the launch check runs; later ones wait a day of wall-clock time.
    #[test]
    fn first_check_is_due_then_once_a_day() {
        let mut c = CheckClock::default();
        assert!(c.due(true, at(0)));
        c.last_success = Some(at(0));
        assert!(!c.due(true, at(23 * HOUR)));
        assert!(c.due(true, at(24 * HOUR)));
    }

    // Tier A: turning automatic checks off stops them all.
    #[test]
    fn never_due_when_off() {
        assert!(!CheckClock::default().due(false, at(0)));
    }

    // Tier A: a failed check does not count as the day's check; it is retried
    // an hour later (not a day later, not on every tick).
    #[test]
    fn failure_retries_after_an_hour() {
        let mut c = CheckClock::default();
        c.failed(at(0));
        assert!(!c.due(true, at(10 * 60)));
        assert!(c.due(true, at(HOUR)));

        let mut c = CheckClock::default();
        c.succeeded(at(0));
        c.failed(at(25 * HOUR));
        assert!(!c.due(true, at(25 * HOUR + 30 * 60)));
        assert!(c.due(true, at(26 * HOUR)));
    }

    // Tier A: failures in a row back off (1 h, 2 h, 4 h … at most a day), so
    // a release without latest.json is not fetched every hour forever; a
    // success resets it.
    #[test]
    fn repeated_failures_back_off_to_a_day() {
        let mut c = CheckClock::default();
        let mut t = 0;
        for expected in [1, 2, 4, 8, 16, 24, 24] {
            c.failed(at(t));
            assert!(!c.due(true, at(t + expected * HOUR - 1)), "{expected}h");
            assert!(c.due(true, at(t + expected * HOUR)), "{expected}h");
            t += expected * HOUR;
        }
        c.succeeded(at(t));
        c.failed(at(t + 24 * HOUR));
        assert!(
            c.due(true, at(t + 25 * HOUR)),
            "a success resets the back-off"
        );
    }

    // Tier A: a (manual) check that fails within a day of a success is still
    // retried an hour later — the failure, not the day-old success, decides.
    #[test]
    fn failure_soon_after_a_success_is_retried_in_an_hour() {
        let mut c = CheckClock::default();
        c.succeeded(at(0));
        c.failed(at(HOUR));
        assert!(!c.due(true, at(2 * HOUR - 1)));
        assert!(c.due(true, at(2 * HOUR)));
    }

    // Tier A: an old failure (before the last success) changes nothing.
    #[test]
    fn old_failure_is_ignored() {
        let mut c = CheckClock::default();
        c.failed(at(HOUR));
        c.succeeded(at(2 * HOUR));
        assert!(!c.due(true, at(3 * HOUR)));
        assert!(c.due(true, at(26 * HOUR)));
    }

    // Tier A: a clock set backwards never makes checks run on every tick.
    #[test]
    fn clock_going_backwards_is_not_due() {
        let mut c = CheckClock::default();
        c.succeeded(at(10 * HOUR));
        assert!(!c.due(true, at(0)));
    }

    // Tier A: only an installed .app may be replaced.
    #[test]
    fn install_location() {
        let ok = Path::new("/Applications/Context Drop.app/Contents/MacOS/context-drop-desktop");
        assert_eq!(install_location_problem(ok), None);
        let home = Path::new(
            "/Users/me/Applications/Context Drop.app/Contents/MacOS/context-drop-desktop",
        );
        assert_eq!(install_location_problem(home), None);
        let dev = Path::new(
            "/Users/me/dev/context-drop/apps/desktop/src-tauri/target/debug/context-drop-desktop",
        );
        assert!(install_location_problem(dev)
            .unwrap()
            .contains("development build"));
        let dmg =
            Path::new("/Volumes/Context Drop/Context Drop.app/Contents/MacOS/context-drop-desktop");
        assert!(install_location_problem(dmg)
            .unwrap()
            .contains("disk image"));
        let tr = Path::new("/private/var/folders/x/T/AppTranslocation/ABC/d/Context Drop.app/Contents/MacOS/context-drop-desktop");
        assert!(install_location_problem(tr)
            .unwrap()
            .contains("temporary location"));
    }
}
