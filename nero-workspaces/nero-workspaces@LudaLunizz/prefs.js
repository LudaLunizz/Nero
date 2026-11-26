// Nero Workspaces settings: open it from the Extensions app (Settings), or
//   gnome-extensions prefs nero-workspaces@LudaLunizz
//
// Touchpad page: the controls GNOME Settings lacks (two-finger scroll speed, the
// per-monitor three-finger swipe) next to GNOME's own pointer speed, so the
// touchpad speeds are in one place. Workspaces page: the per-monitor workspaces.
import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk';
import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';
import {writeScrollPlugin} from './scrollSpeed.js';

// A row with a slider bound to a settings key.
function sliderRow(title, subtitle, settings, key, {lower, upper, step, digits = 0, format = null, marks = []}) {
    const row = new Adw.ActionRow({title, subtitle});
    const adjustment = new Gtk.Adjustment({lower, upper, step_increment: step, page_increment: step * 10});
    const scale = new Gtk.Scale({
        adjustment,
        digits,
        round_digits: digits,
        draw_value: format !== null,
        value_pos: Gtk.PositionType.RIGHT,
        hexpand: true,
        width_request: 280,
        valign: Gtk.Align.CENTER,
    });
    if (format)
        scale.set_format_value_func((_s, v) => format(v));
    for (const [value, label] of marks)
        scale.add_mark(value, Gtk.PositionType.BOTTOM, label);
    settings.bind(key, adjustment, 'value', Gio.SettingsBindFlags.DEFAULT);
    row.add_suffix(scale);
    return row;
}

function switchRow(title, subtitle, settings, key) {
    const row = new Adw.SwitchRow({title, subtitle});
    settings.bind(key, row, 'active', Gio.SettingsBindFlags.DEFAULT);
    return row;
}

function spinRow(title, subtitle, settings, key, lower, upper, step, digits = 0) {
    const row = new Adw.SpinRow({
        title,
        subtitle,
        digits,
        adjustment: new Gtk.Adjustment({lower, upper, step_increment: step, page_increment: step * 10}),
    });
    settings.bind(key, row.adjustment, 'value', Gio.SettingsBindFlags.DEFAULT);
    return row;
}

export default class NeroWorkspacesPrefs extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        const touchpad = new Gio.Settings({schema_id: 'org.gnome.desktop.peripherals.touchpad'});
        window._settings = [settings, touchpad]; // keep the bindings alive
        // The plugin file follows the slider from here too (the shell may still
        // run an older Nero Workspaces that does not know the setting).
        const speedId = settings.connect('changed::touchpad-scroll-speed', () => {
            try {
                writeScrollPlugin(settings.get_int('touchpad-scroll-speed'));
            } catch (e) {
                console.error(`[nero-workspaces] could not write the scroll speed plugin: ${e}`);
            }
        });
        window.connect('close-request', () => settings.disconnect(speedId));
        window.set_default_size(720, 640);

        // ---- Touchpad ----
        const tpPage = new Adw.PreferencesPage({title: 'Touchpad', icon_name: 'input-touchpad-symbolic'});

        const speeds = new Adw.PreferencesGroup({
            title: 'Speed',
            description: 'Pointer speed is GNOME\'s own setting (the same slider as in Settings, Mouse &amp; Touchpad).',
        });
        speeds.add(sliderRow('Pointer speed', null, touchpad, 'speed', {
            lower: -1, upper: 1, step: 0.05, digits: 2,
            marks: [[-1, 'Slow'], [0, null], [1, 'Fast']],
        }));
        speeds.add(sliderRow('Two-finger scroll speed',
            '100 % is GNOME\'s normal speed. Pinch-to-zoom follows it too. Works from the next login.',
            settings, 'touchpad-scroll-speed', {
                lower: 0, upper: 100, step: 1,
                format: v => `${Math.round(v)} %`,
                marks: [[0, null], [25, null], [50, null], [75, null], [100, null]],
            }));
        tpPage.add(speeds);

        const gestures = new Adw.PreferencesGroup({title: 'Gestures'});
        gestures.add(switchRow('Three-finger swipe switches one screen',
            'Swipe left or right with three fingers: only the screen under the pointer goes to its next or previous workspace. Off = GNOME\'s swipe (all screens at once).',
            settings, 'three-finger-swipe'));
        tpPage.add(gestures);
        window.add(tpPage);

        // ---- Workspaces ----
        const wsPage = new Adw.PreferencesPage({title: 'Workspaces', icon_name: 'view-paged-symbolic'});
        const ws = new Adw.PreferencesGroup({title: 'Workspaces per screen'});
        ws.add(spinRow('Workspaces per screen', 'Every screen has this many workspaces of its own.',
            settings, 'workspaces-per-monitor', 1, 12, 1));
        ws.add(switchRow('Go round', 'Past the last workspace comes the first one again (scroll and keys).',
            settings, 'wrap-around'));
        ws.add(switchRow('Show the workspace number', 'Show "2 / 4" on the screen that switched.',
            settings, 'show-osd'));
        wsPage.add(ws);

        const panel = new Adw.PreferencesGroup({title: 'Scroll on the panel'});
        panel.add(switchRow('Panel scroll switches that screen only',
            'Takes over Dash to Panel\'s "Switch workspace" scroll action and does it per screen.',
            settings, 'take-over-panel-scroll'));
        panel.add(spinRow('Scroll per step', '1.0 = one wheel notch.',
            settings, 'scroll-threshold', 0.1, 10, 0.1, 1));
        panel.add(spinRow('Pause between steps (ms)', 'So one flick moves one workspace.',
            settings, 'scroll-cooldown-ms', 0, 2000, 10));
        wsPage.add(panel);
        window.add(wsPage);
    }
}
