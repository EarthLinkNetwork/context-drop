//! Where to put the window on a multi-display desktop.
//!
//! Pure geometry so it can be tested without a window system. Every rect and
//! point is in ONE global coordinate space with a top-left origin — on macOS
//! that is points (the desktop app converts each monitor's physical values
//! with that monitor's own scale factor), elsewhere physical pixels.

use crate::settings::WindowPlacement;

/// Gap kept between the window and the edge of the usable screen area.
pub const EDGE_MARGIN: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// Squared distance from a point to this rect (0 when inside).
    fn distance2(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(0.0).max(x - (self.x + self.w));
        let dy = (self.y - y).max(0.0).max(y - (self.y + self.h));
        dx * dx + dy * dy
    }

    fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// One connected display.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    pub name: String,
    /// The whole screen.
    pub frame: Rect,
    /// The usable area (without the menu bar / Dock / taskbar).
    pub work: Rect,
}

/// A stable-ish identifier for a display: its name plus its origin, so two
/// identical monitors can still be told apart. If the arrangement changes the
/// origin part stops matching and `find_display` falls back to the name.
pub fn display_key(name: &str, frame: &Rect) -> String {
    format!(
        "{}@{},{}",
        name,
        frame.x.round() as i64,
        frame.y.round() as i64
    )
}

/// The display a saved key refers to: exact key first, then the first display
/// with the same name (the arrangement changed), else none (disconnected).
pub fn find_display(displays: &[Display], key: &str) -> Option<usize> {
    if key.is_empty() {
        return None;
    }
    if let Some(i) = displays
        .iter()
        .position(|d| display_key(&d.name, &d.frame) == key)
    {
        return Some(i);
    }
    let name = key.rsplit_once('@').map(|(n, _)| n).unwrap_or(key);
    displays.iter().position(|d| d.name == name)
}

/// The display containing the point, or the nearest one when the point falls
/// in a gap between displays.
pub fn display_near(displays: &[Display], x: f64, y: f64) -> Option<usize> {
    // Containment first: on a shared edge both displays are at distance 0, and
    // only `contains` (half-open) says which one the point is really on.
    if let Some(i) = displays.iter().position(|d| d.frame.contains(x, y)) {
        return Some(i);
    }
    displays
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.frame.distance2(x, y).total_cmp(&b.frame.distance2(x, y)))
        .map(|(i, _)| i)
}

/// Top-left position for a `win_w` x `win_h` window placed at the top of the
/// usable area `work`, horizontally centered on `center_x`, kept fully inside
/// `work` when it fits (pinned to the top-left corner when it does not).
pub fn position_in(work: &Rect, win_w: f64, win_h: f64, center_x: f64) -> (f64, f64) {
    let max_x = (work.x + work.w - win_w - EDGE_MARGIN).max(work.x);
    let max_y = (work.y + work.h - win_h).max(work.y);
    let x = (center_x - win_w / 2.0).clamp((work.x + EDGE_MARGIN).min(max_x), max_x);
    let y = (work.y + EDGE_MARGIN).min(max_y);
    (x, y)
}

/// What to do with the window before showing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Plan {
    /// Leave it where it is.
    Keep,
    /// Move its top-left corner here.
    MoveTo(f64, f64),
}

/// Decide where the window goes when it is shown.
///
/// - `Cursor`: the display the pointer is on, under the pointer (a tray click
///   therefore lands under the menu-bar icon of the clicked screen).
/// - `Display`: the chosen display, top-right; falls back to `Cursor` when
///   that display is not connected.
/// - `Last`: never moved (it stays wherever the user left it).
///
/// If the window is already on the target display it is kept where it is, so
/// a position the user picked on that screen is respected.
pub fn plan_show(
    placement: WindowPlacement,
    displays: &[Display],
    cursor: Option<(f64, f64)>,
    chosen_display: &str,
    window: Rect,
) -> Plan {
    let target = match placement {
        WindowPlacement::Last => return Plan::Keep,
        WindowPlacement::Display => find_display(displays, chosen_display)
            .map(|i| (i, displays[i].work.x + displays[i].work.w)),
        WindowPlacement::Cursor => None,
    };
    let target = target.or_else(|| {
        let (cx, cy) = cursor?;
        display_near(displays, cx, cy).map(|i| (i, cx))
    });
    let Some((i, center_x)) = target else {
        return Plan::Keep;
    };
    let d = &displays[i];
    let (wx, wy) = window.center();
    if d.frame.contains(wx, wy) {
        return Plan::Keep;
    }
    let (x, y) = position_in(&d.work, window.w, window.h, center_x);
    Plan::MoveTo(x, y)
}

