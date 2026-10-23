#!/usr/bin/env python3
"""Reversible per-user installation of built RUWAC artifacts; never allocates a version."""
import argparse, fcntl, hashlib, json, os, platform, re, shutil, stat, subprocess, tempfile, tomllib
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('-b', '--build-root',type=Path,required=True);p.add_argument('-d', '--desktop',action='store_true');p.add_argument('-p', '--prefix',type=Path);p.add_argument('-t', '--target-dir',type=Path,help='Cargo target directory for source builds (default: BUILD_ROOT/target)');a=p.parse_args()
root=a.build_root.resolve();prefix=(a.prefix or Path.home()/'.local').absolute()
version=tomllib.loads((root/'ruwac/Cargo.toml').read_text())['workspace']['package']['version']
if not re.fullmatch(r'(0|[1-9][0-9]*)\.[0-9]\.([0-9]|[1-9][0-9])',version):raise SystemExit('Invalid controller-assigned numeric version')
bundle=None
if (root/'bundle.json').exists() or (root/'bundle.json').is_symlink():
 if a.target_dir is not None:raise SystemExit('Bundle installations cannot override attested artifact paths')
 path=root/'bundle.json'
 if path.is_symlink() or not path.is_file() or path.stat().st_size>1024*1024:raise SystemExit('Unsafe bundle receipt')
 bundle=json.loads(path.read_text())
 if bundle.get('schema')!=1 or bundle.get('kind')!='fedora-testing-bundle' or bundle.get('version')!=version or not re.fullmatch(r'[0-9a-f]{40}',bundle.get('source_commit','')):raise SystemExit('Invalid candidate bundle identity')
 if bundle.get('distribution')!='fedora' or bundle.get('distribution_version')!='44' or bundle.get('architecture')!=platform.machine() or 'ID=fedora\n'not in Path('/etc/os-release').read_text() or 'VERSION_ID=44\n'not in Path('/etc/os-release').read_text():raise SystemExit('Candidate bundle requires Fedora 44 and its declared architecture')
 if bundle.get('security_qualified')is not False or bundle.get('known_advisories')!=['RUSTSEC-2023-0071']:raise SystemExit('Unknown bundle qualification; preserve it for review')
 expected=bundle.get('files')
 if not isinstance(expected,dict)or len(expected)>2000:raise SystemExit('Invalid bundle inventory')
 if any(path.is_symlink()for path in root.rglob('*')):raise SystemExit('Candidate bundles cannot contain symlinks')
 actual={str(path.relative_to(root))for path in root.rglob('*')if path.is_file()and path!=root/'bundle.json'}
 if actual!=set(expected):raise SystemExit('Bundle file inventory differs from its receipt')
 for name,digest in expected.items():
  path=root/name
  if Path(name).is_absolute()or '..'in Path(name).parts or path.is_symlink()or not path.resolve().is_relative_to(root)or not re.fullmatch(r'[0-9a-f]{64}',digest)or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise SystemExit('Bundle file integrity failed')
 if bundle.get('artifacts')!={name:expected.get('target/release/'+name)for name in ['ruwac','ruwac-desktop']}:raise SystemExit('Bundle artifact inventory mismatch')
 print('Installing a Fedora testing candidate. Checksums verify archive integrity, not a signature. Known desktop advisory RUSTSEC-2023-0071 remains unresolved.',file=__import__('sys').stderr)
if any(c=='='or ord(c)<32 for c in str(prefix)):raise SystemExit('Unsupported desktop executable path')
files=['ruwac']+(['ruwac-desktop'] if a.desktop else [])
target=(a.target_dir.resolve() if a.target_dir is not None else root/'target')
artifacts={name:target/'release'/name for name in files}
for name,path in artifacts.items():
 if not path.is_file()or path.is_symlink():raise SystemExit(f'Expected built artifact: {name}')
 output=subprocess.check_output([str(path),'--version'],text=True).strip()
 if not output.endswith(' '+version):raise SystemExit('Artifact version does not match source')
