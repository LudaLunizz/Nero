//! Ctrl-linked resize: which windows move along with the edge being dragged.
//! Port of `isAdjacent` / `isCollinear` in the Tiling Shell patch's
//! `components/tilingsystem/resizeManager.js`.

use crate::{Rect, js_max, js_min};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Does o touch w's `side` edge? A gap up to `reach` px counts, and so does a
/// small overlap (up to a quarter of the smaller window): windows that cannot
/// shrink to their frame stick out a bit. A corner touch is not a shared edge.
pub fn is_adjacent(w: &Rect, o: &Rect, side: Side, reach: f64) -> bool {
    let horiz = matches!(side, Side::Left | Side::Right);
    let (p, len, op, olen) = if horiz {
        (w.x, w.width, o.x, o.width)
    } else {
        (w.y, w.height, o.y, o.height)
    };
    let shared = if horiz {
        js_min(w.bottom(), o.bottom()) - js_max(w.y, o.y)
    } else {
        js_min(w.right(), o.right()) - js_max(w.x, o.x)
    };
    if shared < 8.0 {
        return false;
    }
    let max_overlap = js_min(len, olen) / 4.0;
    if matches!(side, Side::Right | Side::Bottom) {
        let edge = p + len;
        return op <= edge + reach && op >= edge - max_overlap && op + olen > edge;
    }
    let o_end = op + olen;
    o_end >= p - reach && o_end <= p + max_overlap && op < p
}

/// Is o beside w with its `side` edge on the same line (within tol px)? Then a
/// Ctrl-resize of that edge moves o's edge too: the whole line moves.
pub fn is_collinear(w: &Rect, o: &Rect, side: Side, reach: f64) -> bool {
    let tol = js_max(reach, 16.0);
    let diff = match side {
        Side::Top => o.y - w.y,
        Side::Left => o.x - w.x,
        Side::Bottom => o.bottom() - w.bottom(),
        Side::Right => o.right() - w.right(),
    };
    if diff.abs() > tol {
        return false;
    }
    let (a, b) = if matches!(side, Side::Top | Side::Bottom) {
        (Side::Left, Side::Right)
    } else {
        (Side::Top, Side::Bottom)
    };
    is_adjacent(w, o, a, reach) || is_adjacent(w, o, b, reach)
}

/// The smallest size a linked window has shown it can take (0 = not seen yet).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, Deserialize)]
pub struct MinSize {
    pub width: f64,
    pub height: f64,
}

/// Where a linked window goes while the dragged window's edge moves.
///
/// `start` is the linked window at grab start, `delta` the dragged window's
/// change since then (x, y, width, height). `h` / `v` say which of the linked
/// window's edges moves (None = that direction stays), `drag_h` / `drag_v`
/// which edge of the dragged window the user holds.
///
/// Only the moving edge moves. The far edge stays where it was, also when the
/// window cannot get smaller than `min`: then the window stops at `min` against
/// its far edge (it lays over the dragged window a bit) instead of pushing that
/// far edge into the windows beyond it.
pub fn linked_rect(
    start: &Rect,
    delta: &Rect,
    h: Option<Side>,
    v: Option<Side>,
    drag_h: Option<Side>,
    drag_v: Option<Side>,
    min: MinSize,
) -> Rect {
    // how far the held edge moved
    let mh = match drag_h {
        Some(Side::Left) => delta.x,
        Some(Side::Right) => delta.width,
        _ => 0.0,
    };
    let mv = match drag_v {
        Some(Side::Top) => delta.y,
        Some(Side::Bottom) => delta.height,
        _ => 0.0,
    };
    let mut r = *start;
    match h {
        Some(Side::Left) => {
            r.width = js_max(js_max(start.width - mh, min.width), 0.0);
            r.x = start.right() - r.width;
        }
        Some(Side::Right) => r.width = js_max(js_max(start.width + mh, min.width), 0.0),
        _ => {}
    }
    match v {
        Some(Side::Top) => {
            r.height = js_max(js_max(start.height - mv, min.height), 0.0);
            r.y = start.bottom() - r.height;
        }
        Some(Side::Bottom) => r.height = js_max(js_max(start.height + mv, min.height), 0.0),
        _ => {}
    }
    r
}

/// The linked window came out at `actual`. If its far edge moved away from
/// where it was at grab start, it refused to get smaller: remember that size
/// as its minimum, so the next `linked_rect` keeps the far edge in place.
pub fn linked_min(
    start: &Rect,
    actual: &Rect,
    h: Option<Side>,
    v: Option<Side>,
    min: MinSize,
) -> MinSize {
    let mut m = min;
    let off = |a: f64, b: f64| (a - b).abs() > 1.0;
    let pushed_h = match h {
        Some(Side::Left) => off(actual.right(), start.right()),
        Some(Side::Right) => off(actual.x, start.x),
        _ => false,
    };
    if pushed_h && actual.width > m.width {
        m.width = actual.width;
    }
    let pushed_v = match v {
        Some(Side::Top) => off(actual.bottom(), start.bottom()),
        Some(Side::Bottom) => off(actual.y, start.y),
        _ => false,
    };
    if pushed_v && actual.height > m.height {
        m.height = actual.height;
    }
    m
}

/// After a Ctrl-resize: a window whose `side` edge moved (`start` → `now`) may
/// not end up further into its neighbours than it was at grab start. Each
/// partner is (rect at grab start, rect now); only partners that touched that
/// edge at grab start count. A partner that stopped at its minimum size then
/// pushes the edge back, so the two touching windows still share one line.
pub fn clamp_edge(
    start: &Rect,
    now: &Rect,
    side: Side,
    partners: &[(Rect, Rect)],
    reach: f64,
) -> Rect {
    let mut r = *now;
    for (p_start, p_now) in partners {
        if !is_adjacent(start, p_start, side, reach) {
            continue;
        }
        match side {
            Side::Right => {
                let limit = p_now.x + js_max(start.right() - p_start.x, 0.0);
                if r.right() > limit && limit > r.x {
                    r.width = limit - r.x;
                }
            }
            Side::Left => {
                let limit = p_now.right() - js_max(p_start.right() - start.x, 0.0);
                if r.x < limit && limit < r.right() {
                    r.width = r.right() - limit;
                    r.x = limit;
                }
            }
            Side::Bottom => {
                let limit = p_now.y + js_max(start.bottom() - p_start.y, 0.0);
                if r.bottom() > limit && limit > r.y {
                    r.height = limit - r.y;
                }
            }
            Side::Top => {
                let limit = p_now.bottom() - js_max(p_start.bottom() - start.y, 0.0);
                if r.y < limit && limit < r.bottom() {
                    r.height = r.bottom() - limit;
                    r.y = limit;
                }
            }
        }
    }
    r
}
