// gjs -m tests/test-ws-model.mjs : nero-workspaces' state machine (core/src/ws_model.rs)
// through the built wasm, the way extension.js uses it: the model goes out and comes
// back as plain JSON on every call, effects are carried out on fake windows.
// Needs core/build.sh to have run.
import { call } from "../nero-workspaces/nero-workspaces@LudaLunizz/nero.js";

const assert = (c, m) => { if (!c) throw new Error(m); };
const eq = (a, b, m) => assert(JSON.stringify(a) === JSON.stringify(b), `${m}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`);
const config = { count: 4, wrap: false, showOsd: true, scrollThreshold: 1, scrollCooldownMs: 250 };

// Fake GNOME: windows by id, with monitor and real workspace.
const wins = new Map();
const facts = (focus = null) => ({
  monitors: ["DP-1", "HDMI-2"],
  windows: [...wins.values()].map((w, i) => ({ ...w, onAll: false, minimized: false, stack: i })),
  focus,
  focusMonitor: focus === null ? null : wins.get(focus).monitor,
  currentMonitor: 0,
});
let model;
const run = (out) => {
  model = JSON.parse(JSON.stringify(out.model)); // only plain JSON is kept between calls
  for (const e of out.effects) if (e.do === "move") wins.get(e.id).real = e.real;
  return out.effects;
};
const reals = () => [...wins.values()].map((w) => `${w.id}:${w.real}`).join(" ");

// start: a damaged entry in the state file is dropped, GNOME gets count + 1 workspaces
const start = call("wsInit", '{"version":1,"current":{"DP-1":1,"HDMI-2":99}}', config);
eq(start.effects, [{ do: "numWorkspaces", n: 5 }], "start effects");
eq(start.model.current, { "DP-1": 1 }, "saved state checked");
model = start.model;
eq(call("wsInit", null, config).model.current, {}, "no state file");

// windows, one with an id above 2^32 like Mutter's own ids
const big = 4294967297;
for (const [id, monitor] of [[1, 0], [2, 0], [big, 1]]) {
  wins.set(id, { id, monitor, real: 0 });
  run(call("wsTrack", model, facts(), id, false));
}
eq(model.vw, { 1: 1, 2: 1, [big]: 0 }, "visible windows join what their monitor shows");
eq(run(call("wsReconcile", model, facts())), [], "nothing to move at the start");

// switch DP-1 to 0: its two windows are parked on real 2, HDMI-2 untouched
const fx = run(call("wsSwitch", model, facts(), 0, 0, null));
eq(reals(), `1:2 2:2 ${big}:0`, "switch parks one monitor");
assert(fx.some((e) => e.do === "osd" && e.text === "1 / 4"), "osd shown");
assert(fx.some((e) => e.do === "save"), "save asked");

// a key for the HDMI-2 window: move it to workspace 3, focus follows it
const kx = run(call("wsKey", model, facts(big), "move-to-workspace-3", 1, big));
eq(model.vw[big], 2, "moved window belongs to 3");
eq(model.current["HDMI-2"], 2, "its monitor shows 3");
assert(kx.some((e) => e.do === "focus" && e.id === big), "focus follows");
eq(reals(), `1:2 2:2 ${big}:0`, "the moved window stays visible");

// GNOME activates real workspace 2 (window 1 clicked in the taskbar)
const ax = run(call("wsActiveChanged", model, facts(1), 2));
eq(ax[0], { do: "showVisible" }, "back to the visible workspace first");
eq(model.current["DP-1"], 1, "DP-1 shows the clicked window's workspace");
eq(reals(), `1:0 2:0 ${big}:0`, "both DP-1 windows back");

// wheel on the HDMI-2 panel: one notch, one step
run(call("wsPanelScroll", model, facts(), 1, { delta: 1, discrete: true, nowMs: 1000, finished: false }));
eq(model.current["HDMI-2"], 3, "panel scroll steps its monitor");
model = call("wsForgetPanels", model);
eq(model.scroll, {}, "panel states forgotten");

// scroll over an app icon is the taskbar's
assert(call("wsIgnoreScroll", [{ name: "TaskbarAppIcon", dtpIgnore: false }], "CYCLE_WINDOWS"), "app icon ignored");
assert(!call("wsIgnoreScroll", [{ name: "Panel", dtpIgnore: false }], "CYCLE_WINDOWS"), "panel is ours");

// state file round trip, untrack, disable
const text = call("wsStateText", model);
eq(call("wsInit", text, config).model.current, model.current, "state file round trip");
model = call("wsUntrack", model, 2);
assert(!("2" in model.vw), "untracked");
wins.delete(2);
eq(call("wsDisable", model, facts()).map((e) => e.id), [big], "disable brings parked windows back");

// GNOME's own settings
const back = call("wsGiveBack", call("wsSavedGnome", { dynamic: true, onlyOnPrimary: false, num: 4 }));
eq(back, { dynamic: true, onlyOnPrimary: false, num: 4, damaged: false }, "gnome settings round trip");
eq(call("wsKeys").length, 34, "keys");
print("ws model: all passed");
