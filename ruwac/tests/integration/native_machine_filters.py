#!/usr/bin/env python3
"""Mapped sidebar capability filters; generated inventory, no connections."""
import argparse,json,os,subprocess,tempfile
from pathlib import Path
from spike import eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--gui',required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args()
assert os.environ.get('WAYLAND_DISPLAY','').startswith('ruwac-')
root=Path(tempfile.mkdtemp(prefix='machine-filters-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash'}
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 env[key]=str(root/name);Path(env[key]).mkdir(mode=0o700)
profile=Path(env['XDG_CONFIG_HOME'])/'RUWAC/machines.json';profile.parent.mkdir(mode=0o700)
def item(i,machine,protocol,codex='',claude='',favorite=False,recent=0):
 return dict(id='endpoint_demo'+str(i),machine_id='machine_demo'+str(machine),name='Generated '+str(machine),group='Generated',protocol=protocol,server='127.0.0.1',username='',secret_reference='',favorite=favorite,last_used=recent,tags=[],preferences=dict(codex_command=codex,claude_command=claude))
items=[item(0,0,'RDP',favorite=True),item(1,1,'SSH',recent=20),item(2,2,'RDP'),item(3,2,'SSH',codex='codex resume --all'),item(4,3,'SSH',claude='claude --resume'),item(5,4,'SSH',codex='codex resume --all',claude='claude --resume')]
profile.write_text(json.dumps(dict(schema=1,items=items,groups=[])));profile.chmod(0o600)
sock=root/'control.sock';fixture=root/'fixture';fixture.mkdir(mode=0o700);children=[];seq=0

def start(args,name):
 log=(root/(name+'.log')).open('w');child=subprocess.Popen(args,env=env,stdout=log,stderr=log);children.append((child,log))
def command(action,**kw):
 global seq
 seq+=1;tmp=fixture/'request.tmp';tmp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));tmp.replace(fixture/'gui-request.json')
 def read():
  try:
   q=json.loads((fixture/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(read,12)
try:
 start([a.binary,'--socket',str(sock),'serve'],'daemon');eventually(sock.exists)
 start([a.gui,'--new-instance','--socket',str(sock),'--show-machines','--fixture-dir',str(fixture)],'gui')
 eventually(lambda:command('inspect').get('target'),20)
 command('chrome',width=1100,height=800,collapse=False)
 baseline=profile.read_bytes();checks=[]
 for mode,count in [(0,5),(1,1),(2,1),(3,2),(4,4),(5,2),(6,2),(0,5)]:
  info=command('host-shortcuts',fields={'machine-sidebar-filter':mode})
  assert any(w['name']=='machine-sidebar-filter'and w.get('selected')==mode for w in info['widgets']),info
  result=eventually(lambda:(q if (q:=command('profiles'))['rows']==count else None))
  checks.append(dict(mode=mode,rows=result['rows'],labels=result['labels']))
 command('host-shortcuts',fields={'machine-sidebar-filter':5})
 assert eventually(lambda:(q if(q:=command('profiles',search='Generated 2'))['rows']==1 else None))
 assert eventually(lambda:(q if(q:=command('profiles',search='Generated 0'))['rows']==0 else None))
 assert eventually(lambda:(q if(q:=command('profiles',search=''))['rows']==2 else None))
 assert profile.read_bytes()==baseline,'Filtering must not write inventory'
 (a.evidence/'acceptance.json').write_text(json.dumps(dict(PASS=True,cases=checks,search_intersections=3,inventory_unchanged=True,owner_sessions_used=False),indent=2)+'\n')
 print('Native machine filters PASS: eight mode transitions, three search intersections')
finally:
 for child,log in reversed(children):
  if child.poll()is None:child.terminate()
  try:child.wait(timeout=8)
  except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
  log.close()
