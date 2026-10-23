#!/usr/bin/env python3
"""Generated OpenSSH mux forward identity, loopback binding and parent-death cleanup.

The driver calls the production ssh_transport module; browser/HMR and real
remote-machine acceptance remain separate. No owner keys, routes or sessions.
"""
import argparse,http.server,json,os,pwd,select,shlex,socket,subprocess,tempfile,threading,time,urllib.request,shutil
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--driver',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args()
source_root=Path(__file__).resolve().parents[3]
if a.evidence.resolve()==source_root or source_root in a.evidence.resolve().parents:p.error('Evidence must be outside the source checkout')
if os.geteuid()==0:p.error('Use a non-root fixture account')
a.evidence.mkdir(mode=0o700,parents=True,exist_ok=True);root=Path(tempfile.mkdtemp(prefix='web-forward-',dir=a.evidence));root.chmod(0o700)
authroot=Path(tempfile.mkdtemp(prefix='ruwac-web-auth-',dir=f'/run/user/{os.getuid()}'));authroot.chmod(0o700)
env={k:v for k,v in os.environ.items()if k not in ['SSH_AUTH_SOCK','SSH_ASKPASS','SSH_ASKPASS_REQUIRE','RUWAC_WEB_FORWARD_WORKER']};env['HOME']=str(root)
children=[];logs=[];servers=[];controls=[];checks=[]
def run(argv,**kw):return subprocess.run(list(map(str,argv)),env=env,capture_output=True,timeout=8,**kw)
def start(argv,name,**kw):
 log=(root/(name+'.log')).open('wb');logs.append(log);q=subprocess.Popen(list(map(str,argv)),env=env,stderr=log,**kw);children.append(q);return q
def wait(test,timeout=6):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  if result:=test():return result
  time.sleep(.03)
 raise AssertionError('Generated forwarding condition timed out')
def listening(port):
 try:
  with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
 except OSError:return False
def closed(port):return not listening(port)
def mark(name):checks.append(name);print(name,'PASS',flush=True)
def request(port):return urllib.request.urlopen(f'http://127.0.0.1:{port}/',timeout=3).read()
def forward(token,pid,target,host='127.0.0.1',ok=True):
 child=start([a.driver,'open',token,pid,host,target],'forward-'+str(len(children)),stdin=subprocess.PIPE,stdout=subprocess.PIPE)
 assert select.select([child.stdout],[],[],8)[0], 'Forwarding helper did not answer'
 reply=json.loads(child.stdout.readline());assert reply['ok']==ok,reply
 if not ok:assert child.wait(timeout=5)!=0;return child,None
 assert reply['valid'];return child,reply['port']
