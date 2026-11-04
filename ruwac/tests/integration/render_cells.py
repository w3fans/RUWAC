#!/usr/bin/env python3
"""Real PTY compatibility and full-frame palette identity; generated state only."""
import argparse,copy,hashlib,json,os,subprocess,tempfile
from pathlib import Path
from spike import Client,Tui,eventually

p=argparse.ArgumentParser()
for name in ['binary','legacy','evidence']:p.add_argument('--'+name,required=True,type=Path)
a=p.parse_args();root=Path(tempfile.mkdtemp(prefix='palette-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'TERM':'xterm-256color'}
for key in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:
 d=root/key;d.mkdir(mode=0o700);env[key]=str(d)
for key in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(key,None)
servers=[];clients=[];tuis=[];checks=[]

def mark(name):checks.append(name);print(name,'PASS',flush=True)
def start(binary,name):
 sock=root/(name+'.sock')
 with(root/(name+'.log')).open('wb')as out:daemon=subprocess.Popen([str(binary),'--socket',str(sock),'serve'],env=env,stdout=out,stderr=subprocess.STDOUT)
 servers.append(daemon);eventually(sock.exists);client=Client(sock);clients.append(client)
 text='\x1b[38;2;1;2;3m\x1b[48;2;4;5;6m\x1b[1;3;4;7mŽe\u0301界\r\nPALETTE_READY\r\n'
 # exec sleep keeps a single stable foreground identity and a quiet canonical grid.
 import shlex
 row=client.call('session.create',{'name':'generated-'+name,'rows':12,'cols':60,'argv':['/bin/bash','--noprofile','--norc','-c','printf %s '+shlex.quote(text)+';exec /bin/sleep 600']})
 client.call('pane.wait',{'pane':row['pane'],'contains':'PALETTE_READY','timeout_ms':5000})
 client.call('session.attach',{'session':row['session'],'rows':12,'cols':60})
 client.call('client.render') # synchronize foreground-group metadata once
 return client,sock,row

def expand(frame):
 value=copy.deepcopy(frame)
 for pane in value['panes']:
  grid=pane['grid']
  if grid.get('cell_encoding')=='palette-v1':
   palette=grid.pop('cell_palette');indices=grid.pop('cell_indices');grid.pop('cell_encoding')
   grid['cells']=[[palette[i]for i in row]for row in indices]
 return value

try:
 current,sock,row=start(a.binary,'new')
 assert 'palette-v1'in current.call('capabilities')['render_cell_encodings']
 plain=current.call('client.render')
 assert all('cell_encoding'not in pane['grid']for pane in plain['panes'])
 compact=current.call('client.render',{'revision':plain['revision'],'cell_encoding':'palette-v1'})
 assert compact.get('unchanged')is not True
 assert any(pane['grid'].get('cell_encoding')=='palette-v1'for pane in compact['panes'])
 assert expand(compact)==plain
 assert len(json.dumps(compact).encode())*2<len(json.dumps(plain).encode())
 mark('plain-old-client-and-opt-in-palette-have-exact-canonical-equality')
 assert current.call('client.render',{'revision':compact['revision'],'cell_encoding':'palette-v1'})['unchanged']is True
 switched=current.call('client.render',{'revision':compact['revision']})
 assert switched.get('unchanged')is not True and switched==plain
 current.call('client.render',{'cell_encoding':'unknown'},error='INVALID_PARAMS')
 mark('encoding-switch-invalidates-projection-and-unknown-codec-is-refused')
 before=current.call('pane.list',{'session':row['session']})
 old_tui=Tui(a.legacy,sock,row['session'],root/'old-client.ansi',rows=13,cols=60,env=env);tuis.append(old_tui)
 eventually(lambda:b'PALETTE_READY'in old_tui.data)
 assert old_tui.finish()==0;tuis.remove(old_tui)
 after=current.call('pane.list',{'session':row['session']})
 assert before[0]['pid']==after[0]['pid']
 mark('preserved-client-renders-new-engine-without-compact-support-or-process-replacement')
 legacy,old_sock,old_row=start(a.legacy,'old')
 requested=legacy.call('client.render',{'cell_encoding':'palette-v1'})
 assert all('cell_encoding'not in pane['grid']for pane in requested['panes'])
 old_pid=legacy.call('pane.list',{'session':old_row['session']})[0]['pid']
 new_tui=Tui(a.binary,old_sock,old_row['session'],root/'new-client-old-engine.ansi',rows=13,cols=60,env=env);tuis.append(new_tui)
 eventually(lambda:b'PALETTE_READY'in new_tui.data)
 assert new_tui.finish()==0;tuis.remove(new_tui)
 assert legacy.call('pane.list',{'session':old_row['session']})[0]['pid']==old_pid
 mark('new-client-accepts-preserved-engine-plain-fallback-without-restart')
 (a.evidence/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'legacy_sha256':hashlib.sha256(a.legacy.read_bytes()).hexdigest(),'wire_bytes':{'plain':len(json.dumps(plain).encode()),'palette':len(json.dumps(compact).encode())}},indent=2)+'\n')
finally:
 for tui in tuis:
  try:tui.finish('term')
  except Exception:pass
 for client in clients:
  try:client.call('daemon.stop')
  except Exception:pass
  client.close()
 for server in servers:
  if server.poll()is None:server.terminate()
  try:server.wait(timeout=5)
  except subprocess.TimeoutExpired:server.kill();server.wait()
