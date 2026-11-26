// Small GNOME Shell helpers shared by the Nero extensions. core/build.sh copies
// this file into each extension directory (like nero.js); edit it here.
import GLib from 'gi://GLib';

// A logger that tags every line with the extension's name.
export function makeLog(tag) {
    return msg => console.log(`[${tag}] ${msg}`);
}

// Connector name (for example "DP-1") of the logical monitor with this index,
// or `fallback` when there is no such monitor.
export function connectorOf(index, fallback = null) {
    const lm = global.backend.get_monitor_manager().get_logical_monitors().find(l => l.get_number() === index);
    return lm?.get_monitors()[0]?.get_connector() ?? fallback;
}

// Monitor index for a connector name, or -1 if it is empty or not connected.
export function monitorForConnector(connector) {
    return connector ? global.backend.get_monitor_manager().get_monitor_for_connector(connector) : -1;
}

// Everything an extension (or one part of it) has connected or scheduled, in one
// place, so disable() is `registry.destroy()` instead of a list of ids to undo.
//   connect(obj, signal, fn)   like obj.connect(); the id is remembered
//   disconnect(obj, id)        one handler;  disconnectObject(obj): all of this registry's handlers on obj
//   timeout(ms, fn[, prio])    like GLib.timeout_add; fn's return value works as in GLib (SOURCE_CONTINUE keeps it;
//                              anything else, including nothing, ends it). timeoutSeconds(s, fn[, prio]) likewise.
//   later(fn)                  once before the next redraw (needs {later: {add, remove}} at construction: the
//                              Meta.later_add / compositor.get_laters() call differs between GNOME versions)
//   remove(id)                 cancel a timeout or later that has not run yet (ids that already ran are ignored)
//   destroy()                  undo everything; the registry then ignores new work.
// {tag} names the extension in the log. A callback that throws is logged and ends its source, so one bug does not repeat every tick.
export class Registry {
    constructor({later = null, tag = 'nero'} = {}) {
        this._later = later;
        this._tag = tag;
        this._conns = new Map(); // object -> [ids]
        this._sources = new Map(); // id -> remove function
        this._dead = false;
    }

    connect(obj, signal, fn) {
        const id = obj.connect(signal, fn);
        if (this._dead) {
            this._tryDisconnect(obj, id);
            return 0;
        }
        const ids = this._conns.get(obj);
        if (ids)
            ids.push(id);
        else
            this._conns.set(obj, [id]);
        return id;
    }

    disconnect(obj, id) {
        const ids = this._conns.get(obj);
        const i = ids ? ids.indexOf(id) : -1;
        if (i < 0)
            return;
        ids.splice(i, 1);
        if (!ids.length)
            this._conns.delete(obj);
        this._tryDisconnect(obj, id);
    }

    disconnectObject(obj) {
        for (const id of this._conns.get(obj) ?? [])
            this._tryDisconnect(obj, id);
        this._conns.delete(obj);
    }

    timeout(ms, fn, priority = GLib.PRIORITY_DEFAULT) {
        return this._source(fn, run => GLib.timeout_add(priority, ms, run), GLib.Source.remove);
    }

    timeoutSeconds(seconds, fn, priority = GLib.PRIORITY_DEFAULT) {
        return this._source(fn, run => GLib.timeout_add_seconds(priority, seconds, run), GLib.Source.remove);
    }

    later(fn) {
        if (!this._later)
            throw new Error('Registry: later() needs {later: {add, remove}}');
        return this._source(fn, run => this._later.add(run), id => this._later.remove(id));
    }

    remove(id) {
        const remove = this._sources.get(id);
        if (!remove)
            return;
        this._sources.delete(id);
        remove(id);
    }

    destroy() {
        this._dead = true;
        for (const [obj, ids] of this._conns)
            ids.forEach(id => this._tryDisconnect(obj, id));
        this._conns.clear();
        for (const [id, remove] of this._sources)
            remove(id);
        this._sources.clear();
    }

    _tryDisconnect(obj, id) {
        try {
            obj.disconnect(id);
        } catch {
            // the object is already gone
        }
    }

    _source(fn, add, remove) {
        if (this._dead)
            return 0;
        let id = 0;
        id = add(() => {
            // out of the table before fn runs: destroy() or remove(id) inside fn must not
            // cancel the source that is running right now
            this._sources.delete(id);
            let again = false;
            try {
                again = fn() === GLib.SOURCE_CONTINUE;
            } catch (e) {
                logError(e, `[${this._tag}] callback`);
            }
            if (again && !this._dead) {
                this._sources.set(id, remove);
                return GLib.SOURCE_CONTINUE;
            }
            return GLib.SOURCE_REMOVE;
        });
        this._sources.set(id, remove);
        return id;
    }
}