/// Whether a remembered top-left position is still reachable: its title bar
/// must be on some connected display (a display may have been unplugged).
pub fn position_is_visible(displays: &[Display], x: f64, y: f64, win_w: f64) -> bool {
    let probe_x = x + (win_w / 2.0).min(40.0);
    let probe_y = y + 10.0;
    displays.iter().any(|d| d.frame.contains(probe_x, probe_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// A MacBook (main, left) and an external monitor to its right whose top
    /// is higher, as in a typical desk arrangement.
    fn two_displays() -> Vec<Display> {
        vec![
            Display {
                name: "Built-in Retina Display".into(),
                frame: rect(0.0, 0.0, 1512.0, 982.0),
                work: rect(0.0, 33.0, 1512.0, 949.0),
            },
            Display {
                name: "DELL U2720Q".into(),
                frame: rect(1512.0, -200.0, 2560.0, 1440.0),
                work: rect(1512.0, -175.0, 2560.0, 1415.0),
            },
        ]
    }

    const WIN_ON_MAIN: Rect = Rect {
        x: 500.0,
        y: 200.0,
        w: 380.0,
        h: 660.0,
    };

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn cursor_on_other_display_moves_window_under_cursor() {
        let d = two_displays();
        let plan = plan_show(
            WindowPlacement::Cursor,
            &d,
            Some((3000.0, 300.0)),
            "",
            WIN_ON_MAIN,
        );
        assert_eq!(plan, Plan::MoveTo(3000.0 - 190.0, -175.0 + EDGE_MARGIN));
        let Plan::MoveTo(x, y) = plan else {
            unreachable!()
        };
        let moved = rect(x, y, WIN_ON_MAIN.w, WIN_ON_MAIN.h);
        let (cx, cy) = moved.center();
        assert!(
            d[1].frame.contains(cx, cy),
            "window must land on the cursor's display"
        );
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn cursor_on_same_display_keeps_user_position() {
        let d = two_displays();
        let plan = plan_show(
            WindowPlacement::Cursor,
            &d,
            Some((100.0, 900.0)),
            "",
            WIN_ON_MAIN,
        );
        assert_eq!(plan, Plan::Keep);
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn tray_click_at_right_edge_is_clamped_inside_the_screen() {
        let d = two_displays();
        // Menu-bar icon near the far right edge of the external display.
        let plan = plan_show(
            WindowPlacement::Cursor,
            &d,
            Some((4060.0, -190.0)),
            "",
            WIN_ON_MAIN,
        );
        let Plan::MoveTo(x, y) = plan else {
            panic!("expected a move, got {plan:?}")
        };
        assert_eq!(x, 1512.0 + 2560.0 - 380.0 - EDGE_MARGIN);
        assert_eq!(y, -175.0 + EDGE_MARGIN);
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn chosen_display_wins_over_cursor() {
        let d = two_displays();
        let key = display_key(&d[1].name, &d[1].frame);
        // Cursor is on the main display, but the external one was chosen.
        let plan = plan_show(
            WindowPlacement::Display,
            &d,
            Some((100.0, 100.0)),
            &key,
            WIN_ON_MAIN,
        );
        assert_eq!(
            plan,
            Plan::MoveTo(1512.0 + 2560.0 - 380.0 - EDGE_MARGIN, -175.0 + EDGE_MARGIN)
        );
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn disconnected_chosen_display_falls_back_to_cursor() {
        let d = two_displays();
        let plan = plan_show(
            WindowPlacement::Display,
            &d,
            Some((3000.0, 300.0)),
            "LG UltraFine@-1920,0",
            WIN_ON_MAIN,
        );
        assert_eq!(plan, Plan::MoveTo(3000.0 - 190.0, -175.0 + EDGE_MARGIN));
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn rearranged_display_is_found_by_name() {
        let d = two_displays();
        assert_eq!(find_display(&d, "DELL U2720Q@-2560,0"), Some(1));
        assert_eq!(
            find_display(&d, &display_key(&d[0].name, &d[0].frame)),
            Some(0)
        );
        assert_eq!(find_display(&d, ""), None);
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn identical_monitors_are_told_apart_by_origin() {
        let mut d = two_displays();
        d[0].name = "DELL U2720Q".into();
        let key = display_key(&d[1].name, &d[1].frame);
        assert_eq!(find_display(&d, &key), Some(1));
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn last_placement_never_moves() {
        let d = two_displays();
        let plan = plan_show(
            WindowPlacement::Last,
            &d,
            Some((3000.0, 300.0)),
            "",
            WIN_ON_MAIN,
        );
        assert_eq!(plan, Plan::Keep);
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn no_cursor_or_no_displays_keeps_window() {
        let d = two_displays();
        assert_eq!(
            plan_show(WindowPlacement::Cursor, &d, None, "", WIN_ON_MAIN),
            Plan::Keep
        );
        assert_eq!(
            plan_show(
                WindowPlacement::Cursor,
                &[],
                Some((1.0, 1.0)),
                "",
                WIN_ON_MAIN
            ),
            Plan::Keep
        );
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn cursor_in_gap_between_displays_uses_nearest() {
        let d = two_displays();
        // Below the external display's bottom edge (y >= 1240), right of main.
        assert_eq!(display_near(&d, 2000.0, 1300.0), Some(1));
    }

    // Tier A: a pointer on the first column of the right-hand display is on
    // that display, not on the main one whose right edge it touches.
    #[test]
    fn pointer_on_shared_edge_belongs_to_the_display_it_is_on() {
        let d = two_displays();
        assert_eq!(display_near(&d, 1512.0, 300.0), Some(1));
        assert_eq!(display_near(&d, 1511.0, 300.0), Some(0));
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn window_larger_than_screen_is_pinned_top_left() {
        let work = rect(0.0, 25.0, 300.0, 400.0);
        assert_eq!(position_in(&work, 380.0, 660.0, 150.0), (0.0, 25.0));
    }

    // Tier A: geometry the window lands by; a wrong value puts it on the wrong screen.
    #[test]
    fn remembered_position_on_unplugged_display_is_rejected() {
        let d = two_displays();
        assert!(position_is_visible(&d, 2000.0, 100.0, 380.0));
        assert!(!position_is_visible(&d, 5000.0, 100.0, 380.0));
        assert!(!position_is_visible(&d, -2000.0, 100.0, 380.0));
    }
}
