//! Free space between windows, fill free space and the fluid drag shape. Port of the Tiling Shell patch's `utils/freeSpace.js` (2026-10-27/28),
//! function for function, same names in snake_case.

use crate::{Point, Rect, js_max, js_min, overlap_size};

const MIN_KEEP: f64 = 0.33;

/// A tile is taken when a window covers its center or most of its area, so a
/// neighbour that grew a bit into it does not make it look occupied.
pub fn is_tile_vacant(tile: &Rect, occupied: &[Rect]) -> bool {
    let c = tile.center();
    !occupied.iter().any(|o| {
        if o.contains(c) {
            return true;
        }
        matches!(overlap_size(tile, o), Some((w, h)) if w * h > (tile.width * tile.height) / 2.0)
    })
}

/// tile: the layout tile in pixels; occupied: frame rects of the other tiled
/// windows on this monitor; vacant_tiles: the other empty tiles, which the
/// window must not grow into.
pub fn fit_to_free_space(
    tile: &Rect,
    occupied: &[Rect],
    _work_area: &Rect,
    vacant_tiles: &[Rect],
) -> Rect {
    // A window covering most of the tile is the one being replaced (or sits
    // underneath): it is not an edge to snap against.
    let half = (tile.width * tile.height) / 2.0;
    let (occupied, covering): (Vec<Rect>, Vec<Rect>) = occupied
        .iter()
        .partition(|o| !matches!(overlap_size(tile, o), Some((w, h)) if w * h > half));
    let (mut left, mut top) = (tile.x, tile.y);
    let (mut right, mut bottom) = (tile.right(), tile.bottom());

    // Shrink away from windows that grew into the tile.
    for o in &occupied {
        let Some((ow, oh)) = overlap_size(&Rect::from_edges(left, top, right, bottom), o) else {
            continue;
        };
        let oc = o.center();
        let (rcx, rcy) = ((left + right) / 2.0, (top + bottom) / 2.0);
        if oh <= ow {
            if oc.y < rcy {
                top = js_max(top, o.bottom())
            } else {
                bottom = js_min(bottom, o.y)
            }
        } else if oc.x < rcx {
            left = js_max(left, o.right())
        } else {
            right = js_min(right, o.x)
        }
    }
    if right - left < tile.width * MIN_KEEP || bottom - top < tile.height * MIN_KEEP {
        return *tile;
    }

    // Grow into gaps left by windows that shrank, up to the next window, but
    // never across another empty tile and never past the tile to a bare edge.
    let blocked = |s: Rect| {
        s.width > 0.0
            && s.height > 0.0
            && (vacant_tiles.iter().any(|v| s.contains(v.center()))
                || covering.iter().any(|o| overlap_size(&s, o).is_some()))
    };
    let spans_x = |o: &Rect, left: f64, right: f64| o.x < right && o.right() > left;
    let spans_y = |o: &Rect, top: f64, bottom: f64| o.y < bottom && o.bottom() > top;

    let edge = occupied
        .iter()
        .filter(|o| spans_x(o, left, right) && o.bottom() <= top)
        .fold(None, |e: Option<f64>, o| {
            Some(js_max(e.unwrap_or(f64::NEG_INFINITY), o.bottom()))
        });
    if let Some(e) = edge
        && !blocked(Rect::new(left, e, right - left, top - e))
    {
        top = e;
    }
    let edge = occupied
        .iter()
        .filter(|o| spans_x(o, left, right) && o.y >= bottom)
        .fold(None, |e: Option<f64>, o| {
            Some(js_min(e.unwrap_or(f64::INFINITY), o.y))
        });
    if let Some(e) = edge
        && !blocked(Rect::new(left, bottom, right - left, e - bottom))
    {
        bottom = e;
    }
    let edge = occupied
        .iter()
        .filter(|o| spans_y(o, top, bottom) && o.right() <= left)
        .fold(None, |e: Option<f64>, o| {
            Some(js_max(e.unwrap_or(f64::NEG_INFINITY), o.right()))
        });
    if let Some(e) = edge
        && !blocked(Rect::new(e, top, left - e, bottom - top))
    {
        left = e;
    }
    let edge = occupied
        .iter()
        .filter(|o| spans_y(o, top, bottom) && o.x >= right)
        .fold(None, |e: Option<f64>, o| {
            Some(js_min(e.unwrap_or(f64::INFINITY), o.x))
        });
    if let Some(e) = edge
        && !blocked(Rect::new(right, top, e - right, bottom - top))
    {
        right = e;
    }
    Rect::from_edges(left, top, right, bottom)
}

