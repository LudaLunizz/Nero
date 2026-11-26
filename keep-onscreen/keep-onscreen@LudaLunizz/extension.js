// Keep On Screen: on a new window's first frame, shrink and move it into the work area
// of the monitor it opened on. Useful with portrait or small monitors, where apps open at
// their usual size and mutter centres them, so part of the window ends up off-screen.
// The fit itself is Rust (Nero's core/src/fit.rs), run as WebAssembly through nero.js.
// Optional: GNOME Shell's password prompts open on one chosen monitor (password-prompt-monitor)
// and without the dark overlay over all monitors (password-prompt-dim false).
import Meta from 'gi://Meta';
import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {call, plainRect} from './nero.js';

const TYPES = [Meta.WindowType.NORMAL, Meta.WindowType.DIALOG, Meta.WindowType.MODAL_DIALOG];

function fit(window) {
    if (window.maximizedHorizontally || window.maximizedVertically || window.is_fullscreen() || !TYPES.includes(window.get_window_type()))
        return;
    const monitor = window.get_monitor();
    if (monitor < 0)
        return;
    const f = window.get_frame_rect();
    const r = call('fitToWorkArea', plainRect(f), plainRect(window.get_work_area_for_monitor(monitor)));
    if (r.x !== f.x || r.y !== f.y || r.width !== f.width || r.height !== f.height)
        window.move_resize_frame(false, r.x, r.y, r.width, r.height);
}

// GNOME Shell's own password dialogs. Matched by class name: if GNOME renames one, that
// prompt just opens under the pointer as before. A third-party polkit agent (for example
// hyprpolkitagent) shows a normal window instead, which this does not touch.
const PASSWORD_DIALOGS = ['AuthenticationDialog', 'KeyringDialog', 'NetworkSecretDialog', 'ShellMountPasswordDialog'];

// Monitor index for a connector name, or -1 if it is empty or not connected.
function monitorForConnector(connector) {
    return connector ? global.backend.get_monitor_manager().get_monitor_for_connector(connector) : -1;
}

export default class KeepOnScreen extends Extension {
    enable() {
        this._settings = this.getSettings();
        const settings = this._settings;
        const proto = ModalDialog.ModalDialog.prototype;
        const orig = this._open = proto.open;
        proto.open = function (...args) {
            // The overlay is the dialog's lightbox; it fades in inside open(), so switch it off first.
            // The prompt still takes the keyboard until it is answered or cancelled.
            if (this._lightbox && PASSWORD_DIALOGS.includes(this.constructor.name) &&
                !settings.get_boolean('password-prompt-dim'))
                this._lightbox.lightOn = () => {};
            const ok = orig.apply(this, args);
            if (ok && PASSWORD_DIALOGS.includes(this.constructor.name)) {
                const idx = monitorForConnector(settings.get_string('password-prompt-monitor'));
                if (idx >= 0)
                    this._monitorConstraint.index = idx;
            }
            return ok;
        };
        this._id = global.display.connect('window-created', (_d, window) => {
            const actor = window.get_compositor_private();
            if (!actor)
                return;
            const id = actor.connect('first-frame', () => {
                actor.disconnect(id);
                fit(window);
            });
        });
    }

    disable() {
        ModalDialog.ModalDialog.prototype.open = this._open;
        this._open = null;
        this._settings = null;
        global.display.disconnect(this._id);
        this._id = 0;
    }
}
