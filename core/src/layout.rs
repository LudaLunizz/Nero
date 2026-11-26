//! Layout saver: a window's place as a share of its monitor's work area (so it
//! survives a panel or resolution change), and which saved window a newly
//! opened window takes after a login.

use crate::{Rect, fit::fit_to_work_area, js_round};
use serde::{Deserialize, Serialize};

/// A window's frame as fractions of its monitor's work area.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub fx: f64,
    pub fy: f64,
    pub fw: f64,
    pub fh: f64,
}

/// The frame as fractions of the work area (4 decimals: a pixel on an 8K
/// screen, and the saved file stays readable).
pub fn to_spot(frame: &Rect, work_area: &Rect) -> Spot {
    let q = |v: f64, of: f64| {
        if of > 0.0 {
            js_round(v / of * 10000.0) / 10000.0
        } else {
            0.0
        }
    };
    Spot {
        fx: q(frame.x - work_area.x, work_area.width),
        fy: q(frame.y - work_area.y, work_area.height),
        fw: q(frame.width, work_area.width),
        fh: q(frame.height, work_area.height),
    }
}

/// The spot back in pixels on this work area, fully inside it.
pub fn from_spot(spot: &Spot, work_area: &Rect) -> Rect {
    let frame = Rect::new(
        work_area.x + js_round(spot.fx * work_area.width),
        work_area.y + js_round(spot.fy * work_area.height),
        js_round(spot.fw * work_area.width).max(1.0),
        js_round(spot.fh * work_area.height).max(1.0),
    );
    fit_to_work_area(&frame, work_area)
}

/// What matching needs from a saved window (the saved file has more fields).
#[derive(Clone, Debug, Deserialize)]
pub struct Saved {
    pub app: String,
    #[serde(default)]
    pub title: String,
}

fn words(s: &str) -> Vec<String> {
    let mut w: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    w.sort();
    w.dedup();
    w
}

/// How alike two titles are: 2 when equal, else the share of words they have
/// in common (0..=1).
pub fn title_score(a: &str, b: &str) -> f64 {
    if a == b {
        return 2.0;
    }
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let common = wa.iter().filter(|w| wb.contains(w)).count() as f64;
    common * 2.0 / (wa.len() + wb.len()) as f64
}

/// The saved window a new window of `app` with `title` should take: one of the
/// same app that is not `used` yet, the best title match first; with equal
/// scores (or no title in common) the one saved first.
pub fn match_window(saved: &[Saved], used: &[bool], app: &str, title: &str) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, s) in saved.iter().enumerate() {
        if s.app != app || used.get(i).copied().unwrap_or(false) {
            continue;
        }
        let score = title_score(&s.title, title);
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

/// A window that took a saved window during a restore: its place in the saved
/// list (the list is in stacking order, bottom first) and the monitor it is on.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Placed {
    pub index: usize,
    pub monitor: i32,
}

/// Windows open in whatever order the apps start, and each new one lands on
/// top. After window `new_index` is placed, raise these (saved indices, in
/// this order): the new window and every placed window on its monitor that
/// was saved above it, bottom first. That puts back the saved stacking
/// among them without touching other monitors or windows that were not saved.
pub fn restack_order(placed: &[Placed], new_index: usize) -> Vec<usize> {
    let Some(monitor) = placed
        .iter()
        .find(|p| p.index == new_index)
        .map(|p| p.monitor)
    else {
        return Vec::new();
    };
    let mut order: Vec<usize> = placed
        .iter()
        .filter(|p| p.monitor == monitor && p.index >= new_index)
        .map(|p| p.index)
        .collect();
    order.sort_unstable();
    order.dedup();
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(index: usize, monitor: i32) -> Placed {
        Placed { index, monitor }
    }

    #[test]
    fn restacking() {
        // saved bottom first: 4, 5 (verticals) below 6, 7 (horizontals) on monitor 0,
        // 2 on monitor 1. Window 6 opened late, above 7: raise 6, then 7 over it.
        let placed = [pl(7, 0), pl(4, 0), pl(2, 1), pl(5, 0), pl(6, 0)];
        assert_eq!(restack_order(&placed, 6), vec![6, 7]);
        // the lowest one opened last: everything above it on its monitor goes back up
        assert_eq!(restack_order(&placed, 4), vec![4, 5, 6, 7]);
        // the top one: only itself
        assert_eq!(restack_order(&placed, 7), vec![7]);
        // alone on its monitor
        assert_eq!(restack_order(&placed, 2), vec![2]);
        // not placed: nothing
        assert_eq!(restack_order(&placed, 9), Vec::<usize>::new());
    }

    const WA: Rect = Rect::new(1080.0, 1120.0, 2560.0, 1392.0);

    #[test]
    fn spot_round_trip() {
        let f = Rect::new(1380.0, 1200.0, 1200.0, 800.0);
        let s = to_spot(&f, &WA);
        assert_eq!(from_spot(&s, &WA), f);
    }

    #[test]
    fn spot_follows_a_smaller_work_area_and_stays_inside() {
        let s = Spot {
            fx: 0.5,
            fy: 0.5,
            fw: 0.8,
            fh: 0.8,
        };
        let r = from_spot(&s, &Rect::new(0.0, 0.0, 1000.0, 500.0));
        assert_eq!(r, Rect::new(200.0, 100.0, 800.0, 400.0));
    }

    #[test]
    fn zero_work_area_gives_zero_spot() {
        let s = to_spot(
            &Rect::new(5.0, 5.0, 5.0, 5.0),
            &Rect::new(0.0, 0.0, 0.0, 0.0),
        );
        assert_eq!(s.fw, 0.0);
    }

    fn sv(app: &str, title: &str) -> Saved {
        Saved {
            app: app.into(),
            title: title.into(),
        }
    }

    #[test]
    fn matching() {
        let saved = [
            sv("brave.desktop", "Inbox - Mail - Brave"),
            sv("org.gnome.Nautilus.desktop", "Downloads"),
            sv("brave.desktop", "GitHub - Nero - Brave"),
        ];
        let none = [false; 3];
        // exact title wins over order
        assert_eq!(
            match_window(&saved, &none, "brave.desktop", "GitHub - Nero - Brave"),
            Some(2)
        );
        // closest title
        assert_eq!(
            match_window(
                &saved,
                &none,
                "brave.desktop",
                "Pull requests - Nero - Brave"
            ),
            Some(2)
        );
        // nothing in common: first saved
        assert_eq!(
            match_window(&saved, &none, "brave.desktop", "New Tab"),
            Some(0)
        );
        // used ones are skipped
        assert_eq!(
            match_window(&saved, &[true, false, false], "brave.desktop", "New Tab"),
            Some(2)
        );
        // other app or all used: none
        assert_eq!(match_window(&saved, &none, "x.desktop", "Downloads"), None);
        assert_eq!(
            match_window(&saved, &[true, true, true], "brave.desktop", "x"),
            None
        );
    }
}
