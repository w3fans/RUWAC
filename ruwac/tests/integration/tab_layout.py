"""Responsive headers on an isolated 1800x1000 Mutter display; generated PTYs only.

Requires the existing native fixture wrapper with RUWAC_TEST_MUTTER_INPUT=1.
Never run this against an owner desktop, daemon socket or configuration.
"""
from pathlib import Path
import argparse,json,os,subprocess,time
from gi.repository import Gio,GLib
from spike import Client,eventually
p=argparse.ArgumentParser()
for n in ['binary','gui','evidence']:p.add_argument('--'+n,required=True)
a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=Path(a.evidence)/'tab-acceptance';r.mkdir(mode=0o700);sock=r/'s';children=[];seq=0;checks=[]
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash'}
for k,n in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
 d=r/n;d.mkdir(mode=0o700);env[k]=str(d)
for k in ['RUWAC_STATE_FILE','RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE','TMUX','STY']:env.pop(k,None)
(r/'config/RUWAC').mkdir(mode=0o700)
prefs_path=r/'config/RUWAC/ui.json'
old={'schema':3,'scheme':'graphite','term_font':'Fira Code','term_size_pt':13,'pinned':False,'width':280,'collapsed_groups':['Generated group']}
prefs_path.write_text(json.dumps(old));prefs_path.chmod(0o600)
def start(args,name):
 q=subprocess.Popen(list(map(str,args)),env=env,stdout=(r/(name+'.log')).open('w'),stderr=subprocess.STDOUT);children.append(q);return q
def rpc(m,p=None,gui=False):
 with Client(sock.with_suffix('.gui.sock')if gui else sock)as c:return c.call(m,p)
def command(action,**kw):
 global seq
 seq+=1;t=r/'request.tmp';t.write_text(json.dumps({'seq':seq,'action':action,**kw}));t.replace(r/'gui-request.json')
 def answer():
  try:v=json.loads((r/'gui-response.json').read_text());return v['result']if v['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(answer,12)
def save(name,value):
 (r/(name+'.json')).write_text(json.dumps(value,indent=2)+'\n')
def mark(name,**details):
 checks.append({'name':name,**details});print(name,'PASS',flush=True);save('checks',checks)
def panes():return {p['id']:p['pid']for s in rpc('session.list')for p in rpc('pane.list',{'session':s['id']})}
def chrome():return command('chrome')
def tabs():return chrome()['tab_layout']['tabs']
def selected():return next(t for t in tabs()if t['selected'])
def key(code,down):seat('NotifyKeyboardKeycode',GLib.Variant('(ub)',(code,down)))
def press(code):key(code,True);key(code,False);time.sleep(.12)
def chord(mod,code):key(mod,True);press(code);key(mod,False);time.sleep(.2)
def pointer(x,y):
 seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(-10000.,-10000.)));seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',(float(x),float(y))));time.sleep(.08)
def button(code=272,down=True):seat('NotifyPointerButton',GLib.Variant('(ib)',(code,down)))
def click(tab,code=272):
 x,y=tab['origin'];pointer(x+max(2,tab['width']/2),y+tab['height']/2);button(code);button(code,False);time.sleep(.25)
def geometry(half):
 # Maximize first, then tile from known geometry: never alternate tile/restore.
 command('chrome',maximized=True,collapse=True);time.sleep(.3)
 if half:chord(125,105)
 time.sleep(.3);s=chrome();assert s['window_width']==(900 if half else 1800),s['window_width'];return s
