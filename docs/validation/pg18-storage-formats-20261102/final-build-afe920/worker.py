from pathlib import Path
import json,subprocess,shlex,time
p=Path(__file__).parent;t=time.monotonic()
cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=8g','--resource','memory-swap=8g','--resource','cpu-quota=600000','--build-arg','NONCE='+p.name,'--file',str(p/'Dockerfile'),'--output','type=local,dest='+str(p/'export'),str(p/'source')];(p/'build-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with(p/'build.log').open('wb')as log:rc=subprocess.run(['/bin/bash','-c','set -eu;source /data/neon-build/config/env.sh;exec '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT).returncode
phases={n:int((p/'export'/(n+'.exitcode')).read_text())if(p/'export'/(n+'.exitcode')).exists()else None for n in ['ffi-example-build','affected-consumers','checkpoint-wallevel-test']}
passed=rc==0 and all(v==0 for v in phases.values());result={'build_exit':rc,'phases':phases,'seconds':time.monotonic()-t,'normal':False,'Source':False,'E':0};(p/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True);raise SystemExit(0 if passed else 1)
