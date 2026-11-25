// gjs -m tests/test-freeSpace.mjs
import { fitToFreeSpace, isTileVacant, touchingRects, resolveOverlaps, fillGaps, freeTiles } from "../build/tilingshell@ferrarodomenico.com/utils/freeSpace.js";
const assert = { deepEqual(a, b) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(JSON.stringify(a) + " != " + JSON.stringify(b)); } };
const W = { x: 0, y: 0, width: 1440, height: 2560 };
const row = (y, h) => ({ x: 0, y, width: 1440, height: h });
const top = row(0, 640), mid = row(640, 1280), bot = row(1920, 640);
const t = (name, got, want) => { assert.deepEqual(got, want); console.log("ok", name); };
// top window grown to 1000px: middle still vacant, new window starts at 1000
t("vacant when neighbour grew in", isTileVacant(mid, [row(0, 1000)]), true);
t("shrink below grown top", fitToFreeSpace(mid, [row(0, 1000)], W, [bot]), row(1000, 920));
// top shrunk to 400, bottom occupied: middle grows up to 400, stays at bottom window
t("grow into gap", fitToFreeSpace(mid, [row(0, 400), bot], W, []), row(400, 1520));
// bottom tile still empty: middle must not swallow it
t("no grow over empty tile", fitToFreeSpace(mid, [row(0, 640)], W, [bot]), mid);
// double layout, top window grown to 1500: bottom fills 1500..2560
t("double layout", fitToFreeSpace(row(1280, 1280), [row(0, 1500)], W, []), row(1500, 1060));
// dropping on a fully occupied tile keeps the tile
t("occupied keeps tile", fitToFreeSpace(top, [row(0, 700)], W, [mid, bot]), top);
// nothing tiled: top tile stays put
t("empty screen", fitToFreeSpace(top, [], W, [mid, bot]), top);
// untiled window covering most of the tile is ignored (being replaced)
t("covering window ignored", fitToFreeSpace(mid, [row(0, 2000)], W, []), mid);
// terminal above ends at 610 (cell increments): Discord starts at 610
t("snap to short top window", fitToFreeSpace(mid, [row(0, 610)], W, [bot]), row(610, 1310));
t("touching found", touchingRects(row(610, 1310), [row(0, 610), row(1925, 635), { x: 0, y: 0, width: 100, height: 100 }]), [0, 1]);
// window far below behind an empty tile: do not jump over it
t("no jump over empty tile", fitToFreeSpace(top, [row(2200, 360)], W, [mid]), top);
// --- fill the screen ---
// middle of three closed: top and bottom split the gap
t("fill splits gap", fillGaps([top, bot], W), [row(0, 1280), row(1280, 1280)]);
// single window fills screen
t("single fills screen", fillGaps([top], W), [row(0, 2560)]);
// 2-1-2 landscape with middle closed: columns meet at 50%
const L = { x: 0, y: 0, width: 2560, height: 1440 };
const col = (x, y, w, h) => ({ x, y, width: w, height: h });
t("2-1-2 middle closed", fillGaps([col(0,0,563,720), col(0,720,563,720), col(1997,0,563,720), col(1997,720,563,720)], L),
  [col(0,0,1280,720), col(0,720,1280,720), col(1280,0,1280,720), col(1280,720,1280,720)]);
// top-left of 2-1-2 closed: bottom-left grows up, middle untouched
t("2-1-2 corner closed", fillGaps([col(0,720,563,720), col(563,0,1434,1440), col(1997,0,563,720), col(1997,720,563,720)], L),
  [col(0,0,563,1440), col(563,0,1434,1440), col(1997,0,563,720), col(1997,720,563,720)]);
// new window placed in middle of a full-screen window: old one keeps the larger part (tie: top)
t("resolve: newer wins", resolveOverlaps([row(0, 2560), mid], [0, 1], [top, mid, bot]), [row(0, 640), mid]);
// filled top+middle window, new window into middle slot
t("resolve: cut grown window", resolveOverlaps([row(0, 1920), mid, bot], [0, 2, 1]), [row(0, 640), mid, bot]);
// fully covered window drops out
t("resolve: covered drops", resolveOverlaps([row(700, 100), mid], [0, 1]), [null, mid]);
// grown top window (0..1920) does not hide the free middle slot
t("free tiles with grown window", freeTiles([top, mid, bot], [row(0, 1920), bot]).length, 1);
// slot covered by a grown window: do not grow over that window
t("covered slot stays slot", fitToFreeSpace(bot, [top, row(640, 1920)], W, []), bot);
// new window over most of an old one: old one is replaced, not left as a sliver
t("resolve: sliver dropped", resolveOverlaps([row(640, 1480), mid], [0, 1], [top, mid, bot]), [null, mid]);
