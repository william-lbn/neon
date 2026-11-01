from pathlib import Path
import json,subprocess,shlex,time
p=Path(__file__).parent;t=time.monotonic()
cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=8g','--resource','memory-swap=8g','--resource','cpu-quota=600000','--build-arg','NONCE='+p.name,'--file',str(p/'Dockerfile'),'--output','type=local,dest='+str(p/'export'),'/data/neon-build/validation/pg-stderr-compute-integration-build-20261101T150045Z-f6ddd6/source'];(p/'build-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with(p/'build.log').open('wb')as log:rc=subprocess.run(['/bin/bash','-c','set -eu; source /data/neon-build/config/env.sh; exec '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT).returncode
f=p/'export/bin-build.exitcode';result={'build_exit':rc,'bin_build_exit':int(f.read_text())if f.exists()else None,'seconds':time.monotonic()-t,'Source':False,'normal':False,'actual_compute_bin_build_requested':True,'Source':False};(p/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True);raise SystemExit(0 if rc==0 and result['bin_build_exit']==0 else 1)
