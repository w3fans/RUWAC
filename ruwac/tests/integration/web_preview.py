#!/usr/bin/env python3
"""Actual WebKit HTML/JS/ephemeral-session lifecycle, using private fixture actions.
DOM activation is programmatic; this does not claim physical keyboard, HMR or SSH.
"""
import argparse,http.server,json,os,subprocess,sys,tempfile,threading,time
from pathlib import Path
from gi.repository import Gio,GLib
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,required=True)
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
root=Path(tempfile.mkdtemp(prefix='web-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','LP_NUM_THREADS':'2','TOKIO_WORKER_THREADS':'2','RUWAC_GUI_ENTRY':a.gui}
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 d=root/name;d.mkdir(mode=0o700);env[key]=str(d)
for key in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(key,None)
children=[];seq=0;checks=[];session=None;sock=root/'control.sock'
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  page=b'<html><title>Generated fixture</title><body><button id="counter" onclick="this.textContent=++window.count">0</button><textarea id="text"></textarea><script>window.count=0;window.fixtureReady=true</script></body></html>'
  self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(page)));self.end_headers();self.wfile.write(page)
 def log_message(self,*_):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
def start(args,name):
 with (root/(name+'.log')).open('wb')as log:q=subprocess.Popen(list(map(str,args)),env=env,stdout=log,stderr=subprocess.STDOUT)
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
def chord(code):
 key(29,True);key(code,True);key(code,False);key(29,False);time.sleep(.15)
def move(x,y):
 seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x),float(y))));time.sleep(.1)
