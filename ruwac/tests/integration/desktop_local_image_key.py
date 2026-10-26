#!/usr/bin/env python3
"""Owned compositor key/selection callbacks and real PTYs; never the owner seat."""
import argparse,json,os,re,subprocess,sys,tempfile,time,shutil,struct,zlib,hashlib
from pathlib import Path
from gi.repository import Gio,GLib
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,required=True)
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
root=Path(tempfile.mkdtemp(prefix='ergonomics-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2','SHELL':'/bin/bash'}
for key,leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 path=root/leaf;path.mkdir(mode=0o700);env[key]=str(path)
for key in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(key,None)
children=[];seq=0;checks=[];sock=root/'control.sock';session=None
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop'
def start(args,name):
 q=subprocess.Popen(list(map(str,args)),env=env,stdout=(root/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def rpc(method,params=None):
 with Client(sock)as c:return c.call(method,params)
def command(action,**kw):
 global seq
 seq+=1;q=root/'request.tmp';q.write_text(json.dumps(dict(seq=seq,action=action,**kw)));q.replace(root/'gui-request.json')
 def response():
  try:
   d=json.loads((root/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
def mark(name):checks.append(name);print(name,'PASS',flush=True)
def seat(method,args=None):return bus.call_sync(service,session,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
def key(code,down):seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,down)));time.sleep(.04)
def chord(code,shift=False):
 key(29,True)
 if shift:key(42,True)
 key(code,True);key(code,False)
 if shift:key(42,False)
 key(29,False);time.sleep(.15)
def move(x,y):
 seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x),float(y))));time.sleep(.1)
def button(code,down):seat('NotifyPointerButton',GLib.Variant('(ib)',(code,down)));time.sleep(.08)

def peer(action, **kw):
 global seq
 seq+=1;temp=root/'peer-request.tmp';temp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));temp.replace(root/'peer-request.json')
 def response():
  try:
   d=json.loads((root/'peer-response.json').read_text());return d['result'] if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,10)
def focus(who,view=0):
 for _ in range(8):
  if command('inspect')['active']==(who=='gui'):
   if who=='gui':
    info=command('inspect',view=view);ox,oy=info['area_origin'];move(ox+30,oy+40);button(272,True);button(272,False);command('focus-view',view=view)
   return
  key(56,True);key(15,True);key(15,False);key(56,False);time.sleep(.2)
 raise AssertionError('owned compositor focus failed')
def png(path,n):
 def chunk(t,b):return struct.pack('!I',len(b))+t+b+struct.pack('!I',zlib.crc32(t+b))
 raw=b''.join(b'\0'+bytes([n,40,90,255])*8 for _ in range(5))
 path.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',8,5,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
consumer=root/'consumer.py';consumer.write_text("""import os,sys,tty,json,hashlib
from pathlib import Path
tty.setraw(0);r=Path(sys.argv[1]);r.mkdir(exist_ok=True);f=(r/'raw').open('ab',buffering=0);n=0;buf=b''
os.write(1,b'\\x1b[?2004hOpenAI Codex synthetic receiver\\r\\nGPT-6.1-Sol\\r\\n> ')
while True:
 b=os.read(0,8192)
 if not b:break
 f.write(b);buf+=b
 while b'\\x1b[200~'in buf and b'\\x1b[201~'in buf:
  before,tail=buf.split(b'\\x1b[200~',1);path,buf=tail.split(b'\\x1b[201~',1)
  p=Path(path.decode());data=p.read_bytes();assert data.startswith(b'\\x89PNG\\r\\n\\x1a\\n');n+=1
  (r/'image.json').write_text(json.dumps({'count':n,'sha256':hashlib.sha256(data).hexdigest(),'path':str(p)}))
  os.write(1,('\\r\\n[Image #%d]\\r\\nGPT-6.1-Sol\\r\\n> '%n).encode())
""")
# An explicitly synthetic same-user consumer whose executable basename qualifies
# the existing file/process guard. This is not a real provider execution test.
exe=root/'codex';shutil.copy2(sys.executable,exe);exe.chmod(0o700)
stage=root/'stage';stage.mkdir(mode=0o700);rows=[]
try:
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 for n,program in enumerate([exe,sys.executable]):
  folder=root/('consumer'+str(n));folder.mkdir()
  rows.append(rpc('session.create',{'name':'Synthetic image key '+str(n),'argv':[str(program),str(consumer),str(folder)]}))
  rpc('pane.wait',{'pane':rows[-1]['pane'],'contains':'GPT-6.1-Sol','timeout_ms':5000})
 start([a.gui,'--new-instance','--socket',sock,*sum((['--session',v['session']]for v in rows),[]),'--fixture-dir',root,'--staging-dir',stage],'gui')
 eventually(lambda:command('inspect')['tabs']==2,15)
 start([sys.executable,str(Path(__file__).parents[1]/'fixtures/desktop_clipboard.py'),root],'peer')
 eventually(lambda:(root/'peer-ready.json').exists())
 focus('gui');command('tab-select',tab=0);command('focus-view',view=0)
 key(100,True);key(100,False)
 chord(20,shift=True);time.sleep(.3)
 assert command('inspect')['tabs']==2, 'Mapped shortcut opened a host tab'
 eventually(lambda:(root/'consumer0/raw').read_bytes().endswith(b'\x14'))
 mark('MAPPED-CTRL-SHIFT-T-REACHES-CHILD-WITHOUT-OPENING-HOST-TAB')
 key(100,True);key(100,False)
 chord(20,shift=True);eventually(lambda:command('inspect')['tabs']==3)
 command('tab-select',tab=0);command('focus-view',view=0)
 for n,mode in enumerate(['desktop','traditional'],1):
  command('chrome',terminal_shortcuts=mode)
  image=root/('image'+str(n)+'.png');png(image,n)
  focus('peer');peer('set-png',file=image.name);focus('gui')
  chord(47)
  def attached():
   p=root/'consumer0/image.json'
   return p.exists() and json.loads(p.read_text())['count']==n and ('[Image #%d]'%n)in rpc('pane.capture',{'pane':rows[0]['pane'],'lines':100})['text']
  eventually(attached,15)
  receipt=json.loads((root/'consumer0/image.json').read_text());assert receipt['sha256']==hashlib.sha256(image.read_bytes()).hexdigest()
  assert not any(c in (root/'consumer0/raw').read_bytes() for c in [b'\r',b'\n'])
  mark('LOCAL-PNG-CTRL-V-'+mode.upper()+'-GUARDED-DRAFT-NO-SUBMIT')
 focus('peer');peer('set-png',file='image1.png');focus('gui',view=1);command('tab-select',tab=1);command('focus-view',view=1)
 chord(47)
 eventually(lambda:'required'in command('inspect',view=1)['status'],15)
 assert (root/'consumer1/raw').read_bytes()==b''
 mark('FORGED-MODEL-TITLE-CANNOT-AUTHORIZE-IMAGE-INPUT')
finally:
 for child in reversed(children):
  if child.poll() is None:child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
 if session:
  try:seat('Stop')
  except Exception:pass
 (Path(a.evidence)/'acceptance.json').write_text(json.dumps({'PASS':len(checks)==4,'checks':checks,'scope':'Owned compositor, real native keys/GDK clipboard and PTYs; synthetic Codex-named consumer, not provider qualification'},indent=2)+'\n')
