#!/usr/bin/env python3
"""Two mapped GUI windows on independent engines; isolated compositor only."""
import argparse,hashlib,json,os,subprocess,time
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','legacy','evidence']:p.add_argument('--'+name,required=True,type=Path)
p.add_argument("--via-updates",action="store_true")
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=a.evidence/'g';r.mkdir(mode=0o700);sock=r/'old.sock';children=[];new_socket=None;seq=0;checks=[]
env=dict(os.environ,GTK_A11Y='none',GSETTINGS_BACKEND='memory',XDG_STATE_HOME=str(r/'state'))
for key in ['RUWAC_STATE_FILE','RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(key,None)
def start(args,name,custom=None):
 with (r/(name+'.log')).open('w')as log:q=subprocess.Popen(list(map(str,args)),env=custom or env,stdout=log,stderr=subprocess.STDOUT)
 children.append(q);return q
def call(path,m,v=None):
 with Client(path)as c:return c.call(m,v)
def command(directory,action,**options):
 global seq
 seq+=1;t=directory/'request.tmp';t.write_text(json.dumps({'seq':seq,'action':action,**options}));t.replace(directory/'gui-request.json')
 def response():
  try:
   d=json.loads((directory/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
def check(name,condition):
 assert condition,name
 checks.append(name);print(name,'PASS',flush=True)
try:
 old=start([a.legacy,'--socket',sock,'serve'],'old-daemon');eventually(sock.exists)
 row=call(sock,'session.create',{'name':'keep-working','rows':24,'cols':80,'argv':['/bin/sh','-c','printf KEEP_WORKING;exec /bin/sleep 600']})
 original=(sock.stat().st_ino,[(p['id'],p['pid'])for p in call(sock,'pane.list',{'session':row['session']})])
 newer=r/'new-gui';newer.mkdir(mode=0o700);wrapper=r/'gui-entry'
 wrapper.write_text('#!/usr/bin/python3\nimport os,sys\nos.execv(os.environ["TEST_ENGINE_GUI"],[os.environ["TEST_ENGINE_GUI"],*sys.argv[1:],"--fixture-dir",os.environ["TEST_ENGINE_FIXTURE"]])\n');wrapper.chmod(0o700)
 older=r/'old-gui';older.mkdir(mode=0o700)
 first_gui=a.gui;first_env=env
 if a.via_updates:
  packaged=r/'installed';(packaged/'bin').mkdir(parents=True);(packaged/'share/ruwac').mkdir(parents=True)
  os.link(a.binary,packaged/'bin/ruwac');os.link(a.gui,packaged/'bin/ruwac-desktop')
  (packaged/'share/ruwac/rpm-installation.json').write_text('{"kind":"distribution"}')
  first_gui=packaged/'bin/ruwac-desktop'
  first_env=dict(env,RUWAC_GUI_ENTRY=str(wrapper),TEST_ENGINE_GUI=str(a.gui),TEST_ENGINE_FIXTURE=str(newer),RUWAC_STATE_FILE=str(r/'old-engine-checkpoint.json'))
 first=start([first_gui,'--new-instance','--socket',sock,'--session',row['session'],'--fixture-dir',older],'original-gui',first_env)
 state=eventually(lambda:(s if (s:=command(older,'inspect')).get('revision')is not None else None),20)
 check('preserved engine visibly identified','local engine 0.4.49 (preserved)'in state['status'])
 check('older live terminal remains healthy',state['tab_status']=='Ready')
 primary=Path(env['XDG_CONFIG_HOME'])/'RUWAC'
 eventually(lambda:bool(list(primary.glob('workspace*.json'))),10)
 before={str(f):hashlib.sha256(f.read_bytes()).hexdigest()for f in primary.glob('workspace*.json')}
 if a.via_updates:
  update=command(older,'updates',operation='open')
  check('COPR Updates exposes explicit engine action',update['engine_enabled'])
  command(older,'updates',click='update-engine')
 else:
  second=start([a.binary,'--socket',sock,'engine','gui'],'updated-gui',dict(env,RUWAC_GUI_ENTRY=str(wrapper),TEST_ENGINE_GUI=str(a.gui),TEST_ENGINE_FIXTURE=str(newer)))
 state=eventually(lambda:(s if (s:=command(newer,'inspect')).get('revision')is not None else None),20)
 version=subprocess.check_output([str(a.binary),'--version'],text=True).strip().split()[-1]
 check('new mapped GUI uses updated engine',state['status'].endswith('Engine '+version))
 check('new terminal healthy',state['tab_status']=='Ready'and state['tabs']==1)
 # The mapped GUI creates engine-VERSION.gui.sock beside the engine socket.
 # Exercise GUI-first enumeration explicitly; it must never receive engine RPCs.
 expected_socket=sock.parent/('engine-'+version+'.sock')
 candidates=sorted(sock.parent.glob('engine-*.sock'),key=lambda p:p==expected_socket)
 assert expected_socket.with_suffix('.gui.sock')in candidates and candidates[0]!=expected_socket
 matches=[p for p in candidates if p==expected_socket];assert len(matches)==1
 new_socket=matches[0]
 check('independent new session namespace',all(s['id']!=row['session']for s in call(new_socket,'session.list')))
 check('original socket and PTY unchanged',original==(sock.stat().st_ino,[(p['id'],p['pid'])for p in call(sock,'pane.list',{'session':row['session']})]))
 check('original mapped GUI remains usable',command(older,'inspect')['tab_status']=='Ready'and first.poll()is None)
 check('new workspace lives in independent window directory',eventually(lambda:bool(list((primary/'windows').glob('window_*/workspace*.json'))),10))
 check('original saved workspace untouched',before=={str(f):hashlib.sha256(f.read_bytes()).hexdigest()for f in primary.glob('workspace*.json')})
 (a.evidence/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'original_socket_and_pty':original,'new_status':state['status'],'old_state_preserved':True},indent=2)+'\n')
finally:
 if a.via_updates and 'newer' in locals() and (newer/'gui-response.json').exists():
  try:command(newer,'quit')
  except Exception:pass
 for child in reversed(children):
  if child is not children[0] and child.poll()is None:child.terminate()
 for path in [new_socket,sock]:
  if path and path.exists():
   try:call(path,'daemon.stop')
   except Exception:pass
 for child in children:
  if child.poll()is None:child.terminate()
  child.wait(timeout=8)
