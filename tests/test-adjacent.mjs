// gjs -m tests/test-adjacent.mjs : isAdjacent + isCollinear (Rust, through freeSpace.js)
import { isAdjacent, isCollinear } from "../build/tilingshell@ferrarodomenico.com/utils/freeSpace.js";
const assert = (c, m) => { if (!c) throw new Error(m); };
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
print("isAdjacent + isCollinear: all ok");
