from pathlib import Path
import subprocess,json,time,sys
out=Path('/out');result=[]
commands=json.loads(Path('/test-commands.json').read_text())
for i,a in enumerate(commands):
 start=time.monotonic()
 with(out/('step-'+str(i)+'.log')).open('w')as f:rc=subprocess.run(a,stdout=f,stderr=subprocess.STDOUT).returncode
 result.append({'argv':a,'rc':rc,'seconds':time.monotonic()-start});(out/'steps.json').write_text(json.dumps(result,indent=2)+'\n')
 if rc:sys.exit(rc)
