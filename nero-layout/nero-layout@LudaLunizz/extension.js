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
// All decisions and all state are Rust (Nero's core/src/layout_model.rs), run as
// WebAssembly through nero.js. This file reads GNOME's windows, files and apps for
// the core, and carries out what it gets back. The core's model is kept here
// between calls without being read.
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {ExtensionState} from 'resource:///org/gnome/shell/misc/extensionUtils.js';
import {call, plainRect} from './nero.js';
import {makeLog, connectorOf, Registry} from './helpers.js';

// nero-workspaces sets up the fixed workspaces the saved workspace numbers refer to.
const WORKSPACES_UUID = 'nero-workspaces@LudaLunizz';
// How long a login restore waits for nero-workspaces at most, and how long after it
// is active (GNOME applies its workspace settings a moment later).
const WORKSPACES_WAIT_MS = 10000;
const WORKSPACES_SETTLE_MS = 500;
// Set once per GNOME Shell process: a re-enable (after the lock screen, or by hand)
// is not a login and must not restore or start apps again.
const DONE_FLAG = '__neroLayoutLoginDone';
const NO_RECT = {x: 0, y: 0, width: 0, height: 0};

const stateFile = () => Gio.File.new_for_path(GLib.build_filenamev([GLib.get_user_state_dir(), 'nero', 'layout.json']));

const log = makeLog('nero-layout');

// A text file's contents, or null.
function readText(path) {
    try {
        return new TextDecoder().decode(GLib.file_get_contents(path)[1]);
    } catch {
        return null;
    }
}

const isMaximized = w => w.get_maximized ? w.get_maximized() === Meta.MaximizeFlags.BOTH
    : w.maximizedHorizontally && w.maximizedVertically;
const anyMaximized = w => w.get_maximized ? w.get_maximized() !== 0
    : w.maximizedHorizontally || w.maximizedVertically;
const maximize = w => w.get_maximized ? w.maximize(Meta.MaximizeFlags.BOTH) : w.maximize();
const unmaximize = w => w.get_maximized ? w.unmaximize(Meta.MaximizeFlags.BOTH) : w.unmaximize();

// A stable id per window for the core (kept on the window object).
let nextId = 1;
const idOf = w => (w._neroLayoutId ??= nextId++);
const windowById = id => global.display.list_all_windows().find(w => w._neroLayoutId === id) ?? null;
const live = w => !!w.get_compositor_private();

// The desktop file id of a window's app (null for apps without a desktop file).
function appIdOf(w) {
    const app = Shell.WindowTracker.get_default().get_window_app(w);
    return app && !app.is_window_backed() ? app.get_id() : null;
}

// A window as the core's snapshot and claim rules want it.
function snapWin(w) {
    const monitor = w.get_monitor();
    return {
        app: appIdOf(w),
        normal: w.get_window_type() === Meta.WindowType.NORMAL,
        skipTaskbar: w.is_skip_taskbar(),
        transient: !!w.get_transient_for(),
        title: w.get_title() ?? '',
        monitor,
        connector: monitor >= 0 ? connectorOf(monitor) : null,
        frame: plainRect(w.get_frame_rect()),
        workArea: monitor >= 0 ? plainRect(w.get_work_area_for_monitor(monitor)) : NO_RECT,
        onAll: w.is_on_all_workspaces(),
        workspace: w.get_workspace()?.index() ?? null,
        maximized: isMaximized(w),
        fullscreen: w.is_fullscreen(),
        minimized: w.minimized,
        above: w.is_above(),
    };
}

// The session's autostart entries (desktop files in the autostart folders); the
// core decides what they mean.
function autostartEntries() {
    const out = [];
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
                let exec = null;
                try {
                    exec = GLib.shell_parse_argv(get('Exec') ?? '')[1]?.[0] ?? null;
                } catch {
                    // no or unparsable Exec line
                }
                out.push({name, hidden: get('Hidden') === 'true', enabled: get('X-GNOME-Autostart-enabled') !== 'false', exec});
            } catch {
                // unreadable entry: ignore
            }
        }
    }
    return out;
}