def button(down):seat('NotifyPointerButton',GLib.Variant('(ib)',(272,down)));time.sleep(.08)
receiver=root/'receiver.py';output=root/'terminal-input'
receiver.write_text('import os,sys,tty\ntty.setraw(0)\nos.write(1,b"TERMINAL_COPY_TEXT\\r\\nREADY\\r\\n\\x1b[?2004h")\nf=open(sys.argv[1],"ab",buffering=0)\nwhile True:\n b=os.read(0,4096)\n if not b:break\n f.write(b)\n')
try:
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];seat('Start');seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)));key(194,True);key(194,False)
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 row=rpc('session.create',{'name':'web-fixture','argv':[sys.executable,str(receiver),str(output)]});pid=rpc('pane.list',{'session':row['session']})[0]['pid']
 gui=start([a.gui,'--new-instance','--socket',sock,'--session',row['session'],'--allow-workspace-control','--fixture-dir',root],'GUI')
 eventually(lambda:command('inspect')['tabs']==1,15)
 state=command('web-preview');assert state.get('count')==0,'First-class Web fixture unavailable or browser allocated before use'
 mark('terminal-only-start-has-no-Web-view')
 port=server.server_address[1];url=f'http://127.0.0.1:{port}/?code=generated-persist-marker#generated-fragment'
 state=command('web-preview',operation='open',url=url);assert 'error'not in state,state
 eventually(lambda:not command('web-preview')['loading'],25)
 assert js('window.fixtureReady === true')is True
 assert js('document.getElementById("counter").click(); window.count')==1
 mark('real-WebKit-loads-served-HTML-and-executes-interactive-DOM-JavaScript')
 command('web-preview',operation='focus')
 rect=js('let r=document.getElementById("counter").getBoundingClientRect(); [r.x+r.width/2,r.y+r.height/2]');origin=command('web-preview')['browser_origin'];move(origin[0]+rect[0],origin[1]+rect[1]);button(True);button(False)
 eventually(lambda:js('window.count')==2);mark('native-browser-pointer-activates-served-button')
 command('focus-view',view=0)
 def source():
  state=command('history-viewport',view=0)
  for row,cells in enumerate(state.get('cells')or[]):
   text=''.join(c[0]for c in cells)
   if 'TERMINAL_COPY_TEXT'in text:return row,text.index('TERMINAL_COPY_TEXT')
  return None
 rown,col=eventually(source);info=command('inspect',view=0);ox,oy=info['area_origin'];cw,ch=info['cell_metrics'];move(ox+(col+.3)*cw,oy+(rown+.5)*ch);button(True)
 for _ in range(6):seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(15*cw/6,0.)));time.sleep(.035)
 button(False);text=command('copy-state',view=0)['selection'];assert text and 'TERMINAL'in text,text
 chord(46);assert not output.exists()or not output.read_bytes(),'Browser copy leaked ETX into PTY'
 command('web-preview',operation='focus');js('document.getElementById("text").focus(); true');chord(47)
 eventually(lambda:js('document.getElementById("text").value')==text);mark('native-Ctrl-V-pastes-terminal-selection-into-browser')
 browser_text='BROWSER_CLIPBOARD_žółć'
 js('let t=document.getElementById("text");t.value='+json.dumps(browser_text)+';t.focus();t.select();true');chord(46)
 command('focus-view',view=0);chord(47);expected=b'\x1b[200~'+browser_text.encode()+b'\x1b[201~';eventually(lambda:output.exists()and output.read_bytes()==expected);mark('native-browser-Ctrl-C-to-terminal-Ctrl-V-is-one-bracketed-payload')
 command('web-preview',operation='focus');chord(38);assert command('web-preview')['address_focused'];key(58,True);key(58,False);command('web-preview',operation='focus');chord(38);assert command('web-preview')['address_focused'];key(58,True);key(58,False);assert output.read_bytes()==expected;mark('Ctrl-L-with-Caps-Lock-focuses-address-without-terminal-input')
 command('web-preview',operation='focus');joined=command('mixed-join',tab=0);assert 'error'not in joined,joined
 assert rpc('pane.list',{'session':row['session']})[0]['pid']==pid
 command('web-preview',operation='focus');separated=command('mixed-separate');assert 'error'not in separated,separated
 assert js('window.count')==2;mark('native-Web-terminal-mixed-split-and-separate-preserve-document-and-PTY')

 state=command('web-preview');assert state['ephemeral']is True and state['layout']['kind']=='web'and not state['remote'],state
 assert set(state['layout'])<=set(['kind','view_id','endpoint']),state['layout'];mark('ephemeral-Web-session-and-URL-free-layout')
 q=subprocess.run([a.binary,'--socket',sock,'workspace','save','--name','Generated Web layout','--json'],env=env,capture_output=True,text=True,timeout=15);assert q.returncode==0,q.stdout+q.stderr
 snapshots=list((root/'config/RUWAC/snapshots').rglob('*.json'));assert snapshots,'No workspace snapshot persisted'
 assert all('generated-persist-marker'not in p.read_text()and'generated-fragment'not in p.read_text()for p in snapshots);mark('saved-workspace-excludes-URL-query-and-fragment')
 for _ in range(12):command('tab-select',tab=0);command('tab-select',tab=1)
 assert js('window.count')==2 and rpc('pane.list',{'session':row['session']})[0]['pid']==pid;mark('tab-switching-preserves-Web-document-and-original-PTY-PID')
 command('web-preview',operation='focus');web_tab=command('inspect')['selected_tab'];command('close-tab',tab=web_tab);eventually(lambda:command('web-preview').get('count')==0)
 assert rpc('pane.list',{'session':row['session']})[0]['pid']==pid and Path('/proc',str(pid)).exists();mark('closing-Web-view-preserves-original-terminal-process')
 command('workspace-close');gui.wait(timeout=8)
 (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'scope':'Owned GTK/WebKit with real loopback HTTP and programmatic DOM and owned Mutter keyboard/pointer callbacks; no physical input, WebSocket/HMR, remote SSH or external-host claim.'},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 if sock.exists():
  try:rpc('daemon.stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
 server.shutdown();server.server_close();thread.join(timeout=3);print(root,flush=True)
