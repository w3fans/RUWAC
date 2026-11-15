#!/usr/bin/env python3
"""Generated displayless agent and real TUI PTY; no owner clipboard or process."""
import argparse,hashlib,json,os,shutil,socket,struct,subprocess,sys,tempfile,time,zlib
from pathlib import Path
from spike import Client,Tui,eventually,exact
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--engine',required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args()
root=Path(tempfile.mkdtemp(prefix='lih-',dir=Path('/run/user')/str(os.getuid())));root.chmod(0o700)
env={k:v for k,v in os.environ.items()if k not in {'DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY'}}
for name in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']:
 d=root/name;d.mkdir(mode=0o700);env[name]=str(d)
env.update(TERM='xterm-256color',SHELL='/bin/sh',ENV='')
runtime=Path(env['XDG_RUNTIME_DIR']);offers=runtime/'ruwac-image-offers';offers.mkdir(mode=0o700)
receiver=root/'receiver.py';receiver.write_text("""import os,sys,tty\nfrom pathlib import Path\ntty.setraw(0);p=Path(sys.argv[1]);p.touch(mode=0o600)\nassert not any(os.environ.get(k)for k in ['DISPLAY','WAYLAND_DISPLAY','XAUTHORITY'])\nos.write(1,b'\\x1b[?2004hGPT-6.1-Sol\\r\\n> Ask Codex\\r\\n')\nwith p.open('ab',buffering=0)as f:\n while True:\n  b=os.read(0,8192)\n  if not b:break\n  f.write(b)\n""")
agent=root/'codex';shutil.copy2(sys.executable,agent);agent.chmod(0o700)
# A valid, private PNG; the production engine validates it again.
def chunk(k,b):return struct.pack('!I',len(b))+k+b+struct.pack('!I',zlib.crc32(k+b)&0xffffffff)
image=root/'owned.png';image.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',1,1,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(b'\0\xff\0\0'))+chunk(b'IEND',b''));image.chmod(0o600)
sock=root/'control.sock';log=(root/'engine.log').open('wb');engine=subprocess.Popen([a.engine,'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log);tuis=[];listeners=[];checks=[]
def rpc(m,p=None):
 with Client(sock)as c:return c.call(m,p)
def create(label,program=agent):
 receipt=root/(label+'.bin');s=rpc('session.create',{'name':label,'argv':[str(program),str(receiver),str(receipt)]});rpc('pane.wait',{'pane':s['pane'],'contains':'GPT-','timeout_ms':5000});return s,receipt
def tui(session,label,binary=None):
 t=Tui(binary or a.binary,sock,session['session'],root/(label+'.pty'),env=env);tuis.append(t);eventually(lambda:b'GPT-'in t.data);return t
def offer(t):
 start=Path(f'/proc/{t.p.pid}/stat').read_text().rsplit(') ',1)[1].split()[19]
 path=offers/f'{t.p.pid}-{start}.sock';listener=socket.socket(socket.AF_UNIX);listener.settimeout(3);listener.bind(str(path));path.chmod(0o600);listener.listen(4);listeners.append((listener,path));return listener
def receive(peer):return json.loads(exact(peer,struct.unpack('!I',exact(peer,4))[0]))
def send(peer,value):
 data=json.dumps(value).encode();peer.sendall(struct.pack('!I',len(data))+data)
def accepted(t,listener):
 t.send(b'\x16');peer,_=listener.accept();peer.settimeout(3);pid,uid,_=struct.unpack('3i',peer.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert pid==t.p.pid and uid==os.getuid();assert receive(peer)=={'version':1,'kind':'local-image'};return peer
try:
 eventually(sock.exists)
 s,receipt=create('success');t=tui(s,'success');pid=rpc('job.query',{'pane':s['pane']})['foreground']['pid'];identity=Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()[19]
 listener=offer(t);peer=accepted(t,listener)
 send(peer,{'version':1,'path':str(image)});ack=receive(peer);peer.close();expected=b'\x1b[200~'+str(image).encode()+b'\x1b[201~';eventually(lambda:receipt.read_bytes()==expected);assert ack['queued']and ack['submitted']is False
 t.finish();tuis.remove(t);assert rpc('job.query',{'pane':s['pane']})['foreground']['pid']==pid and Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()[19]==identity
 checks.append({'id':'displayless-agent-receives-one-PNG-no-Enter-old-engine-stable-PID','PASS':True})
 # No offer: retain native Ctrl+V exactly, with no asynchronous worker needed.
 s,receipt=create('native-fallback');t=tui(s,'native-fallback');t.send(b'\x16');eventually(lambda:receipt.read_bytes()==b'\x16');t.finish();tuis.remove(t);checks.append({'id':'no-offer-native-Ctrl-V-preserved','PASS':True})
 # Typing revokes the pending GUI offer without delaying literal input.
 s,receipt=create('typing-cancel');t=tui(s,'typing-cancel');listener=offer(t);peer=accepted(t,listener);began=time.monotonic();t.send(b'AFTER');eventually(lambda:receipt.read_bytes()==b'AFTER');assert time.monotonic()-began<1;assert peer.recv(1)==b'';peer.close();t.finish();tuis.remove(t);checks.append({'id':'typing-cancels-handoff-and-stays-responsive','PASS':True})
 # Producer focus revocation before path delivery cannot inject any bytes.
 s,receipt=create('focus-cancel');t=tui(s,'focus-cancel');listener=offer(t);peer=accepted(t,listener);peer.close();time.sleep(.15);assert receipt.read_bytes()==b'';t.send(b'LIVE');eventually(lambda:receipt.read_bytes()==b'LIVE');t.finish();tuis.remove(t);checks.append({'id':'GUI-focus-EOF-no-input','PASS':True})
 # A shell with an agent-looking footer cannot claim an attachment.
 s,receipt=create('forged-footer',sys.executable);t=tui(s,'forged-footer');listener=offer(t);t.send(b'\x16');peer,_=listener.accept();peer.settimeout(3);assert peer.recv(1)==b'';peer.close();assert receipt.read_bytes()==b'';t.finish();tuis.remove(t);checks.append({'id':'forged-agent-footer-refused','PASS':True})
 out={'PASS':True,'checks':checks,'CLI_sha256':hashlib.sha256(Path(a.binary).read_bytes()).hexdigest(),'engine_sha256':hashlib.sha256(Path(a.engine).read_bytes()).hexdigest(),'synthetic_agent_not_provider_acceptance':True,'owner_clipboard_read_or_written':False}
 (root/'acceptance.json').write_text(json.dumps(out,indent=2)+'\n');print('LOCAL_IMAGE_HANDOFF PASS',len(checks),root,flush=True)
finally:
 for t in tuis:
  try:t.finish('term')
  except Exception:pass
 for listener,path in listeners:listener.close();path.unlink(missing_ok=True)
 if engine.poll()is None:
  try:rpc('daemon.stop')
  except Exception:engine.terminate()
  engine.wait(timeout=8)
 log.close()
 destination=a.evidence/root.name;destination.mkdir(mode=0o700)
 for item in root.rglob('*'):
  target=destination/item.relative_to(root)
  if item.is_dir():target.mkdir(mode=0o700,exist_ok=True)
  elif item.is_file()and not item.is_symlink():target.parent.mkdir(mode=0o700,parents=True,exist_ok=True);shutil.copy2(item,target)
 assert root.parent==Path('/run/user')/str(os.getuid())and root.name.startswith('lih-')and not root.is_symlink()
 shutil.rmtree(root)
