from pathlib import Path
import json,subprocess,shlex,time
p=Path(__file__).parent
cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=3g','--resource','memory-swap=3g','--resource','cpu-quota=200000','--build-arg','NONCE='+p.name,'--file',str(p/'Dockerfile'),'--output','type=local,dest='+str(p/'export'),str(p/'source')]
(p/'build-command.json').write_text(json.dumps(cmd,indent=2)+'\n');t=time.monotonic()
with(p/'build.log').open('wb')as log:rc=subprocess.run(['/bin/bash','-c','set -eu; source /data/neon-build/config/env.sh; exec '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT).returncode
test=p/'export/test.exitcode';v={'build_exit':rc,'test_exit':int(test.read_text())if test.exists()else None,'seconds':time.monotonic()-t,'Source':False,'normal':False,'full_compute_build':False};(p/'result.json').write_text(json.dumps(v,indent=2)+'\n');print(json.dumps(v),flush=True);raise SystemExit(0 if rc==0 and v['test_exit']==0 else 1)
