#!/usr/bin/env python3
"""Native Ctrl-click, pinned OpenSSH and WebKit/WebSocket preview fixtures.

Two generated SSH identities translate only their common advertised loopback
port through a test-only server preload library. They are not separate machines.
No owner clipboard, saved account, shell profile or endpoint is changed.
"""
import argparse,http.server,json,os,subprocess,sys,tempfile,threading,time,base64,hashlib,pwd,shlex,shutil,socket,uuid
from gi.repository import Gio,GLib
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,required=True)
p.add_argument('--translation-lib',required=True);a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
root=Path(tempfile.mkdtemp(prefix='web-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2','RUWAC_GUI_ENTRY':a.gui}
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 d=root/name;d.mkdir(mode=0o700);env[key]=str(d)
for key in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(key,None)
children=[];seq=0;checks=[];sock=root/'control.sock'
servers=[];authroot=None;controls=[];session=None
reserved=socket.socket();reserved.bind(('127.0.0.1',0));logical_port=reserved.getsockname()[1]
url=f'http://localhost:{logical_port}/redirect'
class Handler(http.server.BaseHTTPRequestHandler):
 protocol_version='HTTP/1.1'
 def do_GET(self):
  self.server.requests.append({'path':self.path,'upgrade':self.headers.get('Upgrade','')})
  if self.path=='/redirect':
   self.send_response(302);self.send_header('Location',f'http://localhost:{logical_port}/');self.send_header('Content-Length','0');self.end_headers();return
  if self.headers.get('Upgrade','').lower()=='websocket':
   key=self.headers['Sec-WebSocket-Key'];accept=base64.b64encode(hashlib.sha1((key+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
   self.send_response(101);self.send_header('Upgrade','websocket');self.send_header('Connection','Upgrade');self.send_header('Sec-WebSocket-Accept',accept);self.end_headers();self.wfile.flush()
   self.connection.settimeout(.2);self.server.clients.add(self.connection)
   try:
    self.connection.sendall(b'\x81'+bytes([len('ready-'+self.server.label)])+('ready-'+self.server.label).encode())
    while not self.server.stopping:
     try:
      data=self.connection.recv(4096)
      if not data or data[0]&15==8:break
     except socket.timeout:pass
   finally:self.server.clients.discard(self.connection)
   return
  tag=json.dumps(self.server.label)
  page=('<html><title>Generated remote '+self.server.label+'</title><body><button id="counter" onclick="this.textContent=++window.count">0</button><script>window.count=0;window.hostTag='+tag+';window.hmrText="pending";let ws=new WebSocket("ws://"+location.host+"/ws");ws.onmessage=e=>window.hmrText=e.data;</script></body></html>').encode()
  self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(page)));self.end_headers();self.wfile.write(page)
 def log_message(self,*_):pass
def start(args,name,overrides=None):
 with (root/(name+'.log')).open('wb')as log:q=subprocess.Popen(list(map(str,args)),env={**env,**(overrides or {})},stdout=log,stderr=subprocess.STDOUT)
 children.append(q);return q
def rpc(method,params=None):
 with Client(sock)as c:return c.call(method,params)
def command(action,**kw):
 global seq
 seq+=1;q=root/'request.tmp';q.write_text(json.dumps(dict(seq=seq,action=action,**kw)));q.replace(root/'gui-request.json')
 def result():
  try:
   d=json.loads((root/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(result,15)
def js(script,index=0):
 command('web-preview',operation='script',script=script,index=index)
 return eventually(lambda:(d['script_result']if (d:=command('web-preview',index=index)).get('script_result')is not None else None),15)
def mark(name):checks.append(name);print(name,'PASS',flush=True)

bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop'
def seat(method,args=None):return bus.call_sync(service,session,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
def key(code,down):seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,down)));time.sleep(.04)
def source_link(view):
 command('focus-view',view=view)
 def shown():
  q=command('history-viewport',view=view)
  for row,cells in enumerate(q.get('cells')or[]):
   text=''.join(c[0]for c in cells)
   if url in text:return row,text.index(url)
  return None
 row,col=eventually(shown,15);info=command('inspect',view=view);ox,oy=info['area_origin'];cw,ch=info['cell_metrics']
 seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(ox+(col+6.5)*cw,oy+(row+.5)*ch)));time.sleep(.15)
 key(29,True);seat('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));time.sleep(.08);seat('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));key(29,False)
