// node test-adjacent.mjs : checks isAdjacent from the live resizeManager.js
import { readFileSync } from "fs";
import assert from "assert";
const src = readFileSync(new URL("../build/tilingshell@ferrarodomenico.com/components/tilingsystem/resizeManager.js", import.meta.url), "utf8");
const [isAdjacent, isCollinear] = new Function(src.match(/function isAdjacent[\s\S]*?\n}\n/)[0] + src.match(/function isCollinear[\s\S]*?\n}\n/)[0] + "return [isAdjacent, isCollinear];")();
const R = (x, y, width, height) => ({ x, y, width, height });
const left = R(0, 1631, 360, 929), right = R(317, 1681, 806, 879);
assert(isAdjacent(left, right, "right", 10), "small overlap (43 px) links");
assert(isAdjacent(right, left, "left", 10), "and the other way");
assert(isAdjacent(R(0, 0, 300, 500), R(305, 0, 300, 500), "right", 10), "gap within reach links");
assert(!isAdjacent(R(0, 0, 300, 500), R(320, 0, 300, 500), "right", 10), "gap beyond reach does not");
assert(!isAdjacent(R(0, 0, 300, 500), R(100, 0, 300, 500), "right", 10), "big overlap (a layer) does not");
assert(!isAdjacent(R(0, 0, 300, 500), R(300, 600, 300, 500), "right", 10), "no shared length does not");
assert(isAdjacent(R(0, 0, 500, 300), R(0, 290, 500, 300), "bottom", 10), "bottom overlap links");
assert(isAdjacent(R(0, 290, 500, 300), R(0, 0, 500, 300), "top", 10), "top overlap links");
assert(!isAdjacent(R(0, 0, 500, 300), R(0, 0, 500, 300), "bottom", 10), "same rect does not");
// the screenshot case: three side by side, tops on one line
const L = R(0, 215, 465, 600), M = R(465, 215, 1195, 600), Rt = R(1660, 215, 600, 600);
assert(isCollinear(M, L, "top", 10) && isCollinear(M, Rt, "top", 10), "side windows share the top line");
assert(!isCollinear(M, R(0, 400, 465, 400), "top", 10), "top edge elsewhere does not");
assert(!isCollinear(M, R(0, 1000, 465, 300), "top", 10), "window not beside does not");
assert(!isCollinear(M, R(2400, 215, 300, 600), "top", 10), "same line but not touching does not");
console.log("isAdjacent + isCollinear: all ok");
