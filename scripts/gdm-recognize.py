#!/usr/bin/env python3
"""Bounded local recognition of an authenticated remote GDM framebuffer.
Returns geometry only. This helper never receives credentials or sends input.
"""
import csv, io, json, os, subprocess, sys
from pathlib import Path
from PIL import Image
Image.MAX_IMAGE_PIXELS=4096*4096

def words(image, destination, psm):
    image.resize((image.width*3,image.height*3)).convert('L').save(destination)
    tool=os.environ.get('RUWAC_GDM_OCR_TOOL','tesseract')
    # Tesseract receives only this private image, never a credential or reference.
    child=subprocess.Popen([tool,str(destination),'stdout','--psm',str(psm),'tsv'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
    try:
        output=child.communicate(timeout=4)[0]
        if child.returncode or len(output)>256*1024:raise ValueError('OCR unavailable')
    except BaseException:
        child.kill();child.wait();raise
    result=[]
    for row in csv.DictReader(io.StringIO(output.decode('utf-8')),delimiter='\t'):
        if row['text'].strip():
            result.append((row['text'],float(row['conf']),int(row['left'])/3,int(row['top'])/3,int(row['width'])/3,int(row['height'])/3))
    return result

def focus_boxes(image):
    w,h=image.size;data=image.load();lines=[]
    # A focused entry has two long blue horizontal border segments. Avatar
    # outlines, unfocused entries and ambiguous candidates are not authorized.
    for y in range(h):
        start=None
        for x in range(w):
            r,g,b=data[x,y];blue=r>=100 and g>=r+8 and b>=r+15
            if blue and start is None:start=x
            if start is not None and(not blue or x==w-1):
                end=x if blue else x-1
                if 128<=end-start<=700:lines.append((start,end,y))
                start=None
        if len(lines)>128:return []
    boxes=[]
    for left,right,top in lines:
        for left2,right2,bottom in lines:
            if 20<=bottom-top<=64 and abs(left-left2)<=8 and abs(right-right2)<=8:
                box=(max(0,min(left,left2)-3),top,min(w,max(right,right2)+4),bottom+1)
                if not any(abs(box[0]-old[0])<8 and abs(box[1]-old[1])<8 for old in boxes):boxes.append(box)
    return boxes

def users_target(all_words):
    tokens=[q for q in all_words if q[1]>=70]
    target=[]
    for i,q in enumerate(tokens):
        norm=''.join(c for c in q[0].casefold()if c.isalpha())
        if norm=='notlisted':target.append(q)
        elif norm=='not'and i+1<len(tokens)and tokens[i+1][0].casefold().startswith('listed'):
            next_=tokens[i+1]
            if abs(q[3]-next_[3])<8:target.append(('Not listed',min(q[1],next_[1]),q[2],q[3],next_[2]+next_[4]-q[2],max(q[5],next_[5])))
    return target

def recognize(path,trusted=False):
    image=Image.open(path).convert('RGB');w,h=image.size
    if not(200<=w<=4096 and 200<=h<=4096)or w*h>4*1024*1024:raise ValueError('Unsupported recognition size')
    base=path.parent;all_words=words(image,base/'layout.png',11)
    branded=any(t.casefold()=='fedora'and c>=85 for t,c,*_ in all_words)
    if not branded:
        candidates=[q for q in all_words if q[0].casefold()=='fedora'and q[1]>=60]
        if len(candidates)==1:
            _,_,x,y,tw,th=candidates[0]
            # Sparse resized layouts can reduce full-frame confidence. A local
            # word crop must meet the same threshold; candidate confidence alone
            # never authorizes input.
            crop=image.crop((max(0,int(x)-4),max(0,int(y)-4),min(w,int(x+tw)+4),min(h,int(y+th)+4)))
            branded=any(t.casefold()=='fedora'and c>=85 for t,c,*_ in words(crop,base/'branding.png',7))
    # Without the Fedora logo, GDM's own user list ("Not listed?" exactly once
    # and no input field) still identifies the login screen; it only leads to
    # a click. Fields are typed into only later in that same sign-in (trusted).
    unbranded_users=False
    if not branded and not trusted:
        if len(users_target(all_words))!=1:return {'kind':'unknown'}
        unbranded_users=True
    entries=[]
    for index,b in enumerate(focus_boxes(image)):
        crop=image.crop((b[0]+10,b[1]+3,b[2]-20,b[3]-3))
        # Normalize the low-contrast placeholder without preserving OCR text in
        # the reply. Pillow preprocessing stays within the owned private folder.
        from PIL import ImageOps
        crop=ImageOps.autocontrast(ImageOps.invert(crop.convert('L')))
        found=words(crop,base/f'entry-{index}.png',7)
        labels={t.casefold()for t,c,*_ in found if c>=90 and t.casefold()in ['password','username']}
        if len(labels)==1:entries.append({'kind':labels.pop(),'rect':list(b)})
    # The plain GDM user list has no input field; any focused field, labelled
    # or not, means this is not that screen.
    if unbranded_users and focus_boxes(image):return {'kind':'unknown'}
    if len(entries)==1:return entries[0]
    if entries:return {'kind':'ambiguous'}
    target=users_target(all_words)
    if len(target)==1:
        q=target[0];return {'kind':'users','rect':[round(q[2]),round(q[3]),round(q[2]+q[4]),round(q[3]+q[5])]}
    return {'kind':'unknown'}

try:
    path=Path(sys.argv[1]);trusted=sys.argv[2:]==['--trusted-sequence']
    print(json.dumps(recognize(path,trusted),separators=(',',':')))
except Exception:
    print('{"kind":"unavailable"}')
    sys.exit(1)
