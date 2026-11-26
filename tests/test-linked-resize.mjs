// gjs -m tests/test-linked-resize.mjs : Ctrl-linked resize keeps the far edge (Rust, through freeSpace.js)
import { linkedRect, linkedMin, clampEdge } from "../build/tilingshell@ferrarodomenico.com/utils/freeSpace.js";
const assert = (c, m) => { if (!c) throw new Error(m); };
const R = (x, y, width, height) => ({ x, y, width, height });
const eq = (a, b) => a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
// DP-1: left Brave 563..1497, right Brave 1497..1997 at its 500 px minimum
const right = R(1497, 0, 500, 1440), grow = R(0, 0, 100, 0), none = { width: 0, height: 0 };
assert(eq(linkedRect(right, grow, "left", null, "right", null, none), R(1597, 0, 400, 1440)), "plain shrink keeps the right edge");
const min = linkedMin(right, R(1597, 0, 500, 1440), "left", null, none);
assert(min.width === 500, "a pushed far edge teaches the minimum");
assert(eq(linkedRect(right, grow, "left", null, "right", null, min), right), "at its minimum it stays, far edge at 1997");
assert(eq(linkedRect(right, grow, "left", null, "right", null), R(1597, 0, 400, 1440)), "min may be left out");
const start = R(563, 0, 934, 1440);
assert(eq(clampEdge(start, R(563, 0, 1034, 1440), "right", [[right, right]], 10), start), "dragged window goes back to the stuck neighbour");
assert(eq(clampEdge(start, R(563, 0, 1034, 1440), "right", [[right, R(1597, 0, 400, 1440)]], 10), R(563, 0, 1034, 1440)), "a neighbour that followed changes nothing");
print("linked resize: all ok");
