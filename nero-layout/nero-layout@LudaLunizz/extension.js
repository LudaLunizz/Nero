// Nero Layout: remembers where every app window is and puts the windows back after
// the next login.
//
// Saved per window: app (desktop file id), title, monitor (connector name), position
// and size as a share of that monitor's work area, workspace, maximized, full screen,
// minimized, always on top; the list is in stacking order. After a login, each new
// window of a saved app takes the saved window of that app with the closest title and
// is put on its spot and back in its saved stacking order; apps that
// have no window and are not running are started once. What is inside a window (tabs,
// files) is the app's own business: many apps reopen it themselves.
//
// The maths (spot <-> pixels) and the matching are Rust (Nero's core/src/layout.rs),
// run as WebAssembly through nero.js.
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {ExtensionState} from 'resource:///org/gnome/shell/misc/extensionUtils.js';
import {call, plainRect} from './nero.js';
import {makeLog, connectorOf, monitorForConnector, Registry} from './helpers.js';

const FILE_VERSION = 1;
// nero-workspaces sets up the fixed workspaces the saved workspace numbers refer to.
const WORKSPACES_UUID = 'nero-workspaces@LudaLunizz';
// How long a login restore waits for nero-workspaces at most, and how long after it
// is active (GNOME applies its workspace settings a moment later).
const WORKSPACES_WAIT_MS = 10000;
const WORKSPACES_SETTLE_MS = 500;
// Set once per GNOME Shell process: a re-enable (after the lock screen, or by hand)
// is not a login and must not restore or start apps again.
const DONE_FLAG = '__neroLayoutLoginDone';

const stateFile = () => Gio.File.new_for_path(GLib.build_filenamev([GLib.get_user_state_dir(), 'nero', 'layout.json']));

const log = makeLog('nero-layout');

// Seconds since this GNOME Shell process started (Infinity if /proc cannot tell).
function shellUptime() {
    try {
        const dec = new TextDecoder();
        const stat = dec.decode(GLib.file_get_contents('/proc/self/stat')[1]);
        const startTicks = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19]);
        const up = Number(dec.decode(GLib.file_get_contents('/proc/uptime')[1]).split(' ')[0]);
        const s = up - startTicks / 100;
        return Number.isFinite(s) ? s : Infinity;
    } catch {
        return Infinity;
    }
}

const isMaximized = w => w.get_maximized ? w.get_maximized() === Meta.MaximizeFlags.BOTH
    : w.maximizedHorizontally && w.maximizedVertically;
const anyMaximized = w => w.get_maximized ? w.get_maximized() !== 0
    : w.maximizedHorizontally || w.maximizedVertically;
const maximize = w => w.get_maximized ? w.maximize(Meta.MaximizeFlags.BOTH) : w.maximize();
const unmaximize = w => w.get_maximized ? w.unmaximize(Meta.MaximizeFlags.BOTH) : w.unmaximize();

// The desktop file id of a window's app, or null for windows that are not saved
// (dialogs, helper windows, apps without a desktop file).
function appIdOf(w) {
    if (w.get_window_type() !== Meta.WindowType.NORMAL || w.is_skip_taskbar() || w.get_transient_for())
        return null;
    const app = Shell.WindowTracker.get_default().get_window_app(w);
    if (!app || app.is_window_backed())
        return null;
    return app.get_id();
}

// Names (desktop file names and program names) of the session's autostart entries.
// Those apps are started by the session itself; GNOME calls an app "stopped" until it
// shows a window, so starting it here as well could run it twice.
function autostartNames() {
    const names = new Set();
    const base = p => GLib.path_get_basename(p ?? '');
    const dirs = [GLib.get_user_config_dir(), ...GLib.get_system_config_dirs()].map(d => GLib.build_filenamev([d, 'autostart']));
    for (const dir of dirs) {
        let en;
        try {
            en = Gio.File.new_for_path(dir).enumerate_children('standard::name', Gio.FileQueryInfoFlags.NONE, null);
        } catch {
            continue;
        }
        for (let info; (info = en.next_file(null));) {
            const name = info.get_name();
            if (!name.endsWith('.desktop'))
                continue;
            try {
                const kf = new GLib.KeyFile();
                kf.load_from_file(GLib.build_filenamev([dir, name]), GLib.KeyFileFlags.NONE);
                const get = k => {
                    try {
                        return kf.get_string('Desktop Entry', k);
                    } catch {
                        return null;
                    }
                };
                if (get('Hidden') === 'true' || get('X-GNOME-Autostart-enabled') === 'false')
                    continue;
                names.add(name);
                const exec = GLib.shell_parse_argv(get('Exec') ?? '')[1]?.[0];
                if (exec)
                    names.add(base(exec));
            } catch {
                // unreadable entry: ignore
            }
        }
    }
    return names;
}