base=prefix/'lib/ruwac';base.mkdir(parents=True,exist_ok=True)
if base.is_symlink() or base.stat().st_uid!=os.geteuid():raise SystemExit('Unsafe installation directory')
lock=os.open(base/'.install.lock',os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600);m=os.fstat(lock)
if not stat.S_ISREG(m.st_mode)or m.st_uid!=os.geteuid()or m.st_nlink!=1 or m.st_mode&0o077:raise SystemExit('Unsafe installation lock')
try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
except BlockingIOError:raise SystemExit('Another RUWAC installation is active')
# Refuse before writing anything: a refused install must not leave a partial
# immutable release behind that blocks the next, correct attempt.
if not a.desktop and (prefix/'bin'/'ruwac-desktop').is_symlink():raise SystemExit('Retain the desktop installation with --desktop')
release=base/'releases'/version;release.parent.mkdir(exist_ok=True)
if release.parent.is_symlink():raise SystemExit('Unsafe releases directory')
resources={}
for name in ['ruwac.1'] + (['si.z4.ruwac.svg'] if a.desktop else []):
 source=root/'ruwac/packaging'/name
 if source.is_file()and not source.is_symlink():resources[name]=source
resource_hashes={name:hashlib.sha256(path.read_bytes()).hexdigest()for name,path in resources.items()}
manifest={'version':version,'artifacts':{name:hashlib.sha256(path.read_bytes()).hexdigest()for name,path in artifacts.items()},'source_commit':bundle['source_commit']if bundle else subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip(),'dirty':False if bundle else bool(subprocess.check_output(['git','-C',str(root),'diff','--name-only'],text=True).strip())}
if resources:manifest['resources']=resource_hashes
if bundle:manifest['distribution']='fedora-testing-bundle'
receipt=release/'installation.json'
if release.exists():
 if release.is_symlink() or not receipt.is_file() or json.loads(receipt.read_text())!=manifest:raise SystemExit('Existing immutable installation differs; preserve it and use a new controller-assigned release')
 for name,hash in manifest['artifacts'].items():
  if hashlib.sha256((release/name).read_bytes()).hexdigest()!=hash:raise SystemExit('Existing installation artifact has changed')
 for name,digest in resource_hashes.items():
  if hashlib.sha256((release/'resources'/name).read_bytes()).hexdigest()!=digest:raise SystemExit('Existing installation resource has changed')
else:
 temp=Path(tempfile.mkdtemp(prefix='.install-',dir=release.parent))
 try:
  for name,path in artifacts.items():shutil.copyfile(path,temp/name);(temp/name).chmod(0o755)
  if resources:
   (temp/'resources').mkdir()
   for name,path in resources.items():shutil.copyfile(path,temp/'resources'/name);(temp/'resources'/name).chmod(0o644)
  if bundle:
   shutil.copytree(root/'notices',temp/'notices')
   shutil.copyfile(root/'LICENSE',temp/'LICENSE')
   shutil.copyfile(root/'bundle.json',temp/'bundle.json')
  (temp/'installation.json').write_text(json.dumps(manifest,indent=2)+'\n');(temp/'installation.json').chmod(0o600)
  temp.rename(release)
 finally:
  if temp.exists():shutil.rmtree(temp)
def link(path,target,allowed):
 if path.exists() or path.is_symlink():
  if not path.is_symlink() or str(path.readlink())not in allowed:raise SystemExit(f'Refusing unrelated installed path: {path}')
 tmp=path.with_name(path.name+'.ruwac-new');
 if tmp.exists()or tmp.is_symlink():raise SystemExit(f'Installation lock conflict: {tmp}')
 tmp.symlink_to(target);tmp.replace(path)
current=base/'current';old=str(current.readlink())if current.is_symlink()else None
if current.exists()and not current.is_symlink():raise SystemExit('Refusing unrelated current path')
if old and (not old.startswith('releases/')or '..'in Path(old).parts):raise SystemExit('Unsafe current link')
bin=prefix/'bin';bin.mkdir(parents=True,exist_ok=True)
if bin.is_symlink()or bin.stat().st_uid!=os.geteuid():raise SystemExit('Unsafe command directory')
if not a.desktop and (bin/'ruwac-desktop').is_symlink():raise SystemExit('Retain the desktop installation with --desktop')
# Check all externally visible paths before changing current.
for name in files:
 path=bin/name;target=str(Path('..')/'lib/ruwac/current'/name)
 if (path.exists()or path.is_symlink())and (not path.is_symlink()or str(path.readlink())!=target):raise SystemExit(f'Command collision: {name}')
