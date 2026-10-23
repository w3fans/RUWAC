#!/usr/bin/env python3
"""Owned Wayland terminal menus and confirmed PRIMARY paste across a real RDP client.

The generated RDP server supplies a synthetic framebuffer; its received middle
press serial and packet counts gate a native button replay on the second private Mutter
seat. The pinned test server drops middle-button flags; its MouseEvent conversion
is not evidence of wire-level middle-button correctness. Both GUIs and the
SSH selection peer are production code. No owner desktop or real host is used.
"""
import argparse, ast, hashlib, json, os, re, select, subprocess, sys, tempfile, time
from pathlib import Path
from gi.repository import Gio, GLib
from spike import Client, eventually

parser=argparse.ArgumentParser()
for name in ['binary','gui','server','evidence']:parser.add_argument('--'+name,required=True)
a=parser.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test' and os.environ.get('RUWAC_TEST_MUTTER_INPUT')=='1'
# Keep software renderers, icon loaders and Tokio within the existing task
# budget on large hosts. This restricts resources; it never raises the limit.
os.sched_setaffinity(0, sorted(os.sched_getaffinity(0))[:2])
root=Path(tempfile.mkdtemp(prefix='t-',dir=a.evidence));root.chmod(0o700);children=[];inputs=[];checks=[];sequences={};forwarded=0;middle_events=[];gesture_baseline=None;replay_down=False;active_rdp=0
def load(path):
 try:return json.loads(path.read_text())
 except(OSError,ValueError):return {}
def write(path,value):
 temp=path.with_suffix('.tmp');temp.write_text(json.dumps(value));temp.replace(path)
def start(argv,name,env):
 log=(root/(name+'.log')).open('wb');child=subprocess.Popen([str(v)for v in argv],env=env,stdout=log,stderr=log);children.append(child);return child
def mark(name):checks.append(name);print(name,'PASS',flush=True)
def environment(base,parent):
 env={**parent,'GTK_A11Y':'none','NO_AT_BRIDGE':'1','GSETTINGS_BACKEND':'memory','GDK_BACKEND':'wayland','GSK_RENDERER':'cairo','LP_NUM_THREADS':'2','RAYON_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2','PYTHONDONTWRITEBYTECODE':'1','SHELL':'/bin/bash'}
 for key,leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
  path=base/leaf;path.mkdir(parents=True,mode=0o700);env[key]=str(path)
 (base/'config/RUWAC').mkdir(mode=0o700)
 return env

def resources(stage):
 rows=[]
 for proc in Path('/proc').iterdir():
  if not proc.name.isdigit():continue
  try:
   if proc.stat().st_uid!=os.getuid():continue
   environ=(proc/'environ').read_bytes()
   if str(a.evidence).encode()not in environ:continue
   status=dict(line.split(':',1)for line in(proc/'status').read_text().splitlines())
   rows.append({'pid':proc.name,'name':status['Name'].strip(),'threads':int(status['Threads']),'cpus':status['Cpus_allowed_list'].strip()})
  except(OSError,ValueError,KeyError):pass
 (root/('resources-'+stage+'.json')).write_text(json.dumps(rows,indent=2))

def command(who,action,**kw):
 if who=='source' and action in ['rdp-switch','rdp-inspect','rdp-pointer','rdp-policy'] and 'rdp'not in kw:kw['rdp']=active_rdp
 directory=root/who;seq=sequences.get(who,0)+1;sequences[who]=seq
 write(directory/'gui-request.json',dict(seq=seq,action=action,**kw))
 try:return eventually(lambda:v['result']if(v:=load(directory/'gui-response.json')).get('seq')==seq else None,12)
 except AssertionError:
  snapshot=[]
  for child in children:
   try:
    proc=Path('/proc')/str(child.pid);snapshot.append({'pid':child.pid,'status':(proc/'status').read_text(),'threads':{task.name:(task/'wchan').read_text()for task in(proc/'task').iterdir()}})
   except OSError:pass
  (root/'timeout-processes.json').write_text(json.dumps(snapshot,indent=2));raise
