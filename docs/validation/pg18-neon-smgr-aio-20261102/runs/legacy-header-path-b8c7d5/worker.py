from pathlib import Path
import json,subprocess,time,os
p=Path(__file__).parent;raw=subprocess.check_output(['/bin/bash','-c','set -a; source /data/neon-build/config/env.sh >/dev/null; env -0']);env=os.environ.copy();env.update({k.decode():v.decode()for x in raw.split(b'\0')if x and b'='in x for k,v in [x.split(b'=',1)]})
cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=3g','--resource','memory-swap=3g','--resource','cpu-quota=200000','--network','none','--progress','plain','--metadata-file',str(p/'build-metadata.json'),'--output','type=local,dest='+str(p/'export'),'-f',str(p/'Dockerfile'),str(p/'source')]
(p/'command.json').write_text(json.dumps(cmd,indent=2)+'\n');start=time.monotonic()
with(p/'run.log').open('wb')as log:q=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT)
result={'build_exit':q.returncode,'seven_syntax_passed':q.returncode==0 and (p/'export/verdict.txt').exists(),'seconds':time.monotonic()-start,'U':0,'S':0,'E':0,'normal':False}
(p/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True);raise SystemExit(q.returncode)
