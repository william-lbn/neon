
from pathlib import Path
import hashlib,json,os
e=Path('/evidence');root=Path('/home/nonroot/vendor/postgres-v17')
expect=json.loads((e/'v2-expected-image-source-hashes.json').read_text())
actual={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file()}
missing=sorted(set(expect)-set(actual));diff={n:{'expected':h,'actual':actual[n]} for n,h in expect.items() if n in actual and actual[n]!=h};extra=sorted(set(actual)-set(expect))
bin=Path(os.environ['NEON_TEST_PG_BIN'],'postgres')
r={'expectedSourceFileCount':len(expect),'actualFileCount':len(actual),'allExpectedFileSHA256Match':not missing and not diff,'missing':missing,'different':diff,'extraFiles':extra,'postgresSHA256':hashlib.sha256(bin.read_bytes()).hexdigest(),'testLibrarySHA256':hashlib.sha256(Path(os.environ['NEON_TEST_DSG_LIBRARY']).read_bytes()).hexdigest(),'runtimePythonSources':'Latest three Python test files are readonly-mounted from evidence; audit/compile image contains older fixture Python. Runtime source SHA recorded separately; never claim latest fixture compiled into image.'}
(e/'actual-image-source-proof.json').write_text(json.dumps(r,indent=2)+'\n')
(e/'actual-image-all-files-sha256.json').write_text(json.dumps(actual,sort_keys=True,indent=2)+'\n')
print(json.dumps(r));raise SystemExit(0 if r['allExpectedFileSHA256Match'] else 1)
