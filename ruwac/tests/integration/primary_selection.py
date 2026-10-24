#!/usr/bin/env python3
"""Two independent X11 desktops: real GDK PRIMARY transfer, separate CLIPBOARD.

All displays, SSH stand-in and peers are private descendants. The SSH stand-in
executes the exact peer command; authentic external SSH/RDP acceptance is separate.
"""
import argparse,json,os,re,select,shlex,signal,subprocess,sys,tempfile,time,threading
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--helper',required=True);p.add_argument('--evidence',required=True);a=p.parse_args()
root=Path(tempfile.mkdtemp(prefix='primary-bridge-',dir=a.evidence));root.chmod(0o700)
helper=Path(a.helper).resolve();children=[];checks=[];stop=threading.Event();modes={'send':True,'receive':True};lock=threading.Lock();bridge=None
peer_source=r'''import gi,json,sys,faulthandler,signal,os
faulthandler.register(signal.SIGUSR1,all_threads=True)
from pathlib import Path
gi.require_version('Gdk','3.0')
gi.require_version('Gtk','3.0')
from gi.repository import Gdk,GLib,Gtk
Gtk.init()
r=Path(sys.argv[1]);display=Gdk.Display.get_default();primary=Gtk.Clipboard.get_for_display(display,Gdk.SELECTION_PRIMARY);clip=Gtk.Clipboard.get_for_display(display,Gdk.SELECTION_CLIPBOARD);loop=GLib.MainLoop();last=0;providers=[]
native=os.environ.get('RUWAC_PRIMARY_NATIVE_PEER')=='1'
if native:
 window=Gtk.Window();view=Gtk.TextView();scroller=Gtk.ScrolledWindow();scroller.set_policy(Gtk.PolicyType.AUTOMATIC,Gtk.PolicyType.AUTOMATIC);scroller.add(view);window.add(scroller);window.set_default_size(700,450);window.show_all();view.grab_focus()
def poll():
 global last
 p=r/'command.json'
 if not p.exists():return True
 d=json.loads(p.read_text())
 if d['seq']==last:return True
 last=d['seq'];(r/'phase.json').write_text(json.dumps({'seq':last,'phase':'begin','action':d['action']}));target=primary if d['which']=='primary' else clip
 def done(value):(r/'response.json').write_text(json.dumps({'seq':d['seq'],'text':value}))
 if d['action']=='state':
  done({'active':window.is_active(),'focused':view.has_focus(),'height':window.get_allocated_height(),'backend':display.__gtype__.name})
 elif d['action']=='set':
  if native and d['which']=='primary':
   buffer=view.get_buffer();buffer.set_text(d['text']);buffer.select_range(buffer.get_start_iter(),buffer.get_end_iter())
  else:target.set_text(d['text'],-1)
  (r/'phase.json').write_text(json.dumps({'seq':last,'phase':'flush'}));display.flush();done(True)
 else:
  target.request_text(lambda clipboard,text,*args:done(text))
 return True
GLib.timeout_add(15,poll);(r/'ready').touch();loop.run()
'''
def wait(test,timeout=8):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  v=test()
  if v:return v
  time.sleep(.04)
 raise AssertionError('Private selection bridge condition timed out')
def mark(name):checks.append(name);print(name,'PASS',flush=True)
seq=0
def command(peer,action,which='primary',text=None):
 global seq
 seq+=1;n=seq;q=root/peer;d={'seq':n,'action':action,'which':which,'text':text};tmp=q/'pending';tmp.write_text(json.dumps(d));tmp.replace(q/'command.json')
 def response():
  try:
   value=json.loads((q/'response.json').read_text());return value if value['seq']==n else None
  except (OSError,ValueError):return None
 try:return wait(response)['text']
 except Exception:
  for q in [root/'local',root/'remote']:
   pidfile=q/'pid'
   if pidfile.exists():
    try:os.kill(int(pidfile.read_text()),signal.SIGUSR1)
    except ProcessLookupError:pass
  time.sleep(.1)
  raise
def matches(peer,text):return command(peer,'read')==text
def mode(send,receive):
 with lock:modes.update(send=send,receive=receive);bridge.stdin.write((json.dumps(modes)+'\n').encode());bridge.stdin.flush()
 time.sleep(.35)
