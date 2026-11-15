#!/usr/bin/env python3
"""Base-branch-only, add-only contribution labels. Never execute PR code."""
import argparse,json,os,re,sys,urllib.error,urllib.request
from pathlib import Path
LABELS={'area/terminal':('268bd2','Persistent engine, CLI and terminal input'),'area/ui':('6f42c1','Native desktop workspace'),'area/rdp':('0075ca','Embedded RDP transport/display'),'area/clipboard':('0e8a16','Clipboard ownership or transfer'),'area/files':('d4c5f9','SSH/SFTP and file workspaces'),'area/docs':('5319e7','Documentation or governance'),'area/ci':('fbca04','Repository checks and workflows')}
def classify(paths):
 result=set()
 for raw in paths:
  if not isinstance(raw,str) or raw.startswith('/') or '..' in Path(raw).parts:raise ValueError('Invalid changed-file path')
  p=raw.lower()
  if p.startswith('ruwac/crates/') and 'ruwac-clipboard/' not in p:result.add('area/terminal')
  if p.startswith(('ruwac/desktop/','ruwac/gui/')):result.add('area/ui')
  if 'rdp' in p:result.add('area/rdp')
  if 'clipboard' in p or p.endswith('/clip.rs'):result.add('area/clipboard')
  if 'sftp' in p or '/files' in p:result.add('area/files')
  if p.endswith('.md') or p.startswith(('spec/','architecture/')):result.add('area/docs')
  if p.startswith('.github/') or p.startswith('scripts/'):result.add('area/ci')
 return sorted(result)
def report_http_error(error,method,path):
 # Never emit the raw response, request headers or arbitrary server strings.
 raw=error.read(4097);message=None
 if len(raw)<=4096:
  try:
   value=json.loads(raw)
   if isinstance(value,dict):message=value.get('message')
  except (ValueError,UnicodeError):pass
 known={'Resource not accessible by integration','Resource not accessible by personal access token','Bad credentials','Not Found','Validation Failed','Must have admin rights to Repository.','Issues are disabled for this repo','API rate limit exceeded'}
 diagnostic={'event':'github-label-http-error','status':error.code,'method':method,'endpoint':path.split('?',1)[0],'github_message':message if isinstance(message,str)and message in known else 'unrecognized or unavailable','response_truncated':len(raw)>4096}
 request_id=error.headers.get('X-GitHub-Request-Id','')if error.headers else ''
 if re.fullmatch(r'[A-F0-9]+(?::[A-F0-9]+){3,5}',request_id)and len(request_id)<=128:diagnostic['request_id']=request_id
 permissions=error.headers.get('X-Accepted-GitHub-Permissions','')if error.headers else ''
 if len(permissions)<=512:
  accepted=sorted(set(re.findall(r'\b(issues|pull_requests|contents|metadata)=(read|write|admin)\b',permissions)))
  if accepted:diagnostic['accepted_permissions']=['='.join(item)for item in accepted]
 print(json.dumps(diagnostic,sort_keys=True),file=sys.stderr)
def api(method,path,body=None):
 req=urllib.request.Request('https://api.github.com/repos/w3fans/RUWAC'+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Authorization':'Bearer '+os.environ['GITHUB_TOKEN'],'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28','Content-Type':'application/json'})
 try:
  with urllib.request.urlopen(req,timeout=20)as r:
   data=r.read(4*1024*1024+1)
   if len(data)>4*1024*1024:raise ValueError('API response limit')
   return json.loads(data)
 except urllib.error.HTTPError as error:
  try:report_http_error(error,method,path)
  except Exception:pass # Diagnostic failure must not replace the original HTTP refusal.
  raise
def main():
 p=argparse.ArgumentParser();p.add_argument('--files',type=Path);a=p.parse_args()
 if a.files:print(json.dumps({'labels':classify(json.loads(a.files.read_text()))}));return
 event=json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text());pr=event['pull_request'];number=event['number']
 if os.environ.get('GITHUB_REPOSITORY')!='w3fans/RUWAC' or event['repository']['full_name']!='w3fans/RUWAC' or type(number)is not int or not 1<=number<=2147483647:raise ValueError('Repository/event target rejected')
 if pr['base']['repo']['full_name']!='w3fans/RUWAC':raise ValueError('PR base rejected')
 # Verify checkout is exactly the trusted base commit before using write authority.
 import subprocess
 if subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()!=pr['base']['sha']:raise ValueError('Expected trusted base checkout')
 paths=[]
 for page in range(1,32):
  rows=api('GET',f'/pulls/{number}/files?per_page=100&page={page}');paths.extend(row['filename']for row in rows)
  if len(rows)<100:break
 else:raise ValueError('Changed-file pagination limit')
 if len(paths)!=pr['changed_files']:raise ValueError('Changed-file set incomplete; labels left unchanged')
 desired=classify(paths)
 if not desired:return
 known=set()
 for page in range(1,11):
  rows=api('GET',f'/labels?per_page=100&page={page}');known.update(row['name']for row in rows)
  if len(rows)<100:break
 else:raise ValueError('Repository label pagination limit')
 for label in desired:
  if label not in known:
   color,description=LABELS[label];api('POST','/labels',{'name':label,'color':color,'description':description})
 api('POST',f'/issues/{number}/labels',{'labels':desired});print(json.dumps({'applied_labels':desired}))
if __name__=='__main__':main()
