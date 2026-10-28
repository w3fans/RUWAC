#!/usr/bin/env python3
"""Native history drag, edge scrolling, exact PRIMARY and typing cancellation."""
from pathlib import Path
import argparse,hashlib,json,os,subprocess,sys,tempfile,time
from gi.repository import Gio,GLib
from spike import Client,eventually
parser=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:parser.add_argument('--'+name,required=True)
a=parser.parse_args()
assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test','Requires owned compositor'
r=Path(tempfile.mkdtemp(prefix='selection-history-',dir=a.evidence));r.chmod(0o700)
binary=Path(a.binary).resolve();gui=Path(a.gui).resolve()
env={**os.environ,'TERM':'xterm-256color','GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2'}
for k in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(k,None)
children=[];seq=0;results={};bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';seat=None
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
 app=start([gui,'--new-instance','--socket',sock,'--session',row['session'],'--allow-workspace-control','--fixture-dir',root],'GUI',e)
 eventually(lambda:command(root,'inspect').get('target'),15);command(root,'chrome',width=1200,height=850,maximized=True);command(root,'focus-view',view=0);eventually(lambda:'BENCH_READY'in text(root),15)
 eventually(lambda:command(root,'chrome')['maximized']);time.sleep(.2);info=command(root,'inspect');ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
 position=[0.,0.];control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)))
 def move(x,y):
  control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x)-position[0],float(y)-position[1])));position[:]=[float(x),float(y)];time.sleep(.08)
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
 if seat:
  try:control('Stop')
  except Exception:pass
 for p in reversed(children):
  if p.poll()is None:p.terminate()
 for p in reversed(children):
  try:p.wait(timeout=3)
  except subprocess.TimeoutExpired:p.kill();p.wait()
