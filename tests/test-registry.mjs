// gjs -m tests/test-registry.mjs : the Registry in core/js/helpers.js (signals, timeouts, laters, destroy).
// Plain gjs is enough: helpers.js only needs GLib at load time.
import GLib from "gi://GLib";
import Gio from "gi://Gio";
import { Registry } from "../core/js/helpers.js";

const assert = (c, m) => { if (!c) throw new Error(m); };
const spin = (ms) => {  // run the main loop for ms
  const end = GLib.get_monotonic_time() + ms * 1000, ctx = GLib.MainContext.default();
  while (GLib.get_monotonic_time() < end) ctx.iteration(false);
};
const action = () => new Gio.SimpleAction({ name: "x" });
const fire = (a) => a.emit("activate", null);

// signals: connect, disconnect one, disconnectObject, destroy, connect after destroy
{
  const a = action(), b = action(); let n = 0, m = 0;
  const r = new Registry();
  const id = r.connect(a, "activate", () => n++);
  r.connect(a, "activate", () => n += 10);
  r.connect(b, "activate", () => m++);
  fire(a); assert(n === 11, "both handlers on a run");
  r.disconnect(a, id); fire(a); assert(n === 21, "disconnect(a, id) removes one handler");
  r.disconnectObject(a); fire(a); fire(b); assert(n === 21 && m === 1, "disconnectObject(a) leaves b alone");
  r.destroy(); fire(b); assert(m === 1, "destroy disconnects the rest");
  assert(r.connect(b, "activate", () => m++) === 0, "connect after destroy returns 0");
  fire(b); assert(m === 1, "...and does not stay connected");
  r.destroy();  // twice is fine
}

// timeouts: one-shot, CONTINUE, remove, destroy, destroy inside a callback
{
  const r = new Registry(); let once = 0, ticks = 0, removed = 0, destroyed = 0, inside = 0;
  r.timeout(5, () => { once++; });
  r.timeout(5, () => (++ticks < 3 ? GLib.SOURCE_CONTINUE : undefined));
  const id = r.timeout(5, () => { removed++; });
  r.remove(id);
  spin(120);
  assert(once === 1, "a timeout without a return value runs once");
  assert(ticks === 3, `SOURCE_CONTINUE repeats until the callback stops (${ticks})`);
  assert(removed === 0, "remove(id) cancels a pending timeout");
  r.remove(id); r.remove(123456);  // unknown ids are ignored
  r.timeout(5, () => { destroyed++; });
  r.destroy(); spin(60);
  assert(destroyed === 0, "destroy cancels pending timeouts");
  assert(r.timeout(5, () => { destroyed++; }) === 0, "timeout after destroy returns 0");

  const r2 = new Registry();
  r2.timeout(5, () => { inside++; r2.destroy(); return GLib.SOURCE_CONTINUE; });
  spin(80);
  assert(inside === 1, `destroy inside a repeating callback stops it (${inside})`);
}

// a callback that throws is logged once and ends
{
  const logged = [], real = globalThis.logError;
  globalThis.logError = (e, p) => logged.push(`${p}: ${e.message}`);
  const r = new Registry({ tag: "t" }); let calls = 0;
  r.timeout(5, () => { calls++; throw new Error("boom"); });
  spin(80);
  globalThis.logError = real;
  assert(calls === 1 && logged.length === 1 && logged[0].includes("[t]") && logged[0].includes("boom"), `thrower: ${calls} ${logged}`);
  r.destroy();
}

// laters: needs the option; runs once; remove/destroy call the given remove
{
  let thrown = false;
  try { new Registry().later(() => {}); } catch { thrown = true; }
  assert(thrown, "later() without the option says so");
  const queue = new Map(), removedIds = []; let next = 1;
  const r = new Registry({ later: { add: (fn) => { queue.set(next, fn); return next++; }, remove: (id) => { removedIds.push(id); queue.delete(id); } } });
  let ran = 0;
  const a = r.later(() => { ran++; }), b = r.later(() => { ran += 10; }), c = r.later(() => { ran += 100; });
  assert(queue.get(a)() === false, "a later returns SOURCE_REMOVE");
  r.remove(a);  // already ran: ignored
  r.remove(b);
  assert(removedIds.join() === String(b), `remove(b) calls the given remove once: ${removedIds}`);
  r.destroy();
  assert(removedIds.join() === `${b},${c}` && ran === 1, `destroy removes the pending later: ${removedIds} ran=${ran}`);
}
print("registry ok");
