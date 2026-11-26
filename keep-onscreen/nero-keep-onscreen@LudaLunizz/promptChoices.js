// The choices of the "password prompts open on" list in the settings window.
// Pure logic (no GTK), so tests/test-prompt-choices.mjs can check it with plain gjs.
//
// monitors: [{connector, name, width, height, x, y}] as connected now (logical size/position).
// saved:    the connector in password-prompt-monitor ('' = off).
// Returns {labels, values, selected}: values[i] is what goes into the setting for labels[i].

const ORDINAL = ['1st', '2nd', '3rd'];

function place(m, monitors) {
    if (monitors.length < 2)
        return '';
    // Left to right, top to bottom for monitors in one column.
    const order = [...monitors].sort((a, b) => a.x - b.x || a.y - b.y);
    const i = order.indexOf(m);
    if (i === 0)
        return 'leftmost';
    if (i === order.length - 1)
        return 'rightmost';
    return `${ORDINAL[i] ?? `${i + 1}th`} from the left`;
}

export function monitorLabel(m, monitors) {
    const parts = [m.connector];
    if (m.name)
        parts.push(m.name);
    parts.push(`${m.width}×${m.height}`);
    const where = place(m, monitors);
    if (where)
        parts.push(where);
    return parts.join(' · ');
}

export function promptChoices(monitors, saved) {
    const labels = ['Under the pointer (GNOME default)'];
    const values = [''];
    const named = monitors.filter(m => m.connector);
    for (const m of named) {
        if (values.includes(m.connector))
            continue;
        labels.push(monitorLabel(m, named));
        values.push(m.connector);
    }
    // A saved monitor that is unplugged now stays in the list (and selected), so opening
    // the settings does not silently forget it; prompts open under the pointer meanwhile.
    if (saved && !values.includes(saved)) {
        labels.push(`${saved} (not connected now: prompts open under the pointer)`);
        values.push(saved);
    }
    return {labels, values, selected: values.indexOf(saved ?? '')};
}
