//! The whole of nero-workspaces' bookkeeping as one state machine. The JS side
//! (nero-workspaces/extension.js) holds no decisions of its own. It holds the
//! [`Model`] as a JSON value it does not read, and on every GNOME event it hands
//! that model over together with the current [`Facts`]: which windows exist,
//! where they are, and which one has focus. It gets back the new model and a
//! list of [`Effect`]s to carry out (move this window, focus that one, show the
//! OSD, save the state).
//!
//! The model is not kept inside the WebAssembly instance on purpose. nero.js
//! starts a fresh instance after a trap, and state kept in Rust would be lost
//! then. Because JS keeps the model between calls, a failed call only loses
//! that one event.
//!
//! The placement rules themselves (real workspace `vw + 1` for parked windows,
//! the scroll steps, the swipe points) are in [`crate::workspaces`] and
//! [`crate::touchpad`]. This module decides when they are used.

use crate::touchpad as tp;
use crate::workspaces::{self as ws, ScrollEvent, ScrollState, VISIBLE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The version of the saved state file (`~/.local/state/nero/workspaces.json`).
pub const STATE_VERSION: i64 = 1;
/// The most virtual workspaces per monitor.
pub const MAX_COUNT: i64 = 12;

/// The settings of the extension that the logic needs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Virtual workspaces per monitor (the setting may be out of range).
    pub count: i64,
    pub wrap: bool,
    pub show_osd: bool,
    pub scroll_threshold: f64,
    pub scroll_cooldown_ms: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            count: 4,
            wrap: false,
            show_osd: true,
            scroll_threshold: 1.0,
            scroll_cooldown_ms: 250.0,
        }
    }
}

impl Config {
    /// The count in the range GNOME can hold: 1 ..= 12.
    pub fn count(&self) -> i64 {
        self.count.clamp(1, MAX_COUNT)
    }
}

/// Everything nero-workspaces remembers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Model {
    pub config: Config,
    /// Connector -> the virtual workspace that monitor shows.
    pub current: BTreeMap<String, i64>,
    /// Tracked window id -> its virtual workspace.
    pub vw: BTreeMap<u64, i64>,
    /// Panel monitor -> scroll state of that panel.
    pub scroll: BTreeMap<i64, ScrollState>,
    /// The monitor a running three-finger swipe started on.
    pub swipe_monitor: Option<i64>,
}

/// One tracked window as GNOME shows it now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WinFact {
    pub id: u64,
    pub monitor: i64,
    /// The index of `get_workspace()`, or None when it has none.
    pub real: Option<i64>,
    #[serde(default)]
    pub on_all: bool,
    #[serde(default)]
    pub minimized: bool,
    /// The place in the stacking order (higher = more on top).
    #[serde(default)]
    pub stack: i64,
}

/// What JS reports with every event.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Facts {
    /// The connector name of each monitor, by index ("" = unknown).
    pub monitors: Vec<String>,
    /// The tracked windows.
    pub windows: Vec<WinFact>,
    /// The focused window, when it is tracked.
    pub focus: Option<u64>,
    /// The monitor of the focused window, tracked or not.
    pub focus_monitor: Option<i64>,
    /// The monitor under the pointer.
    pub current_monitor: i64,
}

impl Facts {
    fn win(&self, id: u64) -> Option<&WinFact> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// The name a monitor is tracked by: its connector, or "#index".
    pub fn connector(&self, monitor: i64) -> String {
        usize::try_from(monitor)
            .ok()
            .and_then(|i| self.monitors.get(i))
            .filter(|c| !c.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("#{monitor}"))
    }
}

/// One thing for JS to do, in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "camelCase")]
pub enum Effect {
    /// `change_workspace_by_index(real)` of window `id`.
    Move { id: u64, real: i64 },
    /// `Main.activateWindow`: brings the window and its workspace forward.
    Focus { id: u64 },
    /// `window.activate()`: focus a window that is on screen already.
    Raise { id: u64 },
    /// The workspace OSD on a monitor.
    Osd { monitor: i64, text: String },
    /// Write the state file soon (JS waits a moment and asks for [`state_text`]).
    Save,
    /// Set GNOME's `num-workspaces`.
    NumWorkspaces { n: i64 },
    /// Call [`place`] for this window before the next redraw.
    PlaceLater { id: u64 },
    /// Make the real visible workspace the active one again.
    ShowVisible,
}

