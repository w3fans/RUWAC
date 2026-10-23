#!/usr/bin/env python3
"""Real private GNU Screen/tmux/RUWAC chains, without touching user sessions."""
import argparse,fcntl,hashlib,json,os,pty,select,shlex,shutil,signal,struct,subprocess,sys,tempfile,termios,threading,time,traceback
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--evidence',type=Path,required=True);p.add_argument('-E','--legacy-retained-ended-history',action='store_true',help='Expect the verified 0.4.13 baseline to retain its final ended session');a=p.parse_args()
binary=Path(a.binary).resolve();root=Path(tempfile.mkdtemp(prefix='mixed-nesting-',dir=a.evidence));root.chmod(0o700)
missing=[name for name in ['screen','tmux']if not shutil.which(name)]
if missing:
 (root/'receipt.json').write_text(json.dumps({'status':'BLOCKED','missing':missing,'scope':'No mixed nesting qualification'},indent=2)+'\n');raise SystemExit('Mixed nesting acceptance requires installed '+', '.join(missing))
env={**os.environ,'TERM':'xterm-256color','GSETTINGS_BACKEND':'memory','SHELL':'/bin/sh'}
for name in ['TMUX','STY','WINDOW','RUWAC_SHORTCUTS','RUWAC_KEYMAP']:env.pop(name,None)
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:env[key]=str(root/name);Path(env[key]).mkdir(mode=0o700)
screen_dir=root/'screens';screen_dir.mkdir(mode=0o700);env['SCREENDIR']=str(screen_dir)
screen_config=root/'screenrc';screen_config.write_text('startup_message off\ndefscrollback 2000\nvbell off\nautodetach on\nterm screen-256color\nescape ^Aa\n')
tmux_config=root/'tmux.conf';tmux_config.write_text('set -g default-shell /bin/sh\nset -g prefix C-a\nunbind C-b\nbind a send-prefix\nset -g status off\nset -g default-terminal screen-256color\nset -g exit-empty on\nset -g escape-time 10\nset -g set-clipboard off\nset -g mouse off\n')
default_tmux=root/'tmux-default.conf';default_tmux.write_text(tmux_config.read_text().replace('set -g prefix C-a\nunbind C-b\nbind a send-prefix','set -g prefix C-b\nbind C-b send-prefix'))
servers=[];layers=[];uis=[];results=[]
receiver=Path(__file__).resolve().parents[1]/'fixtures/receiver.py'
def run(argv):return subprocess.check_output(argv,env=env,text=True,stderr=subprocess.STDOUT,timeout=15)
def rpc(socket,method,params=None):
 with Client(socket)as c:return c.call(method,params)
class UI:
 def __init__(self,argv,name,prefix=b'\x01'):
  self.prefix=prefix
  self.master,self.slave=pty.openpty();self.old=termios.tcgetattr(self.slave);fcntl.ioctl(self.slave,termios.TIOCSWINSZ,struct.pack('HHHH',30,100,0,0))
  # Each client gets a controlling terminal, owned by this private PTY session.
  wrapper="import fcntl,os,sys,termios; fcntl.ioctl(0,termios.TIOCSCTTY,0); os.execvpe(sys.argv[1],sys.argv[1:],os.environ)"
  self.p=subprocess.Popen([sys.executable,'-c',wrapper,*argv],stdin=self.slave,stdout=self.slave,stderr=self.slave,start_new_session=True,env=env)
  self.data=bytearray();self.stop=False;self.name=name
  def drain():
   while not self.stop:
    if select.select([self.master],[],[],.05)[0]:
     try:b=os.read(self.master,65536)
     except OSError:break
     self.data.extend(b)
  self.thread=threading.Thread(target=drain,daemon=True);self.thread.start();uis.append(self)
 def send(self,b):os.write(self.master,b)
 def finish(self,detach=True):
  if self.p.poll()is None:
   if detach:self.send(self.prefix+b'd')
   else:self.p.terminate()
   self.p.wait(timeout=8)
  time.sleep(.05);self.stop=True;self.thread.join(timeout=1);(root/(self.name+'.pty')).write_bytes(self.data)
  restored=termios.tcgetattr(self.slave)==self.old
  os.close(self.master);os.close(self.slave);uis.remove(self)
  assert restored,'Client did not restore owned PTY modes';return self.p.returncode

