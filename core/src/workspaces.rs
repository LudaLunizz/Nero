//! Workspaces per monitor. GNOME switches workspaces on all monitors at once, so
//! nero-workspaces keeps GNOME on one real workspace (index 0, the one on screen)
//! and gives every monitor its own row of *virtual* workspaces. A window that is
//! on its monitor's current virtual workspace sits on real workspace 0; any other
//! window is parked on real workspace `vw + 1`, out of sight. Switching one
//! monitor only moves that monitor's windows, the other monitors stay as they are.
//!
//! Also here: turning wheel and touchpad scroll events into workspace steps (one
//! step per notch, with a short pause so a flick or a fast wheel cannot race
//! through all workspaces).

use serde::{Deserialize, Serialize};

/// The real workspace that is on screen.
pub const VISIBLE: i64 = 0;

/// A window as JS sees it: its monitor, its virtual workspace and the real
/// workspace it is on now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Win {
    pub id: u64,
    pub monitor: i64,
    pub vw: i64,
    pub real: i64,
}

/// One `change_workspace` call for JS: put window `id` on real workspace `real`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub id: u64,
    pub real: i64,
}

/// The real workspace a window with virtual workspace `vw` belongs on while its
/// monitor shows `current`.
pub fn real_for(vw: i64, current: i64) -> i64 {
    if vw == current { VISIBLE } else { vw + 1 }
}

/// The moves that make `monitor` show virtual workspace `target`: its windows on
/// `target` come to the visible workspace, its other windows are parked. Windows
/// on other monitors are never touched. Windows already in place are left out.
pub fn plan_switch(windows: &[Win], monitor: i64, target: i64) -> Vec<Move> {
    windows
        .iter()
        .filter(|w| w.monitor == monitor)
        .filter_map(|w| {
            let real = real_for(w.vw, target);
            (real != w.real).then_some(Move { id: w.id, real })
        })
        .collect()
}

/// A window's virtual workspace when it was put on real workspace `real` by
/// someone else (an app, the layout saver, GNOME): on the visible workspace it
/// joins what its monitor shows now, on a parked one it belongs to that one.
/// `count` is the number of virtual workspaces; out of range = the last one.
pub fn adopt(real: i64, current: i64, count: i64) -> i64 {
    let count = count.max(1);
    if real <= VISIBLE {
        current.clamp(0, count - 1)
    } else {
        (real - 1).clamp(0, count - 1)
    }
}

/// The virtual workspace `delta` steps from `current`; with `wrap` it goes round,
/// else it stops at the first and the last one.
pub fn step(current: i64, delta: i64, count: i64, wrap: bool) -> i64 {
    let count = count.max(1);
    let next = current + delta;
    if wrap {
        next.rem_euclid(count)
    } else {
        next.clamp(0, count - 1)
    }
}

/// Scroll state of one panel, kept by JS between events.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollState {
    /// Smooth scroll collected since the last step (notches; + = down/right).
    pub acc: f64,
    /// Time of the last step (ms), or of the last event while pausing.
    pub last_step_ms: f64,
    /// Time of the last event (ms).
    pub last_event_ms: f64,
}

/// One scroll event. `delta` is the smooth delta (dy, or dx when dy is 0) or,
/// for a discrete event, -1 (up/left) or +1 (down/right). `finished` = the
/// touchpad said the scroll ended (fingers lifted).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollEvent {
    pub delta: f64,
    pub discrete: bool,
    pub now_ms: f64,
    #[serde(default)]
    pub finished: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollResult {
    pub state: ScrollState,
    /// -1, 0 or +1 workspace.
    pub step: i64,
}

/// Smooth scroll that stopped for this long starts counting from zero again.
const IDLE_RESET_MS: f64 = 400.0;

