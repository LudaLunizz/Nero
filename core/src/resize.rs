//! Ctrl-linked resize: which windows move along with the edge being dragged.
//! Port of `isAdjacent` / `isCollinear` in the Tiling Shell patch's
//! `components/tilingsystem/resizeManager.js`.

use crate::{Rect, js_max, js_min};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

/// The inner gaps of the layout, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct Gaps {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// How far from a window's `side` edge a neighbour may sit and still be linked:
/// the gap on that side plus four times the right gap as error margin (a gap of
/// 0 counts as 2, so windows without gaps still link).
pub fn reach(gaps: &Gaps, side: Side) -> f64 {
    let g = |v: f64| if v == 0.0 { 2.0 } else { v };
    let own = match side {
        Side::Top => gaps.top,
        Side::Bottom => gaps.bottom,
        Side::Left => gaps.left,
        Side::Right => gaps.right,
    };
    g(own) + g(gaps.right) * 4.0
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

/// A window that moves along with the dragged one: its index in the list that was
/// given, and which of its edges moves (None = that direction stays).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Linked {
    pub i: usize,
    pub v: Option<Side>,
    pub h: Option<Side>,
}

/// The windows reached from `rect` over its `side` edge: the ones touching it,
/// the ones beside it on the same line (collinear), and, from each of those,
/// whatever follows on. `remaining` is what is still free to take.
fn find_adjacent(
    rect: &Rect,
    side: Side,
    remaining: &[usize],
    others: &[Rect],
    gaps: &Gaps,
) -> Vec<(usize, Side)> {
    let reach = reach(gaps, side);
    let opposite = side.opposite();
    let (mut result, mut touching, mut collinear, mut rest) = (vec![], vec![], vec![], vec![]);
    for &i in remaining {
        if is_adjacent(rect, &others[i], side, reach) {
            result.push((i, opposite));
            touching.push(i);
        } else if is_collinear(rect, &others[i], side, reach) {
            result.push((i, side));
            collinear.push(i);
        } else {
            rest.push(i);
        }
    }
    for (list, next) in [(touching, opposite), (collinear, side)] {
        for i in list {
            for (j, s) in find_adjacent(&others[i], next, &rest, others, gaps) {
                result.push((j, s));
                rest.retain(|&x| x != j);
            }
        }
    }
    result
}

/// Ctrl-resize: every window (of `others`) that moves along when the dragged
/// window's `drag_v` / `drag_h` edge moves, with the edge of it that moves.
pub fn find_linked(
    drag: &Rect,
    drag_v: Option<Side>,
    drag_h: Option<Side>,
    others: &[Rect],
    gaps: &Gaps,
) -> Vec<Linked> {
    let all: Vec<usize> = (0..others.len()).collect();
    let mut out: Vec<Linked> = vec![];
    if let Some(side) = drag_v {
        for (i, s) in find_adjacent(drag, side, &all, others, gaps) {
            out.push(Linked {
                i,
                v: Some(s),
                h: None,
            });
        }
    }
    if let Some(side) = drag_h {
        for (i, s) in find_adjacent(drag, side, &all, others, gaps) {
            match out.iter_mut().find(|l| l.i == i) {
                Some(l) => l.h = Some(s),
                None => out.push(Linked {
                    i,
                    v: None,
                    h: Some(s),
                }),
            }
        }
    }
    out
}

/// One linked window at settle time: where it was at grab start, where it is
/// now, which of its edges moves, and whether it is gone.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct SettleWin {
    pub start: Rect,
    pub actual: Rect,
    pub h: Option<Side>,
    pub v: Option<Side>,
    #[serde(default)]
    pub gone: bool,
}

/// A window to put back: `i` is the index in the settle list, -1 for the
/// dragged window itself.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Put {
    pub i: i64,
    pub rect: Rect,
}

fn differs(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() > 1.0
        || (a.y - b.y).abs() > 1.0
        || (a.width - b.width).abs() > 1.0
        || (a.height - b.height).abs() > 1.0
}

/// After the grab: the dragged window (and the windows whose edge moved along
/// with it) may not reach further than the windows on the other side of the
/// line could give way (see `clamp_edge`). Returns the windows to put back.
pub fn linked_settle(
    drag_start: &Rect,
    drag_actual: &Rect,
    drag_h: Option<Side>,
    drag_v: Option<Side>,
    entries: &[SettleWin],
    gaps: &Gaps,
) -> Vec<Put> {
    // (index, start, actual, h, v)
    let mut movers = vec![(-1i64, *drag_start, *drag_actual, drag_h, drag_v)];
    for (i, e) in entries.iter().enumerate() {
        if e.gone {
            continue;
        }
        let h = e.h.filter(|s| Some(*s) == drag_h);
        let v = e.v.filter(|s| Some(*s) == drag_v);
        if h.is_some() || v.is_some() {
            movers.push((i as i64, e.start, e.actual, h, v));
        }
    }
    let mut out = vec![];
    for (idx, start, actual, h, v) in movers {
        let mut r = actual;
        for side in [h, v].into_iter().flatten() {
            let opposite = side.opposite();
            let partners: Vec<(Rect, Rect)> = entries
                .iter()
                .enumerate()
                .filter(|(i, e)| {
                    !e.gone && *i as i64 != idx && (e.h == Some(opposite) || e.v == Some(opposite))
                })
                .map(|(_, e)| (e.start, e.actual))
                .collect();
            if !partners.is_empty() {
                r = clamp_edge(&start, &r, side, &partners, reach(gaps, side));
            }
        }
        if differs(&r, &actual) {
            out.push(Put { i: idx, rect: r });
        }
    }
    out
}
