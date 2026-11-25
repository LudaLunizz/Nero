// Keep On Screen: on a new window's first frame, shrink and move it into the work area
// of the monitor it opened on. Useful with portrait or small monitors, where apps open at
// their usual size and mutter centres them, so part of the window ends up off-screen.
// The fit itself is Rust (Nero's core/src/fit.rs), run as WebAssembly through nero.js.
import Meta from 'gi://Meta';
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

export default class KeepOnScreen extends Extension {
    enable() {
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
        global.display.disconnect(this._id);
        this._id = 0;
    }
}
