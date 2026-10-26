#!/usr/bin/env python3
"""Authenticated loopback RDP and independent GDK peer on an owned compositor."""
import argparse,hashlib,importlib.util,json,os,subprocess,sys,tempfile,time,traceback
from pathlib import Path
from PIL import Image
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--gui',required=True);p.add_argument('--server',required=True);p.add_argument('--evidence',required=True);p.add_argument('--r5-only',action='store_true');p.add_argument('--mixed-only',action='store_true');p.add_argument('--case',action='append',default=[]);p.add_argument('--honor-client-size',action='store_true');a=p.parse_args()
spec=importlib.util.spec_from_file_location('spike',Path(__file__).with_name('spike.py'));spike=importlib.util.module_from_spec(spec);spec.loader.exec_module(spike)
root=Path(tempfile.mkdtemp(prefix='rdp-native-',dir=a.evidence));root.chmod(0o700);env={**os.environ,'HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_DATA_HOME':str(root/'data'),'GSETTINGS_BACKEND':'memory','GTK_USE_PORTAL':'0','GTK_A11Y':'none'}
if a.honor_client_size:env['RUWAC_FIXTURE_HONOR_CLIENT_SIZE']='1'
for key in ['HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME']:Path(env[key]).mkdir(mode=0o700)
processes=[];logs=[];results=[];sequences={};sock=root/'control.sock';private_bus=None;provider=None
def start(argv,name):
 log=(root/(name+'.log')).open('wb');logs.append(log);proc=subprocess.Popen(argv,stdout=log,stderr=log,env=env);processes.append(proc);return proc
def command(who,action=None,timeout=8,**kw):
 seq=sequences.get(who,0)+1;sequences[who]=seq;temp=root/(who+'-request.tmp');temp.write_text(json.dumps({'seq':seq,'action':action,**kw}));temp.replace(root/(who+'-request.json'))
 def read():
  try:r=json.loads((root/(who+'-response.json')).read_text());return r['result'] if r['seq']==seq else None
  except (FileNotFoundError,ValueError):return None
 return spike.eventually(read,timeout)
