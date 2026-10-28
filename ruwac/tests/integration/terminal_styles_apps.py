#!/usr/bin/env python3
"""Real styled PTY, fullscreen fzf, durable styles and optional native handlers."""
import argparse,hashlib,json,os,re,signal,subprocess,sys,tempfile,time,shutil
from pathlib import Path
from spike import Client,eventually,Tui
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--gui');p.add_argument('--evidence',type=Path,required=True);a=p.parse_args()
if a.gui and (os.environ.get('GDK_BACKEND')!='wayland' or not Path(os.environ.get('XDG_RUNTIME_DIR','/')).resolve().is_relative_to(a.evidence.resolve())):raise SystemExit('Native styles require an owned Wayland runtime inside evidence')
fzf=shutil.which('fzf');assert fzf,'Actual fzf executable is required, no simulated pass'
root=Path(tempfile.mkdtemp(prefix='terminal-styles-',dir=a.evidence));root.chmod(0o700);sock=root/'control.sock';state=root/'state.json';env={k:v for k,v in os.environ.items()if not k.startswith('FZF_')};env.update(RUWAC_STATE_FILE=str(state),FZF_DEFAULT_OPTS='',TERM='xterm-256color',GTK_A11Y='none',GSETTINGS_BACKEND='memory')
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:env[key]=str(root/name);Path(env[key]).mkdir(mode=0o700)
paint=root/'paint.py';paint.write_text('''import os,tty
tty.setraw(0)
for row,(code,label)in enumerate([('0','STYLES_READY'),('3','ITALIC'),('2','DIM'),('8','HIDDEN'),('9','STRIKE'),('4','SINGLE'),('4:2','DOUBLE'),('4:3','CURLY'),('4:4','DOTTED'),('4:5','DASHED')],1):
 os.write(1,(f'\\x1b[{row};1H\\x1b[0;{code};38;2;23;145;201;48;2;5;9;17;58:2::7:8:9m'+label+' e\\u0301界'+'\\x1b[0m').encode())
os.write(1,b"\\x1b[11;1HSTYLES_COMPLETE")
while os.read(0,1):pass
''')
chooser=root/'chooser.py';chooser.write_text('''import json,os,subprocess,sys
from pathlib import Path
root=Path(sys.argv[1]);os.write(1,b'PARENT-BEFORE-FZF\\r\\n')
p=subprocess.Popen([sys.argv[2],'--ansi','--border','--layout=reverse','--prompt=Pick: ','--header=RUWAC FZF REAL','--print-query','--bind=alt-j:down,alt-k:up'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
(root/'fzf-pid').write_text(str(p.pid))
output,_=p.communicate('alpha\\n\\x1b[3;4mŽivjo 界e\\u0301\\x1b[0m\\nomega\\n'.encode())
(root/'fzf-result.json').write_text(json.dumps({'returncode':p.returncode,'stdout':output.decode()}))
os.write(1,b'FZF-DONE\\r\\n')
''')
server=gui=tui=None;logs=[];owned=[];results=[];seq=0
def rpc(m,q=None,error=None):
 with Client(sock)as c:return c.call(m,q,error=error)
def start(label):
 global server
 f=(root/(label+'.log')).open('wb');logs.append(f);server=subprocess.Popen([a.binary,'--socket',str(sock),'serve'],env=env,stdout=f,stderr=f)
 def ready():
  if server.poll()is not None:raise RuntimeError('Owned daemon failed to start')
  try:return rpc('health')
  except(OSError,EOFError):return None
 eventually(ready,15)
def frame(session):
 with Client(sock)as c:c.call('session.attach',{'session':session,'rows':24,'cols':80});return c.call('client.render')['panes'][0]['grid']
