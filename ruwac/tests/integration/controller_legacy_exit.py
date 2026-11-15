#!/usr/bin/env python3
"""Actual old/current TUIs, controlled canonical splits, and normal process exit."""
import argparse,json,os,shutil,subprocess,sys,tempfile,time,traceback
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--legacy',type=Path,required=True);p.add_argument('--released',type=Path);p.add_argument('--source',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args()
sys.path.insert(0,str(a.source/'tests/integration'))
from spike import Client,Tui,eventually
root=Path(tempfile.mkdtemp(prefix='cc-'));root.chmod(0o700);sock=root/'s';a.evidence.mkdir(parents=True,exist_ok=True)
env={**os.environ,'RUWAC_STATE_FILE':str(root/'state.json')}
for key,leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data')]:
 (root/leaf).mkdir();env[key]=str(root/leaf)
fixture=root/'receiver.py';fixture.write_text('import os,tty,sys\ntty.setraw(0)\nos.write(1,sys.argv[1].encode())\nwhile b"Q" not in os.read(0,128):pass\n')
def argv(marker):return [sys.executable,str(fixture),marker]
readers=[('legacy',a.legacy)]+([('released',a.released)]if a.released else [])+[('current',a.binary)]
log=(a.evidence/'daemon.log').open('wb');daemon=subprocess.Popen([str(a.binary),'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log);tuis=[];cases=[]
def record(name):cases.append({'id':name,'status':'PASS'});print(name,'PASS',flush=True)
try:
 eventually(sock.exists)
 with Client(sock)as admin,Client(sock)as owner,Client(sock)as viewer:
  for label,reader in readers:
   row=admin.call('session.create',{'name':'Generated '+label,'size_policy':'controller_owned','rows':36,'cols':110,'argv':argv('GEN_FIRST')});sid=row['session'];owner.call('session.attach',{'session':sid,'rows':36,'cols':110});g=owner.call('session.sizeClaim',{'session':sid,'expected_generation':admin.call('session.size',{'session':sid})['generation'],'takeover':False})['generation']
   second=owner.call('pane.create',{'window':row['window'],'orientation':'vertical','argv':argv('GEN_SECOND'),'size_generation':g})
   survivor=owner.call('window.create',{'session':sid,'argv':argv('GEN_SURVIVOR'),'size_generation':g})
   for pane in [row['pane'],second['pane'],survivor['pane']]:admin.call('pane.wait',{'pane':pane,'contains':'GEN_','timeout_ms':5000})
   owner.call('window.select',{'window':row['window']});viewer.call('session.attach',{'session':sid,'rows':12,'cols':30})
   before=admin.call('pane.list',{'session':sid});grid=admin.call('session.size',{'session':sid})
   tui=Tui(reader,sock,sid,a.evidence/(label+'.pty'),rows=20,cols=70,env=env);tuis.append(tui);eventually(lambda:b'GEN_SURVIVOR' in tui.data);tui.send(b'\x01p');eventually(lambda:b'GEN_FIRST' in tui.data)
   admin.call('pane.sendText',{'pane':second['pane'],'text':'Q'});eventually(lambda:next(x for x in admin.call('pane.list',{'session':sid})if x['id']==second['pane'])['ended'])
   assert tui.p.poll()is None,'one exited split killed viewer'
   sizes=admin.call('pane.list',{'session':sid});assert [(x['id'],x['rows'],x['cols'])for x in sizes]==[(x['id'],x['rows'],x['cols'])for x in before]
   record(label.upper()+'-ENDED-SPLIT-RETAINS-CANONICAL-LAYOUT')
   drawn_before=len(tui.data)
   admin.call('pane.sendText',{'pane':row['pane'],'text':'Q'});eventually(lambda:tui.p.poll()is not None or b'GEN_SURVIVOR'in tui.data[drawn_before:])
   assert tui.p.poll()is None,(label,'viewer terminated instead of selecting live sibling',tui.p.returncode,bytes(tui.data[-1000:]))
   assert b'GEN_SURVIVOR'in tui.data[drawn_before:]
   assert admin.call('session.size',{'session':sid})==grid
   panes=admin.call('pane.list',{'session':sid});assert len(panes)==3 and sum(x['ended']for x in panes)==2
   assert next(x for x in panes if x['id']==survivor['pane'])['pid']==survivor['pid'];os.kill(survivor['pid'],0)
   assert tui.finish()==0;tuis.remove(tui);record(label.upper()+'-ENDED-WINDOW-SELECTS-LIVE-WITHOUT-SHARED-PRUNE')
   # No-op cleanup cannot mutate canonical layout, dirty revision or poison an owner's key.
   revision=admin.call('health')['revision']
   for _ in range(32):
    out=viewer.call('pane.close',{'pane':second['pane'],'if_ended':True},key='generated-cleanup-'+label);assert out['retained']and out['closed']is False
   for _ in range(8):
    assert viewer.call('pane.close',{'pane':second['pane']})['retained']
   assert admin.call('health')['revision']==revision
   viewer.call('pane.close',{'pane':second['pane'],'if_ended':True},key='x'*129,error='INVALID_PARAMS')
   foreign=admin.call('session.create',{'name':'Generated foreign','size_policy':'controller_owned','argv':['/bin/true']})
   eventually(lambda:admin.call('pane.list',{'session':foreign['session']})[0]['ended'])
   viewer.call('pane.close',{'pane':foreign['pane'],'if_ended':True},error='SIZE_NOT_OWNED')
   owner.call('pane.close',{'pane':second['pane'],'if_ended':True,'size_generation':g},key='generated-cleanup-'+label)
   assert len(admin.call('pane.list',{'session':sid}))==2
   viewer.call('pane.close',{'pane':survivor['pane'],'if_ended':True},error='SIZE_NOT_OWNED')
   record(label.upper()+'-VIEWER-CLEANUP-READONLY-OWNER-KEY-UNPOISONED')
  admin.call('daemon.stop')
 daemon.wait(timeout=6)
except Exception:
 cases.append({'id':'UNHANDLED','status':'FAIL','traceback':traceback.format_exc()});raise
finally:
 for t in tuis:
  try:t.finish('term')
  except Exception:pass
 if daemon.poll()is None:daemon.terminate();daemon.wait(timeout=8)
 log.close();(a.evidence/'result.json').write_text(json.dumps({'PASS':len(cases)==len(readers)*3 and all(x['status']=='PASS'for x in cases),'cases':cases},indent=2)+'\n');shutil.rmtree(root)
