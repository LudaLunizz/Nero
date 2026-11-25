//! keep-onscreen: shrink and move a new window into its monitor's work area.

use crate::{Rect, js_max, js_min};

/// The frame shrunk to fit the work area, then moved fully inside it.
pub fn fit_to_work_area(frame: &Rect, work_area: &Rect) -> Rect {
    let width = js_min(frame.width, work_area.width);
    let height = js_min(frame.height, work_area.height);
    let x = js_min(
        js_max(frame.x, work_area.x),
        work_area.x + work_area.width - width,
    );
    let y = js_min(
        js_max(frame.y, work_area.y),
        work_area.y + work_area.height - height,
    );
    Rect::new(x, y, width, height)
}
