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
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);service='org.gnome.Mutter.RemoteDesktop';session=None
try:
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 session=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
 def seat(method,args=None):return bus.call_sync(service,session,service+'.Session',method,args,None,Gio.DBusCallFlags.NONE,5000,None)
 seat('Start');pointer(1,400);press(194)
 seeds=[rpc('session.create',{'name':'generated-'+str(i),'argv':['/bin/sleep','600']})['session']for i in range(12)]
 baseline=panes()
 args=[a.gui,'--new-instance','--socket',sock,'--allow-workspace-control','--fixture-dir',r]
 for seed in seeds:args+=['--session',seed]
 gui=start(args,'gui');eventually(lambda:command('inspect').get('tabs')==12,15)
 command('chrome',maximized=True,collapse=True);time.sleep(.5);pointer(1,400)
 def positions():return [(t['id'],t['origin'],t['width'],t['height'])for t in tabs()]
 before=positions();save('before-hover',chrome())
 for index in [4,7,2,10]:
  t=tabs()[index];x,y=t['origin'];pointer(x+t['width']/2,y+t['height']/2);time.sleep(.15)
  after=positions();save('hover-'+str(index),chrome())
  assert after==before,('TAB_MOVED_ON_HOVER',before,after)
 pointer(1,400);assert positions()==before;mark('hover-does-not-move-tabs')
 for index in [3,7,1]:
  click(tabs()[index]);assert positions()==before,'TAB_MOVED_ON_SELECTION'
 assert all(t['close_visible']and t['close_width']>0 for t in tabs());mark('inactive-close-buttons-stay-visible-and-selection-keeps-geometry')
 for state in [True,False,True,False]:
  command('tab-presentation',tab=4,title='Working on a much longer generated title'if state else'box',loading=state,indicator=state)
  time.sleep(.15);assert positions()==before,'TAB_MOVED_ON_STATUS_OR_TITLE'
 mark('status-loading-and-title-updates-do-not-move-tabs')
 for n in range(12):command('tab-presentation',tab=n,title='box'if n==0 else'Long generated machine name '+str(n))
 # Only a structural change recalculates name-based widths.
 click(chrome()['tab_new_terminal']);eventually(lambda:len(tabs())==13)
 command('close-tab',tab=12);eventually(lambda:len(tabs())==12);time.sleep(.25)
 assert tabs()[0]['width']<tabs()[1]['width'];mark('short-names-use-shorter-tabs-after-structural-change')
 for compact in [False,True]:
  command('chrome',compact_tabs=compact);time.sleep(.2);before=positions();fonts=[t['font_size']for t in tabs()]
  if not compact:normal_fonts=fonts
  else:assert fonts==normal_fonts,'COMPACT_CHANGED_FONT_SIZE'
  assert all(t['icon_size']==16 and t['indicator_size']==16 and t['close_visible']for t in tabs())
  key(56,True);eventually(lambda:all(t['hint_visible']for t in tabs()[:10]))
  assert [t['hint']for t in tabs()[:10]]==[str(n)for n in range(1,10)]+['0']
  assert not any(t['hint_visible']for t in tabs()[10:]);time.sleep(.2);assert positions()==before,'ALT_HINTS_MOVED_TABS'
  shot=command('snapshot');assert shot['saved'];(r/'native-rust.png').replace(r/('alt-hints-'+str(compact)+'.png'))
  press(4);assert selected()['id']==tabs()[2]['id'];key(56,False);eventually(lambda:not any(t['hint_visible']for t in tabs()))
  assert positions()==before;assert set(baseline.items())<=set(panes().items());mark('native-Alt-hints-and-tab-switch-keep-geometry',compact=compact)
 press(100);before=selected()['id'];key(56,True);time.sleep(.2);assert not any(t['hint_visible']for t in tabs());press(4);assert selected()['id']==before;key(56,False);press(100)
 mark('Map-keys-does-not-show-local-tab-hints-or-steal-Alt-digits')
 command('chrome',two_tab_rows=True);time.sleep(.2);before=positions();key(56,True);eventually(lambda:tabs()[0]['hint_visible']);assert positions()==before;key(56,False)
 assert set(baseline.items())<=set(panes().items());mark('two-rows-preserve-geometry-and-original-PTYs')
 # Exercise a permanently visible inactive X through the real native pointer.
 save('before-X',chrome());shot=command('snapshot');assert shot['saved'];(r/'native-rust.png').replace(r/'before-X.png')
 victim=tabs()[-1];active=selected()['id'];cx,cy=victim['close_origin']
 pointer(cx+victim['close_width']/2,cy+victim['close_height']/2);button();pointer(900,300);button(down=False);time.sleep(.2)
 assert len(tabs())==12 and selected()['id']==active,'Releasing outside X must cancel close'
 pointer(cx+victim['close_width']/2,cy+victim['close_height']/2);button();button(down=False)
 eventually(lambda:len(tabs())==11);assert victim['id']not in [t['id']for t in tabs()]and selected()['id']==active
 assert set(baseline.items())<=set(panes().items());mark('native-inactive-X-closes-only-its-view-and-preserves-PTYs')
 pinned=tabs()[4]['id'];normal=tabs()[4]['width'];command('tab-presentation',tab=4,pinned=True);time.sleep(.25)
 assert tabs()[0]['id']==pinned and not tabs()[0]['close_visible']and tabs()[0]['width']<normal
 command('tab-presentation',tab=0,pinned=False);time.sleep(.25);assert tabs()[0]['close_visible'];assert set(baseline.items())<=set(panes().items())
 # Use the actual popover and native key dispatch, not the action-group fixture.
 # GTK closes the popover before activating its selected GMenu action.
 count=len(tabs());cancel=tabs()[2];click(cancel,273);press(1);time.sleep(.2)
 assert len(tabs())==count,'Dismissing the tab menu must not close a tab'
 victim=tabs()[4];active=selected()['id'];click(victim,273)
 save('native-menu-open',chrome());assert command('snapshot')['saved'];(r/'native-rust.png').replace(r/'native-menu-open.png')
 menu=chrome()['tab_layout']['menu_items'];save('native-menu-items',menu)
 item=next(i for i in menu if i['label']=='Close tab');assert item['sensitive']
 x,y=item['origin'];pointer(x+item['width']/2,y+item['height']/2);button();button(down=False)
 assert command('snapshot')['saved'];(r/'native-rust.png').replace(r/'native-menu-activated.png')
 eventually(lambda:len(tabs())==count-1,5)
 assert victim['id']not in[t['id']for t in tabs()],'Native tab menu closed the wrong page'
 assert selected()['id']==active and set(baseline.items())<=set(panes().items())
 mark('pinned-tabs-retain-compact-presentation-and-session-identity')
 (Path(a.evidence)/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'owner_desktop_or_sessions_used':False,'scope':'Real isolated native pointer/key events and actual TabPage metadata changes; no owner acceptance claim'},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