/// A new model plus what to do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub model: Model,
    pub effects: Vec<Effect>,
}

fn step(model: Model, effects: Vec<Effect>) -> Step {
    Step { model, effects }
}

// ---- state file -------------------------------------------------------------

/// A fresh model from the saved state file text (None = no file). A damaged or
/// hand-edited file cannot leave a monitor on a workspace that does not exist:
/// bad entries are dropped.
pub fn init(saved: Option<&str>, config: Config) -> Model {
    let mut model = Model {
        config,
        ..Default::default()
    };
    let Some(data) = saved.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()) else {
        return model;
    };
    if data.get("version").and_then(serde_json::Value::as_i64) != Some(STATE_VERSION) {
        return model;
    }
    if let Some(cur) = data.get("current").and_then(serde_json::Value::as_object) {
        for (c, v) in cur {
            // whole numbers only (5.0 is fine, 5.5 is not), like Number.isInteger
            let n = v.as_i64().or_else(|| {
                v.as_f64()
                    .filter(|f| f.fract() == 0.0 && f.abs() < 1e15)
                    .map(|f| f as i64)
            });
            if let Some(n) = n.filter(|n| (0..MAX_COUNT).contains(n)) {
                model.current.insert(c.clone(), n);
            }
        }
    }
    model
}

/// [`init`] plus what has to happen at the start: GNOME gets the fixed number
/// of real workspaces (one visible plus one per virtual workspace).
pub fn start(saved: Option<&str>, config: Config) -> Step {
    let model = init(saved, config);
    let n = model.config.count() + 1;
    step(model, vec![Effect::NumWorkspaces { n }])
}

/// The state file text: which workspace each monitor shows.
pub fn state_text(model: &Model) -> String {
    let data = serde_json::json!({ "version": STATE_VERSION, "current": model.current });
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    data.serialize(&mut ser)
        .expect("a JSON value always serializes");
    out.push(b'\n');
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

// ---- GNOME's own settings -----------------------------------------------------

/// GNOME's workspace settings before Nero took them over.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GnomeSettings {
    pub dynamic: bool,
    pub only_on_primary: bool,
    pub num: i64,
}

/// The settings to put back. A field that is None goes back to GNOME's default
/// (it was missing or wrong). `damaged` = the saved text was not JSON at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiveBack {
    pub dynamic: Option<bool>,
    pub only_on_primary: Option<bool>,
    pub num: Option<i64>,
    pub damaged: bool,
}

/// The text to keep in `saved-gnome-settings`.
pub fn saved_gnome(s: &GnomeSettings) -> String {
    serde_json::to_string(s).expect("plain struct")
}

/// What to put back from the text in `saved-gnome-settings`. A damaged or partial
/// value must not leave GNOME on Nero's fixed workspaces.
pub fn give_back(text: &str) -> GiveBack {
    let v: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let field = |k: &str| v.as_ref().and_then(|v| v.get(k));
    GiveBack {
        dynamic: field("dynamic").and_then(serde_json::Value::as_bool),
        only_on_primary: field("onlyOnPrimary").and_then(serde_json::Value::as_bool),
        num: field("num")
            .and_then(serde_json::Value::as_i64)
            .filter(|n| (1..=36).contains(n)),
        damaged: v.is_none(),
    }
}

// ---- windows ------------------------------------------------------------------

/// The virtual workspace a monitor shows, kept inside the current count.
pub fn current_of(model: &Model, facts: &Facts, monitor: i64) -> i64 {
    model
        .current
        .get(&facts.connector(monitor))
        .map_or(0, |c| (*c).clamp(0, model.config.count() - 1))
}

/// A new window (fresh) or one that was there when Nero started. Windows on all
/// workspaces are tracked too, and left alone while they are: GNOME can take them
/// off all workspaces later without a new window.
pub fn track(mut model: Model, facts: &Facts, id: u64, fresh: bool) -> Step {
    let Some(w) = facts.win(id) else {
        return step(model, vec![]);
    };
    if model.vw.contains_key(&id) {
        return step(model, vec![]);
    }
    let cur = current_of(&model, facts, w.monitor);
    let vw = ws::adopt(w.real.unwrap_or(VISIBLE), cur, model.config.count());
    model.vw.insert(id, vw);
    let effects = if fresh {
        place_effects(&model, facts, id)
    } else {
        vec![]
    };
    step(model, effects)
}

