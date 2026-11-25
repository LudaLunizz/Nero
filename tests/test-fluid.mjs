// gjs -m tests/test-fluid.mjs
import { fluidShape } from '../build/tilingshell@ferrarodomenico.com/utils/freeSpace.js';
let fails = 0;
const eq = (name, got, want) => {
  const ok = JSON.stringify(got) === JSON.stringify(want);
  if (!ok) fails++;
  print(`${ok ? 'ok  ' : 'FAIL'} ${name}: ${JSON.stringify(got)}${ok ? '' : ' want ' + JSON.stringify(want)}`);
};
const area = { x: 0, y: 0, width: 1000, height: 600 };
// layout: two columns, right one split in two rows
const tiles = [
  { x: 0, y: 0, width: 500, height: 600 },
  { x: 500, y: 0, width: 500, height: 300 },
  { x: 500, y: 300, width: 500, height: 300 },
];
eq('empty screen: tile under pointer', fluidShape(tiles, [], area, { x: 700, y: 100 }), tiles[1]);
eq('empty screen: left tile', fluidShape(tiles, [], area, { x: 100, y: 100 }), tiles[0]);
eq('pointer over a window: null', fluidShape(tiles, [{ x: 0, y: 0, width: 500, height: 600 }], area, { x: 100, y: 100 }), null);
// left window shrunk to 400 wide: right column free space grows 100 px left
const shrunk = [{ x: 0, y: 0, width: 400, height: 600 }];
eq('two free tiles: top one stretches to the open edge', fluidShape(tiles, shrunk, area, { x: 700, y: 100 }), { x: 400, y: 0, width: 600, height: 300 });
eq('two free tiles: bottom one', fluidShape(tiles, shrunk, area, { x: 700, y: 500 }), { x: 400, y: 300, width: 600, height: 300 });
eq('in the gap next to the window', fluidShape(tiles, shrunk, area, { x: 450, y: 100 }), { x: 400, y: 0, width: 600, height: 300 });
// one free tile with an odd-sized hole: shape = the hole
const busy = [{ x: 0, y: 0, width: 520, height: 600 }, { x: 500, y: 0, width: 500, height: 260 }];
eq('one open space: shape fills it', fluidShape(tiles, busy, area, { x: 800, y: 400 }), { x: 520, y: 260, width: 480, height: 340 });
eq('sliver too thin: null', fluidShape(tiles, [{ x: 0, y: 0, width: 990, height: 600 }], area, { x: 995, y: 300 }), null);
// L-shaped space: window in the top-right tile only
const top = [{ x: 500, y: 0, width: 500, height: 300 }];
eq('L space, left tile', fluidShape(tiles, top, area, { x: 100, y: 100 }), tiles[0]);
eq('L space, bottom-right tile', fluidShape(tiles, top, area, { x: 700, y: 500 }), tiles[2]);
print(fails ? `${fails} failed` : 'all passed');
