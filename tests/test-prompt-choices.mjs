// gjs -m tests/test-prompt-choices.mjs : the monitor list of the keep-onscreen settings window.
import { promptChoices } from "../keep-onscreen/nero-keep-onscreen@LudaLunizz/promptChoices.js";

const assert = (c, m) => { if (!c) throw new Error(m); };
const mons = [
  { connector: "DP-1", name: "Dell U2415", width: 1920, height: 1200, x: 1440, y: 0 },
  { connector: "DP-3", name: "", width: 1440, height: 2560, x: 0, y: 0 },
  { connector: "HDMI-2", name: "7inch", width: 1024, height: 600, x: 3360, y: 0 },
];

// off: first entry, then every monitor in connection order, ordered by position in the label
{
  const c = promptChoices(mons, "");
  assert(c.selected === 0 && c.values[0] === "", "off selects the GNOME default");
  assert(c.values.join() === ",DP-1,DP-3,HDMI-2", `values ${c.values}`);
  assert(c.labels[1] === "DP-1 · Dell U2415 · 1920×1200 · 2nd from the left", c.labels[1]);
  assert(c.labels[2] === "DP-3 · 1440×2560 · leftmost", c.labels[2]);
  assert(c.labels[3].endsWith("rightmost"), c.labels[3]);
}
// saved and connected
{
  const c = promptChoices(mons, "HDMI-2");
  assert(c.selected === 3, `selected ${c.selected}`);
}
// saved but unplugged: kept and selected
{
  const c = promptChoices(mons, "HDMI-1");
  assert(c.values.length === 5 && c.selected === 4 && c.labels[4].startsWith("HDMI-1 (not connected"), c.labels[4]);
}
// one monitor: no position word; null connector skipped; null saved = off
{
  const c = promptChoices([mons[0], { connector: null, width: 1, height: 1, x: 0, y: 0 }], null);
  assert(c.values.join() === ",DP-1" && c.labels[1] === "DP-1 · Dell U2415 · 1920×1200", c.labels.join("|"));
  assert(c.selected === 0, "null saved");
}
print("prompt choices: all passed");
