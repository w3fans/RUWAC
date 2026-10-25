// Write-only PRIMARY receiver; enrollment is delegated to the desktop approval manager.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';
import {socketGuard} from './socket_guard.js';

const IFACE = `<node><interface name="org.ruwac.PrimaryPublisher1">
<method name="Describe"><arg type="s" direction="out"/></method>
<method name="Origin"><arg type="s" direction="out"/></method>
<method name="Request"><arg type="s" direction="in"/>
<arg type="s" direction="out"/></method></interface></node>`;
const PATH = '/org/ruwac/PrimaryPublisher1';
const MAX_TEXT = 131072;
const now = () => GLib.get_monotonic_time();
const exact = (q, fields) => Object.keys(q).sort().join(',') === fields.sort().join(',');
const integer = n => Number.isSafeInteger(n) && n > 0;

export class LeaseReceiver {
    constructor(connection, authorize, allowed, diagnostic = () => {}, observeAllowed = allowed) {
        this.diagnostic = diagnostic;
        this.connection = connection;
        this.bound = socketGuard(connection);
        this.authorize = authorize;
        this.allowed = allowed;
        this.observeAllowed = observeAllowed;
        this.lease = null;
        this.revision = 0;
        this.publishing = false;
        this.publishedRevision = null;
        this.selection = global.display.get_selection();
        this.changed = this.selection.connect('owner-changed', (_s, type) => {
            if (type === Meta.SelectionType.SELECTION_PRIMARY) {
                this.revision++;
                this.publishedRevision = this.publishing ? this.revision : null;
            }
        });
        this.ownerWatch = connection.signal_subscribe('org.freedesktop.DBus',
            'org.freedesktop.DBus', 'NameOwnerChanged', '/org/freedesktop/DBus',
            null, Gio.DBusSignalFlags.NONE, (_c, _s, _p, _i, _m, params) => {
                const [name, _old, replacement] = params.deepUnpack();
                if (name === 'org.gnome.Shell' && _old && _old !== replacement)
                    this.lease = null;
                if (this.lease && name === this.lease.owner && !replacement)
                    this.lease = null;
            });
        this.timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
            const lease = this.lease;
            // Policy reads can revoke the lease synchronously. Never dereference
            // mutable this.lease after that read, or revive its captured value.
            if (lease && (!this.allowed() || this.lease !== lease || now() > lease.expires))
                this.lease = null;
            if (this.lease?.pending && now() > this.lease.pending.expires)
                this.lease.pending = null;
            if (this.lease?.committed && now() > this.lease.committed.expires)
                this.lease.committed = null;
            return GLib.SOURCE_CONTINUE;
        });
        this.exported = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this.exported.export(connection, PATH);
    }

    credentials(sender) {
        const call = method => this.connection.call_sync('org.freedesktop.DBus',
            '/org/freedesktop/DBus', 'org.freedesktop.DBus', method,
            new GLib.Variant('(s)', [sender]), null, Gio.DBusCallFlags.NONE,
            500, null).deepUnpack()[0];
        return {uid: call('GetConnectionUnixUser'), pid: call('GetConnectionUnixProcessID')};
    }

    DescribeAsync(_params, invocation) {
        const metadata = {protocol: 1, max_offer_bytes: MAX_TEXT, write_only: true,
            prepare_commit: true, confirm_before_input: true,
            identity_bound: true, revocable: true};
        invocation.return_value(new GLib.Variant('(s)', [JSON.stringify(metadata)]));
    }

    // Metadata only: no selection data is read and no publication is authorized.
    // Account publication permission is independent of observing user selections.
    OriginAsync(_params, invocation) {
        let response = {ok: false};
        try {
            const {uid} = this.credentials(invocation.get_sender());
            if (uid !== new Gio.Credentials().get_unix_user() ||
                !this.observeAllowed() || !Number.isSafeInteger(this.revision))
                throw new Error('observer');
            response = {ok: true, protocol: 1, revision: this.revision,
                published: this.publishedRevision === this.revision};
        } catch (_error) {
            // Never include caller, capability, paths or copied data.
        }
        invocation.return_value(new GLib.Variant('(s)', [JSON.stringify(response)]));
    }

    RequestAsync([raw], invocation) {
        let response = {ok: false};
        let stage = "frame";
        try {
            if (typeof raw !== 'string' || raw.length > MAX_TEXT * 6 + 2048)
                throw new Error('frame');
            const q = JSON.parse(raw);
            stage = "policy";
            if (!this.allowed()) {
                this.lease = null;
                throw new Error('policy');
            }
            if (!q || Array.isArray(q) || typeof q !== 'object')
                throw new Error('frame');
            const owner = invocation.get_sender();
            if (q.op === 'register') {
                stage = 'registration';
                if (!exact(q, ['op', 'nonce', 'binding', 'protocol']) || q.protocol !== 1 || this.lease ||
                    typeof q.nonce !== 'string' || q.nonce.length > 128)
                    throw new Error('registration');
                stage = "credentials";
                const {uid, pid} = this.credentials(owner);
                stage = "authorization";
                if (!this.authorize(owner, uid, pid, q.nonce, q.binding) || !this.bound(q.binding))
                    throw new Error('caller');
                stage = "allocation";
                this.lease = {owner, uid, pid, binding: Object.freeze({...q.binding}),
                    token: GLib.uuid_string_random() + GLib.uuid_string_random(),
                    epoch: 0, active: false, last: 0, pending: null, committed: null,
                    expires: now() + 8000000};
                response = {ok: true, lease: this.lease.token, protocol: 1};
            } else {
                const lease = this.lease;
                if (!lease || owner !== lease.owner || q.lease !== lease.token || now() > lease.expires)
                    throw new Error('lease');
                if (!this.bound(lease.binding)) {
                    this.lease = null;
                    throw new Error('desktop identity');
                }
                if (q.op === 'revoke') {
                    if (!exact(q, ['op', 'lease']))
                        throw new Error('fields');
                    this.lease = null;
                    response = {ok: true};
                } else if (q.op === 'mode') {
                    if (!exact(q, ['op', 'lease', 'epoch', 'active']) ||
                        !integer(q.epoch) || q.epoch <= lease.epoch || typeof q.active !== 'boolean')
                        throw new Error('mode');
                    lease.epoch = q.epoch;
                    lease.active = q.active;
                    lease.pending = null;
                    lease.committed = null;
                    lease.expires = now() + 8000000;
                    response = {ok: true};
                } else {
                    if (!lease.active || q.epoch !== lease.epoch || !integer(q.id))
                        throw new Error('epoch');
                    if (q.op === 'prepare') {
                        if (!exact(q, ['op', 'lease', 'epoch', 'id', 'text']) ||
                            lease.pending || q.id <= lease.last || typeof q.text !== 'string' ||
                            !q.text || q.text.length > MAX_TEXT || q.text.includes('\0') ||
                            new TextEncoder().encode(q.text).length > MAX_TEXT ||
                            new TextDecoder().decode(new TextEncoder().encode(q.text)) !== q.text)
                            throw new Error('offer');
                        lease.last = q.id;
                        lease.committed = null;
                        lease.pending = {id: q.id, text: q.text,
                            revision: this.revision, expires: now() + 750000};
                        response = {ok: true};
                    } else if (q.op === 'confirm') {
                        if (!exact(q, ['op', 'lease', 'epoch', 'id']))
                            throw new Error('fields');
                        const committed = lease.committed;
                        if (!committed || committed.id !== q.id)
                            throw new Error('committed');
                        lease.committed = null;
                        if (now() > committed.expires || this.revision !== committed.revision ||
                            this.publishedRevision !== committed.revision)
                            throw new Error('stale publication');
                        if (!this.allowed() || !this.bound(lease.binding) || this.lease !== lease) {
                            this.lease = null;
                            throw new Error('confirmation identity');
                        }
                        if (now() > committed.expires || this.revision !== committed.revision ||
                            this.publishedRevision !== committed.revision)
                            throw new Error('stale publication');
                        response = {ok: true, confirmed: true};
                    } else if (q.op === 'commit' || q.op === 'cancel') {
                        if (!exact(q, ['op', 'lease', 'epoch', 'id']))
                            throw new Error('fields');
                        const pending = lease.pending ?? (q.op === 'cancel' ? lease.committed : null);
                        if (!pending || pending.id !== q.id)
                            throw new Error('pending');
                        lease.pending = null;
                        if (q.op === 'cancel') {
                            lease.committed = null;
                            response = {ok: true};
                        } else {
                            if (now() > pending.expires || this.revision !== pending.revision)
                                throw new Error('stale');
                            if (!this.allowed() || !this.bound(lease.binding) || this.lease !== lease) {
                                this.lease = null;
                                throw new Error('publication identity');
                            }
                            if (now() > pending.expires || this.revision !== pending.revision)
                                throw new Error('stale');
                            // The mutation is the commit point. Cancellation never restores over a newer owner.
                            const previousRevision = this.revision;
                            this.publishing = true;
                            try {
                                St.Clipboard.get_default().set_text(St.ClipboardType.PRIMARY, pending.text);
                            } finally {
                                this.publishing = false;
                            }
                            if (this.revision <= previousRevision || this.publishedRevision !== this.revision)
                                throw new Error('publication origin');
                            lease.committed = {id: q.id, revision: this.revision,
                                expires: now() + 750000};
                            response = {ok: true, committed: true};
                        }
                    }
                }
            }
        } catch (_error) {
            this.diagnostic({stage, errorType: _error.constructor.name});
            // Fixed metadata only; never include request text, capability or caller exception.
        }
        invocation.return_value(new GLib.Variant('(s)', [JSON.stringify(response)]));
    }

    destroy() {
        this.lease = null;
        this.exported.unexport();
        GLib.source_remove(this.timer);
        this.connection.signal_unsubscribe(this.ownerWatch);
        this.selection.disconnect(this.changed);
    }
}
