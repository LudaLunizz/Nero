// Nero Workspaces: every monitor switches its own workspaces.
//
// GNOME switches workspaces on all monitors at once. Here GNOME stays on one real
// workspace (index 0, the one on screen) and every monitor gets its own row of
// virtual workspaces: a window on its monitor's current virtual workspace sits on
// real workspace 0, any other window is parked on real workspace vw + 1, out of
// sight. Switching a monitor moves only that monitor's windows.
//
// Switching: the scroll wheel (or touchpad) on a Dash to Panel panel switches the
// monitor of that panel; a three-finger swipe switches the monitor under the
// pointer (GNOME's own swipe, which moves all monitors, is switched off); the
// workspace keys (Super+Page Up/Down, Ctrl+Alt+arrows,
// Super+1..) switch the monitor of the focused window. Activating a parked window
// (taskbar, Alt+Tab, a notification) shows its workspace on its own monitor.
//
// All decisions and all state are Rust (Nero's core/src/ws_model.rs), run as
// WebAssembly through nero.js. This file only passes GNOME's events in, with
// the facts the core needs (windows, monitors, focus), and carries out the
// effects it gets back (move a window, focus one, show the OSD, save). The
// core's model is kept here between calls without being read.
//
// Also here: the two-finger scroll speed of the touchpad, which GNOME has no
// setting for. It is a libinput plugin (core/src/scroll_speed.lua) written to
// ~/.config/libinput/plugins, which Mutter loads at login.
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import Clutter from 'gi://Clutter';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as SwipeTracker from 'resource:///org/gnome/shell/ui/swipeTracker.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {call} from './nero.js';
import {makeLog, connectorOf, Registry} from './helpers.js';
import {writeScrollPlugin} from './scrollSpeed.js';

const DTP_UUID = 'dash-to-panel@jderose9.github.com';
const VISIBLE = 0;
const CONFIG_KEYS = ['workspaces-per-monitor', 'wrap-around', 'show-osd', 'scroll-threshold', 'scroll-cooldown-ms'];

const stateFile = () => Gio.File.new_for_path(GLib.build_filenamev([GLib.get_user_state_dir(), 'nero', 'workspaces.json']));

// GNOME 47 moved the "later" queue to global.compositor.get_laters(); 46 has Meta.later_add.
const laterAdd = fn => {
    const laters = global.compositor?.get_laters?.();
    return laters ? laters.add(Meta.LaterType.BEFORE_REDRAW, fn) : Meta.later_add(Meta.LaterType.BEFORE_REDRAW, fn);
};
const laterRemove = id => {
    const laters = global.compositor?.get_laters?.();
    if (laters)
        laters.remove(id);
    else
        Meta.later_remove(id);
};

const log = makeLog('nero-workspaces');

const TRACKED_TYPES = new Set([
    Meta.WindowType.NORMAL, Meta.WindowType.DIALOG, Meta.WindowType.MODAL_DIALOG, Meta.WindowType.UTILITY,
]);

