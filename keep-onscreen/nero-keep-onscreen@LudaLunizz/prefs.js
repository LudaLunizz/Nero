// Keep On Screen settings: open it from the Extensions app (Settings), or
//   gnome-extensions prefs nero-keep-onscreen@LudaLunizz
//
// Password prompts page: pick the monitor GNOME's password prompts open on (from the
// monitors connected now), switch the dark overlay on or off, and open a test prompt to
// see where the next one will show up.
import Adw from 'gi://Adw';
import Gdk from 'gi://Gdk';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk';
import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';
import {promptChoices} from './promptChoices.js';

const KEY = 'password-prompt-monitor';

function connectedMonitors() {
    const list = Gdk.Display.get_default()?.get_monitors();
    const out = [];
    for (let i = 0; list && i < list.get_n_items(); i++) {
        const m = list.get_item(i);
        const g = m.get_geometry();
        const name = [m.get_manufacturer(), m.get_model()].filter(Boolean).join(' ');
        out.push({connector: m.get_connector(), name, width: g.width, height: g.height, x: g.x, y: g.y});
    }
    return out;
}

export default class KeepOnScreenPrefs extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        window._settings = settings; // keep the binding alive

        const page = new Adw.PreferencesPage({title: 'Password prompts', icon_name: 'dialog-password-symbolic'});
        const group = new Adw.PreferencesGroup({
            title: 'Password prompts',
            description: 'GNOME\'s own prompts: polkit (administrator password), keyring unlock, ' +
                'Wi-Fi/VPN secrets and encrypted disk unlock. They normally open on the monitor under the pointer.',
        });
        page.add(group);

        const model = new Gtk.StringList();
        const combo = new Adw.ComboRow({
            title: 'Open on',
            subtitle: 'Every prompt opens on this monitor, from the next one on',
            model,
        });
        group.add(combo);

        let values = [];
        let filling = false;
        const fill = () => {
            const c = promptChoices(connectedMonitors(), settings.get_string(KEY));
            filling = true;
            model.splice(0, model.get_n_items(), c.labels);
            values = c.values;
            combo.selected = Math.max(c.selected, 0);
            filling = false;
        };
        combo.connect('notify::selected', () => {
            if (filling || combo.selected >= values.length)
                return;
            if (settings.get_string(KEY) !== values[combo.selected])
                settings.set_string(KEY, values[combo.selected]);
        });
        const keyId = settings.connect(`changed::${KEY}`, fill);
        const monitors = Gdk.Display.get_default()?.get_monitors();
        const monId = monitors?.connect('items-changed', fill);
        fill();

        const dim = new Adw.SwitchRow({
            title: 'Dark overlay',
            subtitle: 'Darken all monitors while a prompt is open (GNOME default). ' +
                'Off: the other monitors stay as they are; the prompt still takes the keyboard.',
        });
        settings.bind('password-prompt-dim', dim, 'active', Gio.SettingsBindFlags.DEFAULT);
        group.add(dim);

        // A real polkit prompt, so you see where the next one opens. It only runs `true`.
        const test = new Adw.ActionRow({
            title: 'Try it',
            subtitle: 'Opens an administrator prompt (pkexec true). Cancel it, or type your password: it runs nothing else.',
        });
        const button = new Gtk.Button({label: 'Show a prompt', valign: Gtk.Align.CENTER});
        button.connect('clicked', () => {
            try {
                Gio.Subprocess.new(['pkexec', 'true'], Gio.SubprocessFlags.NONE);
            } catch (e) {
                test.subtitle = `Could not start pkexec: ${e.message}`;
            }
        });
        test.add_suffix(button);
        test.activatable_widget = button;
        group.add(test);

        const note = new Adw.PreferencesGroup({
            description: 'Another polkit agent (for example hyprpolkitagent) can take over the administrator ' +
                'prompt at login; it is then a normal window and this setting does not move it. See the README.',
        });
        page.add(note);

        window.add(page);
        window.connect('close-request', () => {
            settings.disconnect(keyId);
            if (monId)
                monitors.disconnect(monId);
            return false;
        });
    }
}
