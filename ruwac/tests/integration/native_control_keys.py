#!/usr/bin/env python3
"""Real compositor key presses: every Ctrl+letter chord reaches the terminal
program as its control byte (isolated Mutter display, generated receiver).

Desktop shortcuts keep Ctrl+V (paste) and, with a selection, Ctrl+C (copy);
every other chord, including Ctrl+X Ctrl+S, belongs to the program.
"""
from pathlib import Path
import argparse,json,os,subprocess,sys,tempfile,time
from gi.repository import Gio,GLib
from spike import Client,eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,required=True)
a=p.parse_args()
assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test','Requires owned compositor'
r=Path(tempfile.mkdtemp(prefix='control-keys-',dir=a.evidence));r.chmod(0o700)
env={**os.environ,'TERM':'xterm-256color','GTK_A11Y':'none','GSETTINGS_BACKEND':'memory'}
for k in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(k,None)
for k,leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 d=r/leaf;d.mkdir(mode=0o700);env[k]=str(d)
env['RUWAC_STATE_FILE']=str(r/'state.json');sock=r/'ctl'
children=[];seq=0;bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';seat=None;checks=[]
def start(args,name):
 q=subprocess.Popen(list(map(str,args)),env=env,stdout=(r/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def control(m,args=None):return bus.call_sync(service,seat,service+'.Session',m,args,None,Gio.DBusCallFlags.NONE,5000,None)
def key(code,down):control('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,down)))
def command(action,**kw):
 global seq
 seq+=1;tmp=r/'request.tmp';tmp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));tmp.replace(r/'gui-request.json')
 def read():
  try:
   d=json.loads((r/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(read,10)
KEYS={'q':16,'w':17,'e':18,'r':19,'t':20,'y':21,'u':22,'i':23,'o':24,'p':25,'a':30,'s':31,'d':32,'f':33,'g':34,'h':35,'j':36,'k':37,'l':38,'z':44,'x':45,'c':46,'v':47,'b':48,'n':49,'m':50}
log=r/'keys.log';receiver=r/'log.py'
receiver.write_text("import os,sys,tty\nlog=open(sys.argv[1],'ab',0)\ntty.setraw(0)\nos.write(1,b'KEYS_READY')\nwhile True:\n b=os.read(0,256)\n if not b: break\n log.write(b)\n")
try:
 seat=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0];control('Start');control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(1.,1.)))
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 with Client(sock)as c:row=c.call('session.create',{'name':'keys','argv':[sys.executable,str(receiver),str(log)]})
 start([a.gui,'--new-instance','--socket',sock,'--session',row['session'],'--allow-workspace-control','--fixture-dir',r],'gui')
 eventually(lambda:command('inspect').get('target'),15);key(194,True);key(194,False)
 command('chrome',width=1200,height=800,maximized=True);command('focus-view',view=0)
 eventually(lambda:command('chrome')['maximized']);time.sleep(.3);info=command('inspect');ox,oy=info['area_origin']
 control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));control('NotifyPointerMotionRelative',GLib.Variant('(dd)',(ox+60.,oy+60.)))
 control('NotifyPointerButton',GLib.Variant('(ib)',(272,True)));control('NotifyPointerButton',GLib.Variant('(ib)',(272,False)));time.sleep(.5)
 received={}
 def chord(letter):
  before=log.read_bytes()if log.exists()else b''
  key(29,True);key(KEYS[letter],True);key(KEYS[letter],False);key(29,False)
  deadline=time.monotonic()+1.0;got=b''
  while time.monotonic()<deadline:
   got=(log.read_bytes()if log.exists()else b'')[len(before):]
   if got:break
   time.sleep(.02)
  return got
 # A plain letter first proves the keyboard route itself.
 before=log.read_bytes()if log.exists()else b''
 key(KEYS['x'],True);key(KEYS['x'],False)
 eventually(lambda:(log.read_bytes()if log.exists()else b'')[len(before):]==b'x',5)
 for letter in sorted(KEYS):received[letter]=chord(letter).hex()
 (r/'received.json').write_text(json.dumps(received,indent=1))
 wrong={l:h for l,h in received.items() if l!='v' and h!=bytes([ord(l)&31]).hex()}
 print('RECEIVED',json.dumps(received),flush=True);print('WRONG',json.dumps(wrong),flush=True)
 assert not wrong,wrong
 checks.append('every-Ctrl-letter-except-desktop-paste-reaches-the-program')
 before=log.read_bytes();key(29,True);key(KEYS['x'],True);key(KEYS['x'],False);key(KEYS['s'],True);key(KEYS['s'],False);key(29,False)
 eventually(lambda:log.read_bytes()[len(before):]==b'\x18\x13',5)
 checks.append('Ctrl-X-Ctrl-S-chord-with-Control-held-reaches-the-program')
 (Path(a.evidence)/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'received':received},indent=2)+'\n')
 print('Control keys PASS',len(checks),flush=True)
finally:
 for q in reversed(children):
  if q.poll() is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=6)
  except subprocess.TimeoutExpired:q.kill()
