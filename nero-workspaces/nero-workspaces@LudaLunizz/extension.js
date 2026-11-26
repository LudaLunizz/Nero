// Nero Workspaces: every monitor switches its own workspaces.
//
// GNOME switches workspaces on all monitors at once. Here GNOME stays on one real
// workspace (index 0, the one on screen) and every monitor gets its own row of
// virtual workspaces: a window on its monitor's current virtual workspace sits on
// real workspace 0, any other window is parked on real workspace vw + 1, out of
// sight. Switching a monitor moves only that monitor's windows.
//
// Switching: the scroll wheel (or touchpad) on a Dash to Panel panel switches the
// monitor of that panel; the workspace keys (Super+Page Up/Down, Ctrl+Alt+arrows,
// Super+1..) switch the monitor of the focused window. Activating a parked window
// (taskbar, Alt+Tab, a notification) shows its workspace on its own monitor.
//
// The bookkeeping (which window goes where, scroll -> steps) is Rust (Nero's
// core/src/workspaces.rs), run as WebAssembly through nero.js.
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import Clutter from 'gi://Clutter';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {call} from './nero.js';

const STATE_VERSION = 1;
const DTP_UUID = 'dash-to-panel@jderose9.github.com';
const VISIBLE = 0;

const stateFile = () => Gio.File.new_for_path(GLib.build_filenamev([GLib.get_user_state_dir(), 'nero', 'workspaces.json']));

function log(msg) {
    console.log(`[nero-workspaces] ${msg}`);
}

// Keybindings GNOME Shell handles with its workspace switcher, and their modes.
const NO = Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW;
const N = Shell.ActionMode.NORMAL;
const KEYS = [
    ...['left', 'right', 'up', 'down', 'last'].map(t => [`switch-to-workspace-${t}`, NO]),
    ...['left', 'right', 'up', 'down'].map(t => [`move-to-workspace-${t}`, NO]),
    ...Array.from({length: 12}, (_, i) => [`switch-to-workspace-${i + 1}`, NO]),
    ...Array.from({length: 12}, (_, i) => [`move-to-workspace-${i + 1}`, N]),
    ['move-to-workspace-last', N],
];

const TRACKED_TYPES = new Set([
    Meta.WindowType.NORMAL, Meta.WindowType.DIALOG, Meta.WindowType.MODAL_DIALOG, Meta.WindowType.UTILITY,
]);

function connectorOf(index) {
    const lm = global.backend.get_monitor_manager().get_logical_monitors().find(l => l.get_number() === index);
    return lm?.get_monitors()[0]?.get_connector() ?? `#${index}`;
}

