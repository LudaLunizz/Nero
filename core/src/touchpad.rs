//! Touchpad: the three-finger workspace swipe per monitor and the two-finger
//! scroll speed.
//!
//! GNOME's own three-finger swipe slides the workspaces of every monitor at
//! once. nero-workspaces switches GNOME's swipe off and runs its own swipe
//! tracker on the monitor under the pointer: the snap points are that monitor's
//! virtual workspaces, the end of the swipe says which one it shows next.
//!
//! GNOME has no setting for the two-finger scroll speed. libinput (1.30+) loads
//! Lua plugins, Mutter also from `~/.config/libinput/plugins`; the plugin in
//! `scroll_speed.lua` scales finger movement while exactly two fingers touch.
//! It is loaded at login, so a new speed works from the next login.

/// The Lua plugin with the factor left at 1.0.
pub const SCROLL_PLUGIN_TEMPLATE: &str = include_str!("scroll_speed.lua");

const FACTOR_LINE: &str = "local FACTOR = 1.0";

/// The snap points of a swipe over `count` virtual workspaces: 0, 1, .. count-1.
pub fn swipe_points(count: i64) -> Vec<f64> {
    (0..count.max(1)).map(|i| i as f64).collect()
}

/// The virtual workspace a swipe ends on: the snap point nearest `end_progress`,
/// inside 0 .. count-1.
pub fn swipe_target(end_progress: f64, count: i64) -> i64 {
    let last = (count.max(1) - 1) as f64;
    if !end_progress.is_finite() {
        return 0;
    }
    end_progress.round().clamp(0.0, last) as i64
}

/// The scroll speed setting (0..100 %) as a whole percentage.
pub fn scroll_percent(percent: f64) -> i64 {
    if !percent.is_finite() {
        return 100;
    }
    percent.round().clamp(0.0, 100.0) as i64
}

/// The plugin file for a scroll speed of `percent` (0..100), or `None` at 100 %:
/// then no plugin is needed and the file is removed.
pub fn scroll_plugin(percent: f64) -> Option<String> {
    let p = scroll_percent(percent);
    if p >= 100 {
        return None;
    }
    let factor = p as f64 / 100.0;
    let line = format!("local FACTOR = {factor} -- {p} %");
    Some(SCROLL_PLUGIN_TEMPLATE.replacen(FACTOR_LINE, &line, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_cover_every_workspace() {
        assert_eq!(swipe_points(4), vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(swipe_points(0), vec![0.0]);
    }

    #[test]
    fn target_rounds_and_clamps() {
        assert_eq!(swipe_target(1.0, 4), 1);
        assert_eq!(swipe_target(1.4, 4), 1);
        assert_eq!(swipe_target(1.6, 4), 2);
        assert_eq!(swipe_target(-0.7, 4), 0);
        assert_eq!(swipe_target(9.0, 4), 3);
        assert_eq!(swipe_target(f64::NAN, 4), 0);
    }

    #[test]
    fn template_has_the_factor_line_once() {
        assert_eq!(SCROLL_PLUGIN_TEMPLATE.matches(FACTOR_LINE).count(), 1);
    }

    #[test]
    fn plugin_carries_the_factor() {
        let lua = scroll_plugin(35.0).unwrap();
        assert!(lua.contains("local FACTOR = 0.35 -- 35 %"));
        assert!(!lua.contains(FACTOR_LINE));
        assert!(
            scroll_plugin(0.0)
                .unwrap()
                .contains("local FACTOR = 0 -- 0 %")
        );
        assert!(scroll_plugin(-5.0).unwrap().contains("-- 0 %"));
    }

    #[test]
    fn full_speed_needs_no_plugin() {
        assert_eq!(scroll_plugin(100.0), None);
        assert_eq!(scroll_plugin(140.0), None);
        assert_eq!(scroll_plugin(f64::NAN), None);
        assert_eq!(scroll_percent(99.6), 100);
    }
}
