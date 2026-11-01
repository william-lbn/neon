from pathlib import Path
import subprocess,json,shlex,time,hashlib,re,os
p=Path(__file__).parent;sha=lambda q:hashlib.sha256(q.read_bytes()).hexdigest()
def put(n,d):(p/n).write_text(json.dumps(d,indent=2)+'\n')
start=time.monotonic();cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=3g','--resource','memory-swap=3g','--resource','cpu-quota=200000','--build-arg','NONCE='+p.name,'--file',str(p/'Dockerfile'),'--output','type=local,dest='+str(p/'export'),str(p/'source')];put('build-command.json',cmd)
with(p/'build.log').open('wb')as log:rc=subprocess.run(['/bin/bash','-c','set -eu; source /data/neon-build/config/env.sh; exec '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT).returncode
result={'build_exit':rc,'suite_build_exit':int((p/'export/suite-build.exitcode').read_text())if(p/'export/suite-build.exitcode').exists()else None,'Source':False,'normal':False}
if rc!=0 or result['suite_build_exit']!=0:result['status']='BUILD_FAIL';put('result.json',result);raise SystemExit(1)
image='127.0.0.1:5001/neon/pg17-stderr-cap@sha256:46c3e2a3b0eddc18e6bf57911556254d74af65246c827706f4a6d8c3d62f815b'
cmd=['docker','run','--rm','--network=none','--read-only','--cpus=1','--memory=512m','--memory-swap=512m','--tmpfs','/tmp:rw,nosuid,nodev,size=32m','-v',str(p)+':/proof:rw','--entrypoint','/usr/bin/gcc',image,'-O2','-Wall','-Werror','/proof/launch.c','-o','/proof/pg-launch'];put('launch-compile-command.json',cmd)
with(p/'launch-compile.log').open('wb')as log:result['launch_compile_exit']=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT).returncode
if result['launch_compile_exit']!=0:result['status']='BUILD_FAIL_LAUNCH';put('result.json',result);raise SystemExit(1)
report=p/'native-report';report.mkdir();os.chown(report,1000,1000)
inside='set +e; /proof/export/pg-stderr-suite; result=$?; printf "%s\\n" "$result" >/proof/native-report/suite.exitcode; cat /sys/fs/cgroup/cpu.stat >/proof/native-report/cpu.stat; cat /sys/fs/cgroup/memory.peak >/proof/native-report/memory.peak; cat /sys/fs/cgroup/io.stat >/proof/native-report/io.stat; cat /sys/fs/cgroup/memory.max >/proof/native-report/memory.max; cat /sys/fs/cgroup/cpu.max >/proof/native-report/cpu.max; exit "$result"'
cmd=['timeout','--signal=TERM','--kill-after=10s','150s','docker','run','--rm','--init','--network=none','--read-only','--user','1000:1000','--cap-drop=ALL','--security-opt=no-new-privileges','--security-opt=seccomp=unconfined','--cpus=2','--memory=3g','--memory-swap=3g','--pids-limit=128','--tmpfs','/tmp:rw,nosuid,nodev,size=1g,mode=1777','-v',str(p)+':/proof:rw','--entrypoint','/bin/bash',image,'-c',inside];put('native-command.json',cmd)
with(p/'native.stdout').open('wb')as output,(p/'native.stderr').open('wb')as error:result['native_exit']=subprocess.run(cmd,stdout=output,stderr=error).returncode
rows=[json.loads(x)for x in re.findall(r'^PG_STDERR_SUITE_JSON (\{[^\r\n]+\})$',(p/'native.stdout').read_text(),re.M)]
put('actual-reports.json',rows);result['reports']=rows;result['seconds']=time.monotonic()-start;result['status']='PASS_REAL_PG_PIPE_COMPONENT'if result['native_exit']==0 and len(rows)==2 and all(x['actual_EOF_after_last_write_close']and x['wait_did_not_imply_EOF']and x['received_bytes']>8<<20 for x in rows)else'FAIL_RUNTIME_OR_FIXTURE';result['suite_ELF_sha256']=sha(p/'export/pg-stderr-suite');put('result.json',result);print(json.dumps(result),flush=True);raise SystemExit(0 if result['status']=='PASS_REAL_PG_PIPE_COMPONENT'else 1)
