// Nero core loader: runs nero_core.wasm (the Rust code in core/, built by
// core/build.sh and copied next to this file) inside GNOME Shell / gjs.
// call("fitToFreeSpace", tile, occupied, ...) runs the Rust function with that
// JS name and returns its result. Synchronous: no D-Bus, no extra process. A
// Rust panic only becomes a JS exception, it cannot take the shell down.
import Gio from "gi://Gio";

const enc = new TextEncoder();
const dec = new TextDecoder();
let wasmModule = null;
let exp = null;

function instance() {
  if (exp) return exp;
  if (!wasmModule) {
    const file = Gio.File.new_for_uri(import.meta.url).get_parent().get_child("nero_core.wasm");
    const [, bytes] = file.load_contents(null);
    wasmModule = new WebAssembly.Module(bytes);
  }
  exp = new WebAssembly.Instance(wasmModule, {}).exports;
  return exp;
}

export function call(fn, ...args) {
  const e = instance();
  const req = enc.encode(JSON.stringify({ f: fn, a: args }));
  let out;
  try {
    const ptr = e.nero_alloc(req.length);
    new Uint8Array(e.memory.buffer, ptr, req.length).set(req);
    const outPtr = e.nero_call(ptr, req.length);
    out = dec.decode(new Uint8Array(e.memory.buffer, outPtr, e.nero_out_len()));
  } catch (err) {
    exp = null; // a trap leaves the instance in an unknown state: start a fresh one next time
    throw new Error(`nero core: ${fn} failed: ${err}`);
  }
  const res = JSON.parse(out);
  if ("err" in res) throw new Error(`nero core: ${fn}: ${res.err}`);
  return res.ok;
}

// Plain {x, y, width, height} from anything rect-like (Mtk.Rectangle too, whose
// fields JSON.stringify cannot see).
export const plainRect = (r) => (r ? { x: r.x, y: r.y, width: r.width, height: r.height } : r);
export const plainRects = (rs) => rs.map(plainRect);
