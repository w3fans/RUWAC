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
 seeds=[rpc('session.create',{'name':'generated-'+str(i),'argv':['/bin/sleep','600']})['session']for i in range(4)]
 baseline=panes()
 def full_rows(state):
  layout=state['tab_layout']
  for row in range(layout['rows']):
   members=[t for t in layout['tabs']if t['row']==row]
   if not members or not any(t['close_visible']for t in members):continue
   assert not layout['arrows'][row]['visible'],('UNEXPECTED_SMALL_TAB_OVERFLOW',row)
   gap=layout['available'][row]-layout['row_widths'][row]
   assert -2<=gap<=max(6,len(members)*3),('UNUSED_TAB_ROW',row,gap,layout['available'][row],layout['row_widths'][row])
   assert all(t['inside_viewport']for t in members),('CLIPPED_NONOVERFLOW_TAB',row)
  button=state['tab_new_terminal'];x,y=button['origin']
  assert button['mapped']and button['sensitive']and x>=0 and x+button['width']<=state['window_width']
  return True
 for count in [1,2,4]:
  for name in ['gui-request.json','gui-response.json']:(r/name).unlink(missing_ok=True)
  args=[a.gui,'--new-instance','--socket',sock,'--fixture-dir',r]
  for seed in seeds[:count]:args+=['--session',seed]
  gui=start(args,'gui-'+str(count));eventually(lambda:command('inspect').get('tabs')==count,15)
  fonts=None
  for two in [False,True]:
   for compact in [False,True]:
    command('chrome',maximized=True,collapse=True,two_tab_rows=two,compact_tabs=compact);time.sleep(.4)
    state=chrome();save('fill-'+str(count)+'-'+str(two)+'-'+str(compact),state);full_rows(state)
    positions=[(t['id'],t['origin'],t['width'],t['height'])for t in tabs()]
    current_fonts=[t['font_size']for t in tabs()]
    if fonts is None:fonts=current_fonts
    assert fonts==current_fonts
    assert all(t['icon_size']==16 and t['indicator_size']==16 and t['close_visible']for t in tabs())
    click(tabs()[-1]);key(56,True);time.sleep(.2);key(56,False)
    command('tab-presentation',tab=0,title='A longer generated title',loading=True,indicator=True);time.sleep(.15)
    assert positions==[(t['id'],t['origin'],t['width'],t['height'])for t in tabs()]
    assert set(baseline.items())<=set(panes().items())
    mark('few-tabs-fill-row-and-preserve-status-geometry',tabs=count,two_rows=two,compact=compact)
  if count>1:
   command('chrome',two_tab_rows=False,compact_tabs=False)
   command('tab-presentation',tab=0,pinned=True);time.sleep(.3)
   full_rows(chrome());assert not tabs()[0]['close_visible'];assert tabs()[0]['width']<tabs()[1]['width']
   mark('pinned-icon-leaves-rest-of-row-to-flexible-tabs',tabs=count)
  gui.terminate();gui.wait(timeout=8)
 (Path(a.evidence)/'acceptance.json').write_text(json.dumps({'PASS':True,'checks':checks,'scope':'Generated isolated native compositor: one/two/four tabs, both row modes and compact modes, full row allocation, stable title/status/selection/Alt geometry; owner desktop untouched'},indent=2)+'\n')
finally:
 if session:
  try:seat('Stop')
  except Exception:pass
 for q in reversed(children):
  if q.poll()is None:q.terminate()
 for q in reversed(children):
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