def closed(port):
 try:
  with socket.create_connection(('127.0.0.1',port),timeout=.1):return False
 except OSError:return True
def host(index):return js('String(window.hostTag||"pending")',index)
try:
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 authroot=Path(tempfile.mkdtemp(prefix='ruwac-web-auth-',dir=f'/run/user/{os.getuid()}'));authroot.chmod(0o700)
 for name in ['client','host-a','host-b']:
  subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/name)],env=env,check=True);(root/name).chmod(0o600)
 auth=authroot/'authorized_keys';auth.write_bytes((root/'client.pub').read_bytes());auth.chmod(0o600)
 known=root/'known_hosts';known.touch(mode=0o600);profiles=[]
 wrappers=root/'bin';wrappers.mkdir();config=root/'ssh_config';config.write_text('');config.chmod(0o600)
 wrapper=wrappers/'ssh';wrapper.write_text('#!/usr/bin/python3\nimport os,sys\nos.execv("/usr/bin/ssh",["ssh","-F",'+repr(str(config))+']+sys.argv[1:])\n');wrapper.chmod(0o700);env['PATH']=str(wrappers)+os.pathsep+env['PATH']
 remote=root/'remote-home';remote.mkdir(mode=0o700)
 forced=root/'remote-shell';forced.write_text('#!/bin/sh\nexport HOME='+shlex.quote(str(remote))+'\nprintf '+shlex.quote(url+'\r\n')+'\nexec /bin/cat\n');forced.chmod(0o700)
 for label in ['a','b']:
  server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.label=label;server.requests=[];server.clients=set();server.stopping=False;thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start();servers.append((server,thread))
  with socket.socket()as q:q.bind(('127.0.0.1',0));port=q.getsockname()[1]
  cfg=root/('sshd-'+label+'.conf');cfg.write_text(f'ListenAddress 127.0.0.1\nPort {port}\nHostKey {root}/host-{label}\nPidFile {root}/sshd-{label}.pid\nAuthorizedKeysFile {auth}\nSetEnv HOME={remote}\nForceCommand {forced}\nStrictModes yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowTcpForwarding local\nPermitOpen 127.0.0.1:{logical_port}\nAllowUsers {pwd.getpwuid(os.getuid()).pw_name}\nLogLevel ERROR\n')
  daemon=start(['/usr/sbin/sshd','-D','-e','-f',cfg],'sshd-'+label,{'LD_PRELOAD':str(Path(a.translation_lib).resolve()),'RUWAC_FIXTURE_HTTP_PORT':str(server.server_address[1]),'RUWAC_FIXTURE_REQUEST_PORT':str(logical_port)})
  def ready():
   assert daemon.poll()is None
   return not closed(port)
  eventually(ready)
  parts=(root/('host-'+label+'.pub')).read_text().split()
  with known.open('a')as f:f.write(f'[127.0.0.1]:{port} {parts[0]} {parts[1]}\n')
  profiles.append({'id':'endpoint_'+uuid.uuid4().hex,'machine_id':'machine_'+uuid.uuid4().hex,'name':'Generated Web '+label,'group':'Generated','protocol':'SSH','server':f'127.0.0.1:{port}','username':pwd.getpwuid(os.getuid()).pw_name,'secret_reference':'generated-unused','favorite':False,'last_used':0,'tags':[],'preferences':{'identity_file':str(root/'client'),'known_hosts':str(known),'agent_backend':'direct'}})
 library=root/'config/RUWAC/machines.json';library.parent.mkdir(mode=0o700);library.write_text(json.dumps({'schema':1,'items':profiles,'groups':[]}));library.chmod(0o600)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists);row=rpc('session.create',{'name':'web-local-fixture','argv':['/bin/bash','--noprofile','--norc']})
 gui=start([a.gui,'--new-instance','--socket',sock,'--session',row['session'],'--allow-workspace-control','--fixture-dir',root],'GUI');eventually(lambda:command('inspect')['tabs']==1,15)
 contexts=[]
 for index,label in enumerate(['a','b']):
  previous={v['session']for v in command('launch-contexts')['entries']}
  command('profiles',search='Generated Web '+label,open=True)
  def context():
   return next((v for v in command('launch-contexts')['entries']if v['session']not in previous),None)
  context=eventually(context,15);contexts.append(context)
  rpc('pane.wait',{'pane':context['pane'],'contains':url,'timeout_ms':10000})
  source_link(index+1)
  eventually(lambda:command('web-preview',index=index).get('remote')is True,20)
  eventually(lambda:host(index)==label,20)
  mark('native-terminal-Ctrl-click-opens-original-SSH-Web-origin-'+label)
 for _ in range(8):command('tab-select',tab=2);command('tab-select',tab=4)
 assert host(0)=='a'and host(1)=='b';mark('same-advertised-loopback-port-keeps-two-authenticated-SSH-identities-isolated')
 for index,(server,_)in enumerate(servers):
  eventually(lambda:bool(server.clients),10);payload=('updated-'+server.label).encode()
  for stream in list(server.clients):stream.sendall(b'\x81'+bytes([len(payload)])+payload)
  eventually(lambda:js('String(window.hmrText)',index)=='updated-'+server.label,10)
 mark('real-WebSocket-update-arrives-through-each-original-SSH-local-forward')
 assert all(command('web-preview',index=i)['layout']['endpoint']==profiles[i]['id']for i in range(2));mark('absolute-localhost-redirects-preserve-approved-remote-origin')
 js('document.cookie="fixture_cookie=a; SameSite=Strict";true',0)
 assert js('document.cookie === ""',1)is True;mark('separate-remote-previews-do-not-share-cookies')

 from urllib.parse import urlsplit
 restore_ports=[urlsplit(command('web-preview',index=i)['origin']).port for i in range(2)]
 command('workspace-close');gui.wait(timeout=10)
 assert all(closed(port)for port in restore_ports)
 for context in contexts:assert Path('/proc',str(next(p for p in rpc('pane.list',{'session':context['session']})if p['id']==context['pane'])['pid'])).exists()
 for n in ['gui-request.json','gui-response.json']:(root/n).unlink(missing_ok=True)
 seq=0
 gui=start([a.gui,'--new-instance','--socket',sock,'--allow-workspace-control','--fixture-dir',root],'GUI-restored')
 eventually(lambda:command('web-preview').get('count')==2,20)
 assert all(not command('web-preview',index=i)['remote']and not command('web-preview',index=i)['origin']for i in range(2))
 mark('restored-Web-layout-does-not-open-a-forward-or-load-a-remote-address')
 for i,context in enumerate(contexts):
  command('focus-view',view=i+1);eventually(lambda:command('inspect',view=i+1).get('target')==context['pane'],15)
  pid=next(p for p in rpc('pane.list',{'session':context['session']})if p['id']==context['pane'])['pid']
  command('web-preview',operation='address',text=url,index=i);command('web-preview',operation='choose-source',index=i);chosen=command('web-preview',operation='choose-terminal',pid=pid,index=i);assert 'error'not in chosen,chosen
  eventually(lambda:command('web-preview',index=i)['remote']and host(i)==['a','b'][i],20)
 assert js('document.cookie === ""',0)is True
 mark('explicit-original-SSH-chooser-reconnects-restored-preview-with-fresh-ephemeral-storage')
 first_port=urlsplit(command('web-preview',index=0)['origin']).port
 profiles[0]['server']=profiles[1]['server'];library.write_text(json.dumps({'schema':1,'items':profiles,'groups':[]}));library.chmod(0o600)
 eventually(lambda:not command('web-preview',index=0)['remote']and closed(first_port),15)
 eventually(lambda:not servers[0][0].clients,10)
 trap=http.server.ThreadingHTTPServer(('127.0.0.1',first_port),Handler);trap.label='trap';trap.requests=[];trap.clients=set();trap.stopping=False;trap_thread=threading.Thread(target=trap.serve_forever,daemon=True);trap_thread.start();servers.append((trap,trap_thread))
 command('web-preview',operation='control',name='web-reload',index=0);time.sleep(.5)
 assert not trap.requests,'Disconnected preview reloaded an old forwarded port'
 assert host(1)=='b';mark('changed-SSH-profile-closes-forward-WebSocket-and-blocks-stale-port-reload')
 previous={v['session']for v in command('launch-contexts')['entries']};command('profiles',search='Generated Web a',open=True)
 newcontext=eventually(lambda:next((v for v in command('launch-contexts')['entries']if v['session']not in previous),None),15)
 rpc('pane.wait',{'pane':newcontext['pane'],'contains':url,'timeout_ms':10000});pid=next(p for p in rpc('pane.list',{'session':newcontext['session']})if p['id']==newcontext['pane'])['pid']
 command('web-preview',operation='address',text=url,index=0);command('web-preview',operation='choose-source',index=0);command('web-preview',operation='choose-terminal',pid=pid,index=0)
 assert not command('web-preview',index=0)['remote']and 'Connection details changed'in command('web-preview',index=0)['status']
 assert not trap.requests;mark('reusing-ephemeral-browser-for-changed-SSH-identity-is-refused')

 command('web-preview',operation='focus',index=0);command('close-tab',tab=command('inspect')['selected_tab']);eventually(lambda:command('web-preview')['count']==1,10)
 for context in contexts:
  assert next(p for p in rpc('pane.list',{'session':context['session']})if p['id']==context['pane'])['pid']>0
 assert host(0)=='b';mark('closing-one-preview-cancels-only-its-forward-and-keeps-both-SSH-PTYs')
 second_port=urlsplit(command('web-preview')['origin']).port;rpc('pane.close',{'pane':contexts[1]['pane']})
 eventually(lambda:not command('web-preview')['remote']and closed(second_port),10);mark('ended-original-SSH-pane-disconnects-preview-without-reauthentication-or-fallback')
 command('workspace-close');gui.wait(timeout=10)
 (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'scope':'Actual GDK Ctrl-click, real OpenSSH identities and WebKit/HTTP/WebSocket; two same-host generated SSH servers use test-only process-local loopback-port translation. No separate-machine, physical-input or external-host claim.'},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 if sock.exists():
  try:rpc('daemon.stop')
  except Exception:pass
 for child in reversed(children):
  if child.poll()is None:child.terminate()
 for child in reversed(children):
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
 (root/'http-diagnostics.json').write_text(json.dumps([{'label':s.label,'requests':s.requests,'websocket_clients':len(s.clients)}for s,_ in servers],indent=2)+'\n')
 for server,thread in servers:
  server.stopping=True
  for stream in list(server.clients):
   try:stream.shutdown(socket.SHUT_RDWR);stream.close()
   except OSError:pass
  server.shutdown();server.server_close();thread.join(timeout=3)
 if authroot:shutil.rmtree(authroot)
 # Only generated transport identities from this fixture may be removed.
 for p in (root/'config/RUWAC/launches').rglob('*.json')if (root/'config/RUWAC/launches').exists()else []:
  try:
   obj=json.loads(p.read_text());token=obj.get('transport')or'';candidate=Path(f'/run/user/{os.getuid()}/ruwac-ssh')/token
   if len(token)==42 and token.startswith('transport_')and candidate.is_socket():candidate.unlink()
  except(OSError,ValueError):pass
 reserved.close();print(root,flush=True)