/// One event in, at most one step out. A step needs a full notch (`threshold`,
/// 1.0 = one wheel click) of smooth scroll in one direction, or one discrete
/// event, and at least `cooldown_ms` since the previous step. Scroll during the
/// cooldown is thrown away, so the momentum of a fast wheel or a touchpad flick
/// does not carry on into the next workspaces.
pub fn scroll(
    state: &ScrollState,
    ev: &ScrollEvent,
    threshold: f64,
    cooldown_ms: f64,
) -> ScrollResult {
    let mut s = state.clone();
    let threshold = if threshold > 0.0 { threshold } else { 1.0 };
    if ev.now_ms - s.last_event_ms > IDLE_RESET_MS {
        s.acc = 0.0;
    }
    s.last_event_ms = ev.now_ms;
    let cooling = ev.now_ms - s.last_step_ms < cooldown_ms;
    let mut step = 0;
    if cooling {
        s.acc = 0.0;
    } else if ev.discrete {
        step = ev.delta.signum() as i64;
        s.acc = 0.0;
    } else {
        // A change of direction drops what was collected the other way.
        if s.acc != 0.0 && ev.delta != 0.0 && s.acc.signum() != ev.delta.signum() {
            s.acc = 0.0;
        }
        s.acc += ev.delta;
        if s.acc.abs() >= threshold {
            step = s.acc.signum() as i64;
            s.acc = 0.0;
        }
    }
    if step != 0 {
        s.last_step_ms = ev.now_ms;
    }
    if ev.finished {
        s.acc = 0.0;
    }
    ScrollResult { state: s, step }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u64, monitor: i64, vw: i64, real: i64) -> Win {
        Win {
            id,
            monitor,
            vw,
            real,
        }
    }

    #[test]
    fn switch_moves_only_that_monitor() {
        // Monitor 0 shows 0, monitor 1 shows 2.
        let wins = [
            w(1, 0, 0, 0),
            w(2, 0, 1, 2),
            w(3, 0, 2, 3),
            w(4, 1, 2, 0),
            w(5, 1, 0, 1),
        ];
        let moves = plan_switch(&wins, 0, 1);
        assert_eq!(
            moves,
            vec![Move { id: 1, real: 1 }, Move { id: 2, real: 0 }]
        );
        assert!(plan_switch(&wins, 1, 2).is_empty());
        assert_eq!(
            plan_switch(&wins, 1, 0),
            vec![Move { id: 4, real: 3 }, Move { id: 5, real: 0 }]
        );
    }

    #[test]
    fn switch_to_same_workspace_is_nothing() {
        let wins = [w(1, 0, 0, 0), w(2, 0, 3, 4)];
        assert!(plan_switch(&wins, 0, 0).is_empty());
    }

    #[test]
    fn switch_repairs_misplaced_windows() {
        // A window of virtual 1 that ended up visible while monitor shows 0.
        let wins = [w(1, 0, 1, 0)];
        assert_eq!(plan_switch(&wins, 0, 0), vec![Move { id: 1, real: 2 }]);
    }

    #[test]
    fn adopt_and_real_for_round_trip() {
        for count in 1..6 {
            for cur in 0..count {
                for vw in 0..count {
                    let real = real_for(vw, cur);
                    assert_eq!(adopt(real, cur, count), vw, "vw {vw} cur {cur}");
                }
            }
        }
        // Parked further than there are workspaces: the last one.
        assert_eq!(adopt(9, 0, 4), 3);
        assert_eq!(adopt(-1, 2, 4), 2);
    }

    #[test]
    fn step_clamps_or_wraps() {
        assert_eq!(step(0, -1, 4, false), 0);
        assert_eq!(step(3, 1, 4, false), 3);
        assert_eq!(step(1, 1, 4, false), 2);
        assert_eq!(step(0, -1, 4, true), 3);
        assert_eq!(step(3, 1, 4, true), 0);
        assert_eq!(step(0, 1, 0, true), 0);
    }

    fn ev(delta: f64, discrete: bool, now_ms: f64) -> ScrollEvent {
        ScrollEvent {
            delta,
            discrete,
            now_ms,
            finished: false,
        }
    }

    fn run(events: &[ScrollEvent], cooldown: f64) -> Vec<i64> {
        let mut s = ScrollState {
            last_step_ms: -1e9,
            last_event_ms: -1e9,
            ..Default::default()
        };
        let mut out = vec![];
        for e in events {
            let r = scroll(&s, e, 1.0, cooldown);
            s = r.state;
            if r.step != 0 {
                out.push(r.step);
            }
        }
        out
    }

    #[test]
    fn hires_wheel_notch_is_one_step() {
        // A high-resolution wheel: one notch = 8 events of 1/8.
        let evs: Vec<_> = (0..8).map(|i| ev(0.125, false, i as f64 * 5.0)).collect();
        assert_eq!(run(&evs, 250.0), vec![1]);
        // Half a notch: nothing.
        assert!(run(&evs[..4], 250.0).is_empty());
    }

    #[test]
    fn fast_wheel_waits_for_cooldown() {
        // 5 notches in 100 ms: one step; then after the pause another one.
        let mut evs: Vec<_> = (0..5).map(|i| ev(1.0, false, i as f64 * 20.0)).collect();
        evs.push(ev(1.0, false, 400.0));
        assert_eq!(run(&evs, 250.0), vec![1, 1]);
    }

    #[test]
    fn discrete_events_step_and_respect_cooldown() {
        let evs = [
            ev(-1.0, true, 0.0),
            ev(-1.0, true, 50.0),
            ev(-1.0, true, 300.0),
        ];
        assert_eq!(run(&evs, 250.0), vec![-1, -1]);
        // Cooldown 0: every click is a step (Dash to Panel's delay 0).
        assert_eq!(run(&evs, 0.0), vec![-1, -1, -1]);
    }

    #[test]
    fn touchpad_flick_is_one_step() {
        // A flick: 30 events of 0.4 over 300 ms, then finished.
        let mut evs: Vec<_> = (0..30).map(|i| ev(0.4, false, i as f64 * 10.0)).collect();
        evs.push(ScrollEvent {
            delta: 0.0,
            discrete: false,
            now_ms: 300.0,
            finished: true,
        });
        // 0..250 ms is the cooldown after the first step at 20 ms; the rest
        // (from 270 ms) collects 0.4 * 3 = 1.2: a second step. Steps stay few.
        assert!(run(&evs, 250.0).len() <= 2);
        assert_eq!(run(&evs, 400.0), vec![1]);
    }

    #[test]
    fn direction_change_drops_collected() {
        let evs = [
            ev(0.6, false, 0.0),
            ev(-0.6, false, 10.0),
            ev(-0.6, false, 20.0),
        ];
        assert_eq!(run(&evs, 250.0), vec![-1]);
    }

    #[test]
    fn idle_resets_collected() {
        let evs = [ev(0.6, false, 0.0), ev(0.6, false, 1000.0)];
        assert!(run(&evs, 250.0).is_empty());
    }
}
