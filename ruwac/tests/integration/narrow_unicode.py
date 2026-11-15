#!/usr/bin/env python3
"""Real PTYs retain Unicode and size agreement under a one-column split view."""
import argparse,hashlib,json,os,subprocess,sys,tempfile
from pathlib import Path
from spike import Client,eventually

p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--evidence',required=True,type=Path);a=p.parse_args()
root=Path(tempfile.mkdtemp(prefix='narrow-',dir=a.evidence));root.chmod(0o700);sock=root/'c.sock'
env={k:v for k,v in os.environ.items() if k not in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','RUWAC_STATE_FILE']}
env['RUWAC_STATE_FILE']=str(root/'state.json')
for key in ['HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME']:
 path=root/key.lower();path.mkdir(mode=0o700);env[key]=str(path)
dimensions=root/'dimensions.json';app=root/'app.py'
app.write_text('''import fcntl,json,os,struct,tty
from pathlib import Path
tty.setraw(0)
os.write(1,b'READY')
count=0
while True:
 b=os.read(0,1)
 if not b:break
 if b not in [b'p',b'a']:continue
 count+=1
 os.write(1,(('\\x1b[?1049h' if b==b'a' else '\\x1b[?1049l')+'\\x1b[2J\\x1b[H\\x1b[31m界e\\u0301\\x1b[0m').encode())
 rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,0x5413,b'\\0'*8))
 Path('''+repr(str(dimensions))+''').write_text(json.dumps({'rows':rows,'cols':cols,'count':count}))
''')
log=(root/'daemon.log').open('wb');server=None;view=None;cases=[]
def rpc(method,params=None):
 with Client(sock) as c:return c.call(method,params)
try:
 server=subprocess.Popen([a.binary,'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log)
 eventually(lambda:sock.exists() and rpc('health')['ok'])
 s=rpc('session.create',{'name':'narrow Unicode','rows':24,'cols':80,'argv':[sys.executable,str(app)]})
 eventually(lambda:'READY' in rpc('pane.capture',{'pane':s['pane']})['text'])
 view=Client(sock);view.call('session.attach',{'session':s['session'],'rows':24,'cols':80})
 view.call('pane.create',{'window':s['window'],'pane':s['pane'],'orientation':'vertical','argv':[sys.executable,'-c','import time;time.sleep(90)']})
 split=view.call('client.render')['layout']['id'];count=0
 for index in range(40):
  cols=[10,30,80,10][index%4]
  view.call('client.resize',{'rows':24,'cols':cols});view.call('pane.layout',{'window':s['window'],'split':split,'ratio':100})
  frame=view.call('client.render');pane=next(p for p in frame['panes'] if p['id']==s['pane'])
  visible=pane['rect']['cols'];canonical=max(2,visible)
  assert pane['grid']['cols']==canonical,(visible,pane['grid']['cols'])
  rpc('pane.sendText',{'pane':s['pane'],'text':'a' if index%2 else 'p'});count+=1
  def emitted():
   try:return json.loads(dimensions.read_text())['count']==count
   except (FileNotFoundError,json.JSONDecodeError):return False
  eventually(emitted)
  assert json.loads(dimensions.read_text())['cols']==canonical
  eventually(lambda:'界' in rpc('pane.capture',{'pane':s['pane']})['text'])
  capture=rpc('pane.capture',{'pane':s['pane']})['text'];assert 'e\u0301' in capture,capture
  assert rpc('health')['ok']
  rows=rpc('pane.list',{'session':s['session']});assert next(r for r in rows if r['id']==s['pane'])['pid']==s['pid']
  cases.append({'session_columns':cols,'visible_columns':visible,'canonical_columns':canonical,'alternate':bool(index%2),'PTY_size_matches':True,'Unicode_retained':True})
 rpc('daemon.checkpoint')
 assert Path('/proc/'+str(s['pid'])).exists()
finally:
 if view is not None:view.close()
 if server is not None and server.poll() is None:
  server.terminate()
  try:server.wait(timeout=5)
  except subprocess.TimeoutExpired:server.kill();server.wait(timeout=5)
 log.close()
assert len(cases)==40 and 'panicked' not in (root/'daemon.log').read_text()
result={'PASS':True,'cases':cases,'binary_sha256':hashlib.sha256(Path(a.binary).read_bytes()).hexdigest(),'stable_PTY_PID':True,'owner_processes_changed':False,'physical_GUI_acceptance':False}
(root/'acceptance.json').write_text(json.dumps(result,indent=2)+'\n');print('EVIDENCE',root,flush=True)
