//! The boundary to JavaScript (GNOME Shell loads this crate as WebAssembly).
//!
//! One call = one JSON request `{"f": "fitToFreeSpace", "a": [arg, ...]}`, with
//! the JS function name and its arguments in order. The answer is JSON too:
//! `{"ok": result}` or `{"err": "message"}`. `core/js/nero.js` does the JS side.
//!
//! Memory: JS asks `nero_alloc(len)` for room, writes the request there, calls
//! `nero_call(ptr, len)` (which frees the request) and reads `nero_out_len()`
//! bytes at the returned pointer. The answer stays valid until the next call.

use crate::{
    Point, Rect, fit, free_space as fs, layout, line_groups as lg, resize, touchpad as tp,
    ws_model as wm,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::cell::RefCell;

/// The answer as JSON text. Typed results are written straight to text, so
/// object keys keep their field order (x, y, width, height), like the JS did.
type Res = Result<String, String>;

fn arg<T: DeserializeOwned>(a: &[Value], i: usize, name: &str) -> Result<T, String> {
    let v = a.get(i).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| format!("argument {i} ({name}): {e}"))
}

/// An optional argument: missing or null gives the default, like a JS default
/// parameter.
fn opt<T: DeserializeOwned>(a: &[Value], i: usize, name: &str, default: T) -> Result<T, String> {
    match a.get(i) {
        None | Some(Value::Null) => Ok(default),
        Some(_) => arg(a, i, name),
    }
}

