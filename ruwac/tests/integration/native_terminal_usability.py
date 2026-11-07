#!/usr/bin/env python3
"""Owned GTK startup isolation, real nested tmux input and read-only default import."""
import argparse,json,os,shlex,subprocess,sys,tempfile
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--gui',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args()
if not os.environ.get('WAYLAND_DISPLAY','').startswith('ruwac-'):p.error('Explicit owned test compositor required')
if os.environ.get('RUWAC_NATIVE_OWNED_BUS')!='1':
 result=subprocess.run(['dbus-run-session','--',sys.executable,str(Path(__file__).resolve()),*sys.argv[1:]],env={**os.environ,'RUWAC_NATIVE_OWNED_BUS':'1'},stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
 print(result.stdout,end='')
 if result.returncode:print(result.stderr[-2500:],file=sys.stderr)
 raise SystemExit(result.returncode)

a.evidence.mkdir(parents=True,exist_ok=True);root=Path(tempfile.mkdtemp(prefix='native-terminal-usability-',dir=a.evidence));root.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory'}
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:env[key]=str(root/name);Path(env[key]).mkdir(mode=0o700)
env['SHELL']='/bin/bash'
for key in ['TMUX','STY','RUWAC_SHORTCUTS','RUWAC_KEYMAP']:env.pop(key,None)
home=Path(env['HOME']);data=Path(env['XDG_DATA_HOME']);paths=[]
for i,folder in enumerate([data/'remmina',home/'.remmina',home/'.var/app/org.remmina.Remmina/data/remmina']):
 folder.mkdir(parents=True,mode=0o700);file=folder/(str(i)+'.remmina');file.write_text('[remmina]\nname=Generated Remmina '+str(i)+'\ngroup=Lab/'+('A' if i<2 else 'B')+'\nprotocol=RDP\nserver=127.0.0.1\npassword=GENERATED-DO-NOT-IMPORT\n');paths.append(file)
before={str(path):path.read_bytes() for path in paths};sock=root/'control.sock';children=[];seq=0;fixture=root/'fixture';fixture.mkdir(mode=0o700);checks=[]
def start(argv,name):
 log=(root/(name+'.log')).open('w');child=subprocess.Popen(argv,env=env,stdout=log,stderr=log);children.append((child,log));return child
def rpc(method,params=None):
 with Client(sock)as c:return c.call(method,params)
def command(action,**kw):
 global seq
 seq+=1;tmp=fixture/'request.tmp';tmp.write_text(json.dumps({'seq':seq,'action':action,**kw}));tmp.replace(fixture/'gui-request.json')
 def read():
  try:q=json.loads((fixture/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(read,12)
def gui(*options):
 (fixture/'gui-response.json').unlink(missing_ok=True)
 (fixture/'gui-request.json').unlink(missing_ok=True)
 child=start([str(a.gui),'--new-instance','--socket',str(sock),'--fixture-dir',str(fixture),*options],'gui-'+str(len(children)))
 eventually(lambda:command('inspect'),20);command('tab-select',tab=0)
 return child
def terminate(child):child.terminate();child.wait(timeout=10)
admin=None
try:
 daemon=start([str(a.binary),'--socket',str(sock),'serve'],'daemon');eventually(sock.exists)
 admin=Client(sock);session=admin.call('session.create',{'name':'Generated CLI view','argv':['/bin/cat']})
 admin.call('session.attach',{'session':session['session'],'rows':32,'cols':137})
 first=gui('--show-machines');inspect=eventually(lambda:command('inspect').get('target'),20)
 assert inspect!=session['pane'] and len(rpc('session.list'))==2
 assert (admin.call('client.render')['panes'][0]['grid']['rows'],admin.call('client.render')['panes'][0]['grid']['cols'])==(32,137)
 checks.append('default-GTK-creates-own-session-and-preserves-attached-CLI-size')
 command('chrome',width=1600,height=1000,collapse=False,dock=False)
 import time
 time.sleep(.5)
 controls=command('chrome');(root/'chrome-initial.json').write_text(json.dumps(controls,indent=2));command('snapshot');assert controls['header_height']<=33,{k:v for k,v in controls.items()if k!='header_tree'}
 assert controls['content_height']==controls['window_height']-controls['header_height'],controls
 command('chrome',collapse=True);time.sleep(.3)
 narrow=command('chrome');last_tab=narrow['tab_layout']['tabs'][-1];plus=narrow['tab_new_terminal']
 assert last_tab['inside_viewport']and plus['mapped']and 0<=plus['origin'][0]-(last_tab['origin'][0]+last_tab['width'])<=20
 command('chrome',collapse=False,open=True,width=800,height=600)
 eventually(lambda:command('chrome')['open'])
 command('chrome',open=False);eventually(lambda:not command('chrome')['open'])
 checks.append('single-row-header-allocation-collapse-and-controls-popover')
 command('profiles-control',import_expanded=True)
 eventually(lambda:command('profiles-control')['discovered_count']==3)
 preview=command('profiles-control',discovered_index=0)
 assert 'Generated Remmina 0'in preview['status'] and preview['import_enabled'] and preview['import_outside_scroll']
 command('profiles-control',operation='import')
 saved=Path(env['XDG_CONFIG_HOME'])/'RUWAC/machines.json'
 def imported():
  try:return json.loads(saved.read_text())['items']
  except(OSError,ValueError):return None
 profiles=eventually(imported)
 assert len(profiles)==1 and profiles[0].get('secret_reference')==profiles[0]['id']
 assert 'GENERATED-DO-NOT-IMPORT'not in saved.read_text()
 assert all(path.read_bytes()==before[str(path)] for path in paths)
 checks.append('GTK-discovers-native-legacy-flatpak-and-imports-reviewed-metadata-only')
 command('profiles-control',operation='import-all');command('profiles-control',bulk_response='cancel');assert len(imported())==1
 command('profiles-control',operation='import-all')
 paths[1].write_text(paths[1].read_text().replace('Generated Remmina 1','Changed during review'))
 command('profiles-control',bulk_response='import');assert len(imported())==1
 paths[1].write_bytes(before[str(paths[1])])
 command('profiles-control',operation='import-all');command('profiles-control',bulk_response='import')
 eventually(lambda:len(imported())==3)
 identities=[p['id'] for p in imported()]
 command('profiles-control',operation='import-all');command('profiles-control',bulk_response='import');assert [p['id'] for p in imported()]==identities
 assert 'GENERATED-DO-NOT-IMPORT' not in saved.read_text() and all(path.read_bytes()==before[str(path)] for path in paths)
 checks.append('native-bulk-import-confirm-cancel-stale-review-duplicate-and-credential-separation')
 command('profiles-control',import_expanded=False)
 groups=command('profiles-control')['groups'];assert len(groups)==2 and all(g['expanded'] for g in groups),groups
 command('profiles-control',group='Lab › A',expanded=False)
 eventually(lambda:command('profiles-control')['visible']['rows']==1)
 # Search reveals matching machines without forgetting the collapsed state.
 command('profiles',search='Generated Remmina 0');eventually(lambda:command('profiles-control')['visible']['rows']==1)
 assert command('profiles-control')['groups'][0]['expanded']
 command('profiles',search='');eventually(lambda:command('profiles-control')['visible']['rows']==1)
 command('sidebar-shell',width=240)
 assert json.loads((Path(env['XDG_CONFIG_HOME'])/'RUWAC/ui.json').read_text())['collapsed_groups']==['Lab/A']
 checks.append('machine-groups-collapse-search-reveals-matches-and-preferences-survive-width-change')

 command('sidebar-shell',shown=False)
 command('chrome',open=False);baseline=command('chrome')['content_height']
 for i in range(20):
  sample=command('chrome',open=i%2==0);assert sample['content_height']==baseline
 command('chrome',open=False,dock=True);time.sleep(.3)
 docked=command('chrome');assert docked['docked'] and docked['content_height']<baseline and docked['dock_height']<=32,{k:v for k,v in docked.items()if k!='header_tree'}
 command('chrome',dock=False);time.sleep(.3);assert command('chrome')['content_height']==baseline
 command('rdp-open');time.sleep(.3)
 remote=command('rdp-inspect',rdp=0)['viewport']
 for i in range(20):
  command('chrome',open=i%2==0)
  assert command('rdp-inspect',rdp=0)['viewport']==remote
 command('chrome',open=False)
 checks.append('controls-popover-preserves-terminal-and-RDP-size-explicit-dock-takes-space')
 # More than two former pages, generated locally: every row is in the scroll list.
 for i in range(120):
  (paths[0].parent/f'bulk-{i}.remmina').write_text(f'[remmina]\nname=Generated bulk {i:03}\ngroup=Other\nprotocol=RDP\nserver=127.0.0.1:{4000+i}\n')
 terminate(first);first=gui('--show-machines');eventually(lambda:command('inspect').get('target'),20)
 command('profiles-control',import_expanded=True)
 eventually(lambda:command('profiles-control')['discovered_count']==123)
 command('profiles-control',operation='import-all');command('profiles-control',bulk_response='import')
 eventually(lambda:len(imported())==123)
 command('sidebar-shell',shown=True);time.sleep(.3)
 many=command('profiles-control');assert many['visible']['rows']==121,many['visible']
 checks.append('all-123-machines-in-one-scroll-list-with-collapsed-group-preserved')
 command('tab-select',tab=0);command('sidebar-shell',shown=False);time.sleep(.2)
 metrics=[]
 for scheme,bg in [('graphite',0x131416),('nocturne',0x121320),('fjord',0x262a33),('ember',0x181613),('paper',0xfbfaf7),('contrast',0x000000)]:
  c=command('chrome',scheme=scheme);assert c['scheme']==scheme and c['terminal_background']==bg,c
  time.sleep(.1);command('snapshot')
  from PIL import Image
  pixel=Image.open(fixture/'native-rust.png').convert('RGB').getpixel((5,c['header_height']+5));assert pixel==((bg>>16)&255,(bg>>8)&255,bg&255),(scheme,pixel)
 command('chrome',scheme='graphite',size=12);time.sleep(.3);before_grid=command('inspect')['grid_size']
 command('chrome',size=14);time.sleep(.3)
 assert command('chrome')['term_size_pt']==14
 assert command('inspect')['grid_size']!=before_grid,(before_grid,command('inspect'))
 command('snapshot');assert (fixture/'native-rust.png').is_file()
 prefs=json.loads((Path(env['XDG_CONFIG_HOME'])/'RUWAC/ui.json').read_text());assert prefs['schema']==3 and prefs['term_size_pt']==14 and prefs['collapsed_groups']==['Lab/A']
 checks.append('six-live-palettes-and-persisted-schema3-font-size')
 command('sidebar-shell',shown=True)
 result=command('profile-editor',operation='new',fields={'0':'Generated agent host','1':'Agents','3':'127.0.0.1:22222','4':'fixture','13':1},named={'machine-codex-command':'codex resume --all','machine-claude-command':'claude --resume','machine-agent-backend':2},response='save')
 assert not result.get('error'),result
 configured=eventually(lambda:next((p for p in imported()if p['name']=='Generated agent host'),None))
 assert configured['preferences']['codex_command']=='codex resume --all' and configured['preferences']['claude_command']=='claude --resume' and configured['preferences']['agent_backend']=='tmux'
 checks.append('native-profile-saves-agent-commands-and-backend-without-launching')

 terminate(first)
 tmux=root/'tmux.sock';conf=root/'tmux.conf';conf.write_text('set -g prefix C-a\nbind C-a send-prefix\nset -g status off\nset -g set-clipboard off\nset -g default-shell /bin/sh\n')
 receiver=Path(__file__).resolve().parents[1]/'fixtures/receiver.py';output=root/'received.bin'
 argv=['tmux','-S',str(tmux),'-f',str(conf),'new-session','-s','generated',shlex.join([sys.executable,str(receiver),str(output)])]
 nested=rpc('session.create',{'name':'Generated nested tmux','argv':argv})
 rpc('pane.wait',{'pane':nested['pane'],'contains':'RUWAC_RECEIVER_READY','timeout_ms':8000})
 second=gui('--session',nested['session'],'--show-machines');eventually(lambda:command('inspect').get('target')==nested['pane'],20)
 assert any(g['label']=='Lab › A' and not g['expanded'] for g in command('profiles-control')['groups'])
 checks.append('collapsed-machine-groups-restored-after-GTK-restart')
 def key(key,mods=0):command('key',key=key,mods=mods)
 key('a',4);key('c')
 def windows():return subprocess.check_output(['tmux','-S',str(tmux),'list-windows','-t','generated'],env=env,text=True).splitlines()
 eventually(lambda:len(windows())==2)
 assert len(rpc('window.list',{'session':nested['session']}))==1
 key('a',4);key('0');key('x')
 eventually(lambda:output.exists() and output.read_bytes()==b'x')
 checks.append('GTK-single-Control-A-routes-to-real-tmux-without-outer-window-change')
 about=command('about',close=True)
 assert about['donation_url']=='https://idz.si/bs/ruwac-donate.html' and about['qr_modules']>25 and not about['visible']
 checks.append('native-About-version-license-support-and-static-QR')
 # A real Bash login reads an owned profile and interactive rc, including when
 # an older running daemon otherwise defaults to --noprofile/--norc.
 home.joinpath('.bash_profile').write_text('. "$HOME/.bashrc"\n')
 home.joinpath('.bashrc').write_text("PS1='GENERATED_USER_PROMPT> '; printf 'GENERATED_RC_LOADED\\n'\n")
 known={p['id'] for ses in rpc('session.list') for p in rpc('pane.list',{'session':ses['id']})}
 command('new-terminal')
 target=eventually(lambda:q['target'] if (q:=command('inspect',view=1)).get('target') and q['target'] not in known else None)
 eventually(lambda:'GENERATED_USER_PROMPT>' in rpc('pane.capture',{'pane':target})['text'])
 assert 'GENERATED_RC_LOADED' in rpc('pane.capture',{'pane':target})['text']
 count=command('inspect')['tabs']
 # Exercise the GUI input path; an external RPC exit intentionally retains history.
 for letter in ['e','x','i','t','Return']:command('key',view=1,key=letter)
 eventually(lambda:command('inspect')['tabs']==count-1)
 assert second.poll() is None and daemon.poll() is None
 checks.append('native-real-Bash-login-loads-rc-and-exit-closes-only-ended-tab')
 # The caller runs this suite in its own D-Bus session: a repeated normal GUI
 # invocation activates this fixture's primary application without another PTY.
 primary_session=rpc('session.create',{'name':'Generated primary application','argv':['/bin/cat']})
 count=len(rpc('session.list'))
 primary=start([str(a.gui),'--socket',str(sock),'--session',primary_session['session']],'primary-application')
 def primary_ready():
  query=subprocess.run(['gdbus','call','--session','--dest','si.z4.ruwac','--object-path','/si/z4/ruwac','--method','org.freedesktop.DBus.Peer.Ping'],env=env,capture_output=True,text=True,timeout=3)
  return query.returncode==0 and primary.poll()is None
 eventually(primary_ready,10)
 repeat=subprocess.run([str(a.gui),'--socket',str(sock)],env=env,capture_output=True,text=True,timeout=10)
 assert repeat.returncode==0 and len(rpc('session.list'))==count and primary.poll()is None
 checks.append('repeated-GTK-launch-reuses-primary-without-an-orphan-session')

 assert daemon.poll()is None and second.poll()is None
 terminate(second);terminate(primary)
 final=rpc('session.create',{'name':'Generated final shell','argv':['/bin/bash','-l']})
 last=gui('--session',final['session']);eventually(lambda:command('inspect').get('target')==final['pane'],20)
 for letter in ['e','x','i','t','Return']:command('key',view=0,key=letter)
 eventually(lambda:command('terminal-recovery')['ended'],8)
 assert last.poll()is None and daemon.poll()is None and any(s['id']==final['session']for s in rpc('session.list'))
 command('terminal-recovery',operation='terminal-new-ended')
 eventually(lambda:command('inspect')['tabs']==2)
 assert last.poll()is None and daemon.poll()is None
 # Recreate the original ended local shell, then retain the independent new shell.
 command('tab-select',tab=0)
 command('terminal-recovery',operation='terminal-reconnect')
 eventually(lambda:command('inspect')['tabs']==2 and not command('terminal-recovery')['ended'])
 assert last.poll()is None and daemon.poll()is None
 fresh=eventually(lambda:command('inspect',view=1).get('target'))
 rpc('pane.sendText',{'pane':fresh,'text':'exit\n'})
 eventually(lambda:command('terminal-recovery',view=1)['ended'])
 # A failed creation must leave the only recovery view and its independent peer.
 unavailable=sock.with_name('generated-unreachable-socket');sock.rename(unavailable)
 try:
  command('terminal-recovery',view=1,operation='terminal-replace-ended')
  eventually(lambda:'New terminal failed' in command('inspect',view=1)['status'])
  assert command('inspect')['tabs']==2 and last.poll()is None and daemon.poll()is None
 finally:unavailable.rename(sock)
 command('terminal-recovery',view=1,operation='terminal-replace-ended')
 eventually(lambda:command('inspect')['tabs']==2 and (p:=command('inspect',view=1).get('target'))and p!=fresh)
 assert last.poll()is None and daemon.poll()is None
 checks.append('native-last-shell-exit-retains-window-and-can-create-or-recreate-a-terminal')
 receipt={'status':'PASS','checks':checks,'scope':'Actual GTK on owned Wayland, actual daemon/PTys/tmux and generated Remmina files; GTK callbacks are synthetic, not physical input or external SSH acceptance'}
 (root/'acceptance.json').write_text(json.dumps(receipt,indent=2)+'\n');print('Native terminal usability PASS',len(checks),'groups')
finally:
 if admin:admin.close()
 for child,log in reversed(children):
  if child.poll()is None:child.terminate()
  try:child.wait(timeout=10)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  log.close()
 if (root/'tmux.sock').exists():subprocess.run(['tmux','-S',str(root/'tmux.sock'),'kill-server'],env=env,capture_output=True)
