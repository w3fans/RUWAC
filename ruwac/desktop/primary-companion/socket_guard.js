// Validate this Shell's exact configured listener at publication.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const FIELDS = ['owner', 'pid', 'uid', 'start', 'runtime', 'display', 'device', 'inode', 'listener_inode'].sort();
const ATTRS = 'unix::mode,unix::uid,unix::inode,unix::device';
const info = path => {
    const q = Gio.File.new_for_path(path).query_info(ATTRS, Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, null);
    return {mode: q.get_attribute_uint32('unix::mode'), uid: q.get_attribute_uint32('unix::uid'),
        inode: q.get_attribute_uint64('unix::inode'), device: q.get_attribute_uint32('unix::device')};
};

function text(path, limit) {
    const stream = Gio.File.new_for_path(path).read(null);
    try {
        // Short reads are not EOF. Include one extra byte to detect overflow.
        const chunks = [];
        let length = 0;
        while (length <= limit) {
            const part = stream.read_bytes(Math.min(65536, limit + 1 - length), null).get_data();
            if (!part.length)
                break;
            chunks.push(part);
            length += part.length;
        }
        if (length > limit)
            throw new Error('metadata bound');
        const bytes = new Uint8Array(length);
        let offset = 0;
        for (const part of chunks) {
            bytes.set(part, offset);
            offset += part.length;
        }
        return new TextDecoder('utf-8', {fatal: true}).decode(bytes);
    } finally {
        stream.close(null);
    }
}

export function socketGuard(connection) {
    const bus = (method, name) => connection.call_sync('org.freedesktop.DBus',
        '/org/freedesktop/DBus', 'org.freedesktop.DBus', method,
        new GLib.Variant('(s)', [name]), null, Gio.DBusCallFlags.NONE, 500, null).deepUnpack()[0];
    const owner = connection.get_unique_name();
    const pid = bus('GetConnectionUnixProcessID', owner);
    const uid = bus('GetConnectionUnixUser', owner);
    const runtime = GLib.get_user_runtime_dir();
    const start = () => {
        const q = text('/proc/self/stat', 4096);
        return Number(q.slice(q.lastIndexOf(')') + 1).trim().split(/\s+/)[19]);
    };
    const initialStart = start();
    return pin => {
        try {
            if (!pin || Array.isArray(pin) || typeof pin !== 'object' ||
                Object.keys(pin).sort().join(',') !== FIELDS.join(',') ||
                pin.owner !== owner || pin.pid !== pid || pin.uid !== uid || pin.start !== initialStart ||
                pin.runtime !== runtime || typeof pin.display !== 'string' ||
                !/^wayland-[0-9]{1,3}$/.test(pin.display) ||
                typeof pin.listener_inode !== 'string' || !/^[0-9]{1,20}$/.test(pin.listener_inode) ||
                !Number.isSafeInteger(pin.inode) || !Number.isSafeInteger(pin.device))
                return false;
            const dir = info(runtime);
            if ((dir.mode & 0o170000) !== 0o040000 || dir.uid !== uid || (dir.mode & 0o077))
                return false;
            const path = `${runtime}/${pin.display}`;
            const socket = info(path);
            if ((socket.mode & 0o170000) !== 0o140000 || socket.uid !== uid ||
                socket.device !== pin.device || socket.inode !== pin.inode)
                return false;
            const matching = text('/proc/net/unix', 1048576).split('\n').map(line => line.trim().split(/\s+/))
                .filter(q => q.length === 8 && q[7] === path && (parseInt(q[3], 16) & 0x10000) && parseInt(q[4], 16) === 1);
            if (matching.length !== 1 || matching[0][6] !== pin.listener_inode)
                return false;
            const fds = Gio.File.new_for_path('/proc/self/fd').enumerate_children(
                'standard::name,standard::symlink-target', Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, null);
            let held = false;
            try {
                for (let count = 0; count < 512; count++) {
                    const fd = fds.next_file(null);
                    if (!fd)
                        break;
                    if (fd.get_symlink_target() === `socket:[${pin.listener_inode}]`) {
                        held = true;
                        break;
                    }
                }
            } finally {
                fds.close(null);
            }
            const after = info(path);
            return held && after.device === socket.device && after.inode === socket.inode &&
                start() === initialStart && bus('GetNameOwner', 'org.gnome.Shell') === owner;
        } catch (_error) {
            return false;
        }
    };
}