export default class NeroWorkspaces extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._current = new Map(); // connector -> virtual workspace shown
        this._vw = new Map(); // Meta.Window -> virtual workspace
        this._winSignals = new Map(); // Meta.Window -> [ids]
        this._scroll = new Map(); // panel actor -> scroll state
        this._panelSignals = [];
        this._moving = 0;
        this._laters = new Set();
        this._saveId = 0;
        this._loadState();
        this._takeOverMutter();
        this._patchAnimation();
        this._bindKeys();

        const display = global.display;
        const wm = global.workspace_manager;
        this._signals = [
            [display, display.connect('window-created', (_d, w) => this._later(() => this._track(w, true)))],
            [display, display.connect('window-entered-monitor', (_d, _m, w) => this._onMonitor(w))],
            [wm, wm.connect('active-workspace-changed', () => this._onActiveChanged())],
            [this._settings, this._settings.connect('changed::workspaces-per-monitor', () => this._onCountChanged())],
            [global.backend.get_monitor_manager(), global.backend.get_monitor_manager().connect('monitors-changed', () => this._later(() => this._reconcileAll()))],
            [Main.extensionManager, Main.extensionManager.connect('extension-state-changed', (_m, ext) => {
                if (ext.uuid === DTP_UUID)
                    this._hookPanels();
            })],
        ];
        for (const actor of global.get_window_actors())
            this._track(actor.meta_window, false);
        this._reconcileAll();
        if (wm.get_active_workspace_index() !== VISIBLE)
            this._onActiveChanged();
        this._hookPanels();
        // GNOME applies the workspace settings above a moment later (windows on
        // other monitors stop being on all workspaces): place everything again.
        this._later(() => this._readoptAll());
    }

    disable() {
        for (const id of this._laters)
            global.compositor.get_laters().remove(id);
        this._laters.clear();
        if (this._saveId) {
            GLib.source_remove(this._saveId);
            this._saveId = 0;
            this._saveState();
        }
        for (const [obj, id] of this._signals)
            obj.disconnect(id);
        this._signals = null;
        this._unhookPanels();
        this._unbindKeys();
        this._unpatchAnimation();
        // Put every window back on the visible workspace, so nothing is lost
        // when Nero is switched off, then give GNOME its own settings back.
        this._moving++;
        for (const w of this._vw.keys()) {
            try {
                if (w.get_workspace()?.index() !== VISIBLE)
                    w.change_workspace_by_index(VISIBLE, false);
            } catch {
                // window already gone
            }
        }
        this._moving--;
        for (const [w, ids] of this._winSignals)
            ids.forEach(id => w.disconnect(id));
        this._winSignals.clear();
        this._vw.clear();
        this._giveBackMutter();
        this._settings = null;
    }

    // ---- settings ----------------------------------------------------------

    get _count() {
        return Math.max(1, Math.min(12, this._settings.get_int('workspaces-per-monitor')));
    }

    // Fixed workspaces (GNOME's dynamic ones would remove the empty parking
    // workspaces and renumber them) on every monitor. GNOME's own values are kept
    // in our settings and put back by disable().
    _takeOverMutter() {
        const mutter = new Gio.Settings({schema_id: 'org.gnome.mutter'});
        const wmPrefs = new Gio.Settings({schema_id: 'org.gnome.desktop.wm.preferences'});
        if (this._settings.get_string('saved-gnome-settings') === '') {
            this._settings.set_string('saved-gnome-settings', JSON.stringify({
                dynamic: mutter.get_boolean('dynamic-workspaces'),
                onlyOnPrimary: mutter.get_boolean('workspaces-only-on-primary'),
                num: wmPrefs.get_int('num-workspaces'),
            }));
        }
        mutter.set_boolean('dynamic-workspaces', false);
        mutter.set_boolean('workspaces-only-on-primary', false);
        wmPrefs.set_int('num-workspaces', this._count + 1);
    }

    _giveBackMutter() {
        let saved;
        try {
            saved = JSON.parse(this._settings.get_string('saved-gnome-settings'));
        } catch {
            return;
        }
        const mutter = new Gio.Settings({schema_id: 'org.gnome.mutter'});
        const wmPrefs = new Gio.Settings({schema_id: 'org.gnome.desktop.wm.preferences'});
        wmPrefs.set_int('num-workspaces', saved.num);
        mutter.set_boolean('workspaces-only-on-primary', saved.onlyOnPrimary);
        mutter.set_boolean('dynamic-workspaces', saved.dynamic);
        this._settings.set_string('saved-gnome-settings', '');
    }

    _onCountChanged() {
        const count = this._count;
        // Windows on workspaces that go away join the last one.
        for (const [w, vw] of this._vw) {
            if (vw >= count)
                this._vw.set(w, count - 1);
        }
        for (const [c, cur] of this._current) {
            if (cur >= count)
                this._current.set(c, count - 1);
        }
        this._reconcileAll();
        new Gio.Settings({schema_id: 'org.gnome.desktop.wm.preferences'}).set_int('num-workspaces', count + 1);
        this._queueSave();
    }

    // ---- saved state (what each monitor shows) -----------------------------

    _loadState() {
        try {
            const [, bytes] = stateFile().load_contents(null);
            const data = JSON.parse(new TextDecoder().decode(bytes));
            for (const [c, v] of Object.entries(data.current ?? {})) {
                if (Number.isInteger(v))
                    this._current.set(c, v);
            }
        } catch {
            // no state yet
        }
    }

    _queueSave() {
        if (this._saveId)
            return;
        this._saveId = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, 2, () => {
            this._saveId = 0;
            this._saveState();
            return GLib.SOURCE_REMOVE;
        });
    }

    _saveState() {
        try {
            const file = stateFile();
            file.get_parent().make_directory_with_parents(null);
        } catch {
            // exists
        }
        try {
            const data = {version: STATE_VERSION, current: Object.fromEntries(this._current)};
            stateFile().replace_contents(new TextEncoder().encode(`${JSON.stringify(data, null, 1)}\n`),
                null, false, Gio.FileCreateFlags.REPLACE_DESTINATION, null);
        } catch (e) {
            log(`could not save state: ${e}`);
        }
    }

    _currentOf(monitor) {
        return Math.min(this._current.get(connectorOf(monitor)) ?? 0, this._count - 1);
    }

    // ---- windows -----------------------------------------------------------

    _later(fn) {
        const laters = global.compositor.get_laters();
        const id = laters.add(Meta.LaterType.BEFORE_REDRAW, () => {
            this._laters.delete(id);
            try {
                fn();
            } catch (e) {
                logError(e, '[nero-workspaces]');
            }
            return GLib.SOURCE_REMOVE;
        });
        this._laters.add(id);
    }

    _trackable(w) {
        return w && !w.is_override_redirect() && TRACKED_TYPES.has(w.get_window_type()) && !w.get_transient_for();
    }

    _track(w, fresh) {
        // Windows on all workspaces are tracked too (left alone while they are):
        // GNOME can take them off all workspaces later, without a new window.
        if (this._vw.has(w) || !this._trackable(w))
            return;
        const ids = [
            w.connect('workspace-changed', () => this._onWorkspaceChanged(w)),
            w.connect('unmanaged', () => this._untrack(w)),
        ];
        this._winSignals.set(w, ids);
        const real = w.get_workspace()?.index() ?? VISIBLE;
        this._vw.set(w, call('wsAdopt', real, this._currentOf(w.get_monitor()), this._count));
        if (fresh)
            this._place(w);
    }

    _untrack(w) {
        this._winSignals.get(w)?.forEach(id => w.disconnect(id));
        this._winSignals.delete(w);
        this._vw.delete(w);
    }

    _move(w, real) {
        this._moving++;
        try {
            w.change_workspace_by_index(real, false);
        } finally {
            this._moving--;
        }
    }

    // Put one window where its virtual workspace says.
    _place(w) {
        const vw = this._vw.get(w);
        if (vw === undefined || w.is_on_all_workspaces())
            return;
        const real = call('wsRealFor', vw, this._currentOf(w.get_monitor()));
        if (w.get_workspace()?.index() !== real)
            this._move(w, real);
    }

    _onWorkspaceChanged(w) {
        if (this._moving)
            return;
        const ws = w.get_workspace();
        if (!ws) {
            // now on all workspaces: GNOME keeps it on screen everywhere
            return;
        }
        // Moved by someone else (an app, the layout saver, the overview).
        this._vw.set(w, call('wsAdopt', ws.index(), this._currentOf(w.get_monitor()), this._count));
        this._later(() => this._place(w));
    }

    // A window that comes onto a monitor joins the workspace that monitor shows.
    _onMonitor(w) {
        if (!this._vw.has(w) || this._moving)
            return;
        if (w.get_workspace()?.index() === VISIBLE)
            this._vw.set(w, this._currentOf(w.get_monitor()));
        else
            this._later(() => this._place(w));
    }

    _readoptAll() {
        for (const w of this._vw.keys()) {
            const ws = w.get_workspace();
            if (ws && !w.is_on_all_workspaces())
                this._vw.set(w, call('wsAdopt', ws.index(), this._currentOf(w.get_monitor()), this._count));
        }
        this._reconcileAll();
    }

    _reconcileAll() {
        for (const w of this._vw.keys())
            this._place(w);
    }

    _windowsJson() {
        let id = 0;
        const byId = new Map();
        const list = [];
        for (const [w, vw] of this._vw) {
            if (w.is_on_all_workspaces())
                continue;
            id++;
            byId.set(id, w);
            list.push({id, monitor: w.get_monitor(), vw, real: w.get_workspace()?.index() ?? VISIBLE});
        }
        return [list, byId];
    }

    // ---- switching ---------------------------------------------------------

    switchMonitor(monitor, target, focusWindow = null) {
        const count = this._count;
        if (monitor < 0 || target < 0 || target >= count)
            return;
        const focused = global.display.focus_window;
        const [list, byId] = this._windowsJson();
        for (const m of call('wsPlanSwitch', list, monitor, target))
            this._move(byId.get(m.id), m.real);
        this._current.set(connectorOf(monitor), target);
        this._queueSave();

        const time = global.get_current_time();
        if (focusWindow) {
            Main.activateWindow(focusWindow, time);
        } else if (focused && focused.get_monitor() === monitor) {
            // Focus moves to the top window of the workspace that came in, if any.
            const shown = global.display.sort_windows_by_stacking(
                [...this._vw.keys()].filter(w => w.get_monitor() === monitor &&
                    w.get_workspace()?.index() === VISIBLE && !w.minimized));
            const top = shown[shown.length - 1];
            if (top)
                top.activate(time);
        }
        if (this._settings.get_boolean('show-osd')) {
            Main.osdWindowManager.showOne(monitor, Gio.ThemedIcon.new('view-paged-symbolic'),
                `${target + 1} / ${count}`);
        }
    }

    stepMonitor(monitor, delta) {
        const cur = this._currentOf(monitor);
        const next = call('wsStep', cur, delta, this._count, this._settings.get_boolean('wrap-around'));
        if (next !== cur)
            this.switchMonitor(monitor, next);
    }

    // GNOME switched the real workspace (a parked window was activated, or the
    // overview was used): show that workspace on the window's own monitor and
    // go back to the visible workspace.
    _onActiveChanged() {
        if (this._snapping)
            return;
        this._snapping = true;
        this._later(() => {
            this._snapping = false;
            const wm = global.workspace_manager;
            const idx = wm.get_active_workspace_index();
            if (idx === VISIBLE)
                return;
            const fw = global.display.focus_window;
            const owner = fw && this._vw.has(fw) && fw.get_workspace()?.index() === idx ? fw : null;
            const monitor = owner ? owner.get_monitor() : global.display.get_current_monitor();
            const target = call('wsAdopt', idx, this._currentOf(monitor), this._count);
            const time = global.get_current_time();
            this._moving++;
            try {
                wm.get_workspace_by_index(VISIBLE).activate(time);
            } finally {
                this._moving--;
            }
            this.switchMonitor(monitor, target, owner);
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

    // ---- keys ----------------------------------------------------------------

    _bindKeys() {
        for (const [name, mode] of KEYS)
            Main.wm.setCustomKeybindingHandler(name, mode, (d, w, e, b) => this._onKey(w, b));
    }

    _unbindKeys() {
        for (const [name, mode] of KEYS)
            Main.wm.setCustomKeybindingHandler(name, mode, Main.wm._showWorkspaceSwitcher.bind(Main.wm));
    }

    _onKey(window, binding) {
        const [action, , , target] = binding.get_name().split('-');
        const w = window ?? global.display.focus_window;
        const monitor = w ? w.get_monitor() : global.display.get_current_monitor();
        const cur = this._currentOf(monitor);
        const count = this._count;
        const wrap = this._settings.get_boolean('wrap-around');
        let next;
        if (target === 'last')
            next = count - 1;
        else if (target === 'left' || target === 'up')
            next = call('wsStep', cur, -1, count, wrap);
        else if (target === 'right' || target === 'down')
            next = call('wsStep', cur, 1, count, wrap);
        else
            next = Number(target) - 1;
        if (!(next >= 0 && next < count))
            return;
        if (action === 'move') {
            if (!w || !this._vw.has(w))
                return;
            this._vw.set(w, next);
            this.switchMonitor(monitor, next, w);
        } else if (next !== cur) {
            this.switchMonitor(monitor, next);
        }
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
            entry.ids.push(actor.connect('scroll-event', (_a, event) => this._onPanelScroll(actor, monitor, event)));
            // Dash to Panel destroys its panels when it rebuilds them (or is
            // switched off): a destroyed panel must not be touched again.
            entry.ids.push(actor.connect('destroy', () => {
                entry.ids = [];
                this._scroll.delete(actor);
            }));
            this._panelSignals.push(entry);
        }
    }

    _unhookPanels() {
        for (const {actor, ids} of this._panelSignals)
            ids.forEach(id => actor.disconnect(id));
        this._panelSignals = [];
        this._scroll.clear();
        if (this._dtp && this._dtpCreatedId)
            this._dtp.disconnect(this._dtpCreatedId);
        this._dtp = null;
        this._dtpCreatedId = 0;
        if (this._settings && !this._signals) {
            // disabling: give Dash to Panel its scroll action back
            const saved = this._settings.get_string('saved-dtp-scroll');
            const s = saved ? this._dtpSettings() : null;
            if (s && s.get_string('scroll-panel-action') === 'NOTHING')
                s.set_string('scroll-panel-action', saved);
            this._settings.set_string('saved-dtp-scroll', '');
        }
    }

    // Scroll over an app icon that cycles windows (or Dash to Panel's own
    // workspace indicator) is not ours.
    _ignoredSource(actor) {
        const iconAction = this._dtpIconAction?.() ?? 'CYCLE_WINDOWS';
        for (let a = actor; a; a = a.get_parent()) {
            if (a._dtpIgnoreScroll || a.constructor?.name === 'WorkspaceIndicator')
                return true;
            if (/AppIcon/.test(a.constructor?.name ?? '') && iconAction !== 'PASS_THROUGH' && iconAction !== 'NOTHING')
                return true;
        }
        return false;
    }

    _onPanelScroll(actor, monitor, event) {
        if (this._ignoredSource(global.stage.get_event_actor(event)))
            return Clutter.EVENT_PROPAGATE;
        let delta;
        let discrete = false;
        let finished = false;
        const dir = event.get_scroll_direction();
        if (dir === Clutter.ScrollDirection.SMOOTH) {
            const [dx, dy] = event.get_scroll_delta();
            delta = dy !== 0 ? dy : dx;
            finished = (event.get_scroll_finish_flags() & Clutter.ScrollFinishFlags.VERTICAL) !== 0 ||
                (event.get_scroll_finish_flags() & Clutter.ScrollFinishFlags.HORIZONTAL) !== 0;
        } else {
            // Wheels that also send smooth events send these as copies: skip them.
            if (event.is_pointer_emulated())
                return Clutter.EVENT_STOP;
            discrete = true;
            delta = dir === Clutter.ScrollDirection.UP || dir === Clutter.ScrollDirection.LEFT ? -1 : 1;
        }
        const state = this._scroll.get(actor) ?? {acc: 0, lastStepMs: -1e9, lastEventMs: -1e9};
        const res = call('wsScroll', state, {delta, discrete, nowMs: event.get_time(), finished},
            this._settings.get_double('scroll-threshold'), this._settings.get_int('scroll-cooldown-ms'));
        this._scroll.set(actor, res.state);
        if (res.step)
            this.stepMonitor(monitor, res.step);
        return Clutter.EVENT_STOP;
    }
}
