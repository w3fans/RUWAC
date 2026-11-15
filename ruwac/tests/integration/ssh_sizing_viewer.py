#!/usr/bin/env python3
"""Actual authenticated SSH viewer/legacy refusal, using bounded generated peers."""
import argparse,hashlib,json,os,shlex,subprocess,sys,time,fcntl,termios,struct,signal
from pathlib import Path
from spike import Tui,eventually
p=argparse.ArgumentParser(description=__doc__)
for k in ['binary','host','info','evidence']:p.add_argument('--'+k,required=True)
a=p.parse_args();e=Path(a.evidence);e.mkdir(parents=True,exist_ok=True);info=json.loads(Path(a.info).read_text());binary=Path(a.binary).resolve();cases=[];tuis=[]
ssh=['ssh','-T','-oBatchMode=yes','-oStrictHostKeyChecking=yes','-oForwardAgent=no','-oClearAllForwardings=yes','-oConnectTimeout=8',a.host]
def read(path):return subprocess.check_output(ssh+[shlex.join(['python3','-c','from pathlib import Path;import sys;sys.stdout.buffer.write(Path(sys.argv[1]).read_bytes())',path])],timeout=15)
def cli(args,old=False,ok=True):
 q=info['legacy'if old else'current'];base=[str(binary),'--host',a.host,'--remote-binary',info['cli'],'--socket',q['socket']]
 result=subprocess.run(base+args,capture_output=True,timeout=30)
 assert (result.returncode==0)==ok,(args,result.stdout,result.stderr)
 return result
try:
 q=info['current'];s=q['session'];initial=json.loads(read(q['root']+'/state.json'))
 command=['--host',a.host,'--remote-binary',info['cli'],'attach','--session',s['session'],'-O']
 viewer=Tui(binary,Path(q['socket']),s['session'],e/'viewer.pty',rows=18,cols=50,command=command);tuis.append(viewer)
 eventually(lambda:b'Viewer' in viewer.data or viewer.p.poll()is not None,20);assert viewer.p.poll()is None,bytes(viewer.data)
 viewer.send(b'?');eventually(lambda:json.loads(read(q['root']+'/state.json'))['bytes']==initial['bytes']+1)
 after=json.loads(read(q['root']+'/state.json'));assert all(after[k]==initial[k]for k in ['rows','cols','signals','pid'])
 cases.append('SSH-VIEWER-INPUT-PRESERVES-REAL-PTY')
 seen=len(viewer.data)
 fcntl.ioctl(viewer.master,termios.TIOCSWINSZ,struct.pack('HHHH',12,35,0,0));assert os.getpgid(viewer.p.pid)==viewer.p.pid;os.killpg(viewer.p.pid,signal.SIGWINCH)
 eventually(lambda:b'\x1b[12;1H' in viewer.data[seen:],15)
 viewer.send(b'?');eventually(lambda:json.loads(read(q['root']+'/state.json'))['bytes']==initial['bytes']+2)
 after=json.loads(read(q['root']+'/state.json'));assert all(after[k]==initial[k]for k in ['rows','cols','signals','pid'])
 assert viewer.finish()==0;tuis.remove(viewer);cases.append('SSH-VIEWER-RESIZE-AND-DETACH-NEVER-SIGNALS-PTY')
 q=info['legacy'];s=q['session'];baseline=json.loads(read(q['root']+'/state.json'));before=read(q['root']+'/input.bin')
 denied=cli(['attach','--session',s['session'],'--viewer'],old=True,ok=False);assert b'non-sizing viewer'in denied.stderr
 assert read(q['root']+'/input.bin')==before
 cli(['pane','send-text','--id',s['pane'],'--text','?'],old=True)
 eventually(lambda:json.loads(read(q['root']+'/state.json'))['bytes']==baseline['bytes']+1)
 after=json.loads(read(q['root']+'/state.json'));assert all(after[k]==baseline[k]for k in ['rows','cols','signals','pid'])
 cases.append('SSH-ACTUAL-OLD-DAEMON-REFUSED-BEFORE-INPUT-OR-ATTACH')
finally:
 for t in tuis:t.finish('kill')
 (e/'acceptance.json').write_text(json.dumps({'PASS':len(cases)==3,'cases':cases,'local_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'remote_hashes':info['hashes'],'scope':'Actual authenticated OpenSSH and generated real PTYs; remote CLI is resource-bound by a socket-guarded shim. No owner sessions or SSH trust changed.'},indent=2)+'\n')
print('SSH viewer qualification PASS',len(cases),flush=True)