fn ok<T: serde::Serialize>(v: T) -> Res {
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

fn dispatch(f: &str, a: &[Value]) -> Res {
    match f {
        // The version of this build; nero.js compares it with the one core/build.sh
        // stamped into it, so a wasm and a loader from different builds are caught.
        "version" => ok(env!("CARGO_PKG_VERSION")),
        "isTileVacant" => ok(fs::is_tile_vacant(
            &arg(a, 0, "tile")?,
            &arg::<Vec<Rect>>(a, 1, "occupied")?,
        )),
        "fitToFreeSpace" => ok(fs::fit_to_free_space(
            &arg(a, 0, "tile")?,
            &arg::<Vec<Rect>>(a, 1, "occupied")?,
            &opt(a, 2, "workArea", Rect::new(0.0, 0.0, 0.0, 0.0))?,
            &opt::<Vec<Rect>>(a, 3, "vacantTiles", vec![])?,
        )),
        "touchingRects" => ok(fs::touching_rects(
            &arg(a, 0, "rect")?,
            &arg::<Vec<Rect>>(a, 1, "rects")?,
            opt(a, 2, "tol", 10.0)?,
        )),
        "cutAway" => ok(fs::cut_away(&arg(a, 0, "rect")?, &arg(a, 1, "keep")?)),
        "resolveOverlaps" => ok(fs::resolve_overlaps(
            &arg::<Vec<Rect>>(a, 0, "rects")?,
            &arg::<Vec<Option<f64>>>(a, 1, "priority")?
                .into_iter()
                .map(|p| p.unwrap_or(f64::NAN))
                .collect::<Vec<_>>(),
            &opt::<Vec<Rect>>(a, 2, "tiles", vec![])?,
        )),
        "fillGaps" => ok(fs::fill_gaps(
            &arg::<Vec<Option<Rect>>>(a, 0, "rects")?,
            &arg(a, 1, "workArea")?,
        )),
        "freeTiles" => ok(fs::free_tiles(
            &arg::<Vec<Rect>>(a, 0, "tiles")?,
            &arg::<Vec<Rect>>(a, 1, "rects")?,
        )),
        "framesOf" => ok(fs::frames_of(
            &arg::<Vec<Rect>>(a, 0, "tiles")?,
            &arg(a, 1, "rect")?,
        )),
        "placeInFrames" => ok(fs::place_in_frames(
            &arg::<Vec<Rect>>(a, 0, "tiles")?,
            &arg::<Vec<Rect>>(a, 1, "others")?,
            &arg(a, 2, "target")?,
        )),
        "refillFrames" => ok(fs::refill_frames(
            &arg::<Vec<Rect>>(a, 0, "tiles")?,
            &arg::<Vec<Rect>>(a, 1, "wins")?,
        )),
        "growIntoFreeSpace" => ok(fs::grow_into_free_space(
            &arg(a, 0, "rect")?,
            &arg::<Vec<Rect>>(a, 1, "others")?,
            &arg(a, 2, "area")?,
            opt(a, 3, "tol", 10.0)?,
        )),
        "openSpaceAt" => ok(fs::open_space_at(
            arg::<Point>(a, 0, "p")?,
            &arg::<Vec<Rect>>(a, 1, "obstacles")?,
            &arg(a, 2, "area")?,
        )),
        "fluidShape" => ok(fs::fluid_shape(
            &arg::<Vec<Rect>>(a, 0, "tiles")?,
            &arg::<Vec<Rect>>(a, 1, "obstacles")?,
            &arg(a, 2, "area")?,
            arg::<Point>(a, 3, "p")?,
            opt(a, 4, "minSize", 0.33)?,
        )),
        "findChains" => ok(lg::find_chains(&arg::<Vec<lg::Tile>>(a, 0, "tiles")?)),
        "pickChain" => ok(lg::pick_chain(
            &arg::<Vec<lg::Tile>>(a, 0, "tiles")?,
            arg(a, 1, "nx")?,
            arg(a, 2, "ny")?,
            arg(a, 3, "width")?,
            arg(a, 4, "height")?,
            arg(a, 5, "maxPx")?,
        )),
        "isAttached" => ok(lg::is_attached(
            &arg::<Vec<lg::Tile>>(a, 0, "tiles")?,
            &arg(a, 1, "chain")?,
        )),
        "toggleChain" => {
            // returns {result, groups}: the new groups of every tile
            let mut tiles: Vec<lg::Tile> = arg(a, 0, "tiles")?;
            let chain: lg::Chain = arg(a, 1, "chain")?;
            check_chain(&chain, tiles.len())?;
            let result = lg::toggle_chain(&mut tiles, &chain);
            #[derive(serde::Serialize)]
            struct Toggled {
                result: lg::Toggle,
                groups: Vec<Vec<f64>>,
            }
            ok(Toggled {
                result,
                groups: tiles.into_iter().map(|t| t.groups).collect(),
            })
        }
        "groupLine" => ok(lg::group_line(&arg::<Vec<lg::Tile>>(a, 0, "groupTiles")?)),
        "isAdjacent" => ok(resize::is_adjacent(
            &arg(a, 0, "w")?,
            &arg(a, 1, "o")?,
            arg(a, 2, "side")?,
            arg(a, 3, "reach")?,
        )),
        "isCollinear" => ok(resize::is_collinear(
            &arg(a, 0, "w")?,
            &arg(a, 1, "o")?,
            arg(a, 2, "side")?,
            arg(a, 3, "reach")?,
        )),
        "linkedRect" => ok(resize::linked_rect(
            &arg(a, 0, "start")?,
            &arg(a, 1, "delta")?,
            opt(a, 2, "h", None)?,
            opt(a, 3, "v", None)?,
            opt(a, 4, "dragH", None)?,
            opt(a, 5, "dragV", None)?,
            opt(a, 6, "min", resize::MinSize::default())?,
        )),
        "linkedMin" => ok(resize::linked_min(
            &arg(a, 0, "start")?,
            &arg(a, 1, "actual")?,
            opt(a, 2, "h", None)?,
            opt(a, 3, "v", None)?,
            opt(a, 4, "min", resize::MinSize::default())?,
        )),
        "clampEdge" => ok(resize::clamp_edge(
            &arg(a, 0, "start")?,
            &arg(a, 1, "now")?,
            arg(a, 2, "side")?,
            &arg::<Vec<(Rect, Rect)>>(a, 3, "partners")?,
            arg(a, 4, "reach")?,
        )),
        "fitToWorkArea" => ok(fit::fit_to_work_area(
            &arg(a, 0, "frame")?,
            &arg(a, 1, "workArea")?,
        )),
        "toSpot" => ok(layout::to_spot(
            &arg(a, 0, "frame")?,
            &arg(a, 1, "workArea")?,
        )),
        "fromSpot" => ok(layout::from_spot(
            &arg(a, 0, "spot")?,
            &arg(a, 1, "workArea")?,
        )),
        "matchWindow" => ok(layout::match_window(
            &arg::<Vec<layout::Saved>>(a, 0, "saved")?,
            &arg::<Vec<bool>>(a, 1, "used")?,
            &arg::<String>(a, 2, "app")?,
            &arg::<String>(a, 3, "title")?,
        )),
        "wsInit" => ok(wm::start(
            opt::<Option<String>>(a, 0, "saved", None)?.as_deref(),
            arg(a, 1, "config")?,
        )),
        "wsStateText" => ok(wm::state_text(&arg(a, 0, "model")?)),
        "wsSavedGnome" => ok(wm::saved_gnome(&arg(a, 0, "settings")?)),
        "wsGiveBack" => ok(wm::give_back(&arg::<String>(a, 0, "text")?)),
        "wsKeys" => ok(wm::keys()),
        "wsConfig" => ok(wm::set_config(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "config")?,
        )),
        "wsTrack" => ok(wm::track(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "id")?,
            arg(a, 3, "fresh")?,
        )),
        "wsUntrack" => ok(wm::untrack(arg(a, 0, "model")?, arg(a, 1, "id")?)),
        "wsPlace" => ok(wm::place(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "id")?,
        )),
        "wsWindowMoved" => ok(wm::window_moved(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "id")?,
        )),
        "wsEnteredMonitor" => ok(wm::entered_monitor(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "id")?,
        )),
        "wsReconcile" => ok(wm::reconcile(arg(a, 0, "model")?, &arg(a, 1, "facts")?)),
        "wsReadopt" => ok(wm::readopt_all(arg(a, 0, "model")?, &arg(a, 1, "facts")?)),
        "wsDisable" => ok(wm::disable(&arg(a, 0, "model")?, &arg(a, 1, "facts")?)),
        "wsSwitch" => ok(wm::switch(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "monitor")?,
            arg(a, 3, "target")?,
            opt(a, 4, "focusWindow", None)?,
        )),
        "wsStepMonitor" => ok(wm::step_monitor(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "monitor")?,
            arg(a, 3, "delta")?,
        )),
        "wsKey" => ok(wm::key(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            &arg::<String>(a, 2, "binding")?,
            arg(a, 3, "monitor")?,
            opt(a, 4, "window", None)?,
        )),
        "wsActiveChanged" => ok(wm::active_changed(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "active")?,
        )),
        "wsPanelScroll" => ok(wm::panel_scroll(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "monitor")?,
            &arg(a, 3, "event")?,
        )),
        "wsForgetPanels" => ok(wm::forget_panels(arg(a, 0, "model")?)),
        "wsIgnoreScroll" => ok(wm::ignore_scroll(
            &arg::<Vec<wm::ActorInfo>>(a, 0, "chain")?,
            &arg::<String>(a, 1, "iconAction")?,
        )),
        "wsSwipeBegin" => ok(wm::swipe_begin(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "monitor")?,
        )),
        "wsSwipeEnd" => ok(wm::swipe_end(
            arg(a, 0, "model")?,
            &arg(a, 1, "facts")?,
            arg(a, 2, "endProgress")?,
        )),
        "scrollPlugin" => ok(tp::scroll_plugin(arg(a, 0, "percent")?)),
        "restackOrder" => ok(layout::restack_order(
            &arg::<Vec<layout::Placed>>(a, 0, "placed")?,
            arg(a, 1, "newIndex")?,
        )),
        _ => Err(format!("unknown function {f:?}")),
    }
}