pub fn untrack(mut model: Model, id: u64) -> Model {
    model.vw.remove(&id);
    model
}

fn place_effects(model: &Model, facts: &Facts, id: u64) -> Vec<Effect> {
    let (Some(&vw), Some(w)) = (model.vw.get(&id), facts.win(id)) else {
        return vec![];
    };
    if w.on_all {
        return vec![];
    }
    let real = ws::real_for(vw, current_of(model, facts, w.monitor));
    if w.real == Some(real) {
        vec![]
    } else {
        vec![Effect::Move { id, real }]
    }
}

/// Put one window where its virtual workspace says.
pub fn place(model: Model, facts: &Facts, id: u64) -> Step {
    let effects = place_effects(&model, facts, id);
    step(model, effects)
}

/// Someone else (an app, the layout saver, the overview) moved a window to
/// another workspace: it now belongs to that one.
pub fn window_moved(mut model: Model, facts: &Facts, id: u64) -> Step {
    let Some(w) = facts.win(id) else {
        return step(model, vec![]);
    };
    let Some(real) = w.real else {
        // now on all workspaces: GNOME keeps it on screen everywhere
        return step(model, vec![]);
    };
    if !model.vw.contains_key(&id) {
        return step(model, vec![]);
    }
    let cur = current_of(&model, facts, w.monitor);
    model
        .vw
        .insert(id, ws::adopt(real, cur, model.config.count()));
    step(model, vec![Effect::PlaceLater { id }])
}

/// A window came onto another monitor. If it is on screen, it joins the
/// workspace that monitor shows; if it is parked, it is placed again.
pub fn entered_monitor(mut model: Model, facts: &Facts, id: u64) -> Step {
    let Some(w) = facts.win(id) else {
        return step(model, vec![]);
    };
    if !model.vw.contains_key(&id) {
        return step(model, vec![]);
    }
    if w.real == Some(VISIBLE) {
        let cur = current_of(&model, facts, w.monitor);
        model.vw.insert(id, cur);
        step(model, vec![])
    } else {
        step(model, vec![Effect::PlaceLater { id }])
    }
}

/// Every window where its virtual workspace says.
pub fn reconcile(model: Model, facts: &Facts) -> Step {
    let effects = model
        .vw
        .keys()
        .flat_map(|&id| place_effects(&model, facts, id))
        .collect();
    step(model, effects)
}

/// Take every window's workspace as it is now, then place them all. Used once
/// GNOME has applied the workspace settings, which it does a moment after enable.
pub fn readopt_all(mut model: Model, facts: &Facts) -> Step {
    let count = model.config.count();
    for w in &facts.windows {
        if let (Some(real), false, true) = (w.real, w.on_all, model.vw.contains_key(&w.id)) {
            let cur = current_of(&model, facts, w.monitor);
            model.vw.insert(w.id, ws::adopt(real, cur, count));
        }
    }
    reconcile(model, facts)
}

/// New settings. When the count changed, windows and monitors on workspaces
/// that went away go to the last one, and GNOME gets the new number.
pub fn set_config(mut model: Model, facts: &Facts, config: Config) -> Step {
    let old = model.config.count();
    model.config = config;
    let count = model.config.count();
    if count == old {
        return step(model, vec![]);
    }
    for vw in model.vw.values_mut() {
        *vw = (*vw).min(count - 1);
    }
    for cur in model.current.values_mut() {
        *cur = (*cur).min(count - 1);
    }
    let mut out = reconcile(model, facts);
    out.effects.push(Effect::NumWorkspaces { n: count + 1 });
    out.effects.push(Effect::Save);
    out
}

/// What disable() puts back: every window on the visible workspace, so nothing
/// is lost when Nero is switched off.
pub fn disable(model: &Model, facts: &Facts) -> Vec<Effect> {
    facts
        .windows
        .iter()
        .filter(|w| model.vw.contains_key(&w.id) && w.real != Some(VISIBLE))
        .map(|w| Effect::Move {
            id: w.id,
            real: VISIBLE,
        })
        .collect()
}

// ---- switching ----------------------------------------------------------------

