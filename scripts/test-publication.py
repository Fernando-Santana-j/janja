#!/usr/bin/env python3
"""Offline release orchestration checks; GitHub, Git and Windows are isolated fixtures."""
import hashlib,json,os,shutil,subprocess,sys,tempfile
from pathlib import Path
source=Path(__file__).resolve().parent.parent
fake='''#!/usr/bin/env python3
import json,os,sys,shutil
from pathlib import Path
name=Path(sys.argv[0]).name
args=sys.argv[1:]
if name=='git':
    while args and args[0]=='-c': args=args[2:]
with Path('calls.jsonl').open('a') as f: f.write(json.dumps([name,*args])+'\\n')
if name=='cargo':sys.exit(0)
if name=='git':
    if args[:3]==['remote','get-url','origin']:print('https://github.com/Fernando-Santana-j/janja.git')
    elif args[:2]==['branch','--show-current']:print('main')
    elif args[:2]==['rev-parse','--verify']:sys.exit(1)
    elif args==['rev-parse','HEAD']:print('a'*40)
    elif args[:3]==['diff','--cached','--quiet']:sys.exit(1 if Path('staged').exists() else 0)
    elif args[0]=='add':Path('staged').touch()
    elif args[0]=='grep':sys.exit(0 if os.environ.get('PRIVATE_FIXTURE')=='1' else 1)
    elif args[0]=='commit':Path('staged').unlink()
    elif args[0] in ['fetch','merge-base','check-ignore','diff','push','tag']:pass
    else:raise RuntimeError(args)
elif name=='gh':
    if args[:2]==['auth','status']:pass
    elif args[:2]==['repo','view']:print('PUBLIC')
    elif args[:2]==['secret','set']:sys.stdin.read()
    elif args[:2]==['run','list']:print('12345')
    elif args[:2]==['run','watch']:pass
    elif args[:2]==['release','download']:
        target=Path(args[args.index('--dir')+1]);target.mkdir(exist_ok=True,parents=True)
        for file in Path('release').iterdir():shutil.copyfile(file,target/file.name)
    elif args[:2]==['release','view']:
        files=['janja-linux','janja-windows.exe','janja-setup-linux.sh','janja-setup-windows.exe','janja-update.json','janja-update.json.sig']
        print(json.dumps({'tagName':'2026.11.13-r12-leve','url':'https://github.com/Fernando-Santana-j/janja/releases/tag/fixture','isDraft':False,'isPrerelease':False,'assets':[{'name':f} for f in files]}))
    else:raise RuntimeError(args)
'''
with tempfile.TemporaryDirectory(prefix='janja-publish-test-') as temporary:
    base=Path(temporary)
    private=base/'private.pem';public=base/'public.pem'
    subprocess.run(['openssl','genpkey','-algorithm','RSA','-pkeyopt','rsa_keygen_bits:3072','-out',str(private)],check=True,stderr=subprocess.DEVNULL)
    subprocess.run(['openssl','pkey','-in',str(private),'-pubout','-out',str(public)],check=True,stderr=subprocess.DEVNULL)
    for case in ['validation-fails','private-key-blocked','success']:
        root=base/case;root.mkdir()
        for folder in ['scripts','src','assets','.release-keys','bin','release']: (root/folder).mkdir()
        for name in ['publish-release.sh','verify-release.py','update-linux.py']:shutil.copy2(source/'scripts'/name,root/'scripts'/name)
        (root/'scripts/test-updates.py').write_text('print("fixture test")')
        (root/'scripts/test-windows-from-linux.sh').write_text('exit '+('17' if case=='validation-fails' else '0')+'\n')
        (root/'src/lib.rs').write_text('pub const BUILD: &str = "2026.11.13-r12-leve";')
        (root/'assets/update-config.json').write_text('{"repository":"Fernando-Santana-j/janja"}')
        shutil.copy2(private,root/'.release-keys/update-private.pem');shutil.copy2(public,root/'assets/update-public.pem')
        for name in ['gh','git','cargo']:
            (root/'bin'/name).write_text(fake);(root/'bin'/name).chmod(0o755)
        metadata={'schema':1,'repository':'Fernando-Santana-j/janja','version':'2026.11.13-r12-leve','revision':12,'files':{}}
        for platform,name in [('linux-x86_64','janja-setup-linux.sh'),('windows-x86_64','janja-setup-windows.exe')]:
            data=b'installer fixture';(root/'release'/name).write_bytes(data)
            metadata['files'][platform]={'name':name,'size':len(data),'sha256':hashlib.sha256(data).hexdigest()}
        payload=root/'release/janja-update.json';payload.write_text(json.dumps(metadata))
        subprocess.run(['openssl','dgst','-sha256','-sign',str(private),'-out',str(root/'release/janja-update.json.sig'),str(payload)],check=True)
        result=subprocess.run(['bash','scripts/publish-release.sh'],cwd=root,env=dict(os.environ,PATH=str(root/'bin')+os.pathsep+os.environ['PATH'],PRIVATE_FIXTURE='1' if case=='private-key-blocked' else '0'),capture_output=True,text=True)
        calls=[json.loads(line) for line in (root/'calls.jsonl').read_text().splitlines()]
        pushes=[c for c in calls if c[:2]==['git','push']]
        assert 'BEGIN PRIVATE KEY' not in result.stdout
        if case=='success':
            assert result.returncode==0,result.stdout+result.stderr
            assert len(pushes)==2 and (root/'dist/publicacao.json').is_file()
        else:
            assert result.returncode!=0 and not pushes,(result.returncode,calls,result.stdout,result.stderr)
            assert not any(c[:2]==['git','commit'] for c in calls)
        print(case+': OK')
