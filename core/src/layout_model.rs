//! The whole of nero-layout's bookkeeping. This is the same pattern as
//! [`crate::ws_model`]: the JS side keeps the [`Model`] as a JSON value it does
//! not read, passes it in with the facts GNOME gives (windows, monitors), and
//! carries out what comes back.
//!
//! Decided here:
//! - which windows are saved and how (the snapshot rules)
//! - when the file is written (unchanged, fewer windows at session end)
//! - the check of a hand-edited or damaged file
//! - a restore: which window takes which saved place, the steps to put it
//!   there, the stacking order, and which apps to start
//! - whether this enable is a login
//!
//! The spot maths, the title matching and the stacking rule are in
//! [`crate::layout`].

use crate::Rect;
use crate::layout::{self, Placed, Saved, Spot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// The version of `~/.local/state/nero/layout.json`.
pub const FILE_VERSION: i64 = 1;
/// A Shell process younger than this (seconds) is taken as a fresh login.
pub const LOGIN_UPTIME_S: f64 = 120.0;

/// One saved window, as in the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub app: String,
    pub title: String,
    pub connector: Option<String>,
    pub spot: Option<Spot>,
    /// -1 = none (on all workspaces, or a monitor without workspaces of its own).
    pub workspace: i64,
    pub sticky: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    pub minimized: bool,
    pub above: bool,
}

/// A restore that is running.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Restore {
    pub run: u64,
    pub saved: Vec<Entry>,
    pub used: Vec<bool>,
    /// (window id, index in `saved`) of every window that took a saved place.
    pub placed: Vec<(u64, usize)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Model {
    /// The windows last written, in canonical JSON, and how many there were.
    pub last_written: Option<String>,
    pub last_count: usize,
    /// A smaller list seen once and not written yet (see [`save`]).
    pub pending_shrink: Option<String>,
    pub restore: Option<Restore>,
    /// The number of the last restore started (timers of older runs do nothing).
    pub run: u64,
    /// Windows that already took a saved place; they are not claimed again
    /// until a restore-now.
    pub claimed: BTreeSet<u64>,
}

// ---- the file -----------------------------------------------------------------

/// One entry of the file with only well-formed fields, or None when it has no
/// app. The file can be edited by hand or damaged: one wrong field (a title that
/// is not text, a spot that is not four numbers) must not make the whole list
/// unusable, which would leave every window unrestored.
pub fn clean_entry(e: &Value) -> Option<Entry> {
    let o = e.as_object()?;
    let app = o
        .get("app")?
        .as_str()
        .filter(|a| !a.is_empty())?
        .to_string();
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|f| f.is_finite());
    let spot = o.get("spot").and_then(Value::as_object).and_then(|s| {
        Some(Spot {
            fx: num(s.get("fx"))?,
            fy: num(s.get("fy"))?,
            fw: num(s.get("fw"))?,
            fh: num(s.get("fh"))?,
        })
    });
    let whole = |v: &Value| {
        v.as_i64().or_else(|| {
            v.as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 1e15)
                .map(|f| f as i64)
        })
    };
    let flag = |k: &str| o.get(k).and_then(Value::as_bool) == Some(true);
    Some(Entry {
        app,
        title: o
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        connector: o
            .get("connector")
            .and_then(Value::as_str)
            .map(str::to_string),
        spot,
        workspace: o
            .get("workspace")
            .and_then(whole)
            .filter(|w| *w >= 0)
            .unwrap_or(-1),
        sticky: flag("sticky"),
        maximized: flag("maximized"),
        fullscreen: flag("fullscreen"),
        minimized: flag("minimized"),
        above: flag("above"),
    })
}

/// The file's windows, cleaned, and the time it was saved; None when the text
/// is not a layout file of this version.
pub fn parse_file(text: &str) -> Option<(Vec<Entry>, String)> {
    let data: Value = serde_json::from_str(text).ok()?;
    if data.get("version").and_then(Value::as_i64) != Some(FILE_VERSION) {
        return None;
    }
    let windows = data.get("windows")?.as_array()?;
    let saved_at = data
        .get("saved")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some((windows.iter().filter_map(clean_entry).collect(), saved_at))
}

fn canonical(windows: &[Entry]) -> String {
    serde_json::to_string(windows).expect("plain structs")
}

/// What JS needs after reading the file at enable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub model: Model,
    /// The file holds a layout to restore.
    pub has_saved: bool,
}

pub fn start(file: Option<&str>) -> Started {
    let mut model = Model::default();
    let parsed = file.and_then(parse_file);
    if let Some((windows, _)) = &parsed {
        model.last_written = Some(canonical(windows));
        model.last_count = windows.len();
    }
    Started {
        model,
        has_saved: parsed.is_some(),
    }
}