def layout_check(count,two,compact,half):
 command('chrome',two_tab_rows=two,compact_tabs=compact);geometry(half)
 def ready():
  s=chrome();t=s['tab_layout'];return s if len(t['tabs'])==count and t['rows']==(2 if two else 1)and all(x['width']>0 for x in t['tabs'])else None
 s=eventually(ready);time.sleep(.2);s=chrome();save(f'layout-{count}-{two}-{compact}-{half}',s)
 t=s['tab_layout'];b=s['tab_new_terminal'];x,y=b['origin']
 assert b['mapped']and b['sensitive']and x>=0 and x+b['width']<=s['window_width']
 assert t['two_rows']==two and t['compact']==compact
 assert not any(n['type']=='GtkScrollbar'and n['height']>0 for n in s['header_tree'])
 assert all(x['role']=='Tab'and x['title']for x in t['tabs'])
 # Preserve readable controls/fonts; crowded rows may scroll instead of hiding X.
 assert next(x for x in t['tabs']if x['selected'])['inside_viewport']
 assert all(x['close_visible']and x['icon_size']==16 and x['indicator_size']==16 for x in t['tabs'])
 # Equal widths per row: capped near a fifth of the row while few tabs are
 # open, shrinking to a readable minimum; beyond it the row scrolls with arrows.
 minimum=84 if compact else 108
 for row in range(t['rows']):
  members=[x for x in t['tabs']if x['row']==row]
  widths={x['width']for x in members};assert len(widths)==1,('UNEQUAL_TAB_WIDTHS',row,widths)
  # GTK size requests cover the margin box; compare the allocation request.
  requests={x['width_request']for x in members};assert len(requests)==1,('UNEQUAL_TAB_REQUESTS',row,requests)
  width=requests.pop();arrows=t['arrows'][row];full=t['available'][row]
  assert width>=minimum,('TAB_BELOW_MINIMUM',width)
  assert width<=max(minimum,int(full*0.22))+2,('TAB_WIDER_THAN_A_FIFTH',width,full)
  overflow=len(members)*(minimum+2)>full
  assert arrows['visible']==overflow,('ARROWS',row,arrows,len(members),full)
  if not overflow:assert all(x['inside_viewport']for x in members)
 assert panes()==baseline
 if (count,two,compact,half)in [(20,False,False,False),(20,False,False,True),(40,True,True,True)]:
  shot=command('snapshot');assert shot['saved'];(r/'native-rust.png').replace(r/f'tabs-{count}-{two}-{compact}-{half}.png')
 mark('layout',count=count,two=two,compact=compact,half=half,visible=sum(x['inside_viewport']for x in t['tabs']),header_height=s['header_height'])
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';session=None
try:
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
 def seat(method,args=None):return bus.call_sync(service,session,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
 seat('Start');pointer(1,1);press(194)
 seeds=[]
 def launch(count):
  for n in ['gui-request.json','gui-response.json']:(r/n).unlink(missing_ok=True)
  args=[a.gui,'--new-instance','--socket',sock,'--allow-workspace-control','--fixture-dir',r]
  for s in seeds:args+=['--session',s]
  g=start(args,'gui-'+str(count));eventually(lambda:command('inspect').get('tabs')==count,15);return g
 for count in [20,40]:
  while len(seeds)<count:seeds.append(rpc('session.create',{'name':'Generated terminal '+str(len(seeds)),'argv':['/bin/sleep','600']})['session'])
  baseline=panes();gui=launch(count)
  for two,compact in [(False,False),(False,True),(True,False),(True,True)]:
   for half in [False,True]:layout_check(count,two,compact,half)
  if count==20:
   command('chrome',two_tab_rows=False,compact_tabs=False);geometry(False)
   # Equal-width tabs overflow at 20: bring the row start into view with the
   # real left arrow before clicking by position.
   for _ in range(40):
    arrows=chrome()['tab_layout']['arrows'][0]
    if not(arrows['visible']and arrows['left_sensitive']):break
    x,y=arrows['left_origin'];pointer(x+arrows['width']/2,y+arrows['height']/2);button();button(down=False);time.sleep(.08)
   eventually(lambda:all(t['inside_viewport']for t in tabs()[:6]),10);time.sleep(.2)
   before=tabs();ids=[t['id']for t in before]
   click(before[5]);assert selected()['id']==ids[5];assert panes()==baseline
   chord(56,4);eventually(lambda:selected()['id']==ids[2]);mark('native-click-and-alt-digit-keep-model-and-PTYs')
   # Closing the native tab popup restores its header's keyboard focus.
   click(tabs()[2],273);press(1)
   press(107);press(57)
   # Record before asserting so a missing focus route is actionable evidence.
   save('keyboard-end',chrome());assert selected()['id']==ids[-1]
   click(tabs()[-1],273);press(1);press(102);press(57);assert selected()['id']==ids[0]
   click(tabs()[0],273);press(1);press(106);press(57);assert selected()['id']==ids[1]
   click(tabs()[1],273);press(1)
   chord(42,68);menu=chrome();save('keyboard-menu',menu);assert any(n['type']=='GtkPopoverMenu'and n['width']>0 for n in menu['header_tree']);press(1)
   mark('native-header-home-end-arrows-and-keyboard-menu')
   # Native drag, not a synthetic reorder command.
   before=tabs();src=before[2];dst=before[5];sx,sy=src['origin'];dx,dy=dst['origin'];sx+=src['width']/2;sy+=src['height']/2;dx+=dst['width']/2;dy+=dst['height']/2
   pointer(sx,sy);button();time.sleep(.15);
   for step in range(1,17):seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',((dx-sx)/16,(dy-sy)/16)));time.sleep(.025)
   button(down=False);time.sleep(.5);after=tabs();save('after-drag',chrome())
   assert sorted(t['id']for t in after)==sorted(ids)and[t['id']for t in after]!=ids
   assert panes()==baseline;mark('native-drag-reorders-without-restarting-PTYs')
   # Existing detach action and transfer model remain stable, including row preferences.
   command('chrome',two_tab_rows=True,compact_tabs=True)
   detached=command('detach-tab',operation='detach',tab=2);save('detached',detached)
   assert panes()==baseline
   command('detach-tab',operation='return',window=1);eventually(lambda:len(tabs())==count);assert panes()==baseline
   mark('detach-return-keeps-PTY-PIDs')
   geometry(False);src=tabs()[3];sx,sy=src['origin'];sx+=src['width']/2;sy+=src['height']/2
   pointer(sx,sy);button();time.sleep(.15)
   for step in range(16):seat('NotifyPointerMotionRelative',GLib.Variant('(dd)',((700-sx)/16,(400-sy)/16)));time.sleep(.03)
   button(down=False)
   eventually(lambda:len(command('detach-tab',operation='inspect')['windows'])==2)
   assert panes()==baseline
   command('detach-tab',operation='return',window=1);eventually(lambda:len(tabs())==count);assert panes()==baseline
   geometry(False);mark('native-drag-out-detaches-and-returns-with-stable-PTYs')
   click(tabs()[4],273);menu=chrome();save('pointer-menu',menu);assert any(n['type']=='GtkPopoverMenu'and n['width']>0 for n in menu['header_tree']);press(1);assert panes()==baseline;mark('native-right-click-menu')
   s=chrome();b=s['tab_new_terminal'];click(b)
   eventually(lambda:len(tabs())==count+1);assert set(baseline.items())<=set(panes().items());mark('native-plus-adds-one-terminal')
   # Close only the new generated terminal frontend; original model remains.
   command('close-tab',tab=count);eventually(lambda:len(tabs())==count)
   assert set(baseline.items())<=set(panes().items())
   baseline=panes()
   mark('existing-close-action-preserves-persistent-processes')
  else:
   command('chrome',two_tab_rows=False,compact_tabs=False);geometry(True)
   views=rpc('workspace.list',gui=True)['views'];rpc('workspace.focus',{'id':views[-1]['id']},gui=True)
   eventually(lambda:selected()['inside_viewport']);mark('overflow-reveals-selected-last-tab')
   # Pointer coordinates map exactly onto the maximized window.
   geometry(False);views=rpc('workspace.list',gui=True)['views'];rpc('workspace.focus',{'id':views[-1]['id']},gui=True)
   eventually(lambda:selected()['inside_viewport']and chrome()['tab_layout']['arrows'][0]['left_sensitive'],10)
   arrows=chrome()['tab_layout']['arrows'][0];assert arrows['visible']and arrows['left_sensitive']
   save('arrows-before',chrome())
   # Physical pointer clicks on the arrows are owner acceptance; drive the real button.
   for _ in range(count):
    if not chrome()['tab_layout']['arrows'][0]['left_sensitive']:break
    command('chrome',press='tab-scroll-left-0');time.sleep(.03)
   save('arrows-after',chrome())
   eventually(lambda:tabs()[0]['inside_viewport']);assert not chrome()['tab_layout']['arrows'][0]['left_sensitive']
   assert panes()==baseline;mark('overflow-arrows-scroll-the-tab-row')
  command('chrome',two_tab_rows=True,compact_tabs=True)
  saved=json.loads(prefs_path.read_text());assert saved['two_tab_rows']and saved['compact_tabs']
  for k in ['term_font','term_size_pt','width','collapsed_groups']:assert saved[k]==old[k],(k,saved)
  assert prefs_path.stat().st_mode&0o777==0o600
  gui.terminate();gui.wait(timeout=8)
  gui=launch(count);state=chrome();assert state['two_tab_rows']and state['compact_tabs'];assert panes()==baseline
  mark('preferences-survive-GUI-reopen-and-preserve-old-settings',count=count)
  gui.terminate();gui.wait(timeout=8)
 (Path(a.evidence)/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'owner_desktop_or_sessions_used':False,'scope':'Generated PTYs, owned native compositor; physical owner acceptance pending'},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
