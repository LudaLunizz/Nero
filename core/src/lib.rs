//! Nero's window placement logic, ported from the JavaScript of the Tiling Shell
//! patch and keep-onscreen. Everything here works on plain rectangles, so it is
//! the same code in GNOME Shell (as WebAssembly, see `abi`) and in `cargo test`.
//!
//! Numbers are `f64` on purpose: the JavaScript it replaces works on JS numbers,
//! and the results must match it to the last bit (see `tests/` and the JS tests).

use serde::{Deserialize, Serialize};

pub mod abi;
pub mod fit;
pub mod free_space;
pub mod keep_model;
pub mod layout;
pub mod layout_model;
pub mod line_groups;
pub mod resize;
pub mod sysmon;
pub mod taskbar;
pub mod touchpad;
pub mod workspaces;
pub mod ws_model;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }
    /// Half-open: the left/top edge is inside, the right/bottom edge is not.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }
    fn from_edges(l: f64, t: f64, r: f64, b: f64) -> Self {
        Rect {
            x: l,
            y: t,
            width: r - l,
            height: b - t,
        }
    }
}

/// Overlap as (width, height), or None when the rects do not overlap.
pub fn overlap_size(a: &Rect, b: &Rect) -> Option<(f64, f64)> {
    let w = a.right().min(b.right()) - a.x.max(b.x);
    let h = a.bottom().min(b.bottom()) - a.y.max(b.y);
    (w > 0.0 && h > 0.0).then_some((w, h))
}

/// JavaScript's `Math.min` / `Math.max` over a list: NaN wins, like in JS.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// JavaScript's `Math.round`: halves round up (towards +infinity).
pub(crate) fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}
