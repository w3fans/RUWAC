#!/usr/bin/env python3
"""Mapped GTK terminal sizing controls on an isolated compositor, real private PTYs."""
import argparse,hashlib,json,os,subprocess,sys,tempfile,time,traceback
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser(description=__doc__)
for name in ['binary','gui','evidence']:p.add_argument('--'+name,required=True)
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
root=Path(tempfile.mkdtemp(prefix='size-gui-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2','SHELL':'/bin/bash'}
for k,n in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 (root/n).mkdir(mode=0o700);env[k]=str(root/n)
for k in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','RUWAC_STATE_FILE','TMUX','STY']:env.pop(k,None)
children=[];clients=[];seq=0;checks=[];sock=root/'s';fixture=Path(__file__).resolve().parents[1]/'fixtures/sizing_receiver.py'
def start(args,name):
 q=subprocess.Popen(list(map(str,args)),env=env,stdout=(root/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def rpc(m,v=None):
 with Client(sock) as c:return c.call(m,v)
def command(action,**kw):
 global seq
 seq+=1;t=root/'q.tmp';t.write_text(json.dumps(dict(seq=seq,action=action,**kw)));t.replace(root/'gui-request.json')
 def read():
  try:
   d=json.loads((root/'gui-response.json').read_text());return d['result'] if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(read,15)
def mark(name):checks.append({'id':name,'status':'PASS'});print(name,'PASS',flush=True)
try:
 daemon=start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 row=rpc('session.create',{'name':'GTK controlled','size_policy':'controller_owned','rows':42,'cols':120,'argv':[sys.executable,str(fixture),str(root)]});sid=row['session'];pane=row['pane'];pid=row['pid']
 rpc('pane.wait',{'pane':pane,'contains':'SIZING_READY','timeout_ms':5000})
 other=Client(sock);clients.append(other);other.call('session.attach',{'session':sid,'rows':42,'cols':120})
 g=rpc('session.size',{'session':sid})['generation'];g=other.call('session.sizeClaim',{'session':sid,'expected_generation':g,'takeover':False})['generation']
 gui=start([a.gui,'--new-instance','--socket',sock,'--session',sid,'--fixture-dir',root],'GUI')
 eventually(lambda:command('inspect')['panes']==[1],20)
 eventually(lambda:'Viewer' in command('inspect')['status'])
 info=command('sizing-control');assert info['button_visible'] and info['button_label']=='Take sizing control…'
 assert rpc('session.size',{'session':sid})['cols']==120
 mark('GTK-CONTROLLED-ATTACH-IS-VIEWER-WITH-VISIBLE-ACTION')
 info=command('sizing-control',operation='open');assert info['dialog_visible']
 command('sizing-control',operation='cancel');assert rpc('session.size',{'session':sid})['generation']==g
 mark('GTK-CANCEL-DOES-NOT-CLAIM-OR-RESIZE')
 # A delayed confirmation cannot take over an intervening ownership decision.
 command('sizing-control',operation='open')
 other.call('session.sizeRelease',{'session':sid,'size_generation':g})
 g=other.call('session.sizeClaim',{'session':sid,'expected_generation':rpc('session.size',{'session':sid})['generation'],'takeover':False})['generation']
 command('sizing-control',operation='accept')
 eventually(lambda:'SIZE_STALE' in command('inspect')['status'])
 assert rpc('session.size',{'session':sid})['generation']==g and rpc('session.size',{'session':sid})['cols']==120
 mark('GTK-DELAYED-CONFIRMATION-REJECTS-STALE-GENERATION')
 # Explicit command uses current mapped frame. Wait until its updated frame arrives.
 time.sleep(.25);command('command',text='size takeover')
 eventually(lambda:rpc('session.size',{'session':sid})['generation']!=g)
 eventually(lambda:'Controller' in command('inspect')['status'])
 info=command('inspect');size=rpc('session.size',{'session':sid});cw,ch=info['cell_metrics'];width,height=info['area_size']
 assert abs(size['rows']-int(height/ch))<=1 and abs(size['cols']-int(width/cw))<=1,(size,info)
 assert command('sizing-control')['button_label']=='Release sizing control'
 mark('GTK-EXPLICIT-TAKEOVER-USES-ACTUAL-ALLOCATION')
 owned_generation=size['generation'];other.call('client.resize',{'rows':8,'cols':20,'size_generation':g},error='SIZE_NOT_OWNED')
 # A structural GUI operation includes its current lease and updates real panes.
 command('command',text='split vertical');eventually(lambda:len(rpc('pane.list',{'session':sid}))==2)
 sizes=rpc('pane.list',{'session':sid});command('command',text='zoom');time.sleep(.3)
 after=rpc('pane.list',{'session':sid});assert [(p['id'],p['rows'],p['cols']) for p in sizes]==[(p['id'],p['rows'],p['cols']) for p in after]
 assert next(p for p in after if p['id']==pane)['pid']==pid
 mark('GTK-SPLIT-CARRIES-LEASE-ZOOM-IS-PRESENTATION-ONLY')
 command('sizing-control',operation='open') # Controller button explicitly releases, no dialog.
 eventually(lambda:not rpc('session.size',{'session':sid})['controller_present'])
 time.sleep(.25);frozen=rpc('pane.list',{'session':sid});command('command',text='zoom');time.sleep(.2)
 assert rpc('pane.list',{'session':sid})==frozen
 mark('GTK-RELEASE-FREEZES-CANONICAL-GRID')
 # Confirming the button with no owner claims exactly the displayed generation.
 assert command('sizing-control',operation='open')['dialog_visible'];command('sizing-control',operation='accept')
 eventually(lambda:rpc('session.size',{'session':sid})['controller_present'])
 eventually(lambda:'Controller' in command('inspect')['status'])
 mark('GTK-BUTTON-CONFIRMATION-CLAIMS-WITHOUT-IMPLICIT-HANDOVER')
 command('workspace-close');gui.wait(timeout=8)
 eventually(lambda:not rpc('session.size',{'session':sid})['controller_present'])
 assert next(p for p in rpc('pane.list',{'session':sid}) if p['id']==pane)['pid']==pid
 mark('GTK-FRONTEND-CLOSE-RELEASES-LEASE-PRESERVES-PTY')
 rpc('daemon.stop');daemon.wait(timeout=8)
except Exception:
 checks.append({'id':'UNHANDLED','status':'FAIL','traceback':traceback.format_exc()});raise
finally:
 for c in clients:c.close()
 for q in reversed(children):
  if q.poll() is None:q.terminate()
 for q in reversed(children):
  if q.poll() is None:
   try:q.wait(timeout=6)
   except subprocess.TimeoutExpired:q.kill();q.wait(timeout=3)
 receipt={'PASS':len(checks)==8 and all(c['status']=='PASS' for c in checks),'checks':checks,'binary_sha256':hashlib.sha256(Path(a.binary).read_bytes()).hexdigest(),'gui_sha256':hashlib.sha256(Path(a.gui).read_bytes()).hexdigest(),'scope':'Mapped GTK callbacks/confirmation on owned compositor and real PTYs; synthetic fixture actions, not physical user acceptance.'}
 (Path(a.evidence)/'acceptance.json').write_text(json.dumps(receipt,indent=2)+'\n')
