#!/usr/bin/env python3
"""Real isolated PTY: styled mobile capture preserves colors without attaching or resizing."""
import argparse,json,os,re,signal,subprocess,sys,tempfile
from pathlib import Path
from spike import Client,eventually

parser=argparse.ArgumentParser()
parser.add_argument('--binary',required=True)
parser.add_argument('--evidence',type=Path,required=True)
args=parser.parse_args()
root=Path(tempfile.mkdtemp(prefix='mobile-capture-',dir=args.evidence));root.chmod(0o700)
sock=root/'control.sock'
env=dict(os.environ,RUWAC_STATE_FILE=str(root/'state.json'))
paint=root/'paint.py'
paint.write_text("import os,tty\nfrom pathlib import Path\ntty.setraw(0)\nos.write(1,b'\\x1b[38;2;12;34;56;48;5;24;1mRGB\\x1b[0m plain\\r\\n\\x1b[31mred\\x1b[0m\\r\\n\\x1b[31mR\\x1b[32mA\\x1b[33mI\\x1b[34mN\\x1b[35mB\\x1b[36mO\\x1b[37mW\\x1b[0m')\nwhile True:\n b=os.read(0,1)\n if not b:break\n Path(__file__).with_suffix('.input').write_bytes(b)\n")
server=None;child=None

def rpc(method,params=None,error=None):
    with Client(sock) as client:
        return client.call(method,params,error=error)

try:
    with (root/'daemon.log').open('wb') as log:
        server=subprocess.Popen([args.binary,'--socket',str(sock),'serve'],env=env,stdout=log,stderr=log)
    def ready():
        try:return rpc('health')
        except (OSError,EOFError):return None
    eventually(ready,15)
    created=rpc('session.create',{'name':'Mobile colors','rows':4,'cols':40,'argv':[sys.executable,str(paint)]})
    child=created['pid'];pane=created['pane']
    params={'pane':pane,'lines':4,'buffer':'active'}
    eventually(lambda:'RGB' in rpc('pane.capture',params)['text'],10)
    before=rpc('session.list')
    identity=Path(f'/proc/{child}/stat').read_text().rsplit(')',1)[1].split()[19]
    clients_before=rpc('health')['clients']
    plain=rpc('pane.capture',params)
    ansi=rpc('pane.capture',dict(params,format='ansi'))
    assert '\x1b' not in plain['text']
    assert ansi['format']=='ansi' and ansi['pane']==pane
    assert '38;2;12;34;56' in ansi['text'] and '48;5;24' in ansi['text']
    assert len(ansi['text'].encode())<=65536
    assert re.sub(r'\x1b\[[0-9;]*m','',ansi['text'])==plain['text']
    assert rpc('pane.wait',dict(params,contains='RGB'))['matched'] is True
    assert rpc('pane.capture',dict(params,format='ansi',contains='RAINBOW'))['matched'] is True, 'STYLED_MATCH_LOST'
    rpc('pane.capture',dict(params,format='html'),error='INVALID_PARAMS')
    rpc('pane.capture',dict(params,format=123),error='INVALID_PARAMS')
    for invalid in [None,False,[],{}]:
        rpc('pane.capture',dict(params,format=invalid),error='INVALID_PARAMS')
    rpc('pane.wait',dict(params,format='ansi'),error='INVALID_PARAMS')
    assert rpc('session.list')==before
    assert rpc('health')['clients']==clients_before==0
    assert Path(f'/proc/{child}/stat').read_text().rsplit(')',1)[1].split()[19]==identity
    assert not paint.with_suffix('.input').exists(), 'Capture sent terminal input'
    assert rpc('capabilities')['capture_formats']==['plain','ansi']
    assert rpc('capabilities')['render_cell_encodings']==['palette-v1']
    receipt={'passed':True,'real_pty':True,'attached':False,'resized':False,'sends':False,'rgb':True,'indexed_background':True,'default_plain_preserved':True,'plain_match_across_styles':True,'PID_start_preserved':True,'invalid_format_types':True,'palette_capability_preserved':True}
    (args.evidence/'mobile-capture.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print('PASS: real PTY styled capture, default plain compatibility, typed validation and unchanged session dimensions')
finally:
    if child:
        try:
            if str(paint).encode() in Path(f'/proc/{child}/cmdline').read_bytes():os.killpg(child,signal.SIGTERM)
        except (FileNotFoundError,ProcessLookupError):pass
    if server:
        try:rpc('daemon.stop')
        except Exception:server.terminate()
        try:server.wait(timeout=5)
        except subprocess.TimeoutExpired:server.kill();server.wait(timeout=5)
