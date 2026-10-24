#!/usr/bin/env python3
"""Ephemeral PRIMARY-only relay. No clipboard contents, files or passwords logged.

The GUI starts an authenticated --peer using a reviewed SSH route and an exact
remote Wayland session. The peer reads that session's Xwayland PRIMARY bridge;
CLIPBOARD is never read or changed. Both peers exit on EOF/heartbeat expiry.
"""
import json
import hashlib
import os
import re
import signal
import subprocess
import sys
import time

LIMIT = 1024 * 1024
FRAME = LIMIT * 6 + 1024

def trace(event, **fields):
    if os.environ.get("RUWAC_PRIMARY_TRACE") == "1":
        print(json.dumps({"primary_trace":event, **fields}), file=sys.stderr, flush=True)


def session_environment(wayland):
    if not re.fullmatch(r"wayland-[0-9]{1,3}", wayland):
        raise ValueError("Choose an exact remote Wayland session")
    # Read only this SSH account's desktop manager environment. Never choose
    # the first socket or borrow another user's display/authentication file.
    result = subprocess.run(["systemctl", "--user", "show-environment"],
                            capture_output=True, text=True, timeout=5, check=True)
    env = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
    if env.get("WAYLAND_DISPLAY") != wayland:
        raise ValueError("Remote desktop session does not match the configured session")
    display = env.get("DISPLAY", "")
    auth = env.get("XAUTHORITY", "")
    if not re.fullmatch(r":[0-9]{1,5}(?:\.[0-9]+)?", display):
        raise ValueError("The selected desktop has no local Xwayland display")
    import stat
    st = os.lstat(auth)
    if not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o077:
        raise ValueError("Desktop authentication must be an owned private regular file")
    runtime = env.get("XDG_RUNTIME_DIR", "/run/user/" + str(os.getuid()))
    st = os.lstat(runtime)
    if not os.path.isabs(runtime) or not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o077:
        raise ValueError("Desktop runtime must be an owned private directory")
    os.environ.update(DISPLAY=display, XAUTHORITY=auth, XDG_RUNTIME_DIR=runtime)


class Pipe:
    def __init__(self, source, target, receive, stopped):
        self.source, self.target = source, target
        self.receive, self.stopped = receive, stopped
        self.buffer = bytearray()
        self.queue = bytearray()
        self.writer = None
        os.set_blocking(source, False)
        os.set_blocking(target, False)
        self.reader = GLib.io_add_watch(source, GLib.IO_IN | GLib.IO_HUP | GLib.IO_ERR,
                                        self.read)

    def send(self, value):
        data = (json.dumps(value, ensure_ascii=True, separators=(",", ":")) + "\n").encode()
        if len(data) > FRAME or len(self.queue) + len(data) > FRAME * 2:
            self.stopped()
            return
        self.queue.extend(data)
        if self.writer is None:
            self.writer = GLib.io_add_watch(self.target, GLib.IO_OUT | GLib.IO_ERR | GLib.IO_HUP,
                                            self.write)

    def write(self, fd, condition):
        try:
            if condition & (GLib.IO_ERR | GLib.IO_HUP):
                raise BrokenPipeError()
            n = os.write(fd, self.queue)
            del self.queue[:n]
        except BlockingIOError:
            return True
        except OSError:
            self.stopped()
            self.writer = None
            return False
        if self.queue:
            return True
        self.writer = None
        return False

    def read(self, fd, condition):
        try:
            data = os.read(fd, 65536)
            if not data:
                self.stopped()
                return False
            self.buffer.extend(data)
            while b"\n" in self.buffer:
                line, _, rest = self.buffer.partition(b"\n")
                self.buffer = bytearray(rest)
                if len(line) > FRAME:
                    raise ValueError("Frame too large")
                self.receive(json.loads(line))
            if len(self.buffer) > FRAME:
                raise ValueError("Frame too large")
            return True
        except BlockingIOError:
            return True
        except (OSError, ValueError, TypeError, KeyError):
            self.stopped()
            return False


# X11 selections use a terminating zero-length INCR property, not a short pipe
# read, to mark completion. Xlib/ICCCM use native unsigned longs for Atom/Window.
XLIB = None