/// Show virtual workspace `target` on `monitor`. `focus_window` (a tracked id)
/// is brought forward afterwards; without it, if the focus was on this monitor,
/// it moves to the top window of the workspace that came in.
pub fn switch(
    mut model: Model,
    facts: &Facts,
    monitor: i64,
    target: i64,
    focus_window: Option<u64>,
) -> Step {
    let count = model.config.count();
    if monitor < 0 || !(0..count).contains(&target) {
        return step(model, vec![]);
    }
    let list: Vec<ws::Win> = facts
        .windows
        .iter()
        .filter(|w| !w.on_all)
        .filter_map(|w| {
            Some(ws::Win {
                id: w.id,
                monitor: w.monitor,
                vw: *model.vw.get(&w.id)?,
                real: w.real.unwrap_or(VISIBLE),
            })
        })
        .collect();
    let moves = ws::plan_switch(&list, monitor, target);
    model.current.insert(facts.connector(monitor), target);

    let mut effects: Vec<Effect> = moves
        .iter()
        .map(|m| Effect::Move {
            id: m.id,
            real: m.real,
        })
        .collect();
    effects.push(Effect::Save);

    if let Some(id) = focus_window {
        effects.push(Effect::Focus { id });
    } else if facts.focus_monitor == Some(monitor) {
        let real_after = |w: &WinFact| {
            moves
                .iter()
                .find(|m| m.id == w.id)
                .map(|m| m.real)
                .or(w.real)
        };
        let top = facts
            .windows
            .iter()
            .filter(|w| model.vw.contains_key(&w.id))
            .filter(|w| w.monitor == monitor && !w.minimized && real_after(w) == Some(VISIBLE))
            .max_by_key(|w| w.stack);
        if let Some(w) = top {
            effects.push(Effect::Raise { id: w.id });
        }
    }
    if model.config.show_osd {
        effects.push(Effect::Osd {
            monitor,
            text: format!("{} / {}", target + 1, count),
        });
    }
    step(model, effects)
}

/// `delta` workspaces further on `monitor` (stops at the ends unless wrap is on).
pub fn step_monitor(model: Model, facts: &Facts, monitor: i64, delta: i64) -> Step {
    let cur = current_of(&model, facts, monitor);
    let next = ws::step(cur, delta, model.config.count(), model.config.wrap);
    if next == cur {
        return step(model, vec![]);
    }
    switch(model, facts, monitor, next, None)
}

/// A workspace key: `binding` is GNOME's name for it (switch-to-workspace-left,
/// move-to-workspace-3, ...). `monitor` is the monitor of the window the key is
/// for (or the one under the pointer), `window` that window when it is tracked.
pub fn key(model: Model, facts: &Facts, binding: &str, monitor: i64, window: Option<u64>) -> Step {
    let parts: Vec<&str> = binding.split('-').collect();
    let (Some(&action), Some(&target)) = (parts.first(), parts.get(3)) else {
        return step(model, vec![]);
    };
    let cur = current_of(&model, facts, monitor);
    let count = model.config.count();
    let wrap = model.config.wrap;
    let next = match target {
        "last" => count - 1,
        "left" | "up" => ws::step(cur, -1, count, wrap),
        "right" | "down" => ws::step(cur, 1, count, wrap),
        n => match n.parse::<i64>() {
            Ok(n) => n - 1,
            Err(_) => return step(model, vec![]),
        },
    };
    if !(0..count).contains(&next) {
        return step(model, vec![]);
    }
    if action == "move" {
        let Some(id) = window.filter(|id| model.vw.contains_key(id)) else {
            return step(model, vec![]);
        };
        let mut model = model;
        model.vw.insert(id, next);
        switch(model, facts, monitor, next, Some(id))
    } else if next != cur {
        switch(model, facts, monitor, next, None)
    } else {
        step(model, vec![])
    }
}

/// The keys nero-workspaces takes over from GNOME's workspace switcher.
/// `overview` = the key also works in the overview.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyDef {
    pub name: String,
    pub overview: bool,
}

pub fn keys() -> Vec<KeyDef> {
    let k = |name: String, overview| KeyDef { name, overview };
    let mut out = vec![];
    for t in ["left", "right", "up", "down", "last"] {
        out.push(k(format!("switch-to-workspace-{t}"), true));
    }
    for t in ["left", "right", "up", "down"] {
        out.push(k(format!("move-to-workspace-{t}"), true));
    }
    for i in 1..=MAX_COUNT {
        out.push(k(format!("switch-to-workspace-{i}"), true));
    }
    for i in 1..=MAX_COUNT {
        out.push(k(format!("move-to-workspace-{i}"), false));
    }
    out.push(k("move-to-workspace-last".into(), false));
    out
}