def new_layer(kind,child,name,default=False,adapt=True):
 config=default_tmux if default else tmux_config
 directory=root/name;directory.mkdir(mode=0o700)
 # Clearing nesting hints is explicit for these separate fixture servers.
 child=['env','-u','TMUX','-u','STY',*child]
 if kind=='ruwac':
  sock=directory/'control.sock';proc=subprocess.Popen([str(binary),'--socket',str(sock),'serve'],env=env,stdout=(directory/'daemon.log').open('wb'),stderr=subprocess.STDOUT);servers.append((proc,sock));eventually(lambda:sock.exists()and rpc(sock,'health')['ok'])
  session=rpc(sock,'session.create',{'name':name,'argv':child,'rows':30,'cols':100});layer={'kind':kind,'socket':sock,'session':session};attach=[str(binary),'--socket',str(sock),'attach','--session',session['session']]
 elif kind=='tmux':
  sock=directory/'tmux.sock';run(['tmux','-S',str(sock),'-f',str(config),'new-session','-d','-s',name,'-x','100','-y','30',shlex.join(child)])
  layer={'kind':kind,'socket':sock,'name':name};attach=['tmux','-S',str(sock),'-f',str(config),'attach-session','-t',name]
 else:
  run(['screen','-c',str(screen_config),'-dmS',name,*child]);layer={'kind':kind,'name':name};attach=['screen','-c',str(screen_config),*(['-A']if adapt else []),'-r',name]
 layers.append(layer);return layer,attach

def pid_receipt():
 try:return json.loads(pid_file.read_text())
 except (OSError,ValueError):return None
# Preserve the standard raw receiver behavior while recording the actual leaf PID.
# Redraw after resize like an interactive TUI: detached Screen adaptation can
# discard a one-shot banner before the outside display attaches. Input receipts
# and every PID/resize/prefix assertion remain independent of this readiness line.
leaf=root/'leaf.py';leaf.write_text("import fcntl,json,os,runpy,signal,struct,sys,termios\nfrom pathlib import Path\nrecord=Path(sys.argv[2])\nidentity={'pid':os.getpid(),'start':Path('/proc/self/stat').read_text().split()[21]}\ndef resized(*_):\n prior=signal.pthread_sigmask(signal.SIG_BLOCK,{signal.SIGWINCH})\n try:\n  rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\\0'*8)); temp=record.with_name(record.name+'.tmp'); temp.write_text(json.dumps({**identity,'rows':rows,'cols':cols})); temp.replace(record)\n finally:signal.pthread_sigmask(signal.SIG_SETMASK,prior)\n if globals().get('ready',False):os.write(1,b'\\r\\nRUWAC_RECEIVER_READY\\r\\n')\nsignal.signal(signal.SIGWINCH,resized); resized(); ready=True\nsys.argv=[sys.argv[3],sys.argv[1]]\nrunpy.run_path(sys.argv[0],run_name='__main__')\n")
def prefix(kind,default):return b'\x02'if kind=='tmux'and default else b'\x01'
def routed(payload,pattern,default):
 for kind in reversed(pattern):
  key=prefix(kind,default);literal=key+ (key if kind=='tmux'and default else b'a')
  payload=payload.replace(key,literal)
 return payload
def identities_equal(before,after):return all(before[k]==after[k]for k in ['pid','start'])
def windows(layer):
 if layer['kind']=='ruwac':return len(rpc(layer['socket'],'window.list',{'session':layer['session']['session']}))
 if layer['kind']=='tmux':return len(run(['tmux','-S',str(layer['socket']),'list-windows','-t',layer['name'],'-F','#{window_id}']).splitlines())
 return len(run(['screen','-c',str(screen_config),'-S',layer['name'],'-Q','windows']).strip().split('  '))