def xlib():
    global XLIB
    if XLIB is not None:
        return XLIB
    import ctypes as c
    lib = c.CDLL("libX11.so.6")
    d, u, i = c.c_void_p, c.c_ulong, c.c_int
    lib.XInitThreads.argtypes = []; lib.XInitThreads.restype = i
    if not lib.XInitThreads():
        raise ValueError("X11 thread initialization failed")
    prototypes = {
        "XOpenDisplay": ([c.c_char_p], d),
        "XDefaultRootWindow": ([d], u),
        "XCreateSimpleWindow": ([d,u,i,i,c.c_uint,c.c_uint,c.c_uint,u,u],u),
        "XInternAtom": ([d,c.c_char_p,i],u),
        "XSelectInput": ([d,u,c.c_long],i),
        "XConvertSelection": ([d,u,u,u,u,u],i),
        "XDeleteProperty": ([d,u,u],i),
        "XGetSelectionOwner": ([d,u],u),
        "XFlush": ([d],i),
        "XSync": ([d,i],i),
        "XPending": ([d],i),
        "XNextEvent": ([d,c.c_void_p],i),
        "XConnectionNumber": ([d],i),
        "XGetWindowProperty": ([d,u,u,c.c_long,c.c_long,i,u,c.POINTER(u),c.POINTER(i),c.POINTER(u),c.POINTER(u),c.POINTER(c.POINTER(c.c_ubyte))],i),
        "XFree": ([c.c_void_p],i),
        "XDestroyWindow": ([d,u],i),
        "XCloseDisplay": ([d],i),
    }
    for name,(args,result) in prototypes.items():
        getattr(lib,name).argtypes=args;getattr(lib,name).restype=result
    XLIB=lib
    return lib


class SelectionChanges:
    """Track server ownership changes independently of GDK's initial notification."""
    def __init__(self):
        import ctypes as c
        self.lib = xlib()
        self.display = self.lib.XOpenDisplay(os.environ["DISPLAY"].encode())
        if not self.display:
            raise ValueError("Desktop selection display is unavailable")
        self.window = 0
        self.count = 0
        try:
            fixes = c.CDLL("libXfixes.so.3")
            fixes.XFixesQueryExtension.argtypes = [c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_int)]
            fixes.XFixesQueryExtension.restype = c.c_int
            fixes.XFixesQueryVersion.argtypes = [c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_int)]
            fixes.XFixesQueryVersion.restype = c.c_int
            fixes.XFixesSelectSelectionInput.argtypes = [c.c_void_p,c.c_ulong,c.c_ulong,c.c_ulong]
            fixes.XFixesSelectSelectionInput.restype = None
            event, error = c.c_int(), c.c_int()
            major, minor = c.c_int(1), c.c_int(0)
            if not fixes.XFixesQueryExtension(self.display,c.byref(event),c.byref(error)) or not fixes.XFixesQueryVersion(self.display,c.byref(major),c.byref(minor)):
                raise ValueError("Desktop selection change tracking is unavailable")
            self.event = event.value
            self.primary = self.lib.XInternAtom(self.display,b"PRIMARY",0)
            self.window = self.lib.XCreateSimpleWindow(self.display,self.lib.XDefaultRootWindow(self.display),-1,-1,1,1,0,0,0)
            fixes.XFixesSelectSelectionInput(self.display,self.window,self.primary,7)
            self.revision()  # Discard any subscription snapshot before accepting a mode.
        except BaseException:
            self.close()
            raise

    def revision(self):
        import ctypes as c
        class Selection(c.Structure):
            _fields_ = [("type",c.c_int),("serial",c.c_ulong),("send_event",c.c_int),("display",c.c_void_p),("window",c.c_ulong),("subtype",c.c_int),("owner",c.c_ulong),("selection",c.c_ulong),("timestamp",c.c_ulong),("selection_timestamp",c.c_ulong)]
        class Event(c.Union):
            _fields_ = [("type",c.c_int),("selection",Selection),("pad",c.c_long*24)]
        # GDK and this monitor use independent connections. A round trip drains
        # ownership changes already processed before GDK emitted its callback.
        self.lib.XSync(self.display,0)
        for _ in range(4096):
            if not self.lib.XPending(self.display):
                return self.count
            event = Event()
            self.lib.XNextEvent(self.display,c.byref(event))
            if event.type == self.event and not event.selection.send_event and event.selection.selection == self.primary:
                self.count += 1
        raise ValueError("Desktop selection notification limit exceeded")

    def close(self):
        if self.display:
            if self.window:
                self.lib.XDestroyWindow(self.display,self.window)
            self.lib.XCloseDisplay(self.display)
            self.display = None


