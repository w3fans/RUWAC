#!/usr/bin/env python3
"""Real native selection/clipboard of TUI-wrapped URLs; generated private seat only."""
import argparse,json,os,subprocess,sys,tempfile,time
from pathlib import Path
import gi
gi.require_version('Gdk','4.0');gi.require_version('Gtk','4.0')
from gi.repository import Gdk,Gio,GLib,Gtk
Gtk.init()
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,type=Path,required=True)
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=Path(tempfile.mkdtemp(prefix='wrapped-link-',dir=a.evidence));r.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash'}
for name in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:
 q=r/name;q.mkdir(mode=0o700);env[name]=str(q)
for name in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(name,None)
producer=r/'producer.py';producer.write_text(r'''import os,tty
url='https://github.com/w3fans/dPDF/'
tail='actions/runs/41968233752'
tty.setraw(0);os.write(1,b'LINK_READY')
while True:
 mode=os.read(0,1)
 if not mode:break
 if mode not in [b'U',b'P']:continue
 cols=os.get_terminal_size(0).columns
 prefix='x'*max(0,cols-len(url)-len(' from ')-2)+' from '
 style='\x1b[36;4m'if mode==b'U'else''
 reset='\x1b[0m'if mode==b'U'else''
 text='\x1b[2J\x1b[H'+prefix+style+url+reset+'\r\n  '+style+tail+reset+' (the message above the jobs).\r\n'
 os.write(1,text.encode())
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
 row=rpc('session.create',{'name':'Generated wrapped link','argv':[sys.executable,str(producer)]})
 app=start([a.gui,'--new-instance','--socket',sock,'--session',row['session'],'--fixture-dir',r],'GUI')
 eventually(lambda:command('inspect')['tabs']==1,15);command('focus-view',view=0)
 key(29,True);key(56,True);key(28,True);key(28,False);key(56,False);key(29,False);time.sleep(.3)
 display=Gdk.Display.get_default();assert display is not None
 uri='https://github.com/w3fans/dPDF/';tail='actions/runs/41968233752';suffix=' (the message above the jobs).'
 for styled in [True,False]:
  rpc('pane.sendText',{'pane':row['pane'],'text':'U'if styled else'P'})
  def ready():
   state=command('history-viewport',view=0);cells=state.get('cells')or[]
   if len(cells)<2:return None
   lines=[''.join(c[0]for c in cells[i]if c[1]!=True)for i in range(2)]
   if uri not in lines[0]or tail not in lines[1]:return None
   actual=(cells[0][lines[0].index('https://')][6].get('underline')=='single')if len(cells[0][lines[0].index('https://')])>6 else False
   return (state,lines)if actual==styled and len(cells[0])-len(lines[0].rstrip())==2 else None
  state,lines=eventually(ready,15) # Producer uses explicit CRLF; unit regression independently pins wrapped=False.
  info=command('inspect',view=0);ox,oy=info['area_origin'];cw,ch=info['cell_metrics'];first=lines[0].index('from ');last=lines[1].index(suffix)+len(suffix)-1
  seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));time.sleep(.04);seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(ox+(first+.25)*cw,oy+.5*ch)));time.sleep(.08)
  seat('NotifyPointerButton',GLib.Variant('(ib)',(272,True)))
  for _ in range(6):seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',((last-first)*cw/6,ch/6)));time.sleep(.03)
  seat('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.15)
  expected='from '+uri+(''if styled else'\n  ')+tail+suffix
  (a.evidence/('styled-state.json'if styled else'plain-state.json')).write_text(json.dumps(dict(state=state,lines=lines,copy=command('copy-state',view=0),expected=expected)))
  assert command('copy-state',view=0)['selection']==expected,command('copy-state',view=0)
  assert clipboard_text(primary=True)==expected
  key(29,True);key(46,True);key(46,False);key(29,False)
  assert clipboard_text()==expected
  checks.append('styled URL joins only its display margin'if styled else'plain hard newline and indentation preserved')
 command('workspace-close');app.wait(timeout=8)
 (a.evidence/'acceptance.json').write_text(json.dumps(dict(PASS=True,checks=checks,PRIMARY_and_CLIPBOARD=True,real_PTY_and_native_input=True,physical_input=False,owner_desktop_used=False),indent=2)+'\n')
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