def command(action,**kw):
 global seq
 seq+=1;tmp=root/'request.tmp';tmp.write_text(json.dumps({'seq':seq,'action':action,**kw}));tmp.replace(root/'gui-request.json')
 def read():
  try:d=json.loads((root/'gui-response.json').read_text());return d['result']if d['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(read,10)
def record(name,**details):results.append({'id':name,'status':'PASS',**details});print(name,'PASS',flush=True)
try:
 start('daemon');s=rpc('session.create',{'name':'Extended styles','rows':24,'cols':80,'argv':[sys.executable,str(paint)]});owned.append(s['pid']);eventually(lambda:'STYLES_COMPLETE'in rpc('pane.capture',{'pane':s['pane']})['text']);grid=frame(s['session']);cells=grid['cells']
 assert cells[1][0][6]['italic']and cells[2][0][6]['dim']and cells[3][0][6]['hidden']and cells[4][0][6]['strike']
 assert [cells[row][0][6]['underline']for row in range(5,10)]==['single','double','curly','dotted','dashed'];assert all(cells[row][0][6]['underline_color']==[7,8,9]for row in range(5,10))
 record('STYLES-ACTUAL-PTY-CANONICAL',five_underlines=True,Unicode_combining_wide=True)
 tui=Tui(Path(a.binary),sock,s['session'],root/'tui.bin');eventually(lambda:b'4:3'in tui.data and b'58;2;7;8;9'in tui.data,10);assert b';3'in tui.data and b';2'in tui.data and b';8'in tui.data and b';9'in tui.data;# Real TUI mouse selection must preserve styled live rows without a blank frame.
 before=len(tui.data);tui.send(b'\x1b[<0;1;2M\x1b[<32;5;2M');eventually(lambda:b'COPY 'in tui.data[before:])
 selected=bytes(tui.data[before:]);assert b'\x1b[2J'not in selected,'Selection cleared the entire terminal'
 assert b'38;2;23;145;201;48;2;5;9;17'in selected,'Selection discarded live colours'
 assert b'4:3'in selected and b'58;2;7;8;9'in selected,'Selection discarded extended attributes'
 tui.send(b'\x1b[<32;8;2M');time.sleep(.1);assert b'\x1b[2J'not in tui.data[before:]
 tui.finish('term');tui=None
 record('STYLES-REAL-TUI-EMISSION',actual_client_PTY=True,extended_ANSI=True)
 if a.gui:
  f=(root/'desktop.log').open('wb');logs.append(f);gui=subprocess.Popen([a.gui,'--socket',str(sock),'--session',s['session'],'--fixture-dir',str(root)],env=env,stdout=f,stderr=f);eventually(lambda:r if (r:=command('inspect')).get('drawn') and all(n>0 for n in r['drawn']) else None,15)
  print('SNAPSHOT',command('snapshot'),flush=True);eventually(lambda:(root/'native-rust.png').exists()and(root/'native-rust.png').stat().st_size>1000,10)
  record('STYLES-NATIVE-GTK-RENDER',actual_window_snapshot=True,synthetic_actions=True,physical_input=False)
  time.sleep(.5);before_draw=command('inspect')['drawn'][0];time.sleep(1.2);after_draw=command('inspect')['drawn'][0];assert after_draw-before_draw<=2,(before_draw,after_draw)
  record('STYLES-NATIVE-IDLE-REDRAW',observed_ms=1200,additional_draws=after_draw-before_draw,unchanged_frames=True)
 w=rpc('window.create',{'session':s['session'],'name':'Real fzf','argv':[sys.executable,str(chooser),str(root),fzf]});owned.append(w['pid']);eventually(lambda:'RUWAC FZF REAL'in rpc('pane.capture',{'pane':w['pane']})['text'],20)
 assert frame(s['session'])['alternate_screen'];assert 'PARENT-BEFORE-FZF'in rpc('pane.capture',{'pane':w['pane'],'buffer':'primary'})['text'];child=int((root/'fzf-pid').read_text());assert Path(f'/proc/{child}/comm').read_text().strip()=='fzf'
 if a.gui:
  # Select the window through the real navigation path. Merely assigning a
  # target pane in the fixture leaves the GUI client's selected window behind;
  # an asynchronous render can then correctly restore the previous target.
  command('command',text='select 1')
  eventually(lambda:command('inspect').get('target')==w['pane'],15)
  command('switch',view=0,text='Živjo')
  eventually(lambda:'1/3'in rpc('pane.capture',{'pane':w['pane']})['text'],15)
  command('key',key='j',mods=8);command('key',key='k',mods=8);command('key',key='Return')
 else:
  rpc('pane.sendText',{'pane':w['pane'],'text':'Živjo'})
  eventually(lambda:'1/3'in rpc('pane.capture',{'pane':w['pane']})['text'],15)
  rpc('pane.sendText',{'pane':w['pane'],'text':'\x1bj\x1bk'});rpc('pane.sendKey',{'pane':w['pane'],'key':'Enter'})
 eventually(lambda:(root/'fzf-result.json').exists(),15);r=json.loads((root/'fzf-result.json').read_text());text=re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]','',r['stdout']);assert r['returncode']==0 and text.splitlines()==['Živjo','Živjo 界e\u0301'],r
 eventually(lambda:'FZF-DONE'in rpc('pane.capture',{'pane':w['pane']})['text']);record('FZF-ACTUAL-FULLSCREEN-UNICODE',fzf_version=subprocess.check_output([fzf,'--version'],text=True).strip(),actual_child=True,native_handlers=bool(a.gui),Alt_j_k=True,exact_query_and_selection=True,primary_preserved=True)
 rpc('window.select',{'window':s['window']})
 if gui is not None:command('workspace-close');gui.wait(timeout=10);gui=None
 before=frame(s['session'])['cells'];rpc('daemon.checkpoint');saved=json.loads(state.read_bytes());assert saved['schema']==5;server.kill();server.wait(timeout=10);server=None;start('restored');assert frame(s['session'])['cells']==before;row=rpc('pane.list',{'session':s['session']})[0];assert row['ended']and row['restored']and row['pid']is None
 record('STYLES-ACTUAL-SIGKILL-RECOVERY',all_render_attributes_exact=True,ended_without_resurrection=True)
finally:
 if tui is not None:tui.finish('term')
 if gui is not None and gui.poll()is None:gui.terminate();gui.wait(timeout=10)
 if server is not None and server.poll()is None:
  try:rpc('daemon.stop');server.wait(timeout=10)
  except Exception:server.kill();server.wait(timeout=5)
 for pid in owned:
  try:
   data=Path(f'/proc/{pid}/cmdline').read_bytes()
   if str(root).encode()in data:os.killpg(pid,signal.SIGKILL)
  except FileNotFoundError:pass
  except ProcessLookupError:pass
 for f in logs:f.close()
 receipt={'cases':results,'sha256':{'ruwac':hashlib.sha256(Path(a.binary).read_bytes()).hexdigest(),**({'ruwac-desktop':hashlib.sha256(Path(a.gui).read_bytes()).hexdigest()}if a.gui else{})},'scope':'Real owned PTY, TUI/fzf and daemon SIGKILL; native GTK actions on owned display only. No owner config/clipboard/session, physical input or Kitty/AltGr/IME pass'};(root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');(a.evidence/'terminal-styles-acceptance.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
assert len(results)==(6 if a.gui else 4)