// One saved window from layout.json with only well-formed fields, or null when it
// has no app. The file can be edited by hand or damaged: one wrong field (a title
// that is not text, a spot that is not four numbers) must not make the core
// reject the whole list, which would leave every window unrestored.
function cleanEntry(e) {
    if (!e || typeof e !== 'object' || typeof e.app !== 'string' || e.app === '')
        return null;
    const num = v => typeof v === 'number' && Number.isFinite(v);
    const s = e.spot;
    return {
        app: e.app,
        title: typeof e.title === 'string' ? e.title : '',
        connector: typeof e.connector === 'string' ? e.connector : null,
        spot: s && typeof s === 'object' && num(s.fx) && num(s.fy) && num(s.fw) && num(s.fh)
            ? {fx: s.fx, fy: s.fy, fw: s.fw, fh: s.fh} : null,
        workspace: Number.isInteger(e.workspace) && e.workspace >= 0 ? e.workspace : -1,
        sticky: e.sticky === true,
        maximized: e.maximized === true,
        fullscreen: e.fullscreen === true,
        minimized: e.minimized === true,
        above: e.above === true,
    };
}

export default class NeroLayout extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._reg = new Registry(); // everything below that lives as long as the extension
        this._restoreReg = null; // the window-created hook of a running restore
        this._wait = null; // a Registry while waiting for nero-workspaces
        this._restoring = false;
        this._pendingShrink = null;
        const saved = this._load();
        this._lastWritten = saved ? JSON.stringify(saved.windows) : null;

        this._reg.connect(this._settings, 'changed::save-now', () => {
            if (!this._settings.get_boolean('save-now'))
                return;
            this._settings.set_boolean('save-now', false);
            this._save(true);
        });
        this._reg.connect(this._settings, 'changed::restore-now', () => {
            if (!this._settings.get_boolean('restore-now'))
                return;
            this._settings.set_boolean('restore-now', false);
            this._startRestore(this._load(), true);
        });
        this._reg.connect(this._settings, 'changed::save-interval', () => this._startSaveTimer());
        this._startSaveTimer();

        const atLogin = !globalThis[DONE_FLAG] && (Main.layoutManager._startingUp || shellUptime() < 120);
        globalThis[DONE_FLAG] = true;
        if (atLogin && saved) {
            // keep the layout of the last session apart, in case the new one is not wanted
            try {
                stateFile().copy(stateFile().get_parent().get_child('layout.prev.json'), Gio.FileCopyFlags.OVERWRITE, null, null);
            } catch (e) {
                log(`no copy of the last layout: ${e.message}`);
            }
            this._whenWorkspacesReady(() => this._startRestore(saved, false));
        }
    }

    disable() {
        this._stopWaiting();
        this._endRestore();
        this._reg.destroy();
        this._reg = null;
        this._saveTimer = 0;
        this._settings = null;
    }

    _later(ms, fn) {
        this._reg?.timeout(ms, fn); // nothing once disabled
    }

    // At login the saved workspace numbers only fit once nero-workspaces has fixed the
    // number of workspaces; restoring before that would pile windows on the last one.
    // Waits for it when it is enabled (at most 10 s), else runs fn at once.
    _whenWorkspacesReady(fn) {
        const em = Main.extensionManager;
        const pending = () => {
            if (global.settings.get_boolean('disable-user-extensions') ||
                !global.settings.get_strv('enabled-extensions').includes(WORKSPACES_UUID))
                return false;
            const state = em.lookup(WORKSPACES_UUID)?.state;
            return state === undefined || state === ExtensionState.INITIALIZED || state === ExtensionState.ACTIVATING;
        };
        if (!pending()) {
            fn();
            return;
        }
        const go = why => {
            this._stopWaiting();
            log(why);
            this._later(WORKSPACES_SETTLE_MS, fn);
        };
        const wait = this._wait = new Registry();
        wait.connect(em, 'extension-state-changed', (_m, ext) => {
            if (ext.uuid === WORKSPACES_UUID && !pending())
                go('nero-workspaces is ready: restoring');
        });
        wait.timeout(WORKSPACES_WAIT_MS, () => go('nero-workspaces did not come up in time: restoring anyway'));
    }

    _stopWaiting() {
        this._wait?.destroy();
        this._wait = null;
    }

    // ---- saving ----

    _startSaveTimer() {
        this._reg.remove(this._saveTimer);
        this._saveTimer = this._reg.timeoutSeconds(this._settings.get_uint('save-interval'), () => {
            this._save(false);
            return GLib.SOURCE_CONTINUE;
        }, GLib.PRIORITY_LOW);
    }

    _snapshot() {
        const skip = this._settings.get_strv('skip-apps');
        const wins = global.display.sort_windows_by_stacking(global.display.list_all_windows()); // bottom first
        const out = [];
        for (const w of wins) {
            const app = appIdOf(w);
            const monitor = w.get_monitor();
            if (!app || skip.includes(app) || monitor < 0)
                continue;
            // With workspaces on the primary monitor only, windows on the other
            // monitors are on all workspaces anyway: that is not a choice to restore.
            const otherMonitor = Meta.prefs_get_workspaces_only_on_primary() && monitor !== Main.layoutManager.primaryIndex;
            const sticky = w.is_on_all_workspaces() && !otherMonitor;
            out.push({
                app,
                title: w.get_title() ?? '',
                connector: connectorOf(monitor),
                spot: call('toSpot', plainRect(w.get_frame_rect()), plainRect(w.get_work_area_for_monitor(monitor))),
                workspace: sticky || otherMonitor ? -1 : w.get_workspace()?.index() ?? -1,
                sticky,
                maximized: isMaximized(w),
                fullscreen: w.is_fullscreen(),
                minimized: w.minimized,
                above: w.is_above(),
            });
        }
        return out;
    }

    // force: write now, also during a restore and also when windows were closed.
    _save(force) {
        if (this._restoring && !force)
            return;
        let windows;
        try {
            windows = this._snapshot();
        } catch (e) {
            log(`snapshot failed: ${e.message}`);
            return;
        }
        const text = JSON.stringify(windows);
        if (text === this._lastWritten)
            return;
        // Fewer windows than last time: maybe the session is ending and apps are
        // closing. Write it only when it is still the same at the next save.
        const before = this._lastWritten ? JSON.parse(this._lastWritten).length : 0;
        if (!force && windows.length < before && this._pendingShrink !== text) {
            this._pendingShrink = text;
            return;
        }
        this._pendingShrink = null;
        try {
            const file = stateFile();
            GLib.mkdir_with_parents(file.get_parent().get_path(), 0o700);
            const data = JSON.stringify({version: FILE_VERSION, saved: new Date().toISOString(), windows}, null, 1);
            file.replace_contents(new TextEncoder().encode(`${data}\n`), null, false, Gio.FileCreateFlags.PRIVATE | Gio.FileCreateFlags.REPLACE_DESTINATION, null);
            this._lastWritten = text;
        } catch (e) {
            log(`save failed: ${e.message}`);
        }
    }

    _load() {
        try {
            const [, bytes] = stateFile().load_contents(null);
            const data = JSON.parse(new TextDecoder().decode(bytes));
            if (data?.version !== FILE_VERSION || !Array.isArray(data.windows))
                return null;
            return data;
        } catch {
            return null;
        }
    }

    // ---- restoring ----

    // manual: restore-now in a running session (windows already open are placed again).
    _startRestore(data, manual) {
        this._endRestore();
        const skip = this._settings.get_strv('skip-apps');
        const saved = (data?.windows ?? []).map(cleanEntry).filter(e => e && !skip.includes(e.app));
        if (!saved.length)
            return;
        this._restoring = true;
        // Timers of an earlier restore (a restore-now while one runs) must not end
        // or launch apps for this one: they only act while their run is current.
        const run = this._run = (this._run ?? 0) + 1;
        const thisRun = fn => () => {
            if (this._run === run)
                fn();
        };
        this._saved = saved;
        this._used = saved.map(() => false);
        this._placed = [];
        log(`restoring ${saved.length} windows (saved ${data.saved})`);

        for (const w of global.display.sort_windows_by_stacking(global.display.list_all_windows())) {
            if (manual)
                delete w._neroLayoutClaimed;
            this._claim(w);
        }
        const hooks = this._restoreReg = new Registry();
        hooks.connect(global.display, 'window-created', (_d, w) => {
            const actor = w.get_compositor_private();
            if (!actor)
                return;
            const id = hooks.connect(actor, 'first-frame', () => {
                hooks.disconnect(actor, id);
                // after keep-onscreen's fit, and once the title has settled
                this._later(400, () => this._claim(w));
            });
        });
        if (this._settings.get_boolean('launch-apps'))
            this._later(this._settings.get_uint('launch-delay') * 1000, thisRun(() => this._launchMissing()));
        this._later(this._settings.get_uint('restore-seconds') * 1000, thisRun(() => this._endRestore()));
    }

    _endRestore() {
        // also the windows that have not drawn their first frame yet: nothing to wait for any more
        this._restoreReg?.destroy();
        this._restoreReg = null;
        this._restoring = false;
        this._saved = null;
        this._used = null;
        this._placed = null;
    }

    _claim(w) {
        if (!this._restoring || w._neroLayoutClaimed || !w.get_compositor_private())
            return;
        const app = appIdOf(w);
        if (!app)
            return;
        let i;
        try {
            i = call('matchWindow', this._saved.map(e => ({app: e.app, title: e.title ?? ''})), this._used, app, w.get_title() ?? '');
        } catch (e) {
            log(`match failed: ${e.message}`);
            return;
        }
        if (i === null || i === undefined)
            return;
        this._used[i] = true;
        w._neroLayoutClaimed = true;
        this._placed.push({w, index: i});
        const entry = this._saved[i];
        this._place(w, entry);
        this._restack(i);
        // slow clients (XWayland, Electron) may still resize or raise themselves: once more
        // (index i belongs to this run's saved list: a newer restore has its own)
        const run = this._run;
        this._later(1200, () => {
            if (this._run !== run)
                return;
            this._place(w, entry);
            this._restack(i);
        });
    }

    // The saved file is in stacking order (bottom first), but windows open in
    // whatever order their apps start and each new one lands on top. Raise the
    // new window and the saved windows above it on its monitor, bottom first.
    _restack(index) {
        if (!this._placed)
            return;
        const live = this._placed.filter(p => p.w.get_compositor_private() && !p.w.minimized);
        let order;
        try {
            order = call('restackOrder', live.map(p => ({index: p.index, monitor: p.w.get_monitor()})), index);
        } catch (e) {
            log(`restack failed: ${e.message}`);
            return;
        }
        for (const i of order)
            live.find(p => p.index === i)?.w.raise();
    }

    _place(w, e) {
        if (!w.get_compositor_private())
            return; // closed meanwhile
        try {
            const wm = global.workspace_manager;
            if (e.sticky) {
                if (!w.is_on_all_workspaces())
                    w.stick();
            } else if (e.workspace >= 0) {
                // with dynamic workspaces the last one is empty; filling it adds the next
                const idx = Math.min(e.workspace, wm.get_n_workspaces() - 1);
                if (w.get_workspace()?.index() !== idx)
                    w.change_workspace_by_index(idx, false);
            }
            const m = monitorForConnector(e.connector);
            if (m < 0)
                return; // that monitor is not connected: leave the window where it opened
            if (w.is_fullscreen() && !e.fullscreen)
                w.unmake_fullscreen();
            if (anyMaximized(w) && !e.maximized)
                unmaximize(w);
            if (w.get_monitor() !== m)
                w.move_to_monitor(m);
            if (e.spot) {
                const r = call('fromSpot', e.spot, plainRect(w.get_work_area_for_monitor(m)));
                const f = w.get_frame_rect();
                if (!anyMaximized(w) && !w.is_fullscreen() && (r.x !== f.x || r.y !== f.y || r.width !== f.width || r.height !== f.height))
                    w.move_resize_frame(false, r.x, r.y, r.width, r.height);
            }
            if (e.maximized && !isMaximized(w))
                maximize(w);
            if (e.fullscreen && !w.is_fullscreen())
                w.make_fullscreen();
            if (e.above && !w.is_above())
                w.make_above();
            if (e.minimized && !w.minimized)
                w.minimize();
        } catch (err) {
            log(`placing ${e.app} failed: ${err.message}`);
        }
    }

    // Start each saved app that still has no window and is not running, once.
    _launchMissing() {
        if (!this._restoring)
            return;
        const appSys = Shell.AppSystem.get_default();
        const missing = new Set(this._saved.filter((_e, i) => !this._used[i]).map(e => e.app));
        const autostart = autostartNames();
        for (const id of missing) {
            const app = appSys.lookup_app(id);
            if (!app || app.get_state() !== Shell.AppState.STOPPED)
                continue;
            const exe = app.get_app_info()?.get_executable();
            if (autostart.has(id) || (exe && autostart.has(GLib.path_get_basename(exe)))) {
                log(`not starting ${id}: the session autostarts it`);
                continue;
            }
            try {
                app.open_new_window(-1);
                log(`started ${id}`);
            } catch (e) {
                log(`could not start ${id}: ${e.message}`);
            }
        }
    }
}
