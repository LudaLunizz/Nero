//! Line groups for the layout editor. Port of the Tiling Shell patch's
//! `components/layout/lineGroups.js` (2026-11-18).
//!
//! A "line" is one tile edge shared by tiles on both sides. The tiles' `groups`
//! say which lines move together: tiles sharing a group id belong to one
//! slider. `toggle_chain` lets the user join the pieces of one straight line
//! across columns (one slider moves them all) or split it into separate pieces.
//!
//! The JS version keeps references to the tile objects; here tiles are referred
//! to by their index in the `tiles` slice (the JS wrapper maps them back).

use crate::{js_max, js_min};
use serde::{Deserialize, Serialize};

const EPS: f64 = 0.002;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

/// A layout tile in normalised 0..1 coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub groups: Vec<f64>,
}

/// One connected piece of a line: tile indexes before and after it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub prev: Vec<usize>,
    pub next: Vec<usize>,
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub vertical: bool,
    pub c: f64,
    pub from: f64,
    pub to: f64,
    pub segments: Vec<Segment>,
}

/// Tiles on both sides of the line at coordinate c, pieces = connected parts.
/// vertical=true -> line x=c (tiles left/right of it); else line y=c.
fn segments_at(tiles: &[Tile], vertical: bool, c: f64) -> Vec<Segment> {
    let pos = |t: &Tile| if vertical { t.x } else { t.y };
    let len = |t: &Tile| if vertical { t.width } else { t.height };
    let apos = |t: &Tile| if vertical { t.y } else { t.x };
    let alen = |t: &Tile| if vertical { t.height } else { t.width };
    let prev: Vec<usize> = (0..tiles.len())
        .filter(|&i| near(pos(&tiles[i]) + len(&tiles[i]), c))
        .collect();
    let next: Vec<usize> = (0..tiles.len())
        .filter(|&i| near(pos(&tiles[i]), c))
        .collect();

    // union-find with path halving, like the JS Map version
    let mut parent: Vec<Option<usize>> = vec![None; tiles.len()];
    fn find(parent: &mut [Option<usize>], mut t: usize) -> usize {
        while parent[t] != Some(t) {
            let p = parent[t].expect("find() on a tile without a parent");
            parent[t] = parent[p];
            t = parent[t].expect("parent chain broken");
        }
        t
    }
    let mut touched: Vec<usize> = Vec::new(); // insertion order, like a JS Set
    for &p in &prev {
        for &n in &next {
            let (tp, tn) = (&tiles[p], &tiles[n]);
            let overlap =
                js_min(apos(tp) + alen(tp), apos(tn) + alen(tn)) - js_max(apos(tp), apos(tn));
            if overlap <= EPS {
                continue;
            }
            for t in [p, n] {
                if parent[t].is_none() {
                    parent[t] = Some(t);
                }
            }
            let rp = find(&mut parent, p);
            let rn = find(&mut parent, n);
            parent[rp] = Some(rn);
            for t in [p, n] {
                if !touched.contains(&t) {
                    touched.push(t);
                }
            }
        }
    }
    let mut comps: Vec<(usize, Vec<usize>)> = Vec::new(); // insertion order, like a JS Map
    for &t in &touched {
        let r = find(&mut parent, t);
        match comps.iter_mut().find(|(root, _)| *root == r) {
            Some((_, members)) => members.push(t),
            None => comps.push((r, vec![t])),
        }
    }
    comps
        .into_iter()
        .map(|(_, members)| Segment {
            prev: members
                .iter()
                .copied()
                .filter(|t| prev.contains(t))
                .collect(),
            next: members
                .iter()
                .copied()
                .filter(|t| next.contains(t))
                .collect(),
            from: members
                .iter()
                .map(|&t| apos(&tiles[t]))
                .fold(f64::INFINITY, js_min),
            to: members
                .iter()
                .map(|&t| apos(&tiles[t]) + alen(&tiles[t]))
                .fold(f64::NEG_INFINITY, js_max),
        })
        .collect()
}

