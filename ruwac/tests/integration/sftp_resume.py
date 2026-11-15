#!/usr/bin/env python3
"""Real libssh2 resume and encrypted-key acceptance on owned sshd/private Secret Service."""
import argparse,hashlib,json,os,pwd,signal,socket,subprocess,sys,tempfile,time,uuid
from pathlib import Path
from spike import Client,eventually
p=argparse.ArgumentParser();p.add_argument('--driver',required=True);p.add_argument('--evidence',type=Path,required=True);p.add_argument('--binary');p.add_argument('--gui');a=p.parse_args();started=time.monotonic()
root=Path(tempfile.mkdtemp(prefix='sftp-resume-',dir=a.evidence));root.chmod(0o700);remote=root/'remote';remote.mkdir(mode=0o700);results=[];server=None;provider=None;bus=None;gui=None;daemon=None
auth_root=Path(tempfile.mkdtemp(prefix='ruwac-sftp-auth-',dir=Path('/run/user')/str(os.getuid())));auth_root.chmod(0o700);authorized=auth_root/'authorized_keys'
env={**os.environ,'GTK_A11Y':'none','GSETTINGS_BACKEND':'memory','GTK_USE_PORTAL':'0'}
for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')]:env[key]=str(root/name);Path(env[key]).mkdir(mode=0o700)
profile_path=root/'profile.json'
def invoke(action,*args,profile_file=None):
 return json.loads(subprocess.check_output([a.driver,action,str(profile_file or profile_path),*map(str,args)],env=env,text=True,timeout=90))
def pending():
 r=invoke('pending');assert r['ok'],r;return r['result']
def only_job():
 rows=pending();assert len(rows)==1,rows;return rows[0]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def paused(action,local,target,overwrite='skip'):
 r=invoke(action,local,target,overwrite,1048576);assert not r['ok'] and 'paused' in r['error'],r
 job=only_job();part=(target if action=='resume-upload' else local).with_name('ruwac-part-'+job['id']);assert 0<part.stat().st_size<(local if action=='resume-upload' else target).stat().st_size
 journal=Path(env['XDG_STATE_HOME'])/'RUWAC/transfers'/f"{job['id']}.json";assert journal.stat().st_mode&0o777==0o600 and journal.parent.stat().st_mode&0o777==0o700
 return job,part
def start_server():
 global server
 server=subprocess.Popen(['/usr/sbin/sshd','-D','-e','-f',str(root/'sshd.conf')],stdout=(root/'sshd.log').open('ab'),stderr=subprocess.STDOUT,start_new_session=True)
 def ready():
  assert server.poll() is None,'owned sshd terminated'
  try:
   with socket.create_connection(('127.0.0.1',port),.1):return True
  except OSError:return False
 eventually(ready)
def stop_server():
 if server is None or server.poll() is not None:return
 parents={}
 for proc in Path('/proc').iterdir():
  if not proc.name.isdigit():continue
  try:parents[int(proc.name)]=int(next(l.split()[1] for l in (proc/'status').read_text().splitlines() if l.startswith('PPid:')))
  except (OSError,StopIteration):pass
 descendants=set();front={server.pid}
 while front:
  front={pid for pid,parent in parents.items() if parent in front};descendants.update(front)
 for pid in descendants:
  try:
   proc=Path('/proc')/str(pid)
   if proc.stat().st_uid==os.geteuid() and (proc/'comm').read_text().strip().startswith('sshd'):os.kill(pid,signal.SIGTERM)
  except (OSError,ProcessLookupError):pass
 server.terminate();server.wait(timeout=8)