/// The text of a layout file.
pub fn file_text(windows: &[Entry], saved_at: &str) -> String {
    #[derive(Serialize)]
    struct File<'a> {
        version: i64,
        saved: &'a str,
        windows: &'a [Entry],
    }
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    File {
        version: FILE_VERSION,
        saved: saved_at,
        windows,
    }
    .serialize(&mut ser)
    .expect("plain structs");
    out.push(b'\n');
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

// ---- login --------------------------------------------------------------------

/// Seconds since this process started, from `/proc/self/stat` and
/// `/proc/uptime` (clock ticks taken as 100 per second, as on Linux).
pub fn process_uptime(stat: &str, uptime: &str) -> Option<f64> {
    let rest = stat.get(stat.rfind(')')? + 2..)?;
    // after the name: state is field 3, starttime field 22 -> index 19 here
    let start_ticks: f64 = rest.split(' ').nth(19)?.parse().ok()?;
    let up: f64 = uptime.split(' ').next()?.trim().parse().ok()?;
    let s = up - start_ticks / 100.0;
    s.is_finite().then_some(s)
}

/// Is this enable a login? Not when an earlier enable in this Shell process
/// already was one (`done_before`, e.g. after the lock screen). Otherwise yes
/// while the Shell is starting up or is younger than two minutes. When the
/// uptime cannot be read, it is taken as not a login.
pub fn at_login(
    done_before: bool,
    starting_up: bool,
    stat: Option<&str>,
    uptime: Option<&str>,
) -> bool {
    if done_before {
        return false;
    }
    starting_up
        || stat
            .zip(uptime)
            .and_then(|(s, u)| process_uptime(s, u))
            .is_some_and(|s| s < LOGIN_UPTIME_S)
}

// ---- saving -------------------------------------------------------------------

/// A window as GNOME shows it, for the snapshot (bottom of the stack first).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapWin {
    /// The desktop file id of its app; None when it has none or the app is
    /// only "window backed" (no desktop file).
    pub app: Option<String>,
    #[serde(default)]
    pub normal: bool,
    #[serde(default)]
    pub skip_taskbar: bool,
    #[serde(default)]
    pub transient: bool,
    #[serde(default)]
    pub title: String,
    pub monitor: i64,
    pub connector: Option<String>,
    pub frame: Rect,
    pub work_area: Rect,
    #[serde(default)]
    pub on_all: bool,
    pub workspace: Option<i64>,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub above: bool,
}

