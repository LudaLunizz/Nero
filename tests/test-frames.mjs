import { placeInFrames, refillFrames, framesOf } from "../build/tilingshell@ferrarodomenico.com/utils/freeSpace.js";
const eq = (n, a, b) => { if (JSON.stringify(a) !== JSON.stringify(b)) { print("FAIL " + n + " " + JSON.stringify(a) + " != " + JSON.stringify(b)); imports.system.exit(1); } print("ok " + n); };
const row = (y, h) => ({ x: 0, y, width: 1440, height: h });
const T = [row(0, 640), row(640, 1280), row(1920, 640)];
// drop on occupied top, middle free -> top window slides to middle
eq("slide to nearest free", placeInFrames(T, [T[0]], T[0]), [{ i: 0, rect: T[1] }]);
// drop on middle, top+middle occupied, bottom free -> middle goes to bottom, top stays
eq("only the hit window moves", placeInFrames(T, [T[0], T[1]], T[1]), [{ i: 1, rect: T[2] }]);
// all full -> stays underneath (new layer)
eq("full: stack", placeInFrames(T, [T[0], T[1], T[2]], T[1]), []);
// window spanning top+middle, drop on top, bottom occupied -> keeps middle
eq("spanning keeps rest", placeInFrames(T, [row(0, 1920), T[2]], T[0]), [{ i: 0, rect: T[1] }]);
// spanning window, drop on top, bottom free -> moves to nearest free (bottom)... nearest to its centre
eq("spanning to free", placeInFrames(T, [row(0, 1920)], T[0]), [{ i: 0, rect: T[1] }]);
// two layered in middle; top free: topmost of them goes, lower stays? both hit when dropping on middle
eq("two hit, one free", placeInFrames(T, [T[1], T[1]], T[1]), [{ i: 0, rect: T[0] }, { i: 1, rect: T[2] }]);
// refill: two in middle (layer), top frame empty -> lower slides up
eq("refill hidden", refillFrames(T, [T[1], T[1], T[2]]), [{ i: 1, rect: T[0] }]);
eq("refill nothing hidden", refillFrames(T, [T[1]]), []);
// odd-shaped leftover window overlapping target mostly
eq("frameless overlap", placeInFrames(T, [row(0, 300)], T[0]), [{ i: 0, rect: T[1] }]);
eq("framesOf", framesOf(T, row(0, 1920)), [0, 1]);