apps=prefix/'share/applications';launcher=apps/'si.z4.ruwac.desktop'
# Desktop Entry string escaping precedes Exec argument unquoting.
encoded=''.join(('\\'*4 if c=='\\' else '\\'*2+c if c in '"$`' else '%%' if c=='%' else c)for c in str(bin/'ruwac'))
text='[Desktop Entry]\nType=Application\nName=RUWAC\nComment=Persistent terminals, remote desktops and files\nExec="'+encoded+'" gui\nIcon=si.z4.ruwac\nKeywords=terminal;ssh;rdp;remote desktop;multiplexer;screen;tmux;sftp;Codex;Claude;Kimi;\nTerminal=false\nCategories=System;TerminalEmulator;RemoteAccess;\nStartupNotify=true\n'

if a.desktop and apps.is_symlink():raise SystemExit('Unsafe launcher directory')
legacy_text=text.replace('Icon=si.z4.ruwac\nKeywords=terminal;ssh;rdp;remote desktop;multiplexer;screen;tmux;sftp;Codex;Claude;Kimi;','Icon=utilities-terminal')
if a.desktop and launcher.exists()and launcher.read_text()not in {text,legacy_text}:raise SystemExit('Refusing unrelated desktop launcher')
resource_links=[]
for name,relative in [('ruwac.1','man/man1/ruwac.1'),('si.z4.ruwac.svg','icons/hicolor/scalable/apps/si.z4.ruwac.svg')]:
 if name not in resources:continue
 path=prefix/'share'/relative
 parent=path.parent
 # Refuse redirected installation directories, including intermediate parents.
 chain=[]
 while parent!=prefix:
  chain.append(parent);parent=parent.parent
 for parent in reversed(chain):
  if parent.is_symlink()or(parent.exists()and(not parent.is_dir()or parent.stat().st_uid!=os.geteuid())):raise SystemExit('Unsafe resource directory')
  parent.mkdir(exist_ok=True)
 allowed=set()
 if path.exists()or path.is_symlink():
  if not path.is_symlink():raise SystemExit('Refusing unrelated resource')
  prior=path.resolve()
  if not prior.is_relative_to((base/'releases').resolve())or prior.name!=name or prior.parent.name!='resources':raise SystemExit('Refusing unrelated resource link')
  prior_receipt=json.loads((prior.parent.parent/'installation.json').read_text())
  if hashlib.sha256(prior.read_bytes()).hexdigest()!=prior_receipt.get('resources',{}).get(name):raise SystemExit('Installed resource changed')
  allowed.add(str(path.readlink()))
 resource_links.append((path,os.path.relpath(release/'resources'/name,path.parent),allowed))
previous=base/'previous'
if previous.is_symlink()and (not str(previous.readlink()).startswith('releases/')or '..'in previous.readlink().parts):raise SystemExit('Unsafe previous link')
if old and old!='releases/'+version:link(base/'previous',old,{str((base/'previous').readlink())}if(base/'previous').is_symlink()else set())
link(current,'releases/'+version,{old}if old else set())
for name in files:link(bin/name,str(Path('..')/'lib/ruwac/current'/name),{str(Path('..')/'lib/ruwac/current'/name)})
for path,target,allowed in resource_links:link(path,target,allowed)
if a.desktop:
 apps.mkdir(parents=True,exist_ok=True)
 if apps.is_symlink()or apps.stat().st_uid!=os.geteuid():raise SystemExit('Unsafe launcher directory')
 launcher.write_text(text);launcher.chmod(0o644)
 if shutil.which('update-desktop-database'):subprocess.run(['update-desktop-database',str(apps)],check=True)
print(json.dumps({'installed':version,'command':str(bin/'ruwac'),'launcher':str(launcher)if a.desktop else None,'receipt':str(receipt),'previous':old},indent=2))
