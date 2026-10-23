#!/usr/bin/env python3
"""Check locked declarations and the independent headless dependency graph."""
import argparse,hashlib,json,subprocess,tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
BANNED=('gtk','gdk','glib','gio','libadwaita','ironrdp','ssh2','cairo','pango','rusqlite','libsqlite3','webkit','javascriptcore','soup3')
def inspect(name,manifest):
 lock=tomllib.loads(manifest.with_name('Cargo.lock').read_text());locked={(p['name'],p['version'],p.get('source')):p for p in lock['package']}
 metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--manifest-path',str(manifest)],text=True))
 rows=[]
 for p in metadata['packages']:
  source=p.get('source');key=(p['name'],p['version'],source);entry=locked.get(key)
  if entry is None:raise ValueError('Resolved package absent from lock: '+p['name'])
  license_text=p.get('license');license_file=p.get('license_file');file_hash=None
  if not license_text and not license_file:raise ValueError('Missing declared license: '+p['name'])
  if license_file:
   package_root=Path(p['manifest_path']).parent.resolve();path=(package_root/license_file).resolve()
   if not path.is_relative_to(package_root) or path.stat().st_size>2*1024*1024:raise ValueError('Declared license file outside package or oversized: '+p['name'])
   if not path.is_file():raise ValueError('Declared license file missing: '+p['name'])
   file_hash=hashlib.sha256(path.read_bytes()).hexdigest()
  if source and source.startswith('registry+') and not entry.get('checksum'):raise ValueError('Registry checksum missing: '+p['name'])
  if name=='headless' and p['name'].startswith(BANNED):raise ValueError('Graphical/embedded remote dependency in headless graph: '+p['name'])
  rows.append({'name':p['name'],'version':p['version'],'declared_license':license_text,'license_file_sha256':file_hash,'registry_checksum':entry.get('checksum')})
 return {'packages':sorted(rows,key=lambda p:(p['name'],p['version'])),'package_count':len(rows),'lock_sha256':hashlib.sha256(manifest.with_name('Cargo.lock').read_bytes()).hexdigest()}
def verify_vendor():
 base=ROOT/'ruwac/vendor/alacritty_terminal';d=json.loads((base/'UPSTREAM.json').read_text())
 if d['crate']!='alacritty_terminal' or d['version']!='0.26.0' or d['registry_archive_sha256']!='bda177466b9524d59f1b12f0dd30b68696788e9992a7e959021c4a0ed96fcf59':raise ValueError('Unexpected terminal library provenance')
 expected=d['upstream_library_files_sha256']|d['patched_files_sha256']
 actual={str(p.relative_to(base))for p in base.rglob('*')if p.is_file()and (p.suffix=='.rs' or p.name in ['Cargo.toml','Cargo.toml.orig','README.md','LICENSE-APACHE'])}
 if set(expected)!=actual:raise ValueError('Terminal library file inventory changed')
 for name,digest in expected.items():
  raw=base/name
  if raw.is_symlink():raise ValueError('Terminal library symlink: '+name)
  path=raw.resolve()
  if not path.is_relative_to(base.resolve()) or path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise ValueError('Terminal library checksum changed: '+name)
 return {'crate':d['crate'],'version':d['version'],'registry_archive_sha256':d['registry_archive_sha256'],'verified_library_files':len(expected),'patch':'Read-only canonical primary_grid accessor; no parser/mutation replacement'}
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args()
 result={'connector_source':verify_connector(),'pdu_source':verify_pdu(),'graphics_sources':verify_graphics(),'terminal_library_source':verify_vendor(),'headless':inspect('headless',ROOT/'ruwac/Cargo.toml'),'desktop':inspect('desktop',ROOT/'ruwac/desktop/Cargo.toml'),'scope':'Locked declaration/checksum and graphical-dependency exclusion checks; not legal approval, binary notice clearance or a vulnerability scan'}
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n');print('Dependency declarations and headless graph PASS',flush=True)
def verify_connector():
 base=ROOT/'ruwac/desktop/vendor/ironrdp-connector';d=json.loads((base/'UPSTREAM.json').read_text())
 if d['crate']!='ironrdp-connector' or d['version']!='0.10.0' or d['modified_files']!=['src/connection.rs','src/connection_finalization.rs','src/lib.rs']:raise ValueError('Unexpected connector patch provenance')
 for name,digest in d['files'].items():
  path=base/name
  if not path.resolve().is_relative_to(base.resolve()) or path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise ValueError('Connector source checksum changed: '+name)
 return {'crate':d['crate'],'version':d['version'],'registry_archive_sha256':d['crate_sha256'],'verified_files':len(d['files']),'modified_files':d['modified_files'],'scope':'Connect-time bandwidth reply, opt-in EGFX/RDSTLS same-endpoint handover and credential diagnostic redaction; existing CredSSP/crypto preserved'}
def verify_graphics():
 result=[]
 for name,version,commit in [('egfx','0.3.0','e4143f26306a6639a889d80c1c484ca69c365a8f'),('graphics','0.9.0','e4143f26306a6639a889d80c1c484ca69c365a8f'),('session','0.11.0','11a0810cfbbabd8b8023875a05e3041216d4b01b')]:
  base=ROOT/'ruwac/desktop/vendor'/('ironrdp-'+name);d=json.loads((base/'UPSTREAM.json').read_text())
  if (d['crate'],d['version'],d['commit'])!=('ironrdp-'+name,version,commit):raise ValueError('Unexpected graphics source origin')
  actual={str(p.relative_to(base)) for p in base.rglob('*')if p.is_file()and p.name!='UPSTREAM.json'}
  if actual!=set(d['files']):raise ValueError('Graphics source inventory changed')
  for file,digest in d['files'].items():
   path=base/file
   if not path.resolve().is_relative_to(base.resolve())or path.is_symlink()or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise ValueError('Graphics source checksum changed: '+file)
  result.append({'crate':d['crate'],'version':version,'commit':commit,'verified_files':len(actual),'patch':d['patch']})
 return result
def verify_pdu():
 base=ROOT/'ruwac/desktop/vendor/ironrdp-pdu';d=json.loads((base/'UPSTREAM.json').read_text())
 if d['crate']!='ironrdp-pdu' or d['version']!='0.9.0' or d['registry_archive_sha256']!='4ccd1179a4d106df1930347701388b5c79bc3725fa7dab4438d57db0d9d19347' or d['modified_files']!=['src/codecs/clearcodec/bands.rs']:raise ValueError('Unexpected PDU patch provenance')
 actual={str(p.relative_to(base))for p in base.rglob('*')if p.is_file()and p.name!='UPSTREAM.json'}
 if actual!=set(d['files']):raise ValueError('PDU source inventory changed')
 for name,value in d['files'].items():
  p=base/name
  if p.is_symlink()or not p.resolve().is_relative_to(base.resolve())or hashlib.sha256(p.read_bytes()).hexdigest()!=value:raise ValueError('PDU source checksum changed')
 return {'crate':d['crate'],'version':d['version'],'registry_archive_sha256':d['registry_archive_sha256'],'verified_files':len(actual),'modified_files':d['modified_files'],'scope':'Short V-bar coordinate bit correction; other selected registry source unchanged'}
if __name__=='__main__':main()