def peer_command(who,action,which='clipboard',text=None):
 directory=root/who;seq=sequences.get(who,0)+1;sequences[who]=seq
 write(directory/'command.json',dict(seq=seq,action=action,which=which,text=text))
 return eventually(lambda:v['text']if(v:=load(directory/'response.json')).get('seq')==seq else None,10)
def rpc(sock,method,params=None):
 with Client(sock)as client:return client.call(method,params)
def private_input(address=None):
 bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)if address is None else Gio.DBusConnection.new_for_address_sync(address,Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
 service='org.gnome.Mutter.RemoteDesktop'
 eventually(lambda:bus.call_sync('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','NameHasOwner',GLib.Variant('(s)',(service,)),None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0],10)
 path=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
 def call(method,params=None):return bus.call_sync(service,path,service+'.Session',method,params,None,Gio.DBusCallFlags.NONE,5000,None)
 call('Start')
 # Provision the lazy virtual pointer before GTK clients bind their seats.
 call('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)))
 inputs.append(call);return call

def key(call,n,pressed):call('NotifyKeyboardKeycode',GLib.Variant('(ub)',(n,pressed)));time.sleep(.025)
def tap(call,n):key(call,n,True);key(call,n,False)
def alt_tab(call):key(call,56,True);tap(call,15);key(call,56,False);time.sleep(.2)
def move(call,x,y):
 call('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));time.sleep(.04)
 call('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x),float(y))));time.sleep(.05)
def button(call,n,pressed):call('NotifyPointerButton',GLib.Variant('(ib)',(n,pressed)));time.sleep(.06)
def drag(call,start,end):
 move(call,*start);button(call,272,True)
 for _ in range(6):call('NotifyPointerMotionRelative',GLib.Variant('(dd)',((end[0]-start[0])/6,(end[1]-start[1])/6)));time.sleep(.035)
 button(call,272,False);time.sleep(.15)
def front(who,call):
 for _ in range(8):
  command(who,'focus-view')
  if command(who,'inspect')['active']:return
  alt_tab(call)
 raise AssertionError('Private GUI focus precondition failed: '+who)
def fullscreen(who,call):
 front(who,call);key(call,29,True);key(call,56,True);tap(call,28);key(call,56,False);key(call,29,False);time.sleep(.3)
def select_source(history=False):
 front('source',local_input);command('source','switch')
 if history:
  menu=command('source','rdp-tab-menu',tab=0,control='select-text');assert 'Select text…'in menu['menu']
  eventually(lambda:command('source','copy-state')['visible'])
  info=command('source','copy-state');ox,oy=info['origin'];drag(local_input,(ox+1,oy+3),(ox+min(info['size'][0]-3,600),oy+min(info['size'][1]-3,65)))
 else:
  info=command('source','inspect');ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
  drag(local_input,(ox+cw*.5,oy+ch*.5),(ox+(len(payload.split('\n')[-1])-.5)*cw,oy+1.5*ch))
 text=command('source','copy-state')['selection'];assert text.rstrip('\n')==('READY\n'+payload if history else payload),repr(text)
 return text

def forward_middle():
 global replay_down,gesture_baseline
 events=load(root/'source'/('input-'+str(port)+'.mouse.json')).get('events',[])
 if gesture_baseline is None:return events
 state=command('source','rdp-pointer')
 # ironrdp-server 0.13 loses middle flags in its fast-path conversion. Check
 # the real client's middle-press serial and received packet counts independently of
 # the native target paste. No synthetic keyboard or left-click is used.
 delivered=len(events)-gesture_baseline[0]
 pressed=state['middle_presses']>gesture_baseline[1]
 if not replay_down and pressed and delivered>=2:
  button(remote_input,274,True);middle_events.append('down');replay_down=True
 if replay_down and state['held_buttons']==0 and not state['state']['selection_paste_pending']:
  button(remote_input,274,False);middle_events.append('up');replay_down=False;gesture_baseline=None
 return events

def rdp_focus():
 front('source',local_input);command('source','rdp-switch');tap(local_input,42)
 eventually(lambda:command('source','rdp-inspect')['selection_sharing_ready'],10)
 assert command('receiver','inspect')['active']
 info=command('receiver','inspect');ox,oy=info['area_origin'];move(remote_input,ox+80,oy+80)

def middle(release=True, disabled_route=False):
 global gesture_baseline
 gesture_baseline=(len(load(root/'source'/('input-'+str(port)+'.mouse.json')).get('events',[])),command('source','rdp-pointer')['middle_presses'])
 info=command('source','rdp-inspect');w,h=info['viewport'];x,y=w*.5,h*.5
 command('source','rdp-pointer',x=x,y=y,button=2,release=False,disable_primary_paste=disabled_route)
 if release:command('source','rdp-pointer',x=x,y=y,button=2,release=True)
 return x,y

def contents():
 try:return (root/'received.bin').read_bytes()
 except FileNotFoundError:return b''
def check_clipboards():
 assert peer_command('local-peer','read')=='LOCAL_CLIPBOARD_RETAINED'
 assert peer_command('remote-peer','read')=='REMOTE_CLIPBOARD_RETAINED'

def wait_input(expected):
 def done():
  forward_middle()
  return contents()==expected
 eventually(done,12)

proxy_source=r'''
import json,os,socket,struct,sys,threading,time
from pathlib import Path
front,back,root=map(Path,sys.argv[1:]);server=socket.socket(socket.AF_UNIX);server.bind(str(front));os.chmod(front,0o600);server.listen(8);(root/'proxy-ready').touch()
def read(s,n):
 out=b''
 while len(out)<n:
  chunk=s.recv(n-len(out))
  if not chunk:raise ValueError('closed')
  out+=chunk
 return out
def client(s):
 try:
  s.settimeout(5);size=struct.unpack('!I',read(s,4))[0];assert size<=1024*1024;body=read(s,size)
  (root/'proxy-started').write_text(str(time.monotonic_ns()))
  try:delay=float((root/'proxy-delay').read_text())
  except OSError:delay=0
  time.sleep(min(max(delay,0),3))
  with socket.socket(socket.AF_UNIX)as target:
   target.settimeout(3);target.connect(str(back));target.sendall(struct.pack('!I',size)+body);n=struct.unpack('!I',read(target,4))[0];assert n<4096;s.sendall(struct.pack('!I',n)+read(target,n))
 except(OSError,ValueError,AssertionError):pass
 finally:s.close()
while True:
 s,_=server.accept();threading.Thread(target=client,args=(s,),daemon=True).start()
'''
try:
 source=root/'source';receiver=root/'receiver';source.mkdir(mode=0o700);receiver.mkdir(mode=0o700);runtime=root/'r';runtime.mkdir(mode=0o700)
 env_local=environment(source,os.environ);env_remote=environment(receiver,os.environ)
 for k in ['DISPLAY','WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS','XAUTHORITY']:env_remote.pop(k,None)
 env_remote['XDG_RUNTIME_DIR']=str(runtime)
 # A separate pipe supplies the private bus address, never an owner's bus.
 # This private bus needs only explicitly started fixture services. Avoid
 # starting a second GNOME portal/settings/keyring stack during clipboard tests.
 bus_config=root/'remote-bus.xml';bus_config.write_text('<busconfig><type>session</type><listen>unix:tmpdir='+str(runtime)+'</listen><auth>EXTERNAL</auth><policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>')
 log=(root/'remote-bus-2.log').open('wb');bus=subprocess.Popen(['dbus-daemon','--config-file='+str(bus_config),'--nofork','--print-address=1'],env=env_remote,stdout=subprocess.PIPE,stderr=log);children.append(bus)
 assert select.select([bus.stdout],[],[],5)[0];address=bus.stdout.readline().decode().strip();assert address.startswith('unix:');env_remote['DBUS_SESSION_BUS_ADDRESS']=address
 start(['mutter','--headless','--virtual-monitor','900x600','--wayland','--wayland-display','wayland-77'],'remote-mutter',env_remote)
 eventually(lambda:(runtime/'wayland-77').exists(),10);display=re.search(r'Using public X11 display (:[0-9]+)',(root/'remote-mutter.log').read_text()).group(1);auth=list(runtime.glob('.mutter-Xwaylandauth.*'));assert len(auth)==1
 env_remote.update(WAYLAND_DISPLAY='wayland-77',DISPLAY=display,XAUTHORITY=str(auth[0]))
 local_input=private_input();remote_input=private_input(address)
 # The independent GTK peers own only generated Ctrl+C data for separation checks.
 tree=ast.parse(Path(__file__).with_name('primary_selection.py').read_text());peer_source=next(ast.literal_eval(n.value)for n in tree.body if isinstance(n,ast.Assign)and any(isinstance(t,ast.Name)and t.id=='peer_source'for t in n.targets))
 peer=root/'peer.py';peer.write_text(peer_source)
 for name,env,token in [('local-peer',env_local,'LOCAL_CLIPBOARD_RETAINED'),('remote-peer',env_remote,'REMOTE_CLIPBOARD_RETAINED')]:
  directory=root/name;directory.mkdir(mode=0o700);start([sys.executable,peer,directory,env.get('DISPLAY','')],name,dict(env,RUWAC_PRIMARY_NATIVE_PEER='1'));eventually(lambda:(directory/'ready').exists(),10)
  inp=local_input if name=='local-peer'else remote_input;move(inp,400,300);button(inp,272,True);button(inp,272,False);peer_command(name,'set',text=token)
 fake=root/'bin';fake.mkdir(mode=0o700);systemctl=fake/'systemctl';systemctl.write_text('#!/usr/bin/python3\nprint('+repr('WAYLAND_DISPLAY=wayland-77\nDISPLAY='+display+'\nXAUTHORITY='+str(auth[0])+'\nXDG_RUNTIME_DIR='+str(runtime))+')\n');systemctl.chmod(0o700)
 ssh=fake/'ssh';ssh.write_text('#!/usr/bin/python3\nimport os,sys\nos.environ["RUWAC_PRIMARY_TRACE"]="1"\nos.dup2(os.open('+repr(str(root/'selection-peer.log'))+',os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600),2)\nos.execl("/bin/sh","sh","-c",sys.argv[-1])\n');ssh.chmod(0o700);env_local['PATH']=str(fake)+':'+os.environ['PATH']
 subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-subj','/CN=generated-loopback','-addext','subjectAltName=IP:127.0.0.1','-keyout',str(source/'key.pem'),'-out',str(source/'cert.pem')],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);(source/'key.pem').chmod(0o600)
 start([a.server,source],'RDP-server',env_local);ready=eventually(lambda:load(source/'server-ready.json'),12);port=ready['ports'][0]
 profile={'id':'endpoint_primary_terminal','machine_id':'machine_primary_terminal','name':'Generated RDP desktop','group':'','protocol':'RDP','server':'127.0.0.1:'+str(port),'username':'fixture','secret_reference':'generated','favorite':False,'last_used':0,'tags':[],'preferences':{'primary_sync':True,'primary_paste':True,'primary_user':'fixture','primary_display':'wayland-77','clipboard':0}}
 profile_b={**profile,'id':'endpoint_primary_terminal_b','machine_id':'machine_primary_terminal_b','name':'Generated second RDP connection','server':'127.0.0.1:'+str(ready['ports'][1]),'preferences':dict(profile['preferences'])}
 path=Path(env_local['XDG_CONFIG_HOME'])/'RUWAC/machines.json';path.write_text(json.dumps({'schema':1,'items':[profile,profile_b],'groups':[]}));path.chmod(0o600)
 source_program=root/'source.py';payload='GENERATED_terminal_Ž\nsecond_line_end'
 source_program.write_text('import os,tty\ntty.setraw(0)\nos.write(1,b"READY")\nwhile True:\n b=os.read(0,4096)\n if b==b"q":break\n if b in [b"s",b"m"]:\n  os.write(1,(b"\\x1b[?1000h\\x1b[?1006h" if b==b"m" else b"\\x1b[?1000l\\x1b[?1006l")+b"\\x1b[3J\\x1b[2J\\x1b[H"+'+repr(payload.replace('\n','\r\n').encode())+')\n')
 target=root/'target.py';target.write_text('import os,sys,tty\nfrom pathlib import Path\ntty.setraw(0)\np=Path(sys.argv[1]);p.write_bytes(b"")\nos.write(1,b"GENERATED_TARGET_READY\\x1b[?2004h")\nwhile True:\n b=os.read(0,4096)\n if not b:break\n with p.open("ab")as f:f.write(b)\n')
 local_socket=source/'control.sock';remote_socket=receiver/'control.sock'
 for sock,env,name in [(local_socket,env_local,'local-daemon'),(remote_socket,env_remote,'remote-daemon')]:start([a.binary,'--socket',sock,'serve'],name,env);eventually(sock.exists)
 left=rpc(local_socket,'session.create',{'name':'Generated source','argv':[sys.executable,str(source_program)]});right=rpc(remote_socket,'session.create',{'name':'Generated target','argv':[sys.executable,str(target),str(root/'received.bin')]})
 local_target=rpc(local_socket,'session.create',{'name':'Generated local paste target','argv':[sys.executable,str(target),str(root/'received-local.bin')]})
 local_nested=rpc(local_socket,'session.create',{'name':'Generated local nested target','argv':[a.binary,'--socket',str(local_socket),'attach','--session',local_target['session']]})
 start([a.gui,'--new-instance','--socket',local_socket,'--session',left['session'],'--session',local_nested['session'],'--fixture-dir',source],'local-GUI',env_local)
 start([a.gui,'--new-instance','--socket',remote_socket,'--session',right['session'],'--fixture-dir',receiver],'remote-GUI',env_remote)
 command('source','focus-view',view=0)
 eventually(lambda:command('source','inspect').get('target')==left['pane'],20);eventually(lambda:command('receiver','inspect').get('target')==right['pane'],20)
 fullscreen('source',local_input);fullscreen('receiver',remote_input)
 front_socket=runtime/'ruwac';front_socket.mkdir(mode=0o700);proxy=root/'proxy.py';proxy.write_text(proxy_source);start([sys.executable,proxy,front_socket/'control.gui.sock',remote_socket.with_suffix('.gui.sock'),root],'GUI-IPC-delay-proxy',env_remote);eventually(lambda:(root/'proxy-ready').exists())
 rpc(local_socket,'pane.sendText',{'pane':left['pane'],'text':'s'});time.sleep(.25)
 empty=command('source','rdp-tab-menu',tab=0,control='inspect');assert empty['stable_model'] and empty['presented_menu']==empty['menu'];assert 'Copy'in empty['menu'] and 'Dynamic resolution'not in empty['menu'] and empty['enabled']['paste'] and not empty['enabled']['copy'];mark('terminal-tabs-show-terminal-actions-instead-of-disabled-RDP-menu')
 text=select_source();menu=command('source','rdp-tab-menu',tab=0,control='inspect');assert menu['enabled']['copy'];mark('live-terminal-selection-is-copyable')
 resources('before-rdp');command('source','rdp-open',port=port,profile=profile);eventually(lambda:command('source','rdp-inspect').get('metrics',{}).get('decoded_updates',0)>0,20);rdp_focus()
 rdp_menu=command('source','rdp-tab-menu',tab=2,control='inspect');assert rdp_menu['stable_model'] and rdp_menu['presented_menu']==rdp_menu['menu'];assert 'Dynamic resolution'in rdp_menu['menu']and 'Copy'not in rdp_menu['menu']and 'Retry automatic sign-in'in rdp_menu['menu'];mark('RDP-tabs-retain-their-own-connection-menu')
 before=len(middle_events);middle(disabled_route=True);time.sleep(.25);forward_middle()
 assert len(middle_events)==before and contents()==b'' and not (root/'proxy-started').exists()
 assert 'disabled for this connection' in command('source','rdp-inspect')['mouse_selection']
 mark('disabled-selection-route-blocks-local-selection-instead-of-pasting-unrelated-remote-text')
 before=len(middle_events);middle();eventually(lambda:not command('source','rdp-inspect')['selection_paste_pending'],8);forward_middle();assert len(middle_events)==before and contents()==b'';mark('remote-reception-default-deny-does-not-paste-stale-PRIMARY')
 subprocess.run([a.gui,'--enable-primary-receive'],env=env_remote,check=True,stdout=subprocess.DEVNULL)
 expected=b'\x1b[200~'+text.encode()+b'\x1b[201~';x,y=middle(release=False);eventually(lambda:('down'in middle_events)or (forward_middle()and 'down'in middle_events),10);command('source','rdp-pointer',x=x,y=y,button=2,release=True);wait_input(expected);assert middle_events[-2:]==['down','up'];mark('confirmed-remote-primary-then-real-middle-paste-preserves-multiline-Unicode')
 check_clipboards();mark('both-Ctrl-C-clipboards-remain-separate')
 rpc(local_socket,'pane.sendText',{'pane':left['pane'],'text':'q'});eventually(lambda:rpc(local_socket,'pane.list',{'session':left['session']})[0]['ended']);time.sleep(.25);text=select_source();menu=command('source','rdp-tab-menu',tab=0,control='inspect');assert menu['enabled']['copy']and not menu['enabled']['paste'];rdp_focus();(root/'proxy-delay').write_text('.35');middle();wait_input(expected+b'\x1b[200~'+text.encode()+b'\x1b[201~');mark('ended-terminal-selection-pastes-with-release-before-confirmation')
 # Delay a real peer offer, then leave its RDP tab. A late confirmation must
 # not turn into input in a different terminal or a different RDP view.
 text=select_source(history=True);rdp_focus();baseline=contents();count=len(middle_events);(root/'proxy-delay').write_text('.65');old=(root/'proxy-started').read_text();middle();eventually(lambda:(root/'proxy-started').read_text()!=old);command('source','switch');time.sleep(1.2);forward_middle();assert contents()==baseline and len(middle_events)==count;mark('tab-change-cancels-delayed-selection-with-zero-remote-button-events')
 (root/'proxy-delay').write_text('0');text=select_source(history=True);rdp_focus();command('source','rdp-policy',policy=4);count=len(middle_events);middle();time.sleep(.4);forward_middle();assert len(middle_events)==count and contents()==baseline;mark('disabled-clipboard-policy-refuses-selection-paste')
 command('source','rdp-policy',policy=0);check_clipboards();
 # A selection received from an RDP tab must remain a deliberate application
 # selection when the user pastes into another tab. Require the confirmed SSH
 # PRIMARY path, not an unconfirmed middle-button replay with stale text.
 rdp_focus();tap(remote_input,42)
 # Exercise the production RDP drag path that retires a previous local
 # selection before the remote desktop makes its own deliberate selection.
 command('source','rdp-pointer',x=100.,y=100.,button=1,release=False)
 command('source','rdp-pointer',x=130.,y=110.,button=0,release=False)
 command('source','rdp-pointer',x=130.,y=110.,button=1,release=True)
 prior_trace=len((root/'selection-peer.log').read_text().splitlines())
 command('receiver','copy',primary=True);remote_selection=command('receiver','copy-state')['selection'];assert remote_selection
 command('receiver','switch')
 # PRIMARY is delivered to the focused Wayland client; an inactive GTK peer
 # has no current selection offer. Allow the active RDP bridge to publish it,
 # then focus the independent peer for the compositor read and return.
 def relayed():
  rows=(root/'selection-peer.log').read_text().splitlines()[prior_trace:]
  return any((d:=json.loads(line)).get('primary_trace')=='complete'and d.get('size')==len(remote_selection.encode())for line in rows if line.startswith('{'))
 eventually(relayed,10)
 tick=command('source','inspect')['ticks'];eventually(lambda:command('source','inspect')['ticks']>=tick+3)
 for _ in range(8):
  if peer_command('local-peer','state')['active']:break
  alt_tab(local_input)
 else:raise AssertionError('Independent PRIMARY consumer could not gain focus')
 eventually(lambda:peer_command('local-peer','read',which='primary')==remote_selection,10)
 rdp_focus()
 # A selection from this very desktop must keep its native middle-click path,
 # including with the optional inbound receiver and cross-desktop paste off.
 subprocess.run([a.gui,'--disable-primary-receive'],env=env_remote,check=True,stdout=subprocess.DEVNULL)
 same_before=contents();same_proxy=(root/'proxy-started').read_text();middle(disabled_route=True)
 wait_input(same_before+b'\x1b[200~'+remote_selection.encode()+b'\x1b[201~')
 assert (root/'proxy-started').read_text()==same_proxy
 check_clipboards();mark('same-RDP-native-selection-pastes-without-cross-desktop-receiver')
 subprocess.run([a.gui,'--enable-primary-receive'],env=env_remote,check=True,stdout=subprocess.DEVNULL)
 # Keep the original confirmed-path requirement for another connection. The
 # two actual RDP transports share this generated peer desktop, not real hosts.
 port=ready['ports'][1]
 command('source','rdp-open',port=port,profile=profile_b);active_rdp=1
 eventually(lambda:command('source','rdp-inspect').get('metrics',{}).get('decoded_updates',0)>0,20)
 rdp_focus()
 previous=(root/'proxy-started').read_text();before=contents();middle()
 eventually(lambda:(root/'proxy-started').read_text()!=previous,8)
 wait_input(before+b'\x1b[200~'+remote_selection.encode()+b'\x1b[201~')
 check_clipboards();mark('RDP-origin-PRIMARY-remains-pasteable-through-confirmed-tab-path')
 # Match an editor selection in one remote desktop followed by a native middle
 # click in an outer terminal tab. Its nested TUI must receive a paste, rather
 # than choosing an unrelated canonical session buffer.
 front('source',local_input);command('source','focus-view',view=1)
 eventually(lambda:command('source','inspect',view=1).get('target')==local_nested['pane'],10)
 info=command('source','inspect',view=1);ox,oy=info['area_origin']
 move(local_input,ox+25,oy+25);button(local_input,274,True);button(local_input,274,False)
 local_expected=b'\x1b[200~'+remote_selection.encode()+b'\x1b[201~'
 eventually(lambda:(root/'received-local.bin').read_bytes()==local_expected,10)
 check_clipboards();mark('RDP-origin-selection-pastes-exactly-into-outer-nested-terminal-tab')
 rdp_focus()

 # Invalid write requests are refused by the actual native receiver without
 # exposing any read or input operation through the GUI API.
 for value in [{'text':''},{'text':'x\0y'},{'text':'x'*(128*1024+1)},{'text':'test','extra':True}]:
  with Client(remote_socket.with_suffix('.gui.sock'))as client:
   client.call('clipboard.primary.offer',value,error='INVALID_REQUEST' if len(value)!=1 else 'PRIMARY_REFUSED')
 mark('receiver-rejects-empty-NUL-oversized-and-extra-field-requests')
 (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'production_GUI_and_peer':True,'native_Wayland_desktops':2,'RDP':'authenticated generated loopback framebuffer; client middle-press serial and received packet counts gate native private-seat replay; pinned server drops middle flags, no wire-level middle-button assertion','SSH':'stand-in executes exact production peer command','physical_input':False,'owner_desktop_used':False},indent=2)+'\n')
finally:
 for call in reversed(inputs):
  try:call('Stop')
  except Exception:pass
 for child in reversed(children):
  if child.poll()is None:child.terminate()
 for child in reversed(children):
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
 print(root,flush=True)