def read_primary_bytes():
    """A bounded, complete ICCCM transfer over a private unmapped X11 window."""
    import ctypes as c
    import select
    class Selection(c.Structure):
        _fields_=[("type",c.c_int),("serial",c.c_ulong),("send_event",c.c_int),("display",c.c_void_p),("requestor",c.c_ulong),("selection",c.c_ulong),("target",c.c_ulong),("property",c.c_ulong),("time",c.c_ulong)]
    class Property(c.Structure):
        _fields_=[("type",c.c_int),("serial",c.c_ulong),("send_event",c.c_int),("display",c.c_void_p),("window",c.c_ulong),("atom",c.c_ulong),("time",c.c_ulong),("state",c.c_int)]
    class Event(c.Union):
        _fields_=[("type",c.c_int),("selection",Selection),("property",Property),("pad",c.c_long*24)]
    lib=xlib();display=lib.XOpenDisplay(os.environ["DISPLAY"].encode())
    if not display:return None
    window=0;deadline=time.monotonic()+2;data=bytearray();total=0;rejected=False
    try:
        window=lib.XCreateSimpleWindow(display,lib.XDefaultRootWindow(display),-1,-1,1,1,0,0,0)
        prop=lib.XInternAtom(display,b"_RUWAC_PRIMARY_TRANSFER",0)
        primary=lib.XInternAtom(display,b"PRIMARY",0);incr=lib.XInternAtom(display,b"INCR",0)
        target=lib.XInternAtom(display,b"UTF8_STRING",0)
        owner=lib.XGetSelectionOwner(display,primary)
        if not owner:return None
        lib.XSelectInput(display,window,1<<22)
        lib.XConvertSelection(display,primary,target,prop,window,0);lib.XFlush(display)
        fd=lib.XConnectionNumber(display)
        def event():
            while time.monotonic()<deadline:
                if lib.XPending(display):
                    e=Event();lib.XNextEvent(display,c.byref(e));return e
                if not select.select([fd],[],[],max(0,deadline-time.monotonic()))[0]:break
            raise TimeoutError()
        def property_value():
            nonlocal total,rejected
            offset=0;first=True;kind=0;hint=None;property_bytes=0
            while True:
                if time.monotonic()>=deadline:raise TimeoutError()
                actual=c.c_ulong();fmt=c.c_int();items=c.c_ulong();after=c.c_ulong();raw=c.POINTER(c.c_ubyte)()
                status=lib.XGetWindowProperty(display,window,prop,offset,16384,1,0,c.byref(actual),c.byref(fmt),c.byref(items),c.byref(after),c.byref(raw))
                try:
                    if status!=0:raise ValueError("Selection property unavailable")
                    if first:kind=actual.value
                    if actual.value!=kind:raise ValueError("Selection property changed type")
                    if kind==incr:
                        if fmt.value!=32 or items.value!=1 or after.value:raise ValueError("Invalid INCR header")
                        hint=c.cast(raw,c.POINTER(c.c_ulong))[0]&0xffffffff
                        return kind,hint,0
                    if fmt.value!=8:raise ValueError("Selection is not UTF8 text")
                    n=items.value;total+=n;property_bytes+=n
                    if total>LIMIT or after.value>LIMIT-total:
                        rejected=True;data.clear()
                    if not rejected and n:data.extend(c.string_at(raw,n))
                    if not after.value:return kind,None,property_bytes
                    if n==0:raise ValueError("Selection transfer made no progress")
                    offset+=(n+3)//4;first=False
                finally:
                    if raw:lib.XFree(raw)
        while True:
            e=event()
            if e.type==31 and e.selection.requestor==window and e.selection.selection==primary and e.selection.target==target:
                if e.selection.property!=prop:return None
                kind,hint,size=property_value();break
        if kind==incr:
            minimum=hint
            trace("INCR",minimum=minimum)
            if minimum>LIMIT:rejected=True
            # Reading/deleting each whole property acknowledges precisely one
            # chunk. Do not publish until the zero-length terminator is read.
            while True:
                lib.XFlush(display);e=event()
                if e.type!=28 or e.property.window!=window or e.property.atom!=prop or e.property.state!=0:continue
                kind,_,size=property_value()
                if kind==incr:raise ValueError("Nested INCR transfer")
                if size==0:break
            if total<minimum:raise ValueError("Incomplete INCR selection")
        if rejected or lib.XGetSelectionOwner(display,primary)!=owner:return None
        trace("complete",size=len(data))
        return bytes(data)
    except (OSError,ValueError,TimeoutError):
        trace("read-refused")
        return None
    finally:
        if window:lib.XDestroyWindow(display,window)
        lib.XCloseDisplay(display)

