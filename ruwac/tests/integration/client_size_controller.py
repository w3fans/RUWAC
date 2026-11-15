#!/usr/bin/env python3
"""Real owned PTYs: explicit sizing ownership, stale leases and cold recovery."""
import argparse
import hashlib
import fcntl
import signal
import struct
import termios
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import traceback
from spike import Client, Tui, eventually


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--legacy', type=Path)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='sc-'));root.chmod(0o700)
    binary = args.binary.resolve();sock = root / 's';state = root / 'state-checkpoint.json'
    fixture = Path(__file__).resolve().parents[1] / 'fixtures/sizing_receiver.py'
    env = dict(os.environ, RUWAC_STATE_FILE=str(state))
    for key, leaf in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:
        (root/leaf).mkdir(mode=0o700);env[key]=str(root/leaf)
    daemon = None;results=[];clients=[];tuis=[];total=0
    def record(name):
        results.append({'id':name,'status':'PASS'});print(name,'PASS',flush=True)
    def start(name):
        nonlocal daemon
        with (args.evidence/(name+'.log')).open('wb') as log:
            daemon=subprocess.Popen([str(binary),'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log)
        def ready():
            try:
                with Client(sock) as c:return c.call('health')['ok']
            except (OSError,EOFError):return False
        eventually(ready,10)
    def client():
        c=Client(sock);clients.append(c);return c
    def probe(admin,pane):
        nonlocal total
        total+=1;admin.call('pane.sendText',{'pane':pane,'text':'?'})
        eventually(lambda: (root/'state.json').exists() and json.loads((root/'state.json').read_text())['bytes']==total)
        return json.loads((root/'state.json').read_text())
    def claim(c,sid,g,takeover=False):
        return c.call('session.sizeClaim',{'session':sid,'expected_generation':g,'takeover':takeover})
    try:
        start('first');admin=client();owner=client();viewer=client();old=client()
        invalid=subprocess.run([str(binary),'--socket',str(sock),'session','create','-n','invalid generated','-Y','invalid','-d','-j'],capture_output=True,timeout=5)
        assert invalid.returncode!=0 and admin.call('session.list')==[]
        created=admin.call('session.create',{'name':'Generated controller','size_policy':'controller_owned','rows':42,'cols':120,'argv':[sys.executable,str(fixture),str(root)]})
        sid,pane,window,pid=(created[k] for k in ['session','pane','window','pid'])
        admin.call('pane.wait',{'pane':pane,'contains':'SIZING_READY','timeout_ms':5000})
        owner.call('session.attach',{'session':sid,'rows':42,'cols':120})
        baseline=probe(admin,pane);status=admin.call('session.size',{'session':sid});g0=status['generation']
        assert status['policy']=='controller_owned' and status['controller_present'] is False
        viewer.call('session.attach',{'session':sid,'rows':18,'cols':44,'size_role':'viewer'})
        old.call('session.attach',{'session':sid,'rows':25,'cols':80})
        assert old.call('client.resize',{'rows':12,'cols':30})['resized'] is False
        after=probe(admin,pane);assert after['pid']==pid and after['signals']==baseline['signals'] and (after['rows'],after['cols'])==(42,120)
        record('CONTROL-PLAIN-OLD-ATTACH-NEVER-ACQUIRES-OR-RESIZES')
        revision=admin.call('health')['revision']
        for _ in range(32):admin.call('session.size',{'session':sid})
        assert admin.call('health')['revision']==revision
        record('CONTROL-STATUS-QUERY-DOES-NOT-DIRTY-CHECKPOINTS')
        current=claim(owner,sid,g0);g1=current['generation'];assert g1!=g0
        assert probe(admin,pane)['signals']==baseline['signals']
        owner.call('session.sizeClaim',{'session':sid,'expected_generation':g1,'takeover':False},error='SIZE_ALREADY_OWNED')
        viewer.call('session.sizeClaim',{'session':sid,'expected_generation':g0,'takeover':True},error='SIZE_STALE')
        viewer.call('session.sizeClaim',{'session':sid,'expected_generation':g1,'takeover':False},error='SIZE_BUSY')
        owner.call('client.resize',{'rows':40,'cols':110},error='SIZE_STALE')
        resize={'rows':40,'cols':110,'size_generation':g1}
        assert owner.call('client.resize',resize,key='owner-resize')['resized'] is True
        resized=probe(admin,pane);assert (resized['rows'],resized['cols'])==(40,110) and resized['signals']>baseline['signals'] and resized['pid']==pid
        record('CONTROL-EXPLICIT-CAS-CLAIM-AND-RESIZE')
        for _ in range(100):
            viewer.call('client.resize',{'rows':10,'cols':20})
            viewer.call('pane.layout',{'window':window,'zoom':True})
            viewer.call('session.detach');viewer.call('session.attach',{'session':sid,'rows':18,'cols':44})
        after=probe(admin,pane);assert after['signals']==resized['signals'] and (after['rows'],after['cols'])==(40,110)
        record('CONTROL-100-VIEWER-JOINS-ZOOMS-DO-NOT-VOTE')
        # Takeover is explicit, generation-bound and does not resize on its own.
        g2=claim(viewer,sid,g1,True)['generation'];assert g2!=g1
        owner.call('client.resize',resize,key='owner-resize',error='SIZE_NOT_OWNED')
        admin.call('window.create',{'session':sid,'argv':['/bin/cat']},error='SIZE_NOT_OWNED')
        before=probe(admin,pane);assert before['signals']==after['signals']
        viewer.call('client.resize',{'rows':36,'cols':100,'size_generation':g1},error='SIZE_STALE')
        viewer.call('client.resize',{'rows':36,'cols':100,'size_generation':g2})
        after=probe(admin,pane);assert (after['rows'],after['cols'])==(36,100) and after['pid']==pid
        record('CONTROL-TAKEOVER-REVOKES-OLD-CACHE-AND-LEASE')
        # Canonical splits ignore controller zoom as well as observer zoom.
        second=viewer.call('pane.create',{'window':window,'orientation':'vertical','argv':['/bin/cat'],'size_generation':g2})
        sizes=admin.call('pane.list',{'session':sid})
        for c in [viewer,owner,old]:c.call('pane.layout',{'window':window,'zoom':True})
        after_sizes=admin.call('pane.list',{'session':sid})
        assert [(p['id'],p['rows'],p['cols']) for p in sizes]==[(p['id'],p['rows'],p['cols']) for p in after_sizes]
        record('CONTROL-CANONICAL-SPLIT-IGNORES-ALL-LOCAL-ZOOM')
        other=admin.call('session.create',{'name':'Other controlled','size_policy':'controller_owned','argv':['/bin/cat']})
        other_status=admin.call('session.size',{'session':other['session']});assert other_status['generation']!=g0
        viewer.call('window.select',{'window':other['window']},error='TARGET_SESSION')
        viewer.call('session.sizeRelease',{'session':other['session'],'size_generation':g2},error='SIZE_NOT_OWNED')
        record('CONTROL-SESSION-NAMESPACE-AND-GENERATION-ISOLATED')
        tui=Tui(binary,sock,sid,args.evidence/'controller.pty',rows=26,cols=90,env=env)
        tuis.append(tui)
        eventually(lambda: b'Viewer' in tui.data)
        assert admin.call('session.size',{'session':sid})['rows']==36
        tui.send(b'\x01:size takeover\r')
        eventually(lambda: admin.call('session.size',{'session':sid})['rows']==25)
        eventually(lambda: b'Controller' in tui.data)
        fcntl.ioctl(tui.slave,termios.TIOCSWINSZ,struct.pack('HHHH',28,96,0,0));tui.p.send_signal(signal.SIGWINCH)
        eventually(lambda: admin.call('session.size',{'session':sid})['rows']==27)
        tui.send(b'\x01:size release\r')
        eventually(lambda: not admin.call('session.size',{'session':sid})['controller_present'])
        assert tui.finish()==0;tuis.remove(tui)
        # Return to the original canonical grid through another explicit claim.
        g2=claim(viewer,sid,admin.call('session.size',{'session':sid})['generation'])['generation']
        viewer.call('client.resize',{'rows':36,'cols':100,'size_generation':g2})
        record('CONTROL-REAL-TUI-TAKEOVER-RESIZE-RELEASE')
        viewer.close();clients.remove(viewer)
        eventually(lambda: not admin.call('session.size',{'session':sid})['controller_present'])
        frozen=admin.call('pane.list',{'session':sid})
        old.call('client.resize',{'rows':6,'cols':12})
        assert admin.call('pane.list',{'session':sid})==frozen
        released=admin.call('session.size',{'session':sid});g3=released['generation'];assert g3!=g2
        owner.call('session.sizeClaim',{'session':sid,'expected_generation':g2,'takeover':True},error='SIZE_STALE')
        g4=claim(owner,sid,g3)['generation']
        owner.call('session.sizeRelease',{'session':sid,'size_generation':g4})
        assert not admin.call('session.size',{'session':sid})['controller_present']
        record('CONTROL-DISCONNECT-FREEZES-EXPLICIT-RECLAIM-ONLY')
        # The exact previous legacy checkpoint shape remains readable by old readers;
        # controlled policy uses schema7 so old readers reject instead of downgrade.
        g5=claim(owner,sid,admin.call('session.size',{'session':sid})['generation'])['generation']
        admin.call('daemon.checkpoint');saved=json.loads(state.read_text())
        assert saved['schema']==7
        ss=next(s for s in saved['sessions'] if s['id']==sid)
        assert ss['size_policy']=='controller_owned' and (ss['rows'],ss['cols'])==(36,100)
        assert 'size_owner' not in ss and 'size_generation' not in ss
        assert g5 not in state.read_text()
        if args.legacy:
            legacy_socket=root/'legacy-s';legacy_env={**env,'RUWAC_STATE_FILE':str(root/'legacy-state.json')}
            with (args.evidence/'legacy-create.log').open('wb') as log:
                legacy_daemon=subprocess.Popen([str(args.legacy),'--socket',str(legacy_socket),'serve'],env=legacy_env,stdout=log,stderr=log)
            try:
                eventually(legacy_socket.exists)
                rejected=subprocess.run([str(binary),'--socket',str(legacy_socket),'session','create','-n','must not silently downgrade','-Y','controller_owned','-d','-j'],env=legacy_env,capture_output=True,timeout=10)
                assert rejected.returncode!=0 and b'no legacy fallback' in rejected.stderr,rejected.stderr
                with Client(legacy_socket) as old_admin:
                    assert old_admin.call('session.list')==[]
                    old_admin.call('daemon.stop')
                legacy_daemon.wait(timeout=5)
            finally:
                if legacy_daemon.poll() is None:legacy_daemon.terminate();legacy_daemon.wait(timeout=5)
            record('CONTROL-CLI-OLD-ENGINE-REFUSED-BEFORE-CREATE')
            old_state=root/'old-reader-checkpoint.json';old_state.write_bytes(state.read_bytes());old_state.chmod(0o600)
            with (args.evidence/'old-reader.log').open('wb') as log:
                rejected=subprocess.run([str(args.legacy),'--socket',str(root/'old-s'),'serve'],env={**env,'RUWAC_STATE_FILE':str(old_state)},stdout=log,stderr=log,timeout=10)
            assert rejected.returncode!=0 and old_state.read_bytes()==state.read_bytes()
        daemon.kill();daemon.wait(timeout=5);daemon=None
        for c in clients:c.close()
        clients.clear();start('restored');admin=client();owner=client()
        restored=admin.call('session.size',{'session':sid})
        assert restored['policy']=='controller_owned' and not restored['controller_present'] and restored['generation']!=g5
        assert (restored['rows'],restored['cols'])==(36,100)
        owner.call('session.attach',{'session':sid,'rows':10,'cols':20})
        owner.call('session.sizeClaim',{'session':sid,'expected_generation':g5,'takeover':True},error='SIZE_STALE')
        assert admin.call('session.size',{'session':sid})['cols']==100
        owner.call('pane.sendText',{'pane':pane,'text':'must not launch'},error='PANE_EXITED')
        record('CONTROL-CRASH-RESTORES-POLICY-GRID-NOT-LEASE-OR-PROCESS')
        admin.call('daemon.stop');daemon.wait(timeout=5);daemon=None
    except Exception:
        results.append({'id':'UNHANDLED','status':'FAIL','traceback':traceback.format_exc()});raise
    finally:
        for t in tuis:t.finish('term')
        for c in clients:c.close()
        if daemon is not None and daemon.poll() is None:daemon.terminate();daemon.wait(timeout=5)
        (args.evidence/'result.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'cases':results,'PASS':len(results)==(11 if args.legacy else 10) and all(c['status']=='PASS' for c in results)},indent=2)+'\n')

if __name__=='__main__':main()
