#!/usr/bin/env python3
"""Actual TLS/CredSSP and native certificate dialogs on an owned display/keyring."""
import argparse,hashlib,json,os,subprocess,tempfile,time
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--gui',required=True);p.add_argument('--server',required=True);p.add_argument('--evidence',type=Path,required=True);p.add_argument('--auto-prompt',action='store_true');a=p.parse_args()
if os.environ.get('GDK_BACKEND')!='wayland' or not Path(os.environ.get('XDG_RUNTIME_DIR','/')).resolve().is_relative_to(a.evidence.resolve()):
 raise SystemExit('Certificate acceptance requires an owned Wayland runtime inside evidence')
root=Path(tempfile.mkdtemp(prefix='rdp-trust-',dir=a.evidence));root.chmod(0o700);env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','GTK_USE_PORTAL':'0'}
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:env[key]=str(root/name);Path(env[key]).mkdir(mode=0o700)
processes=[];logs=[];results=[];seq=0;private_bus=None;gui=None;input_session=None;sock=root/'control.sock';store=root/'config/RUWAC/machines.json'
def start(args,name):
 log=(root/(name+'.log')).open('wb');logs.append(log);proc=subprocess.Popen(args,env=env,stdout=log,stderr=log);processes.append(proc);return proc
def command(action,**kwargs):
 global seq
 seq+=1;temp=root/'request.tmp';temp.write_text(json.dumps({'seq':seq,'action':action,**kwargs}));temp.replace(root/'gui-request.json')
 def read():
  try:d=json.loads((root/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except (OSError,ValueError):return None
 return eventually(read,10)
def api(verb,*args):
 d=json.loads(subprocess.check_output([a.binary,'--socket',str(sock),'rdp',verb,*args,'--json'],env=env,text=True,timeout=10));assert 'result'in d,d;return d['result']
def create_profile(fields):
 command('profile-editor',operation='new',response='keep',fields=fields)
 eventually(lambda:any(d['fields'] and d['width']>0 for d in command('profile-editor',operation='layout')['dialogs']),10)
 return command('profile-editor',operation='inspect')
def index(profile):
 views=api('list')['views'];return next(i for i,v in enumerate(views)if v['profile']==profile)
def pending(profile):
 i=index(profile)
 return eventually(lambda:r if (r:=command('rdp-trust',rdp=i)).get('pending')else None,20)
def frames(profile):
 return next(v for v in api('list')['views']if v['profile']==profile)
def connected(profile):return eventually(lambda:v if (v:=frames(profile))['phase']=='connected'and v['decoded_updates']>0 else None,20)
def metadata():return json.loads(store.read_text())
def write_metadata(d):
 temp=store.with_suffix('.tmp');temp.write_text(json.dumps(d));temp.chmod(0o600);temp.replace(store)
def restart():
 global gui
 command('workspace-close');gui.wait(timeout=10)
 for name in ['gui-request.json','gui-response.json']:(root/name).unlink(missing_ok=True)
 gui=start([a.gui,'--socket',str(sock),'--fixture-dir',str(root),'--allow-rdp-control'],'gui-restart')
 eventually(lambda:command('inspect'),15)
 api('reconnect','--id',frames(profile)['id'])
def record(name,**details):results.append({'id':name,'status':'PASS',**details});print(name,'PASS',flush=True)
try:
 lines=subprocess.check_output(['dbus-daemon','--session','--fork','--print-address=1','--print-pid=1'],env=env,text=True).splitlines();private_bus=int(lines[1]);env['DBUS_SESSION_BUS_ADDRESS']=lines[0]
 control=root/'keyring';control.mkdir(mode=0o700);env['GNOME_KEYRING_CONTROL']=str(control);log=(root/'keyring.log').open('wb');logs.append(log)
 provider=subprocess.Popen(['gnome-keyring-daemon','--foreground','--components=secrets','--control-directory='+str(control),'--unlock'],env=env,stdin=subprocess.PIPE,stdout=log,stderr=log);processes.append(provider);provider.stdin.write(b'generated-disposable-keyring-only\n');provider.stdin.close();time.sleep(1)
 subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-subj','/CN=RUWAC-local-fixture','-addext','subjectAltName=IP:127.0.0.1','-keyout',str(root/'key.pem'),'-out',str(root/'cert.pem')],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);(root/'key.pem').chmod(0o600)
 server=start([a.server,str(root)],'server');eventually(lambda:(root/'server-ready.json').exists(),10);ready=json.loads((root/'server-ready.json').read_text());port=ready['ports'][0]
 der=subprocess.check_output(['openssl','x509','-in',str(root/'cert.pem'),'-outform','DER']);digest=hashlib.sha256(der).hexdigest();token=f'pin-sha256:{digest}@{port}:127.0.0.1'
 start([a.binary,'--socket',str(sock),'serve'],'daemon');eventually(lambda:sock.exists())
 with Client(sock)as c:session=c.call('session.create',{'name':'Certificate acceptance','argv':['/bin/sh','-c','sleep 300']})
 gui=start([a.gui,'--socket',str(sock),'--session',session['session'],'--fixture-dir',str(root),'--allow-rdp-control'],'gui');eventually(lambda:command('inspect'),15)
 reference='generated-certificate-'+root.name
 subprocess.run(['secret-tool','store','--label=RUWAC disposable certificate test','application','RUWAC','profile',reference],env=env,input=b'generated-local-only',stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True,timeout=10)
 created=create_profile({'0':'Untrusted fixture','1':'Generated trust tests','3':'127.0.0.1:'+str(port),'4':'fixture','9':reference})
 assert not created['error'],created;profile=created['items'][0]['id'];api('open','--profile',profile)
 r=pending(profile);assert r['certificate']['token']==token and r['certificate']['valid_now'],r;assert frames(profile)['decoded_updates']==0
 if a.auto_prompt:
  # A background tab must wait for deliberate foreground selection.
  assert not r['dialog_open'] and frames(profile)['decoded_updates']==0,r
  from gi.repository import Gio,GLib
  input_bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);input_dest='org.gnome.Mutter.RemoteDesktop'
  input_session=input_bus.call_sync(input_dest,'/org/gnome/Mutter/RemoteDesktop',input_dest,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
  def input_call(method,args=None):return input_bus.call_sync(input_dest,input_session,input_dest+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
  input_call('Start')
  def input_key(code,state):input_call('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,state)));time.sleep(.04)
  command('rdp-switch',rdp=index(profile))
  for _ in range(10):
   if command('rdp-trust',rdp=index(profile))['dialog_open']:break
   input_key(56,True);input_key(15,True);input_key(15,False);input_key(56,False);time.sleep(.15)
  else:raise AssertionError('Selected foreground connection did not prompt for certificate review')
  assert frames(profile)['decoded_updates']==0
  record('TRUST-FOREGROUND-AUTO-PROMPT-BACKGROUND-DEFERRED',no_automatic_approval=True)
 baseline=store.read_bytes();command('rdp-trust',rdp=index(profile),operation='review');command('rdp-trust',rdp=index(profile),response='cancel');assert store.read_bytes()==baseline and frames(profile)['decoded_updates']==0
 record('TRUST-DEFAULT-REJECT-CANCEL',real_TLS_inspection=True,zero_authenticated_frames_before_approval=True)
 # Approval must survive an options-only refresh of this exact saved endpoint.
 # Port1 refuses the unrelated generated SSH route; no external account is used.
 subprocess.run([a.gui,'--receive-primary',profile,'fixture','wayland-77','1'],env=env,check=True,stdout=subprocess.DEVNULL)
 subprocess.run([a.gui,'--paste-primary',profile],env=env,check=True,stdout=subprocess.DEVNULL)
 command('rdp-trust',rdp=index(profile),operation='review');command('rdp-trust',rdp=index(profile),response='once');v=connected(profile)
 assert command('rdp-inspect',rdp=index(profile))['selection_settings']=={'enabled':True,'paste':True}
 assert metadata()['items'][0]['preferences']['trust']=='';assert command('rdp-trust',rdp=index(profile))['tab_pin'];record('TRUST-ONCE-AUTHENTICATED',decoded_updates=v['decoded_updates'],profile_trust_unchanged=True)
 restart();r=pending(profile);assert not r['saved_pin']and not r['tab_pin'];assert frames(profile)['decoded_updates']==0
 checkpoints=list((root/'config/RUWAC').glob('workspace*.json'));assert checkpoints and all(token not in path.read_text()for path in checkpoints)
 record('TRUST-ONCE-NOT-CHECKPOINTED',restart_requires_approval=True)
 # Simulate another owner window changing unrelated metadata before Save trust.
 d=metadata();d['items'][0]['tags']=['keep-concurrent-metadata'];write_metadata(d)
 command('rdp-trust',rdp=index(profile),operation='review');command('rdp-trust',rdp=index(profile),response='save');connected(profile)
 assert metadata()['items'][0]['preferences']['trust']==token and metadata()['items'][0]['tags']==['keep-concurrent-metadata']
 record('TRUST-SAVE-FRESH-METADATA',exact_endpoint_pin_saved=True,unrelated_change_preserved=True)
 restart();v=connected(profile);assert not command('rdp-trust',rdp=index(profile))['review_available'];record('TRUST-SAVED-RESTART',authenticated_without_new_prompt=True,decoded_updates=v['decoded_updates'])
 # A changed saved fingerprint never silently trusts the presented certificate.
 d=metadata();wrong=f'pin-sha256:{"00"*32}@{port}:127.0.0.1';d['items'][0]['preferences']['trust']=wrong;write_metadata(d);restart();r=pending(profile);assert r['certificate']['changed']and frames(profile)['decoded_updates']==0
 command('rdp-trust',rdp=index(profile),operation='review');command('rdp-trust',rdp=index(profile),response='save');assert metadata()['items'][0]['preferences']['trust']==wrong and frames(profile)['decoded_updates']==0
 record('TRUST-CHANGED-REQUIRES-CONFIRMATION',zero_authenticated_frames=True)
 command('rdp-trust',rdp=index(profile),confirmation='TRUST',response='save');connected(profile);assert metadata()['items'][0]['preferences']['trust']==token
 record('TRUST-CHANGED-EXPLICIT-REPLACEMENT',confirmed_exact_fingerprint=True)
 # An independently changed profile cannot receive a stale dialog's saved trust.
 created=create_profile({'0':'Stale approval fixture','1':'Generated trust tests','3':'127.0.0.1:'+str(ready['ports'][1]),'4':'fixture','9':reference})
 assert not created['error'],created;other=next(p['id']for p in created['items']if p['name']=='Stale approval fixture')
 api('open','--profile',other);pending(other);command('rdp-trust',rdp=index(other),operation='review')
 d=metadata();row=next(p for p in d['items']if p['id']==other);row['server']='127.0.0.1:'+str(ready['ports'][2]);write_metadata(d);before=store.read_bytes()
 command('rdp-trust',rdp=index(other),response='save');assert store.read_bytes()==before and frames(other)['decoded_updates']==0
 command('rdp-trust',rdp=index(other),response='cancel')
 record('TRUST-STALE-ENDPOINT-REFUSED',changed_profile_preserved=True,no_authenticated_frame=True)

finally:
 if input_session is not None:
  try:input_call('Stop')
  except Exception:pass
 for proc in reversed(processes):
  if proc.poll()is None:
   proc.terminate()
   try:proc.wait(timeout=5)
   except subprocess.TimeoutExpired:proc.kill();proc.wait()
 if private_bus is not None:os.kill(private_bus,15)
 for log in logs:log.close()
 report={'cases':results,'sha256':{name:hashlib.sha256(Path(path).read_bytes()).hexdigest()for name,path in [('ruwac',a.binary),('ruwac-desktop',a.gui)]},'scope':'Actual TLS/CredSSP/framebuffer and native dialog callbacks on owned Wayland/private keyring; generated credentials and server; no external or physical-input claim','evidence':str(root)}
 (root/'receipt.json').write_text(json.dumps(report,indent=2)+'\n');(a.evidence/'rdp-trust-acceptance.json').write_text(json.dumps(report,indent=2)+'\n');print('Evidence',root,flush=True)
assert len(results)==(9 if a.auto_prompt else 8)
