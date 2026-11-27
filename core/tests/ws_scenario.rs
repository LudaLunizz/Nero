// nero-workspaces' state machine (src/ws_model.rs) through the JSON boundary, the
// way extension.js uses it: the model goes out and comes back as plain JSON on
// every call, effects are carried out on fake windows. (This was
// tests/test-ws-model.mjs, which ran the built wasm under gjs.)
use nero_core::abi;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn call(f: &str, a: Value) -> Value {
    let out = abi::call(json!({ "f": f, "a": a }).to_string().as_bytes());
    let v: Value = serde_json::from_str(&out).unwrap();
    match v.get("ok") {
        Some(ok) => ok.clone(),
        None => panic!("{f}: {out}"),
    }
}

struct Fake {
    wins: BTreeMap<u64, (i64, i64)>, // id -> (monitor, real workspace)
    model: Value,
}

impl Fake {
    fn facts(&self, focus: Option<u64>) -> Value {
        let windows: Vec<Value> = self
            .wins
            .iter()
            .enumerate()
            .map(|(i, (id, (monitor, real)))| {
                json!({"id": id, "monitor": monitor, "real": real, "onAll": false, "minimized": false, "stack": i})
            })
            .collect();
        json!({
            "monitors": ["DP-1", "HDMI-2"],
            "windows": windows,
            "focus": focus,
            "focusMonitor": focus.map(|f| self.wins[&f].0),
            "currentMonitor": 0,
        })
    }
    /// Keep the model (as JSON only) and carry out the "move" effects.
    fn run(&mut self, out: Value) -> Vec<Value> {
        self.model = out["model"].clone();
        let effects = out["effects"].as_array().unwrap().clone();
        for e in &effects {
            if e["do"] == "move" {
                self.wins.get_mut(&e["id"].as_u64().unwrap()).unwrap().1 =
                    e["real"].as_i64().unwrap();
            }
        }
        effects
    }
    fn reals(&self) -> String {
        self.wins
            .iter()
            .map(|(id, (_, real))| format!("{id}:{real}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[test]
fn a_session_through_the_json_boundary() {
    let config = json!({"count": 4, "wrap": false, "showOsd": true, "scrollThreshold": 1, "scrollCooldownMs": 250});

    // start: a damaged entry in the state file is dropped, GNOME gets count + 1 workspaces
    let start = call(
        "wsInit",
        json!([r#"{"version":1,"current":{"DP-1":1,"HDMI-2":99}}"#, config]),
    );
    assert_eq!(start["effects"], json!([{"do": "numWorkspaces", "n": 5}]));
    assert_eq!(start["model"]["current"], json!({"DP-1": 1}));
    assert_eq!(
        call("wsInit", json!([null, config]))["model"]["current"],
        json!({})
    );
    let mut g = Fake {
        wins: BTreeMap::new(),
        model: start["model"].clone(),
    };

    // windows, one with an id above 2^32 like Mutter's own ids
    let big: u64 = 4_294_967_297;
    for (id, monitor) in [(1u64, 0i64), (2, 0), (big, 1)] {
        g.wins.insert(id, (monitor, 0));
        let out = call("wsTrack", json!([g.model, g.facts(None), id, false]));
        g.run(out);
    }
    assert_eq!(
        g.model["vw"],
        json!({"1": 1, "2": 1, big.to_string(): 0}),
        "visible windows join what their monitor shows"
    );
    let out = call("wsReconcile", json!([g.model, g.facts(None)]));
    assert!(g.run(out).is_empty(), "nothing to move at the start");

    // switch DP-1 to 0: its two windows are parked on real 2, HDMI-2 untouched
    let out = call("wsSwitch", json!([g.model, g.facts(None), 0, 0, null]));
    let fx = g.run(out);
    assert_eq!(g.reals(), format!("1:2 2:2 {big}:0"));
    assert!(
        fx.iter().any(|e| e["do"] == "osd" && e["text"] == "1 / 4"),
        "osd shown"
    );
    assert!(fx.iter().any(|e| e["do"] == "save"), "save asked");

    // a key for the HDMI-2 window: move it to workspace 3, focus follows it
    let out = call(
        "wsKey",
        json!([g.model, g.facts(Some(big)), "move-to-workspace-3", 1, big]),
    );
    let kx = g.run(out);
    assert_eq!(g.model["vw"][big.to_string()], 2);
    assert_eq!(g.model["current"]["HDMI-2"], 2);
    assert!(
        kx.iter().any(|e| e["do"] == "focus" && e["id"] == big),
        "focus follows"
    );
    assert_eq!(
        g.reals(),
        format!("1:2 2:2 {big}:0"),
        "the moved window stays visible"
    );

    // GNOME activates real workspace 2 (window 1 clicked in the taskbar)
    let out = call("wsActiveChanged", json!([g.model, g.facts(Some(1)), 2]));
    let ax = g.run(out);
    assert_eq!(
        ax[0],
        json!({"do": "showVisible"}),
        "back to the visible workspace first"
    );
    assert_eq!(g.model["current"]["DP-1"], 1);
    assert_eq!(
        g.reals(),
        format!("1:0 2:0 {big}:0"),
        "both DP-1 windows back"
    );

    // wheel on the HDMI-2 panel: one notch, one step
    let out = call(
        "wsPanelScroll",
        json!([g.model, g.facts(None), 1, {"delta": 1, "discrete": true, "nowMs": 1000, "finished": false}]),
    );
    g.run(out);
    assert_eq!(
        g.model["current"]["HDMI-2"], 3,
        "panel scroll steps its monitor"
    );
    g.model = call("wsForgetPanels", json!([g.model]));
    assert_eq!(g.model["scroll"], json!({}), "panel states forgotten");

    // scroll over an app icon is the taskbar's
    let icon = json!([{"name": "TaskbarAppIcon", "dtpIgnore": false}]);
    let panel = json!([{"name": "Panel", "dtpIgnore": false}]);
    assert_eq!(
        call("wsIgnoreScroll", json!([icon, "CYCLE_WINDOWS"])),
        json!(true)
    );
    assert_eq!(
        call("wsIgnoreScroll", json!([panel, "CYCLE_WINDOWS"])),
        json!(false)
    );

    // state file round trip, untrack, disable
    let text = call("wsStateText", json!([g.model]));
    assert_eq!(
        call("wsInit", json!([text, config]))["model"]["current"],
        g.model["current"],
        "state file round trip"
    );
    g.model = call("wsUntrack", json!([g.model, 2]));
    assert!(g.model["vw"].get("2").is_none(), "untracked");
    g.wins.remove(&2);
    let ids: Vec<Value> = call("wsDisable", json!([g.model, g.facts(None)]))
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].clone())
        .collect();
    assert_eq!(ids, vec![json!(big)], "disable brings parked windows back");

    // GNOME's own settings
    let saved = call(
        "wsSavedGnome",
        json!([{"dynamic": true, "onlyOnPrimary": false, "num": 4}]),
    );
    assert_eq!(
        call("wsGiveBack", json!([saved])),
        json!({"dynamic": true, "onlyOnPrimary": false, "num": 4, "damaged": false})
    );
    assert_eq!(call("wsKeys", json!([])).as_array().unwrap().len(), 34);
}
