from pathlib import Path
import subprocess,json,re,time,hashlib
p=Path(__file__).parent;start=time.monotonic();cmd=json.loads((p/'native-command.json').read_text())
with(p/'native.stdout').open('wb')as out,(p/'native.stderr').open('wb')as err:rc=subprocess.run(cmd,stdout=out,stderr=err).returncode
rows=[json.loads(v)for v in re.findall(r'^PG_STDERR_SUITE_JSON (\{[^\r\n]+\})$',(p/'native.stdout').read_text(),re.M)]
result={'native_exit':rc,'reports':rows,'seconds':time.monotonic()-start,'Source':False,'normal':False,'status':'PASS_REAL_PG_PIPE_COMPONENT'if rc==0 and len(rows)==2 and all(v['actual_EOF_after_last_write_close']and v['wait_did_not_imply_EOF']and v['received_bytes']>8<<20 for v in rows)else'FAIL_RUNTIME_OR_FIXTURE'}
(p/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True);raise SystemExit(0 if result['status'].startswith('PASS_')else 1)
