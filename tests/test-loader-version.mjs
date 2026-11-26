// gjs -m tests/test-loader-version.mjs : nero.js refuses a wasm of another build.
// Needs core/build.sh to have run (apply.sh does it): the wasm is taken from an extension directory.
import GLib from "gi://GLib";
import Gio from "gi://Gio";

const assert = (c, m) => { if (!c) throw new Error(m); };
const here = Gio.File.new_for_uri(import.meta.url).get_parent().get_path();
const root = GLib.build_filenamev([here, ".."]);
const read = (p) => GLib.file_get_contents(p)[1];
const dec = new TextDecoder();
const cargo = dec.decode(read(GLib.build_filenamev([root, "core", "Cargo.toml"])));
const version = /^version = "([^"]+)"/m.exec(cargo)[1];
const wasm = read(GLib.build_filenamev([root, "nero-workspaces", "nero-workspaces@LudaLunizz", "nero_core.wasm"]));
const loaderSrc = dec.decode(read(GLib.build_filenamev([root, "core", "js", "nero.js"])));
assert(loaderSrc.includes('"@NERO_CORE_VERSION@"'), "core/js/nero.js carries the stamp placeholder");

const dirs = [];
// A fresh directory (so a fresh module) holding a nero.js with the given stamp and the real wasm.
async function loaderWith(stamp) {
  const dir = GLib.dir_make_tmp("nero-loader-XXXXXX");
  dirs.push(dir);
  GLib.file_set_contents(GLib.build_filenamev([dir, "nero_core.wasm"]), wasm);
  GLib.file_set_contents(GLib.build_filenamev([dir, "nero.js"]), loaderSrc.replace("@NERO_CORE_VERSION@", stamp));
  return import(Gio.File.new_for_path(GLib.build_filenamev([dir, "nero.js"])).get_uri());
}
const message = (fn) => { try { fn(); } catch (e) { return String(e.message); } return null; };

const good = await loaderWith(version);
assert(good.call("version") === version, "a matching loader runs and reports the core version");

const wrong = await loaderWith("9.9.9");
for (let i = 0; i < 2; i++) {  // the bad instance is not kept: the second call fails the same way
  const m = message(() => wrong.call("version"));
  assert(m && m.includes(`version ${version}`) && m.includes("9.9.9"), `a stamped-wrong loader refuses (call ${i + 1}): ${m}`);
}

const raw = await loaderWith("@NERO_CORE_VERSION@");
const m = message(() => raw.call("version"));
assert(m && m.includes("not stamped"), `an unstamped nero.js refuses: ${m}`);
for (const d of dirs) {
  for (const f of ["nero.js", "nero_core.wasm"]) GLib.unlink(GLib.build_filenamev([d, f]));
  GLib.rmdir(d);
}
print("loader version ok");
