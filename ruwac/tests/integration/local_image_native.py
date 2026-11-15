#!/usr/bin/env python3
"""Real GTK PNG owner -> local nested TUI -> displayless synthetic agent.
Runs only inside the owned Mutter/DBus fixture. Never reads an owner clipboard.
"""
import argparse,hashlib,json,os,shutil,subprocess,sys,tempfile,time
from pathlib import Path
from spike import Client,eventually
from PIL import Image
from gi.repository import Gio,GLib
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--gui',required=True);p.add_argument('--engine',required=True);p.add_argument('--evidence',type=Path,required=True);p.add_argument('--baseline',action='store_true');p.add_argument('--agent',choices=['codex','claude'],default='codex');a=p.parse_args()
assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
root=Path(tempfile.mkdtemp(prefix='lni-',dir=Path('/run/user')/str(os.getuid())));root.chmod(0o700)
for name in ['config','cache','data','stage']:(root/name).mkdir(mode=0o700)
env={**os.environ,'XDG_CONFIG_HOME':str(root/'config'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_DATA_HOME':str(root/'data'),'GSETTINGS_BACKEND':'memory','GTK_USE_PORTAL':'0','GTK_A11Y':'none'}
headless={k:v for k,v in env.items()if k not in ['DISPLAY','WAYLAND_DISPLAY','XAUTHORITY']}
agent=root/a.agent;READY='GPT-' if a.agent=='codex' else 'shift+tab to cycle';shutil.copy2(sys.executable,agent);agent.chmod(0o700)
footer={'codex':"b'\\x1b[?2004hGPT-6.1-Sol\\r\\n> Ask Codex\\r\\n'",'claude':"'\\x1b[?2004h\\u276f \\r\\n  \\u23f5\\u23f5 auto mode on (shift+tab to cycle)\\r\\n'.encode()"}[a.agent]
receiver=root/'receiver.py';receiver.write_text("""FOOTER="""+footer+"""\nimport os,sys,tty\nfrom pathlib import Path\nassert not any(os.environ.get(k)for k in ['DISPLAY','WAYLAND_DISPLAY','XAUTHORITY'])\ntty.setraw(0);out=Path(sys.argv[1]);out.touch(mode=0o600)\nos.write(1,FOOTER)\nwith out.open('ab',buffering=0)as f:\n while True:\n  b=os.read(0,8192)\n  if not b:break\n  f.write(b)\n""")
processes=[];logs=[];sequence={};engines=[]
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';seat=None
def control(method,args=None):return bus.call_sync(service,seat,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
def key(code,down):control('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,down)))
def start(args,name,environment):
 log=(root/(name+'.log')).open('wb');logs.append(log);q=subprocess.Popen(args,env=environment,stdout=log,stderr=log);processes.append(q);return q
def rpc(sock,method,params=None):
 with Client(sock)as c:return c.call(method,params)
def command(who,action,timeout=8,**kw):
 n=sequence.get(who,0)+1;sequence[who]=n;temp=root/(who+'-request.tmp');temp.write_text(json.dumps({'seq':n,'action':action,**kw}));temp.replace(root/(who+'-request.json'))
 def response():
  try:x=json.loads((root/(who+'-response.json')).read_text());return x['result']if x['seq']==n else None
  except(OSError,ValueError):return None
 return eventually(response,timeout)
def focus(who):
 for _ in range(8):
  active=command('gui','inspect')['active']if who=='gui'else command('peer','input-probe')['active']
  if active:
   if who=='peer':
    before=command('peer','input-probe')['events'];key(194,True);key(194,False);eventually(lambda:command('peer','input-probe')['events']>before)
   else:key(42,True);key(42,False)
   time.sleep(.1)
   if who=='gui':command('gui','focus-view',view=0)
   return
  key(56,True);key(15,True);key(15,False);key(56,False);time.sleep(.2)
 raise AssertionError('Isolated compositor focus unavailable')
try:
 seat=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];control('Start');control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 inner=root/'inner.sock';outer=root/'outer.sock'
 for name,sock in [('inner',inner),('outer',outer)]:
  server=start([a.engine,'--socket',str(sock),'serve'],name,headless);engines.append((sock,server));eventually(sock.exists)
 receipt=root/'agent.bin';session=rpc(inner,'session.create',{'name':'Generated displayless '+a.agent,'argv':[str(agent),str(receiver),str(receipt)]});rpc(inner,'pane.wait',{'pane':session['pane'],'contains':READY,'timeout_ms':5000});agent_pid=rpc(inner,'job.query',{'pane':session['pane']})['foreground']['pid'];agent_start=Path(f'/proc/{agent_pid}/stat').read_text().rsplit(') ',1)[1].split()[19]
 nested=rpc(outer,'session.create',{'name':'Local nested TUI','argv':[a.binary,'--socket',str(inner),'attach','--session',session['session']]});rpc(outer,'pane.wait',{'pane':nested['pane'],'contains':READY,'timeout_ms':5000})
 window=start([a.gui,'--socket',str(outer),'--session',nested['session'],'--fixture-dir',str(root),'--staging-dir',str(root/'stage')],'gui',env)
 peer_script=Path(__file__).resolve().parents[1]/'fixtures/desktop_clipboard.py';peer=start(['python3',str(peer_script),str(root)],'peer',env);eventually(lambda:(root/'peer-ready.json').exists());eventually(lambda:command('gui','inspect')['drawn'][0]>0,20)
 image=root/'generated.png';Image.new('RGB',(16,12),'#5a1636').save(image);image.chmod(0o600);digest=hashlib.sha256(image.read_bytes()).hexdigest()
 def diagnostic(label):
  (root/(label+'.json')).write_text(json.dumps({'agent_hex':receipt.read_bytes().hex(),'gui':command('gui','inspect'),'peer':command('peer','inspect'),'outer_job':rpc(outer,'job.query',{'pane':nested['pane']}),'outer_frame':rpc(outer,'pane.capture',{'pane':nested['pane'],'lines':40})},indent=2)+'\n')
 diagnostic('before-focus')
 focus('peer');command('peer','set-png',file='generated.png');focus('gui');diagnostic('before-paste');command('gui','key',key='v',mods=4);time.sleep(.3);diagnostic('after-paste')
 if a.baseline:
  eventually(lambda:receipt.read_bytes()==b'\x16',5)
  assert not list((root/'stage').rglob('*.png'))or not b'\x1b[200~'in receipt.read_bytes()
  checks=[{'id':'published-GUI-negative-control-forwards-native-key-no-image','PASS':True}]
 else:
  def attached():
   data=receipt.read_bytes()
   if not data.startswith(b'\x1b[200~')or not data.endswith(b'\x1b[201~'):return False
   assert data.count(b'\x1b[200~')==data.count(b'\x1b[201~')==1
   path=Path(data[6:-6].decode());assert path.is_file()and hashlib.sha256(path.read_bytes()).hexdigest()==digest;assert path.stat().st_mode&0o777==0o600
   return str(path)
  staged=eventually(attached,10)
  assert b'\r'not in receipt.read_bytes()and b'\n'not in receipt.read_bytes()
  assert rpc(inner,'job.query',{'pane':session['pane']})['foreground']['pid']==agent_pid and Path(f'/proc/{agent_pid}/stat').read_text().rsplit(') ',1)[1].split()[19]==agent_start
  assert not any(row.startswith((b'DISPLAY=',b'WAYLAND_DISPLAY=',b'XAUTHORITY='))for row in Path(f'/proc/{agent_pid}/environ').read_bytes().split(b'\0'))
  checks=[{'id':'native-GDK-PNG-to-local-nested-displayless-agent','PASS':True,'staged':staged,'sha256':digest},{'id':'single-literal-bracketed-paste-without-Enter','PASS':True},{'id':'existing-public59-engine-agent-PID-start-preserved','PASS':True}]
 result={'PASS':True,'checks':checks,'expected_baseline_failure':a.baseline,'synthetic_GTK_key_callback':True,'actual_GDK_clipboard_and_real_PTY':True,'physical_or_provider_acceptance_claimed':False,'CLI_sha256':hashlib.sha256(Path(a.binary).read_bytes()).hexdigest(),'GUI_sha256':hashlib.sha256(Path(a.gui).read_bytes()).hexdigest(),'engine_sha256':hashlib.sha256(Path(a.engine).read_bytes()).hexdigest()};(root/'acceptance.json').write_text(json.dumps(result,indent=2)+'\n');print('LOCAL_IMAGE_NATIVE PASS',root,flush=True)
finally:
 if seat:
  try:control('Stop')
  except Exception:pass
 for q in reversed(processes):
  if q.poll()is None:
   if any(q is x for _,x in engines):
    sock=next(sock for sock,x in engines if x is q)
    try:rpc(sock,'daemon.stop')
    except Exception:q.terminate()
   else:q.terminate()
   try:q.wait(timeout=8)
   except subprocess.TimeoutExpired:q.kill();q.wait(timeout=3)
 for log in logs:log.close()
 destination=a.evidence/root.name;destination.mkdir(mode=0o700)
 for item in root.rglob('*'):
  target=destination/item.relative_to(root)
  if item.is_dir():target.mkdir(mode=0o700,exist_ok=True)
  elif item.is_file()and not item.is_symlink():target.parent.mkdir(mode=0o700,parents=True,exist_ok=True);shutil.copy2(item,target)
 assert root.parent==Path('/run/user')/str(os.getuid())and root.name.startswith('lni-')and not root.is_symlink();shutil.rmtree(root)
