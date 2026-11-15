#!/usr/bin/env python3
"""Deliberate PRIMARY middle-paste across tabs, including preserved old engines.

Runs only on an owned Wayland display. Production GTK click callbacks and real
PTYs carry the text; selection setup is synthetic, not physical touchpad input.
"""
import argparse,json,os,subprocess,sys,tempfile,time
from pathlib import Path
from gi.repository import Gio,GLib
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,required=True)
p.add_argument('--legacy-binary');a=p.parse_args()
assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
root=Path(tempfile.mkdtemp(prefix='primary-tabs-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','RAYON_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2','SHELL':'/bin/bash'}
for key,leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 path=root/leaf;path.mkdir(mode=0o700);env[key]=str(path)
for key in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(key,None)
children=[];seq=0;checks=[]
def start(args,name):
 q=subprocess.Popen(list(map(str,args)),env=env,stdout=(root/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def rpc(sock,method,params=None):
 with Client(sock)as c:return c.call(method,params)
def command(action,**kw):
 global seq
 seq+=1;q=root/'request.tmp';q.write_text(json.dumps(dict(seq=seq,action=action,**kw)));q.replace(root/'gui-request.json')
 def response():
  try:
   d=json.loads((root/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
receiver=root/'receiver.py';receiver.write_text('import os,sys,tty\ntty.setraw(0)\nos.write(1,b"READY\\r\\n\\x1b[?2004h")\nf=open(sys.argv[1],"ab",buffering=0)\nwhile True:\n b=os.read(0,4096)\n if not b:break\n f.write(b)\n')
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';session=None
try:
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
 def seat(method,args=None):return bus.call_sync(service,session,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
 seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)))
 for down in [True,False]:seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(194,down)))
 specs=[('current',a.binary)]
 if a.legacy_binary:specs.append(('legacy',a.legacy_binary))
 for label,binary in specs:
  sock=root/(label+'.sock');start([binary,'--socket',sock,'serve'],label+'-daemon');eventually(sock.exists)
  rows=[];outputs=[]
  # Two ordinary tabs and one nested TUI tab all use this engine version.
  for n in range(3):
   out=root/(label+str(n)+'.input');outputs.append(out)
   inner=rpc(sock,'session.create',{'name':label+str(n),'argv':[sys.executable,str(receiver),str(out)]})
   if n==2:
    inner=rpc(sock,'session.create',{'name':label+'nested','argv':[a.binary,'--socket',str(sock),'attach','--session',inner['session']]})
   rows.append(inner)
  for name in ['gui-request.json','gui-response.json']:(root/name).unlink(missing_ok=True)
  gui=start([a.gui,'--new-instance','--socket',sock,*sum((['--session',v['session']]for v in rows),[]),'--fixture-dir',root],label+'-GUI')
  eventually(lambda:command('inspect')['tabs']==3,15)
  eventually(lambda:command('inspect')['active'],10)
  for target in [1,2]:
   command('focus-view',view=0);command('copy',view=0,primary=True)
   # The selection belongs to the first tab, not the destination's internal
   # session buffer. A middle click must transfer its complete Unicode text.
   text=command('copy-state',view=0)['selection'];assert text
   command('focus-view',view=target)
   eventually(lambda:command('inspect',view=target)['target']==rows[target]['pane'])
   time.sleep(.1)
   command('click',view=target,button=2,x=20.,y=20.)
   command('click',view=target,button=2,x=20.,y=20.,release=True)
   expected=b'\x1b[200~'+text.encode()+b'\x1b[201~'
   def pasted():return outputs[target].exists()and outputs[target].read_bytes()==expected
   try:eventually(pasted,8)
   except Exception:
    (root/'failure.json').write_text(json.dumps({'engine':label,'tab':target,'state':command('inspect',view=target),'received_hex':outputs[target].read_bytes().hex()if outputs[target].exists()else None},indent=2));raise
   checks.append({'engine':label,'destination':'nested TUI'if target==2 else'ordinary terminal','PRIMARY_between_tabs':True,'complete_Unicode_and_one_bracket_pair':True})
   print(label,target,'PASS',flush=True)
  command('workspace-close');gui.wait(timeout=8)
 (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'physical_input':False,'owner_desktop_used':False},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
 print(root,flush=True)
