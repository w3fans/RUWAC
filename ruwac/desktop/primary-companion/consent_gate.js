// Account consent gate. Connection enrollment remains a separate requirement.
import Gio from 'gi://Gio';
import {PERMISSION_KEY, readPermission} from './consent_state.js';

export class ConsentGate {
    constructor(settings, invalidate) {
        this.settings = settings;
        this.invalidate = invalidate;
        this.uid = Gio.Credentials.new().get_unix_user();
        this.closed = false;
        this.signature = null;
        // Every observed change invalidates capabilities, including re-enabling.
        this.changed = settings.connect(`changed::${PERMISSION_KEY}`, () => {
            this.invalidate();
        });
        // Gio only emits changed for keys read after connecting the handler.
        // Prime the subscription without writing or opting in during startup.
        this.allowed();
    }

    allowed() {
        let enabled = false;
        if (!this.closed) {
            try {
                const current = readPermission(this.settings);
                if (current.signature !== this.signature) {
                    this.signature = current.signature;
                    this.invalidate();
                }
                enabled = current.enabled;
            } catch (_error) {
                // Missing/unreadable consent is refusal, never an implicit opt-in.
            }
        }
        if (!enabled)
            this.invalidate();
        return enabled;
    }

    authorize(_owner, uid, pid, _nonce) {
        return this.allowed() && uid === this.uid &&
            Number.isSafeInteger(pid) && pid > 0;
    }

    destroy() {
        if (this.closed)
            return;
        this.closed = true;
        this.settings.disconnect(this.changed);
        this.invalidate();
    }
}