def check_peer_epochs():
 # Exercise the exact production peer protocol with controlled completion order.
 # The separate native desktops below still prove real selection/INCR transport.
 import importlib.util,types
 spec=importlib.util.spec_from_file_location('primary_epoch_fixture',helper)
 module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 class Primary:
  def __init__(self,changed):self.readers=[];self.last_digest=None;self.owner_revision=0;self.changes=types.SimpleNamespace(revision=lambda:self.owner_revision)
  def cancel(self):pass
  def read(self,ready):self.readers.append(ready)
 class Pipe:
  def __init__(self,*args):self.sent=[]
  def send(self,value):self.sent.append(value)
 module.Primary=Primary;module.Pipe=Pipe;module.loop=types.SimpleNamespace(quit=lambda:None)
 module.GLib=types.SimpleNamespace(timeout_add=lambda *args:1,source_remove=lambda *args:None)
 peer=module.Peer(0,1)
 def mode_at(epoch,send):peer.message({'type':'mode','epoch':epoch,'send':send,'receive':False})
 def complete(text):peer.primary.readers.pop(0)(text)
 mode_at(1,True);peer.changed();peer.publish();complete('INITIAL_GDK_NOTIFICATION')
 assert peer.pipe.sent[-1]=={'type':'text','epoch':1,'initial':True,'text':'INITIAL_GDK_NOTIFICATION'}
 peer.primary.owner_revision+=1;peer.changed();peer.publish();complete('FIRST_ACTIVE_SELECTION')
 assert peer.pipe.sent[-1]=={'type':'text','epoch':1,'initial':False,'text':'FIRST_ACTIVE_SELECTION'}
 mode_at(2,False);peer.primary.owner_revision+=1;peer.changed();mode_at(3,True);peer.publish();complete('BACKGROUND_SNAPSHOT')
 assert peer.pipe.sent[-1]=={'type':'text','epoch':3,'initial':True,'text':'BACKGROUND_SNAPSHOT'}
 peer.primary.owner_revision+=1;peer.changed();peer.publish();complete('NEW_SELECTION_ON_RETURN')
 assert peer.pipe.sent[-1]=={'type':'text','epoch':3,'initial':False,'text':'NEW_SELECTION_ON_RETURN'}
 peer.changed();peer.publish();count=len(peer.pipe.sent);mode_at(4,False);complete('LATE_REPLY')
 assert len(peer.pipe.sent)==count
 peer.publish();assert not peer.primary.readers
 mark('peer-epochs-distinguish-deliberate-selections-from-background-and-late-replies')
