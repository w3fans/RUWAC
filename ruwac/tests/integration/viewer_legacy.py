#!/usr/bin/env python3
"""Current viewer CLI must refuse a real published older daemon before attach."""
import argparse,hashlib,json,os,subprocess,sys,tempfile,termios
from pathlib import Path
from spike import Client,Tui,eventually
p=argparse.ArgumentParser(description=__doc__)
for k in ['binary','legacy','evidence']:p.add_argument('--'+k,type=Path,required=True)
a=p.parse_args();a.evidence.mkdir(parents=True,exist_ok=True);root=Path(tempfile.mkdtemp(prefix='vl-'));root.chmod(0o700);sock=root/'s';children=[];tuis=[]
try:
 with(a.evidence/'daemon.log').open('wb')as log:daemon=subprocess.Popen([str(a.legacy),'--socket',str(sock),'serve'],stdout=log,stderr=log,env={**os.environ,'RUWAC_STATE_FILE':str(root/'checkpoint.json')})
 children.append(daemon);eventually(sock.exists)
 with Client(sock)as owner:
  source=Path(__file__).resolve().parents[1]
  fixture=source/'fixtures/sizing_receiver.py'
  s=owner.call('session.create',{'name':'Generated old daemon','rows':40,'cols':100,'argv':[sys.executable,str(fixture),str(root)]})
  owner.call('pane.wait',{'pane':s['pane'],'contains':'SIZING_READY','timeout_ms':5000});owner.call('session.attach',{'session':s['session'],'rows':40,'cols':100})
  owner.call('client.input',{'pane':s['pane'],'kind':'keyboard','text':'?'});eventually(lambda:(root/'state.json').exists());before=json.loads((root/'state.json').read_text())
  t=Tui(a.binary,sock,s['session'],a.evidence/'refusal.pty',rows=18,cols=50,command=['attach','--session',s['session'],'--viewer']);tuis.append(t)
  eventually(lambda:t.p.poll()is not None,8);assert t.p.returncode!=0 and b'cannot attach a non-sizing viewer' in t.data,bytes(t.data)
  assert termios.tcgetattr(t.slave)==t.old,'Capability refusal changed terminal modes'
  assert t.finish('kill')!=0;tuis.remove(t)
  owner.call('client.input',{'pane':s['pane'],'kind':'keyboard','text':'?'})
  eventually(lambda:json.loads((root/'state.json').read_text())['bytes']==before['bytes']+1)
  after=json.loads((root/'state.json').read_text());assert all(after[k]==before[k]for k in ['pid','rows','cols','signals'])
  owner.call('daemon.stop');daemon.wait(timeout=5)
  (a.evidence/'acceptance.json').write_text(json.dumps({'PASS':True,'no_attach_resize_or_input':True,'tty_modes_preserved':True,'before':before,'after':after,'binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'legacy_sha256':hashlib.sha256(a.legacy.read_bytes()).hexdigest()},indent=2)+'\n')
  print('Actual older-daemon viewer refusal PASS',flush=True)
finally:
 for t in tuis:t.finish('term'if t.p.poll()is None else'kill')
 for child in children:
  if child.poll()is None:child.terminate();child.wait(timeout=5)
 # The enclosing bounded wrapper owns/removes the short temporary root.
