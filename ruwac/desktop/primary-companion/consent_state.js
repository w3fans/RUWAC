// Each explicit action writes one atomic state and generation.
import GLib from 'gi://GLib';

export const PERMISSION_KEY = 'publication-permission';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export function readPermission(settings) {
    try {
        const [enabled, generation] = settings.get_value(PERMISSION_KEY).deepUnpack();
        if (typeof enabled !== 'boolean' || typeof generation !== 'string' ||
            (generation !== '' && !UUID.test(generation)) || (enabled && generation === ''))
            return {enabled: false, signature: 'invalid'};
        return {enabled, generation, signature: JSON.stringify([enabled, generation])};
    } catch (_error) {
        return {enabled: false, signature: 'unavailable'};
    }
}

export function writePermission(settings, enabled) {
    if (typeof enabled !== 'boolean')
        throw new Error('Invalid permission action');
    const generation = GLib.uuid_string_random();
    if (!settings.set_value(PERMISSION_KEY, new GLib.Variant('(bs)', [enabled, generation])))
        throw new Error('Permission action refused');
    return generation;
}