try:
 check_peer_epochs()
 displays=[]
 for name in ['local','remote']:
  q=root/name;q.mkdir()
  read,write=os.pipe();log=(q/'xvfb.log').open('wb')
  child=subprocess.Popen(['Xvfb','-displayfd',str(write),'-screen','0','1000x700x24','-nolisten','tcp','-ac'],pass_fds=[write],stdout=log,stderr=log);children.append(child);os.close(write)
  # Xvfb may write the digits and newline separately. Closing after the
  # first read can give its second write EPIPE and kill a ready server.
  number=b'';deadline=time.monotonic()+5
  try:
   while not number.endswith(b'\n'):
    remaining=deadline-time.monotonic();assert remaining>0 and select.select([read],[],[],remaining)[0],'Xvfb display handshake timed out'
    chunk=os.read(read,32-len(number));assert chunk and len(number)+len(chunk)<=32,'Invalid Xvfb display handshake';number+=chunk
   assert re.fullmatch(rb'[0-9]+\n',number),'Invalid Xvfb display number';display=':'+number.decode().strip()
  finally:os.close(read)
  displays.append(display)
  program=q/'peer.py';program.write_text(peer_source);env={**os.environ,'GDK_BACKEND':'x11','DISPLAY':display,'PYTHONDONTWRITEBYTECODE':'1','NO_AT_BRIDGE':'1','GTK_A11Y':'none'}
  child=subprocess.Popen([sys.executable,str(program),str(q),display],env=env,stdout=(q/'peer.log').open('wb'),stderr=subprocess.STDOUT);children.append(child);(q/'pid').write_text(str(child.pid));wait(lambda:(q/'ready').exists())
 auth=root/'auth';auth.touch(mode=0o600)
 fake=root/'bin';fake.mkdir()
 systemctl=fake/'systemctl';systemctl.write_text('#!/usr/bin/python3\nprint('+repr('WAYLAND_DISPLAY=wayland-77\nDISPLAY='+displays[1]+'\nXAUTHORITY='+str(auth))+')\n');systemctl.chmod(0o700)
 ssh=fake/'ssh';ssh.write_text('#!/usr/bin/python3\nimport os,sys\nos.execl("/bin/sh","sh","-c",sys.argv[-1])\n');ssh.chmod(0o700)
 remote='python3 -c '+shlex.quote(helper.read_text())+' --peer wayland-77'
 env={**os.environ,'NO_AT_BRIDGE':'1','GTK_A11Y':'none','RUWAC_PRIMARY_TRACE':'1','PATH':str(fake)+':'+os.environ['PATH'],'DISPLAY':displays[0],'GDK_BACKEND':'x11','PYTHONDONTWRITEBYTECODE':'1'}
 bridge=subprocess.Popen([sys.executable,str(helper),'--bridge',json.dumps(['ssh',remote])],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(root/'bridge.log').open('wb'));children.append(bridge)
 assert select.select([bridge.stdout],[],[],8)[0];assert json.loads(bridge.stdout.readline())['status']=='ready'
 def heartbeat():
  while not stop.wait(.5):
   try:
    with lock:bridge.stdin.write((json.dumps(modes)+'\n').encode());bridge.stdin.flush()
   except OSError:return
 thread=threading.Thread(target=heartbeat,daemon=True);thread.start();mode(True,True)
 command('local','set','clipboard','LOCAL_CTRL_C');command('remote','set','clipboard','REMOTE_CTRL_C')
 text='REMOTE_SELECTED_Ž_中文_🦀\nsecond line'
 command('remote','set',text=text);wait(lambda:matches('local',text));mark('remote-primary-to-local-unicode-multiline')
 command('local','set',text='LOCAL_SELECTED');wait(lambda:matches('remote','LOCAL_SELECTED'));mark('local-primary-to-remote')
 assert command('local','read','clipboard')=='LOCAL_CTRL_C';assert command('remote','read','clipboard')=='REMOTE_CTRL_C';mark('both-Ctrl-C-clipboards-preserved')
 mode(False,False);command('remote','set',text='INACTIVE_REMOTE');time.sleep(.4);assert matches('local','LOCAL_SELECTED');mark('inactive-remote-selection-refused')
 command('local','set',text='LOCAL_WHILE_OUTSIDE_RDP');mode(True,True);wait(lambda:matches('remote','LOCAL_WHILE_OUTSIDE_RDP'));mark('local-selection-forwarded-on-return-to-RDP')
 mode(False,True);command('local','set',text='SEND_DISABLED');time.sleep(.4);assert matches('remote','LOCAL_WHILE_OUTSIDE_RDP');command('remote','set',text='RECEIVE_ONLY');wait(lambda:matches('local','RECEIVE_ONLY'));mark('direction-policy-receive-only')
 bulk='large_Ž_line\n'*12000
 command('remote','set',text=bulk);wait(lambda:matches('local',bulk));mark('large-multiline-selection')
 # An independent xsel owner forces the ICCCM INCR branch. Read the exact
 # peer protocol directly so the diagnostic X11 publisher is not involved.
 mode(False,False)
 direct=subprocess.Popen([sys.executable,str(helper),'--peer','wayland-77'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(root/'INCR-peer.log').open('wb'));children.append(direct)
 os.set_blocking(direct.stdout.fileno(),False);pending=bytearray()
 def frame():
  end=time.monotonic()+8
  while time.monotonic()<end:
   if b'\n' in pending:
    line,_,rest=pending.partition(b'\n');pending[:]=rest;return json.loads(line)
   if select.select([direct.stdout],[],[],max(0,end-time.monotonic()))[0]:
    data=os.read(direct.stdout.fileno(),65536);assert data;pending.extend(data);assert len(pending)<7*1024*1024
  raise AssertionError('INCR peer frame timed out')
 assert frame()['type']=='hello';direct.stdin.write(b'{"type":"mode","epoch":1,"send":true,"receive":false}\n');direct.stdin.flush();time.sleep(.25)
 # Drain initialization ownership notifications, then re-enter the same
 # desktop with a new epoch while its existing selection remains unchanged.
 # The first subsequent deliberate selection must replace another tab's text.
 while b'\n' in pending or select.select([direct.stdout],[],[],0)[0]:frame()
 direct.stdin.write(b'{"type":"mode","epoch":2,"send":true,"receive":false}\n');direct.stdin.flush();time.sleep(.15)
 incr_text='INCR_transfer_line\n'*30000;incr_file=root/'INCR.txt';incr_file.write_text(incr_text)
 with incr_file.open('rb') as stream:
  owner=subprocess.Popen(['xsel','--primary','--input','--nodetach'],stdin=stream,env={**env,'DISPLAY':displays[1]},stdout=(root/'xsel.log').open('wb'),stderr=subprocess.STDOUT);children.append(owner)
 for _ in range(4):
  value=frame();assert value['type']=='text' and value['epoch']==2
  if value['text']==incr_text:
   assert value['initial'] is False,'First deliberate selection was mislabeled as an initial snapshot'
   break
 else:raise AssertionError('INCR data did not match the independent owner')
 assert '"primary_trace": "INCR"' in (root/'INCR-peer.log').read_text()
 mark('first-deliberate-remote-selection-is-not-a-stale-initial-snapshot')
 direct.stdin.close();assert direct.wait(timeout=5)==0;mark('independent-INCR-owner-complete-transfer-and-terminator')
 mode(False,True)
 command('remote','set',text='x'*(1024*1024+1));time.sleep(.7);assert matches('local',bulk);mark('oversized-selection-refused')
 command('remote','set',text='FRESH_AFTER_OVERSIZE');wait(lambda:matches('local','FRESH_AFTER_OVERSIZE'));mark('fresh-selection-after-refusal')
 stop.set();thread.join(timeout=2);bridge.stdin.close();assert bridge.wait(timeout=6)==0;mark('EOF-stops-relay-and-SSH')
 (root/'acceptance.json').write_text(json.dumps({'checks':checks,'PASS':True,'transport':'private SSH stand-in; exact production helper, two real independent Xvfb/GDK desktops','physical_input':False,'external_RDP':False,'Wayland_native_selection':False},indent=2)+'\n')
finally:
 stop.set()
 for child in reversed(children):
  if child.poll()is None:child.terminate()
 for child in reversed(children):
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
 print(root,flush=True)
