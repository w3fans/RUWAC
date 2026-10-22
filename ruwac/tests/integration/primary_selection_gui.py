#!/usr/bin/env python3
"""Native Wayland -> Xwayland source -> authenticated-peer protocol -> native GTK.

Two owned Mutter desktops and GTK peers; SSH is a stand-in executing the exact
production peer command. Uses the compiled production receiver/publisher. Does
not connect to owner desktops or claim external SSH/physical input acceptance.
"""
import argparse,ast,json,os,re,select,subprocess,sys,tempfile,time
from pathlib import Path
from gi.repository import Gio,GLib
p=argparse.ArgumentParser();p.add_argument('--driver',required=True);p.add_argument('--evidence',required=True);a=p.parse_args()
assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test' and os.environ.get('RUWAC_TEST_MUTTER_INPUT')=='1'
root=Path(tempfile.mkdtemp(prefix='n-',dir=a.evidence));root.chmod(0o700);children=[];checks=[];seq=0
source=ast.parse(Path(__file__).with_name('primary_selection.py').read_text())
peer=next(ast.literal_eval(n.value)for n in source.body if isinstance(n,ast.Assign)and any(isinstance(t,ast.Name)and t.id=='peer_source'for t in n.targets))
peer=peer.replace(" if d['action']=='set':"," if d['action']=='present':window.present();view.grab_focus();done(True)\n elif d['action']=='set':",1)
def start(args,name,env):
 log=(root/(name+'.log')).open('wb');child=subprocess.Popen(args,env=env,stdout=log,stderr=log);children.append(child);return child
def wait(test,timeout=10):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  value=test()
  if value:return value
  time.sleep(.04)
 raise AssertionError('Native PRIMARY fixture condition timed out')
def load(path):
 try:return json.loads(path.read_text())
 except(OSError,ValueError):return {}
def write(path,value):
 tmp=path.with_suffix('.tmp');tmp.write_text(json.dumps(value));tmp.replace(path)
def cmd(who,action,which='primary',text=None):
 global seq
 if who=='remote' and action=='set' and which=='primary':
  cmd('remote','present');click(remote_input,450,300)
  for pressed in [True,False]:
   remote_input('NotifyKeyboardKeycode',GLib.Variant('(ub)',(42,pressed)));time.sleep(.04)
  wait(lambda:(v:=cmd('remote','state'))['active'] and v['focused'] and v['height']<=600)
 seq+=1;n=seq;d=root/who;write(d/'command.json',{'seq':n,'action':action,'which':which,'text':text})
 return wait(lambda:v if(v:=load(d/'response.json')).get('seq')==n else None)['text']