/// Every internal straight line as chains of touching pieces.
pub fn find_chains(tiles: &[Tile]) -> Vec<Chain> {
    let mut out: Vec<Chain> = Vec::new();
    for vertical in [true, false] {
        let mut coords: Vec<f64> = Vec::new();
        for t in tiles {
            let c = if vertical {
                t.x + t.width
            } else {
                t.y + t.height
            };
            if c > EPS && c < 1.0 - EPS && !coords.iter().any(|&k| near(k, c)) {
                coords.push(c);
            }
        }
        for c in coords {
            let mut segs = segments_at(tiles, vertical, c);
            // stable, like Array.prototype.sort; NaN compares as equal
            segs.sort_by(|a, b| {
                a.from
                    .partial_cmp(&b.from)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut current: Option<usize> = None; // index into out
            for s in segs {
                match current {
                    Some(k) if s.from <= out[k].to + EPS => {
                        out[k].to = js_max(out[k].to, s.to);
                        out[k].segments.push(s);
                    }
                    _ => {
                        out.push(Chain {
                            vertical,
                            c,
                            from: s.from,
                            to: s.to,
                            segments: vec![s],
                        });
                        current = Some(out.len() - 1);
                    }
                }
            }
        }
    }
    out
}

/// Chain nearest to a pointer (normalised 0..1 coords) within max_px pixels.
pub fn pick_chain(
    tiles: &[Tile],
    nx: f64,
    ny: f64,
    width: f64,
    height: f64,
    max_px: f64,
) -> Option<Chain> {
    let mut best = None;
    let mut best_d = max_px;
    for ch in find_chains(tiles) {
        let gap = |v: f64| {
            if v < ch.from {
                ch.from - v
            } else if v > ch.to {
                v - ch.to
            } else {
                0.0
            }
        };
        let d = if ch.vertical {
            ((nx - ch.c).abs() * width).hypot(gap(ny) * height)
        } else {
            (gap(nx) * width).hypot((ny - ch.c).abs() * height)
        };
        if d <= best_d {
            best_d = d;
            best = Some(ch);
        }
    }
    best
}

/// Group ids that run along this segment: shared by a tile on each side.
fn segment_ids(tiles: &[Tile], seg: &Segment) -> Vec<f64> {
    let mut ids = Vec::new();
    for &p in &seg.prev {
        for &n in &seg.next {
            for &g in &tiles[p].groups {
                if tiles[n].groups.contains(&g) && !ids.contains(&g) {
                    ids.push(g);
                }
            }
        }
    }
    ids
}

fn chain_tiles(chain: &Chain) -> Vec<usize> {
    let mut set = Vec::new();
    for s in &chain.segments {
        for &t in s.prev.iter().chain(&s.next) {
            if !set.contains(&t) {
                set.push(t);
            }
        }
    }
    set
}

fn fresh_id(tiles: &[Tile]) -> f64 {
    tiles
        .iter()
        .flat_map(|t| t.groups.iter().copied())
        .fold(0.0, js_max)
        + 1.0
}

/// "attached" when one group id runs through every piece of a multi-piece chain.
pub fn is_attached(tiles: &[Tile], chain: &Chain) -> bool {
    if chain.segments.len() < 2 {
        return false;
    }
    let all: Vec<Vec<f64>> = chain
        .segments
        .iter()
        .map(|s| segment_ids(tiles, s))
        .collect();
    all[0].iter().any(|g| all.iter().all(|s| s.contains(g)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Toggle {
    Separated,
    Attached,
    Single,
}

/// Toggle the chain: attached -> one group per piece; else -> one group for
/// all. Changes the tiles' groups.
pub fn toggle_chain(tiles: &mut [Tile], chain: &Chain) -> Toggle {
    let members = chain_tiles(chain);
    let mut old: Vec<f64> = Vec::new();
    for s in &chain.segments {
        for g in segment_ids(tiles, s) {
            if !old.contains(&g) {
                old.push(g);
            }
        }
    }
    let strip = |tiles: &mut [Tile]| {
        for &t in &members {
            tiles[t].groups.retain(|g| !old.contains(g));
        }
    };
    if is_attached(tiles, chain) {
        strip(tiles);
        let mut id = fresh_id(tiles);
        for s in &chain.segments {
            for &t in s.prev.iter().chain(&s.next) {
                tiles[t].groups.push(id);
            }
            id += 1.0;
        }
        return Toggle::Separated;
    }
    if chain.segments.len() == 1 && old.len() == 1 {
        return Toggle::Single;
    }
    strip(tiles);
    let id = fresh_id(tiles);
    for &t in &members {
        tiles[t].groups.push(id);
    }
    Toggle::Attached
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GroupLine {
    pub vertical: bool,
    pub c: f64,
    /// index of a member whose edge lies on the line
    pub tile: usize,
}

/// For the editor: orientation + coordinate of one group's line when every tile
/// of the group touches the same line (works for chains of several pieces,
/// which the stock detection misreads). None -> use the stock detection.
pub fn group_line(group_tiles: &[Tile]) -> Option<GroupLine> {
    for vertical in [true, false] {
        for t in group_tiles {
            let c = if vertical {
                t.x + t.width
            } else {
                t.y + t.height
            };
            let prev: Vec<usize> = (0..group_tiles.len())
                .filter(|&i| {
                    let u = &group_tiles[i];
                    near(
                        if vertical {
                            u.x + u.width
                        } else {
                            u.y + u.height
                        },
                        c,
                    )
                })
                .collect();
            let next = group_tiles
                .iter()
                .filter(|u| near(if vertical { u.x } else { u.y }, c))
                .count();
            if !prev.is_empty() && next > 0 && prev.len() + next == group_tiles.len() {
                return Some(GroupLine {
                    vertical,
                    c,
                    tile: prev[0],
                });
            }
        }
    }
    None
}
