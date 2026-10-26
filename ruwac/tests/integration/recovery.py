#!/usr/bin/env python3
"""Real daemon crash/restart checkpoints; every child/socket/file is fixture-owned."""
import argparse,hashlib,json,os,signal,stat,subprocess,sys,tempfile,time
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--evidence',required=True);a=p.parse_args()
binary=Path(a.binary).resolve();root=Path(tempfile.mkdtemp(prefix='recovery-',dir=a.evidence));root.chmod(0o700)
sock=root/'control.sock';state=root/'durable.json';env={**os.environ,'RUWAC_STATE_FILE':str(state)};server=None;owned=[];results=[];checkpoint_client=None
def start(name='daemon',socket_path=None,configuration=None):
 global server
 server=subprocess.Popen([str(binary),'--socket',str(socket_path or sock),'serve'],env=configuration or env,stdout=(root/(name+'.log')).open('wb'),stderr=subprocess.STDOUT)
 eventually(lambda:health(socket_path or sock),10)
def health(path):
 try:
  with Client(path)as c:return c.call('health')
 except (OSError,EOFError):return None
def rpc(method,params=None,error=None):
 with Client(sock)as c:return c.call(method,params,error=error)
def stop(crash=False):
 global server
 if crash:server.kill()
 else:rpc('daemon.stop')
 server.wait(timeout=10);server=None

def page(pane):
 out=[];offset=0;epoch=None
 while True:
  q={'pane':pane,'offset':offset,'lines':500}
  if epoch is not None:q['epoch']=epoch
  r=rpc('pane.history',q);epoch=r['epoch'];out.append(r)
  if r['next_offset']>=r['total_lines']or r['next_offset']==offset:break
  offset=r['next_offset']
 return {'text':''.join(r['text']for r in out),'wrapped':sum((r['wrapped']for r in out),[]),'total':out[-1]['total_lines']}
def checkpoint_current():
 h=rpc('health');persistence=h['persistence']
 return not persistence['pending'] and persistence['checkpoint_revision']==h['revision'] and persistence['error']is None
def reject(name,path,configuration=None):
 before=path.read_bytes()if path.is_file()else None
 proc=subprocess.Popen([str(binary),'--socket',str(root/(name+'.sock')),'serve'],env=configuration or {**env,'RUWAC_STATE_FILE':str(path)},stdout=(root/(name+'.log')).open('wb'),stderr=subprocess.STDOUT)
 try:
  assert proc.wait(timeout=5)!=0,name
  if before is not None:assert path.read_bytes()==before,name
 finally:
  if proc.poll()is None:proc.kill();proc.wait()