try:
 for name in ['client','host-a','host-b']:
  q=run(['ssh-keygen','-q','-t','ed25519','-N','','-f',root/name]);assert q.returncode==0,q.stderr
  (root/name).chmod(0o600)
 auth=authroot/'authorized_keys';auth.write_bytes((root/'client.pub').read_bytes());auth.chmod(0o600)
 known=root/'known_hosts';known.touch(mode=0o600)
 endpoints=[]
 for label in ['a','b']:
  payload=('generated-origin-'+label).encode()
  class Handler(http.server.BaseHTTPRequestHandler):
   def do_GET(self,payload=payload):
    self.send_response(200);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
   def log_message(self,*_):pass
  server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start();servers.append((server,thread));target=server.server_address[1]
  with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  cfg=root/('sshd-'+label);cfg.write_text(f'ListenAddress 127.0.0.1\nPort {port}\nHostKey {root}/host-{label}\nPidFile {root}/sshd-{label}.pid\nAuthorizedKeysFile {auth}\nStrictModes yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowTcpForwarding local\nPermitOpen 127.0.0.1:{target}\nAllowUsers {pwd.getpwuid(os.getuid()).pw_name}\nLogLevel ERROR\n')
  child=start(['/usr/sbin/sshd','-D','-e','-f',cfg],'sshd-'+label,stdout=subprocess.DEVNULL);wait(lambda:listening(port));assert child.poll()is None
  key=(root/('host-'+label+'.pub')).read_text().split()
  with known.open('a')as f:f.write(f'[127.0.0.1]:{port} {key[0]} {key[1]}\n')
  endpoints.append((port,target,payload))
 config=root/'ssh_config'
 def route(index):
  config.write_text(f'Host generated-route\n HostName 127.0.0.1\n Port {endpoints[index][0]}\n User {pwd.getpwuid(os.getuid()).pw_name}\n IdentityFile {root}/client\n IdentitiesOnly yes\n UserKnownHostsFile {known}\n StrictHostKeyChecking yes\n BatchMode yes\n ConnectTimeout 3\n');config.chmod(0o600)
 masters=[]
 for index in range(2):
  route(index);reply=run([a.driver,'prepare'],input=json.dumps(['ssh','-F',str(config),'-N','generated-route']).encode());assert reply.returncode==0,reply.stderr
  reply=json.loads(reply.stdout);planned=reply['argv'];token=reply['token'];control=Path(next(v.removeprefix('ControlPath=')for v in planned if v.startswith('ControlPath=')));assert control.parent==Path(f'/run/user/{os.getuid()}/ruwac-ssh')and control.name==token;controls.append(control)
  master=start(planned,'master-'+str(index),stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL);wait(control.exists);masters.append((master,token))
 # Alias now points at B; only the already authenticated A master permits A's
 # service port. A fallback/re-resolved route cannot satisfy this request.
 route(1);master,token=masters[0];child,local=forward(token,master.pid,endpoints[0][1]);assert request(local)==endpoints[0][2]
 mark('changed-alias-cannot-redirect-original-authenticated-master-forward')
 binding=[line.split()[1]for line in Path('/proc/net/tcp').read_text().splitlines()[1:]if line.split()[1].endswith(':%04X'%local)]
 assert binding and all(v.startswith('0100007F:')for v in binding);mark('listener-is-bound-only-to-local-IPv4-loopback')
 child2,local2=forward(masters[1][1],masters[1][0].pid,endpoints[1][1]);assert request(local2)==endpoints[1][2]and request(local)==endpoints[0][2];mark('two-verified-original-masters-keep-their-generated-origins-isolated')
 child.stdin.close();child.wait(timeout=5);wait(lambda:closed(local));assert master.poll()is None;mark('normal-preview-close-removes-forward-and-preserves-original-master')
 child,local=forward(token,master.pid,endpoints[0][1]);assert request(local)==endpoints[0][2];child.kill();child.wait(timeout=5);wait(lambda:closed(local));assert master.poll()is None;mark('abrupt-frontend-death-closes-helper-pipe-and-removes-forward-with-master-still-live')
 forward(token,master.pid+1,endpoints[0][1],ok=False);mark('wrong-original-peer-PID-is-refused-before-opening-a-forward')
 forward(token,master.pid,endpoints[0][1],host='example.test',ok=False);mark('arbitrary-non-loopback-target-is-refused')
 master.terminate();master.wait(timeout=5);forward(token,master.pid,endpoints[0][1],ok=False);mark('dead-original-master-never-falls-back-to-a-new-SSH-route')
 (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'scope':'Production transport module and real generated loopback OpenSSH/HTTP servers; no native WebKit, HMR, physical input or external-host claim.'},indent=2)+'\n')
finally:
 for q in reversed(children):
  if q.poll()is None:
   if q.stdin:
    try:q.stdin.close()
    except OSError:pass
   q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
 for server,thread in servers:server.shutdown();server.server_close();thread.join(timeout=3)
 for control in controls:
  if control.exists():control.unlink()
 for log in logs:log.close()
 shutil.rmtree(authroot)
 print(root,flush=True)
