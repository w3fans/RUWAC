#!/usr/bin/env python3
"""Generated SSH localhost HTTPS service; explicit ephemeral certificate review."""
import argparse,base64,hashlib,http.server,json,os,pwd,shlex,shutil,socket,ssl,subprocess,tempfile,threading,time,uuid
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,type=Path,required=True)
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=Path(tempfile.mkdtemp(prefix='web-tls-',dir=a.evidence));r.chmod(0o700);children=[];seq=0;checks=[];server=None;authroot=None
E={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','RUWAC_GUI_ENTRY':str(a.gui)}
for k,n in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 E[k]=str(r/n);Path(E[k]).mkdir(mode=0o700)
for k in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY','RUWAC_STATE_FILE']:E.pop(k,None)
sock=r/'control.sock'
def start(args,name,env=None):
 with(r/(name+'.log')).open('w')as f:q=subprocess.Popen(list(map(str,args)),env=env or E,stdout=f,stderr=subprocess.STDOUT)
 children.append(q);return q
def rpc(method,params=None):
 with Client(sock)as c:return c.call(method,params)
def cmd(action,**kw):
 global seq
 seq+=1;t=r/'request.tmp';t.write_text(json.dumps(dict(seq=seq,action=action,**kw)));t.replace(r/'gui-request.json')
 def read():
  try:q=json.loads((r/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(read,15)
def web(**kw):return cmd('web-preview',**kw)
def js(script,index=0):
 web(operation='script',script=script,index=index)
 return eventually(lambda:(q['script_result']if(q:=web(index=index)).get('script_result')is not None else None),15)
def mark(s):checks.append(s);print(s,'PASS',flush=True)
class Handler(http.server.BaseHTTPRequestHandler):
 protocol_version='HTTP/1.1'
 def do_GET(self):
  self.server.requests.append(self.path);self.server.handshakes.append(dict(path=self.path,reused=self.connection.session_reused,version=self.connection.version()))
  if self.path.startswith('/pair?'):
   self.send_response(302);self.send_header('Set-Cookie','generated_session=paired; Secure; HttpOnly; SameSite=Strict; Path=/');self.send_header('Location','/app');self.send_header('Content-Length','0');self.end_headers();return
  if self.headers.get('Upgrade','').lower()=='websocket':
   key=self.headers['Sec-WebSocket-Key'];accept=base64.b64encode(hashlib.sha1((key+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
   self.send_response(101);self.send_header('Upgrade','websocket');self.send_header('Connection','Upgrade');self.send_header('Sec-WebSocket-Accept',accept);self.end_headers();self.wfile.flush();self.connection.sendall(b'\x81\x0bremote-live');self.connection.settimeout(.2)
   try:
    while not self.server.stopping:
     try:
      if not self.connection.recv(4096):break
     except socket.timeout:pass
   except OSError:pass
   return
  if self.path=='/asset.js':body=b'window.assetLoaded=true;let w=new WebSocket("wss://"+location.host+"/ws");w.onmessage=e=>{window.live=e.data;fetch("/live-ack?value="+encodeURIComponent(e.data))};'
  else:
   self.server.session_seen='generated_session=paired'in self.headers.get('Cookie','');body=b'<html><title>Remote localhost fixture</title><body>remote app<script src="/asset.js"></script></body></html>'
  self.send_response(200);self.send_header('Content-Type','text/javascript'if self.path=='/asset.js'else'text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
 def log_message(self,*args):pass
try:
 subprocess.run(['openssl','req','-x509','-newkey','ec','-pkeyopt','ec_paramgen_curve:P-256','-nodes','-keyout',str(r/'key.pem'),'-out',str(r/'cert.pem'),'-days','1','-subj','/CN=localhost','-addext','subjectAltName=DNS:localhost,IP:127.0.0.1'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.requests=[];server.handshakes=[];server.stopping=False;server.session_seen=False
 tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(r/'cert.pem',r/'key.pem');server.socket=tls.wrap_socket(server.socket,server_side=True);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start();port=server.server_address[1]
 for name in ['client','host']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(r/name)],check=True,env=E)
 authroot=Path(tempfile.mkdtemp(prefix='ruwac-tls-auth-',dir=f'/run/user/{os.getuid()}'));authroot.chmod(0o700);auth=authroot/'authorized_keys';auth.write_bytes((r/'client.pub').read_bytes());auth.chmod(0o600)
 with socket.socket()as q:q.bind(('127.0.0.1',0));sshport=q.getsockname()[1]
 forced=r/'remote-shell';forced.write_text('#!/bin/sh\nprintf READY_REMOTE\\\\n\nexec /bin/cat\n');forced.chmod(0o700)
 cfg=r/'sshd.conf';cfg.write_text(f'ListenAddress 127.0.0.1\nPort {sshport}\nHostKey {r}/host\nPidFile {r}/sshd.pid\nAuthorizedKeysFile {auth}\nForceCommand {forced}\nStrictModes yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowTcpForwarding local\nPermitOpen 127.0.0.1:{port}\nAllowUsers {pwd.getpwuid(os.getuid()).pw_name}\nLogLevel ERROR\n')
 known=r/'known_hosts';parts=(r/'host.pub').read_text().split();known.write_text(f'[127.0.0.1]:{sshport} {parts[0]} {parts[1]}\n');known.chmod(0o600)
 config=r/'ssh_config';config.write_text('');bin=r/'bin';bin.mkdir();ssh=bin/'ssh';ssh.write_text('#!/usr/bin/python3\nimport os,sys\nos.execv("/usr/bin/ssh",["ssh","-F",'+repr(str(config))+']+sys.argv[1:])\n');ssh.chmod(0o700);E['PATH']=str(bin)+os.pathsep+E['PATH']
 start(['/usr/sbin/sshd','-D','-e','-f',cfg],'sshd')
 def ready():
  try:
   with socket.create_connection(('127.0.0.1',sshport),timeout=.1):return True
  except OSError:return False
 eventually(ready)
 profile=dict(id='endpoint_'+uuid.uuid4().hex,machine_id='machine_'+uuid.uuid4().hex,name='Generated TLS remote',group='Generated',protocol='SSH',server=f'127.0.0.1:{sshport}',username=pwd.getpwuid(os.getuid()).pw_name,secret_reference='',favorite=False,last_used=0,tags=[],preferences=dict(identity_file=str(r/'client'),known_hosts=str(known),agent_backend='direct'))
 library=r/'config/RUWAC/machines.json';library.parent.mkdir(mode=0o700);library.write_text(json.dumps(dict(schema=1,items=[profile],groups=[])));library.chmod(0o600)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists);row=rpc('session.create',dict(name='local',argv=['/bin/bash','--noprofile','--norc']))
 gui=start([a.gui,'--new-instance','--socket',sock,'--session',row['session'],'--fixture-dir',r],'gui');eventually(lambda:cmd('inspect').get('target'),20)
 cmd('profiles',search='Generated TLS remote',open=True);context=eventually(lambda:next(iter(cmd('launch-contexts')['entries']),None),20);rpc('pane.wait',dict(pane=context['pane'],contains='READY_REMOTE',timeout_ms=10000));pid=next(p['pid']for p in rpc('pane.list',dict(session=context['session']))if p['id']==context['pane'])
 path='/pair?ticket=generated-pairing-value'
 web(operation='remote-service',pid=pid,fields={'remote-service-port':str(port),'remote-service-path':path,'remote-service-https':True},respond='open')
 eventually(lambda:web().get('remote')is True,20);eventually(lambda:not web().get('loading',True),20);time.sleep(.3)
 state=web();assert state.get('error_visible')and state.get('failed')and'Certificate'in state['status'],'TLS_FAILURE_NOT_VISIBLE: '+str(state)
 assert not server.requests and 'generated-pairing-value'not in state.get('failure_detail','');mark('TLS-failure-visible-after-finished-no-HTTP-request-or-ticket-in-error')
 assert state['certificate_review_available'];web(operation='certificate-review');web(operation='certificate-response',accept=False);assert not server.requests;mark('cancel-does-not-trust-or-consume-pairing-request')
 web(operation='certificate-review');web(operation='certificate-response',accept=True);eventually(lambda:not web().get('loading',True),20);print('After approval:',json.dumps(web()),'HTTP paths:',json.dumps(server.requests),flush=True);eventually(lambda:'/live-ack?value=remote-live'in server.requests,20)
 assert server.session_seen and server.requests.count(path)==1;assert not web()['error_visible'];mark('explicit-approval-loads-app-assets-Secure-cookie-and-WSS-through-original-SSH')
 print('Second service:',json.dumps(web(operation='remote-service',pid=pid,fields={'remote-service-port':str(port),'remote-service-path':'/app','remote-service-https':True},respond='open')),flush=True);time.sleep(2);print('Second preview:',json.dumps(web(index=1)),'TLS requests:',json.dumps(server.handshakes),flush=True);eventually(lambda:web(index=1).get('error_visible'),20);mark('new-preview-does-not-inherit-certificate-approval')
 before=len(server.requests);web(operation='certificate-review',index=1);web(operation='address',index=1,text=f'https://localhost:{port}/changed');web(operation='certificate-response',accept=True,index=1);time.sleep(.3);assert len(server.requests)==before;mark('changed-address-refuses-stale-certificate-approval')
 cmd('focus-view',view=1);cmd('host-shortcuts',click='new-web-preview');q=web(operation='remote-service',existing=True,pid=pid,respond='cancel');assert not q.get('open')and'Generated TLS remote'in q['dialog']['remote-service-summary'];mark('main-preview-action-targets-selected-verified-terminal')
 # A queued approval must not outlive its original preview.
 web(operation='certificate-review',index=1);web(operation='focus',index=1);cmd('close-tab',tab=cmd('inspect')['selected_tab']);eventually(lambda:web()['count']==1,10)
 before=len(server.requests);web(operation='certificate-response',index=0,accept=True);time.sleep(.3);assert len(server.requests)==before and web()['remote'];mark('closed-preview-refuses-queued-approval-and-preserves-other-preview')
 # Saved identity changes invalidate pending approval without replacing SSH.
 web(operation='remote-service',pid=pid,fields={'remote-service-port':str(port),'remote-service-path':'/app','remote-service-https':True},respond='open');eventually(lambda:web(index=1).get('error_visible'),20);web(operation='certificate-review',index=1)
 before=len(server.requests);profile['server']='127.0.0.1:1';library.write_text(json.dumps(dict(schema=1,items=[profile],groups=[])));library.chmod(0o600)
 web(operation='certificate-response',index=1,accept=True);eventually(lambda:not web(index=1)['remote'],15);time.sleep(.3);assert len(server.requests)==before
 assert next(p['pid']for p in rpc('pane.list',dict(session=context['session']))if p['id']==context['pane'])==pid;mark('changed-SSH-identity-refuses-pending-approval-without-replacing-PTY')
 (a.evidence/'acceptance.json').write_text(json.dumps(dict(PASS=True,checks=checks,scope='Owned loopback HTTPS server through generated pinned OpenSSH and real WebKit. No external host, owner session or physical hardware claim.'),indent=2)+'\n')
finally:
 if sock.exists():
  try:rpc('daemon.stop')
  except Exception:pass
 for child in reversed(children):
  if child.poll()is None:child.terminate()
  try:child.wait(timeout=8)
  except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
 if server:server.stopping=True;server.shutdown();server.server_close()
 if authroot:shutil.rmtree(authroot)
