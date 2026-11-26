// Small GNOME Shell helpers shared by the Nero extensions. core/build.sh copies
// this file into each extension directory (like nero.js); edit it here.

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
