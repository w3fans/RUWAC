#!/usr/bin/env python3
"""Native nested-session copy to the desktop CLIPBOARD; private seat only.

GUI -> outer engine pane -> nested `ruwac -r inner` client -> inner engine.
Checks that the selection made in a nested bash pane, and an application's
OSC 52 CLIPBOARD write backed by a click, reach the desktop CLIPBOARD as read
by another Wayland client, and that an unsolicited OSC 52 write does not.
--inner-binary/--outer-binary run the inner/outer engine from another (e.g. preserved older)
release; its OSC 52 CLIPBOARD result is then recorded, not asserted.
"""
import argparse,json,os,subprocess,sys,tempfile,time
from pathlib import Path
import gi
gi.require_version('Gdk','4.0');gi.require_version('Gtk','4.0')
from gi.repository import Gdk,Gio,GLib,Gtk
Gtk.init()
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,type=Path,required=True)
p.add_argument('--inner-binary',type=Path);p.add_argument('--outer-binary',type=Path);p.add_argument('--baseline',action='store_true')
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
inner_binary=a.inner_binary or a.binary;outer_binary=a.outer_binary or a.binary;legacy_inner=a.inner_binary is not None
r=Path(tempfile.mkdtemp(prefix='nested-clipboard-',dir=a.evidence));r.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash','TERM':'xterm-256color'}
for name in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:
 q=r/name;q.mkdir(mode=0o700);env[name]=str(q)
for name in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(name,None)
(r/'rc').write_text("PS1='$ '\nunset PROMPT_COMMAND\n")
producer=r/'producer.py';producer.write_text("""import base64,os,sys,time,tty
tty.setraw(0)
os.write(1,b'\\x1b[?1000h\\x1b[?1006hOSC52_PRODUCER_READY\\r\\n')
def offer(text):os.write(1,b'\\x1b]52;c;'+base64.b64encode(text.encode())+b'\\x07')
if sys.argv[1:]==['unsolicited']:
 time.sleep(1.5);offer('UNSOLICITED clipboard write')
count=0;pending=b''
while True:
 data=os.read(0,256)
 if not data:break
 pending+=data
 while b'\\x1b[<' in pending:
  start=pending.index(b'\\x1b[<');end=min([i for i in(pending.find(b'M',start),pending.find(b'm',start))if i>=0],default=-1)
  if end<0:break
  report=pending[start:end+1];pending=pending[end+1:]
  if report.endswith(b'm'):
   count+=1;offer('osc52 clipboard text '+str(count))
""")
children=[];seq=0;outer=r/'outer'/'c.sock';inner=r/'inner'/'c.sock';seat_path=None;display=None
for q in [outer,inner]:q.parent.mkdir(mode=0o700)
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop'
def seat(method,args=None):return bus.call_sync(service,seat_path,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
def start(argv,name):
 q=subprocess.Popen(list(map(str,argv)),env=env,stdout=(r/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def rpc(sock,method,args=None):
 with Client(sock)as c:return c.call(method,args)
def pump():
 while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
def command(action,**kw):
 global seq
 seq+=1;tmp=r/'request.tmp';tmp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));tmp.replace(r/'gui-request.json')
 def response():
  pump()
  try:q=json.loads((r/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
def key(code,pressed):seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,pressed)));time.sleep(.03)
def chord(code,shift=False):
 key(29,True)
 if shift:key(42,True)
 key(code,True);key(code,False)
 if shift:key(42,False)
 key(29,False);time.sleep(.3)
def clipboard_text(primary=False):
 """The desktop selection as another native client reads it."""
 results=[];reader=Gtk.Window();reader.set_default_size(200,100);reader.present()
 def focused():
  pump();return reader.is_active()
 eventually(focused,8)
 clip=display.get_primary_clipboard()if primary else display.get_clipboard()
 def done(obj,result):
  try:results.append(obj.read_text_finish(result))
  except GLib.Error:results.append(None)
 clip.read_text_async(None,done)
 def value():
  pump();return results if results else None
 try:eventually(value,8);return results[0]
 finally:
  reader.close();pump()
  eventually(lambda:command('inspect')['active'],8);command('focus-view',view=0);time.sleep(.2)
def set_clipboard(text):
 """Another native client owns CLIPBOARD with `text` (a real input serial is required)."""
 reader=Gtk.Window();reader.set_default_size(200,100);reader.present()
 def focused():
  pump();return reader.is_active()
 eventually(focused,8);key(194,True);key(194,False);pump()
 display.get_clipboard().set_content(Gdk.ContentProvider.new_for_value(text))
 for _ in range(10):
  pump();time.sleep(.02)
 reader.close();pump()
 eventually(lambda:command('inspect')['active'],8);command('focus-view',view=0);time.sleep(.2)
def screen_lines():
 cells=command('history-viewport',view=0).get('cells')or[]
 return[''.join(c[0]for c in line if c[1]!=True)for line in cells]
def locate(text):
 def found():
  for row,line in enumerate(screen_lines()):
   col=line.find(text)
   if col>=0:return(row,col)
 return eventually(found,12)
def pointer_to(col,row):
 info=command('inspect',view=0);assert info['active'];ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
 seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));time.sleep(.03)
 seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(ox+(col+.5)*cw,oy+(row+.5)*ch)));time.sleep(.08)
 return cw
