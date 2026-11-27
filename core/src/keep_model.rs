//! keep-onscreen's decisions. There is no state to keep here (the extension only
//! holds signal ids), so these are plain functions: JS reports what GNOME shows,
//! the core says what to do.
//!
//! - [`fit`]: does a new window have to be shrunk and moved, and to where
//! - [`prompt_before`] / [`prompt_after`]: GNOME's password prompts, whether the
//!   dark overlay is switched off and which monitor the prompt is moved to
//! - [`prompt_choices`]: the list of the "password prompts open on" setting

use crate::{Rect, fit::fit_to_work_area};
use serde::{Deserialize, Serialize};

/// A new window at its first frame.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FitWin {
    /// The name of its `Meta.WindowType` (NORMAL, DIALOG, ...).
    pub window_type: String,
    pub monitor: i64,
    pub frame: Rect,
    pub work_area: Rect,
    /// Maximized in at least one direction.
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

/// The window types that are kept on screen.
const FIT_TYPES: [&str; 3] = ["NORMAL", "DIALOG", "MODAL_DIALOG"];

/// The frame the window has to be moved and resized to, or None to leave it:
/// it is maximized or full screen, is not a normal window or dialog, has no
/// monitor, or already fits.
pub fn fit(w: &FitWin) -> Option<Rect> {
    if w.maximized || w.fullscreen || w.monitor < 0 || !FIT_TYPES.contains(&w.window_type.as_str())
    {
        return None;
    }
    let r = fit_to_work_area(&w.frame, &w.work_area);
    (r != w.frame).then_some(r)
}

/// GNOME Shell's own password dialogs. Matched by class name: if GNOME renames
/// one, that prompt just opens under the pointer as before. A third-party polkit
/// agent shows a normal window instead, which is not touched.
const PASSWORD_DIALOGS: [&str; 4] = [
    "AuthenticationDialog",
    "KeyringDialog",
    "NetworkSecretDialog",
    "ShellMountPasswordDialog",
];

pub fn is_password_dialog(class_name: &str) -> bool {
    PASSWORD_DIALOGS.contains(&class_name)
}

/// Before the dialog opens: should its overlay (lightbox) be switched off?
/// `dim` is the password-prompt-dim setting.
pub fn prompt_before(class_name: &str, dim: bool) -> bool {
    is_password_dialog(class_name) && !dim
}

/// After the dialog opened: the monitor to move it to, or None to leave it.
/// `monitor` is the index of the monitor with the connector from
/// password-prompt-monitor (negative: none set, or not connected now).
pub fn prompt_after(class_name: &str, monitor: i64) -> Option<i64> {
    (is_password_dialog(class_name) && monitor >= 0).then_some(monitor)
}

// ---- the settings window ------------------------------------------------------

/// A connected monitor, as the settings window sees it (logical size and place).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Monitor {
    pub connector: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub width: i64,
    pub height: i64,
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Choices {
    pub labels: Vec<String>,
    /// What goes into the setting for each label.
    pub values: Vec<String>,
    /// The index of the saved value in `values`.
    pub selected: i64,
}

fn place(i: usize, monitors: &[&Monitor]) -> String {
    if monitors.len() < 2 {
        return String::new();
    }
    // Left to right, top to bottom for monitors in one column.
    let mut order: Vec<usize> = (0..monitors.len()).collect();
    order.sort_by_key(|&j| (monitors[j].x, monitors[j].y));
    let pos = order.iter().position(|&j| j == i).unwrap_or(0);
    if pos == 0 {
        "leftmost".into()
    } else if pos == order.len() - 1 {
        "rightmost".into()
    } else {
        match pos {
            1 => "2nd from the left".into(),
            2 => "3rd from the left".into(),
            n => format!("{}th from the left", n + 1),
        }
    }
}

fn label(i: usize, monitors: &[&Monitor]) -> String {
    let m = monitors[i];
    let mut parts = vec![m.connector.clone().unwrap_or_default()];
    if let Some(n) = m.name.as_deref().filter(|n| !n.is_empty()) {
        parts.push(n.to_string());
    }
    parts.push(format!("{}×{}", m.width, m.height));
    let where_ = place(i, monitors);
    if !where_.is_empty() {
        parts.push(where_);
    }
    parts.join(" · ")
}

