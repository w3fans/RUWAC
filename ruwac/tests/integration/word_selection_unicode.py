#!/usr/bin/env python3
"""Generated PTYs: double click selects words without changing mouse ownership."""
import argparse,hashlib,json,os,re,subprocess,sys,tempfile,time
from pathlib import Path
from spike import Client,Tui,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--evidence',required=True);p.add_argument('--baseline',action='store_true');a=p.parse_args()
root=Path(tempfile.mkdtemp(prefix='held-drag-',dir=a.evidence));root.chmod(0o700)
runtime=Path(tempfile.mkdtemp(prefix='ruwac-drag-'));runtime.chmod(0o700)
sock=runtime/'s';binary=Path(a.binary).resolve();ui=None;server=None;checks=[]
env={**os.environ,'HOME':str(root),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'RUWAC_STATE_FILE':str(root/'state.json'),'TERM':'xterm-256color'}
for name in ['config','data','cache']:(root/name).mkdir(mode=0o700)
def rpc(method,params=None):
 with Client(sock)as c:return c.call(method,params)
def mouse(button,x,y,release=False):
 ui.send((f'\x1b[<{button};{x};{y}'+('m'if release else'M')).encode())
try:
 log=(root/'engine.log').open('wb');server=subprocess.Popen([str(binary),'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log)
 eventually(lambda:sock.exists()and rpc('health')['ok'])
 writer=root/'writer.py';writer.write_text('import os,tty\nfrom pathlib import Path\ntty.setraw(0)\np=Path(__file__).with_suffix(".input");p.touch(mode=0o600)\nos.write(1,(\'before 👩\u200d💻 after\\r\\nbefore čebe\\u0301la after\\r\\nbefore 界 after\\r\\n\'+\'x\'*76+\' \'+\'abcdefgh\\r\\n\').encode())\nos.write(1,b"READY\\r\\n")\nwith p.open("ab",buffering=0)as out:\n while True:out.write(os.read(0,4096))\n')
 row=rpc('session.create',{'name':'held-drag','rows':23,'cols':80,'argv':[sys.executable,str(writer)]})
 rpc('pane.wait',{'pane':row['pane'],'contains':'READY','timeout_ms':5000})
 ui=Tui(binary,sock,row['session'],root/'terminal.pty',rows=24,cols=80,env=env)
 eventually(lambda:b'READY'in ui.data)
 visible=rpc('pane.capture',{'pane':row['pane']})['text']
 assert 'before 👩‍💻 after' in visible,visible
 for x,y,expected in [(8,1,'👩‍💻'),(11,1,'👩‍💻'),(11,2,'čebe\u0301la'),(9,3,'界'),(2,5,'abcdefgh')]:
  rpc('buffer.set',{'session':row['session'],'text':''});time.sleep(.45)
  for _ in range(2):
   mouse(0,x,y);time.sleep(.04);mouse(0,x,y,True);time.sleep(.06)
  time.sleep(.1)
  actual=rpc('buffer.get',{'session':row['session']})['text']
  assert actual==expected,{'reason':'UNICODE_WORD_CELL_MAPPING','position':[x,y],'expected':expected,'actual':actual}
  checks.append('word selected at '+str((x,y)))
 assert writer.with_suffix('.input').read_bytes()==b'',writer.with_suffix('.input').read_bytes()
 checks.append('mouse never reaches application input')
 assert next(p for p in rpc('pane.list',{'session':row['session']})if p['id']==row['pane'])['pid']==row['pid']
 checks.append('PTY PID preserved')
 ui.finish();ui=None
finally:
 if ui is not None:ui.finish('term')
 if server is not None and server.poll()is None:
  rpc('daemon.stop');server.wait(timeout=5)
 (root/'receipt.json').write_text(json.dumps({'PASS':len(checks)==7,'checks':checks,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()},indent=2)+'\n')
 import shutil
 shutil.rmtree(runtime)
print('Terminal Unicode word-selection regression PASS',flush=True)
