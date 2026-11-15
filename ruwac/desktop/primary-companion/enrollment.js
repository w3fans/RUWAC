import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';
import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';
import * as Dialog from 'resource:///org/gnome/shell/ui/dialog.js';

const PATH = '/org/ruwac/PrimaryEnrollment1';
const IFACE = `<node><interface name="org.ruwac.PrimaryEnrollment1">
<method name="Describe"><arg type="s" direction="out"/></method>
<method name="Enroll"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
<method name="Cancel"><arg type="b" direction="out"/></method>
</interface></node>`;
const now = () => GLib.get_monotonic_time();
const exact = (value, fields) => value && typeof value === 'object' &&
    !Array.isArray(value) && Object.keys(value).sort().join(',') === fields.sort().join(',');
const bindingKey = binding => JSON.stringify(Object.entries(binding).sort(([a], [b]) => a.localeCompare(b)));

export class EnrollmentManager {
    constructor(connection, receiver, allowed) {
        this.connection = connection;
        this.receiver = receiver;
        this.allowed = allowed;
        this.pending = null;
        this.grant = null;
        this.closed = false;
        this.ownerWatch = connection.signal_subscribe('org.freedesktop.DBus',
            'org.freedesktop.DBus', 'NameOwnerChanged', '/org/freedesktop/DBus',
            null, Gio.DBusSignalFlags.NONE, (_c, _s, _p, _i, _m, params) => {
                const [owner, _old, replacement] = params.deepUnpack();
                if (!replacement) {
                    if (this.pending?.owner === owner)
                        this.finish(false);
                    if (this.grant?.owner === owner)
                        this.grant = null;
                }
            });
        this.timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
            const pending = this.pending;
            if (pending && (!this.allowed() || now() > pending.deadline ||
                !this.receiver.bound(pending.binding)))
                this.finish(false);
            const grant = this.grant;
            if (grant && (!this.allowed() || now() > grant.expires ||
                !this.receiver.bound(grant.binding)))
                this.grant = null;
            return GLib.SOURCE_CONTINUE;
        });
        this.exported = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this.exported.export(connection, PATH);
    }

    reply(invocation, value) {
        try {
            invocation.return_value(new GLib.Variant('(s)', [JSON.stringify(value)]));
        } catch (_error) {
            // A disconnected requester must not retain a prompt or grant.
        }
    }

    DescribeAsync(_params, invocation) {
        this.reply(invocation, {protocol: 1, explicit_desktop_approval: true,
            prompt_timeout_ms: 30000, grant_timeout_ms: 8000, one_use: true});
    }

    EnrollAsync([raw], invocation) {
        try {
            if (this.closed || typeof raw !== 'string' || raw.length > 4096 ||
                !this.allowed() || this.pending || this.grant || this.receiver.lease)
                throw new Error('unavailable');
            const request = JSON.parse(raw);
            if (!exact(request, ['protocol', 'nonce', 'binding', 'label']) || request.protocol !== 1 ||
                typeof request.nonce !== 'string' || !/^[0-9a-f]{48}$/.test(request.nonce) ||
                typeof request.label !== 'string' || !request.label.trim() ||
                new TextEncoder().encode(request.label).length > 160 ||
                /[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/.test(request.label) ||
                new TextDecoder().decode(new TextEncoder().encode(request.label)) !== request.label ||
                !this.receiver.bound(request.binding))
                throw new Error('request');
            const owner = invocation.get_sender();
            const {uid, pid} = this.receiver.credentials(owner);
            if (uid !== Gio.Credentials.new().get_unix_user() || !Number.isSafeInteger(pid) || pid <= 0)
                throw new Error('caller');
            this.pending = {owner, uid, pid, nonce: request.nonce,
                binding: Object.freeze({...request.binding}), invocation,
                deadline: now() + 30000000, dialog: null};
            const dialog = new ModalDialog.ModalDialog({shouldFadeIn: false,
                shouldFadeOut: false, destroyOnClose: false});
            this.pending.dialog = dialog;
            dialog.contentLayout.add_child(new Dialog.MessageDialogContent({
                title: 'Allow RUWAC selection paste?',
                description: `Connection label (provided by the requesting app): ${request.label}\n` +
                    `Local process: ${pid}\n\n` +
                    'Allow this connection to deliver text for deliberate middle-click or ' +
                    'three-finger-tap pastes in this unlocked desktop? This does not read ' +
                    'your clipboard. Approval is temporary and applies only to this connection.',
            }));
            const deny = dialog.addButton({label: 'Deny', key: Clutter.KEY_Escape,
                default: true, action: () => this.finish(false)});
            dialog.addButton({label: 'Allow this connection', action: () => this.finish(true)});
            dialog.setInitialKeyFocus(deny);
            dialog.connect('closed', () => this.finish(false));
            dialog.connect('destroy', () => {
                if (this.pending?.dialog === dialog)
                    this.pending.dialog = null;
                this.finish(false);
            });
            if (!dialog.open())
                this.finish(false);
        } catch (_error) {
            if (this.pending?.invocation === invocation)
                this.finish(false);
            else
                this.reply(invocation, {ok: false});
        }
    }

    finish(accept) {
        const pending = this.pending;
        if (!pending)
            return;
        let approved = false;
        try {
            if (accept && !this.closed && this.allowed() && now() <= pending.deadline &&
                this.pending === pending && this.receiver.bound(pending.binding)) {
                const credentials = this.receiver.credentials(pending.owner);
                approved = credentials.uid === pending.uid && credentials.pid === pending.pid &&
                    this.pending === pending;
            }
        } catch (_error) {
            approved = false;
        }
        // A policy read may already have invalidated and answered this request.
        if (this.pending !== pending)
            return;
        this.pending = null;
        pending.dialog?.close();
        pending.dialog?.destroy();
        if (approved) {
            this.grant = {owner: pending.owner, uid: pending.uid, pid: pending.pid,
                nonce: pending.nonce, binding: pending.binding, expires: now() + 8000000};
            this.reply(pending.invocation, {ok: true, enrolled: true, expires_in_ms: 8000});
        } else {
            this.reply(pending.invocation, {ok: false});
        }
    }

    consume(owner, uid, pid, nonce, binding) {
        const grant = this.grant;
        if (!grant || owner !== grant.owner)
            return false;
        const accepted = !this.closed && this.allowed() && now() <= grant.expires &&
            uid === grant.uid && pid === grant.pid && nonce === grant.nonce &&
            this.receiver.bound(binding) && bindingKey(binding) === bindingKey(grant.binding) &&
            this.grant === grant;
        // Even a failed attempt by the approved caller consumes its grant.
        if (this.grant === grant)
            this.grant = null;
        return accepted;
    }

    CancelAsync(_params, invocation) {
        const owner = invocation.get_sender();
        const matched = this.pending?.owner === owner || this.grant?.owner === owner;
        if (this.pending?.owner === owner)
            this.finish(false);
        if (this.grant?.owner === owner)
            this.grant = null;
        invocation.return_value(new GLib.Variant('(b)', [matched]));
    }

    invalidate() {
        this.grant = null;
        this.finish(false);
    }

    destroy() {
        if (this.closed)
            return;
        this.closed = true;
        this.invalidate();
        this.exported.unexport();
        GLib.source_remove(this.timer);
        this.connection.signal_unsubscribe(this.ownerWatch);
    }
}
