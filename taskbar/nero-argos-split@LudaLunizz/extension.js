// Moves the Argos buttons from the main panel into the Dash to Panel taskbar of
// one chosen monitor (gsettings key `monitor`, a connector name like DP-1).
import GLib from 'gi://GLib';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

export default class NeroArgosSplit extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._moved = new Map(); // container -> [original parent, destroy handler id]
        this._box = null;
        this._boxId = 0;
        this._settingsId = this._settings.connect('changed::monitor', () => {
            this._restore();
            this._trySync();
        });
        // Argos and Dash to Panel load in an unknown order and Dash to Panel
        // rebuilds its panels on setting/monitor changes, so retry on a timer.
        this._timeout = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 2, () => {
            this._trySync();
            return GLib.SOURCE_CONTINUE;
        });
        this._trySync();
    }

    // A throw (a Dash to Panel update changed its insides) must not end the timer.
    _trySync() {
        try {
            this._sync();
        } catch (e) {
            logError(e, 'nero-argos-split');
        }
    }

    disable() {
        GLib.source_remove(this._timeout);
        this._timeout = 0;
        this._settings.disconnect(this._settingsId);
        this._settings = null;
        this._restore();
        this._moved = null;
    }

    _targetBox() {
        const connector = this._settings.get_string('monitor');
        const panels = global.dashToPanel?.panels;
        if (!connector || !panels) return null;
        const idx = global.backend.get_monitor_manager()
            .get_monitor_for_connector(connector);
        if (idx < 0) return null;
        const panel = panels.find(p => p.monitor?.index === idx);
        // the main panel itself is already in the right place when it is the target
        if (!panel || panel.panel === Main.panel || panel.isPrimary) return null;
        return panel._rightBox ?? null;
    }

    _sync() {
        const box = this._targetBox();
        if (!box) {
            // target gone (unplugged, setting cleared): buttons back home
            if (this._box) this._restore();
            return;
        }
        if (box !== this._box) {
            this._restore();
            this._box = box;
            // Dash to Panel destroys a panel with everything in it (monitor or setting
            // change). 'destroy' handlers run before the children die, so take the
            // buttons home first; the timer then moves them into the new panel.
            this._boxId = box.connect('destroy', () => this._restore());
        }
        // the buttons keep their order: the k-th Argos button goes to place k
        let at = 0;
        for (const [name, button] of Object.entries(Main.panel.statusArea)) {
            if (!name.startsWith('argos-button-')) continue;
            const container = button.container;
            if (!container) continue;
            if (container.get_parent() === box) {
                at++;
                continue;
            }
            const old = container.get_parent();
            if (!this._moved.has(container)) {
                // Argos destroys its buttons on disable or script change; touching a
                // destroyed one from disable() can crash gnome-shell, so forget it.
                const id = container.connect('destroy', () => this._moved.delete(container));
                const index = old ? old.get_children().indexOf(container) : 0;
                this._moved.set(container, [old ?? Main.panel._rightBox, id, Math.max(index, 0)]);
            }
            old?.remove_child(container);
            box.insert_child_at_index(container, at++);
        }
    }

    // Put every moved button back into the main panel box it came from, at its old
    // place (lowest first, so each index means what it meant before the move).
    _restore() {
        this._forgetBox();
        const moved = [...this._moved].sort((a, b) => a[1][2] - b[1][2]);
        for (const [container, [parent, id, index]] of moved) {
            container.disconnect(id);
            container.get_parent()?.remove_child(container);
            parent.insert_child_at_index(container, index);
        }
        this._moved.clear();
    }

    _forgetBox() {
        if (this._boxId) this._box.disconnect(this._boxId);
        this._box = null;
        this._boxId = 0;
    }
}