/// Indexes of the rects that share an edge with rect (within tol pixels).
pub fn touching_rects(rect: &Rect, rects: &[Rect], tol: f64) -> Vec<usize> {
    let (rl, rt, rr, rb) = (rect.x, rect.y, rect.right(), rect.bottom());
    rects
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            let (l, t, r, b) = (o.x, o.y, o.right(), o.bottom());
            let span_x = l < rr && r > rl;
            let span_y = t < rb && b > rt;
            (span_x && ((b - rt).abs() <= tol || (t - rb).abs() <= tol))
                || (span_y && ((r - rl).abs() <= tol || (l - rr).abs() <= tol))
        })
        .map(|(i, _)| i)
        .collect()
}

/// Each window belongs to the tile it matches best (intersection over union);
/// tiles that belong to no window are free, even when a grown window covers
/// them. Returns the indexes of the free tiles.
pub fn free_tiles(tiles: &[Rect], rects: &[Rect]) -> Vec<usize> {
    let mut taken = vec![false; tiles.len()];
    for r in rects {
        let (mut best, mut best_score) = (None, 0.0);
        for (i, t) in tiles.iter().enumerate() {
            let Some((w, h)) = overlap_size(t, r) else {
                continue;
            };
            let inter = w * h;
            let score = inter / (t.width * t.height + r.width * r.height - inter);
            if score > best_score {
                best_score = score;
                best = Some(i);
            }
        }
        if let Some(b) = best {
            taken[b] = true;
        }
    }
    (0..tiles.len()).filter(|&i| !taken[i]).collect()
}

/// Window menu "Fill free space": the window grows left/right, then up/down,
/// until it meets another window or the screen edge. Windows it already
/// overlaps (layers underneath) do not stop it. tol: touching edges count as
/// touching, not overlapping.
pub fn grow_into_free_space(rect: &Rect, others: &[Rect], area: &Rect, tol: f64) -> Rect {
    let (al, at, ar, ab) = (area.x, area.y, area.right(), area.bottom());
    let mut l = js_max(al, rect.x);
    let mut t = js_max(at, rect.y);
    let mut r = js_min(ar, rect.right());
    let mut b = js_min(ab, rect.bottom());
    let blockers: Vec<&Rect> = others
        .iter()
        .filter(|o| match overlap_size(o, rect) {
            None => true,
            Some((w, h)) => w <= tol || h <= tol,
        })
        .collect();
    let side: Vec<&Rect> = blockers
        .iter()
        .copied()
        .filter(|o| o.y < b - tol && o.bottom() > t + tol)
        .collect();
    let (l0, r0) = (l, r);
    l = side
        .iter()
        .filter(|o| o.right() <= l0 + tol)
        .map(|o| o.right())
        .fold(al, js_max);
    r = side
        .iter()
        .filter(|o| o.x >= r0 - tol)
        .map(|o| o.x)
        .fold(ar, js_min);
    let ends: Vec<&Rect> = blockers
        .iter()
        .copied()
        .filter(|o| o.x < r - tol && o.right() > l + tol)
        .collect();
    let (t0, b0) = (t, b);
    t = ends
        .iter()
        .filter(|o| o.bottom() <= t0 + tol)
        .map(|o| o.bottom())
        .fold(at, js_max);
    b = ends
        .iter()
        .filter(|o| o.y >= b0 - tol)
        .map(|o| o.y)
        .fold(ab, js_min);
    Rect::from_edges(l, t, r, b)
}

// --- Fluid drag shape (2026-10-28) -------------------------------------------
// While dragging, the preview takes the shape of the open space under the
// pointer instead of the fixed tile. The layout stays the reference: when the
// open space holds two or more tiles, the shape is the tile under the pointer,
// stretched out to the edge of the open space on the sides where no other tile
// of that space lies.

/// Largest empty rectangle around point p: grow sideways then up/down, and
/// up/down then sideways; keep the bigger one.
pub fn open_space_at(p: Point, obstacles: &[Rect], area: &Rect) -> Rect {
    let (al, at, ar, ab) = (area.x, area.y, area.right(), area.bottom());
    struct Box {
        l: f64,
        t: f64,
        r: f64,
        b: f64,
    }
    let sideways = |bx: &mut Box| {
        for o in obstacles {
            if !(o.y < bx.b && o.bottom() > bx.t) {
                continue;
            }
            if o.right() <= p.x {
                bx.l = js_max(bx.l, o.right());
            }
            // an edge exactly at the pointer bounds the space too
            if o.x >= p.x {
                bx.r = js_min(bx.r, o.x);
            }
        }
    };
    let up_down = |bx: &mut Box| {
        for o in obstacles {
            if !(o.x < bx.r && o.right() > bx.l) {
                continue;
            }
            if o.bottom() <= p.y {
                bx.t = js_max(bx.t, o.bottom());
            }
            if o.y >= p.y {
                bx.b = js_min(bx.b, o.y);
            }
        }
    };
    // the first pass only looks at the pointer's row (or column)
    let grow = |horizontal_first: bool| {
        let mut bx = Box {
            l: al,
            t: at,
            r: ar,
            b: ab,
        };
        if horizontal_first {
            (bx.t, bx.b) = (p.y, p.y);
            sideways(&mut bx);
            (bx.t, bx.b) = (at, ab);
            up_down(&mut bx);
        } else {
            (bx.l, bx.r) = (p.x, p.x);
            up_down(&mut bx);
            (bx.l, bx.r) = (al, ar);
            sideways(&mut bx);
        }
        Rect::from_edges(bx.l, bx.t, bx.r, bx.b)
    };
    let a = grow(true);
    let c = grow(false);
    if a.width * a.height >= c.width * c.height {
        a
    } else {
        c
    }
}

