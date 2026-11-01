from pathlib import Path
import datetime, hashlib, json, os, secrets, subprocess

root = Path('/data/neon-build')
repo = root / 'src/neon'
base = '3e5aa22c264b809645819f88b37783a5aad53a53'
pg = 'adecb5ecaf131fffde035a56f88c634403309b46'
tools_image = '127.0.0.1:5001/neon/compute-v17-stage@sha256:ece1e02faadce8c6c0c949e8314226ccfa958cef74902b89f790d89a0e28c020'
pg_image = '127.0.0.1:5001/neon/pg17-dsm-standalone-fix@sha256:b164d61a939502fa3ba983c0359f7832b9345ccefeb15c00326eaceab61a3a40'
pg_elf = '649f4535d07e682fe5015fd8df8be23f626e73bd772da21b16e1c032721c53c6'
assert subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip() == base
assert subprocess.check_output(['git', '-C', str(repo/'vendor/postgres-v17'), 'rev-parse', 'HEAD'], text=True).strip() == pg
nonce = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + secrets.token_hex(3)
p = root / 'validation' / ('root-pg17-required-extensions-build-' + nonce)
p.mkdir(mode=0o700)
source = p/'source'
source.mkdir()
# 只从Linux Git对象导出；省略文档和Python测试，不复制工作树或Mac内容。
with (p/'archive.stderr').open('wb') as errors:
    archive = subprocess.Popen(['git', '-C', str(repo), 'archive', '--format=tar', base], stdout=subprocess.PIPE, stderr=errors)
    unpack = subprocess.run(['tar', '-x', '--exclude=docs', '--exclude=doc', '--exclude=.github', '--exclude=test_runner', '-C', str(source)], stdin=archive.stdout, capture_output=True)
    archive.stdout.close()
    rc = archive.wait()
    (p/'unpack.stderr').write_bytes(unpack.stderr)
    assert rc == 0 and unpack.returncode == 0
