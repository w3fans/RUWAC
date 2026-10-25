#!/usr/bin/env python3
"""Owned compositor key/selection callbacks and real PTYs; never the owner seat."""
import argparse,json,os,re,subprocess,sys,tempfile,time
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
receiver=root/'receiver.py';receiver.write_text('import os,sys,tty\ntty.setraw(0)\nos.write(1,b"SELECT_CLIPBOARD\\r\\nREADY\\r\\n\\x1b[?2004h")\nf=open(sys.argv[1],"ab",buffering=0)\nwhile True:\n b=os.read(0,4096)\n if not b:break\n f.write(b)\n if b==b"H":os.write(1,b"".join(("HISTORY_%03d\\r\\n"%n).encode()for n in range(150)))\n')
interrupt=root/'interrupt.py';interrupt.write_text('import os,sys,signal,tty\ntty.setcbreak(0)\nsignal.signal(signal.SIGINT,lambda *_:open(sys.argv[1],"w").write("SIGINT"))\nos.write(1,b"INTERRUPT_READY\\r\\n")\nwhile True:os.read(0,4096)\n')
mouse_receiver=root/'mouse_receiver.py';mouse_receiver.write_text('import os,sys,tty\ntty.setraw(0)\nf=open(sys.argv[1],"ab",buffering=0)\nos.write(1,b"\\x1b[?1049h\\x1b[?1003h\\x1b[?1006hAPP_MOUSE_READY")\nwhile True:\n b=os.read(0,4096)\n if not b:break\n f.write(b)\n if b"C" in b:os.write(1,b"\\x1b[?1003l\\x1b[?1002l\\x1b[?1000hMODE_CLICK")\n')
try:
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 outputs=[root/'source.input',root/'destination.input'];rows=[rpc('session.create',{'name':'keys-'+str(n),'argv':[sys.executable,str(receiver),str(out)]})for n,out in enumerate(outputs)]
 signalled=root/'signalled';rows.append(rpc('session.create',{'name':'interrupt','argv':[sys.executable,str(interrupt),str(signalled)]}))
 mouse_input=root/'mouse.input';rows.append(rpc('session.create',{'name':'fullscreen-mouse','argv':[sys.executable,str(mouse_receiver),str(mouse_input)]}))
 gui=start([a.gui,'--new-instance','--socket',sock,*sum((['--session',v['session']]for v in rows),[]),'--fixture-dir',root],'GUI')
 eventually(lambda:command('inspect')['tabs']==4,15)
 info=command('inspect',view=0);ox,oy=info['area_origin'];move(ox+30,oy+40);button(272,True);button(272,False);eventually(lambda:command('inspect')['active'])
 command('focus-view',view=0);eventually(lambda:'SELECT_CLIPBOARD'in rpc('pane.capture',{'pane':rows[0]['pane']})['text'])
 def visible_source():
  state=command('history-viewport',view=0)
  for row,cells in enumerate(state.get('cells')or[]):
   text=''.join(c[0]for c in cells)
   if 'SELECT_CLIPBOARD'in text:return row,text.index('SELECT_CLIPBOARD'),state
  return None
 selected_row,selected_col,visible=eventually(visible_source,12)
 info=command('inspect',view=0);(root/'selection-precondition.json').write_text(json.dumps({'inspect':info,'visible':visible,'row':selected_row,'col':selected_col},indent=2)+'\n')
 ox,oy=info['area_origin'];cw,ch=info['cell_metrics'];move(ox+(selected_col+.3)*cw,oy+(selected_row+.5)*ch);button(272,True)
 for _ in range(6):seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(14*cw/6,0.)));time.sleep(.035)
 button(272,False);text=command('copy-state',view=0)['selection'];assert text and 'SELECT'in text,repr(text)
 chord(46);assert not outputs[0].exists()or not outputs[0].read_bytes(), 'Selected Ctrl+C emitted a PTY control byte'
 command('focus-view',view=1);chord(47)
 expected=b'\x1b[200~'+text.encode()+b'\x1b[201~'
 eventually(lambda:outputs[1].exists()and outputs[1].read_bytes()==expected)
 mark('selected-Ctrl-C-and-text-Ctrl-V-use-exact-one-bracketed-payload-without-ETX-or-SYN')
 command('focus-view',view=2);chord(46);eventually(signalled.exists);assert signalled.read_text()=='SIGINT';mark('unselected-Ctrl-C-delivers-real-PTY-SIGINT')
 command('focus-view',view=0);chord(46,shift=True);command('focus-view',view=1);chord(47,shift=True);eventually(lambda:outputs[1].read_bytes()==expected*2);mark('Ctrl-Shift-C-V-remain-available')
 command('chrome',terminal_shortcuts='traditional');eventually(lambda:command('chrome')['terminal_shortcuts']=='traditional')
 time.sleep(.2);command('focus-view',view=1);chord(47);eventually(lambda:outputs[1].read_bytes()==expected*2+b'\x16');mark('traditional-mode-retains-literal-Ctrl-V')
 command('chrome',terminal_shortcuts='desktop');time.sleep(.2)
 # Full-screen mouse applications (including Codex) own their wheel input.
 command('focus-view',view=3);eventually(lambda:'APP_MOUSE_READY'in rpc('pane.capture',{'pane':rows[3]['pane']})['text'])
 def check_app_wheel(label,offset=False):
  info=command('inspect',view=3);ox,oy=info['area_origin'];cw,ch=info['cell_metrics'];cols,grid_rows=info['grid_size'];column=min(8,cols-2);row=min(5,grid_rows-2)
  if offset:assert ox>100,info
  move(ox+(column+.5)*cw,oy+(row+.5)*ch);time.sleep(.1)
  for direction,button_id in [(-30.,64),(30.,65)]:
   before=mouse_input.stat().st_size if mouse_input.exists()else 0
   seat('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,direction,0)));seat('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,direction,1)))
   def wheel_reports():
    if not mouse_input.exists():return None
    return [(int(x),int(y))for b,x,y in re.findall(rb'\x1b\[<(\d+);(\d+);(\d+)M',mouse_input.read_bytes()[before:])if int(b)==button_id]or None
   reports=eventually(wheel_reports,8)
   assert all(point==(column+1,row+1)for point in reports),(reports,{'expected':[column+1,row+1],'origin':[ox,oy],'metrics':[cw,ch]})
  mark(label)
 check_app_wheel('native-fullscreen-wheel-up-and-down-use-exact-widget-local-terminal-coordinates')
 rpc('pane.sendText',{'pane':rows[3]['pane'],'text':'C'});eventually(lambda:'MODE_CLICK'in rpc('pane.capture',{'pane':rows[3]['pane']})['text'])
 check_app_wheel('wheel-position-updates-without-application-motion-reporting')
 command('focus-view',view=0);command('mixed-join',tab=3,vertical=False);command('focus-view',view=3);time.sleep(.4)
 check_app_wheel('fullscreen-wheel-in-offset-right-hand-split-keeps-pane-coordinates',offset=True)
 command('mixed-separate');time.sleep(.2)
 command('focus-view',view=0);rpc('pane.sendText',{'pane':rows[0]['pane'],'text':'H'});eventually(lambda:'HISTORY_149'in rpc('pane.capture',{'pane':rows[0]['pane']})['text'])
 info=command('inspect',view=0);ox,oy=info['area_origin'];move(ox+cw,oy+3*ch)
 seat('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,-30.,0)));seat('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,-30.,1)))
 state=eventually(lambda:(s if (s:=command('history-viewport',view=0)).get('offset',0)>0 and not s['pending']else None))
 assert state['terminal_canvas']and state['focus']and state['lines']>50;mark('first-native-wheel-scrolls-canonical-history-on-same-focused-terminal-canvas')
 command('workspace-close');gui.wait(timeout=8)
 (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'physical_input':False,'owner_desktop_used':False},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 if sock.exists():
  try:rpc('daemon.stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
 print(root,flush=True)