/// GNOME made another real workspace the active one (a parked window was
/// activated, or the overview was used). That workspace is shown on the
/// window's own monitor, and the visible workspace becomes active again.
pub fn active_changed(model: Model, facts: &Facts, active: i64) -> Step {
    if active == VISIBLE {
        return step(model, vec![]);
    }
    let owner = facts
        .focus
        .filter(|id| model.vw.contains_key(id))
        .and_then(|id| facts.win(id))
        .filter(|w| w.real == Some(active));
    let monitor = owner.map_or(facts.current_monitor, |w| w.monitor);
    let target = ws::adopt(
        active,
        current_of(&model, facts, monitor),
        model.config.count(),
    );
    let mut out = switch(model, facts, monitor, target, owner.map(|w| w.id));
    out.effects.insert(0, Effect::ShowVisible);
    out
}

// ---- scroll and swipe ---------------------------------------------------------

/// One scroll event on the panel of `monitor`.
pub fn panel_scroll(mut model: Model, facts: &Facts, monitor: i64, ev: &ScrollEvent) -> Step {
    let state = model.scroll.get(&monitor).cloned().unwrap_or(ScrollState {
        acc: 0.0,
        last_step_ms: -1e9,
        last_event_ms: -1e9,
    });
    let res = ws::scroll(
        &state,
        ev,
        model.config.scroll_threshold,
        model.config.scroll_cooldown_ms,
    );
    model.scroll.insert(monitor, res.state);
    if res.step == 0 {
        return step(model, vec![]);
    }
    step_monitor(model, facts, monitor, res.step)
}

/// Dash to Panel rebuilt or dropped its panels: their scroll states go.
pub fn forget_panels(mut model: Model) -> Model {
    model.scroll.clear();
    model
}

/// One actor between the scrolled actor and the stage, for [`ignore_scroll`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorInfo {
    /// Its JS class name.
    pub name: String,
    /// Dash to Panel marked it `_dtpIgnoreScroll`.
    #[serde(default)]
    pub dtp_ignore: bool,
}

/// Scroll over an app icon that cycles windows, or over Dash to Panel's own
/// workspace indicator, is not ours. `icon_action` = Dash to Panel's
/// `scroll-icon-action`.
pub fn ignore_scroll(chain: &[ActorInfo], icon_action: &str) -> bool {
    let icon_ours = matches!(icon_action, "PASS_THROUGH" | "NOTHING");
    chain.iter().any(|a| {
        a.dtp_ignore || a.name == "WorkspaceIndicator" || (a.name.contains("AppIcon") && !icon_ours)
    })
}

/// What JS needs to start a swipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwipeBegin {
    pub model: Model,
    /// The monitor the swipe is on (the pointer's when GNOME gave none).
    pub monitor: i64,
    pub points: Vec<f64>,
    pub current: i64,
}

pub fn swipe_begin(mut model: Model, facts: &Facts, monitor: i64) -> SwipeBegin {
    let monitor = if monitor < 0 {
        facts.current_monitor
    } else {
        monitor
    };
    let current = current_of(&model, facts, monitor);
    model.swipe_monitor = Some(monitor);
    let points = tp::swipe_points(model.config.count());
    SwipeBegin {
        model,
        monitor,
        points,
        current,
    }
}