fn intersect(a: &Rect, b: &Rect) -> Option<Rect> {
    let (l, t) = (js_max(a.x, b.x), js_max(a.y, b.y));
    let (r, bt) = (js_min(a.right(), b.right()), js_min(a.bottom(), b.bottom()));
    (r > l && bt > t).then(|| Rect::from_edges(l, t, r, bt))
}

/// tiles: layout tiles in pixels; obstacles: the other visible windows.
/// Returns None when the pointer is over a window or the open space is too
/// small to be useful: then the plain tile under the pointer is used.
pub fn fluid_shape(
    tiles: &[Rect],
    obstacles: &[Rect],
    area: &Rect,
    p: Point,
    min_size: f64,
) -> Option<Rect> {
    if obstacles.iter().any(|o| o.contains(p)) {
        return None;
    }
    let free = open_space_at(p, obstacles, area);
    // free tiles that reach into the open space; a tile mostly covered by a
    // window is not free
    let inside: Vec<Rect> = tiles
        .iter()
        .copied()
        .filter(|t| {
            matches!(intersect(t, &free), Some(i) if i.width > 2.0 && i.height > 2.0)
                && is_tile_vacant(t, obstacles)
        })
        .collect();
    if inside.len() <= 1 {
        let under = tiles.iter().find(|t| t.contains(p));
        if let (true, Some(u)) = (inside.is_empty(), under)
            && (free.width < u.width * min_size || free.height < u.height * min_size)
        {
            return None;
        }
        return Some(free);
    }
    let home = inside.iter().position(|t| t.contains(p)).or_else(|| {
        let mut best = f64::INFINITY;
        let mut home = None;
        for (k, t) in inside.iter().enumerate() {
            let c = t.center();
            let d = (c.x - p.x).powi(2) + (c.y - p.y).powi(2);
            if d < best {
                best = d;
                home = Some(k);
            }
        }
        home
    })?;
    let s = intersect(&inside[home], &free)?;
    let (mut l, mut t, mut r, mut b) = (s.x, s.y, s.right(), s.bottom());
    // by position, not by value: two equal tiles are still two tiles
    let others: Vec<&Rect> = inside
        .iter()
        .enumerate()
        .filter(|(k, _)| *k != home)
        .map(|(_, o)| o)
        .collect();
    let tol = 2.0;
    let rows = |o: &Rect, t: f64, b: f64| o.y < b - tol && o.bottom() > t + tol;
    let cols = |o: &Rect, l: f64, r: f64| o.x < r - tol && o.right() > l + tol;
    if !others.iter().any(|o| rows(o, t, b) && o.right() <= l + tol) {
        l = free.x;
    }
    if !others.iter().any(|o| rows(o, t, b) && o.x >= r - tol) {
        r = free.right();
    }
    if !others
        .iter()
        .any(|o| cols(o, l, r) && o.bottom() <= t + tol)
    {
        t = free.y;
    }
    if !others.iter().any(|o| cols(o, l, r) && o.y >= b - tol) {
        b = free.bottom();
    }
    // Follow the windows already placed (2026-11-28): the windows that border
    // the open space above or below set its columns, so a full-width window
    // gets a full-width neighbour instead of the layout's columns. Rows stay
    // from the layout. Portrait screens never split into columns.
    if area.height > area.width {
        (l, r) = (free.x, free.right());
    } else if let Some((nl, nr)) = follow_columns(&free, obstacles, p.x, tol) {
        (l, r) = (nl, nr);
    }
    Some(Rect::from_edges(l, t, r, b))
}

/// The open space's span around x, cut at the side edges of the windows that
/// touch its top or bottom. None when no window touches it there: then the
/// layout tiles decide the columns.
fn follow_columns(free: &Rect, obstacles: &[Rect], x: f64, tol: f64) -> Option<(f64, f64)> {
    let (lo, hi) = (free.x, free.right());
    let (mut l, mut r) = (lo, hi);
    let mut any = false;
    for o in obstacles {
        let touches = (o.bottom() - free.y).abs() <= tol || (o.y - free.bottom()).abs() <= tol;
        if !touches || o.x >= hi - tol || o.right() <= lo + tol {
            continue;
        }
        any = true;
        for e in [o.x, o.right()] {
            if e > lo + tol && e < hi - tol {
                if e <= x {
                    l = js_max(l, e);
                } else {
                    r = js_min(r, e);
                }
            }
        }
    }
    any.then_some((l, r))
}