/// The app a window is saved under: dialogs, helper windows and windows
/// without a taskbar button are not saved.
pub fn app_of(w: &SnapWin) -> Option<&str> {
    if !w.normal || w.skip_taskbar || w.transient {
        return None;
    }
    w.app.as_deref().filter(|a| !a.is_empty())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SnapOpts {
    /// Apps that are never saved or restored.
    pub skip: Vec<String>,
    /// GNOME's workspaces-only-on-primary.
    pub only_on_primary: bool,
    pub primary: i64,
}

/// The windows to save, in stacking order.
pub fn snapshot(wins: &[SnapWin], opts: &SnapOpts) -> Vec<Entry> {
    wins.iter()
        .filter_map(|w| {
            let app = app_of(w)?;
            if opts.skip.iter().any(|s| s == app) || w.monitor < 0 {
                return None;
            }
            // With workspaces on the primary monitor only, windows on the other
            // monitors are on all workspaces anyway: that is not a choice to restore.
            let other_monitor = opts.only_on_primary && w.monitor != opts.primary;
            let sticky = w.on_all && !other_monitor;
            Some(Entry {
                app: app.to_string(),
                title: w.title.clone(),
                connector: w.connector.clone(),
                spot: Some(layout::to_spot(&w.frame, &w.work_area)),
                workspace: if sticky || other_monitor {
                    -1
                } else {
                    w.workspace.unwrap_or(-1)
                },
                sticky,
                maximized: w.maximized,
                fullscreen: w.fullscreen,
                minimized: w.minimized,
                above: w.above,
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveOut {
    /// The model once the file is written. JS keeps it only when the write
    /// worked; after a failed write it keeps the old one and tries again.
    pub model: Model,
    /// The file text to write, or None: nothing to write now.
    pub write: Option<String>,
}

/// Drop the claimed ids of windows that are gone (`ids` = every window now), so
/// the model does not grow over a long session. Without `ids`: unchanged.
pub fn forget_gone(mut model: Model, ids: Option<&[u64]>) -> Model {
    if let Some(ids) = ids {
        model.claimed.retain(|id| ids.contains(id));
    }
    model
}

/// A periodic save (or a forced one: save-now). Nothing is written while a
/// restore runs, or when nothing changed. When there are fewer windows than last
/// time, the session may be ending and apps closing, so the smaller list is only
/// written when it is still the same at the next save.
pub fn save(
    mut model: Model,
    wins: &[SnapWin],
    opts: &SnapOpts,
    force: bool,
    now: &str,
) -> SaveOut {
    if model.restore.is_some() && !force {
        return SaveOut { model, write: None };
    }
    let windows = snapshot(wins, opts);
    let text = canonical(&windows);
    if model.last_written.as_deref() == Some(text.as_str()) {
        // back to what is on disk: a shrink seen before no longer counts as seen
        model.pending_shrink = None;
        return SaveOut { model, write: None };
    }
    if !force
        && windows.len() < model.last_count
        && model.pending_shrink.as_deref() != Some(text.as_str())
    {
        model.pending_shrink = Some(text);
        return SaveOut { model, write: None };
    }
    model.pending_shrink = None;
    let write = file_text(&windows, now);
    // a failed write keeps the old model in JS, so a dropped pending_shrink
    // there is the old one too
    model.last_written = Some(text);
    model.last_count = windows.len();
    SaveOut {
        model,
        write: Some(write),
    }
}

// ---- restoring ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreStart {
    pub model: Model,
    /// The run number, for the timers of this restore; None = nothing to restore.
    pub run: Option<u64>,
    pub count: usize,
    pub saved_at: String,
}

/// Start a restore from the file text. `manual` = restore-now in a running
/// session: windows that were placed before take part again. A restore that
/// was running ends first.
pub fn restore_start(
    model: Model,
    file: Option<&str>,
    skip: &[String],
    manual: bool,
) -> RestoreStart {
    let mut model = restore_end(model, None);
    if manual {
        model.claimed.clear();
    }
    let (saved, saved_at) = file.and_then(parse_file).unwrap_or_default();
    let saved: Vec<Entry> = saved
        .into_iter()
        .filter(|e| !skip.contains(&e.app))
        .collect();
    if saved.is_empty() {
        return RestoreStart {
            model,
            run: None,
            count: 0,
            saved_at,
        };
    }
    model.run += 1;
    let run = model.run;
    let count = saved.len();
    model.restore = Some(Restore {
        run,
        used: vec![false; count],
        saved,
        placed: vec![],
    });
    RestoreStart {
        model,
        run: Some(run),
        count,
        saved_at,
    }
}

/// End the restore. With `run`, only if that run is still the current one (a
/// timer of an older restore-now does nothing).
pub fn restore_end(mut model: Model, run: Option<u64>) -> Model {
    if run.is_none_or(|r| r == model.run) {
        model.restore = None;
    }
    model
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndOut {
    pub model: Model,
    /// The restore ended now (or none was running): stop waiting for windows.
    pub ended: bool,
}

/// [`restore_end`] for JS, which also needs to know whether it ended.
pub fn end(model: Model, run: Option<u64>) -> EndOut {
    let model = restore_end(model, run);
    let ended = model.restore.is_none();
    EndOut { model, ended }
}

/// A window that may take a saved place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimWin {
    pub id: u64,
    #[serde(flatten)]
    pub snap: SnapWin,
    /// It has a compositor actor (is shown).
    #[serde(default)]
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClaimOut {
    pub model: Model,
    /// The run the window was claimed in, or None: not claimed.
    pub run: Option<u64>,
}

/// Does this window take one of the saved places? The best-matching saved
/// window of its app that is still free.
pub fn claim(mut model: Model, w: &ClaimWin) -> ClaimOut {
    let none = |model| ClaimOut { model, run: None };
    if !w.live || model.claimed.contains(&w.id) {
        return none(model);
    }
    let Some(app) = app_of(&w.snap) else {
        return none(model);
    };
    let Some(r) = model.restore.as_mut() else {
        return none(model);
    };
    let saved: Vec<Saved> = r
        .saved
        .iter()
        .map(|e| Saved {
            app: e.app.clone(),
            title: e.title.clone(),
        })
        .collect();
    let Some(i) = layout::match_window(&saved, &r.used, app, &w.snap.title) else {
        return none(model);
    };
    // a model from JS with a short `used` list must not trap
    if r.used.len() < r.saved.len() {
        r.used.resize(r.saved.len(), false);
    }
    r.used[i] = true;
    r.placed.push((w.id, i));
    let run = r.run;
    model.claimed.insert(w.id);
    ClaimOut {
        model,
        run: Some(run),
    }
}

/// A monitor as JS reports it for placing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub connector: Option<String>,
    pub work_area: Rect,
}

/// The window being placed, as it is now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceWin {
    pub id: u64,
    pub live: bool,
    #[serde(default)]
    pub on_all: bool,
    pub workspace: Option<i64>,
    pub monitor: i64,
    pub frame: Rect,
    #[serde(default)]
    pub maximized: bool,
    /// Maximized in at least one direction.
    #[serde(default)]
    pub any_maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub above: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceFacts {
    pub window: PlaceWin,
    pub n_workspaces: i64,
    pub monitors: Vec<Monitor>,
}

/// One window call for JS, in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "camelCase")]
pub enum Action {
    Stick,
    Workspace { index: i64 },
    Unfullscreen,
    Unmaximize,
    ToMonitor { monitor: i64 },
    MoveResize { rect: Rect },
    Maximize,
    Fullscreen,
    Above,
    Minimize,
}

/// The steps that put a claimed window on its saved place. With `run`, only
/// while that restore is still the current one (the second try 1.2 s later).
pub fn place(model: &Model, facts: &PlaceFacts, run: Option<u64>) -> Vec<Action> {
    let w = &facts.window;
    let Some(r) = model
        .restore
        .as_ref()
        .filter(|r| run.is_none_or(|x| x == r.run))
    else {
        return vec![];
    };
    let Some(e) = r
        .placed
        .iter()
        .find(|p| p.0 == w.id)
        .and_then(|p| r.saved.get(p.1))
    else {
        return vec![];
    };
    if !w.live {
        return vec![]; // closed meanwhile
    }
    let mut out = vec![];
    if e.sticky {
        if !w.on_all {
            out.push(Action::Stick);
        }
    } else if e.workspace >= 0 && facts.n_workspaces > 0 {
        // with dynamic workspaces the last one is empty; filling it adds the next
        let idx = e.workspace.min(facts.n_workspaces - 1);
        if w.workspace != Some(idx) {
            out.push(Action::Workspace { index: idx });
        }
    }
    let m = e.connector.as_deref().and_then(|c| {
        facts
            .monitors
            .iter()
            .position(|m| m.connector.as_deref() == Some(c))
    });
    let Some(m) = m else {
        return out; // that monitor is not connected: leave the window where it opened
    };
    let mut fullscreen = w.fullscreen;
    let mut any_max = w.any_maximized;
    let mut maximized = w.maximized;
    if fullscreen && !e.fullscreen {
        out.push(Action::Unfullscreen);
        fullscreen = false;
    }
    if any_max && !e.maximized {
        out.push(Action::Unmaximize);
        any_max = false;
        maximized = false;
    }
    let moved = w.monitor != m as i64;
    if moved {
        out.push(Action::ToMonitor { monitor: m as i64 });
    }
    if let Some(spot) = &e.spot {
        let rect = layout::from_spot(spot, &facts.monitors[m].work_area);
        // after a monitor move the frame is not known here: always set it then
        if !any_max && !fullscreen && (moved || rect != w.frame) {
            out.push(Action::MoveResize { rect });
        }
    }
    if e.maximized && !maximized {
        out.push(Action::Maximize);
    }
    if e.fullscreen && !fullscreen {
        out.push(Action::Fullscreen);
    }
    if e.above && !w.above {
        out.push(Action::Above);
    }
    if e.minimized && !w.minimized {
        out.push(Action::Minimize);
    }
    out
}

/// A placed window for restacking: shown and not minimized ones take part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackWin {
    pub id: u64,
    pub monitor: i64,
    pub live: bool,
    #[serde(default)]
    pub minimized: bool,
}

/// The window ids to raise, in this order, after window `id` was placed (see
/// [`layout::restack_order`]).
pub fn restack(model: &Model, wins: &[StackWin], id: u64) -> Vec<u64> {
    let Some(r) = &model.restore else {
        return vec![];
    };
    let index_of = |wid: u64| r.placed.iter().find(|p| p.0 == wid).map(|p| p.1);
    let Some(new_index) = index_of(id) else {
        return vec![];
    };
    let live: Vec<(u64, Placed)> = wins
        .iter()
        .filter(|w| w.live && !w.minimized)
        .filter_map(|w| {
            Some((
                w.id,
                Placed {
                    index: index_of(w.id)?,
                    monitor: i32::try_from(w.monitor).ok()?,
                },
            ))
        })
        .collect();
    let placed: Vec<Placed> = live.iter().map(|p| p.1).collect();
    layout::restack_order(&placed, new_index)
        .into_iter()
        .filter_map(|i| live.iter().find(|p| p.1.index == i).map(|p| p.0))
        .collect()
}

/// The apps of saved windows no window took (yet).
pub fn missing_apps(model: &Model) -> Vec<String> {
    let Some(r) = &model.restore else {
        return vec![];
    };
    let mut seen = HashSet::new();
    r.saved
        .iter()
        .zip(&r.used)
        .filter(|(_, used)| !**used)
        .map(|(e, _)| e.app.clone())
        .filter(|a| seen.insert(a.clone()))
        .collect()
}

/// One autostart entry of the session (a .desktop file in an autostart folder).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartEntry {
    /// The file name.
    pub name: String,
    #[serde(default)]
    pub hidden: bool,
    /// X-GNOME-Autostart-enabled (missing = true).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The program of its Exec line (first word, as parsed by GLib).
    pub exec: Option<String>,
}

fn yes() -> bool {
    true
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// An app GNOME knows, for [`launch_plan`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub id: String,
    /// Shell.AppState.STOPPED.
    pub stopped: bool,
    pub executable: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LaunchPlan {
    /// Start these (once).
    pub start: Vec<String>,
    /// Not started: the session autostarts them.
    pub autostart: Vec<String>,
}

/// Which of the missing apps to start, with `run` the current restore. Apps
/// that are running are left alone. So are apps the session starts itself:
/// GNOME calls an app "stopped" until it shows a window, so starting it here as
/// well could run it twice.
pub fn launch_plan(
    model: &Model,
    run: u64,
    apps: &[AppInfo],
    autostart: &[AutostartEntry],
) -> LaunchPlan {
    if model.restore.as_ref().is_none_or(|r| r.run != run) {
        return LaunchPlan::default();
    }
    let mut names: HashSet<&str> = HashSet::new();
    for a in autostart.iter().filter(|a| !a.hidden && a.enabled) {
        names.insert(&a.name);
        if let Some(x) = a.exec.as_deref().filter(|x| !x.is_empty()) {
            names.insert(basename(x));
        }
    }
    let mut plan = LaunchPlan::default();
    for id in missing_apps(model) {
        let Some(app) = apps.iter().find(|a| a.id == id) else {
            continue;
        };
        if !app.stopped {
            continue;
        }
        let exe = app.executable.as_deref().map(basename);
        if names.contains(id.as_str()) || exe.is_some_and(|x| names.contains(x)) {
            plan.autostart.push(id);
        } else {
            plan.start.push(id);
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WA: Rect = Rect::new(0.0, 32.0, 2560.0, 1408.0);

    fn snap(app: &str, title: &str, monitor: i64) -> SnapWin {
        SnapWin {
            app: Some(app.into()),
            normal: true,
            skip_taskbar: false,
            transient: false,
            title: title.into(),
            monitor,
            connector: Some(["DP-1", "HDMI-2"][monitor as usize].into()),
            frame: Rect::new(100.0, 132.0, 1280.0, 704.0),
            work_area: WA,
            on_all: false,
            workspace: Some(0),
            maximized: false,
            fullscreen: false,
            minimized: false,
            above: false,
        }
    }

    fn file(windows: &[Entry]) -> String {
        file_text(windows, "2026-11-27T06:00:00.000Z")
    }

    #[test]
    fn clean_entry_keeps_what_is_good() {
        let e = clean_entry(&json!({
            "app": "a.desktop", "title": 5, "connector": "DP-1",
            "spot": {"fx": 0.1, "fy": 0, "fw": "x", "fh": 1},
            "workspace": 2.0, "maximized": true, "above": "yes"
        }))
        .unwrap();
        assert_eq!(e.title, "");
        assert_eq!(e.spot, None);
        assert_eq!(e.workspace, 2);
        assert!(e.maximized && !e.above);
        assert_eq!(clean_entry(&json!({"app": ""})), None);
        assert_eq!(clean_entry(&json!("a.desktop")), None);
        assert_eq!(
            clean_entry(&json!({"app": "a", "workspace": -3}))
                .unwrap()
                .workspace,
            -1
        );
        assert_eq!(
            clean_entry(&json!({"app": "a", "workspace": 1.5}))
                .unwrap()
                .workspace,
            -1
        );
    }

    #[test]
    fn parse_file_drops_bad_entries_not_the_file() {
        let t =
            r#"{"version":1,"saved":"x","windows":[{"app":"a"},7,{"title":"no app"},{"app":"b"}]}"#;
        let (w, at) = parse_file(t).unwrap();
        assert_eq!(
            w.iter().map(|e| e.app.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(at, "x");
        assert!(parse_file(r#"{"version":2,"windows":[]}"#).is_none());
        assert!(parse_file(r#"{"version":1,"windows":{}}"#).is_none());
        assert!(parse_file("{").is_none());
    }

    #[test]
    fn snapshot_rules() {
        let mut dialog = snap("d.desktop", "", 0);
        dialog.normal = false;
        let mut sticky = snap("s.desktop", "", 1);
        sticky.on_all = true;
        let mut none = snap("x", "", 0);
        none.app = None;
        let wins = [
            snap("a.desktop", "A", 0),
            dialog,
            sticky.clone(),
            none,
            snap("skip.desktop", "", 0),
        ];
        let opts = SnapOpts {
            skip: vec!["skip.desktop".into()],
            only_on_primary: false,
            primary: 0,
        };
        let s = snapshot(&wins, &opts);
        assert_eq!(
            s.iter().map(|e| e.app.as_str()).collect::<Vec<_>>(),
            ["a.desktop", "s.desktop"]
        );
        assert_eq!((s[1].sticky, s[1].workspace), (true, -1));
        assert_eq!(s[0].spot.unwrap().fw, 0.5);
        // only on primary: the window on monitor 1 is on all workspaces, but not by choice
        let opts = SnapOpts {
            only_on_primary: true,
            ..opts
        };
        let s = snapshot(&[sticky], &opts);
        assert_eq!((s[0].sticky, s[0].workspace), (false, -1));
    }

    #[test]
    fn save_writes_changes_and_waits_on_a_shrink() {
        let opts = SnapOpts::default();
        let two = [snap("a.desktop", "A", 0), snap("b.desktop", "B", 0)];
        let out = save(Model::default(), &two, &opts, false, "t1");
        let text = out.write.clone().unwrap();
        assert!(text.starts_with("{\n \"version\": 1,\n \"saved\": \"t1\",\n \"windows\": ["));
        let m = out.model;
        // unchanged: nothing
        assert_eq!(save(m.clone(), &two, &opts, false, "t2").write, None);
        // one window less: only on the second time
        let one = &two[..1];
        let o1 = save(m.clone(), one, &opts, false, "t3");
        assert_eq!(o1.write, None);
        let o2 = save(o1.model, one, &opts, false, "t4");
        assert!(o2.write.is_some());
        // forced: at once
        assert!(save(m.clone(), one, &opts, true, "t5").write.is_some());
        // shrink seen, back to the written list, then the same shrink again: that
        // is a first sighting again, not the second
        let o1 = save(m.clone(), one, &opts, false, "t7");
        let back = save(o1.model, &two, &opts, false, "t8");
        assert_eq!(back.write, None);
        assert_eq!(save(back.model, one, &opts, false, "t9").write, None);
        // a file read at start counts as written
        let started = start(Some(&text));
        assert!(started.has_saved);
        assert_eq!(save(started.model, &two, &opts, false, "t6").write, None);
    }

    #[test]
    fn no_saving_during_a_restore_unless_forced() {
        let e = snapshot(&[snap("a.desktop", "A", 0)], &SnapOpts::default());
        let r = restore_start(Model::default(), Some(&file(&e)), &[], false);
        let wins = [snap("b.desktop", "B", 0)];
        assert_eq!(
            save(r.model.clone(), &wins, &SnapOpts::default(), false, "t").write,
            None
        );
        assert!(
            save(r.model, &wins, &SnapOpts::default(), true, "t")
                .write
                .is_some()
        );
    }

    #[test]
    fn login_detection() {
        let stat = "1234 (gnome-shell) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 50000 21";
        assert_eq!(process_uptime(stat, "560.5 1000.0\n"), Some(60.5));
        assert!(at_login(false, false, Some(stat), Some("560.5 1")));
        assert!(!at_login(false, false, Some(stat), Some("9000 1")));
        assert!(!at_login(true, true, Some(stat), Some("560.5 1")));
        assert!(at_login(false, true, None, None));
        assert!(!at_login(false, false, None, None));
        // a name with spaces and brackets
        let odd = "1 (a) b) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 100 21";
        assert_eq!(process_uptime(odd, "2 0"), Some(1.0));
        // cut off right after the name, or a multi-byte char after it: no panic
        assert_eq!(process_uptime("1 (gnome-shell)", "2 0"), None);
        assert_eq!(process_uptime("1 (x)é", "2 0"), None);
    }

    fn restoring(entries: &[Entry]) -> Model {
        restore_start(Model::default(), Some(&file(entries)), &[], false).model
    }

    fn claim_win(id: u64, app: &str, title: &str) -> ClaimWin {
        ClaimWin {
            id,
            snap: snap(app, title, 0),
            live: true,
        }
    }

    #[test]
    fn claim_takes_the_best_free_place_once() {
        let entries = snapshot(
            &[
                snap("b.desktop", "Mail - Brave", 0),
                snap("b.desktop", "GitHub - Brave", 1),
            ],
            &SnapOpts::default(),
        );
        let m = restoring(&entries);
        let c = claim(m, &claim_win(7, "b.desktop", "GitHub - Brave"));
        assert_eq!(c.run, Some(1));
        assert_eq!(c.model.restore.as_ref().unwrap().placed, vec![(7, 1)]);
        // claimed already: not again
        let again = claim(
            c.model.clone(),
            &claim_win(7, "b.desktop", "GitHub - Brave"),
        );
        assert_eq!(again.run, None);
        // the next one gets the other place
        let c2 = claim(c.model, &claim_win(8, "b.desktop", "New Tab"));
        assert_eq!(
            c2.model.restore.as_ref().unwrap().placed,
            vec![(7, 1), (8, 0)]
        );
        // nothing left
        assert_eq!(
            claim(c2.model.clone(), &claim_win(9, "b.desktop", "x")).run,
            None
        );
        // a short `used` list from JS does not trap
        let mut short = c2.model.clone();
        short.restore.as_mut().unwrap().used.clear();
        short.claimed.clear();
        assert!(claim(short, &claim_win(10, "b.desktop", "x")).run.is_some());
        // closed windows leave `claimed` at the next save
        let kept = forget_gone(c2.model.clone(), Some(&[8]));
        assert_eq!(kept.claimed.iter().copied().collect::<Vec<_>>(), [8]);
        assert_eq!(
            forget_gone(c2.model.clone(), None).claimed,
            c2.model.claimed
        );
        // a restore-now frees the claimed windows
        let again = restore_start(c2.model, Some(&file(&entries)), &[], true);
        assert_eq!(again.run, Some(2));
        assert_eq!(
            claim(again.model, &claim_win(7, "b.desktop", "x")).run,
            Some(2)
        );
    }

    #[test]
    fn restore_skips_apps_and_empty_files() {
        let entries = snapshot(&[snap("a.desktop", "A", 0)], &SnapOpts::default());
        let r = restore_start(
            Model::default(),
            Some(&file(&entries)),
            &["a.desktop".into()],
            false,
        );
        assert_eq!((r.run, r.model.restore.is_none()), (None, true));
        assert_eq!(restore_start(Model::default(), None, &[], false).run, None);
    }

    #[test]
    fn old_timers_do_not_end_a_new_restore() {
        let entries = snapshot(&[snap("a.desktop", "A", 0)], &SnapOpts::default());
        let r1 = restore_start(Model::default(), Some(&file(&entries)), &[], false);
        let r2 = restore_start(r1.model, Some(&file(&entries)), &[], true);
        let o = end(r2.model, r1.run);
        assert!(!o.ended);
        let m = o.model;
        assert!(m.restore.is_some());
        assert!(restore_end(m, r2.run).restore.is_none());
    }

    fn monitors() -> Vec<Monitor> {
        vec![
            Monitor {
                connector: Some("DP-1".into()),
                work_area: WA,
            },
            Monitor {
                connector: Some("HDMI-2".into()),
                work_area: Rect::new(2560.0, 0.0, 1920.0, 1080.0),
            },
        ]
    }

    fn place_win(id: u64, monitor: i64) -> PlaceWin {
        PlaceWin {
            id,
            live: true,
            on_all: false,
            workspace: Some(0),
            monitor,
            frame: Rect::new(0.0, 0.0, 10.0, 10.0),
            maximized: false,
            any_maximized: false,
            fullscreen: false,
            minimized: false,
            above: false,
        }
    }

    fn claimed(entry: Entry) -> Model {
        let m = restoring(std::slice::from_ref(&entry));
        let w = ClaimWin {
            id: 1,
            snap: SnapWin {
                app: Some(entry.app.clone()),
                title: entry.title.clone(),
                ..snap("x", "", 0)
            },
            live: true,
        };
        claim(m, &w).model
    }

    #[test]
    fn place_steps() {
        let mut e = snapshot(&[snap("a.desktop", "A", 1)], &SnapOpts::default()).remove(0);
        e.workspace = 3;
        e.above = true;
        let m = claimed(e.clone());
        let facts = PlaceFacts {
            window: place_win(1, 0),
            n_workspaces: 3,
            monitors: monitors(),
        };
        let a = place(&m, &facts, None);
        let rect = layout::from_spot(&e.spot.unwrap(), &monitors()[1].work_area);
        assert_eq!(
            a,
            vec![
                Action::Workspace { index: 2 },
                Action::ToMonitor { monitor: 1 },
                Action::MoveResize { rect },
                Action::Above
            ]
        );
        // already there: nothing
        let mut there = place_win(1, 1);
        there.workspace = Some(2);
        there.frame = rect;
        there.above = true;
        let facts2 = PlaceFacts {
            window: there,
            ..facts.clone()
        };
        assert!(place(&m, &facts2, None).is_empty());
        // another run's timer: nothing
        assert!(place(&m, &facts, Some(9)).is_empty());
        // closed: nothing
        let mut gone = facts.clone();
        gone.window.live = false;
        assert!(place(&m, &gone, None).is_empty());
    }

    #[test]
    fn place_unmaximizes_before_moving_and_maximizes_after() {
        let mut e = snapshot(&[snap("a.desktop", "A", 0)], &SnapOpts::default()).remove(0);
        let mut w = place_win(1, 0);
        w.maximized = true;
        w.any_maximized = true;
        let facts = PlaceFacts {
            window: w,
            n_workspaces: 5,
            monitors: monitors(),
        };
        let a = place(&claimed(e.clone()), &facts, None);
        assert_eq!(a[0], Action::Unmaximize);
        assert!(matches!(a[1], Action::MoveResize { .. }));
        // saved maximized and maximized now: no move, no unmaximize
        e.maximized = true;
        assert!(place(&claimed(e.clone()), &facts, None).is_empty());
        // monitor not connected: only the workspace part
        e.connector = Some("DP-9".into());
        e.sticky = true;
        assert_eq!(place(&claimed(e), &facts, None), vec![Action::Stick]);
    }

    #[test]
    fn restack_by_saved_order_on_one_monitor() {
        let entries = snapshot(
            &[
                snap("a.desktop", "1", 0),
                snap("a.desktop", "2", 0),
                snap("a.desktop", "3", 0),
            ],
            &SnapOpts::default(),
        );
        let mut m = restoring(&entries);
        // windows open in the order 3, 1, 2
        for (id, t) in [(30, "3"), (10, "1"), (20, "2")] {
            m = claim(m, &claim_win(id, "a.desktop", t)).model;
        }
        let sw = |id, monitor| StackWin {
            id,
            monitor,
            live: true,
            minimized: false,
        };
        let wins = [sw(30, 0), sw(10, 0), sw(20, 0)];
        assert_eq!(restack(&m, &wins, 20), vec![20, 30]);
        assert_eq!(restack(&m, &wins, 10), vec![10, 20, 30]);
        assert!(restack(&m, &wins, 99).is_empty());
    }

    #[test]
    fn launch_only_stopped_non_autostart_apps() {
        let entries = snapshot(
            &[
                snap("a.desktop", "", 0),
                snap("b.desktop", "", 0),
                snap("c.desktop", "", 0),
                snap("d.desktop", "", 0),
                snap("a.desktop", "", 0),
            ],
            &SnapOpts::default(),
        );
        let r = restore_start(Model::default(), Some(&file(&entries)), &[], false);
        let m = claim(r.model, &claim_win(1, "d.desktop", "")).model;
        assert_eq!(missing_apps(&m), ["a.desktop", "b.desktop", "c.desktop"]);
        let app = |id: &str, stopped, exe: Option<&str>| AppInfo {
            id: id.into(),
            stopped,
            executable: exe.map(String::from),
        };
        let apps = [
            app("a.desktop", true, Some("/usr/bin/aa")),
            app("b.desktop", false, None),
            app("c.desktop", true, Some("cc")),
        ];
        let auto = [AutostartEntry {
            name: "whatever.desktop".into(),
            hidden: false,
            enabled: true,
            exec: Some("/opt/cc".into()),
        }];
        let plan = launch_plan(&m, 1, &apps, &auto);
        assert_eq!(plan.start, ["a.desktop"]);
        assert_eq!(plan.autostart, ["c.desktop"]);
        // a disabled autostart entry does not count
        let off = [AutostartEntry {
            enabled: false,
            ..auto[0].clone()
        }];
        assert_eq!(
            launch_plan(&m, 1, &apps, &off).start,
            ["a.desktop", "c.desktop"]
        );
        // an older run: nothing
        assert_eq!(launch_plan(&m, 7, &apps, &auto), LaunchPlan::default());
    }

    #[test]
    fn model_survives_json() {
        let entries = snapshot(&[snap("a.desktop", "A", 0)], &SnapOpts::default());
        let m = claim(
            restoring(&entries),
            &claim_win(4294967297, "a.desktop", "A"),
        )
        .model;
        let t = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Model>(&t).unwrap(), m);
    }
}
