import Adw from 'gi://Adw?version=1';
import {PERMISSION_KEY, readPermission, writePermission} from './consent_state.js';

export function consentPage(settings) {
    const page = new Adw.PreferencesPage({title: 'Mouse selection',
        icon_name: 'edit-paste-symbolic'});
    const group = new Adw.PreferencesGroup({title: 'Desktop permission',
        description: 'Off by default. Enable only on a desktop where you want ' +
            'RUWAC to deliver text for deliberate middle-click or three-finger-tap pastes.'});
    const row = new Adw.SwitchRow({title: 'Allow mouse-selection publication',
        subtitle: 'Applies to processes running as your desktop user. Each RUWAC ' +
            'connection also needs separate permission. Works only while this ' +
            'GNOME desktop is unlocked; it does not read your clipboard.'});
    let refreshing = false;
    const refresh = () => {
        refreshing = true;
        try {
            row.set_active(readPermission(settings).enabled);
        } finally {
            refreshing = false;
        }
    };
    const changed = settings.connect(`changed::${PERMISSION_KEY}`, refresh);
    refresh(); // Subscribe before reading; construction never writes permission.
    row.connect('notify::active', () => {
        if (!refreshing)
            writePermission(settings, row.get_active());
    });
    // This candidate page is created for each preferences window, never reused.
    page.connect('unrealize', () => settings.disconnect(changed));
    group.add(row);
    group.add(new Adw.ActionRow({title: 'Turning this off revokes current registrations',
        subtitle: 'Turning it back on requires fresh enrollment. Your existing ' +
            'selection and Ctrl+C / Ctrl+V clipboard are preserved.'}));
    page.add(group);
    return page;
}