export default class NeroLayout extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._reg = new Registry(); // everything below that lives as long as the extension
        this._restoreReg = null; // the window-created hook of a running restore
        this._wait = null; // a Registry while waiting for nero-workspaces
        const file = this._loadText();
        const start = call('lyStart', file);
        this._model = start.model; // the core's state (core/src/layout_model.rs Model), opaque here

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
            this._startRestore(this._loadText(), true);
        });
        this._reg.connect(this._settings, 'changed::save-interval', () => this._startSaveTimer());
        this._startSaveTimer();

        const atLogin = call('lyAtLogin', !!globalThis[DONE_FLAG], !!Main.layoutManager._startingUp,
            readText('/proc/self/stat'), readText('/proc/uptime'));
        globalThis[DONE_FLAG] = true;
        if (atLogin && start.hasSaved) {
            // keep the layout of the last session apart, in case the new one is not wanted
            try {
                stateFile().copy(stateFile().get_parent().get_child('layout.prev.json'), Gio.FileCopyFlags.OVERWRITE, null, null);
            } catch (e) {
                log(`no copy of the last layout: ${e.message}`);
            }
            this._whenWorkspacesReady(() => this._startRestore(file, false));
        }
    }

    disable() {
        this._stopWaiting();
        this._endRestore(null);
        this._reg.destroy();
        this._reg = null;
        this._saveTimer = 0;
        this._model = null;
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

    _snapOpts() {
        return {
            skip: this._settings.get_strv('skip-apps'),
            onlyOnPrimary: Meta.prefs_get_workspaces_only_on_primary(),
            primary: Main.layoutManager.primaryIndex,
        };
    }

    // force: write now, also during a restore and also when windows were closed.
    _save(force) {
        let out;
        try {
            const wins = global.display.sort_windows_by_stacking(global.display.list_all_windows()); // bottom first
            out = call('lySave', this._model, wins.map(snapWin), this._snapOpts(), force, new Date().toISOString());
        } catch (e) {
            log(`snapshot failed: ${e.message}`);
            return;
        }
        if (out.write === null) {
            this._model = out.model;
            return;
        }
        try {
            const file = stateFile();
            GLib.mkdir_with_parents(file.get_parent().get_path(), 0o700);
            file.replace_contents(new TextEncoder().encode(out.write), null, false,
                Gio.FileCreateFlags.PRIVATE | Gio.FileCreateFlags.REPLACE_DESTINATION, null);
            this._model = out.model; // only once it is on disk
        } catch (e) {
            log(`save failed: ${e.message}`);
        }
    }

    _loadText() {
        return readText(stateFile().get_path());
    }

    // ---- restoring ----

    // manual: restore-now in a running session (windows already open are placed again).
    _startRestore(file, manual) {
        this._restoreReg?.destroy();
        this._restoreReg = null;
        const start = call('lyRestoreStart', this._model, file, this._settings.get_strv('skip-apps'), manual);
        this._model = start.model;
        if (start.run === null)
            return;
        const run = start.run;
        log(`restoring ${start.count} windows (saved ${start.savedAt})`);

        for (const w of global.display.sort_windows_by_stacking(global.display.list_all_windows()))
            this._claim(w);
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
            this._later(this._settings.get_uint('launch-delay') * 1000, () => this._launchMissing(run));
        this._later(this._settings.get_uint('restore-seconds') * 1000, () => this._endRestore(run));
    }

    // run: only if that restore is still the current one; null: whatever runs.
    _endRestore(run) {
        if (!this._model)
            return;
        const out = call('lyRestoreEnd', this._model, run);
        this._model = out.model;
        if (out.ended) {
            // also the windows that have not drawn their first frame yet: nothing to wait for any more
            this._restoreReg?.destroy();
            this._restoreReg = null;
        }
    }

    _claim(w) {
        if (!this._model)
            return;
        let out;
        try {
            out = call('lyClaim', this._model, {id: idOf(w), ...snapWin(w), live: live(w)});
        } catch (e) {
            log(`match failed: ${e.message}`);
            return;
        }
        this._model = out.model;
        if (out.run === null)
            return;
        const id = idOf(w);
        this._place(w, null);
        this._restack(id);
        // slow clients (XWayland, Electron) may still resize or raise themselves: once more
        // (the core does nothing if a newer restore has started meanwhile)
        const run = out.run;
        this._later(1200, () => {
            this._place(w, run);
            this._restack(id);
        });
    }

    // The saved file is in stacking order (bottom first), but windows open in
    // whatever order their apps start and each new one lands on top. The core says
    // which windows to raise, bottom first.
    _restack(id) {
        if (!this._model)
            return;
        const wins = global.display.list_all_windows().filter(w => w._neroLayoutId !== undefined)
            .map(w => ({id: w._neroLayoutId, monitor: w.get_monitor(), live: live(w), minimized: w.minimized}));
        let order;
        try {
            order = call('lyRestack', this._model, wins, id);
        } catch (e) {
            log(`restack failed: ${e.message}`);
            return;
        }
        for (const i of order)
            windowById(i)?.raise();
    }

    _monitors() {
        const ws = global.workspace_manager.get_active_workspace();
        return Array.from({length: global.display.get_n_monitors()}, (_, i) => ({
            connector: connectorOf(i),
            workArea: plainRect(ws.get_work_area_for_monitor(i)),
        }));
    }

    _place(w, run) {
        if (!this._model)
            return;
        const facts = {
            window: {
                id: idOf(w),
                live: live(w),
                onAll: w.is_on_all_workspaces(),
                workspace: w.get_workspace()?.index() ?? null,
                monitor: w.get_monitor(),
                frame: plainRect(w.get_frame_rect()),
                maximized: isMaximized(w),
                anyMaximized: anyMaximized(w),
                fullscreen: w.is_fullscreen(),
                minimized: w.minimized,
                above: w.is_above(),
            },
            nWorkspaces: global.workspace_manager.get_n_workspaces(),
            monitors: this._monitors(),
        };
        try {
            for (const a of call('lyPlace', this._model, facts, run)) {
                switch (a.do) {
                case 'stick': w.stick(); break;
                case 'workspace': w.change_workspace_by_index(a.index, false); break;
                case 'unfullscreen': w.unmake_fullscreen(); break;
                case 'unmaximize': unmaximize(w); break;
                case 'toMonitor': w.move_to_monitor(a.monitor); break;
                case 'moveResize': w.move_resize_frame(false, a.rect.x, a.rect.y, a.rect.width, a.rect.height); break;
                case 'maximize': maximize(w); break;
                case 'fullscreen': w.make_fullscreen(); break;
                case 'above': w.make_above(); break;
                case 'minimize': w.minimize(); break;
                default: log(`unknown action ${a.do}`);
                }
            }
        } catch (err) {
            log(`placing ${w.get_title()} failed: ${err.message}`);
        }
    }

    // Start each saved app that still has no window and is not running, once.
    _launchMissing(run) {
        if (!this._model)
            return;
        const appSys = Shell.AppSystem.get_default();
        const apps = [];
        for (const id of call('lyMissingApps', this._model)) {
            const app = appSys.lookup_app(id);
            if (app)
                apps.push({id, stopped: app.get_state() === Shell.AppState.STOPPED, executable: app.get_app_info()?.get_executable() ?? null});
        }
        const plan = call('lyLaunchPlan', this._model, run, apps, autostartEntries());
        for (const id of plan.autostart)
            log(`not starting ${id}: the session autostarts it`);
        for (const id of plan.start) {
            try {
                appSys.lookup_app(id).open_new_window(-1);
                log(`started ${id}`);
            } catch (e) {
                log(`could not start ${id}: ${e.message}`);
            }
        }
    }
}
