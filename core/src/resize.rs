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
