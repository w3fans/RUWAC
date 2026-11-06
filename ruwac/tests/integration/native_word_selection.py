#!/usr/bin/env python3
"""Native double-click word selection, Unicode/wrap and mouse ownership; private seat only."""
import argparse,json,os,subprocess,sys,tempfile,time
from pathlib import Path
import gi
gi.require_version('Gdk','4.0');gi.require_version('Gtk','4.0')
from gi.repository import Gdk,Gio,GLib,Gtk
Gtk.init()
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,type=Path,required=True)
p.add_argument('--baseline',action='store_true');a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=Path(tempfile.mkdtemp(prefix='wrapped-link-',dir=a.evidence));r.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash'}
for name in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:
 q=r/name;q.mkdir(mode=0o700);env[name]=str(q)
for name in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(name,None)
producer=r/'producer.py';producer.write_text("import os,tty\nfrom pathlib import Path\ntty.setraw(0)\np=Path(__file__).with_suffix('.input');p.touch(mode=0o600)\nos.write(1,b'WORD_READY')\nwhile True:\n key=os.read(0,1)\n if not key: break\n modes={b'A':'before alpha_beta-42 after',b'U':'before čebe\\u0301la after',b'C':'before 界 after',b'E':'before 👩\u200d💻 after',b'H':'left\\r\\nright'}\n if key in modes or key in [b'W',b'M']:\n  text=modes.get(key,'before alpha_beta-42 after')\n  if key==b'W':\n   cols=os.get_terminal_size(0).columns;text='x'*(cols-4)+' '+'abcdefgh'\n  mode='\\x1b[?1000h\\x1b[?1006h'if key==b'M'else'\\x1b[?1000l\\x1b[?1006l'\n  os.write(1,(mode+'\\x1b[2J\\x1b[H'+text+'\\r\\n').encode())\n else:\n  with p.open('ab',buffering=0)as out:out.write(key)\n")
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
  try:q=json.loads((r/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
def key(code,pressed):seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,pressed)));time.sleep(.03)
def clipboard_text(primary=False):
 results=[];reader=Gtk.Window();reader.set_default_size(200,100);reader.present()
 def focused():
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  return reader.is_active()
 eventually(focused,8)
 clip=display.get_primary_clipboard()if primary else display.get_clipboard()
 def done(obj,result):results.append(obj.read_text_finish(result))
 clip.read_text_async(None,done)
 def value():
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  return results if results else None
 try:eventually(value,8);return results[0]
 finally:
  reader.close()
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  eventually(lambda:command('inspect')['active'],8)
checks=[]
try:
 seat_path=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 row_id=rpc('session.create',{'name':'Generated wrapped link','argv':[sys.executable,str(producer)]})
 app=start([a.gui,'--new-instance','--socket',sock,'--session',row_id['session'],'--fixture-dir',r],'GUI')
 eventually(lambda:command('inspect')['tabs']==1,15);command('focus-view',view=0)
 key(29,True);key(56,True);key(28,True);key(28,False);key(56,False);key(29,False);time.sleep(.3)
 display=Gdk.Display.get_default();assert display is not None
 def double_at(col,row=0,shift=False):
  info=command('inspect',view=0);assert info['active'];ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
  seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));time.sleep(.03)
  seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(ox+(col+.5)*cw,oy+(row+.5)*ch)));time.sleep(.08)
  if shift:key(42,True)
  for _ in range(2):
   seat('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.04)
   seat('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.06)
  if shift:key(42,False)
  time.sleep(.2)
  return command('copy-state',view=0)['selection']
 cases=[('A',10,0,'alpha_beta'),('U',10,0,'čebe\u0301la'),('C',8,0,'界'),('E',8,0,'👩‍💻'),('W',1,1,'abcdefgh'),('H',2,1,'right')]
 for mode,col,row,expected in cases:
  time.sleep(.6)
  rpc('pane.sendText',{'pane':row_id['pane'],'text':mode})
  def ready_word():
   state=command('history-viewport',view=0);cells=state.get('cells')or[]
   if len(cells)<=row:return None
   lines=[''.join(c[0]for c in line if c[1]!=True)for line in cells]
   if mode=='W':return state if 'defgh' in lines[1] else None
   return state if expected in lines[row] else None
  state=eventually(ready_word,12)
  actual=double_at(col,row)
  if a.baseline:
   assert actual!=expected,{'expected':expected,'actual':actual}
   checks.append('published baseline selects a single cell instead of word')
   break
  assert actual==expected,{'mode':mode,'expected':expected,'actual':actual,'copy':command('copy-state',view=0)}
  assert clipboard_text(primary=True)==expected
  checks.append('Unicode/soft-wrap word '+mode)
 if not a.baseline:
  time.sleep(.6);rpc('pane.sendText',{'pane':row_id['pane'],'text':'M'})
  def app_mouse_enabled():
   with Client(sock)as observer:
    observer.call('session.attach',{'session':row_id['session'],'rows':24,'cols':80,'size_role':'viewer'})
    return any(p['id']==row_id['pane']and p['grid'].get('mouse')for p in observer.call('client.render')['panes'])
  eventually(app_mouse_enabled,3)
  before=producer.with_suffix('.input').read_bytes()
  local=double_at(10)
  after=eventually(lambda:producer.with_suffix('.input').read_bytes()or None,5)
  assert after!=before and b'\x1b[<' in after and local!='alpha_beta'
  checks.append('application owns normal double-click mouse events')
  time.sleep(.6);before=producer.with_suffix('.input').read_bytes()
  assert double_at(10,shift=True)=='alpha_beta'
  assert producer.with_suffix('.input').read_bytes()==before
  checks.append('Shift double click overrides application without injecting mouse')
 command('workspace-close');app.wait(timeout=8)
 (a.evidence/'acceptance.json').write_text(json.dumps(dict(PASS=True,checks=checks,PRIMARY=True,real_PTY_and_native_input=True,physical_input=False,owner_desktop_used=False),indent=2)+'\n')
finally:
 if display:display.close()
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
