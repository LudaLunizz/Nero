import { findChains, pickChain, toggleChain, isAttached, groupLine } from "../build/tilingshell@ferrarodomenico.com/components/layout/lineGroups.js";
import assert from "node:assert";
const mk = (a) => a.map(([x,y,width,height,groups]) => ({x,y,width,height,groups}));
const L1 = () => mk([[0,0,.22,.5,[1]],[0,.5,.22,.5,[3]],[.22,0,.56,.5,[4,1]],[.22,.5,.56,.5,[5,3]],[.78,0,.22,.5,[4]],[.78,.5,.22,.5,[5]]]);
let t = L1();
const ch = findChains(t);
console.log(ch.map(c=>`${c.vertical?'x':'y'}=${c.c.toFixed(2)} segs=${c.segments.length} att=${isAttached(c)}`));
// horizontal line y=.5 across 3 columns: one chain, 3 pieces, not attached
const h = ch.find(c=>!c.vertical);
assert.equal(h.segments.length,3); assert.equal(isAttached(h),false);
// pointer near y=.5 in the left column
const p = pickChain(t,.1,.51,2560,1440,60); assert.ok(p && !p.vertical);
assert.equal(toggleChain(t,p),"attached");
assert.ok(isAttached(findChains(t).find(c=>!c.vertical)));
const g = t[0].groups.find(x=>t[1].groups.includes(x)); assert.ok(g);
assert.deepEqual(t.filter(u=>u.groups.includes(g)).length,6);
// stock detection would misread; ours must say horizontal line y=.5
const gl = groupLine(t.filter(u=>u.groups.includes(g))); assert.ok(gl && !gl.vertical && Math.abs(gl.c-.5)<.001);
// toggle again -> separated
assert.equal(toggleChain(t,findChains(t).find(c=>!c.vertical)),"separated");
for (const c of findChains(t)) if(!c.vertical) { assert.equal(c.segments.length,3); assert.equal(isAttached(c),false);
  for (const s of c.segments){ const ids=[...s.prev[0].groups].filter(x=>s.next[0].groups.includes(x)); assert.equal(ids.length,1);} }
// vertical x=.22: top (group 1) + bottom (group 3) -> attach, then separate
t = L1();
const v = pickChain(t,.221,.25,2560,1440,60); assert.ok(v.vertical);
console.log("vertical chain pieces", v.segments.length);
assert.equal(toggleChain(t,v),"attached"); assert.ok(isAttached(findChains(t).find(c=>c.vertical&&Math.abs(c.c-.22)<.01)));
assert.equal(toggleChain(t,findChains(t).find(c=>c.vertical&&Math.abs(c.c-.22)<.01)),"separated");
// pick too far -> null
assert.equal(pickChain(L1(),.11,.25,2560,1440,40),null);
// tile count / geometry untouched
assert.equal(t.length,6);
console.log("all ok");
