"""Owned Unix RPC proxy: delay real frozen history; optionally synthesize oversized paging for the cap test."""
import json,os,socket,struct,threading
from spike import exact

class HistoryDelay:
 def __init__(self,path,upstream,seconds,truncated=False):
  self.truncated=truncated
  self.upstream=str(upstream);self.seconds=seconds;self.stop=threading.Event();self.frozen=threading.Event();self.sockets=[];self.threads=[];self.errors=[];self.lock=threading.Lock()
  self.listener=socket.socket(socket.AF_UNIX);self.listener.bind(str(path));os.chmod(path,0o600);self.listener.listen(16);self.listener.settimeout(.2)
  self.thread=threading.Thread(target=self.accept);self.thread.start()
 def accept(self):
  while not self.stop.is_set():
   try:client,_=self.listener.accept()
   except socket.timeout:continue
   except OSError:break
   t=threading.Thread(target=self.forward,args=(client,));self.threads.append(t);t.start()
 def forward(self,client):
  upstream=socket.socket(socket.AF_UNIX)
  with self.lock:self.sockets.extend([client,upstream])
  try:
   upstream.connect(self.upstream)
   while not self.stop.is_set():
    header=exact(client,4);size=struct.unpack('!I',header)[0];assert size<=16*1024*1024
    data=exact(client,size);request=json.loads(data);upstream.sendall(header+data)
    header=exact(upstream,4);size=struct.unpack('!I',header)[0];assert size<=16*1024*1024
    data=exact(upstream,size)
    if self.truncated and request['method']=='pane.history':
     # Each response stays below the real1MiB framing cap. The real worker
     # must stop accumulating at8MiB and label the incomplete prefix.
     value=json.loads(data);value.pop('error',None);offset=request['params'].get('offset',0)
     value['result']={'epoch':request['params'].get('epoch'),'text':('T'*1023+'\n')*500,'next_offset':offset+500,'total_lines':100000}
     data=json.dumps(value).encode();header=struct.pack('!I',len(data))
    if request['method']=='pane.history.freeze':
     self.frozen.set()
     if self.seconds >= 1.5:
      # Keep a slow framed response making progress below the real RPC's
      # two-second idle timeout. This exercises the GUI's five-second copy
      # deadline rather than corrupting the persistent stream with a late
      # reply after an intentionally timed-out RPC.
      client.sendall(header)
      count=max(1,int(self.seconds/.5))
      for index in range(count):
       if self.stop.wait(self.seconds/count):return
       client.sendall(data[len(data)*index//count:len(data)*(index+1)//count])
      continue
     if self.stop.wait(self.seconds):break
    client.sendall(header+data)
  except (OSError,EOFError):pass
  except BaseException as e:self.errors.append(repr(e))
  finally:client.close();upstream.close()
 def close(self):
  self.stop.set();self.listener.close()
  self.thread.join(2);assert not self.thread.is_alive()
  with self.lock:
   for s in self.sockets:
    try:s.shutdown(socket.SHUT_RDWR)
    except OSError:pass
    s.close()
  for t in self.threads:t.join(2);assert not t.is_alive()
  assert not self.errors,self.errors