sha = lambda q: hashlib.sha256(q.read_bytes()).hexdigest()
files = {str(q.relative_to(source)): sha(q) for q in source.rglob('*') if q.is_file() and not q.is_symlink()}
(source/'.required-extension-inputs.sha256').write_text(''.join(h+'  '+n+'\n' for n,h in sorted(files.items())))
dockerfile = '''FROM PG_IMAGE AS actualpg
FROM TOOLS_IMAGE AS check
USER nonroot
WORKDIR /home/nonroot/neon-extension-source
COPY --chown=1000:1000 --from=actualpg /usr/local/pgsql/ /usr/local/pgsql/
COPY --chown=1000:1000 . .
ENV CARGO_BUILD_JOBS=4 CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/home/nonroot/target RUSTUP_TOOLCHAIN=1.88.0-x86_64-unknown-linux-gnu GIT_VERSION=BASE_GIT
RUN --network=none --mount=type=cache,id=neon-cargo-registry-amd64,target=/home/nonroot/.cargo/registry,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-cargo-git-amd64,target=/home/nonroot/.cargo/git,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-core-target-amd64,target=/home/nonroot/target,uid=1000,gid=1000,sharing=locked --mount=type=cache,id=neon-ccache-amd64,target=/home/nonroot/.cache/ccache,uid=1000,gid=1000,sharing=locked set -eu; \
 mkdir -p /home/nonroot/extension-proof/artifacts; \
 test "$(cat /sys/fs/cgroup/memory.max)" = 8589934592; test "$(cat /sys/fs/cgroup/cpu.max)" = "600000 100000"; \
 cat /sys/fs/cgroup/cpu.max >/home/nonroot/extension-proof/cpu.max; cat /sys/fs/cgroup/memory.max >/home/nonroot/extension-proof/memory.max; \
 sha256sum -c .required-extension-inputs.sha256 >/home/nonroot/extension-proof/actual-input-verification.txt; \
 echo 'PG_ELF  /usr/local/pgsql/bin/postgres' | sha256sum -c - >/home/nonroot/extension-proof/PG-identity-before.txt; \
 rustc --version >/home/nonroot/extension-proof/rustc-version.txt; cargo --version >/home/nonroot/extension-proof/cargo-version.txt; \
 /usr/local/pgsql/bin/pg_config >/home/nonroot/extension-proof/pg-config.txt; \
 printenv CARGO_TARGET_DIR CARGO_BUILD_JOBS CARGO_NET_OFFLINE >/home/nonroot/extension-proof/cargo-execution-env.txt; \
 touch /home/nonroot/neon-extension-source/pgxn/neon/communicator/build.rs; \
 set +e; make -C /home/nonroot/neon-extension-source/pgxn -j6 -f /home/nonroot/neon-extension-source/pgxn/Makefile PG_CONFIG=/usr/local/pgsql/bin/pg_config NEON_CARGO_ARTIFACT_TARGET_DIR=/home/nonroot/target/debug CARGO_BUILD_FLAGS='--locked --offline --message-format=json-render-diagnostics' CARGO_PROFILE='--profile=dev' neon neon_rmgr >/home/nonroot/extension-proof/build.log 2>&1; \
 rc=$?; echo "$rc" >/home/nonroot/extension-proof/build.exitcode; set -e; \
 if test "$rc" = 0; then \
  cp /usr/local/pgsql/lib/neon.so /usr/local/pgsql/lib/neon_rmgr.so /home/nonroot/extension-proof/artifacts/; \
  mkdir -p /home/nonroot/extension-proof/artifacts/extension; cp /usr/local/pgsql/share/extension/neon* /home/nonroot/extension-proof/artifacts/extension/; \
  sha256sum /usr/local/pgsql/lib/neon.so /usr/local/pgsql/lib/neon_rmgr.so >/home/nonroot/extension-proof/extension-ELF.sha256; \
  sha256sum /home/nonroot/target/debug/libcommunicator.a /home/nonroot/neon-extension-source/pgxn/neon/communicator/communicator_bindings.h >/home/nonroot/extension-proof/cargo-artifacts.sha256; \
  readelf -d /usr/local/pgsql/lib/neon.so >/home/nonroot/extension-proof/neon.dynamic.txt; \
  readelf -d /usr/local/pgsql/lib/neon_rmgr.so >/home/nonroot/extension-proof/neon-rmgr.dynamic.txt; \
 fi; \
 sha256sum /usr/local/pgsql/bin/postgres >/home/nonroot/extension-proof/PG-identity-after.txt; \
 cat /sys/fs/cgroup/cpu.stat >/home/nonroot/extension-proof/cpu.stat; cat /sys/fs/cgroup/memory.peak >/home/nonroot/extension-proof/memory.peak; cat /sys/fs/cgroup/io.stat >/home/nonroot/extension-proof/io.stat
FROM scratch
COPY --from=check /home/nonroot/extension-proof/ /
'''.replace('PG_IMAGE', pg_image).replace('TOOLS_IMAGE', tools_image).replace('BASE_GIT', base).replace('PG_ELF', pg_elf)
(p/'Dockerfile').write_text(dockerfile)
# 原仓库ignore面向完整生产镜像。本配方需要精确Git归档的每个输入及校验清单。
ignore=p/'Dockerfile.dockerignore'
ignore.write_text('# Explicit Git-object input set; verify every selected file in the build step.\n')
lock = {'base_neon_git':base,'actual_PG17_git':pg,'PG17_parent_gitlink_not_moved':True,'tools_image':tools_image,'actual_PG17_image':pg_image,'PG17_postgres_sha256':pg_elf,'actual_COPY':files,'Dockerfile_sha256':sha(p/'Dockerfile'),'Dockerfile_specific_ignore_sha256':sha(ignore),'extension_build_layout':'in-tree within isolated Git snapshot; business worktree unchanged','scope':'只构建真实PG17必须的neon/neon_rmgr扩展；复用准确PG17安装/headers，不重建PG/全部Rust，不是运行时或E验收','limits':{'registered_cpu':6,'registered_memory_GiB':8,'single_executing_RUN_cpu':6,'single_executing_RUN_memory_GiB':8,'Cargo_jobs':4,'make_jobs':6},'normal':False,'E':0}
(p/'source-lock.json').write_text(json.dumps(lock,ensure_ascii=False,indent=2)+'\n')
worker = '''from pathlib import Path
import datetime,json,os,subprocess
p=Path(__file__).parent
raw=subprocess.run(['/bin/bash','-c','set -a; source /data/neon-build/config/env.sh >/dev/null; env -0'],capture_output=True,check=True).stdout
env=os.environ.copy();env.update({k.decode():v.decode()for x in raw.split(b'\\0')if x and b'='in x for k,v in [x.split(b'=',1)]})
for key in ['HTTP_PROXY','HTTPS_PROXY','http_proxy','https_proxy']:assert env.get(key)=='http://127.0.0.1:7890'
cmd=['docker','buildx','build','--builder','neon-data','--resource','memory=8g','--resource','memory-swap=8g','--resource','cpu-quota=600000','--network','none','--progress','plain','--metadata-file',str(p/'build-metadata.json'),'--output','type=local,dest='+str(p/'export'),'-f',str(p/'Dockerfile'),str(p/'source')]
(p/'command.json').write_text(json.dumps(cmd,indent=2)+'\\n')
start=datetime.datetime.now(datetime.timezone.utc)
with(p/'run.log').open('wb')as log: rc=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
q=p/'export/build.exitcode';phase=int(q.read_text())if q.is_file()else None
out={'build_exit':rc,'extension_make_exit':phase,'seconds':(datetime.datetime.now(datetime.timezone.utc)-start).total_seconds(),'PG_rebuilt':False,'full_Rust_services_rebuilt':False,'normal':False,'E':0}
(p/'result.json').write_text(json.dumps(out,indent=2)+'\\n')
raise SystemExit(0 if rc==0 and phase==0 else 1)
'''
compile(worker, str(p/'worker.py'), 'exec')
(p/'worker.py').write_text(worker)
spec = {'protocol_version':1,'nonce':nonce,'unit':'neon-root-pg17-required-extensions-'+nonce.lower()+'.service','kind':'heavy','evidence_dir':str(p),'argv':['/usr/bin/python3',str(p/'worker.py')]}
f=p/'budget-spec.json'
f.write_text(json.dumps(spec,indent=2)+'\n')
f.chmod(0o600)
budget=root/'continuous/resource-budget/budget.py'
for action in ['reserve','launch']:
    result=subprocess.run(['python3',str(budget),action,'--spec',str(f)],capture_output=True,text=True)
    (p/(action+'-receipt.json')).write_text(json.dumps({'exit':result.returncode,'stdout':result.stdout,'stderr':result.stderr},indent=2)+'\n')
    if result.returncode:
        print(json.dumps({'path':str(p),'action':action,'exit':result.returncode,'detail':result.stdout[:2000],'error':result.stderr[:1000]}))
        raise SystemExit(result.returncode)
print(json.dumps({'path':str(p),'unit':spec['unit'],'nonce':nonce,'source_files':len(files),'source_lock_sha256':sha(p/'source-lock.json'),'state':'ACTUALLY_LAUNCHED_NOT_PASSED'}))
