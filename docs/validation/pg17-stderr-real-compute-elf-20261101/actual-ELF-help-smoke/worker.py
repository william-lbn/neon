from pathlib import Path
import json,subprocess,time
p=Path(__file__).parent;t=time.monotonic();cmd=json.loads((p/'command.json').read_text())
with(p/'stdout').open('wb')as out,(p/'stderr').open('wb')as err:rc=subprocess.run(cmd,stdout=out,stderr=err).returncode
result={'exit':rc,'seconds':time.monotonic()-t,'actual_compute_help_passed':rc==0 and 'Usage:'in(p/'stdout').read_text(),'Source':False,'normal':False};(p/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True);raise SystemExit(0 if result['actual_compute_help_passed']else 1)
