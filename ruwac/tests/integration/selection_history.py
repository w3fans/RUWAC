#!/usr/bin/env python3
"""Native history drag, edge scrolling, exact PRIMARY and typing cancellation."""
from pathlib import Path
import argparse,hashlib,json,os,re,subprocess,sys,tempfile,time
from gi.repository import Gio,GLib
from spike import Client,eventually
parser=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:parser.add_argument('--'+name,required=True)
parser.add_argument("--history-delay-ms",type=int)
parser.add_argument("--truncate-history",action="store_true")
parser.add_argument("--cancel-selection",choices=["focus","typing","timeout"])
a=parser.parse_args()
if a.history_delay_ms is not None:assert 0<=a.history_delay_ms<=6000
assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test','Requires owned compositor'
r=Path(tempfile.mkdtemp(prefix='selection-history-',dir=a.evidence));r.chmod(0o700)
binary=Path(a.binary).resolve();gui=Path(a.gui).resolve()
env={**os.environ,'TERM':'xterm-256color','GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2'}
for k in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(k,None)
proxy=None;children=[];seq=0;results={};bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';seat=None
def start(args,name,e):
 with(r/(name+'.log')).open('wb')as log:p=subprocess.Popen(list(map(str,args)),env=e,stdout=log,stderr=subprocess.STDOUT)
 children.append(p);return p
def rpc(sock,m,p=None):
 with Client(sock)as c:return c.call(m,p)
