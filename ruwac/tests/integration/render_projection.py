#!/usr/bin/env python3
"""Real clients retain quiet frames across hidden output, without missing visible changes."""
import argparse,hashlib,json,os,subprocess,sys,tempfile
from pathlib import Path
from spike import Client,eventually

def run(a):
 root=Path(tempfile.mkdtemp(prefix='render-projection-',dir=a.evidence));root.chmod(0o700);sock=root/'ctl';binary=Path(a.binary).resolve()
 receiver=root/'receiver.py'
 receiver.write_text(r"""import os,tty
tty.setraw(0)
os.write(1,b'PROJECTION_READY\r\n')
pending=b''
while True:
 data=os.read(0,1024)
 if not data:break
 pending+=data
 while b'\n'in pending:
  b,pending=pending.split(b'\n',1)
  if b==b'q':raise SystemExit(0)
  os.write(1,b'\r\nACK_'+b+b'\r\n')
  if b==b'm':os.write(1,b'\x1b[?1000h\x1b[?1006h')
""")
 compile(receiver.read_text(),str(receiver),'exec')
 env={**os.environ,'RUWAC_STATE_FILE':str(root/'state.json'),'TERM':'xterm-256color'}
 with(root/'daemon.log').open('wb')as log:server=subprocess.Popen([str(binary),'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log)
 clients=[];checks=[]
 def record(name):checks.append({'case':name,'PASS':True});print(name,'PASS',flush=True)
 try:
  eventually(sock.exists)
  control=Client(sock);clients.append(control)
  def create(name):
   row=control.call('session.create',{'name':name,'rows':24,'cols':80,'argv':[sys.executable,str(receiver)]})
   eventually(lambda:'PROJECTION_READY'in control.call('pane.capture',{'pane':row['pane']})['text'])
   return row
  def output(pane,value):
   control.call('pane.sendText',{'pane':pane,'text':value+'\n'})
   eventually(lambda:'ACK_'+value in control.call('pane.capture',{'pane':pane})['text'])
  visible=create('Visible');hidden=create('Hidden session')
  eventually(lambda:'PROJECTION_READY'in control.call('pane.capture',{'pane':visible['pane']})['text'])
  viewers=[]
  for _ in range(2):
   c=Client(sock);clients.append(c);c.call('session.attach',{'session':visible['session'],'rows':24,'cols':80});viewers.append(c)
  initial=[c.call('client.render')for c in viewers]
  output(hidden['pane'],'other_session_one')
  for c,frame in zip(viewers,initial):
   quiet=c.call('client.render',{'revision':frame['revision']});assert quiet.get('unchanged')is True,'Hidden-session output rebuilt a quiet visible frame';assert quiet['revision']>frame['revision']
   # Both existing GUI/TUI clients retaining their full-frame revision and
   # clients following the returned global revision must remain quiet.
   output(hidden['pane'],'other_session_two')
   assert c.call('client.render',{'revision':frame['revision']}).get('unchanged')is True
   assert c.call('client.render',{'revision':quiet['revision']}).get('unchanged')is True
  record('HIDDEN-SESSION-OUTPUT-QUIET-TWO-CLIENTS')
  c=viewers[0];old=initial[0]['revision'];fresh=c.call('client.render');assert fresh.get('unchanged')is not True and fresh['panes'][0]['grid']==initial[0]['panes'][0]['grid']
  assert c.call('client.render',{'revision':old}).get('unchanged')is not True
  assert c.call('client.render',{'revision':2**63}).get('unchanged')is not True
  record('EXPLICIT-REFRESH-STALE-AND-FUTURE-REVISIONS')
  frame=c.call('client.render');output(visible['pane'],'visible_change');changed=c.call('client.render',{'revision':frame['revision']});assert changed.get('unchanged')is not True
  assert 'ACK_visible_change'in''.join(x[0]for row in changed['panes'][0]['grid']['cells']for x in row)
  record('VISIBLE-OUTPUT-DELIVERED')
  window=control.call('window.create',{'session':visible['session'],'name':'Hidden window','argv':[sys.executable,str(receiver)]})
  eventually(lambda:'PROJECTION_READY'in control.call('pane.capture',{'pane':window['pane']})['text'])
  frame=c.call('client.render');assert frame['window']==visible['window']
  output(window['pane'],'hidden_window_change');assert c.call('client.render',{'revision':frame['revision']}).get('unchanged')is True
  control.call('window.rename',{'window':window['window'],'name':'Renamed hidden window'})
  changed=c.call('client.render',{'revision':frame['revision']});assert changed.get('unchanged')is not True and any(w['name']=='Renamed hidden window'for w in changed['windows'])
  record('HIDDEN-WINDOW-OUTPUT-QUIET-STATUS-RENAME-DELIVERED')
  c.call('window.select',{'window':window['window']});focused=c.call('client.render',{'revision':changed['revision']});assert focused.get('unchanged')is not True and focused['window']==window['window']
  c.call('client.resize',{'rows':18,'cols':60});resized=c.call('client.render',{'revision':focused['revision']});assert resized.get('unchanged')is not True and resized['rows']==18 and resized['cols']==60
  record('FOCUS-AND-RESIZE-DELIVERED')
  output(window['pane'],'m');modes=c.call('client.render',{'revision':resized['revision']});assert modes.get('unchanged')is not True and modes['panes'][0]['grid']['mouse']is True and modes['panes'][0]['grid']['mouse_sgr']is True
  record('VISIBLE-INPUT-MODES-DELIVERED')
  control.call('pane.sendText',{'pane':window['pane'],'text':'q\n'})
  eventually(lambda:control.call('pane.capture',{'pane':window['pane']})['ended'])
  ended=c.call('client.render',{'revision':modes['revision']});assert ended.get('unchanged')is not True and ended['panes'][0]['ended']is True
  record('VISIBLE-EXIT-DELIVERED')
  (root/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual owned daemon and PTYs; no owner sessions'},indent=2)+'\n')
 finally:
  if clients:
   try:clients[0].call('daemon.stop')
   except Exception:pass
  for c in clients:c.close()
  if server.poll()is None:
   try:server.wait(timeout=5)
   except subprocess.TimeoutExpired:server.terminate();server.wait(timeout=3)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--evidence',type=Path,required=True);run(p.parse_args())