export default class NeroWorkspaces extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._model = null; // the core's state (core/src/ws_model.rs Model), opaque here
        this._ids = new Map(); // tracked Meta.Window -> id for the core
        this._wins = new Map(); // id -> Meta.Window
        this._nextId = 1;
        this._winReg = new Registry({tag: 'nero-workspaces'}); // the signals of every tracked window
        this._panelSignals = [];
        this._reg = new Registry({tag: 'nero-workspaces', later: {add: laterAdd, remove: laterRemove}});
        this._moving = 0;
        this._saveId = 0;
        // a later cancelled by an earlier disable() never cleared it
        this._snapping = false;
        // GNOME Shell does not call disable() when enable() throws: undo what was
        // done so far ourselves, or GNOME's workspace settings, keys and swipe
        // would stay taken over by an extension that is not running.
        try {
            this._enable();
        } catch (e) {
            try {
                this.disable();
            } catch (e2) {
                logError(e2, '[nero-workspaces] undo after a failed enable');
            }
            throw e;
        }
    }

    _enable() {
        // Fails here, before anything is changed, when nero_core.wasm is missing or broken.
        const start = call('wsInit', this._loadStateText(), this._config());
        this._model = start.model;
        this._takeOverMutter();
        this._apply(start); // GNOME's number of workspaces
        this._patchAnimation();
        this._bindKeys();
        this._setupSwipe();
        this._writeScrollPlugin();

        const display = global.display;
        const wm = global.workspace_manager;
        const reg = this._reg;
        reg.connect(display, 'window-created', (_d, w) => this._later(() => {
            // closed before this later ran (a splash screen): its 'unmanaged' is past
            if (global.display.list_all_windows().includes(w))
                this._track(w, true);
        }));
        reg.connect(display, 'window-entered-monitor', (_d, _m, w) => this._onMonitor(w));
        reg.connect(wm, 'active-workspace-changed', () => this._onActiveChanged());
        for (const key of CONFIG_KEYS)
            reg.connect(this._settings, `changed::${key}`, () => this._onConfigChanged());
        reg.connect(this._settings, 'changed::three-finger-swipe', () => this._setupSwipe());
        reg.connect(this._settings, 'changed::take-over-panel-scroll', () => {
            if (!this._settings.get_boolean('take-over-panel-scroll'))
                this._giveBackDtpScroll();
            this._hookPanels();
        });
        reg.connect(this._settings, 'changed::touchpad-scroll-speed', () => this._writeScrollPlugin());
        reg.connect(global.backend.get_monitor_manager(), 'monitors-changed',
            () => this._later(() => this._apply(call('wsReconcile', this._model, this._facts()))));
        reg.connect(Main.extensionManager, 'extension-state-changed', (_m, ext) => {
            if (ext.uuid === DTP_UUID)
                this._hookPanels();
        });
        for (const actor of global.get_window_actors())
            this._track(actor.meta_window, false);
        this._apply(call('wsReconcile', this._model, this._facts()));
        if (wm.get_active_workspace_index() !== VISIBLE)
            this._onActiveChanged();
        this._hookPanels();
        // GNOME applies the workspace settings above a moment later (windows on
        // other monitors stop being on all workspaces): place everything again.
        this._later(() => this._apply(call('wsReadopt', this._model, this._facts())));
    }

    disable() {
        const savePending = this._saveId;
        this._saveId = 0;
        this._reg?.destroy(); // laters, the save timer and the signals
        this._reg = null;
        if (savePending)
            this._saveState();
        this._unhookPanels();
        this._unbindKeys();
        this._teardownSwipe();
        this._unpatchAnimation();
        // Put every window back on the visible workspace, so nothing is lost
        // when Nero is switched off, then give GNOME its own settings back.
        if (this._model) {
            try {
                this._doEffects(call('wsDisable', this._model, this._facts()));
            } catch (e) {
                logError(e, '[nero-workspaces] putting the windows back');
            }
        }
        this._winReg.destroy();
        this._ids.clear();
        this._wins.clear();
        this._model = null;
        this._giveBackMutter();
        this._settings = null;
    }

    // ---- talking to the core -----------------------------------------------

    _config() {
        const s = this._settings;
        return {
            count: s.get_int('workspaces-per-monitor'),
            wrap: s.get_boolean('wrap-around'),
            showOsd: s.get_boolean('show-osd'),
            scrollThreshold: s.get_double('scroll-threshold'),
            scrollCooldownMs: s.get_int('scroll-cooldown-ms'),
        };
    }

    // What the core needs to know about GNOME right now. `extra` is a window
    // that is being tracked and has no id in the maps yet: [window, id].
    _facts(extra = null) {
        const tracked = [...this._ids];
        if (extra)
            tracked.push(extra);
        const byWin = new Map(tracked);
        const stack = new Map(global.display.sort_windows_by_stacking([...byWin.keys()]).map((w, i) => [w, i]));
        const windows = [];
        for (const [w, id] of tracked) {
            windows.push({
                id,
                monitor: w.get_monitor(),
                real: w.get_workspace()?.index() ?? null,
                onAll: w.is_on_all_workspaces(),
                minimized: w.minimized,
                stack: stack.get(w) ?? 0,
            });
        }
        const fw = global.display.focus_window;
        const n = global.display.get_n_monitors();
        return {
            monitors: Array.from({length: n}, (_, i) => connectorOf(i, '')),
            windows,
            focus: byWin.get(fw) ?? null,
            focusMonitor: fw ? fw.get_monitor() : null,
            currentMonitor: global.display.get_current_monitor(),
        };
    }

    // A core answer {model, effects}: keep the model, carry out the effects.
    _apply(out) {
        this._model = out.model;
        this._doEffects(out.effects);
    }

    _doEffects(effects) {
        const time = global.get_current_time();
        for (const e of effects) {
            const w = e.id !== undefined ? this._wins.get(e.id) : null;
            switch (e.do) {
            case 'move':
                if (w)
                    this._move(w, e.real);
                break;
            case 'focus':
                if (w)
                    Main.activateWindow(w, time);
                break;
            case 'raise':
                w?.activate(time);
                break;
            case 'osd':
                Main.osdWindowManager.showOne(e.monitor, Gio.ThemedIcon.new('view-paged-symbolic'), e.text);
                break;
            case 'save':
                this._queueSave();
                break;
            case 'numWorkspaces':
                new Gio.Settings({schema_id: 'org.gnome.desktop.wm.preferences'}).set_int('num-workspaces', e.n);
                break;
            case 'placeLater':
                this._later(() => this._apply(call('wsPlace', this._model, this._facts(), e.id)));
                break;
            case 'showVisible':
                this._moving++;
                try {
                    global.workspace_manager.get_workspace_by_index(VISIBLE).activate(time);
                } finally {
                    this._moving--;
                }
                break;
            default:
                log(`unknown effect ${e.do}`);
            }
        }
    }

    _later(fn) {
        this._reg?.later(fn); // nothing once disabled
    }

    // ---- settings ----------------------------------------------------------

    _onConfigChanged() {
        this._apply(call('wsConfig', this._model, this._facts(), this._config()));
    }

    // Fixed workspaces (GNOME's dynamic ones would remove the empty parking
    // workspaces and renumber them) on every monitor. GNOME's own values are kept
    // in our settings and put back by disable(); the number is set by the core.
    _takeOverMutter() {
        const mutter = new Gio.Settings({schema_id: 'org.gnome.mutter'});
        const wmPrefs = new Gio.Settings({schema_id: 'org.gnome.desktop.wm.preferences'});
        if (this._settings.get_string('saved-gnome-settings') === '') {
            this._settings.set_string('saved-gnome-settings', call('wsSavedGnome', {
                dynamic: mutter.get_boolean('dynamic-workspaces'),
                onlyOnPrimary: mutter.get_boolean('workspaces-only-on-primary'),
                num: wmPrefs.get_int('num-workspaces'),
            }));
        }
        mutter.set_boolean('dynamic-workspaces', false);
        mutter.set_boolean('workspaces-only-on-primary', false);
    }

    _giveBackMutter() {
        const text = this._settings.get_string('saved-gnome-settings');
        if (text === '')
            return; // nothing taken over
        let back;
        try {
            back = call('wsGiveBack', text);
        } catch (e) {
            // without the core GNOME still gets its defaults back
            logError(e, '[nero-workspaces] reading the saved GNOME settings');
            back = {dynamic: null, onlyOnPrimary: null, num: null, damaged: true};
        }
        if (back.damaged)
            log('saved GNOME workspace settings are damaged: putting back GNOME\'s defaults');
        const mutter = new Gio.Settings({schema_id: 'org.gnome.mutter'});
        const wmPrefs = new Gio.Settings({schema_id: 'org.gnome.desktop.wm.preferences'});
        // null = missing or wrong: GNOME's default
        const put = (s, key, v, set) => (v === null ? s.reset(key) : set(key, v));
        put(wmPrefs, 'num-workspaces', back.num, (k, v) => wmPrefs.set_int(k, v));
        put(mutter, 'workspaces-only-on-primary', back.onlyOnPrimary, (k, v) => mutter.set_boolean(k, v));
        put(mutter, 'dynamic-workspaces', back.dynamic, (k, v) => mutter.set_boolean(k, v));
        this._settings.set_string('saved-gnome-settings', '');
    }

    // ---- saved state (what each monitor shows) -----------------------------

    // The file text, or null; the core checks what is in it.
    _loadStateText() {
        try {
            const [, bytes] = stateFile().load_contents(null);
            return new TextDecoder().decode(bytes);
        } catch {
            return null; // no state yet
        }
    }

    _queueSave() {
        if (this._saveId)
            return;
        this._saveId = this._reg?.timeoutSeconds(2, () => {
            this._saveId = 0;
            this._saveState();
        }, GLib.PRIORITY_LOW) ?? 0;
    }

    _saveState() {
        try {
            const file = stateFile();
            // same rights as nero-layout, which keeps window titles in this folder
            GLib.mkdir_with_parents(file.get_parent().get_path(), 0o700);
            file.replace_contents(new TextEncoder().encode(call('wsStateText', this._model)),
                null, false, Gio.FileCreateFlags.PRIVATE | Gio.FileCreateFlags.REPLACE_DESTINATION, null);
        } catch (e) {
            log(`could not save state: ${e}`);
        }
    }

    // For the shell tests: connector -> workspace shown, window -> virtual workspace.
    get _current() {
        return new Map(Object.entries(this._model?.current ?? {}));
    }

    get _vw() {
        const vw = this._model?.vw ?? {};
        return new Map([...this._ids].filter(([, id]) => id in vw).map(([w, id]) => [w, vw[id]]));
    }

    // ---- windows -----------------------------------------------------------

    _trackable(w) {
        return w && !w.is_override_redirect() && TRACKED_TYPES.has(w.get_window_type()) && !w.get_transient_for();
    }

    _track(w, fresh) {
        if (this._ids.has(w) || !this._trackable(w))
            return;
        // the core call first: if it throws, nothing is connected (a later
        // _track would otherwise connect the signals a second time)
        const id = this._nextId;
        const out = call('wsTrack', this._model, this._facts([w, id]), id, fresh);
        this._nextId++;
        this._ids.set(w, id);
        this._wins.set(id, w);
        this._winReg.connect(w, 'workspace-changed', () => this._onWorkspaceChanged(w));
        this._winReg.connect(w, 'unmanaged', () => this._untrack(w));
        this._apply(out);
    }

    _untrack(w) {
        const id = this._ids.get(w);
        this._winReg.disconnectObject(w);
        this._ids.delete(w);
        this._wins.delete(id);
        if (this._model && id !== undefined)
            this._model = call('wsUntrack', this._model, id);
    }

    _move(w, real) {
        this._moving++;
        try {
            w.change_workspace_by_index(real, false);
        } finally {
            this._moving--;
        }
    }

    _onWorkspaceChanged(w) {
        if (this._moving || !this._ids.has(w))
            return;
        // Moved by someone else (an app, the layout saver, the overview).
        this._apply(call('wsWindowMoved', this._model, this._facts(), this._ids.get(w)));
    }

    _onMonitor(w) {
        if (!this._ids.has(w) || this._moving)
            return;
        this._apply(call('wsEnteredMonitor', this._model, this._facts(), this._ids.get(w)));
    }

    // ---- switching ---------------------------------------------------------

    switchMonitor(monitor, target, focusWindow = null) {
        const focus = focusWindow ? this._ids.get(focusWindow) ?? null : null;
        this._apply(call('wsSwitch', this._model, this._facts(), monitor, target, focus));
    }

    stepMonitor(monitor, delta) {
        this._apply(call('wsStepMonitor', this._model, this._facts(), monitor, delta));
    }

    // GNOME switched the real workspace (a parked window was activated, or the
    // overview was used): the core shows that workspace on the window's own
    // monitor and goes back to the visible workspace.
    _onActiveChanged() {
        if (this._snapping)
            return;
        this._snapping = true;
        this._later(() => {
            this._snapping = false;
            const idx = global.workspace_manager.get_active_workspace_index();
            this._apply(call('wsActiveChanged', this._model, this._facts(), idx));
        });
    }

    // No slide animation for real workspace switches: the only ones left are
    // GNOME's own jumps that _onActiveChanged turns back at once.
    _patchAnimation() {
        const anim = Main.wm._workspaceAnimation;
        if (!anim)
            return;
        this._origAnimateSwitch = anim.animateSwitch;
        anim.animateSwitch = (_from, _to, _direction, onComplete) => onComplete();
    }

    _unpatchAnimation() {
        const anim = Main.wm._workspaceAnimation;
        if (anim && this._origAnimateSwitch)
            delete anim.animateSwitch;
        this._origAnimateSwitch = null;
    }

    // ---- touchpad --------------------------------------------------------------

    // GNOME's three-finger swipe slides every monitor: it is switched off (and
    // kept off: GNOME turns it on again when the overview closes) and our own
    // tracker switches only the monitor the swipe starts on.
    _setupSwipe() {
        this._teardownSwipe();
        if (!this._settings.get_boolean('three-finger-swipe'))
            return;
        const gnome = Main.wm._workspaceAnimation?._swipeTracker;
        if (!gnome)
            return;
        this._gnomeTracker = gnome;
        gnome.enabled = false;
        this._gnomeTrackerId = gnome.connect('notify::enabled', () => {
            if (gnome.enabled)
                gnome.enabled = false;
        });
        const tracker = new SwipeTracker.SwipeTracker(global.stage,
            Clutter.Orientation.HORIZONTAL, Shell.ActionMode.NORMAL, {
                allowDrag: false,
                allowScroll: false,
                phase: Clutter.EventPhase.CAPTURE,
                name: 'Nero workspace swipe',
            });
        tracker.connect('begin', (t, monitor) => this._swipeBegin(t, monitor));
        tracker.connect('end', (_t, _duration, endProgress) =>
            this._apply(call('wsSwipeEnd', this._model, this._facts(), endProgress)));
        this._tracker = tracker;
    }

    _teardownSwipe() {
        if (this._tracker) {
            this._tracker.destroy();
            this._tracker = null;
        }
        if (this._gnomeTracker) {
            this._gnomeTracker.disconnect(this._gnomeTrackerId);
            this._gnomeTracker.enabled = !Main.overview.visible;
            this._gnomeTracker = null;
            this._gnomeTrackerId = 0;
        }
    }

    _swipeBegin(tracker, monitor) {
        let begin;
        try {
            begin = call('wsSwipeBegin', this._model, this._facts(), monitor);
        } catch (e) {
            // the tracker waits for confirmSwipe: give it a swipe that goes nowhere
            logError(e, '[nero-workspaces] swipe begin');
            tracker.confirmSwipe(1, [0], 0, 0);
            return;
        }
        this._model = begin.model;
        const wm = global.workspace_manager;
        tracker.orientation = wm.layout_rows !== -1 ? Clutter.Orientation.HORIZONTAL : Clutter.Orientation.VERTICAL;
        // The page size: a swipe as wide as the monitor moves one workspace.
        const geo = global.display.get_monitor_geometry(begin.monitor);
        const distance = tracker.orientation === Clutter.Orientation.HORIZONTAL ? geo.width : geo.height;
        tracker.confirmSwipe(distance, begin.points, begin.current, begin.current);
    }

    // The scroll speed lives in a libinput plugin file (none at 100 %); Mutter
    // reads it at login.
    _writeScrollPlugin() {
        try {
            writeScrollPlugin(this._settings.get_int('touchpad-scroll-speed'));
        } catch (e) {
            log(`could not write the scroll speed plugin: ${e}`);
        }
    }

    // ---- keys ----------------------------------------------------------------

    _keys() {
        const all = Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW;
        return call('wsKeys').map(k => [k.name, k.overview ? all : Shell.ActionMode.NORMAL]);
    }

    _bindKeys() {
        this._boundKeys = this._keys(); // kept: unbinding must work even if the core no longer does
        for (const [name, mode] of this._boundKeys)
            Main.wm.setCustomKeybindingHandler(name, mode, (d, w, e, b) => this._onKey(w, b));
    }

    _unbindKeys() {
        for (const [name, mode] of this._boundKeys ?? [])
            Main.wm.setCustomKeybindingHandler(name, mode, Main.wm._showWorkspaceSwitcher.bind(Main.wm));
        this._boundKeys = null;
    }

    _onKey(window, binding) {
        const w = window ?? global.display.focus_window;
        const monitor = w ? w.get_monitor() : global.display.get_current_monitor();
        this._apply(call('wsKey', this._model, this._facts(), binding.get_name(), monitor, this._ids.get(w) ?? null));
    }

    // ---- Dash to Panel scroll ----------------------------------------------

    _dtpSettings() {
        const ext = Main.extensionManager.lookup(DTP_UUID);
        if (!ext)
            return null;
        try {
            const src = Gio.SettingsSchemaSource.new_from_directory(
                GLib.build_filenamev([ext.path, 'schemas']), Gio.SettingsSchemaSource.get_default(), false);
            const schema = src.lookup('org.gnome.shell.extensions.dash-to-panel', true);
            return schema ? new Gio.Settings({settings_schema: schema}) : null;
        } catch {
            return null;
        }
    }

    _hookPanels() {
        this._unhookPanels();
        const dtp = global.dashToPanel;
        if (!dtp)
            return;
        this._dtp = dtp;
        this._dtpCreatedId = dtp.connect('panels-created', () => this._hookPanels());
        if (!this._settings.get_boolean('take-over-panel-scroll'))
            return;
        // Dash to Panel's own panel scroll switches all monitors: hand it to us
        // (its value is kept and put back by disable()).
        const s = this._dtpSettings();
        if (s) {
            const action = s.get_string('scroll-panel-action');
            if (action === 'SWITCH_WORKSPACE') {
                this._settings.set_string('saved-dtp-scroll', action);
                s.set_string('scroll-panel-action', 'NOTHING');
            }
            this._dtpIconAction = () => s.get_string('scroll-icon-action');
        }
        if (this._settings.get_string('saved-dtp-scroll') !== 'SWITCH_WORKSPACE')
            return; // the user picked another panel scroll action: leave it
        for (const p of dtp.panels ?? []) {
            const actor = p.panel;
            const monitor = p.monitor?.index;
            if (!actor || monitor === undefined)
                continue;
            const entry = {actor, ids: []};
            entry.ids.push(actor.connect('scroll-event', (_a, event) => this._onPanelScroll(monitor, event)));
            // Dash to Panel destroys its panels when it rebuilds them (or is
            // switched off): a destroyed panel must not be touched again.
            entry.ids.push(actor.connect('destroy', () => {
                entry.ids = [];
            }));
            this._panelSignals.push(entry);
        }
    }

    _unhookPanels() {
        for (const {actor, ids} of this._panelSignals)
            ids.forEach(id => actor.disconnect(id));
        this._panelSignals = [];
        // disable() goes on after this: a throw here must not leave the keys,
        // the swipe and the parked windows taken over
        try {
            if (this._model)
                this._model = call('wsForgetPanels', this._model);
        } catch (e) {
            logError(e, '[nero-workspaces] forgetting the panels');
        }
        try {
            if (this._dtp && this._dtpCreatedId)
                this._dtp.disconnect(this._dtpCreatedId);
        } catch {
            // Dash to Panel already dropped its handlers (it was switched off)
        }
        this._dtp = null;
        this._dtpCreatedId = 0;
        if (this._settings && !this._reg)
            this._giveBackDtpScroll(); // disabling
    }

    // Dash to Panel gets its own panel scroll action back.
    _giveBackDtpScroll() {
        const saved = this._settings.get_string('saved-dtp-scroll');
        const s = saved ? this._dtpSettings() : null;
        if (s && s.get_string('scroll-panel-action') === 'NOTHING')
            s.set_string('scroll-panel-action', saved);
        this._settings.set_string('saved-dtp-scroll', '');
    }

    // The actors from the scrolled one up to the stage, for the core to judge.
    _actorChain(actor) {
        const chain = [];
        for (let a = actor; a; a = a.get_parent())
            chain.push({name: a.constructor?.name ?? '', dtpIgnore: !!a._dtpIgnoreScroll});
        return chain;
    }

    _onPanelScroll(monitor, event) {
        const iconAction = this._dtpIconAction?.() ?? 'CYCLE_WINDOWS';
        if (call('wsIgnoreScroll', this._actorChain(global.stage.get_event_actor(event)), iconAction))
            return Clutter.EVENT_PROPAGATE;
        let delta;
        let discrete = false;
        let finished = false;
        const dir = event.get_scroll_direction();
        if (dir === Clutter.ScrollDirection.SMOOTH) {
            const [dx, dy] = event.get_scroll_delta();
            delta = dy !== 0 ? dy : dx;
            const flags = event.get_scroll_finish_flags();
            finished = (flags & (Clutter.ScrollFinishFlags.VERTICAL | Clutter.ScrollFinishFlags.HORIZONTAL)) !== 0;
        } else {
            // Wheels that also send smooth events send these as copies: skip them.
            if (event.is_pointer_emulated())
                return Clutter.EVENT_STOP;
            discrete = true;
            delta = dir === Clutter.ScrollDirection.UP || dir === Clutter.ScrollDirection.LEFT ? -1 : 1;
        }
        this._apply(call('wsPanelScroll', this._model, this._facts(), monitor,
            {delta, discrete, nowMs: event.get_time(), finished}));
        return Clutter.EVENT_STOP;
    }
}
