#!/usr/bin/env python3
"""Installed/live discovery and explicit coexistence preserve an older engine."""
import argparse, concurrent.futures, json, os, subprocess, sys, tempfile
from pathlib import Path
from spike import Client, eventually
p=argparse.ArgumentParser()
p.add_argument('--binary',required=True,type=Path)
p.add_argument('--legacy',required=True,type=Path)
p.add_argument('--evidence',required=True,type=Path)
a=p.parse_args();binary=a.binary.resolve();legacy=a.legacy.resolve()
for key in ['RUWAC_STATE_FILE','RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:
 os.environ.pop(key,None)
r=Path(tempfile.mkdtemp(prefix='engines-',dir=os.environ.get('TMPDIR')))
r.chmod(0o700);os.environ['XDG_STATE_HOME']=str(r/'state');socket=r/'old.sock';children=[];clients=[];new_socket=None;nested_socket=None
checks=[]
def check(name, condition):
 assert condition,name
 checks.append(name)
def cli(args,env=None,ok=True):
 q=subprocess.run([str(binary),*map(str,args)],env=env,capture_output=True,text=True,timeout=15)
 if ok:
  assert q.returncode==0,(args,q.stdout,q.stderr)
  return json.loads(q.stdout)['result']
 assert q.returncode!=0,(args,q.stdout)
 return q

def peer_pid(path):
 import socket as s,struct
 with s.socket(s.AF_UNIX,s.SOCK_STREAM)as c:
  c.connect(str(path));return struct.unpack('3i',c.getsockopt(s.SOL_SOCKET,s.SO_PEERCRED,12))[0]

def server(path,exe):
 log=(r/(path.name+'.log')).open('w')
 try:c=subprocess.Popen([str(exe),'--socket',str(path),'serve'],stdout=log,stderr=subprocess.STDOUT)
 finally:log.close()
 children.append(c);eventually(path.exists);return c

def connect(path):
 c=Client(path);clients.append(c);return c
try:
 missing=cli(['--socket',socket,'engine','status','-j'])
 check('read-only absent discovery never starts engine',missing['running'] is False and not socket.exists())
 cli(['--socket',socket,'engine','gui'],env=dict(os.environ,RUWAC_GUI_ENTRY=str(r/'missing-gui')),ok=False)
 check('missing GUI cannot start an unused engine',not list(r.glob('engine-*.sock')))
 old=server(socket,legacy);c=connect(socket)
 row=c.call('session.create',{'name':'preserved-old','rows':24,'cols':80,'argv':['/bin/sh','-c','printf PRESERVED_ENGINE;exec /bin/sleep 600']})
 panes=c.call('pane.list',{'session':row['session']})
 before=(socket.stat().st_ino,peer_pid(socket),[(v['id'],v['pid'])for v in panes])
 report=cli(['--socket',socket,'engine','status'])
 check('installed and live versions distinguished',report['running'] and not report['matches_installed'] and report['running_engine']!=report['installed_cli'])
 with concurrent.futures.ThreadPoolExecutor(max_workers=2)as pool:
  starts=list(pool.map(lambda _:cli(['--socket',socket,'engine','new']),range(2)))
 q=next(v for v in starts if v['start_requested']);new_socket=Path(q['engine']['socket'])
 check('concurrent launches converge on one engine',len({v['engine']['socket']for v in starts})==1 and all(v['engine']['matches_installed']for v in starts))
 check('new engine separate and verified',new_socket!=socket and q['engine']['matches_installed'] and q['start_requested'])
 check('durable independent checkpoint',Path(q['history']).is_relative_to(Path(os.environ['XDG_STATE_HOME'])) and q['history']!=str(socket.with_suffix('.history.json')))
 check('no migration',not q['sessions_migrated'])
 n=connect(new_socket)
 check('new namespace empty',n.call('session.list')==[])
 fresh=n.call('session.create',{'name':'new-work','rows':24,'cols':80,'argv':['/bin/sh','-c','printf "INDEPENDENT_HISTORY\\nSOCKET=%s\\nSTATE=%s\\n" "$RUWAC_SOCKET" "${RUWAC_STATE_FILE-unset}";exec /bin/sleep 600']})
 n.call('pane.wait',{'pane':fresh['pane'],'contains':'SOCKET='+str(new_socket),'timeout_ms':5000})
 check('new shell commands inherit their own engine socket','SOCKET='+str(new_socket)in n.call('pane.capture',{'pane':fresh['pane'],'lines':50})['text'])
 check('engine checkpoint override never leaks into a new shell','STATE=unset'in n.call('pane.capture',{'pane':fresh['pane'],'lines':50})['text'])
 nested_socket=r/'nested.sock'
 nested_code='import os,subprocess,sys,time;subprocess.run([sys.argv[1],"--socket",sys.argv[2],"start"],check=True);print("NESTED_READY",flush=True);time.sleep(600)'
 runner=n.call('session.create',{'name':'nested-launch','rows':24,'cols':120,'argv':[sys.executable,'-c',nested_code,str(binary),str(nested_socket)]})
 n.call('pane.wait',{'pane':runner['pane'],'contains':'NESTED_READY','timeout_ms':10000})
 nested=connect(nested_socket)
 nested_row=nested.call('session.create',{'name':'nested-own-state','rows':24,'cols':120,'argv':['/bin/sh','-c','printf "NESTED_SOCKET=%s\\n" "$RUWAC_SOCKET";exec /bin/sleep 600']})
 nested.call('pane.wait',{'pane':nested_row['pane'],'contains':'NESTED_SOCKET='+str(nested_socket),'timeout_ms':5000})
 check('explicit nested engine owns its child client routing','NESTED_SOCKET='+str(nested_socket)in nested.call('pane.capture',{'pane':nested_row['pane'],'lines':50})['text'])
 n.call('daemon.checkpoint');nested.call('daemon.checkpoint')
 parent_history=Path(q['history']).read_text();nested_history=nested_socket.with_suffix('.history.json').read_text()
 check('nested engine never opens parent checkpoint',fresh['session']in parent_history and nested_row['session']not in parent_history and nested_row['session']in nested_history and fresh['session']not in nested_history)
 nested.call('daemon.stop');nested.close();clients.remove(nested);eventually(lambda:not nested_socket.exists())
 n.call('session.attach',{'session':fresh['session'],'rows':24,'cols':80})
 frame=n.call('client.render',{'cell_encoding':'palette-v1'})
 check('new engine actually activates palette frames',frame['panes'][0]['grid'].get('cell_encoding')=='palette-v1')
 again=cli(['--socket',socket,'engine','new','--json'])
 check('reuse same updated engine',not again['start_requested'] and peer_pid(new_socket)!=old.pid and again['history']==q['history'])
 check('original listener and PTY preserved',before==(socket.stat().st_ino,peer_pid(socket),[(v['id'],v['pid'])for v in c.call('pane.list',{'session':row['session']})]))
 check('existing live work still available',not c.call('pane.list',{'session':row['session']})[0]['ended'])
 n.call('pane.wait',{'pane':fresh['pane'],'contains':'INDEPENDENT_HISTORY','timeout_ms':5000})
 n.call('daemon.checkpoint');n.call('daemon.stop');n.close();clients.remove(n)
 eventually(lambda:not new_socket.exists())
 # The returned argv includes the history override; restart via the actual
 # command path and verify recovery, rather than merely inspecting the string.
 argv=q['terminal_argv'][:-2]+['start']
 argv[argv.index('ruwac')]=str(binary)
 restarted=subprocess.run(argv,capture_output=True,text=True,timeout=15)
 check('returned launcher restarts successfully',restarted.returncode==0)
 n=connect(new_socket)
 restored=n.call('session.list')
 check('durable recovered text', 'INDEPENDENT_HISTORY' in n.call('pane.capture',{'pane':fresh['pane'],'lines':50})['text'])
 check('independent durable recovery',any(v['id']==fresh['session'] and v.get('restored')for v in restored) and all(v['id']!=row['session']for v in restored))
 # A recording GUI entry proves socket, new-window namespace and checkpoint
 # reach the actual exec path without opening an owner's desktop.
 entry=r/'gui-entry';gui_record=r/'gui-launch.json'
 entry.write_text('#!/usr/bin/python3\nimport json,os,sys\nfrom pathlib import Path\nPath(os.environ["ENGINE_GUI_RECORD"]).write_text(json.dumps({"argv":sys.argv[1:],"history":os.environ.get("RUWAC_STATE_FILE"),"socket":os.environ.get("RUWAC_SOCKET")}))\n');entry.chmod(0o700)
 env=dict(os.environ,RUWAC_GUI_ENTRY=str(entry),ENGINE_GUI_RECORD=str(gui_record))
 launch=subprocess.run([str(binary),'--socket',str(socket),'engine','gui'],env=env,capture_output=True,text=True,timeout=15)
 check('GUI exec succeeds',launch.returncode==0)
 gui=json.loads(gui_record.read_text())
 check('independent GUI window and durable restart environment',gui['argv'][:3]==['--socket',str(new_socket),'--new-instance'] and gui['argv'][3]=='--window-id' and gui['argv'][4].startswith('window_') and gui['history']==q['history'] and gui['socket']==str(new_socket))
 n.call('daemon.stop');n.close();clients.remove(n);eventually(lambda:not new_socket.exists())
 foreign=server(new_socket,legacy);foreign_inode=new_socket.stat().st_ino
 cli(['--socket',socket,'engine','new'],ok=False)
 check('different existing version refused without replacement',foreign.poll()is None and new_socket.stat().st_ino==foreign_inode)
 e=dict(os.environ,RUWAC_STATE_FILE=str(r/'custom.json'))
 cli(['--socket',socket,'engine','new'],env=e,ok=False)
 check('explicit checkpoint never silently shared',not(r/'custom.json').exists())
 cli(['--socket','relative/sock','engine','new'],ok=False)
 check('relative endpoints refused',True)
 unsafe=r/'unsafe';unsafe.mkdir(mode=0o755)
 cli(['--socket',unsafe/'sock','engine','status'],ok=False)
 check('unsafe parent refused',not(unsafe/'sock').exists())
 link=r/'linked';link.symlink_to(r,target_is_directory=True)
 cli(['--socket',link/'sock','engine','new'],ok=False)
 check('symlink parent refused',True)
 cli(['--socket',socket,'engine','new','unexpected'],ok=False)
 check('unexpected arguments fail before side effects',foreign.poll()is None)
 # SSH transport must run discovery on its endpoint, never mistake this CLI
 # for the remote installed version. A strict recording transport tests routing.
 shim=r/'ssh';record=r/'ssh-args.json'
 shim.write_text('#!/usr/bin/python3\nimport json,os,sys\nfrom pathlib import Path\nPath(os.environ["ENGINE_SSH_RECORD"]).write_text(json.dumps(sys.argv[1:]))\nos.execv("/bin/sh",["sh","-c",sys.argv[-1]])\n');shim.chmod(0o700)
 env=dict(os.environ,PATH=str(r)+os.pathsep+os.environ['PATH'],ENGINE_SSH_RECORD=str(record))
 remote=cli(['--host','fixture','--remote-binary',binary,'--socket',socket,'engine','status'],env=env)
 arguments=json.loads(record.read_text())
 check('remote discovery stays on requested endpoint',remote['socket']==str(socket) and remote['running_engine']==report['running_engine'] and 'StrictHostKeyChecking=yes'in arguments and 'ForwardAgent=no'in arguments)
 cli(['--host','fixture','engine','new'],env=env,ok=False)
 check('remote new cannot accidentally start local engine',foreign.poll()is None)
 a.evidence.mkdir(parents=True,exist_ok=True)
 (a.evidence/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'legacy_version':report['running_engine'],'installed_candidate':report['installed_cli'],'original_socket_and_pty':before},indent=2)+'\n')
finally:
 for path in [nested_socket,new_socket,socket]:
  if path and path.exists():
   try:
    with Client(path)as x:x.call('daemon.stop')
   except Exception:
    try:
     x=Client(path);x.call('daemon.stop');x.close()
    except Exception:pass
 for c in clients:
  try:c.close()
  except Exception:pass
 for child in children:
  if child.poll()is None:child.terminate()
  child.wait(timeout=8)
