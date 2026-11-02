from pathlib import Path
import datetime,json,os,subprocess
p=Path(__file__).parent
raw=subprocess.run(['/bin/bash','-c','set -a; source /data/neon-build/config/env.sh >/dev/null; env -0'],capture_output=True,check=True).stdout
env=os.environ.copy();env.update({k.decode():v.decode()for x in raw.split(b'\0')if x and b'='in x for k,v in [x.split(b'=',1)]})
for key in ['HTTP_PROXY','HTTPS_PROXY','http_proxy','https_proxy']:assert env.get(key)=='http://127.0.0.1:7890'
cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=8g','--resource','memory-swap=8g','--resource','cpu-quota=600000','--network','none','--progress','plain','--metadata-file',str(p/'build-metadata.json'),'--output','type=local,dest='+str(p/'export'),'-f',str(p/'Dockerfile'),str(p/'source')]
(p/'command.json').write_text(json.dumps(cmd,indent=2)+'\n')
start=datetime.datetime.now(datetime.timezone.utc)
with(p/'run.log').open('wb')as log: rc=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
q=p/'export/build.exitcode';phase=int(q.read_text())if q.is_file()else None
out={'build_exit':rc,'extension_make_exit':phase,'seconds':(datetime.datetime.now(datetime.timezone.utc)-start).total_seconds(),'PG_rebuilt':False,'PG18_source_head':'8912380d95f3f66b335b03cbe5dfb4b281d85ac2','full_Rust_services_rebuilt':False,'normal':False,'E':0}
(p/'result.json').write_text(json.dumps(out,indent=2)+'\n')
raise SystemExit(0 if rc==0 and phase==0 else 1)