class Primary:
    def __init__(self, changed):
        xlib()
        os.environ["GDK_BACKEND"] = "x11"
        Gdk.set_allowed_backends("x11")
        if not Gtk.init_check():
            raise ValueError("Desktop display is unavailable")
        self.display = Gdk.Display.get_default()
        if self.display is None or "X11" not in self.display.__class__.__name__:
            raise ValueError("Mouse selection sharing requires this desktop's Xwayland bridge")
        self.clip = self.display.get_primary_clipboard()
        self.provider = None
        self.generation = 0
        self.cancellable = None
        self.last_digest = None
        self.read_serial = 0
        self.reading = False
        self.queued = None
        self.changed = changed
        self.changes = SelectionChanges()
        self.clip.connect("changed", self.on_change)

    def on_change(self, clip):
        trace("changed", generation=self.generation)
        if self.provider is not None and clip.get_content() == self.provider:
            return
        self.provider = None
        self.generation += 1
        self.last_digest = None
        self.cancel()
        self.changed()

    def cancel(self):
        # Invalidate delivery immediately, but drain an already negotiated X11
        # INCR transfer. Aborting during TARGETS negotiation can stall the owner.
        self.read_serial += 1
        self.queued = None

    def set(self, text):
        if not isinstance(text, str) or not text or len(text.encode("utf-8")) > LIMIT or "\0" in text:
            return
        self.cancel()
        self.generation += 1
        digest = hashlib.sha256(text.encode("utf-8")).digest()
        if self.last_digest == digest and self.provider is not None and self.clip.get_content() == self.provider:
            return
        self.last_digest = digest
        self.provider = Gdk.ContentProvider.new_for_value(text)
        self.clip.set_content(self.provider)
        self.display.flush()

    def read(self, ready):
        if self.reading:
            self.queued = ready
            return
        import threading
        self.reading = True
        serial, generation = self.read_serial, self.generation
        def fetch():
            data = read_primary_bytes()
            def complete():
                self.reading = False
                queued, self.queued = self.queued, None
                if queued:
                    GLib.idle_add(lambda: self.read(queued) or False)
                if data and serial == self.read_serial and generation == self.generation:
                    try:
                        text = data.decode("utf-8")
                    except UnicodeError:
                        return False
                    digest = hashlib.sha256(data).digest()
                    if "\0" not in text and digest != self.last_digest:
                        self.last_digest = digest
                        ready(text)
                return False
            GLib.idle_add(complete)
        threading.Thread(target=fetch, daemon=True).start()


