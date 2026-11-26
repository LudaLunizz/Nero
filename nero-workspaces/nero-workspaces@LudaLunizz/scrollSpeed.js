// The two-finger scroll speed plugin file, shared by the extension and its
// settings window (so a new speed is on disk at once, even before the shell has
// the extension that knows the setting). The Lua itself comes from the Rust core
// (core/src/touchpad.rs, core/src/scroll_speed.lua); Mutter loads it at login.
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import {call} from './nero.js';

export const PLUGIN_NAME = 'nero-scroll-speed.lua';

export function pluginFile() {
    return Gio.File.new_for_path(GLib.build_filenamev([GLib.get_user_config_dir(), 'libinput', 'plugins', PLUGIN_NAME]));
}

// Write the plugin for `percent` (0..100), or remove it at 100 %. Throws on errors.
export function writeScrollPlugin(percent) {
    const file = pluginFile();
    const lua = call('scrollPlugin', percent);
    if (lua === null) {
        if (file.query_exists(null))
            file.delete(null);
        return;
    }
    try {
        file.get_parent().make_directory_with_parents(null);
    } catch {
        // exists
    }
    if (file.query_exists(null)) {
        const [, old] = file.load_contents(null);
        if (new TextDecoder().decode(old) === lua)
            return;
    }
    file.replace_contents(new TextEncoder().encode(lua), null, false, Gio.FileCreateFlags.REPLACE_DESTINATION, null);
}
