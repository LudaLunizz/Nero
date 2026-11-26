// Nero core loader: runs nero_core.wasm (the Rust code in core/, built by
// core/build.sh and copied next to this file) inside GNOME Shell / gjs.
// call("fitToFreeSpace", tile, occupied, ...) runs the Rust function with that
// JS name and returns its result. Synchronous: no D-Bus, no extra process. A
// Rust panic only becomes a JS exception, it cannot take the shell down.
//
// core/build.sh stamps the core's version into this file and copies it next to
// the wasm it was built with. The first call checks that the two match, so a wasm
// and a loader from different builds fail with a clear message instead of
// "unknown function" in the middle of a drag.
import Gio from "gi://Gio";

// Replaced by core/build.sh with the version of the core it built.
const EXPECTED_CORE_VERSION = "@NERO_CORE_VERSION@";

const enc = new TextEncoder();
const dec = new TextDecoder();
let wasmModule = null;
let exp = null;

// One request to a wasm instance; throws on a trap, returns the parsed answer.
function run(e, fn, args) {
  const req = enc.encode(JSON.stringify({ f: fn, a: args }));
  const ptr = e.nero_alloc(req.length);
  new Uint8Array(e.memory.buffer, ptr, req.length).set(req);
  const outPtr = e.nero_call(ptr, req.length);
  return JSON.parse(dec.decode(new Uint8Array(e.memory.buffer, outPtr, e.nero_out_len())));
}

// Throws if this nero.js and nero_core.wasm are not from the same build.
function checkVersion(e) {
  const what = "nero_core.wasm and nero.js must come from the same core/build.sh run";
  if (EXPECTED_CORE_VERSION.startsWith("@"))
    throw new Error(`nero core: this nero.js was not stamped by core/build.sh (${what})`);
  const res = run(e, "version", []);
  if ("err" in res)
    throw new Error(`nero core: nero_core.wasm does not report a version, it is older than this nero.js ${EXPECTED_CORE_VERSION} (${what})`);
  if (res.ok !== EXPECTED_CORE_VERSION)
    throw new Error(`nero core: nero_core.wasm is version ${res.ok} but this nero.js is ${EXPECTED_CORE_VERSION} (${what})`);
}

function instance() {
  if (exp) return exp;
  if (!wasmModule) {
    const file = Gio.File.new_for_uri(import.meta.url).get_parent().get_child("nero_core.wasm");
    const [, bytes] = file.load_contents(null);
    wasmModule = new WebAssembly.Module(bytes);
  }
  const e = new WebAssembly.Instance(wasmModule, {}).exports;
  checkVersion(e); // a mismatched instance is never kept: every call repeats the error
  exp = e;
  return exp;
}

export function call(fn, ...args) {
  const e = instance();
  let res;
  try {
    res = run(e, fn, args);
  } catch (err) {
    exp = null; // a trap leaves the instance in an unknown state: start a fresh one next time
    throw new Error(`nero core: ${fn} failed: ${err}`);
  }
  if ("err" in res) throw new Error(`nero core: ${fn}: ${res.err}`);
  return res.ok;
}

// Plain {x, y, width, height} from anything rect-like (Mtk.Rectangle too, whose
// fields JSON.stringify cannot see).
export const plainRect = (r) => (r ? { x: r.x, y: r.y, width: r.width, height: r.height } : r);
export const plainRects = (rs) => rs.map(plainRect);