try:
 start()
 script=root/'writer.py';script.write_text("import sys,time\nfor i in range(5200):\n print(f'line-{i:05} ž界e\\u0301🧪\\tTAB',flush=True)\nsys.stdout.write('\\x1b[38;2;1;2;3mFINAL-CELLS ž界e\\u0301\\x1b[0m');sys.stdout.flush()\ntime.sleep(600)\n")
 one=rpc('session.create',{'name':'recover Unicode','rows':24,'cols':80,'argv':[sys.executable,str(script)]});owned.append(one['pid'])
 eventually(lambda:'FINAL-CELLS' in rpc('pane.capture',{'pane':one['pane']})['text'])
 # A nested persistent layout and foreground application state are captured separately.
 second=rpc('pane.create',{'window':one['window'],'orientation':'vertical','argv':['/bin/bash','--noprofile','--norc']});owned.append(second['pid'])
 rpc('pane.sendText',{'pane':second['pane'],'text':"printf '\\033[?1049h\\033[?1000h\\033[?2004hALT-SCREEN'; sleep 600\n"})
 eventually(lambda:'ALT-SCREEN' in rpc('pane.capture',{'pane':second['pane']})['text'])
 third=rpc('pane.create',{'window':one['window'],'pane':second['pane'],'orientation':'horizontal','argv':['/bin/sh','-c','printf EXIT-23; exit 23']});owned.append(third['pid'])
 eventually(lambda:next(x for x in rpc('pane.list',{'session':one['session']})if x['id']==third['pane'])['ended'])
 eventually(lambda:next(x for x in rpc('pane.list',{'session':one['session']})if x['id']==third['pane'])['exit_code']==23)
 rpc('window.rename',{'window':one['window'],'name':'restored layout'})
 rpc('pane.select',{'pane':one['pane']})
 rpc('buffer.set',{'session':one['session'],'text':'PRIVATE-REGISTER-SENTINEL-DO-NOT-PERSIST'})
 # Keep the sizing client attached through snapshot/checkpoint/SIGKILL.
 # Closing it here races the baseline against an asynchronous detach resize.
 checkpoint_client=Client(sock)
 checkpoint_client.call('session.attach',{'session':one['session'],'rows':24,'cols':80})
 before=checkpoint_client.call('client.render');before_ids=[x['id']for x in before['panes']]
 (root/'recovery-grid-before.json').write_text(json.dumps(before)+'\n')
 captures={pid:rpc('pane.capture',{'pane':pid,'lines':2000})['text']for pid in before_ids}
 histories={pid:page(pid)for pid in before_ids}
 assert '\t'not in histories[one['pane']]['text'] and 'TAB'in histories[one['pane']]['text']
 rpc('daemon.checkpoint');data=state.read_bytes();assert stat.S_IMODE(state.stat().st_mode)==0o600
 assert b'PRIVATE-REGISTER-SENTINEL-DO-NOT-PERSIST'not in data
 assert not any(key in json.loads(data)for key in ['clients','argv','input','cache'])
 stop(crash=True);checkpoint_client.close();checkpoint_client=None;start('after-crash')
 panes=rpc('pane.list',{'session':one['session']});assert {p['id']for p in panes}==set(before_ids)
 assert all(p['ended']and p['restored']and p['pid']is None for p in panes),panes
 assert next(p for p in panes if p['id']==third['pane'])['exit_code']==23
 for pid in before_ids:
  assert rpc('pane.capture',{'pane':pid,'lines':2000})['text']==captures[pid]
  assert page(pid)==histories[pid]
  for method,params in [('pane.sendText',{'text':'ignored'}),('pane.sendKey',{'key':'Enter'}),('buffer.paste',{})]:rpc(method,{'pane':pid,**params},error='PANE_EXITED')
 assert rpc('buffer.get',{'session':one['session']})['text']==''
 with Client(sock)as c:
  c.call('session.attach',{'session':one['session'],'rows':24,'cols':80});after=c.call('client.render')
  (root/'recovery-grid-after.json').write_text(json.dumps(after)+'\n')
  assert after['layout']==before['layout']and after['active_pane']==before['active_pane']
  for old,new in zip(before['panes'],after['panes']):
   assert old['grid']['cells']==new['grid']['cells']
   assert new['grid']['restored']and not new['grid']['mouse']and not new['grid']['bracketed_paste']
  c.call('client.resize',{'rows':30,'cols':100});render=c.call('client.render')
  assert all(len(p['grid']['cells'])==p['rect']['rows']for p in render['panes'])
  c.call('session.detach')  # Acknowledge layout changes before checking idle writes.
 results.append({'id':'DAEMON-SIGKILL-RECOVERY','status':'PASS','panes':3,'stable_ids':True,'history_rows':histories[one['pane']]['total'],'canonical_cells_equal':True,'wrapped_unicode_history_equal':True,'tab_cells_restored':True,'plain_tabs_expanded_once':True,'clipboard_register_excluded':True,'restored_input_refused':True,'exit_code':23,'checkpoint_bytes':len(data)})
 # Read-only queries do not trigger recurring disk writes once the checkpoint is current.
 rpc('daemon.checkpoint');eventually(checkpoint_current,10);original=state.stat().st_mtime_ns
 (root/'idle-checkpoint-ready.json').write_text(json.dumps(rpc('health'))+'\n')
 for _ in range(12):rpc('health');rpc('session.list');time.sleep(.2)
 assert state.stat().st_mtime_ns==original
 results.append({'id':'CHECKPOINT-IDLE','status':'PASS','read_only_queries_do_not_write':True})
 # Normal stop/restart and a new socket use the explicitly persistent archive.
 live=rpc('session.create',{'name':'new live after recovery','argv':['/bin/bash','--noprofile','--norc']});owned.append(live['pid'])
 rpc('pane.sendText',{'pane':live['pane'],'text':'printf AUTOMATIC-CHECKPOINT\n'})
 eventually(lambda:state.exists()and live['pane']in state.read_text()and 'AUTOMATIC-CHECKPOINT'in state.read_text(),7)
 stop();old=sock;sock=root/'after-runtime-loss.sock';start('new-runtime')
 assert len(rpc('session.list'))==2
 assert all(p['ended']and p['pid']is None for p in rpc('pane.list',{'session':live['session']}))
 results.append({'id':'DURABLE-PATH-AND-AUTOSAVE','status':'PASS','periodic_write':True,'normal_stop':True,'runtime_socket_changed':True,'processes_not_resurrected':True})
 # Explicit close is durable and IDs disappear on the following restart.
 for row in rpc('pane.list',{'session':live['session']}):rpc('pane.close',{'pane':row['id']})
 stop();start('after-close')
 rpc('pane.list',{'session':live['session']},error='STALE_ID')
 results.append({'id':'DURABLE-CLOSE','status':'PASS','closed_resources_not_restored':True})
 # A failed write preserves the last checkpoint and surfaces its status.
 rpc('daemon.checkpoint');eventually(checkpoint_current,10)
 original=state.read_bytes();state.chmod(0o644)
 rpc('window.rename',{'window':one['window'],'name':'write failure recovered'})
 rpc('daemon.checkpoint',error='CHECKPOINT')
 assert state.read_bytes()==original and rpc('health')['persistence']['error']
 state.chmod(0o600);rpc('daemon.checkpoint');assert rpc('health')['persistence']['error']is None
 results.append({'id':'CHECKPOINT-WRITE-ERROR','status':'PASS','last_checkpoint_preserved':True,'health_reports_failure':True,'retry_recovers':True})
 # Same checkpoint cannot be owned by a second socket/server.
 duplicate=subprocess.Popen([str(binary),'--socket',str(root/'duplicate.sock'),'serve'],env=env,stdout=(root/'duplicate.log').open('wb'),stderr=subprocess.STDOUT)
 assert duplicate.wait(timeout=5)!=0
 stop()
 # The ordinary socket persists to XDG_STATE_HOME across removal of runtime state.
 normal={**os.environ};normal.pop('RUWAC_STATE_FILE',None)
 home=root/'ordinary-home';home.mkdir(mode=0o700);normal['HOME']=str(home)
 normal['XDG_STATE_HOME']=str(home/'state')
 normal_id=None
 for generation in range(2):
  runtime=root/f'runtime-{generation}';runtime.mkdir(mode=0o700);normal['XDG_RUNTIME_DIR']=str(runtime)
  regular_socket=runtime/'ruwac/control.sock'
  proc=subprocess.Popen([str(binary),'serve'],env=normal,stdout=(root/f'ordinary-{generation}.log').open('wb'),stderr=subprocess.STDOUT)
  try:
   eventually(lambda:health(regular_socket),10)
   with Client(regular_socket)as c:
    if generation==0:
     created=c.call('session.create',{'name':'default durable','argv':['/bin/sh','-c','printf ORDINARY-DURABLE; exit 19']});normal_id=created['session'];owned.append(created['pid'])
     eventually(lambda:c.call('pane.list',{'session':normal_id})[0]['ended'])
    else:
     rows=c.call('pane.list',{'session':normal_id});assert rows[0]['restored']and rows[0]['pid']is None
     assert 'ORDINARY-DURABLE'in c.call('pane.capture',{'pane':rows[0]['id']})['text']
    c.call('daemon.stop')
   assert proc.wait(timeout=10)==0
  finally:
   if proc.poll()is None:proc.kill();proc.wait()
  import shutil
  shutil.rmtree(runtime)
 assert (home/'state/RUWAC/terminal-history.json').exists()
 results.append({'id':'ORDINARY-XDG-STATE-RECOVERY','status':'PASS','runtime_removed':True,'default_storage_outside_runtime':True,'same_restored_session_id':True})
 # Corrupt, forged, oversized and unsafe paths remain untouched and do not block.
 valid=state.read_bytes()
 bad=root/'bad.json';bad.write_bytes(b'{invalid');bad.chmod(0o600);reject('corrupt',bad)
 forged=json.loads(valid);forged['sessions'][0]['active']='window_'+'0'*32;bad.write_text(json.dumps(forged));reject('invalid-graph',bad)
 forged=json.loads(valid);grid=forged['panes'][0]['terminal']['grid']
 if grid.get('cell_encoding')=='palette-v1':grid['cell_palette'][0]=['\u001b',False,0,0,False,False]
 else:grid['cells'][0][0]=['\u001b',False,0,0,False,False]
 bad.write_text(json.dumps(forged));reject('unsafe-canonical-cell',bad)
 forged=json.loads(valid);forged['schema']=99;bad.write_text(json.dumps(forged));reject('unknown-schema',bad)
 bad.write_bytes(valid);bad.chmod(0o644);reject('unsafe-mode',bad);bad.chmod(0o600)
 linked=root/'linked.json';linked.symlink_to(bad);reject('symlink',linked)
 hard=root/'hard.json';os.link(bad,hard);reject('hardlink',hard);hard.unlink()
 fifo=root/'fifo.json';os.mkfifo(fifo,0o600);reject('fifo',fifo)
 large=root/'large.json';large.touch(mode=0o600)
 with large.open('r+b')as f:f.truncate(128*1024*1024+1)
 reject('oversized',large)
 results.append({'id':'CHECKPOINT-BOUNDARIES','status':'PASS','negative_cases':['parallel owner','corrupt','graph','cell control','schema','mode','symlink','hardlink','fifo','oversized'],'original_files_preserved':True})
finally:
 if checkpoint_client is not None:checkpoint_client.close()
 if server and server.poll()is None:
  server.kill();server.wait(timeout=5)
 # Only fixture-owned child PIDs are touched; avoid recycled PIDs via session identity.
 for pid in owned:
  try:
   if os.getsid(pid)==pid:os.killpg(pid,signal.SIGKILL)
  except ProcessLookupError:pass
 receipt={'results':results,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'evidence':str(root)}
 (root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 print(json.dumps(receipt,indent=2))
