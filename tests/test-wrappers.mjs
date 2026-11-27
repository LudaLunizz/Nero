// gjs -m tests/test-wrappers.mjs : the JS wrappers of the Tiling Shell patch reach the
// Rust core (WebAssembly) and bring the answer back in the shape the patch expects.
// The cases themselves are cargo tests (core/tests); here is one call per wrapper.
import * as fs from "../build/tilingshell@ferrarodomenico.com/utils/freeSpace.js";
import { findChains, pickChain, toggleChain, isAttached, groupLine } from "../build/tilingshell@ferrarodomenico.com/components/layout/lineGroups.js";

const eq = (n, a, b) => { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${n}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`); };
const R = (x, y, width, height) => ({ x, y, width, height });
const row = (y, h) => R(0, y, 1440, h);
const W = R(0, 0, 1440, 2560);
const T = [row(0, 640), row(640, 1280), row(1920, 640)];
const A = R(0, 0, 2560, 1440);

eq("isTileVacant", fs.isTileVacant(T[1], [row(0, 1000)]), true);
eq("fitToFreeSpace", fs.fitToFreeSpace(T[1], [row(0, 1000)], W, [T[2]]), row(1000, 920));
eq("touchingRects", fs.touchingRects(row(610, 1310), [row(0, 610), R(0, 0, 100, 100)]), [0]);
eq("cutAway", fs.cutAway(R(0, 0, 10, 10), R(0, 0, 10, 10)), null);
eq("resolveOverlaps", fs.resolveOverlaps([row(700, 100), T[1]], [0, 1]), [null, T[1]]);
eq("fillGaps", fs.fillGaps([T[0], T[2]], W), [row(0, 1280), row(1280, 1280)]);
eq("freeTiles", fs.freeTiles(T, [row(0, 1920), T[2]]).length, 1);
eq("framesOf", fs.framesOf(T, row(0, 1920)), [0, 1]);
eq("placeInFrames", fs.placeInFrames(T, [T[0]], T[0]), [{ i: 0, rect: T[1] }]);
eq("refillFrames", fs.refillFrames(T, [T[1], T[1], T[2]]), [{ i: 1, rect: T[0] }]);
eq("growIntoFreeSpace", fs.growIntoFreeSpace(R(800, 300, 600, 400), [], A), A);
eq("fluidShape", fs.fluidShape(T, [], W, { x: 100, y: 100 }), T[0]);
eq("openSpaceAt", typeof fs.openSpaceAt, "function");
eq("isAdjacent", fs.isAdjacent(R(0, 0, 300, 500), R(305, 0, 300, 500), "right", 10), true);
eq("isCollinear", fs.isCollinear(R(465, 215, 1195, 600), R(0, 215, 465, 600), "top", 10), true);
eq("linkedRect", fs.linkedRect(R(1497, 0, 500, 1440), R(0, 0, 100, 0), "left", null, "right", null, { width: 0, height: 0 }), R(1597, 0, 400, 1440));
eq("linkedMin", fs.linkedMin(R(1497, 0, 500, 1440), R(1597, 0, 500, 1440), "left", null, { width: 0, height: 0 }).width, 500);
const start = R(563, 0, 934, 1440), stuck = R(1497, 0, 500, 1440);
eq("clampEdge", fs.clampEdge(start, R(563, 0, 1034, 1440), "right", [[stuck, stuck]], 10), start);

// line groups: tiles are changed in place, chains are plain objects
const mk = (a) => a.map(([x, y, width, height, groups]) => ({ x, y, width, height, groups }));
const t = mk([[0, 0, .22, .5, [1]], [0, .5, .22, .5, [3]], [.22, 0, .56, .5, [4, 1]], [.22, .5, .56, .5, [5, 3]], [.78, 0, .22, .5, [4]], [.78, .5, .22, .5, [5]]]);
const h = findChains(t).find((c) => !c.vertical);
eq("findChains", [h.segments.length, isAttached(h)], [3, false]);
eq("pickChain", pickChain(t, .1, .51, 2560, 1440, 60) !== null, true);
eq("toggleChain", toggleChain(t, pickChain(t, .1, .51, 2560, 1440, 60)), "attached");
const g = t[0].groups.find((x) => t[1].groups.includes(x));
const gl = groupLine(t.filter((u) => u.groups.includes(g)));
eq("groupLine", [gl.vertical, Math.abs(gl.c - .5) < .001], [false, true]);
print("wrappers: all ok");
