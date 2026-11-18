#!/usr/bin/env python3
"""Native machine editor: a Windows RDP entry saves without selection-sharing
defaults (wayland-0, account, agent backend); private seat only."""
import argparse,json,os,subprocess,sys,tempfile,time
from pathlib import Path
import gi
gi.require_version('Gdk','4.0');gi.require_version('Gtk','4.0')
from gi.repository import GLib,Gtk
Gtk.init()
from spike import eventually
p=argparse.ArgumentParser()
for name in ['binary','gui','evidence']:p.add_argument('--'+name,type=Path,required=True)
p.add_argument('--baseline',action='store_true');a=p.parse_args();assert os.environ.get('WAYLAND_DISPLAY')=='ruwac-test'
r=Path(tempfile.mkdtemp(prefix='machine-editor-',dir=a.evidence));r.chmod(0o700)
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','SHELL':'/bin/bash'}
for name in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:
 q=r/name;q.mkdir(mode=0o700);env[name]=str(q)
for name in ['RUWAC_SOCKET','RUWAC_SESSION','RUWAC_PANE']:env.pop(name,None)
children=[];seq=0;sock=r/'c.sock'
def start(argv,name):
 q=subprocess.Popen(list(map(str,argv)),env=env,stdout=(r/(name+'.log')).open('wb'),stderr=subprocess.STDOUT);children.append(q);return q
def command(action,**kw):
 global seq
 seq+=1;tmp=r/'request.tmp';tmp.write_text(json.dumps(dict(seq=seq,action=action,**kw)));tmp.replace(r/'gui-request.json')
 def response():
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  try:q=json.loads((r/'gui-response.json').read_text());return q['result']if q['seq']==seq else None
  except(OSError,ValueError):return None
 return eventually(response,12)
def entry(items,name):
 # Empty settings are omitted when stored; read them as ''/False.
 e=next(i for i in items if i['name']==name);d={'primary_display':'','primary_user':'','agent_backend':'','primary_sync':False};d.update(e.get('preferences',{}));return {'preferences':d}
checks=[];failures=[];observed={}
def check(name,ok,detail):
 observed[name]=detail;(checks if ok else failures).append(name)
try:
 start([a.binary,'--socket',sock,'serve'],'daemon');eventually(sock.exists)
 app=start([a.gui,'--new-instance','--socket',sock,'--fixture-dir',r],'GUI')
 eventually(lambda:command('inspect') is not None,15)
 # New Windows host: only name, server and user are typed.
 command('profile-editor',operation='new',response='keep',fields={'0':'Windows host','3':'192.0.2.10','4':'Administrator'})
 eventually(lambda:any(d['width']>0 for d in command('profile-editor',operation='layout')['dialogs']),10)
 saved=command('profile-editor',operation='inspect',response='save')
 w=entry(saved['items'],'Windows host')['preferences']
 check('new RDP entry saves no selection-sharing or agent defaults',
  w['primary_display']==''and w['primary_user']==''and w['agent_backend']=='' and not saved['error'],
  {k:w[k]for k in['primary_display','primary_user','agent_backend','primary_sync']})
 # Reopen and save unchanged: the blanks stay blank.
 command('profiles',search='Windows host')
 command('profile-editor',operation='edit',response='keep')
 eventually(lambda:any(d['width']>0 for d in command('profile-editor',operation='layout')['dialogs']),10)
 again=command('profile-editor',operation='inspect',response='save')
 w=entry(again['items'],'Windows host')['preferences']
 check('re-saving an unchanged entry keeps the blanks',w['primary_display']==''and w['agent_backend']=='',{k:w[k]for k in['primary_display','agent_backend']})
 # Selection sharing still gets its default session when ticked.
 command('profile-editor',operation='new',response='keep',fields={'0':'Linux desktop','3':'192.0.2.11','4':'deskuser'},named={'primary-sync':True,'primary-user':'deskuser'})
 eventually(lambda:any(d['width']>0 for d in command('profile-editor',operation='layout')['dialogs']),10)
 linux=command('profile-editor',operation='inspect',response='save')
 w=entry(linux['items'],'Linux desktop')['preferences']
 check('selection sharing defaults its session to wayland-0',w['primary_sync']and w['primary_display']=='wayland-0',{k:w[k]for k in['primary_display','primary_user','primary_sync']})
 command('workspace-close');app.wait(timeout=8)
 if a.baseline:
  assert failures,{'baseline_unexpectedly_passed':checks};result=dict(PASS=True,baseline=True,baseline_failures=failures,observed=observed)
 else:
  assert not failures,{'failures':failures,'observed':observed};result=dict(PASS=True,checks=checks,observed=observed,owner_desktop_used=False)
 (a.evidence/'acceptance.json').write_text(json.dumps(result,indent=2)+'\n');print(f'Machine editor PASS {len(checks)} checks')
finally:
 (a.evidence/'machine-editor-observed.json').write_text(json.dumps(dict(checks=checks,failures=failures,observed=observed),indent=2)+'\n')
 for q in reversed(children):
  if q.poll()is None:q.terminate()
  try:q.wait(timeout=5)
  except subprocess.TimeoutExpired:q.kill();q.wait()