/// The choices of the "password prompts open on" list: GNOME's default, then
/// every connected monitor. `saved` is the connector in password-prompt-monitor
/// ('' or None = off).
pub fn prompt_choices(monitors: &[Monitor], saved: Option<&str>) -> Choices {
    let mut labels = vec!["Under the pointer (GNOME default)".to_string()];
    let mut values = vec![String::new()];
    let named: Vec<&Monitor> = monitors
        .iter()
        .filter(|m| m.connector.as_deref().is_some_and(|c| !c.is_empty()))
        .collect();
    for (i, m) in named.iter().enumerate() {
        let c = m.connector.clone().unwrap_or_default();
        if values.contains(&c) {
            continue;
        }
        labels.push(label(i, &named));
        values.push(c);
    }
    // A saved monitor that is unplugged now stays in the list (and selected), so
    // opening the settings does not silently forget it; prompts open under the
    // pointer meanwhile.
    let saved = saved.unwrap_or("");
    if !saved.is_empty() && !values.iter().any(|v| v == saved) {
        labels.push(format!(
            "{saved} (not connected now: prompts open under the pointer)"
        ));
        values.push(saved.to_string());
    }
    let selected = values
        .iter()
        .position(|v| v == saved)
        .map_or(-1, |p| p as i64);
    Choices {
        labels,
        values,
        selected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(window_type: &str, frame: Rect) -> FitWin {
        FitWin {
            window_type: window_type.into(),
            monitor: 0,
            frame,
            work_area: Rect::new(0.0, 32.0, 1000.0, 600.0),
            maximized: false,
            fullscreen: false,
        }
    }

    #[test]
    fn fit_moves_and_shrinks() {
        let w = win("NORMAL", Rect::new(-50.0, 0.0, 1200.0, 400.0));
        assert_eq!(fit(&w), Some(Rect::new(0.0, 32.0, 1000.0, 400.0)));
        // already inside: nothing
        assert_eq!(
            fit(&win("DIALOG", Rect::new(10.0, 40.0, 100.0, 100.0))),
            None
        );
    }

    #[test]
    fn fit_leaves_what_it_should() {
        let off = Rect::new(-50.0, 0.0, 1200.0, 400.0);
        for t in ["DOCK", "MENU", "SPLASHSCREEN", "UTILITY", "DESKTOP"] {
            assert_eq!(fit(&win(t, off)), None, "{t}");
        }
        for t in ["NORMAL", "DIALOG", "MODAL_DIALOG"] {
            assert!(fit(&win(t, off)).is_some(), "{t}");
        }
        let mut w = win("NORMAL", off);
        w.maximized = true;
        assert_eq!(fit(&w), None);
        w.maximized = false;
        w.fullscreen = true;
        assert_eq!(fit(&w), None);
        w.fullscreen = false;
        w.monitor = -1;
        assert_eq!(fit(&w), None);
    }

    #[test]
    fn password_prompts() {
        assert!(prompt_before("AuthenticationDialog", false));
        assert!(!prompt_before("AuthenticationDialog", true));
        assert!(!prompt_before("SomeOtherDialog", false));
        assert_eq!(prompt_after("KeyringDialog", 1), Some(1));
        assert_eq!(prompt_after("KeyringDialog", 0), Some(0));
        assert_eq!(prompt_after("KeyringDialog", -1), None);
        assert_eq!(prompt_after("SomeOtherDialog", 1), None);
        for c in ["NetworkSecretDialog", "ShellMountPasswordDialog"] {
            assert!(is_password_dialog(c));
        }
    }

    fn mon(
        connector: Option<&str>,
        name: &str,
        width: i64,
        height: i64,
        x: i64,
        y: i64,
    ) -> Monitor {
        Monitor {
            connector: connector.map(String::from),
            name: Some(name.into()),
            width,
            height,
            x,
            y,
        }
    }

    fn three() -> Vec<Monitor> {
        vec![
            mon(Some("DP-1"), "Dell U2415", 1920, 1200, 1440, 0),
            mon(Some("DP-3"), "", 1440, 2560, 0, 0),
            mon(Some("HDMI-2"), "7inch", 1024, 600, 3360, 0),
        ]
    }

    #[test]
    fn choices_off_lists_every_monitor() {
        let c = prompt_choices(&three(), Some(""));
        assert_eq!(c.selected, 0);
        assert_eq!(c.values, ["", "DP-1", "DP-3", "HDMI-2"]);
        assert_eq!(
            c.labels[1],
            "DP-1 · Dell U2415 · 1920×1200 · 2nd from the left"
        );
        assert_eq!(c.labels[2], "DP-3 · 1440×2560 · leftmost");
        assert!(c.labels[3].ends_with("rightmost"));
    }

    #[test]
    fn choices_keep_the_saved_monitor() {
        assert_eq!(prompt_choices(&three(), Some("HDMI-2")).selected, 3);
        let c = prompt_choices(&three(), Some("HDMI-1"));
        assert_eq!((c.values.len(), c.selected), (5, 4));
        assert!(c.labels[4].starts_with("HDMI-1 (not connected"));
    }

    #[test]
    fn choices_edge_cases() {
        let m = vec![
            mon(Some("DP-1"), "Dell U2415", 1920, 1200, 0, 0),
            mon(None, "", 1, 1, 0, 0),
        ];
        let c = prompt_choices(&m, None);
        assert_eq!(c.values, ["", "DP-1"]);
        assert_eq!(c.labels[1], "DP-1 · Dell U2415 · 1920×1200");
        assert_eq!(c.selected, 0);
        // a repeated connector is listed once; four monitors get "4th"; one column sorts by y
        let m = vec![
            mon(Some("A"), "", 1, 1, 0, 0),
            mon(Some("A"), "", 1, 1, 5, 0),
            mon(Some("B"), "", 1, 1, 10, 0),
            mon(Some("C"), "", 1, 1, 15, 0),
            mon(Some("D"), "", 1, 1, 20, 0),
        ];
        let c = prompt_choices(&m, Some(""));
        assert_eq!(c.values, ["", "A", "B", "C", "D"]);
        assert_eq!(c.labels[3], "C · 1×1 · 4th from the left");
        let col = vec![
            mon(Some("T"), "", 1, 1, 0, 100),
            mon(Some("U"), "", 1, 1, 0, 0),
        ];
        assert!(prompt_choices(&col, None).labels[1].ends_with("rightmost"));
    }
}
