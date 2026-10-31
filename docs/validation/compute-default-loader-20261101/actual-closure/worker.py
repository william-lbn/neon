from pathlib import Path
import subprocess,json,time,hashlib,datetime
u=Path(__file__).resolve().parent;start=time.monotonic();result={'scope':'ACTUAL_IMMUTABLE_NONE_RUNTIME_LOADER_COMPONENT_ONLY','normal':False,'E':0};runtime='127.0.0.1:5001/neon/compute-v17@sha256:47ef74c7bee6018c1e8578486d1196bd0c9b6d84040f6ccdba97823dcd019e55'
try:
 cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=2g','--resource','memory-swap=2g','--resource','cpu-quota=200000','--progress','plain','--output','type=local,dest='+str(u/'compiled'),'-f',str(u/'Dockerfile'),str(u/'source')];(u/'compile-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
 with (u/'compile.log').open('wb') as f:r=subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,timeout=1200)
 result['compile_exit']=r.returncode;assert r.returncode==0
 receipt=u/'receipt';receipt.mkdir();binary=u/'compiled/closure.test';result['probe_binary_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest()
 cmd=['docker','run','--rm','--init','--name','neon-closure-20261031t202356z-0d36d0','--read-only','--user','0:0','--network','none','--cpus','1','--memory','1g','--memory-swap','1g','--pids-limit','64','--cap-drop','ALL','--cap-add','SETUID','--cap-add','SETGID','--security-opt','no-new-privileges','--mount','type=bind,src='+str(binary)+',dst=/closure.test,readonly','--mount','type=bind,src='+str(receipt)+',dst=/receipt','--entrypoint','/closure.test',runtime,'-test.v','-test.timeout=60s','-test.run=^TestDEVActualNoRpathRuntimeClosure$'];(u/'runtime-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
 with (u/'runtime.log').open('wb') as f:r=subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,timeout=100)
 result['runtime_exit']=r.returncode;result['pass']=r.returncode==0
except BaseException as e:result['error']=repr(e);result['pass']=False
result['seconds']=time.monotonic()-start;result['endUTC']=datetime.datetime.now(datetime.timezone.utc).isoformat();(u/'result.json').write_text(json.dumps(result,indent=2)+'\n');raise SystemExit(0 if result['pass'] else 1)
