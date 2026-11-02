from pathlib import Path
import json,subprocess,time,os
p=Path(__file__).parent
raw=subprocess.check_output(['/bin/bash','-c','set -a; source /data/neon-build/config/env.sh >/dev/null; env -0'])
env=os.environ.copy();env.update({k.decode():v.decode()for x in raw.split(b'\0')if x and b'='in x for k,v in [x.split(b'=',1)]})
image=json.loads((p/'source-artifact-lock.json').read_text())['runtime_image'];name=p.name.lower()
cmd=['docker','run','--name',name,'--pull=never','--network=none','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--cpus=2','--memory=3g','--memory-swap=3g','--pids-limit=256','--shm-size=128m','--user=1000:1000','--tmpfs','/tmp:rw,nosuid,nodev,size=1g,mode=1777','-v',str(p)+':/input:ro','-v',str(p/'evidence')+':/evidence:rw','-v',str(p/'install')+':/opt/pg18:ro','--entrypoint','/bin/bash',image,'/input/run.sh']
(p/'command.json').write_text(json.dumps(cmd,indent=2)+'\n');start=time.monotonic()
with(p/'run.log').open('wb')as f:q=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=150)
o=json.loads(subprocess.check_output(['docker','inspect',name],env=env,text=True))[0]
(p/'container.json').write_text(json.dumps({'ID':o['Id'],'Image':o['Image'],'State':o['State'],'HostConfig':{k:o['HostConfig'][k]for k in ['Memory','MemorySwap','NanoCpus','ReadonlyRootfs','NetworkMode','PidsLimit','ShmSize','CapDrop','SecurityOpt']}},indent=2)+'\n')
log=(p/'run.log').read_text();passed=q.returncode==0 and 'Ran 1 test'in log and '\nOK'in log and not o['State']['OOMKilled']
result={'status':'PASS_PG18_SLRU3_BOUNDARY_COMPONENT'if passed else'FAIL_ACTUAL_PG18_FORMAL_EXTENSION_COMPONENT','unique_Wait_exit':q.returncode,'OOMKilled':o['State']['OOMKilled'],'seconds':time.monotonic()-start,'peer_is_model':True,'targets':1,'normal':False,'Source':False,'E':0}
(p/'result.json').write_text(json.dumps(result,indent=2)+'\n')
z=subprocess.run(['docker','rm',name],env=env,capture_output=True,text=True);(p/'cleanup.json').write_text(json.dumps({'exit':z.returncode,'stdout':z.stdout,'stderr':z.stderr},indent=2)+'\n')
print(json.dumps(result),flush=True);raise SystemExit(int(not passed))
