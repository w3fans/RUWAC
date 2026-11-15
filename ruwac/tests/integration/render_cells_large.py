#!/usr/bin/env python3
"""A non-sizing viewer of independently zoomed maximum-size canonical grids."""
import argparse,fcntl,gc,json,os,struct,subprocess,tempfile,termios
from pathlib import Path
from spike import Client,Tui,eventually
p=argparse.ArgumentParser();p.add_argument('--binary',required=True,type=Path);p.add_argument('--evidence',required=True,type=Path);a=p.parse_args()
def kernel_size(pid):
 fd=os.open(f'/proc/{pid}/fd/0',os.O_RDONLY|os.O_NOCTTY)
 try:return struct.unpack('HHHH',fcntl.ioctl(fd,termios.TIOCGWINSZ,b'\0'*8))[:2]
 finally:os.close(fd)
r=Path(tempfile.mkdtemp(prefix='large-grid-',dir=a.evidence));sock=r/'s';clients=[];tui=None
with(r/'daemon.log').open('w')as out:server=subprocess.Popen([str(a.binary),'--socket',str(sock),'serve'],stdout=out,stderr=subprocess.STDOUT)
try:
 eventually(sock.exists);first=Client(sock);clients.append(first)
 row=first.call('session.create',{'name':'generated-large-viewer','rows':512,'cols':512,'argv':['/bin/bash','--noprofile','--norc','-c','printf GRID_A;exec /bin/sleep 600']})
 first.call('session.attach',{'session':row['session'],'rows':512,'cols':512})
 second=first.call('pane.create',{'window':row['window'],'orientation':'vertical','argv':['/bin/bash','--noprofile','--norc','-c','printf GRID_B;exec /bin/sleep 600']})
 other=Client(sock);clients.append(other);other.call('session.attach',{'session':row['session'],'rows':512,'cols':512})
 for c,pane in [(first,row['pane']),(other,second['pane'])]:c.call('pane.select',{'pane':pane});c.call('pane.layout',{'window':row['window'],'zoom':True})
 before=first.call('pane.list',{'session':row['session']});pids={p['id']:p['pid']for p in before};sizes={pane:kernel_size(pid)for pane,pid in pids.items()};assert set(sizes.values())=={(512,512)}
 viewer=Client(sock);clients.append(viewer);viewer.call('session.attach',{'session':row['session'],'rows':40,'cols':120,'size_role':'viewer'});viewer.call('pane.layout',{'window':row['window'],'zoom':False})
 frame=viewer.call('client.render',{'cell_encoding':'palette-v1','max_response_bytes':64*1024*1024})
 assert frame['size_role']=='viewer'and len(frame['panes'])==2
 for pane in frame['panes']:
  grid=pane['grid'];assert grid['rows']==512 and grid['cols']==512
  assert 'cell_encoding'not in grid,'Oversized aggregate must use existing plain fallback'
  assert len(grid['cells'])==512 and all(len(cells)==512 for cells in grid['cells'])
  line=''.join(cell[0]for cell in grid['cells'][0]if not cell[1]);assert 'GRID_A'in line or 'GRID_B'in line
 del frame,pane,grid;gc.collect()
 compact=first.call('client.render',{'cell_encoding':'palette-v1','max_response_bytes':64*1024*1024})
 assert len(compact['panes'])==1 and compact['panes'][0]['grid']['cell_encoding']=='palette-v1'
 del compact;gc.collect()
 # TUI starts zoomed because the sizing clients' default is zoomed; explicitly
 # toggle only this non-sizing viewer to exercise the large unzoomed frame.
 tui=Tui(a.binary,sock,row['session'],r/'viewer.ansi',rows=41,cols=120,env={**os.environ,'TERM':'xterm-256color'},command=['attach','--session',row['session'],'--viewer'])
 eventually(lambda:b'RUWAC'in tui.data,15);tui.send(b'\x01z')
 eventually(lambda:b'GRID_A'in tui.data and b'GRID_B'in tui.data,15)
 assert tui.p.poll()is None
 assert tui.finish()==0;tui=None
 after=first.call('pane.list',{'session':row['session']});assert {p['id']:p['pid']for p in after}==pids
 assert all(not p['ended']for p in after);assert {pane:kernel_size(pid)for pane,pid in pids.items()}==sizes
 (a.evidence/'acceptance.json').write_text(json.dumps({'PASS':True,'canonical_grids':2,'cells_per_grid':262144,'plain_fallback_preserves_large_view':True,'single_zoom_remains_compact':True,'actual_tui_unzoomed_viewer_stays_connected':True,'pane_pids_preserved':pids,'kernel_pty_sizes_preserved':sizes},indent=2)+'\n')
finally:
 if tui:
  try:tui.finish('term')
  except Exception:pass
 for c in clients:
  try:c.call('daemon.stop')
  except Exception:pass
  c.close()
 if server.poll()is None:server.terminate()
 server.wait(timeout=8)