pub fn swipe_end(mut model: Model, facts: &Facts, end_progress: f64) -> Step {
    let Some(monitor) = model.swipe_monitor.take() else {
        return step(model, vec![]);
    };
    let target = tp::swipe_target(end_progress, model.config.count());
    if target == current_of(&model, facts, monitor) {
        return step(model, vec![]);
    }
    switch(model, facts, monitor, target, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: u64, monitor: i64, real: i64) -> WinFact {
        WinFact {
            id,
            monitor,
            real: Some(real),
            on_all: false,
            minimized: false,
            stack: id as i64,
        }
    }

    fn facts(windows: Vec<WinFact>) -> Facts {
        Facts {
            monitors: vec!["DP-1".into(), "HDMI-2".into()],
            windows,
            focus: None,
            focus_monitor: None,
            current_monitor: 0,
        }
    }

    /// Apply the moves to the facts, like GNOME would.
    fn apply(f: &mut Facts, effects: &[Effect]) {
        for e in effects {
            if let Effect::Move { id, real } = e {
                f.windows.iter_mut().find(|w| w.id == *id).unwrap().real = Some(*real);
            }
        }
    }

    fn tracked(f: &Facts) -> Model {
        let mut m = init(None, Config::default());
        for w in &f.windows {
            m = track(m, f, w.id, false).model;
        }
        m
    }

    fn reals(f: &Facts) -> Vec<(u64, i64)> {
        f.windows.iter().map(|w| (w.id, w.real.unwrap())).collect()
    }

    #[test]
    fn init_drops_bad_entries() {
        let m = init(
            Some(
                r#"{"version":1,"current":{"DP-1":2,"HDMI-2":12,"X":-1,"Y":1.5,"Z":"3","W":3.0}}"#,
            ),
            Config::default(),
        );
        assert_eq!(
            m.current,
            BTreeMap::from([("DP-1".into(), 2), ("W".into(), 3)])
        );
        assert!(
            init(
                Some(r#"{"version":2,"current":{"DP-1":2}}"#),
                Config::default()
            )
            .current
            .is_empty()
        );
        assert!(init(Some("not json"), Config::default()).current.is_empty());
        assert!(
            init(Some(r#"{"version":1,"current":null}"#), Config::default())
                .current
                .is_empty()
        );
    }

    #[test]
    fn start_sets_the_number_of_workspaces() {
        let c = Config {
            count: 40,
            ..Config::default()
        };
        assert_eq!(
            start(None, c).effects,
            vec![Effect::NumWorkspaces { n: 13 }]
        );
    }

    #[test]
    fn state_text_round_trips() {
        let mut m = init(None, Config::default());
        m.current.insert("DP-1".into(), 2);
        let t = state_text(&m);
        assert_eq!(
            t,
            "{\n \"current\": {\n  \"DP-1\": 2\n },\n \"version\": 1\n}\n"
        );
        assert_eq!(init(Some(&t), Config::default()).current, m.current);
    }

    #[test]
    fn give_back_resets_what_is_wrong() {
        let g = give_back(&saved_gnome(&GnomeSettings {
            dynamic: true,
            only_on_primary: false,
            num: 4,
        }));
        assert_eq!(
            g,
            GiveBack {
                dynamic: Some(true),
                only_on_primary: Some(false),
                num: Some(4),
                damaged: false
            }
        );
        let g = give_back(r#"{"dynamic":"yes","num":99}"#);
        assert_eq!(
            (g.dynamic, g.only_on_primary, g.num, g.damaged),
            (None, None, None, false)
        );
        assert!(give_back("{oops").damaged);
    }

    #[test]
    fn switch_moves_one_monitor_and_back() {
        // two windows on DP-1, one on HDMI-2, all visible
        let mut f = facts(vec![fact(1, 0, 0), fact(2, 0, 0), fact(3, 1, 0)]);
        let m = tracked(&f);
        let out = switch(m, &f, 0, 1, None);
        apply(&mut f, &out.effects);
        assert_eq!(reals(&f), vec![(1, 1), (2, 1), (3, 0)]);
        assert_eq!(out.model.current["DP-1"], 1);
        assert!(out.effects.contains(&Effect::Save));
        assert!(out.effects.contains(&Effect::Osd {
            monitor: 0,
            text: "2 / 4".into()
        }));
        let out = switch(out.model, &f, 0, 0, None);
        apply(&mut f, &out.effects);
        assert_eq!(reals(&f), vec![(1, 0), (2, 0), (3, 0)]);
    }

    #[test]
    fn switch_raises_the_top_window_that_came_in() {
        let mut f = facts(vec![fact(1, 0, 0), fact(2, 0, 0), fact(3, 0, 0)]);
        let m = tracked(&f);
        // send 2 and 3 to workspace 1 with the move key
        let mut f2 = f.clone();
        f2.focus = Some(2);
        f2.focus_monitor = Some(0);
        let out = key(m, &f2, "move-to-workspace-2", 0, Some(2));
        assert!(out.effects.contains(&Effect::Focus { id: 2 }));
        apply(&mut f, &out.effects);
        // that switched DP-1 to 1: only 2 is visible there, 1 and 3 are parked
        assert_eq!(reals(&f), vec![(1, 1), (2, 0), (3, 1)]);
        assert_eq!(out.model.vw[&3], 0, "3 was not moved by the key");
        assert!(reconcile(out.model.clone(), &f).effects.is_empty());
        // back to 0 with focus on DP-1: 1 and 3 come in, 3 (on top) is raised
        f.focus = Some(2);
        f.focus_monitor = Some(0);
        let out = key(out.model, &f, "switch-to-workspace-left", 0, Some(2));
        assert!(out.effects.contains(&Effect::Raise { id: 3 }));
    }

    #[test]
    fn minimized_and_other_monitor_are_not_raised() {
        let mut f = facts(vec![fact(1, 0, 1), fact(2, 1, 0)]);
        f.windows[0].minimized = true;
        let mut m = tracked(&f);
        m.current.insert("DP-1".into(), 1);
        f.focus_monitor = Some(0);
        let out = switch(m, &f, 0, 0, None);
        assert!(
            !out.effects
                .iter()
                .any(|e| matches!(e, Effect::Raise { .. }))
        );
    }

    #[test]
    fn on_all_windows_are_left_alone() {
        let mut f = facts(vec![fact(1, 0, 0)]);
        f.windows[0].on_all = true;
        let m = tracked(&f);
        let out = switch(m, &f, 0, 2, None);
        assert!(!out.effects.iter().any(|e| matches!(e, Effect::Move { .. })));
        assert!(place(out.model, &f, 1).effects.is_empty());
    }

    #[test]
    fn fresh_window_goes_to_what_its_monitor_shows() {
        let mut f = facts(vec![fact(1, 0, 0)]);
        let mut m = tracked(&f);
        m.current.insert("DP-1".into(), 2);
        // a new window on DP-1, opened on the visible workspace: joins 2, stays visible
        f.windows.push(fact(2, 0, 0));
        let out = track(m, &f, 2, true);
        assert_eq!(out.model.vw[&2], 2);
        assert!(out.effects.is_empty());
        // tracking twice changes nothing
        assert_eq!(track(out.model.clone(), &f, 2, true).model, out.model);
    }

    #[test]
    fn moved_by_someone_else_is_adopted_then_placed() {
        let f = facts(vec![fact(1, 0, 3)]);
        let m = tracked(&f);
        assert_eq!(m.vw[&1], 2);
        let mut f2 = f.clone();
        f2.windows[0].real = Some(2);
        let out = window_moved(m, &f2, 1);
        assert_eq!(out.model.vw[&1], 1);
        assert_eq!(out.effects, vec![Effect::PlaceLater { id: 1 }]);
        // monitor DP-1 shows 0: workspace 1 is parked on real 2, already there
        assert!(place(out.model, &f2, 1).effects.is_empty());
    }

    #[test]
    fn entering_a_monitor_visible_joins_its_workspace() {
        let mut f = facts(vec![fact(1, 0, 0)]);
        let mut m = tracked(&f);
        m.current.insert("HDMI-2".into(), 3);
        f.windows[0].monitor = 1;
        let out = entered_monitor(m, &f, 1);
        assert_eq!(out.model.vw[&1], 3);
        f.windows[0].real = Some(2);
        assert_eq!(
            entered_monitor(out.model, &f, 1).effects,
            vec![Effect::PlaceLater { id: 1 }]
        );
    }

    #[test]
    fn count_shrinks_windows_and_monitors() {
        let mut f = facts(vec![fact(1, 0, 4)]);
        let mut m = tracked(&f);
        m.current.insert("DP-1".into(), 3);
        assert_eq!(m.vw[&1], 3);
        let out = set_config(
            m,
            &f,
            Config {
                count: 2,
                ..Config::default()
            },
        );
        assert_eq!(out.model.vw[&1], 1);
        assert_eq!(out.model.current["DP-1"], 1);
        apply(&mut f, &out.effects);
        assert_eq!(reals(&f), vec![(1, 0)]);
        assert!(out.effects.contains(&Effect::NumWorkspaces { n: 3 }));
        // only other settings: nothing to do
        let c = Config {
            count: 2,
            wrap: true,
            ..Config::default()
        };
        assert!(set_config(out.model, &f, c).effects.is_empty());
    }

    #[test]
    fn keys_parse() {
        let f = facts(vec![fact(1, 0, 0)]);
        let m = tracked(&f);
        let cur = |s: &Step| s.model.current.get("DP-1").copied();
        assert_eq!(
            cur(&key(m.clone(), &f, "switch-to-workspace-last", 0, None)),
            Some(3)
        );
        assert_eq!(
            cur(&key(m.clone(), &f, "switch-to-workspace-3", 0, None)),
            Some(2)
        );
        assert_eq!(
            cur(&key(m.clone(), &f, "switch-to-workspace-left", 0, None)),
            None
        );
        assert_eq!(
            cur(&key(m.clone(), &f, "switch-to-workspace-9", 0, None)),
            None
        );
        assert_eq!(
            cur(&key(m.clone(), &f, "move-to-workspace-2", 0, None)),
            None
        );
        assert_eq!(cur(&key(m, &f, "bogus", 0, None)), None);
        let k = keys();
        assert_eq!(k.len(), 5 + 4 + 12 + 12 + 1);
        assert!(
            !k.iter()
                .find(|k| k.name == "move-to-workspace-1")
                .unwrap()
                .overview
        );
    }

    #[test]
    fn activating_a_parked_window_shows_its_workspace() {
        let mut f = facts(vec![fact(1, 0, 0), fact(2, 1, 3)]);
        let m = tracked(&f);
        f.focus = Some(2);
        f.focus_monitor = Some(1);
        let out = active_changed(m, &f, 3);
        assert_eq!(out.effects[0], Effect::ShowVisible);
        assert_eq!(out.model.current["HDMI-2"], 2);
        assert!(out.effects.contains(&Effect::Move { id: 2, real: 0 }));
        assert!(out.effects.contains(&Effect::Focus { id: 2 }));
        assert!(
            !out.effects
                .iter()
                .any(|e| matches!(e, Effect::Move { id: 1, .. }))
        );
        assert!(active_changed(out.model, &f, VISIBLE).effects.is_empty());
    }

    #[test]
    fn panel_scroll_steps_its_own_monitor() {
        let f = facts(vec![fact(1, 0, 0), fact(2, 1, 0)]);
        let mut m = tracked(&f);
        let ev = |t: f64| ScrollEvent {
            delta: 1.0,
            discrete: true,
            now_ms: t,
            finished: false,
        };
        m = panel_scroll(m, &f, 1, &ev(0.0)).model;
        assert_eq!(m.current.get("HDMI-2"), Some(&1));
        assert_eq!(m.current.get("DP-1"), None);
        // inside the cooldown: no step
        m = panel_scroll(m, &f, 1, &ev(100.0)).model;
        assert_eq!(m.current.get("HDMI-2"), Some(&1));
        assert!(forget_panels(m).scroll.is_empty());
    }

    #[test]
    fn swipe_switches_where_it_started() {
        let mut f = facts(vec![fact(1, 0, 0)]);
        f.current_monitor = 1;
        let m = tracked(&f);
        let b = swipe_begin(m, &f, -1);
        assert_eq!((b.monitor, b.current, b.points.len()), (1, 0, 4));
        let out = swipe_end(b.model, &f, 2.4);
        assert_eq!(out.model.current["HDMI-2"], 2);
        assert_eq!(out.model.swipe_monitor, None);
        assert!(swipe_end(out.model, &f, 1.0).effects.is_empty());
    }

    #[test]
    fn scroll_sources() {
        let a = |n: &str| ActorInfo {
            name: n.into(),
            dtp_ignore: false,
        };
        assert!(ignore_scroll(
            &[a("StWidget"), a("TaskbarAppIcon")],
            "CYCLE_WINDOWS"
        ));
        assert!(!ignore_scroll(&[a("TaskbarAppIcon")], "PASS_THROUGH"));
        assert!(ignore_scroll(&[a("WorkspaceIndicator")], "NOTHING"));
        assert!(ignore_scroll(
            &[ActorInfo {
                name: "X".into(),
                dtp_ignore: true
            }],
            "NOTHING"
        ));
        assert!(!ignore_scroll(&[a("Panel")], "CYCLE_WINDOWS"));
    }

    #[test]
    fn disable_brings_every_window_back() {
        let f = facts(vec![fact(1, 0, 0), fact(2, 0, 3), fact(3, 1, 2)]);
        let m = tracked(&f);
        assert_eq!(
            disable(&m, &f),
            vec![
                Effect::Move { id: 2, real: 0 },
                Effect::Move { id: 3, real: 0 }
            ]
        );
    }

    #[test]
    fn model_survives_json() {
        let f = facts(vec![fact(4294967297, 0, 2)]);
        let mut m = tracked(&f);
        m.scroll.insert(1, ScrollState::default());
        let text = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Model>(&text).unwrap(), m);
    }
}
