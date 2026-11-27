// Keep On Screen: on a new window's first frame, shrink and move it into the work area
// of the monitor it opened on. Useful with portrait or small monitors, where apps open at
// their usual size and mutter centres them, so part of the window ends up off-screen.
// Optional: GNOME Shell's password prompts open on one chosen monitor (password-prompt-monitor)
// and without the dark overlay over all monitors (password-prompt-dim false).
// Every decision is Rust (Nero's core/src/keep_model.rs), run as WebAssembly through
// nero.js; this file reports what GNOME shows and does what the core answers.
import Meta from 'gi://Meta';
import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {call, plainRect} from './nero.js';
import {monitorForConnector} from './helpers.js';

// Meta.WindowType value -> its name (NORMAL, DIALOG, ...), for the core.
const TYPE_NAMES = new Map(Object.entries(Meta.WindowType).map(([name, value]) => [value, name]));

function fit(window) {
    const monitor = window.get_monitor();
    const f = window.get_frame_rect();
    const r = call('koFit', {
        windowType: TYPE_NAMES.get(window.get_window_type()) ?? '',
        monitor,
        frame: plainRect(f),
        workArea: monitor >= 0 ? plainRect(window.get_work_area_for_monitor(monitor)) : plainRect(f),
        maximized: window.maximizedHorizontally || window.maximizedVertically,
        fullscreen: window.is_fullscreen(),
    });
    if (r)
        window.move_resize_frame(false, r.x, r.y, r.width, r.height);
}

export default class KeepOnScreen extends Extension {
    enable() {
        this._settings = this.getSettings();
        const settings = this._settings;
        const proto = ModalDialog.ModalDialog.prototype;
        const orig = this._open = proto.open;
        this._on = {active: true};
        const on = this._on;
        this._frameIds = new Map(); // actor -> first-frame handler id
        proto.open = this._patched = function (...args) {
            if (!on.active)
                return orig.apply(this, args); // disabled, but another extension still calls us
            // The overlay is the dialog's lightbox; it fades in inside open(), so switch it off first.
            // The prompt still takes the keyboard until it is answered or cancelled.
            // A core that fails (missing or old wasm) must never stop a password prompt
            // from opening: then the prompt simply opens as GNOME has it.
            const name = this.constructor.name;
            try {
                if (this._lightbox && call('koPromptBefore', name, settings.get_boolean('password-prompt-dim')))
                    this._lightbox.lightOn = () => {};
            } catch (e) {
                logError(e, 'nero-keep-onscreen: koPromptBefore');
            }
            const ok = orig.apply(this, args);
            if (ok) {
                try {
                    const idx = call('koPromptAfter', name, monitorForConnector(settings.get_string('password-prompt-monitor')));
                    if (idx !== null)
                        this._monitorConstraint.index = idx;
                } catch (e) {
                    logError(e, 'nero-keep-onscreen: koPromptAfter');
                }
            }
            return ok;
        };
        this._id = global.display.connect('window-created', (_d, window) => {
            const actor = window.get_compositor_private();
            if (!actor)
                return;
            const forget = () => {
                this._frameIds.delete(actor);
                actor.disconnect(id);
                actor.disconnect(destroyId);
            };
            const id = actor.connect('first-frame', () => {
                forget();
                fit(window);
            });
            // closed before its first frame: nothing left to disconnect in disable()
            const destroyId = actor.connect('destroy', forget);
            this._frameIds.set(actor, [id, destroyId]);
        });
    }

    disable() {
        // Put back the original open() only if ours is still the one in place: another
        // extension that patched it after us has our function as its original, and
        // overwriting it would drop its patch. Ours then only passes calls on.
        this._on.active = false;
        this._on = null;
        const proto = ModalDialog.ModalDialog.prototype;
        if (proto.open === this._patched)
            proto.open = this._open;
        else
            console.warn('[nero-keep-onscreen] another extension replaced ModalDialog.open after us: left as it is');
        this._open = null;
        this._patched = null;
        for (const [actor, ids] of this._frameIds) {
            try {
                for (const id of ids)
                    actor.disconnect(id);
            } catch {
                // actor already gone
            }
        }
        this._frameIds = null;
        this._settings = null;
        global.display.disconnect(this._id);
        this._id = 0;
    }
}