def drag(text,shift=False):
 row,col=locate(text);cw=pointer_to(col,row)
 if shift:key(42,True)
 seat('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.08)
 steps=len(text)-1
 for _ in range(steps):
  seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(cw,0.)));time.sleep(.02)
 time.sleep(.1);seat('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.1)
 if shift:key(42,False)
 time.sleep(.6)
def click(text):
 row,col=locate(text);pointer_to(col,row)
 seat('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.08)
 seat('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.8)
checks=[];failures=[];observed={}
def check(name,ok,detail):
 observed[name]=detail
 if ok:checks.append(name)
 else:failures.append(name)
try:
 seat_path=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 start([inner_binary,'--socket',inner,'serve'],'inner-daemon');eventually(inner.exists)
 inner_ids=rpc(inner,'session.create',{'name':'inner','argv':['/bin/bash','--rcfile',str(r/'rc'),'-i']})
 start([outer_binary,'--socket',outer,'serve'],'outer-daemon');eventually(outer.exists)
 outer_ids=rpc(outer,'session.create',{'name':'outer','argv':[str(a.binary),'--socket',str(inner),'-r','inner']})
 app=start([a.gui,'--new-instance','--socket',outer,'--session',outer_ids['session'],'--fixture-dir',r],'GUI')
 eventually(lambda:command('inspect')['tabs']==1,15);command('focus-view',view=0)
 key(29,True);key(56,True);key(28,True);key(28,False);key(56,False);key(29,False);time.sleep(.3)
 display=Gdk.Display.get_default();assert display is not None
 observed['inner_engine']=subprocess.run([str(inner_binary),'--version'],capture_output=True,text=True).stdout.strip()
 rpc(inner,'pane.sendText',{'pane':inner_ids['pane'],'text':'clear; echo NESTCOPY_alpha_beta; echo SHIFTCOPY_delta_eps\r'})
 locate('SHIFTCOPY_delta_eps')

 set_clipboard('clipboard sentinel 1')
 drag('NESTCOPY_alpha_beta');chord(46,shift=True)
 got=clipboard_text()
 check('nested bash drag then Ctrl+Shift+C sets desktop CLIPBOARD',got=='NESTCOPY_alpha_beta',{'clipboard':got,'primary':clipboard_text(primary=True),'buffer':rpc(inner,'buffer.get',{'session':inner_ids['session']}).get('text')})

 # The view owns CLIPBOARD now. An unfocused data-control client (wl-copy, as
 # Claude Code uses natively) replaces it; the focused view must paste the new text.
 pasted=r/'pasted.txt'
 rpc(inner,'pane.sendText',{'pane':inner_ids['pane'],'text':'clear; echo READ_PASTE; IFS= read -r line; printf %s "$line" > '+str(pasted)+'\r'})
 locate('READ_PASTE');time.sleep(.3)
 q=subprocess.run(['wl-copy','external unfocused copy 1'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10);time.sleep(.8)
 chord(47,shift=True);time.sleep(.6)
 rpc(inner,'pane.sendText',{'pane':inner_ids['pane'],'text':'\r'})
 got=eventually(lambda:pasted.read_text() if pasted.exists() else None,8)
 check('focused view pastes text an unfocused client copied after the view',got=='external unfocused copy 1',{'pasted':got,'wl_copy_exit':q.returncode,'desktop':clipboard_text()})
 rpc(inner,'pane.sendText',{'pane':inner_ids['pane'],'text':'clear; echo NESTCOPY_alpha_beta; echo SHIFTCOPY_delta_eps\r'})
 locate('SHIFTCOPY_delta_eps')

 set_clipboard('clipboard sentinel 2')
 drag('SHIFTCOPY_delta_eps',shift=True);chord(46,shift=True)
 got=clipboard_text()
 check('nested Shift+drag then Ctrl+Shift+C sets desktop CLIPBOARD',got=='SHIFTCOPY_delta_eps',{'clipboard':got})

 set_clipboard('clipboard sentinel 3')
 rpc(inner,'pane.sendText',{'pane':inner_ids['pane'],'text':'clear; python3 '+str(producer)+' unsolicited\r'})
 locate('OSC52_PRODUCER_READY');time.sleep(3.5)
 got=clipboard_text()
 check('unsolicited application OSC 52 CLIPBOARD write is ignored',got=='clipboard sentinel 3',{'clipboard':got})

 click('OSC52_PRODUCER_READY')
 got=clipboard_text()
 name='application OSC 52 CLIPBOARD write after a click reaches desktop CLIPBOARD'
 if legacy_inner:
  observed['legacy_inner_osc52_clipboard']=got;checks.append('legacy inner engine OSC 52 CLIPBOARD result recorded')
 else:check(name,got=='osc52 clipboard text 1',{'clipboard':got})

 command('workspace-close');app.wait(timeout=8)
 if a.baseline:
  assert failures,{'baseline_unexpectedly_passed':checks}
  result=dict(PASS=True,baseline=True,baseline_failures=failures,checks=checks,observed=observed)
 else:
  assert not failures,{'failures':failures,'observed':observed}
  result=dict(PASS=True,checks=checks,observed=observed,legacy_inner=legacy_inner,real_PTY_and_native_input=True,physical_input=False,owner_desktop_used=False)
 (a.evidence/'acceptance.json').write_text(json.dumps(result,indent=2)+'\n')
 print(f'Nested clipboard PASS {len(checks)} checks')
finally:
 (a.evidence/'nested-clipboard-observed.json').write_text(json.dumps(dict(checks=checks,failures=failures,observed=observed),indent=2)+'\n')
 if display:display.close()
 if seat_path:
  try:seat('Stop')
  except Exception:pass
 for sock in [outer,inner]:
  if sock.exists():
   try:rpc(sock,'daemon.stop')
   except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
