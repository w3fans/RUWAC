#!/usr/bin/env python3
"""Owned remote daemon/PTYs for SSH viewer qualification; run in a bounded job."""
import sys,os,json,subprocess,time,hashlib,shutil,tempfile
from pathlib import Path
binary,legacy,evidence,source=sys.argv[1:];evidence=Path(evidence)
assert evidence.resolve()==Path(os.environ['RUWAC_TEST_RUN_ROOT']).resolve()
sys.path.insert(0,source+'/ruwac/tests/integration')
from spike import Client,eventually
root=Path(tempfile.mkdtemp(prefix='vs-'));root.chmod(0o700)
children=[];clients=[];info={'hashes':{k:hashlib.sha256(Path(v).read_bytes()).hexdigest()for k,v in [('current',binary),('legacy',legacy)]}}
try:
 for name,exe in [('current',binary),('legacy',legacy)]:
  sock=root/(name+'.sock');folder=root/name;folder.mkdir(mode=0o700)
  with(evidence/(name+'.log')).open('wb')as log:
   children.append(subprocess.Popen([exe,'--socket',str(sock),'serve'],stdout=log,stderr=log,env={**os.environ,'RUWAC_STATE_FILE':str(folder/'checkpoint.json')}))
  eventually(sock.exists);c=Client(sock);clients.append(c)
  s=c.call('session.create',{'name':'Generated SSH viewer','rows':40,'cols':100,'argv':[sys.executable,source+'/ruwac/tests/fixtures/sizing_receiver.py',str(folder)]})
  c.call('pane.wait',{'pane':s['pane'],'contains':'SIZING_READY','timeout_ms':5000})
  c.call('session.attach',{'session':s['session'],'rows':40,'cols':100})
  c.call('client.input',{'pane':s['pane'],'kind':'keyboard','text':'?'})
  eventually(lambda:(folder/'state.json').exists())
  info[name]={'socket':str(sock),'root':str(folder),'session':s}
 # Commands started by separate SSH channels join this exact fixture cgroup
 # before the actual product CLI is executed. This preserves bounded lifetime.
 cg=Path('/proc/self/cgroup').read_text().strip().split('::',1)[1]
 group=Path('/sys/fs/cgroup'+cg);assert group.name.startswith('ruwac-bounded-') and int((group/'memory.max').read_text())<=4*1024**3
 # Register only this generated shim's PID if cgroup.procs needs privilege;
 # the product itself is always executed with the original user's identity.
 binder="""import os,re,sys,json,stat
from pathlib import Path
pid,start,uid,cg,evidence=sys.argv[1:];uid=int(uid);proc=Path('/proc',pid);e=Path(evidence)
assert e.stat().st_uid==uid and stat.S_IMODE(e.stat().st_mode)==0o700
q=json.loads((e/'limits.json').read_text())
assert re.fullmatch(r'/user.slice/user-'+str(uid)+r'.slice/user@'+str(uid)+r'.service/app.slice/ruwac-bounded-[a-f0-9]{12}.service',cg)
base=Path('/sys/fs/cgroup'+cg);assert base.name==q['unit']+'.service'
assert 0<int((base/'memory.max').read_text())<=4*1024**3 and int((base/'pids.max').read_text())<=256
assert proc.stat().st_uid==uid and proc.joinpath('stat').read_text().rsplit(')',1)[1].split()[19]==start
assert proc.joinpath('cmdline').read_bytes().split(bytes([0]))[1]==str(e/'cli').encode()
(base/'cgroup.procs').write_text(pid)
assert proc.joinpath('stat').read_text().rsplit(')',1)[1].split()[19]==start
assert proc.joinpath('cgroup').read_text().strip().split('::',1)[1]==cg
"""
 isolated={k:os.environ[k]for k in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_RUNTIME_DIR','RUWAC_TEST_RUN_ROOT']}
 shim=evidence/'cli'
 shim.write_text('#!/usr/bin/python3\nimport os,sys,subprocess\nfrom pathlib import Path\ngroup=Path('+repr(str(group))+')\nassert group.is_dir() and int((group/"memory.max").read_text())<=4*1024**3\na=sys.argv[1:]\nassert "--socket" in a and a[a.index("--socket")+1] in '+repr([info[k]['socket']for k in ['current','legacy']])+'\ntry:(group/"cgroup.procs").write_text(str(os.getpid()))\nexcept PermissionError:\n start=Path("/proc/self/stat").read_text().rsplit(")",1)[1].split()[19]\n subprocess.run(["sudo","-n",sys.executable,"-I","-c",'+repr(binder)+',str(os.getpid()),start,str(os.getuid()),'+repr(cg)+','+repr(str(evidence))+'],check=True,timeout=10)\nos.environ.update('+repr(isolated)+')\nos.execv('+repr(binary)+',['+repr(binary)+',*a])\n')
 shim.chmod(0o700);info['cli']=str(shim);info['shim_sha256']=hashlib.sha256(shim.read_bytes()).hexdigest()
 (evidence/'ready.json').write_text(json.dumps(info));deadline=time.monotonic()+180
 while not(evidence/'stop').exists():
  if any(p.poll()is not None for p in children):raise RuntimeError('fixture daemon exited')
  if time.monotonic()>deadline:raise RuntimeError('finite fixture deadline')
  time.sleep(.1)
finally:
 for c in clients:
  try:c.call('daemon.stop')
  except (OSError,EOFError,AssertionError):pass
  c.close()
 for p in children:
  if p.poll()is None:
   try:p.wait(timeout=5)
   except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=5)
 shutil.rmtree(root)
 print('Owned SSH viewer fixtures stopped',flush=True)
