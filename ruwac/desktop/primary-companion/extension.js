import Gio from 'gi://Gio';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {ConsentGate} from './consent_gate.js';
import {LeaseReceiver} from './lease_receiver.js';
import {EnrollmentManager} from './enrollment.js';

export default class PrimaryExtension extends Extension {
    enable() {
        try {
            const invalidate = () => {
                if (this.receiver)
                    this.receiver.lease = null;
                this.enrollment?.invalidate();
            };
            this.consent = new ConsentGate(this.getSettings(), invalidate);
            const sessionAllowed = () => Main.sessionMode.currentMode === 'user' &&
                !Main.sessionMode.isLocked && !Main.sessionMode.isGreeter;
            const allowed = () => this.consent.allowed() && sessionAllowed();
            this.receiver = new LeaseReceiver(Gio.DBus.session,
                (owner, uid, pid, nonce, binding) =>
                    this.enrollment?.consume(owner, uid, pid, nonce, binding) === true,
                allowed, undefined, sessionAllowed);
            this.enrollment = new EnrollmentManager(Gio.DBus.session, this.receiver, allowed);
        } catch (error) {
            this.disable();
            throw error;
        }
    }

    disable() {
        this.enrollment?.destroy();
        this.enrollment = null;
        this.receiver?.destroy();
        this.receiver = null;
        this.consent?.destroy();
        this.consent = null;
    }
}