try:
 for name in ['host','client']:
  subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/name)],check=True);(root/name).chmod(0o600)
 authorized.write_text((root/'client.pub').read_text());authorized.chmod(0o600)
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 (root/'sshd.conf').write_text(f'ListenAddress 127.0.0.1\nPort {port}\nHostKey {root}/host\nPidFile {root}/sshd.pid\nAuthorizedKeysFile {authorized}\nStrictModes yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nPermitRootLogin no\nAllowUsers {pwd.getpwuid(os.getuid()).pw_name}\nSubsystem sftp internal-sftp\nLogLevel ERROR\n')
 host=(root/'host.pub').read_text().split();known=root/'known_hosts';known.write_text(f'[127.0.0.1]:{port} {host[0]} {host[1]}\n');known.chmod(0o600)
 profile={'id':'endpoint_'+uuid.uuid4().hex,'machine_id':'machine_'+uuid.uuid4().hex,'name':'Resume fixture','group':'Files','protocol':'SSH','server':f'127.0.0.1:{port}','username':pwd.getpwuid(os.getuid()).pw_name,'secret_reference':'generated-unused','favorite':False,'last_used':0,'tags':[],'preferences':{'identity_file':str(root/'client'),'known_hosts':str(known)}}
 profile_path.write_text(json.dumps(profile));profile_path.chmod(0o600);start_server()
 source=root/'source ž; $(literal).bin';source.write_bytes(bytes(range(256))*(8*1024*1024//256));expected=digest(source)
 target=remote/'target "quoted".bin';target.write_bytes(b'original remote file')
 job,part=paused('resume-upload',source,target,'overwrite');offset=part.stat().st_size;assert target.read_bytes()==b'original remote file'
 tick=time.monotonic();r=invoke('resume',job['id']);resume_ms=(time.monotonic()-tick)*1000;assert r['ok'] and r['result']['resumed_from']==offset and r['result']['sha256']==expected,r
 assert digest(target)==expected and not part.exists() and not pending()
 results.append({'id':'SFTP-RESUME-UPLOAD-OVERWRITE','status':'PASS','resumed_bytes':offset,'sha256':expected,'original_preserved_until_commit':True,'resume_elapsed_ms':resume_ms,'source_bytes':source.stat().st_size})
 local=root/'download.bin';local.write_bytes(b'original local file');job,part=paused('resume-download',local,target,'overwrite');offset=part.stat().st_size;assert local.read_bytes()==b'original local file'
 r=invoke('resume',job['id']);assert r['ok'] and r['result']['resumed_from']==offset and digest(local)==expected,r
 assert not part.exists() and not pending()
 results.append({'id':'SFTP-RESUME-DOWNLOAD-OVERWRITE','status':'PASS','resumed_bytes':offset,'source_and_destination_hash_match':True})
 target2=remote/'tamper.bin';job,part=paused('resume-upload',source,target2);data=part.read_bytes();part.write_bytes(b'X'+data[1:]);r=invoke('resume',job['id']);assert not r['ok'] and 'prefix' in r['error'],r
 assert not target2.exists() and part.exists();r=invoke('discard',job['id']);assert r['ok'] and not part.exists() and not pending(),r
 results.append({'id':'SFTP-RESUME-TAMPER-DISCARD','status':'PASS','changed_prefix_rejected':True,'only_owned_partial_removed':True})
 job,part=paused('resume-upload',source,target2);before=source.stat();data=source.read_bytes();source.write_bytes(b'Y'+data[1:]);os.utime(source,ns=(before.st_atime_ns,before.st_mtime_ns))
 r=invoke('resume',job['id']);assert not r['ok'] and 'Source bytes changed' in r['error'],r
 assert part.exists() and not target2.exists();assert invoke('discard',job['id'])['ok'];source.write_bytes(data)
 target2.write_bytes(b'reviewed original');job,part=paused('resume-upload',source,target2,'overwrite');target2.write_bytes(b'owner changed destination')
 r=invoke('resume',job['id']);assert not r['ok'] and 'Destination changed' in r['error'],r
 assert target2.read_bytes()==b'owner changed destination' and invoke('discard',job['id'])['ok']
 results.append({'id':'SFTP-RESUME-SOURCE-DESTINATION-GUARDS','status':'PASS','same_metadata_changed_source_rejected':True,'changed_destination_preserved':True})
 local=root/'symlink-copy';job,part=paused('resume-download',local,target);saved=root/'saved-part';part.rename(saved);victim=root/'unrelated';victim.write_bytes(b'untouched');part.symlink_to(victim)
 assert not invoke('resume',job['id'])['ok'] and not invoke('discard',job['id'])['ok'];assert victim.read_bytes()==b'untouched' and part.is_symlink()
 part.unlink();saved.rename(part);assert invoke('discard',job['id'])['ok']
 job,part=paused('resume-upload',source,target2,'overwrite');original=profile_path.read_bytes();changed=json.loads(original);changed['username']='different';profile_path.write_text(json.dumps(changed));r=invoke('pending');assert r['ok'] and len(r['result'])==1,r
 assert not invoke('resume',job['id'])['ok'] and part.exists();profile_path.write_bytes(original);assert invoke('discard',job['id'])['ok']
 results.append({'id':'SFTP-RESUME-SYMLINK-PROFILE-GUARDS','status':'PASS','unknown_symlink_preserved':True,'changed_endpoint_cannot_retarget_resume':True})
 job,part=paused('resume-upload',source,target2,'overwrite');old_config=(root/'sshd.conf').read_text();old_known=known.read_text();stop_server()
 subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/'host-replacement')],check=True)
 (root/'host-replacement').chmod(0o600)
 changed_key=(root/'host-replacement.pub').read_text().split();known.write_text(f'[127.0.0.1]:{port} {changed_key[0]} {changed_key[1]}\n');(root/'sshd.conf').write_text(old_config.replace(str(root/'host')+'\n',str(root/'host-replacement')+'\n'));start_server()
 r=invoke('resume',job['id']);assert not r['ok'] and 'Original SSH host key changed' in r['error'],r
 assert not invoke('discard',job['id'])['ok'] and part.exists();stop_server();known.write_text(old_known);(root/'sshd.conf').write_text(old_config);start_server();assert invoke('discard',job['id'])['ok']
 results.append({'id':'SFTP-RESUME-ORIGINAL-HOST-IDENTITY','status':'PASS','actually_rotated_known_host_key_still_cannot_retarget_job':True,'original_identity_restored_for_cleanup':True})
 large=root/'loss.bin';large.write_bytes(bytes(range(256))*(64*1024*1024//256));loss_target=remote/'loss-target';loss_target.write_bytes(b'original during network loss')
 child=subprocess.Popen([a.driver,'resume-upload',str(profile_path),str(large),str(loss_target),'overwrite'],env=env,stdout=subprocess.PIPE,text=True)
 try:
  eventually(lambda:any(p.stat().st_size>=1048576 for p in remote.glob('ruwac-part-transfer_*')),20);stop_server();r=json.loads(child.communicate(timeout=20)[0]);assert not r['ok'] and loss_target.read_bytes()==b'original during network loss',r
  job=only_job();part=loss_target.with_name('ruwac-part-'+job['id']);assert part.exists();offset=part.stat().st_size;assert offset>0
  start_server();r=invoke('resume',job['id']);assert r['ok'] and r['result']['resumed_from']==offset and digest(loss_target)==digest(large),r
  assert not pending() and not part.exists()
  results.append({'id':'SFTP-REAL-TRANSPORT-LOSS-RESUME','status':'PASS','sshd_connection_terminated':True,'retained_bytes':offset,'fresh_process_resume':True,'original_preserved':True})
 finally:
  if child.poll() is None:child.terminate();child.wait(timeout=8)
 stop_server();wrapper=Path(__file__).parents[1]/'fixtures/sftp_drop_reply.py';sftp_server=next(p for p in [Path('/usr/libexec/openssh/sftp-server'),Path('/usr/lib/openssh/sftp-server')] if p.is_file())
 config=root/'sshd.conf';config.write_text(config.read_text().replace('Subsystem sftp internal-sftp',f'Subsystem sftp {sys.executable} {wrapper} {root} {sftp_server}'));start_server()
 for kind in ['backup','final']:
  fault=remote/'fault-final';fault.write_bytes(b'original fault fixture');(root/('drop-'+kind+'-once')).write_text('armed')
  r=invoke('resume-upload',source,fault,'overwrite');assert not r['ok'],r;assert (root/('withheld-'+kind+'.json')).exists()
  job=only_job();assert job['phase']=='commit';backups=list(remote.glob('ruwac-backup-transfer_*'));assert len(backups)==1 and backups[0].read_bytes()==b'original fault fixture'
  if kind=='backup':assert not fault.exists()
  else:assert digest(fault)==digest(source)
  r=invoke('resume',job['id']);assert r['ok'] and digest(fault)==digest(source) and not pending(),r
  assert not list(remote.glob('ruwac-backup-transfer_*')) and not list(remote.glob('ruwac-part-transfer_*'))
  results.append({'id':'SFTP-REAL-LOST-'+kind.upper()+'-RENAME-REPLY','status':'PASS','real_OpenSSH_rename_completed_before_channel_EOF':True,'original_backup_preserved_then_cleaned':True,'resume_commit_recovery':True})
 busy_target=remote/'busy-target';job,part=paused('resume-upload',large,busy_target);child=subprocess.Popen([a.driver,'resume',str(profile_path),job['id']],env=env,stdout=subprocess.PIPE,text=True)
 try:
  def locked():
   assert child.poll() is None,'resume ended before concurrent lock check'
   return any('FLOCK' in l and l.split()[4]==str(child.pid) for l in Path('/proc/locks').read_text().splitlines())
  eventually(locked,10);r=invoke('resume',job['id']);assert not r['ok'] and 'busy' in r['error'],r
  r=invoke('discard',job['id']);assert not r['ok'] and 'busy' in r['error'],r
  r=json.loads(child.communicate(timeout=90)[0]);assert r['ok'] and digest(busy_target)==digest(large) and not pending(),r
  results.append({'id':'SFTP-REAL-CONCURRENT-JOB-LOCK','status':'PASS','live_resume_confirmed_by_kernel_flock':True,'second_resume_and_discard_rejected':True})
 finally:
  if child.poll() is None:child.terminate();child.wait(timeout=8)
 quota_target=remote/'quota-target';job,part=paused('resume-upload',source,quota_target);journal_root=Path(env['XDG_STATE_HOME'])/'RUWAC/transfers';copies=[]
 for _ in range(127):
  clone={**job,'id':'transfer_'+uuid.uuid4().hex};path=journal_root/(clone['id']+'.json');path.write_text(json.dumps(clone));path.chmod(0o600);copies.append(path)
 assert len(pending())==128;r=invoke('resume-upload',source,remote/'quota-refused','skip');assert not r['ok'] and '128 pending' in r['error'],r
 for path in copies:path.unlink()
 assert invoke('discard',job['id'])['ok'] and not pending()
 results.append({'id':'SFTP-JOURNAL-QUOTA-128','status':'PASS','scope':'One real paused transfer plus 127 synthetic journal records; resource-bound test, not 128 network transfers','new_transfer_refused_without_remote_write':True})
 # Encrypt a generated key with a pipe-supplied passphrase, never a secret argv.
 passphrase='generated-key-passphrase-'+uuid.uuid4().hex;encrypted=root/'encrypted.pem';public=root/'encrypted.pub.pem'
 subprocess.run(['openssl','genpkey','-algorithm','RSA','-pkeyopt','rsa_keygen_bits:2048','-aes-256-cbc','-pass','stdin','-out',str(encrypted)],input=passphrase+'\n',text=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True);encrypted.chmod(0o600)
 public.write_bytes(subprocess.check_output(['openssl','pkey','-in',str(encrypted),'-passin','stdin','-pubout'],input=(passphrase+'\n').encode(),stderr=subprocess.DEVNULL))
 ssh_public=subprocess.check_output(['ssh-keygen','-i','-m','PKCS8','-f',str(public)],text=True);authorized.write_text((root/'client.pub').read_text()+ssh_public);authorized.chmod(0o600)
 lines=subprocess.check_output(['dbus-daemon','--session','--fork','--print-address=1','--print-pid=1'],env=env,text=True).splitlines();env['DBUS_SESSION_BUS_ADDRESS']=lines[0];bus=int(lines[1]);control=root/'keyring';control.mkdir(mode=0o700);env['GNOME_KEYRING_CONTROL']=str(control)
 provider=subprocess.Popen(['gnome-keyring-daemon','--foreground','--components=secrets','--control-directory='+str(control),'--unlock'],env=env,stdin=subprocess.PIPE,stdout=(root/'keyring.log').open('wb'),stderr=subprocess.STDOUT);provider.stdin.write(b'generated-private-keyring-only\n');provider.stdin.close();time.sleep(1)
 reference='generated-key-'+uuid.uuid4().hex
 def set_secret(value):subprocess.run(['secret-tool','store','--label=RUWAC generated SSH key','application','RUWAC','profile',reference],input=value,text=True,env=env,check=True,stdout=subprocess.DEVNULL)
 key_profile={**profile,'preferences':{**profile['preferences'],'identity_file':str(encrypted),'identity_secret_reference':reference}};key_path=root/'key-profile.json';key_path.write_text(json.dumps(key_profile));key_path.chmod(0o600)
 set_secret('generated-wrong-passphrase');r=invoke('list',remote,profile_file=key_path);assert not r['ok'] and passphrase not in json.dumps(r),r
 set_secret(passphrase);r=invoke('list',remote,profile_file=key_path);assert r['ok'],r
 key_profile['preferences']['identity_secret_reference']='missing-generated-reference';key_path.write_text(json.dumps(key_profile));assert not invoke('list',remote,profile_file=key_path)['ok'];key_profile['preferences']['identity_secret_reference']=reference;key_path.write_text(json.dumps(key_profile))
 assert passphrase not in key_path.read_text() and passphrase not in profile_path.read_text()
 results.append({'id':'SFTP-REAL-ENCRYPTED-KEY-SECRET-SERVICE','status':'PASS','actual_encrypted_RSA_authentication':True,'wrong_and_missing_secret_rejected':True,'no_secret_in_profile_or_argv':True})
 if a.gui and a.binary:
  sock=root/'control.sock';daemon=subprocess.Popen([a.binary,'--socket',str(sock),'serve'],env=env,stdout=(root/'daemon.log').open('wb'),stderr=subprocess.STDOUT)
  def daemon_ready():
   if daemon.poll() is not None:raise RuntimeError('Owned fixture daemon exited during startup')
   try:
    with Client(sock) as c:return c.call('health')
   except (ConnectionError,FileNotFoundError):return None
  eventually(daemon_ready)
  with Client(sock) as c:session=c.call('session.create',{'name':'Files resume native','argv':['/bin/sh','-c','sleep 300']})
  gui=subprocess.Popen([a.gui,'--socket',str(sock),'--session',session['session'],'--fixture-dir',str(root)],env=env,stdout=(root/'gui.log').open('wb'),stderr=subprocess.STDOUT);seq=0
  def command(action,**kw):
   global seq
   seq+=1;tmp=root/'request.tmp';tmp.write_text(json.dumps({'seq':seq,'action':action,**kw}));tmp.replace(root/'gui-request.json')
   def read():
    try:d=json.loads((root/'gui-response.json').read_text());return d['result'] if d['seq']==seq else None
    except (OSError,ValueError):return None
   return eventually(read,10)
  command('inspect');native_target=remote/'native-target';job,part=paused('resume-upload',source,native_target)
  command('files-open',profile=profile,path=str(remote));eventually(lambda:'Remote directory ready' in command('files-inspect')['labels'],15);command('files-transfer',operation='open');eventually(lambda:len(command('files-transfer')['rows'])==1)
  command('files-transfer',operation='resume',id=job['id']);eventually(lambda:native_target.exists(),20)
  eventually(lambda:any('Transfer verified' in l for l in command('files-inspect')['labels']),20);assert digest(native_target)==digest(source) and not pending()
  results.append({'id':'SFTP-NATIVE-PENDING-RESUME','status':'PASS','native_dialog_job_row_and_resume_handler':True,'actual_SFTP_bytes_and_hash':True})
  command('tab-select',tab=0);native_ref='generated-native-key-'+uuid.uuid4().hex
  # SSH-specific controls belong to a saved SSH endpoint; a newly created
  # connection form starts as RDP. Save its protocol before opening the key UI.
  created=command('profile-editor',operation='new',fields={'0':'Encrypted SSH native','3':profile['server'],'4':profile['username'],'12':str(known),'13':1},response='save')
  assert next(p for p in created['items']if p['name']=='Encrypted SSH native')['protocol']=='SSH'
  edited=command('profile-editor',operation='edit',fields={'11':str(encrypted)},named={'ssh-key-passphrase-reference':native_ref},key_passphrase=passphrase,credential_action='Set / replace key passphrase',response='keep');assert edited['applied']
  def secret_ready():
   r=subprocess.run(['secret-tool','lookup','application','RUWAC','profile',native_ref],env=env,text=True,capture_output=True);return r.returncode==0 and r.stdout.rstrip('\n')==passphrase
  eventually(secret_ready,10);saved=command('profile-editor',operation='update',response='save');profile_saved=next(p for p in saved['items'] if p['name']=='Encrypted SSH native');assert profile_saved['preferences']['identity_secret_reference']==native_ref
  native_file=root/'native-key-profile.json';native_file.write_text(json.dumps(profile_saved));native_file.chmod(0o600);assert invoke('list',remote,profile_file=native_file)['ok'];assert passphrase not in (Path(env['XDG_CONFIG_HOME'])/'RUWAC/machines.json').read_text()
  results.append({'id':'SFTP-NATIVE-KEY-PASSPHRASE-EDITOR','status':'PASS','real_GTK_secret_store_and_profile_save':True,'actual_SSH_authentication':True,'metadata_excludes_passphrase':True})
finally:
 if gui is not None and gui.poll() is None:gui.terminate();gui.wait(timeout=10)
 if daemon is not None and daemon.poll() is None:
  try:
   with Client(root/'control.sock') as c:c.call('daemon.stop')
   daemon.wait(timeout=8)
  except Exception:daemon.kill();daemon.wait()
 if provider is not None and provider.poll() is None:provider.terminate();provider.wait(timeout=8)
 if bus is not None:
  try:os.kill(bus,signal.SIGTERM)
  except ProcessLookupError:pass
 stop_server()
 if authorized.exists():authorized.unlink()
 auth_root.rmdir()
 receipt={'elapsed_ms':(time.monotonic()-started)*1000,'results':results,'driver_sha256':hashlib.sha256(Path(a.driver).read_bytes()).hexdigest(),'scope':'Real owned OpenSSH SFTP and private Secret Service; native actions are synthetic GTK callbacks, not physical input or external-host acceptance'}
 # Successful fixtures retain receipts and diagnostics, not repeatable bulk
 # transfer bytes. Failed runs preserve their payloads for diagnosis.
 if sys.exc_info()[0] is None and len(results)==(14 if a.gui and a.binary else 12):
  removed=[]
  for relative in ['source ž; $(literal).bin','download.bin','loss.bin',
                   'remote/target "quoted".bin','remote/loss-target',
                   'remote/fault-final','remote/busy-target','remote/native-target']:
   payload=root/relative
   if payload.exists():
    assert payload.is_file() and not payload.is_symlink()
    removed.append({'path':relative,'bytes':payload.stat().st_size})
    payload.unlink()
  receipt['generated_payload_cleanup']={'removed':removed,'preserved':'All JSON, logs, keys, profiles and source; no owner data used'}
 if a.binary and a.gui:receipt['hashes']={n:hashlib.sha256(Path(path).read_bytes()).hexdigest() for n,path in [('ruwac',a.binary),('ruwac-desktop',a.gui)]}
 (root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2),flush=True)
assert len(results)==(14 if a.gui and a.binary else 12)
