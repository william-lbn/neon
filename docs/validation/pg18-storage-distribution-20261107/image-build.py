from pathlib import Path as P
import os,subprocess as s,json,hashlib,time,shutil
os.umask(0o077);os.environ['DOCKER_CONFIG']='/data/neon-build/config/docker';d=P(__file__).parent;ext=P('/data/neon-build/validation/PG18-原WAL回放扩展补齐-20261107T0550Z')
deadline=time.monotonic()+240
while not(ext/'result-v2.PUBLIC.json').exists():
 if time.monotonic()>deadline:raise RuntimeError('walredo build outcome absent')
 time.sleep(2)
assert json.loads((ext/'result-v2.PUBLIC.json').read_text())['exit']==0
context=d/'context';context.mkdir();shutil.copyfile(ext/'export/neon_walredo.so',context/'neon_walredo.so')
base='127.0.0.1:5001/neon-candidate/pageserver@sha256:e6160b8bcb6eb3a4bb1a02d4bd43145f4396987d2d7ad40fbb9e83756674d055'
(d/'Dockerfile').write_text('FROM '+base+'\nCOPY --chown=0:0 --chmod=0755 neon_walredo.so /usr/local/v18/lib/neon_walredo.so\n')
(d/'Dockerfile.dockerignore').write_text('')
tag='127.0.0.1:5001/neon-candidate/pageserver:pg18-walredo-36574552'
def run(label,args,timeout=100):
 (d/(label+'.argv.json')).write_text(json.dumps(args))
 with(d/(label+'.log')).open('xb')as f:q=s.run(args,stdin=s.DEVNULL,stdout=f,stderr=f,timeout=timeout)
 assert q.returncode==0,label+' returned '+str(q.returncode)
run('build',['docker','buildx','build','--builder','neon-data','--resource','cpu-quota=200000','--resource','memory=2g','--resource','memory-swap=2g','--network=none','--progress=plain','--metadata-file='+str(d/'metadata.json'),'--push','-t',tag,'-f',str(d/'Dockerfile'),str(context)],220)
pin=tag.rsplit(':',1)[0]+'@'+json.loads((d/'metadata.json').read_text())['containerimage.digest'];run('pull',['docker','pull',pin],100)
run('actual-walredo-load',['docker','run','--rm','--pull=never','--network=none','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--cpus=1','--memory=512m','--memory-swap=512m','-e','LD_LIBRARY_PATH=/usr/local/v18/lib','--entrypoint','/usr/local/v18/bin/postgres',pin,'--wal-redo'],30)
inspect=json.loads(s.check_output(['docker','image','inspect',pin]))[0]
cid=s.check_output(['docker','create',pin],text=True).strip()
try:
 run('readback',['docker','cp',cid+':/usr/local/v18/lib/neon_walredo.so',str(d/'actual-neon_walredo.so')])
finally:run('remove-never-started',['docker','rm',cid])
hs=hashlib.sha256((context/'neon_walredo.so').read_bytes()).hexdigest();assert hashlib.sha256((d/'actual-neon_walredo.so').read_bytes()).hexdigest()==hs
run('visible',['docker','tag',pin,'neon-pg18-visible/pageserver:walredo-36574552'])
(d/'result.PUBLIC.json').write_text(json.dumps({'passed':True,'image_pin':pin,'id':inspect['Id'],'walredo_sha256':hs,'same_Pageserver_PG_ELFs':True,'actual_walredo_load_exit':0,'PG18_K8s_accepted':0},indent=2))