/// A chain from JS refers to tiles by index: they must exist.
fn check_chain(chain: &lg::Chain, n: usize) -> Result<(), String> {
    let bad = chain
        .segments
        .iter()
        .flat_map(|s| s.prev.iter().chain(&s.next))
        .any(|&t| t >= n);
    if bad {
        Err("chain refers to a tile that is not in tiles".into())
    } else {
        Ok(())
    }
}

/// One request in, one answer out (both JSON). Safe to call from Rust too.
pub fn call(request: &[u8]) -> String {
    let res = (|| -> Res {
        let req: Value =
            serde_json::from_slice(request).map_err(|e| format!("bad request: {e}"))?;
        let f = req
            .get("f")
            .and_then(Value::as_str)
            .ok_or("request has no \"f\"")?;
        let a = req
            .get("a")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        dispatch(f, a)
    })();
    match res {
        Ok(v) => format!("{{\"ok\":{v}}}"),
        Err(e) => json!({ "err": e }).to_string(),
    }
}

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn nero_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` must come from `nero_alloc(len)` and hold `len` written bytes; it is
/// freed here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nero_call(ptr: *mut u8, len: usize) -> *const u8 {
    // SAFETY: the caller hands back the buffer from nero_alloc(len), now filled.
    let request = unsafe { Vec::from_raw_parts(ptr, len, len.max(1)) };
    let answer = call(&request);
    drop(request);
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = answer.into_bytes();
        o.as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn nero_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}