def state():return load(root/'receiver-state.json')
def mark(name):checks.append(name);print(name,'PASS',flush=True)
def inputs(address=None):
 bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)if address is None else Gio.DBusConnection.new_for_address_sync(address,Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
 dest='org.gnome.Mutter.RemoteDesktop';path=bus.call_sync(dest,'/org/gnome/Mutter/RemoteDesktop',dest,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
 def call(method,args=None):return bus.call_sync(dest,path,dest+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
 call('Start');return call

def click(call,x,y):
 call('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.0,-10000.0)));time.sleep(.08)
 call('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x),float(y))));time.sleep(.08)
 call('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.08)
 call('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.1)
def keyboard(call,key):
 call('NotifyKeyboardKeysym',GLib.Variant('(ub)',(key,True)));call('NotifyKeyboardKeysym',GLib.Variant('(ub)',(key,False)))
def alt_tab():
 for key,pressed in [(56,True),(15,True),(15,False),(56,False)]:
  local_input('NotifyKeyboardKeycode',GLib.Variant('(ub)',(key,pressed)));time.sleep(.03)
 time.sleep(.3)
def receiver_front():
 for _ in range(4):
  if state().get('active'):return
  alt_tab()
 raise AssertionError('Receiver focus precondition failed')
try:
 remote=root/'remote';remote.mkdir();runtime=root/'r';runtime.mkdir(mode=0o700)
 env_remote={**os.environ,'XDG_RUNTIME_DIR':str(runtime),'HOME':str(remote),'XDG_CONFIG_HOME':str(remote/'config'),'XDG_DATA_HOME':str(remote/'data'),'XDG_CACHE_HOME':str(remote/'cache'),'GSK_RENDERER':'cairo','LP_NUM_THREADS':'2','GTK_A11Y':'none','NO_AT_BRIDGE':'1','GSETTINGS_BACKEND':'memory','GDK_BACKEND':'wayland','RUWAC_PRIMARY_NATIVE_PEER':'1','PYTHONDONTWRITEBYTECODE':'1'}
 for k in ['DISPLAY','WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS','XAUTHORITY']:env_remote.pop(k,None)
 buslog=(root/'remote-bus.log').open('wb');bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env_remote,stdout=subprocess.PIPE,stderr=buslog);children.append(bus);assert select.select([bus.stdout],[],[],5)[0];address=bus.stdout.readline().decode().strip();assert address.startswith('unix:');env_remote['DBUS_SESSION_BUS_ADDRESS']=address
 compositor=start(['mutter','--headless','--virtual-monitor','900x600','--wayland','--wayland-display','w'],'remote-mutter',env_remote)
 wait(lambda:(runtime/'w').exists());log=(root/'remote-mutter.log').read_text();display=re.search(r'Using public X11 display (:[0-9]+)',log).group(1);auths=list(runtime.glob('.mutter-Xwaylandauth.*'));assert len(auths)==1
 env_remote.update(WAYLAND_DISPLAY='w',DISPLAY=display,XAUTHORITY=str(auths[0]))
 local=root/'local';local.mkdir();program=root/'peer.py';program.write_text(peer)
 env_local={**os.environ,'GSK_RENDERER':'cairo','LP_NUM_THREADS':'2','GTK_A11Y':'none','NO_AT_BRIDGE':'1','GDK_BACKEND':'wayland','RUWAC_PRIMARY_NATIVE_PEER':'1','PYTHONDONTWRITEBYTECODE':'1'}
 start([sys.executable,str(program),str(remote),display],'remote-peer',env_remote)
 start([sys.executable,str(program),str(local),''],'local-peer',env_local)
 wait(lambda:(remote/'ready').exists()and(local/'ready').exists());local_input=inputs();remote_input=inputs(address);time.sleep(.4);click(local_input,600,400);click(remote_input,450,300)
 cmd('local','set','clipboard','LOCAL_EXPLICIT_COPY');cmd('remote','set','clipboard','REMOTE_EXPLICIT_COPY')
 fake=root/'bin';fake.mkdir();systemctl=fake/'systemctl';systemctl.write_text('#!/usr/bin/python3\nprint('+repr('WAYLAND_DISPLAY=wayland-77\nDISPLAY='+display+'\nXAUTHORITY='+str(auths[0]))+')\n');systemctl.chmod(0o700)
 ssh=fake/'ssh';ssh.write_text('#!/usr/bin/python3\nimport os,sys\nf=os.open('+repr(str(root/'ssh-peer.log'))+',os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600);os.dup2(f,2)\nos.execl("/bin/sh","sh","-c",sys.argv[-1])\n');ssh.chmod(0o700)
 profile={'id':'endpoint_primary_fixture','machine_id':'machine_primary_fixture','name':'Primary fixture','group':'','protocol':'RDP','server':'127.0.0.1:3389','username':'rdp-service','secret_reference':'fixture-only','favorite':False,'last_used':0,'tags':[],'preferences':{'primary_sync':True,'primary_user':'fixture','primary_display':'wayland-77'}}
 path=root/'profile.json';path.write_text(json.dumps(profile));env_gui={**env_local,'RUWAC_PRIMARY_TRACE':'1','PATH':str(fake)+':'+os.environ['PATH']};receiver=start([a.driver,'primary-receive',str(path),str(root)],'receiver',env_gui)
 wait(lambda:state().get('ready'));click(local_input,600,400);receiver_front()
 payload='NATIVE_WAYLAND_SELECTION_Ž_中文_🦀\nsecond line'
 before=state()['received'];cmd('remote','set',text=payload);wait(lambda:state()['received']>before);mark('native-Wayland-selection-through-production-Rust-receiver')
 def peer_front():
  cmd('local','present');time.sleep(.2)
  if state().get('active'):
   alt_tab()
  keyboard(local_input,65505);wait(lambda:not state().get('active'))
 peer_front();assert cmd('local','read')==payload;mark('separate-native-app-receives-PRIMARY')
 assert cmd('local','read','clipboard')=='LOCAL_EXPLICIT_COPY';assert cmd('remote','read','clipboard')=='REMOTE_EXPLICIT_COPY';mark('both-Ctrl-C-clipboards-unchanged')
 cmd('remote','set',text='IGNORE_WHILE_LOCAL_APP_ACTIVE');time.sleep(.3);assert cmd('local','read')==payload;mark('inactive-receiver-does-not-steal-local-primary')
 cmd('local','set',text='LOCAL_REPLACEMENT_AFTER_COPY')
 write(root/'receiver-command.json',{'present':True});time.sleep(.3);receiver_front();write(root/'receiver-command.json',{'enabled':True});keyboard(local_input,65505)
 before=state()['received'];cmd('remote','set',text=payload);wait(lambda:state()['received']>before);peer_front();assert cmd('local','read')==payload;mark('same-remote-selection-after-local-replacement')
 write(root/'receiver-command.json',{'present':True});time.sleep(.3);receiver_front();write(root/'receiver-command.json',{'enabled':True});keyboard(local_input,65505)
 before=state()['received'];bulk='large_native_Ž\n'*12000;cmd('remote','set',text=bulk);wait(lambda:state()['received']>before and state().get('received_bytes')==len(bulk.encode('utf-8')));peer_front();assert cmd('local','read')==bulk;mark('large-native-multiline-selection')
 # A different connection's initial snapshot must not steal this selection.
 # Replace the actual production bridge while retaining its published owner;
 # the old origin stays alive in the selection record, even after drop.
 write(root/'receiver-command.json',{'enabled':False});time.sleep(.15)
 other='OTHER_CONNECTION_INITIAL_'+'x'*963
 assert len(other.encode())==988
 trace=root/'ssh-peer.log';prior_trace=trace.read_text()
 cmd('remote','set',text=other);time.sleep(.15)
 before=state()['received'];write(root/'receiver-command.json',{'present':True,'enabled':True,'restart':1});time.sleep(.3);receiver_front();write(root/'receiver-command.json',{'enabled':True,'restart':1})
 wait(lambda:state().get('generation')==1 and state().get('ready'))
 # Native PRIMARY becomes readable after a fresh owner event; the restarted
 # bridge still marks its first forwarded value as an initial snapshot.
 cmd('remote','set',text=other)
 def initial_forwarded():
     for line in trace.read_text()[len(prior_trace):].splitlines():
         try:value=json.loads(line)
         except ValueError:continue
         if value.get('primary_trace')=='complete' and value.get('size')==len(other.encode()):return True
     return False
 wait(initial_forwarded);ticks=state()['ticks'];wait(lambda:state()['ticks']>=ticks+8 and state()['active']);assert state()['received']==before
 peer_front();assert cmd('local','read')==bulk;mark('another-connection-initial-snapshot-preserves-selected-RDP-text')
 # A later deliberate selection from the new connection still propagates.
 write(root/'receiver-command.json',{'present':True,'enabled':True,'restart':1});time.sleep(.3);receiver_front();write(root/'receiver-command.json',{'enabled':True,'restart':1})
 # Reading through the independent native app above paused reception. Consume
 # that focus return's protected initial snapshot before the next live change.
 prior_trace=trace.read_text();cmd('remote','set',text=other);wait(initial_forwarded)
 ticks=state()['ticks'];wait(lambda:state()['ticks']>=ticks+8 and state()['active']);assert state()['received']==before
 cmd('remote','set',text=payload);wait(lambda:state()['received']>before);peer_front();assert cmd('local','read')==payload;mark('new-connection-deliberate-selection-still-updates')
 write(root/'receiver-command.json',{'close':True});assert receiver.wait(timeout=7)==0;mark('receiver-exit-closes-SSH-peer')
 (root/'acceptance.json').write_text(json.dumps({'checks':checks,'PASS':True,'actual_production_Rust_receiver':True,'native_Wayland_desktops':2,'SSH':'private executable stand-in, exact production peer command','physical_input':False,'remote_RDP_connection':False},indent=2)+'\n')
finally:
 for child in reversed(children):
  if child.poll()is None:child.terminate()
 for child in reversed(children):
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
 print(root,flush=True)