def cleanup_owned_layers():
 # Each completed matrix case releases its servers before the next begins.
 # This keeps the matrix bounded rather than retaining sixteen detached chains.
 for ui in reversed(uis[:]):
  try:ui.finish(False)
  except Exception:
   if ui.p.poll()is None:ui.p.kill();ui.p.wait()
 for layer in reversed(layers):
  try:
   if layer['kind']=='screen':subprocess.run(['screen','-c',str(screen_config),'-S',layer['name'],'-X','quit'],env=env,capture_output=True,timeout=10)
   elif layer['kind']=='tmux':subprocess.run(['tmux','-S',str(layer['socket']),'kill-server'],env=env,capture_output=True,timeout=10)
  except (OSError,subprocess.SubprocessError):pass
 for proc,sock in reversed(servers):
  if proc.poll()is None:
   try:rpc(sock,'daemon.stop');proc.wait(timeout=5)
   except Exception:proc.kill();proc.wait()
 layers.clear();servers.clear()

patterns=[['ruwac','screen'],['tmux','ruwac'],['ruwac','screen','tmux'],['screen','ruwac','tmux'],['tmux','ruwac','screen','ruwac'],['screen','tmux','ruwac','screen'],['ruwac','screen','tmux','screen','ruwac'],['tmux','screen','ruwac','tmux','screen']]
try:
 for index,(default,pattern) in enumerate([(mode,pattern)for mode in [False,True]for pattern in patterns]):
  started=time.monotonic();name='chain-'+str(index);out=root/(name+'.bin');pid_file=root/(name+'.pid.json');pid_file.unlink(missing_ok=True)
  try:
   attach=[sys.executable,str(leaf),str(out),str(pid_file),str(receiver)]
   chain=[]
   for depth,kind in reversed(list(enumerate(pattern))):
    layer,attach=new_layer(kind,attach,f'n{index}l{depth}',default);chain.insert(0,layer)
   ui=UI(attach,name,prefix(pattern[0],default));eventually(lambda:out.exists()and b'RUWAC_RECEIVER_READY'in ui.data,20);identity=pid_receipt();assert identity
   payload='literal Živjo e\u0301 🧪 ; $HOME '.encode();ui.send(payload);eventually(lambda:out.read_bytes()==payload,10)
   ui.send(routed(b'\x01',pattern,default));eventually(lambda:out.read_bytes()==payload+b'\x01',10)
   ui.send(routed(b'\x02',pattern,default));eventually(lambda:out.read_bytes()==payload+b'\x01\x02',10)
   deep=chain[-1];key=prefix(pattern[-1],default)
   ui.send(routed(key+b'c',pattern[:-1],default));eventually(lambda:windows(deep)==2,10)
   ui.send(routed(key+b'0',pattern[:-1],default));time.sleep(.15)
   ui.send(b'/paste-on/');eventually(lambda:out.read_bytes().endswith(b'/paste-on/'),10);time.sleep(.4)
   before=out.read_bytes();paste='mixed paste Ž界 e\u0301 🧪\nsecond line'.encode()+(b'\x01d\x02d'if 'screen'not in pattern else b'');ui.send(b'\x1b[200~'+paste+b'\x1b[201~')
   expected=before+b'\x1b[200~'+paste+b'\x1b[201~';eventually(lambda:out.read_bytes()==expected,10);assert ui.p.poll()is None
   assert ui.finish()==0
   assert identities_equal(identity,pid_receipt());os.kill(identity['pid'],0)
   ui=UI(attach,name+'-reattach',prefix(pattern[0],default));eventually(lambda:b'RUWAC_RECEIVER_READY'in ui.data or b'SEEN:'in ui.data,20)
   ui.send(b'REATTACH');eventually(lambda:out.read_bytes()==expected+b'REATTACH',10);assert identities_equal(identity,pid_receipt())
   # An attach-time size signal can precede a nested legacy client's startup.
   # Record that retained size, then drive a real physical resize after input
   # has proved the full chain is active. Do not assume attach itself resized it.
   attach_size=pid_receipt()
   fcntl.ioctl(ui.slave,termios.TIOCSWINSZ,struct.pack('HHHH',30,101,0,0));os.kill(ui.p.pid,signal.SIGWINCH)
   eventually(lambda:(r if (r:=pid_receipt())and r['cols']==101 else None),10)
   fcntl.ioctl(ui.slave,termios.TIOCSWINSZ,struct.pack('HHHH',30,100,0,0));os.kill(ui.p.pid,signal.SIGWINCH)
   eventually(lambda:(r if (r:=pid_receipt())and r['cols']==100 else None),10);initial_size=pid_receipt()
   fcntl.ioctl(ui.slave,termios.TIOCSWINSZ,struct.pack('HHHH',42,137,0,0));os.kill(ui.p.pid,signal.SIGWINCH)
   size=eventually(lambda:(r if (r:=pid_receipt())and r['cols']==137 and r['rows']>initial_size['rows']else None),10)
   assert identities_equal(identity,size)and size['rows']==initial_size['rows']+12;assert ui.finish()==0
   results.append({'id':f'MIXED-NESTING-{index+1}','status':'PASS','layers':pattern,'depth':len(pattern),'seconds':time.monotonic()-started,'literal_utf8_and_prefix_exact':True,'single_bracketed_paste_pair':True,'paste_contains_both_control_prefixes': 'screen'not in pattern,'prefix_configuration':'tmux default Control-B'if default else 'common Control-A','GNU_Screen_display_adaptation_explicit':True,'deep_window_create_select':True,'initial_attach_leaf':[attach_size['rows'],attach_size['cols']],'explicit_resize_baseline':True,'leaf_resize_propagated':{'before':[initial_size['rows'],initial_size['cols']],'after':[size['rows'],size['cols']]},'same_leaf_pid_and_start_time_after_detach_reattach':True,'termios_restored':True,'receiver_sha256':hashlib.sha256(out.read_bytes()).hexdigest()});print(name,'PASS',pattern,'default tmux prefix'if default else 'common prefix',flush=True)
  except Exception as error:
   results.append({'id':f'MIXED-NESTING-{index+1}','status':'FAIL','layers':pattern,'error':str(error),'traceback':traceback.format_exc()});print(name,'FAIL',error,flush=True);raise
  finally:
   cleanup_owned_layers()
 # Default Screen reattachment may restore its retained window dimensions.
 # Test that policy separately from the explicit -A display-fit matrix above.
 retained=root/'default-retained.bin';pid_file=root/'default-retained.pid.json';pid_file.unlink(missing_ok=True)
 saved_screen,child=new_layer('screen',[sys.executable,str(leaf),str(retained),str(pid_file),str(receiver)],'retained-screen',adapt=False)
 saved_outer,attach=new_layer('ruwac',child,'retained-ruwac');ui=UI(attach,'default-retained')
 eventually(lambda:retained.exists()and b'RUWAC_RECEIVER_READY'in ui.data,20)
 run(['screen','-c',str(screen_config),'-S',saved_screen['name'],'-p','0','-X','width','-w','80','24'])
 before=eventually(lambda:r if (r:=pid_receipt())and (r['rows'],r['cols'])==(24,80)else None,10)
 assert ui.finish()==0
 ui=UI(attach,'default-retained-reattach');eventually(lambda:b'RUWAC_RECEIVER_READY'in ui.data,20)
 ui.send(b'DEFAULT_RESUME');eventually(lambda:retained.read_bytes()==b'DEFAULT_RESUME',10)
 restored=pid_receipt();assert identities_equal(before,restored)and(restored['rows'],restored['cols'])==(24,80)
 fcntl.ioctl(ui.slave,termios.TIOCSWINSZ,struct.pack('HHHH',42,137,0,0));os.kill(ui.p.pid,signal.SIGWINCH)
 changed=eventually(lambda:r if (r:=pid_receipt())and(r['rows'],r['cols'])==(41,137)else None,10)
 assert identities_equal(before,changed)and ui.finish()==0
 results.append({'id':'GNU-SCREEN-DEFAULT-RETAINED-SIZE','status':'PASS','explicitly_retained_window':[24,80],'reattached_physical_terminal':[30,100],'restored_leaf':[restored['rows'],restored['cols']],'actual_outer_resize_leaf':[changed['rows'],changed['cols']],'same_leaf_pid_and_start':True,'resumed_input_exact':True,'termios_restored':True})
 print('GNU-SCREEN-DEFAULT-RETAINED-SIZE PASS',flush=True)
 # Screen's input parser still treats its own prefix in terminal paste as commands.
 # Record this separately rather than weakening the ordinary byte-preservation tests.
 boundary=root/'screen-prefix-paste.bin';pid_file=root/'boundary.pid.json';pid_file.unlink(missing_ok=True)
 screen_layer,child=new_layer('screen',[sys.executable,str(leaf),str(boundary),str(pid_file),str(receiver)],'upstream-screen')
 layer,attach=new_layer('ruwac',child,'upstream-ruwac');ui=UI(attach,'upstream-prefix-paste')
 eventually(lambda:boundary.exists()and b'RUWAC_RECEIVER_READY'in ui.data,20)
 ui.send(b'/paste-on/');eventually(lambda:boundary.read_bytes()==b'/paste-on/');time.sleep(.4)
 ui.send(b'\x1b[200~BEFORE-PREFIX\x01dAFTER-PREFIX\x1b[201~')
 def boundary_ended():
  session_id=layer['session']['session']
  if not any(s['id']==session_id for s in rpc(layer['socket'],'session.list')):return 'retired'
  try:panes=rpc(layer['socket'],'pane.list',{'session':session_id})
  except AssertionError as error:
   response=error.args[0]if error.args else None
   if not isinstance(response,dict)or response.get('error',{}).get('code')!='STALE_ID':raise
   # The attached client can retire the final ended window between these two
   # RPCs. Confirm deletion rather than requiring a dead session to linger.
   return 'retired'if not any(s['id']==session_id for s in rpc(layer['socket'],'session.list'))else None
  return next((p for p in panes if p['id']==layer['session']['pane']and p['ended']),None)
 ended=eventually(boundary_ended,10)
 assert boundary.read_bytes()==b'/paste-on/\x1b[200~BEFORE-PREFIX'
 # Do not send detach while the client is retiring its completed window.
 assert ui.p.wait(timeout=10)==0;assert ui.finish()==0
 if a.legacy_retained_ended_history:
  # This known historical baseline closed its attached frontend but kept ended
  # history. Current candidates must continue to retire the completed session.
  assert run([str(binary),'--version']).strip()=='RUWAC 0.4.13'
  assert any(s['id']==layer['session']['session']for s in rpc(layer['socket'],'session.list'))
  assert any(p['id']==layer['session']['pane']and p['ended']for p in rpc(layer['socket'],'pane.list',{'session':layer['session']['session']}))
 else:
  eventually(lambda:not any(s['id']==layer['session']['session']for s in rpc(layer['socket'],'session.list')),10)
 results.append({'id':'GNU-SCREEN-PASTE-PREFIX-BOUNDARY','status':'LIMITATION','version':run(['screen','--version']).strip(),'actual_nested_Screen_detach':True,'RUWAC_outer_paste_remains_literal':True,'unescaped_Screen_prefix_in_paste_not_safe':True,'scope':'Observed upstream input interpretation; no broad arbitrary-control paste qualification'})
 print('GNU-SCREEN-PASTE-PREFIX-BOUNDARY LIMITATION',flush=True)

finally:
 cleanup_owned_layers()
 receipt={'results':results,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'versions':{'screen':run(['screen','--version']).strip(),'tmux':run(['tmux','-V']).strip()},'evidence':str(root),'scope':'Actual private local PTYs and owned GNU Screen/tmux/RUWAC servers; common Control-A and native tmux Control-B configurations; explicit upstream Screen paste-prefix limitation; no owner session, remote graphical or physical input claim'}
 (root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
assert len(results)==18 and sum(c['status']=='PASS'for c in results)==17 and results[-1]['status']=='LIMITATION'
