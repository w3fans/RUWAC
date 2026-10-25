"""Select the available PRIMARY backend inside one reviewed SSH exec.

No receiver fallback after native identity, capability or permission refusal.
Legacy compatibility is only for a compositor without the native endpoint.
"""
import argparse
import json
import os
import subprocess
import sys

from gi.repository import Gio, GLib
from compositor_identity import identify, verify_bus_peer, Refused
from native_publisher import DESCRIPTION, exact, NativePublisher
from primary_native_peer import NativePeer, desktop_environment
from types import SimpleNamespace


def connect(address):
    loop = GLib.MainLoop()
    cancel = Gio.Cancellable()
    state = dict(waiting=True, connection=None)

    def finished(_source, result):
        try:
            connection = Gio.DBusConnection.new_for_address_finish(result)
            if state['waiting']:
                state['connection'] = connection
            else:
                connection.close(None, NativePublisher._closed_result)
        except GLib.Error:
            pass
        loop.quit()

    def expired():
        state['waiting'] = False
        cancel.cancel()
        loop.quit()
        return False

    timer = GLib.timeout_add(2500, expired)
    Gio.DBusConnection.new_for_address(address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT |
                                      Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
                                      None, cancel, finished)
    loop.run()
    if state['waiting']:
        GLib.source_remove(timer)
    state['waiting'] = False
    if state['connection'] is None:
        raise Refused('Account bus unavailable')
    return state['connection']


def native_available(selected):
    connection = connect(selected['bus'])
    try:
        verify_bus_peer(connection, os.getuid())
        owner = connection.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', ('org.gnome.Shell',)),
            None, Gio.DBusCallFlags.NONE, 500, None).unpack()[0]
        if type(owner) is not bool:
            raise Refused('Invalid compositor metadata')
        if not owner:
            return False
        identity = identify(connection, selected['runtime'], selected['display'])
        try:
            reply = connection.call_sync(identity['owner'], '/org/ruwac/PrimaryPublisher1',
                'org.ruwac.PrimaryPublisher1', 'Describe', None, None,
                Gio.DBusCallFlags.NONE, 500, None).unpack()[0]
        except GLib.Error as error:
            # Endpoint absence is a compatibility boundary. Authentication,
            # access denial, timeout, malformed capabilities and identity errors
            # are NOT absence and must not switch to the legacy GUI receiver.
            if Gio.DBusError.get_remote_error(error) in {
                'org.freedesktop.DBus.Error.UnknownMethod',
                'org.freedesktop.DBus.Error.UnknownObject',
                'org.freedesktop.DBus.Error.UnknownInterface',
            }:
                return False
            raise
        if not isinstance(reply, str) or len(reply) > 4096 or not exact(json.loads(reply), DESCRIPTION):
            raise Refused('Unsupported native publisher')
        # Do not accept metadata from a replaced compositor or listener.
        identify(connection, selected['runtime'], selected['display'], expected=identity)
        return True
    finally:
        connection.close(None, NativePublisher._closed_result)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--desktop', required=True)
    parser.add_argument('--label', required=True)
    args = parser.parse_args()
    try:
        selected = desktop_environment(args.desktop)
        if not native_available(selected):
            import legacy_primary
            sys.argv = ['primary-selection', '--peer', args.desktop]
            legacy_primary.run()
            return 0
        peer = NativePeer(SimpleNamespace(bus=selected['bus'], runtime=selected['runtime'],
                          display=selected['display'], label=args.label),
                          xdisplay=selected['xdisplay'], authority=selected['authority'])
        try:
            peer.loop.run()
        finally:
            peer.stop()
        return peer.exit_code
    except (OSError, ValueError, RuntimeError, GLib.Error, subprocess.SubprocessError):
        # Paths, SSH/account details and arbitrary exceptions stay off stdout.
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
