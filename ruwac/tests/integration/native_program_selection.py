#!/usr/bin/env python3
"""Native nested application PRIMARY-to-CLIPBOARD promotion; private seat only."""
import argparse,hashlib,json,os,subprocess,sys,tempfile,time
from pathlib import Path
import gi
gi.require_version('Gdk','4.0');gi.require_version('Gtk','4.0')
from gi.repository import Gdk,Gio,GLib,Gtk
Gtk.init()
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,type=Path,required=True)
p.add_argument('--baseline',action='store_true');p.add_argument('--pending-baseline',action='store_true');a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=Path(tempfile.mkdtemp(prefix='program-selection-',dir=a.evidence));r.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash'}
for name in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:
 q=r/name;q.mkdir(mode=0o700);env[name]=str(q)
for name in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(name,None)
producer=r/'producer.py';producer.write_text("import os,tty,base64\nfrom pathlib import Path\ntty.setraw(0)\np=Path(__file__).with_suffix('.input');p.touch(mode=0o600)\ncount=0\nos.write(1,b'\\x1b[?1000h\\x1b[?1006hPROGRAM_SELECTION_READY')\nwhile True:\n key=os.read(0,1)\n if not key: break\n with p.open('ab',buffering=0)as out:out.write(key)\n if key==b'm':\n  count+=1\n  os.write(1,b'\\x1b]52;p;'+base64.b64encode(('nested selected text '+str(count)).encode())+b'\\x07')\n")
proxy=r/'proxy.py';proxy.write_text('''import json,os,socket,struct,threading,time
from pathlib import Path
r=Path(__file__).parent
listener=socket.socket(socket.AF_UNIX);listener.bind(str(r/'gui.sock'));os.chmod(r/'gui.sock',0o600);listener.listen()
def frame(s):
 def read(n):
  data=b''
  while len(data)<n:
   block=s.recv(n-len(data))
   if not block:raise EOFError()
   data+=block
  return data
 header=read(4);n=struct.unpack('>I',header)[0];assert n<=64*1024*1024
 return header+read(n)
def client(down):
 up=socket.socket(socket.AF_UNIX);up.connect(str(r/'control.sock'));pending=set();lock=threading.Lock()
 def pump(src,dst,response):
  try:
   while True:
    data=frame(src);obj=json.loads(data[4:]);ident=json.dumps(obj.get('id'))
    if not response and obj.get('method')=='pane.selection':
     with lock:pending.add(ident)
    if response:
     with lock:hold=ident in pending;pending.discard(ident)
     if hold and (r/'delay-selection').exists():
      (r/'selection-request-pending').touch();end=time.monotonic()+5
      while not (r/'release-selection').exists() and time.monotonic()<end:time.sleep(.01)
     if hold and (r/'fail-selection').exists():
      obj={'version':0,'id':obj['id'],'error':{'code':'STALE_ID','message':'generated selection expired'}}
      payload=json.dumps(obj).encode();data=struct.pack('>I',len(payload))+payload
      (r/'selection-reply-failed').touch()
    dst.sendall(data)
  except (EOFError,OSError):pass
  finally:
   for s in [src,dst]:
    try:s.shutdown(socket.SHUT_RDWR)
    except OSError:pass
    s.close()
 threading.Thread(target=pump,args=(down,up,False),daemon=True).start()
 threading.Thread(target=pump,args=(up,down,True),daemon=True).start()
while True:
 down,_=listener.accept();client(down)
''')
children=[];seq=0;sock=r/'control.sock';seat_path=None;display=None
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop'
def seat(method,args=None):return bus.call_sync(service,seat_path,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
def start(argv,name):
 q=subprocess.Popen(list(map(str,argv)),env=env,stdout=(r/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def rpc(method,args=None):
 with Client(sock)as c:return c.call(method,args)
def command(action,**kw):
 global seq
 seq+=1;tmp=r/'request.tmp';tmp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));tmp.replace(r/'gui-request.json')
 def response():
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  try:q=json.loads((r/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
def key(code,pressed):seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,pressed)));time.sleep(.03)
checks=[]
try:
 seat_path=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 row_id=rpc('session.create',{'name':'Generated nested selection','argv':[sys.executable,str(producer)]})
 start([sys.executable,proxy],'relay');eventually((r/'gui.sock').exists)
 app=start([a.gui,'--new-instance','--socket',r/'gui.sock','--session',row_id['session'],'--fixture-dir',r],'GUI')
 eventually(lambda:command('inspect')['tabs']==1,15);command('focus-view',view=0)
 key(29,True);key(56,True);key(28,True);key(28,False);key(56,False);key(29,False);time.sleep(.3)
 display=Gdk.Display.get_default();assert display is not None
 click_count=0
 def click():
  global click_count,expected
  info=command('inspect',view=0);assert info['active'];ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
  seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)))
  seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(ox+10*cw,oy+ch/2)));time.sleep(.1)
  seat('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.1)
  seat('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.6)
  click_count+=1;expected=hashlib.sha256(('nested selected text '+str(click_count)).encode()).hexdigest()
 def chord(code,shift=False):
  key(29,True)
  if shift:key(42,True)
  key(code,True);key(code,False)
  if shift:key(42,False)
  key(29,False);time.sleep(.2)
 click()
 assert command('copy-state',view=0)['selection']=='', 'application mouse selection became outer grid selection'
 assert command('read-text',primary=True).get('sha256')==expected, 'application PRIMARY offer was not accepted'
 chord(46,shift=True)
 actual=command('read-text',primary=False).get('sha256')
 if a.baseline:
  assert actual!=expected, actual
  checks.append('published baseline cannot copy approved application selection')
 else:
  assert actual==expected, {'PRIMARY_PROMOTION_FAILED':actual}
  checks.append('explicit Ctrl+Shift+C copies approved nested application selection')
  before=producer.with_suffix('.input').read_bytes()
  chord(46)
  assert producer.with_suffix('.input').read_bytes()==before, 'Ctrl+C interrupted the application despite a selection'
  checks.append('Ctrl+C with approved selection does not interrupt application')
  # Alt letters are application keys, including with the workspace strip present.
  for code,char in [(30,b'a'),(45,b'x'),(31,b's')]:
   before=producer.with_suffix('.input').read_bytes()
   key(56,True);key(code,True);key(code,False);key(56,False)
   eventually(lambda:producer.with_suffix('.input').read_bytes()[len(before):]==b'\x1b'+char,4)
  checks.append('native Alt+A/X/S reach PTY as exact Meta bytes')
  before=producer.with_suffix('.input').read_bytes()
  chord(46)
  eventually(lambda:producer.with_suffix('.input').read_bytes()[len(before):]==b'\x03',4)
  checks.append('typing invalidates selection so Ctrl+C reaches application again')
  for shifted in [False,True]:
   (r/'delay-selection').touch();(r/'release-selection').unlink(missing_ok=True);(r/'selection-request-pending').unlink(missing_ok=True)
   click();eventually((r/'selection-request-pending').exists,4)
   before=producer.with_suffix('.input').read_bytes();previous=command('read-text',primary=False)
   chord(46,shift=shifted)
   (r/'release-selection').touch();(r/'delay-selection').unlink()
   if a.pending_baseline:
    eventually(lambda:b'\x03' in producer.with_suffix('.input').read_bytes()[len(before):],4)
    checks.append('previous candidate interrupts application during pending selection')
    break
   eventually(lambda:command('read-text',primary=False).get('sha256')==expected,4)
   assert producer.with_suffix('.input').read_bytes()==before, 'pending selection copy injected an interrupt'
   checks.append('pending application selection completes keyboard copy '+('Ctrl+Shift+C'if shifted else'Ctrl+C'))

  if a.pending_baseline:
   command('workspace-close');app.wait(timeout=8)
   (a.evidence/'acceptance.json').write_text(json.dumps(dict(PASS=True,checks=checks,pending_negative_control=True),indent=2)+'\n')
   raise SystemExit(0)
  (r/'delay-selection').touch();(r/'fail-selection').touch();(r/'release-selection').unlink();(r/'selection-request-pending').unlink()
  click();eventually((r/'selection-request-pending').exists,4)
  before=producer.with_suffix('.input').read_bytes();previous=command('read-text',primary=False)
  chord(46);(r/'release-selection').touch();eventually((r/'selection-reply-failed').exists,4);time.sleep(.4)
  assert command('read-text',primary=False)==previous, 'failed selection changed clipboard'
  chord(46)
  eventually(lambda:producer.with_suffix('.input').read_bytes()[len(before):]==b'\x03',4)
  checks.append('failed selection cancels pending copy and restores application interrupt')
  (r/'fail-selection').unlink();(r/'release-selection').unlink();(r/'selection-request-pending').unlink()
  click();eventually((r/'selection-request-pending').exists,4);chord(46,shift=True)
  # A different native window owns PRIMARY now. Do not revive this view's old text.
  reader=Gtk.Window();reader.set_default_size(200,100);reader.present()
  def focused_reader():
   while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
   return reader.is_active()
  eventually(focused_reader,8)
  # Wayland requires a real input serial before a client can claim a selection.
  key(194,True);key(194,False)
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  display.get_primary_clipboard().set_content(Gdk.ContentProvider.new_for_value('different owner'))
  display.get_clipboard().set_content(Gdk.ContentProvider.new_for_value('clipboard sentinel'))
  for _ in range(10):
   while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
   time.sleep(.02)
  (r/'release-selection').touch();time.sleep(.2)
  reader.close()
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  eventually(lambda:command('inspect')['active'],8);command('focus-view',view=0)
  chord(46,shift=True)
  final=command('read-text',primary=False)
  assert final.get('sha256')==hashlib.sha256(b'clipboard sentinel').hexdigest(), final
  checks.append('lost focus and PRIMARY ownership cancel queued copy and late reply')

 command('workspace-close');app.wait(timeout=8)
 (a.evidence/'acceptance.json').write_text(json.dumps(dict(PASS=True,checks=checks,PRIMARY=True,real_PTY_and_native_input=True,physical_input=False,owner_desktop_used=False),indent=2)+'\n')
finally:
 if seat_path:
  try:seat('Stop')
  except Exception:pass
 if sock.exists():
  try:rpc('daemon.stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
