from pathlib import Path
import subprocess,json,time,sys
u=Path(sys.argv[1]);nonce=sys.argv[2];a=['docker','buildx','build','--builder','neon-data','--resource','memory=8g','--resource','memory-swap=8g','--resource','cpu-quota=400000','--progress','plain','--build-arg','NONCE='+nonce,'--metadata-file',str(u/'build-metadata.json'),'--output','type=local,dest='+str(u/'receipt'),'-f',str(u/'Dockerfile'),str(u)];(u/'command.json').write_text(json.dumps({'argv':a,'actual_RUN':'4CPU/8GiB/no extra swap','Cargo_jobs':4,'test_network':'none','client_limits_separate':True},indent=2)+'\n');start=time.monotonic()
with(u/'run.log').open('w')as f:rc=subprocess.run(a,stdout=f,stderr=subprocess.STDOUT).returncode
v={'build_exit':rc,'seconds':time.monotonic()-start}
for name in ['source.rc','test.rc']:
 p=u/'receipt'/name;v[name]=int(p.read_text().strip())if p.exists()else None
v['pass']=rc==0 and v['source.rc']==0 and v['test.rc']==0;(u/'result.json').write_text(json.dumps(v,indent=2)+'\n');sys.exit(0 if v['pass']else 1)
