from pathlib import Path
import subprocess,json,time,shlex
p=Path(__file__).parent;start=time.monotonic();cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=3g','--resource','memory-swap=3g','--resource','cpu-quota=200000','--network','none','--progress','plain','--output','type=local,dest='+str(p/'export'),'-f',str(p/'Dockerfile'),str(p/'source')];(p/'command.json').write_text(json.dumps(cmd,indent=2)+chr(10))
with(p/'build.log').open('wb')as log:q=subprocess.run(['/bin/bash','-c','set -eu;source /data/neon-build/config/env.sh;exec '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT)
f=p/'export/headers.exitcode';rc=int(f.read_text())if f.exists()else None;r={'build_exit':q.returncode,'headers_exit':rc,'seconds':time.monotonic()-start,'Source':False,'normal':False,'E':0};(p/'result.json').write_text(json.dumps(r,indent=2)+chr(10));print(json.dumps(r),flush=True);raise SystemExit(0 if q.returncode==0 and rc==0 else 1)
