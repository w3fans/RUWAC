#!/usr/bin/env python3
"""Generated PTYs: live-shell drag must scroll while held, without a second click."""
import argparse,hashlib,json,os,re,subprocess,sys,tempfile,time
from pathlib import Path
from spike import Client,Tui,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--evidence',required=True);a=p.parse_args()
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
 writer=root/'writer.py';writer.write_text('import os,tty\nfrom pathlib import Path\ntty.setraw(0)\np=Path(__file__).with_suffix(".input");p.touch(mode=0o600)\nfor i in range(300):os.write(1,("ROW%04d xyz\\r\\n"%i).encode())\nos.write(1,b"READY\\r\\n")\nwith p.open("ab",buffering=0)as out:\n while True:out.write(os.read(0,4096))\n')
 row=rpc('session.create',{'name':'held-drag','rows':23,'cols':80,'argv':[sys.executable,str(writer)]})
 rpc('pane.wait',{'pane':row['pane'],'contains':'READY','timeout_ms':5000})
 ui=Tui(binary,sock,row['session'],root/'terminal.pty',rows=24,cols=80,env=env)
 eventually(lambda:b'READY'in ui.data)
 # No wheel, keyboard copy command, second press, or pointer movement while held.
 mouse(0,2,12);mouse(32,2,1);time.sleep(.65);mouse(0,2,1,True)
 text=eventually(lambda:rpc('buffer.get',{'session':row['session']})['text'])
 rows=[int(v)for v in re.findall(r'ROW(\d{4})',text)]
 assert rows and min(rows)<270,{'reason':'stationary top-edge drag stayed within visible page','copied_rows':rows,'text':text}
 checks.append('live-shell held drag reaches older rows without another click')
 # A held drag inside the page must neither scroll nor send input to the child.
 rpc('buffer.set',{'session':row['session'],'text':''})
 mouse(0,2,12);mouse(32,10,10);time.sleep(.25);mouse(0,10,10,True)
 inside=eventually(lambda:rpc('buffer.get',{'session':row['session']})['text'])
 inside_rows=[int(v)for v in re.findall(r'ROW(\d{4})',inside)]
 assert inside_rows and min(inside_rows)>=270,inside
 checks.append('interior hold does not scroll')
 # Start from existing history and hold the bottom edge to reach newer output.
 for _ in range(12):mouse(64,2,10)
 time.sleep(.1);rpc('buffer.set',{'session':row['session'],'text':''})
 mouse(0,2,5);mouse(32,20,23);time.sleep(.8);mouse(0,20,23,True)
 bottom=eventually(lambda:rpc('buffer.get',{'session':row['session']})['text'])
 bottom_rows=[int(v)for v in re.findall(r'ROW(\d{4})',bottom)]
 assert bottom_rows and max(bottom_rows)>=295,bottom
 checks.append('existing-history bottom edge reaches newer rows')
 assert writer.with_suffix('.input').read_bytes()==b''
 checks.append('selection bytes never reach application PTY')
 panes=rpc('pane.list',{'session':row['session']});assert next(v for v in panes if v['id']==row['pane'])['pid']==row['pid']
 checks.append('shell PID preserved')
 ui.finish();ui=None
finally:
 if ui is not None:ui.finish('term')
 if server is not None and server.poll()is None:
  rpc('daemon.stop');server.wait(timeout=5)
 (root/'receipt.json').write_text(json.dumps({'PASS':len(checks)==5,'checks':checks,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()},indent=2)+'\n')
 import shutil
 shutil.rmtree(runtime)
print('Terminal held-drag regression PASS',flush=True)
