"""Exact GNOME bus/listener identity, not offer authorization."""
import os
import re
import stat
import time
from pathlib import Path

from gi.repository import Gio, GLib


class Refused(ValueError):
    pass


def verify_bus_peer(connection, uid):
    """Authenticate the local transport before trusting bus credential replies."""
    try:
        stream = connection.get_stream()
        if not isinstance(stream, Gio.SocketConnection):
            raise Refused("bus transport is not a Unix socket")
        sock = stream.get_socket()
        if sock.get_family() != Gio.SocketFamily.UNIX:
            raise Refused("bus transport is not a Unix socket")
        credentials = sock.get_credentials()
        if credentials is None or credentials.get_unix_user() != uid:
            raise Refused("wrong bus peer uid")
    except Refused:
        raise
    except (AttributeError, TypeError, OSError, GLib.Error):
        raise Refused("bus peer credentials unavailable") from None


def identify(connection, runtime, display, *, expected=None, uid=None):
    """Fail closed; return metadata only. Caller provides the exact bus/runtime."""
    deadline = time.monotonic() + 2
    uid = os.getuid() if uid is None else uid
    verify_bus_peer(connection, uid)
    runtime = Path(runtime)
    if not runtime.is_absolute() or not re.fullmatch(r"wayland-[0-9]{1,3}", display):
        raise Refused("invalid explicit desktop")
    rstat = runtime.lstat()
    if not stat.S_ISDIR(rstat.st_mode) or rstat.st_uid != uid or rstat.st_mode & 0o077:
        raise Refused("invalid runtime identity")
    path = runtime / display

    def bounded_text(path, limit):
        if time.monotonic() >= deadline:
            raise Refused("desktop identity deadline")
        with path.open("rb") as stream:
            value = stream.read(limit + 1)
        if len(value) > limit:
            raise Refused("desktop identity metadata bound")
        return value.decode("utf-8", errors="strict")

    def socket_identity():
        row = path.lstat()
        if not stat.S_ISSOCK(row.st_mode) or row.st_uid != uid:
            raise Refused("invalid socket identity")
        return row.st_dev, row.st_ino

    def bus(method, signature, args):
        if time.monotonic() >= deadline:
            raise Refused("desktop identity deadline")
        return connection.call_sync(
            "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
            method, GLib.Variant(signature, args), None, Gio.DBusCallFlags.NONE,
            max(1, min(500, int((deadline-time.monotonic())*1000))), None,
        ).unpack()[0]

    def process(pid):
        directory = Path("/proc") / str(pid)
        if directory.stat().st_uid != uid:
            raise Refused("wrong compositor uid")
        # comm may contain spaces and parentheses; fields after the final ')' start at3.
        value = bounded_text(directory / "stat", 4096).rsplit(")", 1)[1].split()
        return int(value[19])

    try:
        socket_before = socket_identity()
        owner = bus("GetNameOwner", "(s)", ("org.gnome.Shell",))
        if not re.fullmatch(r":[0-9]+\.[0-9]+", owner):
            raise Refused("invalid bus owner")
        pid = bus("GetConnectionUnixProcessID", "(s)", (owner,))
        if bus("GetConnectionUnixUser", "(s)", (owner,)) != uid:
            raise Refused("wrong bus uid")
        start = process(pid)
        listeners = []
        for line in bounded_text(Path("/proc/net/unix"), 1048576).splitlines()[1:]:
            fields = line.split(maxsplit=7)
            if len(fields) == 8 and fields[7] == str(path):
                if int(fields[3], 16) & 0x10000 and int(fields[4], 16) == 1:
                    listeners.append(fields[6])
        if len(listeners) != 1:
            raise Refused("ambiguous or absent listener")
        wanted = "socket:[" + listeners[0] + "]"
        found = False
        for count, fd in enumerate((Path("/proc") / str(pid) / "fd").iterdir()):
            if count >= 4096 or time.monotonic() >= deadline:
                raise Refused("desktop listener lookup bound")
            try:
                if os.readlink(fd) == wanted:
                    found = True
                    break
            except FileNotFoundError:
                continue
        if not found:
            raise Refused("bus owner does not hold configured listener")
        if process(pid) != start or socket_identity() != socket_before:
            raise Refused("desktop identity changed during inspection")
        if bus("GetNameOwner", "(s)", ("org.gnome.Shell",)) != owner:
            raise Refused("bus owner changed during inspection")
        result = dict(owner=owner, pid=pid, uid=uid, start=start,
                      runtime=str(runtime), display=display,
                      device=socket_before[0], inode=socket_before[1],
                      listener_inode=listeners[0])
        if expected is not None and result != expected:
            raise Refused("registered desktop identity changed")
        return result
    except Refused:
        raise
    except (OSError, ValueError, IndexError, UnicodeError, GLib.Error):
        raise Refused("desktop identity unavailable") from None
