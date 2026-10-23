#!/usr/bin/env python3
"""Explicit repository inventory. Private and generated material is never inferred."""
from pathlib import Path
import argparse,ast,json,sys
ROOT_FILES={'.gitignore','README.md','CHANGELOG.md','RELEASE-NOTES.md','LICENSE','CONTRIBUTING.md','SECURITY.md','CODE_OF_CONDUCT.md'}
ARCH_FILES={f'{n:02}-{name}.md' for n,name in enumerate(['RUWAC-DECISION','ARCHITECTURE','PROTOCOL-V0','ACCEPTANCE-MATRIX','BENCHMARKS','OPEN-ISSUES','IMPLEMENTATION-ROADMAP','COMPONENTS-LICENSES'])}|{'evidence-index.json','dependency-licenses.json','benchmark-results.json','acceptance-results.json'}
VENDOR_FILES={f'ruwac/desktop/vendor/ironrdp-connector/{name}' for name in ['README.md','LICENSE-APACHE','UPSTREAM.json','RUWAC-PATCH.md']}|{'ruwac/vendor/alacritty_terminal/'+name for name in ['Cargo.toml.orig','README.md','LICENSE-APACHE','UPSTREAM.json','RUWAC-PATCH.md']}
SCRIPT_FILES={'acceptance-gate.py','primary-selection.py','agent-bootstrap.py','remote-agent.py','public-files.py','install-user.py','dependency-check.py','update-user.py','gdm-recognize.py','privacy-check.py','test-privacy-check.py','gitleaks.toml','bundle-user.py','rpm-user.py','rpm-update.py','testing-subjects.py','verified-quality.py','license-sources.json'}
VENDOR_FILES |= {f'ruwac/desktop/vendor/ironrdp-{crate}/{name}' for crate in ['egfx','graphics','session','pdu'] for name in ['README.md','LICENSE-APACHE','LICENSE-MIT','UPSTREAM.json']}
# Pinned upstream codec test vectors are source fixtures, not compiled artifacts.
VENDOR_FILES |= {
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/32x64_rgb_raw.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/32x64_rgb_raw.bmp',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x24_argb_rle.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x24_argb_rle.bmp',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x35_ycocg_rle_ss.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x35_ycocg_rle_ss.bmp',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x64_aycocg_rle.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x64_aycocg_rle.bmp',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x64_ycocg_raw_ss.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x64_ycocg_raw_ss.bmp',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x64_ycocg_rle_ss.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/rdp6/test_assets/64x64_ycocg_rle_ss.bmp',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/decoded.0.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/decoded.1.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/decoded.2.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/decoded.3.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/decoded.4.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/encoded.0.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/encoded.1.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/encoded.2.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/encoded.3.bin',
    'ruwac/desktop/vendor/ironrdp-graphics/src/zgfx/test_assets/encoded.4.bin',
}
GITHUB_FILES={'.github/workflows/release.yml','.github/workflows/quality.yml','.github/workflows/labels.yml','.github/CODEOWNERS','.github/PULL_REQUEST_TEMPLATE.md','.github/ISSUE_TEMPLATE/bug_report.yml','.github/ISSUE_TEMPLATE/feature_request.yml','.github/ISSUE_TEMPLATE/config.yml','.github/scripts/label-pr.py','.github/scripts/test-label-pr.py'}
FORBIDDEN={'.agents','.codex','.claude','.kimi','.githooks','.vscode','.aws','AGENTS.md','CLAUDE.md','SKILL.md','FIRST-PROMPT.md','OWNER-SPIKE-SPEC.md'}
def files(root):
 out=[]
 candidates=[root/f for f in ROOT_FILES]
 for directory in ['architecture','scripts','ruwac','.github','spec']:
  candidates.extend((root/directory).rglob('*'))
 for p in candidates:
  r=p.relative_to(root)
  if p.is_symlink():continue
  if not p.is_file():continue
  if any(x in FORBIDDEN or x in {'target','.git','__pycache__'} for x in r.parts):continue
  if r.as_posix() in VENDOR_FILES:out.append(r)
  elif r.parts[:2]==('ruwac','notices') and len(r.parts)==4 and r.name in {'LICENSE-APACHE','LICENSE-MIT'}:out.append(r)
  elif len(r.parts)==1 and r.name in ROOT_FILES:out.append(r)
  elif r.parts[0]=='spec' and len(r.parts)==2 and r.suffix=='.md':out.append(r)
  elif r.parts[0]=='ruwac' and r.parts[1]=='desktop' and (r.name in {'Cargo.toml','Cargo.lock','chrome.css'} or r.suffix=='.rs'):out.append(r)
  elif r.as_posix() in {'ruwac/packaging/ruwac.1','ruwac/packaging/si.z4.ruwac.svg','ruwac/packaging/codex-shortcut-symbolic.svg','ruwac/packaging/ruwac.spec','ruwac/packaging/ruwac.desktop','ruwac/packaging/ruwac-rpm-update.service','ruwac/packaging/ruwac-rpm-update.timer'}:out.append(r)
  elif r.parts[0]=='ruwac' and r.parts[1]=='gui' and r.suffix=='.py':out.append(r)
  elif r.parts[0]=='architecture' and len(r.parts)==2 and r.name in ARCH_FILES:out.append(r)
  elif r.as_posix() in GITHUB_FILES:out.append(r)
  elif r.as_posix()=='ruwac/tests/fixtures/web_loopback.c':out.append(r)
  elif r.parts[0]=='scripts' and len(r.parts)==2 and r.name in SCRIPT_FILES:out.append(r)
  elif r.parts[0]=='ruwac' and ((r.name in {'Cargo.toml','Cargo.lock'} and len(r.parts)<=4) or (r.suffix=='.rs' and 'src' in r.parts) or (r.suffix=='.py' and r.parts[1]=='tests')):out.append(r)
 return sorted(out)
def verify(root,paths):
 allowed=set(files(root));bad=[]
 for name in paths:
  p=Path(name)
  if p not in allowed:bad.append('not allowlisted source entry (path withheld)');continue
 if bad:raise SystemExit('\n'.join(bad))
 import runpy
 privacy=runpy.run_path(str(root/'scripts/privacy-check.py'))
 try:privacy['verify'](root,paths,privacy['private_terms'](root))
 except ValueError as error:raise SystemExit(str(error))
 return len(paths)
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--check',action='store_true');p.add_argument('--tracked',action='store_true');a=p.parse_args();root=a.root.resolve()
 if a.tracked:
  import subprocess
  tracked=subprocess.check_output(['git','-C',str(root),'ls-files','-z']).decode().split('\0');tracked=[x for x in tracked if x];print('Verified repository inventory:',verify(root,tracked),'files')
 elif a.check:
  inventory=files(root);verify(root,[str(x) for x in inventory])
  for r in inventory:
   if r.suffix=='.py':ast.parse((root/r).read_text(),filename=str(r))
  if (root/'ruwac/target').exists():raise SystemExit('build artifacts in source workspace')
  print('Source inventory checked:',len(inventory),'files')
 else:
  for r in files(root):print(r.as_posix())