def control(m,args=None):return bus.call_sync(service,seat,service+'.Session',m,args,None,Gio.DBusCallFlags.NONE,5000,None)
def key(code,down):control('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,down)))
def command(root,action,**kw):
 global seq
 seq+=1;tmp=root/'request.tmp';tmp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));tmp.replace(root/'gui-request.json')
 def read():
  try:
   d=json.loads((root/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 deadline=time.monotonic()+8
 while time.monotonic()<deadline:
  result=read()
  if result is not None:return result
  time.sleep(.001)
 raise RuntimeError('GUI fixture response timeout')
def text(root):return '\n'.join(''.join(c[0]for c in row)for row in command(root,'history-viewport')['cells'])
receiver=r/'echo.py';receiver.write_text('import os,tty\ntty.setraw(0)\nfor i in range(60):os.write(1,("\\x1b[32mROW%04d \\x1b[0m"%i+"abcdefghijklmNOPQRSTUVWXYZ0123456789 "*5+"\\r\\n").encode())\nos.write(1,b"\\r\\nBENCH_READY\\r\\n\\x1b[?1000h\\x1b[?1006h")\ni=0\nwhile True:\n b=os.read(0,1)\n if not b:break\n i+=1\n os.write(1,("ACK%04d\\r\\n"%i).encode())\n')
try:
 seat=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];control('Start');control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 root=r/'selection';root.mkdir(mode=0o700);e=dict(env)
 for k,leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
  d=root/leaf;d.mkdir(mode=0o700);e[k]=str(d)
 e['RUWAC_STATE_FILE']=str(root/'state.json');sock=root/'ctl'
 server=start([binary,'--socket',sock,'serve'],'daemon',e);eventually(sock.exists)
 row=rpc(sock,'session.create',{'name':'owned-selection','argv':[sys.executable,str(receiver)]})
 gui_socket=sock
 if a.history_delay_ms is not None:
  from history_delay import HistoryDelay
  gui_socket=root/'proxy';proxy=HistoryDelay(gui_socket,sock,a.history_delay_ms/1000,truncated=a.truncate_history)
 app=start([gui,'--new-instance','--socket',gui_socket,'--session',row['session'],'--allow-workspace-control','--fixture-dir',root],'GUI',e)
 eventually(lambda:command(root,'inspect').get('target'),15);command(root,'chrome',width=1200,height=850,maximized=True);command(root,'focus-view',view=0);eventually(lambda:'BENCH_READY'in text(root),15)
 eventually(lambda:command(root,'chrome')['maximized']);time.sleep(.2);info=command(root,'inspect');ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
 position=[0.,0.];control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)))
 def move(x,y):
  control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x)-position[0],float(y)-position[1])));position[:]=[float(x),float(y)];time.sleep(.08)
 if proxy is not None:
  if a.cancel_selection or a.truncate_history:
   # Wayland clipboard ownership needs a real input serial, not only a
   # synthetic fixture command. Shift itself sends no bytes to the PTY.
   key(42,True);key(42,False);time.sleep(.05)
   command(root,'copy',primary=True);eventually(lambda:command(root,'copy-state')['visible'])
   command(root,'focus-view',view=0);command(root,'copy',primary=True)
   def primary_seed():
    value=command(root,'read-text',primary=True)
    (r/'primary-seed.json').write_text(json.dumps(value)+'\n')
    return value.get('sha256')
   prior_primary=eventually(primary_seed)
   command(root,'switch',view=0);eventually(lambda:command(root,'history-viewport')['focus'])
   (r/'primary-after-seed-hide.json').write_text(json.dumps(command(root,'read-text',primary=True))+'\n')
  visible_markers=set(re.findall(r'ROW[0-9]{4}',text(root)))
  assert visible_markers,'Fixture starting screen has no row markers'
  # All native events are submitted without an artificial held-button delay.
  # Delay injection happens after the real daemon has frozen its canonical data.
  key(42,True);move(ox+10*cw,oy+6*ch)
  control('NotifyPointerButton',GLib.Variant('(ib)',(272,True)))
  # Keep the pointer inside the canvas so GDK focus remains on this terminal.
  control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(5*cw,-6*ch)))
  for _ in range(3):
   control('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,-30.,0)))
   control('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,-30.,1)))
  control('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));key(42,False)
  eventually(proxy.frozen.is_set,5)
  if a.truncate_history:
   eventually(lambda:not command(root,'history-viewport')['pending'],8)
   assert command(root,'copy-state')['selection']=='','Truncated prefix was selected'
   assert '8 MiB limit' in command(root,'inspect')['status'],'Inline cap lacks visible feedback'
   assert command(root,'inspect')['tab_status']=='ReadyWithWarning','Live terminal with advisory shown as failed'
   command(root,'switch',view=0,text='z')
   eventually(lambda:'ACK'in rpc(sock,'pane.capture',{'pane':row['pane']})['text'])
   assert command(root,'inspect')['tab_status']=='ReadyWithWarning','Healthy terminal lost its warning/health distinction'
   assert command(root,'read-text',primary=True).get('sha256')==prior_primary,'Truncated prefix replaced PRIMARY'
   (r/'acceptance.json').write_text(json.dumps({'PASS':True,'truncated_refused':True,'primary_unchanged':True,'scope':'Real worker8MiB accumulation cap, synthetic sub1MiB RPC pages behind actual frozen daemon snapshot','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'gui_sha256':hashlib.sha256(gui.read_bytes()).hexdigest()},indent=2)+'\n')
   raise SystemExit(0)
  if a.cancel_selection:
   assert a.history_delay_ms>=500,'Cancellation must precede the delayed reply'
   if a.cancel_selection=='focus':
    command(root,'about');eventually(lambda:not command(root,'history-viewport')['focus'])
    key(56,True);key(62,True);key(62,False);key(56,False) # Alt+F4 closes the focused About window.
    eventually(lambda:command(root,'history-viewport')['focus'])
   elif a.cancel_selection=='typing':command(root,'switch',view=0,text='x')
   else:assert a.history_delay_ms>5000,'Timeout case must exceed the copy intent deadline'
   eventually(lambda:not command(root,'history-viewport')['pending'],8)
   time.sleep(a.history_delay_ms/1000+.25)
   assert command(root,'copy-state')['selection']=='','Cancelled selection resurrected'
   if a.cancel_selection=='timeout':assert 'timed out' in command(root,'inspect')['status'],'Timeout lacks visible feedback'
   if a.cancel_selection=='timeout':assert command(root,'inspect')['tab_status']=='ReadyWithWarning'
   command(root,'focus-view',view=0)
   focus_state=command(root,'history-viewport')
   primary_after=command(root,'read-text',primary=True)
   (r/'primary-after-cancel.json').write_text(json.dumps({'primary':primary_after,'focus':focus_state['focus']})+'\n')
   assert primary_after.get('sha256')==prior_primary,'Cancelled selection replaced PRIMARY'
   (r/'acceptance.json').write_text(json.dumps({'PASS':True,'history_delay_ms':a.history_delay_ms,'cancellation':a.cancel_selection,'primary_unchanged':True,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'gui_sha256':hashlib.sha256(gui.read_bytes()).hexdigest()},indent=2)+'\n')
   print('Pending history copy cancellation PASS',a.cancel_selection,flush=True)
   raise SystemExit(0)
  eventually(lambda:not command(root,'history-viewport')['pending'],8)
  selected=command(root,'copy-state')['selection'];assert selected,'Fast release lost its selection'
  viewport=command(root,'history-viewport');assert viewport['offset']>0,viewport
  assert command(root,'inspect')['tab_status']=='Ready','Healthy terminal lacks ready icon'
  assert len(selected)>500,'Fast release silently copied only the visible fragment'
  historical_markers=set(re.findall(r'ROW[0-9]{4}',selected))-visible_markers
  assert historical_markers,'Fast release silently copied only the starting screen'
  expected_hash=hashlib.sha256(selected.encode()).hexdigest()
  eventually(lambda:command(root,'read-text',primary=True).get('sha256')==expected_hash,8)
  assert 'ACK'not in rpc(sock,'pane.capture',{'pane':row['pane'],'lines':100})['text'],'Mouse input leaked to application'
  (r/'acceptance.json').write_text(json.dumps({'PASS':True,'history_delay_ms':a.history_delay_ms,'historical_markers':sorted(historical_markers),'selected_bytes':len(selected.encode()),'selected_logical_lines':len(selected.splitlines()),'offset':viewport['offset'],'primary_sha256':expected_hash,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'gui_sha256':hashlib.sha256(gui.read_bytes()).hexdigest(),'scope':'Actual Mutter Shift+drag+wheel+immediate release; real frozen RPC reply delayed by proxy'},indent=2)+'\n')
  print('Fast history release PASS',a.history_delay_ms,flush=True)
  raise SystemExit(0)
 key(42,True);move(ox+10*cw,oy+6*ch);control('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.08)
 move(ox+15*cw,oy+3*ch);time.sleep(.2)
 pinned=command(root,'history-viewport');assert any(c[2]!=256 for row in pinned['cells']for c in row),'Starting-screen colours disappeared'
 key(42,False) # Local selection remains captured after Shift is released.
 move(ox+15*cw,oy-ch);time.sleep(2.5)
 edge=command(root,'history-viewport');selected_before=command(root,'copy-state').get('selection','');assert selected_before,'Drag failed to begin'
 move(ox+15*cw,oy+2*ch);control('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,30.,0)));control('NotifyPointerAxis',GLib.Variant('(ddu)',(0.,30.,1)));time.sleep(.15)
 after=command(root,'history-viewport');selected_after=command(root,'copy-state').get('selection','')
 control('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));key(42,False)
 assert edge['offset']>0,edge
 assert edge['offset']>info['grid_size'][1],(edge['offset'],info['grid_size']) # Soft-wrapped logical lines are not viewport rows.
 assert selected_after,'Wheel cleared the history-anchored selection'
 assert selected_before.rsplit('\n',1)[-1]==selected_after.rsplit('\n',1)[-1],'Selection anchor moved'
 assert 'ACK'not in rpc(sock,'pane.capture',{'pane':row['pane'],'lines':100})['text'],'Mouse input leaked to application'
 assert command(root,'copy-state')['selection']==selected_after
 expected_hash=hashlib.sha256(selected_after.encode()).hexdigest()
 eventually(lambda:command(root,'read-text',primary=True).get('sha256')==expected_hash,8)
 # A key cancels the pinned snapshot, returns to live output and cannot
 # resurrect the old selection when a subsequent history reply arrives.
 command(root,'switch',view=0,text='x')
 eventually(lambda:'ACK'in rpc(sock,'pane.capture',{'pane':row['pane']})['text'])
 time.sleep(.2)
 assert command(root,'copy-state')['selection']=='','Typing retained selection'
 assert command(root,'history-viewport')['offset']==0,'Typing retained history offset'
 receipt={'typing_cancels_selection':True,'PASS':True,'edge_hold_offset':edge['offset'],'wheel_offset':after['offset'],'selection_before_bytes':len(selected_before.encode()),'selected_logical_lines':len(selected_before.splitlines()),'selected_visual_rows':edge['offset']+7,'visible_rows':info['grid_size'][1],'selection_after_bytes':len(selected_after.encode()),'scope':'Actual isolated Mutter Shift+drag outside terminal, followed by native wheel while holding; no owner clipboard or physical/remote claim'}
 (r/'acceptance.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True)
 command(root,'workspace-close');app.wait(timeout=8);rpc(sock,'daemon.stop');server.wait(timeout=5)
finally:
 if proxy is not None:proxy.close()
 if seat:
  try:control('Stop')
  except Exception:pass
 for p in reversed(children):
  if p.poll()is None:p.terminate()
 for p in reversed(children):
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait()