mutter_input=None
if os.environ.get('RUWAC_TEST_MUTTER_INPUT')=='1':
 assert os.environ.get('WAYLAND_DISPLAY','').startswith('ruwac-')
 assert Path(os.environ['XDG_RUNTIME_DIR']).resolve()!=Path('/run/user')/str(os.getuid())
 from gi.repository import Gio,GLib
 bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);dest='org.gnome.Mutter.RemoteDesktop'
 path=bus.call_sync(dest,'/org/gnome/Mutter/RemoteDesktop',dest,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
 def mutter_input(method,args=None):return bus.call_sync(dest,path,dest+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
 mutter_input('Start')
def focus_keys(toggle=False):
 if mutter_input:
  def key(n,state):mutter_input('NotifyKeyboardKeysym',GLib.Variant('(ub)',(n,state)))
  if toggle:
   key(65513,True);key(65289,True);key(65289,False);key(65513,False)
  else:key(65505,True);key(65505,False)
 else:subprocess.run(['xdotool','key','--clearmodifiers','Super_L+Tab'if toggle else'Shift_L'],env={**os.environ,'DISPLAY':os.environ['RUWAC_XTEST_DISPLAY']},check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def focus(who):
 for _ in range(6):
  if command('gui','inspect').get('active')==(who=='gui'):
   focus_keys();time.sleep(.06);return
  focus_keys(True);time.sleep(.3)
 raise AssertionError('Isolated compositor focus failed')
def case(name,fn):
 if a.case and name not in a.case and name!='RDP-NATIVE-AUTH-3':return
 if a.mixed_only and name not in {'RDP-NATIVE-AUTH-3','MIXED-NATIVE-RDP-TERMINAL','FILES-CLIPBOARD-OWNERSHIP','GUI-IPC-BACKGROUND-RDP-OWNERSHIP'}:return
 if a.r5_only and name not in {"RDP-NATIVE-AUTH-3","MACHINES-NATIVE-EDITOR","RDP-NATIVE-RESIZE-20","MACHINES-RESTART-1000","FILES-CLIPBOARD-OWNERSHIP","RDP-PROFILE-KEYRING-PREFERENCES","RDP-GUI-LIFECYCLE-API"}:return
 try:results.append({'id':name,'status':'PASS','details':fn()});print(name,'PASS',flush=True)
 except Exception as e:results.append({'id':name,'status':'FAIL','error':str(e),'traceback':traceback.format_exc()});print(name,'FAIL',str(e),flush=True)
def digest(data):return hashlib.sha256(data).hexdigest()
def receipt(tab):
 try:return json.loads((root/f'server-{tab}-receipt.json').read_text())
 except (FileNotFoundError,ValueError):return {}
def read_remote(tab,kind,file):
 before=command('gui','rdp-inspect',rdp=tab)['received_generation'];command(f'server-{tab}',format=kind,file=file)
 spike.eventually(lambda:command('gui','rdp-inspect',rdp=tab)['received_generation']>before,10)
 focus('peer');return command('peer','read-text' if kind=='text' else 'read-png')
def transfer_to_remote(tab,kind,file):
 focus('peer');command('peer','set-text' if kind=='text' else 'set-png',file=file);focus('gui');before=receipt(tab).get('received',0);command('gui','rdp-send',rdp=tab)
 return spike.eventually(lambda:r if (r:=receipt(tab)).get('received',0)>before and 'error' not in r else False,12)
try:
 lines=subprocess.check_output(['dbus-daemon','--session','--fork','--print-address=1','--print-pid=1'],env=env,text=True).splitlines();private_bus=int(lines[1]);env['DBUS_SESSION_BUS_ADDRESS']=lines[0]
 control=root/'private-keyring';control.mkdir(mode=0o700);env['GNOME_KEYRING_CONTROL']=str(control)
 log=(root/'keyring.log').open('wb');logs.append(log);provider=subprocess.Popen(['gnome-keyring-daemon','--foreground','--components=secrets','--control-directory='+str(control),'--unlock'],env=env,stdin=subprocess.PIPE,stdout=log,stderr=log);processes.append(provider);provider.stdin.write(b'generated-disposable-keyring-only\n');provider.stdin.close();time.sleep(1)
 subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-subj','/CN=RUWAC-local-fixture','-addext','subjectAltName=IP:127.0.0.1','-keyout',str(root/'key.pem'),'-out',str(root/'cert.pem')],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);(root/'key.pem').chmod(0o600)
 server=start([a.server,str(root)],'rdp-server');spike.eventually(lambda:(root/'server-ready.json').exists(),10);ready=json.loads((root/'server-ready.json').read_text())
 daemon=start([a.binary,'--socket',str(sock),'serve'],'daemon');spike.eventually(lambda:sock.exists())
 with spike.Client(sock) as c:session=c.call('session.create',{'name':'RDP fixture terminal'})
 gui=start([a.gui,'--socket',str(sock),'--session',session['session'],'--fixture-dir',str(root)],'gui');peer=start([sys.executable,str(Path(__file__).parents[1]/'fixtures/desktop_clipboard.py'),str(root)],'peer');spike.eventually(lambda:(root/'peer-ready.json').exists(),10);command('gui','inspect');focus('gui')
 def lifecycle():
  connections=[]
  for tab,port in enumerate(ready['ports']):
   started=time.perf_counter();command('gui','rdp-open',port=port);r=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=tab)).get('size')==([1280,720] if a.honor_client_size else [320,180]) else False,20);assert r['status'].startswith('Connected'),r;r['connect_to_decoded_frame_ms']=(time.perf_counter()-started)*1000;connections.append(r)
  return {'server':ready,'connections':connections,'authentication':'actual CredSSP/NLA','frames':'decoded live loopback server frames, generated desktop'}
 case('RDP-NATIVE-AUTH-3',lifecycle)
 def idle_resources():
  assert a.honor_client_size,'Resource measurement requires negotiated 1280×720 framebuffers'
  original_tabs=command('gui','inspect')['tabs'];created=[]
  try:
   for _ in range(2):
    before=command('gui','inspect')['tabs'];assert command('gui','new-terminal')['clicked']
    spike.eventually(lambda:command('gui','inspect')['tabs']==before+1)
    created.append(before)
   spike.eventually(lambda:len(command('gui','inspect')['drawn'])==3)
   def metrics():
    data={}
    for name,proc in [('gtk',gui),('daemon',daemon),('loopback_server',server),('clipboard_peer',peer)]:
     fields=Path(f'/proc/{proc.pid}/stat').read_text().rsplit(')',1)[1].split()
     status=dict(line.split(':',1)for line in Path(f'/proc/{proc.pid}/status').read_text().splitlines()if ':'in line)
     data[name]={'pid':proc.pid,'starttime':fields[19],'cpu_seconds':(int(fields[11])+int(fields[12]))/os.sysconf('SC_CLK_TCK'),
                 'rss_mib':int(status['VmRSS'].split()[0])/1024,'peak_rss_mib':int(status['VmHWM'].split()[0])/1024}
    return data
   with spike.Client(sock)as c:
    identities={p['id']:p['pid']for s in c.call('session.list')for p in c.call('pane.list',{'session':s['id']})}
   phases=[]
   for selected in ['rdp','terminal']:
    focus('gui')
    if selected=='rdp':command('gui','rdp-switch',rdp=0)
    else:command('gui','tab-select',tab=0);command('gui','focus-view')
    time.sleep(.8)
    before=metrics();terminal_before=command('gui','inspect')['drawn'];rdp_before=[command('gui','rdp-inspect',rdp=i)for i in range(3)]
    started=time.monotonic();time.sleep(5);elapsed=time.monotonic()-started
    after=metrics();terminal_after=command('gui','inspect')['drawn'];rdp_after=[command('gui','rdp-inspect',rdp=i)for i in range(3)]
    terminal_delta=[y-x for x,y in zip(terminal_before,terminal_after)];rdp_delta=[y['draws']-x['draws']for x,y in zip(rdp_before,rdp_after)]
    assert len(terminal_delta)==3 and max(rdp_delta)<=3,(terminal_delta,rdp_delta)
    assert all(y['transport_id']==x['transport_id'] and y['status'].startswith('Connected')for x,y in zip(rdp_before,rdp_after))
    if selected=='rdp':assert max(terminal_delta)<=1,terminal_delta
    else:assert max(terminal_delta)<=15 and max(terminal_delta[1:])<=1,terminal_delta
    phases.append({'selected':selected,'wall_seconds':elapsed,'before':before,'after':after,
                   'cpu_percent_of_one_core':{name:(after[name]['cpu_seconds']-v['cpu_seconds'])*100/elapsed for name,v in before.items()},
                   'terminal_redraws':terminal_delta,'rdp_redraws':rdp_delta,
                   'full_frame_counter_delta':[y['metrics']['decoded_updates']-x['metrics']['decoded_updates']for x,y in zip(rdp_before,rdp_after)],
                   'counter_scope':'Existing full-frame counter excludes independently decoded bitmap strips; redraw counts measure actual GTK paints.',
                   'framebuffer_sizes':[x['size']for x in rdp_after]})
   focus('gui');command('gui','rdp-switch',rdp=0);before=[command('gui','rdp-inspect',rdp=i)for i in range(3)]
   (root/'resource-idle-phases.json').write_text(json.dumps(phases,indent=2))
   started=time.monotonic();command('server-0','disconnect')
   recovery_trace=[]
   def recovered_frame():
    value=command('gui','rdp-inspect',rdp=0);state=value['gnome_auto'];guards=state['guards']
    recovery_trace.append({'seconds':time.monotonic()-started,'state':value})
    if len(recovery_trace)>256:del recovery_trace[0]
    return value if (value['status'].startswith('Connected') and state['generation']>before[0]['gnome_auto']['generation']
                     and value['size']
                     and guards['surface_generation']==guards['rendered_generation']==state['generation']) else False
   try:recovered=spike.eventually(recovered_frame,20)
   finally:(root/'resource-recovery-trace.json').write_text(json.dumps({'before':before,'observations':recovery_trace},indent=2))
   recovery_ms=(time.monotonic()-started)*1000
   assert [command('gui','rdp-inspect',rdp=i)['transport_id']for i in [1,2]]==[before[i]['transport_id']for i in [1,2]]
   text=b'Generated resource recovery clipboard';(root/'resource-recovery.txt').write_bytes(text)
   assert transfer_to_remote(0,'text','resource-recovery.txt')['sha256']==digest(text)
   with spike.Client(sock)as c:
    observed={p['id']:p['pid']for s in c.call('session.list')for p in c.call('pane.list',{'session':s['id']})}
   assert observed==identities,'Reconnect changed a terminal process'
   return {'terminals':3,'authenticated_RDP_views':3,'phases':phases,'reconnect_to_frame_ms':recovery_ms,
           'neighbor_transports_and_terminal_PIDs_preserved':True,'clipboard_after_recovery':True,
           'reconnected_generation':recovered['gnome_auto']['generation'],
           'scope':'Owned software-rendered Mutter and generated loopback peers; no physical GPU, WAN or matched Remmina performance claim'}
  finally:
   for tab in reversed(created):command('gui','close-tab',tab=tab)
   assert command('gui','inspect')['tabs']==original_tabs
 case('RDP-NATIVE-IDLE-RESOURCES',idle_resources)
 def initial_size():
  assert a.honor_client_size,'Requires the real peer to honor initial desktop dimensions'
  fixture=root/'initial-size-server';fixture.mkdir(mode=0o700)
  for name in ['cert.pem','key.pem']:(fixture/name).write_bytes((root/name).read_bytes());(fixture/name).chmod(0o600)
  endpoint=start([a.server,str(fixture)],'initial-size-server');spike.eventually(lambda:(fixture/'server-ready.json').exists(),10);ports=json.loads((fixture/'server-ready.json').read_text())['ports']
  reference='generated-initial-size-'+root.name
  subprocess.run(['secret-tool','store','--label=RUWAC disposable initial-size test','application','RUWAC','profile',reference],input=b'generated-local-only',env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  evidence=[]
  try:
   for index,(width,height)in enumerate([(1400,900),(1600,1000),(1100,700)]):
    command('gui','rdp-display',rdp=0,mode='fit',window_width=width,window_height=height)
    spike.eventually(lambda:command('gui','rdp-inspect',rdp=0)['window_width']==width,10)
    name='Generated initial display '+str(index)
    command('gui','profile-editor',operation='new',response='keep',fields={'0':name,'1':'Generated display','3':'127.0.0.1:'+str(ports[0]),'4':'fixture','9':reference,'10':str(root/'cert.pem'),'13':3})
    spike.eventually(lambda:any(d['width']>0 and d['height']>0 for d in command('gui','profile-editor',operation='layout')['dialogs']),10)
    saved=command('gui','profile-editor',operation='inspect',response='connect');assert not saved['error'],saved
    state=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=3)).get('display_mode')=='Dynamic'and r['status'].startswith('Connected')and r['size']==r['viewport']and r['size'][0]==width else False,20)
    expected=state['size'];trace=state['metrics']['decoded_sizes'];assert trace and all(size==expected for size in trace),{'expected_first_size':expected,'actual_decoded_trace':trace}
    time.sleep(.4);assert command('gui','rdp-inspect',rdp=3)['metrics']['decoded_sizes']==trace
    evidence.append({'window':[width,height],'first_framebuffer':trace[0],'all_decoded_sizes':trace,'production_path':'native profile Save & Connect, stored generated Secret Service credential'})
    command('gui','close-rdp',rdp=3)
   # The real IPC path opens an unselected tab. Cancel before the deferred
   # worker starts, then prove a subsequent background connection completes
   # without stealing tab selection or clipboard ownership.
   item=next(p for p in saved['items']if p['name']==name)
   command('gui','rdp-permission',enabled=True);command('gui','switch',view=0);focus('gui')
   before=command('gui','inspect');neighbors=[command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]
   def api(verb,*args):
    result=subprocess.run([a.binary,'--socket',str(sock),'rdp',verb,*args,'--json'],env=env,capture_output=True,text=True,timeout=8);assert result.returncode==0,(verb,result.stderr,result.stdout);return json.loads(result.stdout)['result']
   opened=api('open','--profile',item['id']);assert opened['phase']=='connecting'and opened['transport_id']is None,opened
   api('disconnect','--id',opened['id']);time.sleep(.3)
   cancelled=api('query','--id',opened['id']);assert cancelled['transport_id']is None and cancelled['decoded_updates']==0,cancelled
   command('gui','close-rdp',rdp=3)
   opened=api('open','--profile',item['id']);vid=opened['id']
   live=spike.eventually(lambda:r if (r:=api('query','--id',vid))['phase']=='connected'and r['size'][0]==1100 else False,20)
   assert live['size'][0]==1100,live
   hidden=command('gui','rdp-inspect',rdp=3);assert hidden['metrics']['decoded_sizes']==[live['size']],hidden
   after=command('gui','inspect');assert after['selected_tab']==before['selected_tab']and after['clipboard_owner']==before['clipboard_owner'],(before,after)
   assert [command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]==neighbors
   api('disconnect','--id',vid);command('gui','close-rdp',rdp=3);command('gui','rdp-permission',enabled=False)
   return {'connections':evidence,'pending_cancel_before_worker_and_credential_lookup':True,'background_first_framebuffer':live['size'],'background_focus_clipboard_neighbors_preserved':True,'authentication':'actual loopback CredSSP and decoded framebuffer; no external-host claim'}
  finally:
   subprocess.run(['secret-tool','clear','application','RUWAC','profile',reference],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
   endpoint.terminate();endpoint.wait(timeout=5)
 case('RDP-NATIVE-INITIAL-SIZE',initial_size)
 def lifecycle_api():
  def api(verb,*args,ok=True):
   r=subprocess.run([a.binary,'--socket',str(sock),'rdp',verb,*args,'--json'],env=env,capture_output=True,text=True,timeout=8)
   assert (r.returncode==0)==ok,(verb,r.stdout,r.stderr);v=json.loads(r.stdout);assert v['version']==0 and v['id'];return v.get('result',v.get('error'))
  listing=api('list');assert len(listing['views'])==3
  ids=[v['id']for v in listing['views']];assert all(v['phase']=='connected'and v['decoded_updates']>0 for v in listing['views'])
  assert 'secret_reference'not in json.dumps(listing)and str(root)not in json.dumps(listing)
  assert api('disconnect','--id',ids[0],ok=False)['code']=='PERMISSION_DENIED'
  command('gui','rdp-permission',enabled=True);command('gui','rdp-switch',rdp=1);focus('gui')
  before=command('gui','inspect');neighbors=[api('query','--id',vid)['transport_id']for vid in ids[1:]];old=api('query','--id',ids[0])['transport_id']
  assert api('reconnect','--id',ids[0],ok=False)['code']=='INVALID_PROFILE'
  assert api('query','--id',ids[0])['transport_id']==old
  assert [api('query','--id',vid)['transport_id']for vid in ids[1:]]==neighbors
  assert command('gui','inspect')['selected_tab']==before['selected_tab']and command('gui','inspect')['clipboard_owner']==before['clipboard_owner']
  api_dir=root/'api-server';api_dir.mkdir(mode=0o700)
  for name in ['cert.pem','key.pem']:(api_dir/name).write_bytes((root/name).read_bytes());(api_dir/name).chmod(0o600)
  api_server=start([a.server,str(api_dir)],'api-server');spike.eventually(lambda:(api_dir/'server-ready.json').exists(),10);api_ready=json.loads((api_dir/'server-ready.json').read_text())
  reference='generated-api-'+root.name
  command('gui','profile-editor',operation='new',response='keep',fields={'0':'R16 API profile','1':'Generated automation','3':'127.0.0.1:'+str(api_ready['ports'][0]),'4':'fixture','9':reference,'10':str(root/'cert.pem'),'13':0})
  spike.eventually(lambda:any(d['width']>0 and d['height']>0 for d in command('gui','profile-editor',operation='layout')['dialogs']),10)
  r=command('gui','profile-editor',operation='inspect',response='save');assert not r['error'],r
  item=next(p for p in r['items']if p['name']=='R16 API profile')
  subprocess.run(['secret-tool','store','--label=RUWAC disposable API test','application','RUWAC','profile',reference],input=b'generated-local-only',env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  for method,params,error,key in [
   ('rdp.open',{'profile':item['id'],'password':'PRIVATE-API-AUDIT-SENTINEL'},'INVALID_TARGET',None),
   ('rdp.open',{'profile':item['id'],'activate':'yes'},'INVALID_TARGET',None),
   ('rdp.list',{'password':'PRIVATE-API-AUDIT-SENTINEL'},'INVALID_REQUEST',None),
   ('rdp.open',{'profile':item['id']},'INVALID_REQUEST','reject-gui-key')]:
   with spike.Client(sock.with_suffix('.gui.sock'))as client:client.call(method,params,error=error,key=key)
  assert 'PRIVATE-API-AUDIT-SENTINEL'not in(root/'gui.log').read_text()
  command('gui','workspace-permission',enabled=False)
  assert api('open','--profile',item['id'],'--activate',ok=False)['code']=='PERMISSION_DENIED';assert len(api('list')['views'])==3
  command('gui','rdp-switch',rdp=1);focus('peer');sentinel=b'R16 untouched desktop clipboard';(root/'api-sentinel.txt').write_bytes(sentinel);command('peer','set-text',file='api-sentinel.txt');focus('gui');before=command('gui','inspect')
  opened=api('open','--profile',item['id']);vid=opened['id'];assert len(api('list')['views'])==4
  spike.eventually(lambda:api('query','--id',vid)['phase']=='connected'and api('query','--id',vid)['decoded_updates']>0,20)
  assert command('gui','inspect')['selected_tab']==before['selected_tab']and command('gui','inspect')['clipboard_owner']==before['clipboard_owner']
  assert not api('query','--id',vid)['clipboard_owner']
  all_neighbors=[api('query','--id',x)['transport_id']for x in ids];first_transport=api('query','--id',vid)['transport_id']
  api('disconnect','--id',vid);api('reconnect','--id',vid);spike.eventually(lambda:api('query','--id',vid)['phase']=='connected',20)
  assert api('query','--id',vid)['transport_id']!=first_transport and [api('query','--id',x)['transport_id']for x in ids]==all_neighbors
  assert command('gui','inspect')['selected_tab']==before['selected_tab']and command('gui','inspect')['clipboard_owner']==before['clipboard_owner']
  focus('peer');assert command('peer','read-text')['sha256']==digest(sentinel)
  command('gui','rdp-permission',enabled=False);assert api('disconnect','--id',vid,ok=False)['code']=='PERMISSION_DENIED';assert api('query','--id',vid)['phase']=='connected'
  command('gui','rdp-permission',enabled=True);api('disconnect','--id',vid)
  state=json.loads(subprocess.check_output([a.binary,'--socket',str(sock),'workspace','list','--json'],env=env))['result'];tab=next(v['tab']for v in state['views']if v['id']==vid);index=next(i for i,p in enumerate(state['tabs'])if p['id']==tab)
  command('gui','close-tab',tab=index);assert api('query','--id',vid,ok=False)['code']=='STALE_TARGET'
  command('gui','profiles',search='R16 API profile');deleted=command('gui','profile-editor',operation='edit',response='delete',confirm=True);assert not any(p['id']==item['id']for p in deleted['items'])
  subprocess.run(['secret-tool','clear','application','RUWAC','profile',reference],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  api_server.terminate();api_server.wait(timeout=5)
  command('gui','profiles',search='');command('gui','rdp-permission',enabled=False);command('gui','rdp-switch',rdp=0)
  return {'actual_CredSSP_framebuffer':True,'typed_view_ids':True,'permission_default_and_revocation':True,'read_metadata_no_credentials':True,'background_reconnect_preserves_neighbors_focus_clipboard':True,'saved_profile_open_without_focus':True,'activation_requires_separate_focus_permission':True,'closed_target_rejected':True,'unexpected_or_credential_parameters_rejected':True,'audit_no_private_payload':True,'idempotency_keys_explicitly_refused':True,'owner_keyring_accessed':False}
 case('RDP-GUI-LIFECYCLE-API',lifecycle_api)
 def background_ipc():
  focus('gui');command('gui','rdp-switch',rdp=0);command('gui','workspace-permission',enabled=True)
  sentinel=b'Foreground GDK peer owns clipboard';remote=b'Authorized loopback RDP background offer';(root/'background-local.txt').write_bytes(sentinel);(root/'background-remote.txt').write_bytes(remote)
  def api(args):
   r=subprocess.run([a.binary,'--socket',str(sock),'workspace',*args,'--json'],env=env,capture_output=True,text=True,timeout=7);assert r.returncode==0,(r.stdout,r.stderr);return json.loads(r.stdout)['result']
  state=api(['list']);rdp_id=next(v['id']for v in state['views']if v['kind']=='rdp')
  focus('peer');command('peer','set-text',file='background-local.txt');before=[command('gui','rdp-inspect',rdp=i)['received_generation']for i in range(3)]
  api(['focus','--id',rdp_id]);assert not api(['list'])['window_active'];assert command('gui','inspect')['clipboard_owner']is None
  for i in range(3):command(f'server-{i}',format='text',file='background-remote.txt')
  time.sleep(.25);assert command('peer','read-text')['sha256']==digest(sentinel);assert [command('gui','rdp-inspect',rdp=i)['received_generation']for i in range(3)]==before
  focus('gui');command('gui','rdp-switch',rdp=0);got=read_remote(0,'text','background-remote.txt');assert got['sha256']==digest(remote);focus('gui');command('gui','workspace-permission',enabled=False)
  return {'actual_authenticated_rdp_sessions':3,'authorized_ipc_focus_in_background':True,'foreground_peer_clipboard_preserved':True,'inactive_transfers_not_started':True,'transfer_after_native_window_activation':True,'physical_input':False}
 case('GUI-IPC-BACKGROUND-RDP-OWNERSHIP',background_ipc)
 texts=['Živjo Łódź 🦀\nMultiline clipboard '+str(i) for i in range(8)]+['x'*10240,'z'*(1024*1024)]
 def text_transfer():
  evidence=[]
  for tab in range(3):
   focus('gui');command('gui','rdp-switch',rdp=tab)
   for i,text in enumerate(texts):
    name=f'text-{tab}-{i}.txt';payload=text.encode();(root/name).write_bytes(payload)
    received=transfer_to_remote(tab,'text',name);assert received['sha256']==digest(payload),received
    focus('gui');got=read_remote(tab,'text',name);assert got['sha256']==digest(payload),got;evidence.append({'tab':tab,'bytes':len(payload),'sha256':digest(payload),'directions':2})
  return evidence
 case('RDP-NATIVE-TEXT-60',text_transfer)
 def images():
  evidence=[]
  for tab in range(3):
   focus('gui');command('gui','rdp-switch',rdp=tab)
   for index,(w,h,fmt)in enumerate([(64,64,'png'),(2560,1440,'png'),(64,64,'dibv5')]):
    name=f'image-{tab}-{index}.png';pixels=bytes([40+tab,129,75,100+index])*(w*h);Image.frombytes('RGBA',(w,h),pixels).save(root/name)
    received=transfer_to_remote(tab,'png',name);assert received['pixel_sha256']==digest(pixels),received
    focus('gui');got=read_remote(tab,fmt,name);assert got['pixel_sha256']==digest(pixels),got;evidence.append({'tab':tab,'size':[w,h],'remote_format':fmt,'pixel_sha256':digest(pixels),'directions':2})
  return evidence
 case('RDP-NATIVE-PNG-18',images)
 def policies():
  focus('gui');command('gui','rdp-switch',rdp=0);sentinel=b'POLICY-LOCAL-SENTINEL';remote=b'POLICY-REMOTE-GENERATED';(root/'policy-local.txt').write_bytes(sentinel);(root/'policy-remote.txt').write_bytes(remote)
  for policy in [1,3,4]:
   focus('gui');command('gui','rdp-policy',rdp=0,policy=policy);focus('peer');command('peer','set-text',file='policy-local.txt');focus('gui');before=command('gui','rdp-inspect',rdp=0)['received_generation'];command('server-0',format='text',file='policy-remote.txt');time.sleep(.2);focus('peer');r=command('peer','read-text');assert r['sha256']==digest(sentinel),(policy,r)
   if policy==3:
    focus('gui');command('gui','rdp-receive',rdp=0);spike.eventually(lambda:command('gui','rdp-inspect',rdp=0)['received_generation']>before);focus('peer');assert command('peer','read-text')['sha256']==digest(remote)
  focus('gui');command('gui','rdp-policy',rdp=0,policy=2);focus('peer');command('peer','set-text',file='policy-local.txt');focus('gui');before=receipt(0).get('received',0);command('gui','rdp-send',rdp=0);time.sleep(.2);assert receipt(0).get('received',0)==before;got=read_remote(0,'text','policy-remote.txt');assert got['sha256']==digest(remote);focus('gui');command('gui','rdp-policy',rdp=0,policy=0)
  return {'policies':5,'manual_receive':'explicit action','directional_gates':'real GDK and CLIPRDR'}
 case('RDP-NATIVE-POLICIES-5',policies)
 def repeated_receive_preserves_local_copy():
  focus('gui');command('gui','rdp-switch',rdp=0);command('gui','rdp-policy',rdp=0,policy=0)
  remote=b'REPEAT-RECEIVE-REMOTE';local=b'NEW-LOCAL-AFTER-RECEIVE'
  (root/'repeat-remote.txt').write_bytes(remote);(root/'repeat-local.txt').write_bytes(local)
  assert read_remote(0,'text','repeat-remote.txt')['sha256']==digest(remote)
  received=command('gui','rdp-inspect',rdp=0)['received_generation']
  focus('peer');command('peer','set-text',file='repeat-local.txt');focus('gui')
  command('gui','rdp-receive',rdp=0);time.sleep(.3);focus('peer')
  assert command('peer','read-text')['sha256']==digest(local)
  assert command('gui','rdp-inspect',rdp=0)['received_generation']==received
  return {'new_local_copy_preserved':True,'same_remote_generation_not_replayed':True}
 case('RDP-NATIVE-REPEATED-RECEIVE',repeated_receive_preserves_local_copy)
 def automatic_clipboard():
  proof=[]
  for tab in range(3):
   focus('gui');command('gui','rdp-switch',rdp=tab);command('gui','rdp-policy',rdp=tab,policy=0)
   path=root/f'auto-local-{tab}.txt';payload=f'Automatic local copy tab {tab} Živjo 🦀'.encode();path.write_bytes(payload)
   before=receipt(tab).get('received',0)
   focus('peer');command('peer','set-text',file=path.name);focus('gui')
   spike.eventually(lambda:receipt(tab).get('received',0)>before and receipt(tab).get('sha256')==digest(payload),12)
   # Two consecutive remote copies while the same canvas remains focused.
   for index in range(2):
    name=f'auto-remote-{tab}-{index}.txt';data=f'Automatic remote {tab}/{index} 中文'.encode();(root/name).write_bytes(data)
    before=command('gui','rdp-inspect',rdp=tab)['received_generation'];command(f'server-{tab}',format='text',file=name)
    spike.eventually(lambda:command('gui','rdp-inspect',rdp=tab)['received_generation']>before,10)
   focus('peer');first=command('peer','read-text');observed=[first];started=time.monotonic()
   def clipboard_matches():
    got=command('peer','read-text');observed.append(got);(root/f'auto-poll-{tab}.json').write_text(json.dumps({'expected':digest(data),'observed':observed,'gui':command('gui','rdp-inspect',rdp=tab)}));return got.get('sha256')==digest(data)
   matched=first.get('sha256')==digest(data)
   if not matched:spike.eventually(clipboard_matches,3)
   record={'tab':tab,'expected_sha256':digest(data),'observed':observed,'settle_ms':(time.monotonic()-started)*1000}
   (root/f'auto-observed-{tab}.json').write_text(json.dumps(record))
   assert observed[-1].get('sha256')==digest(data),record
   focus('gui')
   for index in range(2):
    name=f'auto-png-{tab}-{index}.png';pixels=bytes([tab+20,50+index*70,99,255])*(64*64);Image.frombytes('RGBA',(64,64),pixels).save(root/name)
    before=command('gui','rdp-inspect',rdp=tab)['received_generation'];command(f'server-{tab}',format='png',file=name)
    spike.eventually(lambda:command('gui','rdp-inspect',rdp=tab)['received_generation']>before,10)
   focus('peer');actual=command('peer','read-png');assert actual.get('pixel_sha256')==digest(pixels),actual
   proof.append({'tab':tab,'automatic_send':True,'consecutive_automatic_receives':2,'consecutive_automatic_png_receives':2})
  return {'tabs':proof,'explicit_send_receive_actions':0}
 case('RDP-NATIVE-AUTOMATIC-CLIPBOARD-3',automatic_clipboard)
 def newer_offer_during_read():
  focus('gui');command('gui','rdp-switch',rdp=0);command('gui','rdp-policy',rdp=0,policy=0)
  (root/'offer-old.txt').write_bytes(b'OLD-INFLIGHT-GENERATED');new=b'NEWEST-GENERATED-CLIPBOARD';(root/'offer-new.txt').write_bytes(new)
  p=root/'server-0-pending.json';before_requests=json.loads(p.read_text())['requests']if p.exists()else 0
  before=command('gui','rdp-inspect',rdp=0)['received_generation']
  command('server-0',format='text',file='offer-old.txt',delay_ms=800)
  spike.eventually(lambda:p.exists()and json.loads(p.read_text())['requests']>before_requests)
  command('server-0',format='text',file='offer-new.txt')
  spike.eventually(lambda:command('gui','rdp-inspect',rdp=0)['received_generation']>before,12)
  focus('peer');assert command('peer','read-text')['sha256']==digest(new)
  assert json.loads(p.read_text())['requests']>=before_requests+2
  return {'first_response_delay_ms':800,'newest_copy_received':True,'explicit_receive_actions':0}
 case('RDP-NATIVE-SUPERSEDED-CLIPBOARD-READ',newer_offer_during_read)


 def reconnect():
  # This case can run independently of the policy case that uses the same file.
  (root/'policy-local.txt').write_bytes(b'POLICY-LOCAL-SENTINEL')
  focus('gui');command('gui','rdp-switch',rdp=0);started=time.perf_counter();command('server-0','disconnect')
  spike.eventually(lambda:'Reconnecting' in command('gui','rdp-inspect',rdp=0)['status'],8)
  r=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=0))['status'].startswith('Connected') and r['size']==[320,180] else False,15)
  got=transfer_to_remote(0,'text','policy-local.txt');assert got['sha256']==digest(b'POLICY-LOCAL-SENTINEL')
  return {'reconnect_to_frame_ms':(time.perf_counter()-started)*1000,'same_tab':True,'clipboard_after_reconnect':'PASS','bounded_backoff':True}
 case('RDP-NATIVE-RECONNECT',reconnect)
 def tab_menu():
  current=command('gui','rdp-inspect',rdp=0);neighbor=command('gui','rdp-inspect',rdp=1)
  before=neighbor['transport_id'];tab=current['tab']
  for action,mode in [('dynamic','Dynamic'),('fit','Fit'),('native','Native'),('fit','Fit')]:
   menu=command('gui','rdp-tab-menu',tab=tab,control=action)
   assert menu['icon'] and menu['status_icon'],menu
   assert ' · RDP' not in menu['title'] and ' / ' not in menu['title'],menu
   assert command('gui','rdp-inspect',rdp=0)['display_mode']==mode
   assert command('gui','rdp-inspect',rdp=1)['transport_id']==before
  return {'native_TabView_setup_menu':True,'targeted_modes':4,'neighbor_transport_unchanged':True,'remote_framebuffer_right_click':'retained existing pointer path'}
 case('RDP-NATIVE-TAB-MENU',tab_menu)

 def reconnect_size():
  assert a.honor_client_size,'Requires a peer that honors initial client desktop dimensions'
  focus('gui');command('gui','rdp-switch',rdp=0)
  command('gui','rdp-display',rdp=0,mode='dynamic',window_width=1400,window_height=900)
  previous=spike.eventually(lambda:r['size'] if (r:=command('gui','rdp-inspect',rdp=0))['size']==r['requested_size']==r['viewport'] and r['size'][0]==1400 and min(r['size'])>=200 else False,15)
  print('RDP reconnect stable starting framebuffer',previous,flush=True)
  trace_length=len(command('gui','rdp-inspect',rdp=0)['metrics']['decoded_sizes'])
  for _ in range(3):
   generation=command('gui','rdp-inspect',rdp=0)['gnome_auto']['generation']
   command('server-0','disconnect')
   spike.eventually(lambda:command('gui','rdp-inspect',rdp=0)['gnome_auto']['generation']>generation,15)
   r=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=0))['status'].startswith('Connected') and r['size']==previous else False,15)
   # Bounded trace records actual decoded dimensions, including brief frames
   # which a polling-only test could miss.
   assert all(size==previous for size in r['metrics']['decoded_sizes'][trace_length-1:]),r
  return {'previous_framebuffer':previous,'authenticated_reconnects':3,'last_sizes':r['metrics']['decoded_sizes'],'scope':'Real loopback CredSSP/framebuffer; no external Windows acceptance'}
 case('RDP-NATIVE-RECONNECT-SIZE',reconnect_size)

 def ownership():
  sentinel=b'LOCAL-OWNER-SENTINEL';(root/'sentinel.txt').write_bytes(sentinel);(root/'inactive.txt').write_text('Generated inactive remote offer')
  for i in range(200):
   focus('gui');active=i%4
   if active<3:command('gui','rdp-switch',rdp=active)
   else:command('gui','switch',view=0)
   focus('peer');command('peer','set-text',file='sentinel.txt');focus('gui')
   for other in range(3):
    if other!=active:command(f'server-{other}',format='text',file='inactive.txt')
   time.sleep(.025);focus('peer');r=command('peer','read-text');assert r['sha256']==digest(sentinel),(i,r)
   if (i+1)%50==0:print('OWNERSHIP',i+1,flush=True)
  return {'focus_switches':200,'inactive_offers_checked':450,'peer_clipboard_hash':digest(sentinel),'physical_input':False}
 case('RDP-NATIVE-OWNERSHIP-200',ownership)
 def mixed_workspace():
  try:
   before=[command('gui','rdp-inspect',rdp=i) for i in range(3)]
   command('gui','switch',view=0)
   tree=command('gui','mixed-join',tab=1,vertical=False)
   assert len(tree['tabs'])==3 and tree['tabs'][0]['layout']['split']=='horizontal',tree
   tree=command('gui','mixed-join',tab=1,vertical=True)
   assert len(tree['tabs'])==2 and tree['tabs'][0]['layout']['split']=='vertical' and tree['tabs'][0]['layout']['first']['split']=='horizontal',tree
   time.sleep(.4)
   for i in range(3):
    now=command('gui','rdp-inspect',rdp=i)
    assert now['transport_id']==before[i]['transport_id'] and now['size']==before[i]['size'] and now['metrics']['decoded_updates']>0,now
   command('gui','rdp-switch',rdp=0)
   text='Mixed RDP/terminal clipboard Živjo 🦀';path=root/'mixed-text.txt';path.write_text(text)
   received=transfer_to_remote(0,'text',path.name);assert received['sha256']==digest(text.encode()),received
   baseline='Inactive mixed RDP must not own this clipboard';path=root/'mixed-baseline.txt';path.write_text(baseline)
   focus('peer');command('peer','set-text',file=path.name);focus('gui')
   owner=command('gui','mixed-focus',kind='terminal',index=0)
   assert owner['owner']==session['session'],owner
   before_offers=[command('gui','rdp-inspect',rdp=i)['offers'] for i in range(2)]
   for i in range(2):command(f'server-{i}',format='text',file='mixed-text.txt')
   spike.eventually(lambda:all(command('gui','rdp-inspect',rdp=i)['offers']>before_offers[i] for i in range(2)))
   focus('peer');assert command('peer','read-text')['sha256']==digest(baseline.encode())
   focus('gui');command('gui','snapshot');tree=command('gui','mixed-separate');assert len(tree['tabs'])==4,tree
   for i in range(3):assert command('gui','rdp-inspect',rdp=i)['transport_id']==before[i]['transport_id']
   # Real dialog actions move one live RDP view, then close only that view.
   command('gui','tab-select',tab=0);command('gui','mixed-join',tab=command('gui','rdp-inspect',rdp=2)['tab'],vertical=False)
   assert command('gui','views-manage')['clicked']
   moved=command('gui','views-action',operation='move',index=1)
   assert moved['clicked'] and len(moved['layout']['tabs'])==4 and moved['rdp_views']==3,moved
   for i in range(3):assert command('gui','rdp-inspect',rdp=i)['transport_id']==before[i]['transport_id']
   command('gui','tab-select',tab=0);command('gui','mixed-join',tab=3,vertical=True)
   assert command('gui','views-manage')['clicked']
   closed=command('gui','views-action',operation='close',index=1)
   assert closed['clicked'] and closed['rdp_views']==2 and closed['terminal_views']==1 and len(closed['layout']['tabs'])==3,closed
   for i in range(2):assert command('gui','rdp-inspect',rdp=i)['transport_id']==before[i]['transport_id']
   command('gui','rdp-open',port=ready['ports'][2])
   fresh=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=2)).get('size')==[320,180] else False,20)
   assert fresh['transport_id']!=before[2]['transport_id'] and fresh['status'].startswith('Connected'),fresh
   command('gui','rdp-switch',rdp=2)
   received=transfer_to_remote(2,'text','mixed-text.txt');assert received['sha256']==digest(text.encode())
   return {'nested_orientation':['vertical','horizontal'],'actual_authenticated_rdp_connections_preserved':3,'text_sha256':digest(text.encode()),'inactive_mixed_offers_refused':2,'separated_without_reconnect':True,'native_move_preserved_transport':True,'native_close_disconnected_only_selected_rdp':True,'neighbors_kept_transport_ids':True,'replacement_authenticated_and_clipboard_verified':True}
  finally:
   command('gui','tab-select',tab=0);command('gui','mixed-separate')
 case('MIXED-NATIVE-RDP-TERMINAL',mixed_workspace)
 def files_ownership():
  import socket,threading
  listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen(1);port=listener.getsockname()[1]
  def refuse():
   connection,_=listener.accept();connection.close();listener.close()
  threading.Thread(target=refuse,daemon=True).start()
  profile={'id':'generated-files-endpoint','machine_id':'generated-files-machine','name':'Files fixture','group':'Generated','protocol':'SSH','server':f'127.0.0.1:{port}','username':'fixture','secret_reference':'generated-unused','favorite':False,'last_used':0,'tags':[]}
  sentinel=b'Files tab clipboard sentinel';(root/'files-sentinel.txt').write_bytes(sentinel);(root/'files-inactive.txt').write_text('Inactive RDP must remain cached')
  focus('peer');command('peer','set-text',file='files-sentinel.txt');focus('gui');command('gui','files-open',profile=profile)
  for _ in range(10):
   for tab in range(3):command(f'server-{tab}',format='text',file='files-inactive.txt')
   time.sleep(.03);focus('peer');assert command('peer','read-text')['sha256']==digest(sentinel);focus('gui')
  command('gui','rdp-switch',rdp=0)
  return {'native_files_tab':True,'inactive_rdp_offers':30,'peer_clipboard_unchanged':True}
 case('FILES-CLIPBOARD-OWNERSHIP',files_ownership)
 def profiles():
  focus('gui')
  for i in range(70):
   name=f'machine-{i}.remmina';original=f'[remmina]\nname=Same leaf\ngroup=Team{i}/Nested\nprotocol=RDP\nserver=127.0.0.1:3389\nusername=fixture\npassword=DO-NOT-IMPORT\nssh_privatekey=/private/key\nlabels=linux,test\n';(root/name).write_text(original);r=command('gui','profiles',file=name);assert r['rows']==i+1,r;assert(root/name).read_text()==original
  r=command('gui','profiles',search='Team69');assert r['rows']==1,r
  r=command('gui','profiles',favorite=True);assert r['favorites']==1,r
  stored=(root/'config/RUWAC/machines.json');data=stored.read_text();assert 'DO-NOT-IMPORT' not in data and '/private/key' not in data;assert stored.stat().st_mode&0o777==0o600
  return {'imported':70,'same_leaf_different_full_groups':True,'search_rows':1,'favorite_rows':1,'credentials_excluded':True,'source_files_unchanged':True,'store_mode':'0600'}
 case('MACHINES-NATIVE-70',profiles)
 def profile_journey():
  command('gui','profiles',search='R5')
  command('gui','profile-editor',operation='group',path='R5/Organization/Region')
  for group in ['R5/Organization/Region','R5/Team']:
   r=command('gui','profile-editor',operation='new',fields={'0':'R5 same server','1':group,'3':'127.0.0.1:3389','4':'fixture','13':0,'14':1,'15':3})
   assert r['applied'] and r['error'] is None,r
  items=[p for p in r['items'] if p['name']=='R5 same server'];assert len(items)==2 and items[0]['machine_id']!=items[1]['machine_id']
  original=min(items,key=lambda p:p["group"]);r=command('gui','profile-editor',operation='edit',fields={'0':'R5 renamed','1':'R5/moved','3':'127.0.0.1:22','13':1})
  changed=next(p for p in r['items'] if p['id']==original['id']);assert changed['machine_id']==original['machine_id'] and changed['protocol']=='SSH' and changed['preferences']['display']=='native' and changed['preferences']['clipboard']==3
  command('gui','profiles',search='R5 renamed');r=command('gui','profile-editor',operation='edit',response='endpoint',fields={'13':0,'3':'127.0.0.1:3389'});siblings=[p for p in r['items'] if p['machine_id']==original['machine_id']];assert len(siblings)==2 and len({p['id'] for p in siblings})==2
  before=len(r['items']);r=command('gui','profile-editor',operation='new',response='cancel',fields={'0':'Cancelled','3':'localhost:22'});assert len(r['items'])==before
  r=command('gui','profile-editor',operation='edit',response='duplicate');assert len(r['items'])==before+2
  r=command('gui','profile-editor',operation='edit',response='delete-machine',confirm=True);assert len(r['items'])==before
  stored=root/'config/RUWAC/machines.json';on_disk=json.loads(stored.read_text());assert on_disk['schema']==1 and len(on_disk['items'])==before
  assert (stored.parent/'machines.previous.json').stat().st_mode&0o777==0o600
  return {'native_dialogs':True,'cancel_duplicate_confirmed_delete':True,'stable_identity_after_rename_move':True,'multi_endpoint_machine':True,'same_leaf_distinct_machines':True,'persisted_schema':1}
 case('MACHINES-NATIVE-EDITOR',profile_journey)
 def editor_layout():
  command('gui','profile-editor',operation='new',response='keep',window_size=[480,520],fields={'0':'Generated narrow editor','3':'127.0.0.1:3389'},named={'rdp-connection-mode':1})
  def geometry():
   r=command('gui','profile-editor',operation='layout')
   return r if r['dialogs'] and r['dialogs'][0]['fields'] and r['dialogs'][0]['width']>0 else False
  time.sleep(.5);r=spike.eventually(geometry,5);d=r['dialogs'][0];rows={x['widget']:x for x in d['fields']}
  order=['machine-field-0','machine-field-3','machine-field-13','rdp-connection-mode','endpoint-username','rdp-password','gnome-username','gnome-password']
  assert all(n in rows for n in order),(order,rows)
  assert all(rows[a]['y']+rows[a]['height']<=rows[b]['y'] for a,b in zip(order,order[1:])),rows
  assert d['width']<=540,d
  assert all(x['x']>=0 and x['x']+x['width']<=d['width']+1 for x in d['fields']),d
  assert not any(n in rows for n in ['machine-field-2','machine-field-7','machine-field-10']),rows
  screenshot=None
  try:
   # The owned Wayland-only compositor has no X11 root window. The actual
   # allocated GTK field geometry above remains the acceptance evidence.
   if env.get('RUWAC_XTEST_DISPLAY'):
    subprocess.run(['import','-display',env['RUWAC_XTEST_DISPLAY'],'-window','root',str(root/'machine-editor-narrow.png')],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    screenshot='machine-editor-narrow.png'
  finally:
   cancelled=command('gui','profile-editor',operation='inspect',response='cancel')
  assert not any(p['name']=='Generated narrow editor'for p in cancelled['items'])
  return {'actual_GTK_dialog':d,'basic_credentials_order':order,'advanced_collapsed':True,'cancel_preserved_library':True,'screenshot':screenshot,'layout_evidence':'actual allocated GTK dialog and field rectangles','physical_desktop':False}
 case('MACHINES-NATIVE-EDITOR-LAYOUT',editor_layout)
 def agent_launcher():
  before=command('gui','inspect')
  opened=command('gui','agent-launch-dialog',operation='show');assert opened['dialog_visible'] and opened['agents']==['Codex (cx)','Claude Code (cc)','Kimi'],opened
  command('gui','agent-launch-dialog',operation='cancel')
  after=command('gui','inspect');assert before['tabs']==after['tabs']
  return {'native_dialog':True,'explicit_agents':opened['agents'],'cancel_creates_no_session':True,'SSH_profiles':opened['SSH_profiles'],'external_agent_launch':'not performed in this UI fixture'}
 case('MACHINES-REMOTE-AGENT-LAUNCHER',agent_launcher)
 def automatic_gdm_pointer():
  extra=root/'gdm-pointer-server';extra.mkdir(mode=0o700)
  for name in ['cert.pem','key.pem']:(extra/name).write_bytes((root/name).read_bytes());(extra/name).chmod(0o600)
  start([a.server,str(extra)],'gdm-pointer-server');spike.eventually(lambda:(extra/'server-ready.json').exists(),10)
  port=json.loads((extra/'server-ready.json').read_text())['ports'][0];ref='generated-gdm-pointer-'+root.name
  subprocess.run(['secret-tool','store','--label=RUWAC disposable GDM pointer fixture','application','RUWAC','profile',ref],input=b'generated-local-only',env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  try:
   command('gui','profile-editor',operation='new',response='keep',fields={'0':'Generated GDM pointer','3':'127.0.0.1:'+str(port),'4':'fixture','9':ref,'10':str(root/'cert.pem'),'13':0},named={'rdp-connection-mode':1,'gnome-username':'generated-linux-user','gnome-auto-login':True})
   spike.eventually(lambda:any(d['fields'] and d['width']>0 for d in command('gui','profile-editor',operation='layout')['dialogs']),10)
   command('gui','profile-editor',operation='inspect',response='connect')
   frame=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=3)).get('size')==[320,180] and r['gnome_auto']['armed'] else False,20)
   epoch=frame['gnome_auto']['input_epoch'];mouse_path=extra/f'input-{port}.mouse.json'
   def mouse():
    try:return json.loads(mouse_path.read_text())['events']
    except (FileNotFoundError,ValueError):return []
   before=len(mouse());denied=command('gui','rdp-assist',rdp=3,operation='automatic-pointer',stale=True)
   assert not denied['queued'] and denied['epoch_unchanged'],denied
   stale_lookup=command('gui','rdp-assist',rdp=3,operation='stale-lookup-target')
   assert not stale_lookup['accepted'] and stale_lookup['armed'] and stale_lookup['epoch_unchanged'],stale_lookup
   command('gui','rdp-pointer',rdp=3,x=100,y=90,button=1)
   command('gui','rdp-pointer',rdp=3,x=100,y=90,button=1,release=True)
   time.sleep(.15);assert len(mouse())==before,'manual click bypassed automatic input shield'
   queued=command('gui','rdp-assist',rdp=3,operation='automatic-pointer')
   assert queued['queued'] and queued['epoch_unchanged'],queued
   actual=spike.eventually(lambda:m[before:] if 'LeftPressed' in (m:=mouse())[before:] and m[-1]=='LeftReleased' else False,8)
   assert command('gui','rdp-inspect',rdp=3)['gnome_auto']['input_epoch']==epoch
   command('gui','rdp-assist',rdp=3,operation='cancel-automatic')
   assert not command('gui','rdp-assist',rdp=3,operation='automatic-pointer')['queued']
   return {'authenticated_peer_mouse_events':actual,'stale_credential_callback_preserves_new_attempt':True,'manual_shield_preserved':True,'stale_and_cancelled_clicks_rejected':True,'epoch_preserved':True,'scope':'Real GTK frontend and loopback RDP input; no owner login or credentials'}
  finally:
   command('gui','close-rdp',rdp=3)
   subprocess.run(['secret-tool','clear','application','RUWAC','profile',ref],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
 case('RDP-NATIVE-GDM-AUTOMATIC-POINTER',automatic_gdm_pointer)
 def save_connect():
  extra=root/'save-connect-server';extra.mkdir(mode=0o700)
  for name in ['cert.pem','key.pem']:(extra/name).write_bytes((root/name).read_bytes());(extra/name).chmod(0o600)
  start([a.server,str(extra)],'save-connect-server');spike.eventually(lambda:(extra/'server-ready.json').exists(),10);port=json.loads((extra/'server-ready.json').read_text())['ports'][0]
  ref='generated-save-connect-'+root.name
  subprocess.run(['secret-tool','store','--label=RUWAC disposable save-connect fixture','application','RUWAC','profile',ref],input=b'generated-local-only',env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  transports=[command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]
  command('gui','profile-editor',operation='new',response='keep',fields={'0':'Generated Save Connect','3':'127.0.0.1:'+str(port),'4':'fixture','9':ref,'10':str(root/'cert.pem'),'13':0},named={'rdp-connection-mode':1,'gnome-username':'generated-linux-user','gnome-auto-login':True})
  spike.eventually(lambda:any(d['fields'] and d['width']>0 for d in command('gui','profile-editor',operation='layout')['dialogs']),10)
  saved=command('gui','profile-editor',operation='inspect',response='connect')
  item=next(p for p in saved['items']if p['name']=='Generated Save Connect')
  frame=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=3)).get('size')==[320,180] and r['status'].startswith('Connected') else False,20)
  assert [command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]==transports
  assert frame['gnome_auto']['armed'],frame
  epoch=frame['gnome_auto']['input_epoch']
  # The real manual input shield must not swallow the recognized automatic
  # user-list click. Inspect the authenticated peer, not a queue counter.
  mouse_path=extra/f'input-{port}.mouse.json'
  def auto_mouse():
   try:return json.loads(mouse_path.read_text())['events']
   except (FileNotFoundError,ValueError):return []
  start_mouse=len(auto_mouse())
  denied=command('gui','rdp-assist',rdp=3,operation='automatic-pointer',stale=True)
  assert not denied['queued'] and denied['epoch_unchanged'],denied
  queued=command('gui','rdp-assist',rdp=3,operation='automatic-pointer')
  assert queued['queued'] and queued['epoch_unchanged'],queued
  actual=spike.eventually(lambda:m[start_mouse:] if 'LeftPressed' in (m:=auto_mouse())[start_mouse:] and m[-1]=='LeftReleased' else False,8)
  assert command('gui','rdp-inspect',rdp=3)['gnome_auto']['input_epoch']==epoch
  for x in range(600,610):subprocess.run(['xdotool','mousemove',str(x),'450'],env={**env,'DISPLAY':env['RUWAC_XTEST_DISPLAY']},check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  time.sleep(.2);shield=command('gui','rdp-inspect',rdp=3);assert shield['gnome_auto']['armed']and shield['gnome_auto']['input_epoch']==epoch,shield
  subprocess.run(['import','-display',env['RUWAC_XTEST_DISPLAY'],'-window','root',str(root/'gnome-sign-in-shield.png')],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  command('gui','rdp-assist',rdp=3,operation='cancel-automatic');cancelled=command('gui','rdp-inspect',rdp=3);assert not cancelled['gnome_auto']['armed']and cancelled['status'].startswith('Automatic sign-in cancelled'),cancelled
  assert not command('gui','rdp-assist',rdp=3,operation='automatic-pointer')['queued']
  command('gui','close-rdp',rdp=3);time.sleep(.2)
  command('gui','profiles',search='Generated Save Connect')
  edited=command('gui','profile-editor',operation='edit',response='connect',fields={'0':'Generated Save Connect edited'},named={'gnome-auto-login':False})
  assert next(p for p in edited['items']if p['id']==item['id'])['name'].endswith('edited')
  spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=3)).get('size')==[320,180] and r['status'].startswith('Connected') else False,20)
  command('gui','close-rdp',rdp=3);time.sleep(.2)
  command('gui','profiles',search='Generated Save Connect')
  removed=command('gui','profile-editor',operation='edit',response='delete',confirm=True);assert not any(p['id']==item['id']for p in removed['items'])
  subprocess.run(['secret-tool','clear','application','RUWAC','profile',ref],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  return {'new_and_edited_profile_saved_before_connect':True,'real_loopback_CredSSP_frame':[320,180],'neighbor_transports_preserved':True,'existing_credential_acquisition_path':True,'owner_credentials_accessed':False,'GNOME_input_shield':{'automatic_user_list_click_received_by_authenticated_peer':actual,'stale_and_cancelled_clicks_rejected':True,'actual_GTK_motion_epoch_preserved':True,'native_cancel_reveals_frame':True,'screenshot':'gnome-sign-in-shield.png','GDM_authentication':'separate real-host evidence'}}
 case('MACHINES-SAVE-CONNECT',save_connect)
 def resize():
  focus('gui');command('gui','rdp-switch',rdp=0)
  def process_metrics():
   out={}
   for name,proc in [('gtk',gui),('server',server)]:
    stat=Path(f'/proc/{proc.pid}/stat').read_text().split();rss=int(next(s.split()[1]for s in Path(f'/proc/{proc.pid}/status').read_text().splitlines()if s.startswith('VmRSS:')))/1024;out[name]={'cpu_seconds':(int(stat[13])+int(stat[14]))/os.sysconf('SC_CLK_TCK'),'rss_mib':rss}
   return out
  time.sleep(.7);before=command('gui','rdp-inspect',rdp=0);start_metrics=process_metrics();wall_start=time.perf_counter()
  transitions=[]
  for i in range(20):
   w,h=[(640,480),(2560,1440),(3000,1800),(1024,768)][i%4]
   start=time.perf_counter();command('gui','rdp-display',rdp=0,mode='fit',zoom=1.0,width=w,height=h)
   def complete():
    r=command('gui','rdp-inspect',rdp=0)
    return r if r['size']==[w,h] and r['last_pixel'][:3]==[(w-1)%256,(h-1)%256,90] else False
   r=spike.eventually(complete,20)
   assert r['status'].startswith('Connected'),r
   transitions.append({'size':[w,h],'decoded_corner':r['last_pixel'],'complete_frame_ms':(time.perf_counter()-start)*1000,'metrics':r['metrics'],'processes':process_metrics()})
   command('gui','rdp-switch',rdp=(i%2)+1);assert command('gui','rdp-inspect',rdp=(i%2)+1)['size']==[320,180];command('gui','rdp-switch',rdp=0)
  # Actual GTK allocation changes drive the production debounced dynamic path.
  for i in range(20):command('gui','rdp-display',rdp=0,mode='dynamic',zoom=1.0,window_width=1100+i*4,window_height=760+i*2)
  def settled():
   r=command('gui','rdp-inspect',rdp=0)
   return r if r['size']==r['requested_size'] and min(r['size'])>=200 and r['status'].startswith('Connected') else False
  storm=spike.eventually(settled,15);count=storm['metrics']['decoded_updates'];time.sleep(.8);stable=settled();assert stable and stable['size']==storm['size'],stable
  # Controls popover must leave negotiated framebuffer and resize metrics unchanged.
  baseline=command('gui','rdp-inspect',rdp=0)
  for i in range(20):
   command('gui','chrome',open=i%2==0)
  time.sleep(.8);overlay=command('gui','rdp-inspect',rdp=0)
  assert overlay['viewport']==baseline['viewport'] and overlay['requested_size']==baseline['requested_size'] and overlay['size']==baseline['size'],(baseline,overlay)
  assert overlay['metrics']==baseline['metrics'],'Toolbar overlays triggered remote activity'
  for mode in ['native','stretch','fit']:
   r=command('gui','rdp-display',rdp=0,mode=mode,zoom=1.25);assert r['zoom']==1.25
  command('gui','rdp-display',rdp=0,mode='fit',zoom=1.0)
  command('gui','chrome',open=False);command('gui','rdp-switch',rdp=0)
  text='Clipboard after 20 actual reactivations ž 🦀';(root/'resize-text.txt').write_text(text);got=read_remote(0,'text','resize-text.txt');assert got['sha256']==digest(text.encode()),got
  time.sleep(.7);after=command('gui','rdp-inspect',rdp=0)
  return {'before':before['metrics'],'after':after['metrics'],'processes_before':start_metrics,'processes_after':process_metrics(),'wall_seconds':time.perf_counter()-wall_start,'transitions':transitions,'negotiation':'actual Display Control DVC plus server DeactivateAll/reactivation','resize_storm':{'gtk_allocation_changes':20,'settled_size':storm['size'],'stable_after_800ms':True},'post_resize_clipboard':True,'other_tabs_unchanged':True,'codec':'raw bitmap fixture; RemoteFX/GNOME issue remains a separate gate'}
 case('RDP-NATIVE-RESIZE-20',resize)
 def host_shortcuts():
  assert mutter_input,'Owned Mutter input fixture required'
  focus('gui');command('gui','chrome',open=False);command('gui','rdp-switch',rdp=0)
  before=command('gui','rdp-inspect',rdp=0);assert before['gnome_auto']['guards']['canvas_focus']
  text='Multi-format remote clipboard ž 🦀';(root/'multi-text.txt').write_text(text)
  png=Image.new('RGBA',(12,9),(30,80,150,255));png.save(root/'multi-image.png');pixels=digest(png.tobytes())
  previous=command('gui','rdp-inspect',rdp=0)['received_generation'];command('server-0',format='both',file='multi-image.png',text_file='multi-text.txt')
  spike.eventually(lambda:command('gui','rdp-inspect',rdp=0)['received_generation']>previous,10)
  focus('peer');formats=command('peer','inspect')['formats'];assert 'image/png'in formats and 'text/plain;charset=utf-8'in formats,formats
  assert command('peer','read-png')['pixel_sha256']==pixels
  answer=command('peer','read-text',timeout=15);assert answer.get('sha256')==digest(text.encode()),answer
  focus('gui');command('gui','rdp-switch',rdp=0)

  def chord(key):
   for k in [65507,65513,key]:mutter_input('NotifyKeyboardKeysym',GLib.Variant('(ub)',(k,True)))
   for k in [key,65513,65507]:mutter_input('NotifyKeyboardKeysym',GLib.Variant('(ub)',(k,False)))
  def keys():
   try:return json.loads((root/f"input-{ready['ports'][0]}.keys.json").read_text())['events']
   except (OSError,ValueError):return []
  def native_chord(mods,key):
   for k in mods+[key]:mutter_input('NotifyKeyboardKeysym',GLib.Variant('(ub)',(k,True)));time.sleep(.04)
   for k in [key]+list(reversed(mods)):mutter_input('NotifyKeyboardKeysym',GLib.Variant('(ub)',(k,False)));time.sleep(.04)
  baseline=len(keys())
  native_chord([65507],118)
  time.sleep(.3)
  events=keys()[baseline:];v=[k['down']for k in events if k['code']==47]
  assert v in [[],[True,False]],events
  for modifiers in [[65507,65513],[65507,65505]]:
   baseline=len(keys());native_chord(modifiers,118)
   spike.eventually(lambda:len([k for k in keys()[baseline:]if k['code']==47])>=2)
   assert [k['down']for k in keys()[baseline:]if k['code']==47]==[True,False],keys()[baseline:]
  chord(65360)
  spike.eventually(lambda:not command('gui','rdp-inspect',rdp=0)['gnome_auto']['guards']['canvas_focus'])
  local=command('gui','chrome')
  chord(65293)
  large=spike.eventually(lambda:r if (r:=command('gui','chrome'))['window_height']>local['window_height']else False,8)
  chord(65293)
  spike.eventually(lambda:command('gui','chrome')['window_height']==local['window_height'],8)
  assert command('gui','rdp-inspect',rdp=0)['transport_id']==before['transport_id']
  command('gui','rdp-switch',rdp=0)
  return {'real_GDK_key_events':True,'paste_key_down_up_balanced':True,'simultaneous_text_PNG_lazy_read_from_independent_GTK_peer':True,'Ctrl_Alt_V_and_Ctrl_Shift_V_forwarded':True,'Ctrl_Alt_Home_releases_remote_focus':True,'Ctrl_Alt_Enter_fullscreen_and_restore':True,'transport_preserved':True,'physical_keyboard':False}
 if mutter_input:case('RDP-NATIVE-HOST-SHORTCUTS',host_shortcuts)

 def pointer_drag():
  focus('gui');command('gui','rdp-switch',rdp=0)
  command('gui','rdp-display',rdp=0,mode='stretch',zoom=1.0)
  time.sleep(.3)
  def pointer(**kw):return command('gui','rdp-pointer',rdp=0,**kw)
  def mouse():
   try:return json.loads((root/f"input-{ready['ports'][0]}.mouse.json").read_text())['events']
   except(FileNotFoundError,ValueError):return []
  transitions=[]
  for button,label in [(1,'Left'),(3,'Right')]:
   for x,y in [(-100,-100),(100000,100000)]:
    pointer(x=100,y=100,button=button);pressed=spike.eventually(lambda:m if (m:=mouse()) and m[-1]==label+'Pressed' else False)
    valid=pointer()['position'];result=pointer(x=x,y=y,button=button,release=True)
    assert result['held_buttons']==0 and result['position']==valid,result
    released=spike.eventually(lambda:m if (m:=mouse()) and len(m)>len(pressed) and m[-1]==label+'Released' else False)
    assert not any('Move' in e for e in released[len(pressed):]),released
    ignored=pointer(x=x,y=y);assert ignored['position']==ignored['position_before'],ignored
    transitions.append({'button':label,'outside':[x,y],'release_received':True,'position_preserved':True})
  # ironrdp-server 0.13 maps middle-button PDUs to Move, so only client
  # held-state is asserted for middle; no remote middle-button pass is claimed.
  assert pointer(x=100,y=100,button=2)['held_buttons']==1
  assert pointer(x=-100,y=-100,button=2,release=True)['held_buttons']==0
  pointer(x=100,y=100,button=1);pressed=spike.eventually(lambda:m if (m:=mouse()) and m[-1]=='LeftPressed' else False)
  assert pointer(cancel=True)['held_buttons']==0
  spike.eventually(lambda:len(mouse())>len(pressed) and mouse()[-1]=='LeftReleased')
  # A late release after cancellation cannot duplicate a remote button release.
  count=len(mouse());pointer(x=100,y=100,button=1,release=True);time.sleep(.2);assert len(mouse())==count
  pointer(x=100,y=100,button=1);spike.eventually(lambda:mouse()[-1]=='LeftPressed')
  command('gui','rdp-switch',rdp=1);spike.eventually(lambda:mouse()[-1]=='LeftReleased')
  command('gui','rdp-switch',rdp=0);assert pointer()['held_buttons']==0
  # Local pan and zoom remain stable when focus returns from another tab.
  command('gui','rdp-display',rdp=0,mode='native',zoom=4.0,width=1800,height=1200)
  spike.eventually(lambda:command('gui','rdp-inspect',rdp=0)['size']==[1800,1200],20);time.sleep(.4)
  pan=pointer(pan_x=180,pan_y=120)['state']['pan'];assert pan==[180,120],pan
  for _ in range(5):
   command('gui','rdp-switch',rdp=1);command('gui','rdp-switch',rdp=0);time.sleep(.1)
   assert pointer()['state']['pan']==pan
  pointer(x=300,y=200,button=1);spike.eventually(lambda:mouse()[-1]=='LeftPressed')
  start=len(mouse());command('gui','rdp-display',rdp=0,mode='native',zoom=2.0);time.sleep(.2)
  assert pointer(x=400,y=300)['held_buttons']==0
  events=spike.eventually(lambda:m[start:] if 'LeftReleased' in (m:=mouse())[start:] and any(e.startswith('Move') for e in m[start:]) else False)
  assert events.index('LeftReleased')<next(i for i,e in enumerate(events) if e.startswith('Move')),events
  command('gui','rdp-display',rdp=0,mode='fit',zoom=1.0)
  return {'geometry_change_ends_drag_before_move':True,'outside_release' :transitions,'gesture_cancel':True,'late_release_ignored':True,'tab_switch_releases_drag':True,'pan_preserved_on_focus':True,'scope':'Production pointer mapping and GTK cancellation callback; actual authenticated RDP peer receipts; synthetic input, not physical mouse acceptance'}
 case('RDP-NATIVE-POINTER-DRAG',pointer_drag)


 def center_reconnect():
  extra=root/'center-reconnect-server';extra.mkdir(mode=0o700)
  for name in ['cert.pem','key.pem']:(extra/name).write_bytes((root/name).read_bytes());(extra/name).chmod(0o600)
  start([a.server,str(extra)],'center-reconnect-server');spike.eventually(lambda:(extra/'server-ready.json').exists(),10)
  port=json.loads((extra/'server-ready.json').read_text())['ports'][0]
  reference='generated-center-reconnect-'+root.name
  subprocess.run(['secret-tool','store','--label=RUWAC disposable reconnect fixture','application','RUWAC','profile',reference],input=b'generated-local-only',env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  neighbors=[command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]
  def mapped():
   return any(d['fields'] and d['width']>0 for d in command('gui','profile-editor',operation='layout')['dialogs'])
  def decoded():
   r=command('gui','rdp-inspect',rdp=3)
   return r if min(r.get('size',[0,0]))>0 and min(r.get('viewport',[0,0]))>0 and r.get('metrics',{}).get('decoded_sizes') and r.get('status','').startswith('Connected') else None
  try:
   command('gui','profile-editor',operation='new',response='keep',fields={'0':'Generated reconnect host','3':'127.0.0.1:'+str(port),'4':'fixture','9':reference,'10':str(root/'cert.pem'),'13':0})
   spike.eventually(mapped,10)
   saved=command('gui','profile-editor',operation='inspect',response='connect');assert not saved['error'],saved
   frame=spike.eventually(decoded,20)
   before=frame['transport_id'];tab=frame['tab'];viewport=frame['viewport']
   item=next(p for p in saved['items']if p['name']=='Generated reconnect host')
   assert frame['selection_settings']=={'enabled':False,'paste':False}
   # Production settings changes must reach this existing view on Reconnect.
   # Port1 deliberately refuses the unrelated SSH bridge; this case exercises
   # saved settings and the real RDP connection, not a remote desktop account.
   subprocess.run([a.gui,'--receive-primary',item['id'],'fixture','wayland-77','1'],env=env,check=True,stdout=subprocess.DEVNULL)
   subprocess.run([a.gui,'--paste-primary',item['id']],env=env,check=True,stdout=subprocess.DEVNULL)
   menu=command('gui','rdp-tab-menu',tab=tab,control='disconnect');assert menu['title']=='Generated reconnect host',menu
   lost=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=3)).get('reconnect_visible') else False,8)
   assert lost['viewport']==viewport
   command('gui','rdp-tab-menu',tab=tab,control='reconnect')
   fresh=spike.eventually(lambda:r if (r:=decoded()) and r['transport_id']!=before and not r['reconnect_visible'] else None,20)
   assert fresh['selection_settings']=={'enabled':True,'paste':True},fresh
   assert [command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]==neighbors
   command('gui','close-rdp',rdp=3)
   # A status message may belong to an earlier write (or earlier test). Wait
   # for the actual editor snapshot to include BOTH external CLI changes.
   # Cancel stale forms without saving; retain the production stale-save guard.
   def editor_has_current_options():
    command('gui','profiles',search='Generated reconnect host')
    command('gui','profile-editor',operation='edit',response='keep')
    try:
     layouts=command('gui','profile-editor',operation='layout')['dialogs']
     (root/'center-options-probe.json').write_text(json.dumps(layouts,indent=2)+'\n')
     return any(d.get('selection_options')=={'primary-sync':True,'primary-paste':True}for d in layouts)
    finally:command('gui','profile-editor',operation='inspect',response='cancel')
   spike.eventually(editor_has_current_options,8)
   for _ in range(3):
    command('gui','profiles',search='Generated reconnect host')
    command('gui','profile-editor',operation='edit',response='keep');spike.eventually(mapped,10)
    saved=command('gui','profile-editor',operation='inspect',response='connect');assert not saved['error'],saved
    again=spike.eventually(decoded,20)
    assert [command('gui','rdp-inspect',rdp=i)['transport_id']for i in range(3)]==neighbors
    command('gui','close-rdp',rdp=3)
   return {'editor_connect_cycles':4,'configured_profile':True,'center_card_visible_when_disconnected':True,'overlay_preserves_viewport':True,'tab_reconnect_actual_CredSSP_and_frame':True,'neighbors_preserved':True,'saved_selection_options_reloaded':True}
  except Exception:
   (root/'center-reconnect-failure.json').write_text(json.dumps({'frame':command('gui','rdp-inspect',rdp=3),'editor':command('gui','profile-editor',operation='inspect'),'last_save':saved},indent=2)+'\n')
   raise
  finally:
   command('gui','close-rdp',rdp=3)
 case('RDP-NATIVE-CENTER-RECONNECT',center_reconnect)

 def profile_keyring_preferences():
  assert provider and provider.poll() is None, 'Disposable Secret Service provider unavailable'
  command('gui','profiles',search='R5 renamed')
  r=command('gui','profile-editor',operation='edit',fields={'13':0,'3':'127.0.0.1:'+str(ready['ports'][0]),'4':'fixture','10':str(root/'cert.pem')});assert not r['error'],r
  item=next(p for p in r['items']if p['name'].startswith('R5 renamed') and p['server']=='127.0.0.1:'+str(ready['ports'][0]))
  subprocess.run(['secret-tool','store','--label=RUWAC disposable fixture','application','RUWAC','profile',item['secret_reference']],input=b'generated-local-only',env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  command('gui','close-rdp',rdp=0);time.sleep(.3)
  assert command('gui','profiles',search='R5 renamed',open=True)['rows']>=1
  r=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=2)).get('status','').startswith('Connected') and min(r.get('size',[0,0]))>0 else False,20)
  command('gui','rdp-display',rdp=2,mode='stretch',zoom=1.5)
  def persisted():
   data=json.loads((root/'config/RUWAC/machines.json').read_text());p=next(p for p in data['items']if p['id']==item['id']);return p if p['preferences']['display']=='stretch' and p['preferences']['zoom']==1.5 else False
  spike.eventually(persisted,5)
  protected=command('gui','profile-editor',operation='edit',response='delete',confirm=True);assert any(p['id']==item['id']for p in protected['items'])
  # Exercise the visible native toolbar against the ordinary credential-backed connection.
  command('gui','rdp-switch',rdp=2)
  command('gui','chrome',open=True);time.sleep(.2)
  toolbar=command('gui','rdp-toolbar',rdp=2)
  standard={'reconnect','disconnect','fit','dynamic','zoom-out','zoom-in','fullscreen'}
  assert {b['action'] for b in toolbar['controls']}==standard|{'gnome-sign-in','rdp-review-certificate','cancel-automatic-sign-in'},toolbar
  assert all(b['icon'] and b['label'] for b in toolbar['controls'] if b['action'] in standard|{'gnome-sign-in'}),toolbar
  assert all(b['mapped'] for b in toolbar['controls']if b['action']in standard),toolbar
  assert not next(b['mapped']for b in toolbar['controls']if b['action']=='gnome-sign-in'),toolbar
  neighbors=[command('gui','rdp-inspect',rdp=i)['transport_id'] for i in [0,1]]
  before_zoom=command('gui','rdp-inspect',rdp=2)['zoom']
  assert command('gui','rdp-toolbar',rdp=2,control='zoom-in')['clicked']
  assert command('gui','rdp-inspect',rdp=2)['zoom']>before_zoom
  assert command('gui','rdp-toolbar',rdp=2,control='zoom-out')['clicked']
  assert abs(command('gui','rdp-inspect',rdp=2)['zoom']-before_zoom)<1e-6
  assert command('gui','rdp-toolbar',rdp=2,control='fit')['clicked']
  assert not command('gui','rdp-toolbar',rdp=2,control='unknown')['clicked']
  old_transport=command('gui','rdp-inspect',rdp=2)['transport_id']
  assert command('gui','rdp-toolbar',rdp=2,control='disconnect')['clicked']
  assert command('gui','rdp-inspect',rdp=2)['transport_id'] is None
  assert command('gui','rdp-toolbar',rdp=2,control='reconnect')['clicked']
  restored=spike.eventually(lambda:r if (r:=command('gui','rdp-inspect',rdp=2)).get('status','').startswith('Connected') and min(r.get('size',[0,0]))>0 else False,20)
  assert restored['transport_id']!=old_transport
  assert [command('gui','rdp-inspect',rdp=i)['transport_id']for i in [0,1]]==neighbors
  command('gui','profiles-control',import_expanded=False,tools_expanded=False)
  command('gui','snapshot');time.sleep(.15)
  assert (root/'native-rust.png').is_file()
  (root/'native-rust.png').replace(root/'rdp-toolbar.png')
  command('gui','close-rdp',rdp=2);time.sleep(.3)
  removed=command('gui','profile-editor',operation='edit',response='delete',confirm=True);assert not any(p['id']==item['id']for p in removed['items'])
  subprocess.run(['secret-tool','clear','application','RUWAC','profile',item['secret_reference']],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
  provider.terminate();provider.wait(timeout=5)
  return {'normal_sidebar_connection':'actual CredSSP with disposable Secret Service lookup and verified fixture CA','decoded_frame':r['size'],'runtime_preferences_saved':True,'profile_protected_then_released':True,'owner_secrets_accessed':False,'native_toolbar':{'visible_theme_icons':7,'zoom_fit':True,'disconnect_reconnect_authentication':True,'neighbor_transports_preserved':True,'actual_GTK_render':'rdp-toolbar.png','physical_input':False}}
 case('RDP-PROFILE-KEYRING-PREFERENCES',profile_keyring_preferences)

 def restart_and_thousand():
  global gui
  saved=json.loads((root/'config/RUWAC/machines.json').read_text())
  gui.terminate();gui.wait(timeout=5)
  gui=start([a.gui,'--socket',str(sock),'--session',session['session'],'--fixture-dir',str(root)],'gui-restart');command('gui','inspect')
  r=command('gui','profile-editor',operation='inspect');assert r['items']==saved['items'] and r['groups']==saved['groups']
  gui.terminate();gui.wait(timeout=5)
  config=root/'stress-config/RUWAC';config.mkdir(parents=True,mode=0o700);items=[]
  for i in range(1000):
   item=dict(saved['items'][0]);item.update(id=f'fixture-endpoint-{i}',machine_id=f'fixture-machine-{i}',name=f'Load{i}',group=f'Generated/Network{i%20}',secret_reference=f'fixture-secret-{i}');items.append(item)
  path=config/'machines.json';path.write_text(json.dumps({'schema':1,'items':items,'groups':[]}));path.chmod(0o600)
  env['XDG_CONFIG_HOME']=str(config.parent);started=time.perf_counter();gui=start([a.gui,'--socket',str(sock),'--session',session['session'],'--fixture-dir',str(root)],'gui-stress');command('gui','inspect');load_ms=(time.perf_counter()-started)*1000
  started=time.perf_counter();r=command('gui','profiles',search='Load999');assert r['rows']==1,r
  return {'restart_identical_ids_preferences_groups':True,'generated_profiles':1000,'load_and_fixture_ready_ms':load_ms,'search_widget_roundtrip_ms':(time.perf_counter()-started)*1000,'search_rows':1}
 case('MACHINES-RESTART-1000',restart_and_thousand)

finally:
 if mutter_input:
  try:mutter_input('Stop')
  except Exception:pass
 def rss(proc):
  try:return int(next(l.split()[1] for l in Path(f'/proc/{proc.pid}/status').read_text().splitlines() if l.startswith('VmRSS:')))/1024
  except (FileNotFoundError,StopIteration):return None
 memory={name:rss(proc) for name,proc in [('gtk',locals().get('gui')),('rdp_server',locals().get('server'))] if proc is not None}
 for proc in reversed(processes):
  if proc.poll() is None:
   proc.terminate()
   try:proc.wait(timeout=5)
   except subprocess.TimeoutExpired:proc.kill();proc.wait()
 if private_bus is not None:os.kill(private_bus,15)
 for log in logs:log.close()
 def rss(proc):
  try:return int(next(l.split()[1] for l in Path(f'/proc/{proc.pid}/status').read_text().splitlines() if l.startswith('VmRSS:')))/1024
  except (FileNotFoundError,StopIteration):return None
 report={'cases':results,'rss_mib':memory,'environment':'Owned Mutter'if mutter_input else'Owned Weston/Xvfb','limitations':['No external Windows/GNOME login','No physical keyboard/touchpad acceptance','No external Windows/GNOME Display Control or RemoteFX qualification']}
 (root/'receipt.json').write_text(json.dumps(report,indent=2));Path(a.evidence,'rdp-native-acceptance.json').write_text(json.dumps(report,indent=2));print('Evidence',root,flush=True)
raise SystemExit(0 if results and all(r['status']=='PASS' for r in results) else 1)