def offer_to_gui(text, session):
    # Publish through the already focused native GUI. A background Xwayland
    # helper cannot claim native Wayland PRIMARY on its own. Never read CLIPBOARD.
    import socket, stat, struct
    data = text.encode("utf-8")
    if not data or len(data) > 128 * 1024 or "\0" in text:
        return False
    path = os.path.join(os.environ["XDG_RUNTIME_DIR"], "ruwac", "control.gui.sock")
    stage = "socket"
    try:
        st = os.lstat(path)
        if not stat.S_ISSOCK(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o077:
            return False
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(1.5)
            client.connect(path)
            pid, uid, _ = struct.unpack("3i", client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            if uid != os.getuid():
                return False
            stage = "session"
            env = dict(item.split(b"=", 1) for item in open("/proc/" + str(pid) + "/environ", "rb").read().split(b"\0") if b"=" in item)
            if env.get(b"WAYLAND_DISPLAY", b"").decode() != session:
                trace("offer-refused", stage=stage)
                return False
            stage = "request"
            request = json.dumps({"version":0, "id":"primary-offer", "method":"clipboard.primary.offer", "params":{"text":text}}, ensure_ascii=True, separators=(",", ":")).encode()
            if len(request) > 1024 * 1024:
                return False
            client.sendall(struct.pack("!I", len(request)) + request)
            def read(n):
                result = bytearray()
                while len(result) < n:
                    chunk = client.recv(n - len(result))
                    if not chunk:
                        raise ValueError("GUI closed")
                    result.extend(chunk)
                return bytes(result)
            size = struct.unpack("!I", read(4))[0]
            if size > 4096:
                return False
            response = json.loads(read(size))
            if response.get("result") != {"offered":True}:
                trace("offer-refused", stage="receiver", code=response.get("error", {}).get("code"))
                return False
        # Success from GDK alone is insufficient: wait for this exact complete
        # selection to cross the compositor's native/Xwayland boundary.
        stage = "compositor"
        deadline = time.monotonic() + 1.5
        while time.monotonic() < deadline:
            if read_primary_bytes() == data:
                return True
            time.sleep(0.02)
    except (OSError, ValueError, UnicodeError, KeyError) as error:
        trace("offer-refused", stage=stage, error_type=type(error).__name__)
    trace("offer-unconfirmed", stage=stage)
    return False


class Peer:
    def __init__(self, source, target, local=False):
        self.epoch = 0
        self.enabled = False
        self.receive_enabled = False
        self.last_heartbeat = time.monotonic()
        self.local = local
        self.primary = Primary(self.changed)
        # Classify the selection event, not the first successful read after a
        # mode change. A fresh selection in this active epoch must supersede
        # another tab's selection even when it is the first one we have read.
        self.changed_epoch = None
        self.mode_revision = self.primary.changes.revision()
        self.allow_offer = False
        self.offering = False
        self.pipe = Pipe(source, target, self.message, loop.quit)
        self.timer = None
        self.dirty = False
        GLib.timeout_add(1000, self.expiry)

    def expiry(self):
        if time.monotonic() - self.last_heartbeat > 10:
            loop.quit()
            return False
        return True

    def changed(self):
        self.dirty = True
        self.changed_epoch = self.epoch if self.enabled and self.primary.changes.revision() > self.mode_revision else None
        if self.timer:
            GLib.source_remove(self.timer)
        def settled():
            self.timer = None
            self.publish()
            return False
        self.timer = GLib.timeout_add(75, settled)

    def publish(self):
        if not self.enabled:
            return
        epoch = self.epoch
        initial = self.changed_epoch != epoch
        self.dirty = False
        def ready(text):
            if self.enabled and self.epoch == epoch:
                self.pipe.send({"type":"text", "epoch":epoch, "initial":initial, "text":text})
        self.primary.read(ready)

    def message(self, value):
        if not isinstance(value, dict):
            raise ValueError("Invalid message")
        kind = value.get("type")
        if kind == "hello" and self.local:
            print('{"status":"ready"}', flush=True)
        elif kind == "mode":
            epoch = value["epoch"]
            if type(epoch) is not int or epoch < self.epoch:
                raise ValueError("Invalid epoch")
            self.last_heartbeat = time.monotonic()
            if epoch != self.epoch:
                self.primary.cancel()
                # A local selection may have replaced our last delivery while
                # this remote canvas was inactive. Equal text is a new copy.
                self.primary.last_digest = None
                self.changed_epoch = None
                self.mode_revision = self.primary.changes.revision()
            self.epoch = epoch
            self.enabled = value["send"] is True
            self.receive_enabled = value["receive"] is True
            self.allow_offer = value.get("paste") is True
        elif kind == "offer":
            text, request_id, epoch = value.get("text"), value.get("id"), value.get("epoch")
            if self.local or not self.allow_offer or epoch != self.epoch or type(request_id) is not int or request_id <= 0 or not isinstance(text, str) or not text or len(text.encode("utf-8")) > 128 * 1024 or "\0" in text:
                raise ValueError("Invalid PRIMARY offer")
            if self.offering:
                self.pipe.send({"type":"offer-result", "epoch":epoch, "id":request_id, "ok":False})
                return
            self.offering = True
            import threading
            def work():
                ok = offer_to_gui(text, sys.argv[2])
                def complete():
                    self.offering = False
                    self.pipe.send({"type":"offer-result", "epoch":epoch, "id":request_id, "ok":ok and self.allow_offer and self.epoch == epoch})
                    return False
                GLib.idle_add(complete)
            try:
                threading.Thread(target=work, daemon=True).start()
            except RuntimeError:
                self.offering = False
                trace("offer-refused", stage="worker")
                self.pipe.send({"type":"offer-result", "epoch":epoch, "id":request_id, "ok":False})
        elif kind == "text":
            if self.receive_enabled and value.get("epoch") == self.epoch:
                self.primary.set(value["text"])
        else:
            raise ValueError("Unknown message")


def run():
    global Gdk, Gio, GLib, Gtk, loop
    if len(sys.argv) < 3 or sys.argv[1] not in ["--peer", "--bridge"]:
        raise ValueError("Expected a private peer or reviewed SSH bridge")
    if sys.argv[1] == "--peer":
        session_environment(sys.argv[2])
    os.environ["GDK_BACKEND"] = "x11"
    import gi
    gi.require_version("Gdk", "4.0")
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gdk, Gio, GLib, Gtk
    loop = GLib.MainLoop()
    signal.signal(signal.SIGTERM, lambda *_: loop.quit())
    signal.signal(signal.SIGINT, lambda *_: loop.quit())
    if sys.argv[1] == "--peer":
        peer = Peer(0, 1)
        peer.pipe.send({"type":"hello"})
        try:
            loop.run()
        finally:
            peer.primary.changes.close()
        return
    argv = json.loads(sys.argv[2])
    if not isinstance(argv, list) or not argv or argv[0] != "ssh" or not all(isinstance(x, str) for x in argv):
        raise ValueError("Expected a reviewed SSH command")
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=sys.stderr if os.environ.get("RUWAC_PRIMARY_TRACE") == "1" else subprocess.DEVNULL)
    local = None
    try:
        local = Peer(child.stdout.fileno(), child.stdin.fileno(), local=True)
        # Peer.send addresses the opposite desktop; local messages require the
        # opposite direction. The same epoch invalidates both pending reads.
        remote_message = local.message
        def remote(value):
            remote_message(value)
        local.pipe.receive = remote
        def control(value):
            if not isinstance(value, dict) or type(value.get("send")) is not bool or type(value.get("receive")) is not bool:
                raise ValueError("Invalid GUI selection policy")
            changed = (local.enabled, local.receive_enabled) != (value["send"], value["receive"])
            if changed:
                local.epoch += 1
                local.primary.cancel()
                local.primary.last_digest = None
            local.enabled = value["send"]
            local.receive_enabled = value["receive"]
            local.last_heartbeat = time.monotonic()
            local.pipe.send({"type":"mode", "epoch":local.epoch,
                             "send":local.receive_enabled, "receive":local.enabled})
            if changed and local.enabled and local.dirty:
                local.publish()
        Pipe(0, 1, control, loop.quit)
        loop.run()
    finally:
        if local is not None:
            local.primary.changes.close()
        child.stdin.close()
        child.terminate()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


if __name__ == "__main__":
    try:
        run()
    except Exception as error:
        trace("unavailable", error_type=type(error).__name__)
        # Remote failures can contain desktop paths/SSH details. The GUI reports
        # a fixed actionable status; never print arbitrary exception contents.
        print('{"status":"unavailable"}', file=sys.stderr, flush=True)
        raise SystemExit(1)
