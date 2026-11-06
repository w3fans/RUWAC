#!/usr/bin/env python3
"""Word selection uses source-pane history geometry in fresh and populated splits."""
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
 writer=root/'writer.py';writer.write_text('import os,tty,sys\nfrom pathlib import Path\ntty.setraw(0)\np=Path(__file__).with_suffix(".input");p.touch(mode=0o600)\nif len(sys.argv)>1:\n for i in range(300):os.write(1,("OLD%04d\\r\\n"%i).encode())\nos.write(1,b"before alpha_beta-42 after\\r\\n")\nos.write(1,b"READY\\r\\n")\nwith p.open("ab",buffering=0)as out:\n while True:out.write(os.read(0,4096))\n')
 row=rpc('session.create',{'name':'held-drag','rows':23,'cols':80,'argv':[sys.executable,str(writer)]})
 rpc('pane.wait',{'pane':row['pane'],'contains':'READY','timeout_ms':5000})
 split=rpc('pane.create',{'window':row['window'],'pane':row['pane'],'orientation':'horizontal','argv':[sys.executable,str(writer)]})
 rpc('pane.wait',{'pane':split['pane'],'contains':'READY','timeout_ms':5000})
 ui=Tui(binary,sock,row['session'],root/'terminal.pty',rows=24,cols=80,env=env)
 eventually(lambda:b'READY'in ui.data)
 visible=rpc('pane.capture',{'pane':row['pane']})['text']
 assert 'before alpha_beta-42 after' in visible, visible
 def select_word(pane_id,label):
  with Client(sock)as observer:
   observer.call('session.attach',{'session':row['session'],'rows':24,'cols':80,'size_role':'viewer'})
   frame=observer.call('client.render')
   pane=next(p for p in frame['panes']if p['id']==pane_id)
   lines=[''.join(c[0]for c in line if c[1]!=True)for line in pane['grid']['cells']]
   y=next(i for i,line in enumerate(lines)if 'alpha_beta'in line);x=lines[y].index('alpha_beta')+3
   screen_x=pane['rect']['x']+x+1;screen_y=pane['rect']['y']+y+1
  rpc('buffer.set',{'session':row['session'],'text':''});time.sleep(.45)
  for _ in range(2):
   mouse(0,screen_x,screen_y);time.sleep(.04);mouse(0,screen_x,screen_y,True);time.sleep(.06)
  time.sleep(.15)
  actual=rpc('buffer.get',{'session':row['session']})['text']
  assert actual=='alpha_beta',{'reason':'FRESH_SPLIT_WORD_SELECTION','case':label,'actual':actual,'rect':pane['rect'],'visible':lines[y]}
  checks.append(label)
 select_word(row['pane'],'fresh top split')
 select_word(split['pane'],'fresh bottom split')
 populated=rpc('pane.create',{'window':row['window'],'pane':split['pane'],'orientation':'horizontal','argv':[sys.executable,str(writer),'populated']})
 rpc('pane.wait',{'pane':populated['pane'],'contains':'READY','timeout_ms':5000});time.sleep(.2)
 select_word(populated['pane'],'populated shallow split')
 assert writer.with_suffix('.input').read_bytes()==b''
 checks.append('mouse never reaches any application')
 panes=rpc('pane.list',{'session':row['session']})
 assert all(next(p for p in panes if p['id']==item['pane'])['pid']==item['pid']for item in [row,split,populated])
 checks.append('all three PTY PIDs preserved')
 ui.finish();ui=None
finally:
 if ui is not None:ui.finish('term')
 if server is not None and server.poll()is None:
  rpc('daemon.stop');server.wait(timeout=5)
 (root/'receipt.json').write_text(json.dumps({'PASS':len(checks)==5,'checks':checks,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()},indent=2)+'\n')
 import shutil
 shutil.rmtree(runtime)
print('Fresh split word-selection regression PASS',flush=True)
