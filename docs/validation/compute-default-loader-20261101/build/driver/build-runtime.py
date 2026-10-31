from pathlib import Path
import os,sys,subprocess,hashlib,json,tarfile,io,re,time,datetime,urllib.request,concurrent.futures,threading,copy,signal
OUT=Path('/data/neon-build/validation/compute-no-rpath-pg-20261031T193328Z-45711b')
DEV=Path('/data/neon-build/validation/compute-no-rpath-pg-20261031T193328Z-45711b/driver')
SNAP=Path('/data/neon-build/validation/compute-no-rpath-pg-20261031T193328Z-45711b/immutable-source')
META=json.loads((OUT/'snapshot-inputs.json').read_text());SOURCE=json.loads((OUT/'source-snapshot.json').read_text())
NEON=META['neonCommit'];PG=META['pg17Commit'];PREFIX='no-rpath-pg-b95b142190cd-20261031t193328z-45711b';REG='127.0.0.1:5001/neon'
STAGES=OUT/'stages';mutex=threading.Lock();start=time.monotonic()
def jwrite(path,value):
 t=path.with_suffix(path.suffix+'.tmp');t.write_text(json.dumps(value,indent=2)+'\n');t.replace(path)
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def available():
 m={}
 for l in Path('/proc/meminfo').read_text().splitlines():
  a,b=l.split(':',1);m[a]=int(b.strip().split()[0])*1024
 return m['MemAvailable']
def run_logged(args,path,timeout,stdin=None):
 t=time.monotonic()
 with path.open('wb') as log:
  p=subprocess.Popen(args,stdin=stdin,stdout=log,stderr=subprocess.STDOUT)
  try:rc=p.wait(timeout=timeout)
  except subprocess.TimeoutExpired:
   p.terminate()
   try:p.wait(timeout=20)
   except subprocess.TimeoutExpired:p.kill();p.wait()
   rc=124
 return rc,time.monotonic()-t
def registry_digest(repo,tag):
 req=urllib.request.Request('http://127.0.0.1:5001/v2/'+repo+'/manifests/'+tag,headers={'Accept':'application/vnd.oci.image.index.v1+json,application/vnd.docker.distribution.manifest.list.v2+json,application/vnd.oci.image.manifest.v1+json,application/vnd.docker.distribution.manifest.v2+json'})
 with urllib.request.urlopen(req,timeout=20) as response:return response.headers['Docker-Content-Digest']
def rawblock(text,name):
 m=re.search(r'(?m)^FROM [^\n]+ AS '+re.escape(name)+r'\s*\n',text);assert m
 n=re.search(r'(?m)^FROM ',text[m.end():])
 return text[m.start():m.end()+n.start() if n else len(text)]
def seed_dependency_record():
 # 仅复用无PG业务产物的通用工具层，旧source与当前源块准确同一性显式证明。
 old=Path('/data/neon-build/snapshots/test-only-pg17-compute-20261030T155759Z')
 a=rawblock((old/'compute/compute-node.Dockerfile').read_text(),'build-deps')
 b=rawblock((SNAP/'compute/compute-node.Dockerfile').read_text(),'build-deps');assert a==b
 imageID='sha256:641ed0bef5386c99a90812d05c278e7cc2b469b9a0419f1a747440cab0deebeb'
 tag=PREFIX+'-build-deps-cache';image=REG+'/compute-v17-stage:'+tag
 subprocess.run(['docker','tag',imageID,image],check=True)
 rc,secs=run_logged(['docker','push',image],OUT/'cached-build-deps-push.log',180)
 if rc:raise RuntimeError('cached tool layer registry push failed '+str(rc))
 digest=registry_digest('neon/compute-v17-stage',tag)
 record={'kind':'compute-stage','pg':'v17','aliases':['build-deps'],'image':image,'digest':digest,'source_commit':'b82d1b9e95ceb9387e0be6e280cd2eed4eca299c','current_source_block_matches':NEON,'rawBuildDepsSourceBlockSHA256':hashlib.sha256(b.encode()).hexdigest(),'cacheImageID':imageID,'PGSourceCompiledInCachedLayer':False,'TEST_ONLY':True,'elapsedSeconds':secs}
 jwrite(STAGES/'build-deps.json',record);jwrite(OUT/'cached-build-deps-source-equivalence.json',record)
def source_checks(selector,strip=''):
 return ''.join(v['sha256']+'  '+(n[len(strip):] if strip else n)+'\n' for n,v in sorted(SOURCE['files'].items()) if selector(n))
GEN={
 'ci/pre-main-probe-pg-inputs.sha256':(OUT/'pg-inputs.sha256').read_bytes(),
 'ci/pre-main-probe-pgxn-inputs.sha256':source_checks(lambda n:n.startswith('pgxn/')).encode(),
 'ci/pre-main-probe-rust-inputs.sha256':source_checks(lambda n:n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml'] or n.startswith(('.cargo/','.config/')) or n.startswith(('libs/','compute_tools/','proxy/','control_plane/','pageserver/','storage_broker/','storage_controller/','safekeeper/','endpoint_storage/','storage_scrubber/','workspace_hack/','pgxn/')) and n.endswith(('.rs','.toml','.proto'))).encode(),
 'ci/pre-main-probe-source.json':json.dumps(META,sort_keys=True).encode()+b'\n'}
CAP='printf "PROBE_MEMORY_MAX="; cat /sys/fs/cgroup/memory.max; printf "PROBE_CPU_MAX="; cat /sys/fs/cgroup/cpu.max; test "$(cat /sys/fs/cgroup/memory.max)" = "$PROBE_MEMORY_MAX_BYTES"; test "$(cat /sys/fs/cgroup/memory.swap.max)" = "0"; '
def overlay(raw,target):
 assert raw.startswith('# syntax=');raw=raw.split('\n',1)[1]
 # 只改构建缓存与来源/资源断言；原PG配置、make目标和三个Rust release profile不改。
 parts=re.split(r'(?m)(?=^FROM )',raw);result=[parts[0]]
 for block in parts[1:]:
  first=block.splitlines()[0];m=re.search(r' AS ([\w-]+)$',first,re.I);stage=m.group(1)
  block=block.replace(first,first+'\nARG PROBE_MEMORY_MAX_BYTES',1)
  if stage=='pg-build' and stage==target:
   anchor='RUN cd postgres &&'
   check='COPY ci/pre-main-probe-pg-inputs.sha256 /tmp/pre-main-pg-inputs.sha256\nRUN '+CAP+'mkdir -p /pre-main-source-proof && cd /postgres && sha256sum -c /tmp/pre-main-pg-inputs.sha256 > /pre-main-source-proof/pg-before-official-patches.check && cp /tmp/pre-main-pg-inputs.sha256 /pre-main-source-proof/pg-inputs.sha256\n'
   assert anchor in block;block=block.replace(anchor,check+'\n'+anchor,1)
  if stage=='neon-ext-build' and stage==target:
   anchor='RUN make '
   check='COPY ci/pre-main-probe-pgxn-inputs.sha256 /tmp/pre-main-pgxn-inputs.sha256\nRUN '+CAP+'mkdir -p /pre-main-source-proof && sha256sum -c /tmp/pre-main-pgxn-inputs.sha256 > /pre-main-source-proof/pgxn-before-build.check && cp /tmp/pre-main-pgxn-inputs.sha256 /pre-main-source-proof/pgxn-inputs.sha256\n'
   assert anchor in block;block=block.replace(anchor,check+'\n'+anchor,1)
  if stage=='compute-tools' and stage==target:
   anchor='RUN --mount=type=cache,uid=1000'
   check='COPY ci/pre-main-probe-rust-inputs.sha256 /tmp/pre-main-rust-inputs.sha256\nRUN '+CAP+'mkdir -p /home/nonroot/pre-main-source-proof && sha256sum -c /tmp/pre-main-rust-inputs.sha256 > /home/nonroot/pre-main-source-proof/rust-inputs.check && cp /tmp/pre-main-rust-inputs.sha256 /home/nonroot/pre-main-source-proof/rust-inputs.sha256\n'
   assert anchor in block;block=block.replace(anchor,check+'\n'+anchor,1)
  lines=block.splitlines(True);changed=[];i=0
  while i<len(lines):
   line=lines[i]
   if not line.startswith('RUN '):changed.append(line);i+=1;continue
   instruction=line;i+=1
   while instruction.rstrip().endswith('\\') and i<len(lines):instruction+=lines[i];i+=1
   mounts=[]
   if stage in ['pg-build','neon-ext-build'] and any(k in instruction for k in ['make ','make -','cd postgres']):
    mounts.append('--mount=type=cache,id=compute-ccache-amd64,target=/home/nonroot/.cache/ccache,sharing=shared')
   if stage=='compute-tools' and 'cargo build' in instruction:
    instruction=re.sub(r'--mount=type=cache,[^\s]+','',instruction)
    mounts+=['--mount=type=cache,id=neon-cargo-registry-amd64,target=/home/nonroot/.cargo/registry,uid=1000,gid=1000,sharing=locked','--mount=type=cache,id=neon-cargo-git-amd64,target=/home/nonroot/.cargo/git,uid=1000,gid=1000,sharing=locked','--mount=type=cache,id=neon-core-target-amd64,target=/home/nonroot/target,uid=1000,gid=1000,sharing=locked']
   if stage=='neon-ext-build' and 'install-compute' in instruction:
    mounts+=['--mount=type=cache,id=compute-registry-uid0-amd64,target=/home/nonroot/.cargo/registry,uid=0,gid=0,sharing=locked','--mount=type=cache,id=compute-git-uid0-amd64,target=/home/nonroot/.cargo/git,uid=0,gid=0,sharing=locked','--mount=type=cache,id=compute-target-neon-ext-build-amd64,target=/home/nonroot/target,uid=0,gid=0,sharing=locked']
   if mounts:
    instruction=instruction.replace('RUN ','RUN '+' '.join(mounts)+' ',1)
    # 前置mount选项后加真实cgroup断言，保持原命令/&&语义。
    pos=4
    while instruction[pos:].startswith('--mount='):
     space=instruction.index(' ',pos);pos=space+1
    instruction=instruction[:pos]+CAP+instruction[pos:]
   changed.append(instruction)
  result.append(''.join(changed))
 return ''.join(result)
def context_tar(path,dockerfile):
 # 以Git归档原mode创建stream，不把Host只读参考树的chmod带入COPY。
 selected={}
 archives={}
 for source in SOURCE['sources']:
  p=source['archive'];assert sha(p)==source['sha256'];tf=tarfile.open(p);archives[p]=tf
  for m in tf.getmembers():selected[m.name]=(tf,m)
 with tarfile.open(path,'w') as out:
  root=tarfile.TarInfo('.');root.type=tarfile.DIRTYPE;root.mode=0o755;root.uid=root.gid=0;out.addfile(root)
  for n,(tf,m) in selected.items():out.addfile(copy.copy(m),tf.extractfile(m) if m.isfile() else None)
  generated={**GEN,'ci/pre-main-probe.Dockerfile':dockerfile.encode()}
  for n,data in generated.items():
   t=tarfile.TarInfo(n);t.mode=0o644;t.size=len(data);t.uid=t.gid=0;out.addfile(t,io.BytesIO(data))
 for tf in archives.values():tf.close()
 path.chmod(0o444)
def build(target,memoryGiB=8):
 while available()<12<<30:raise RuntimeError('Insufficient actual MemAvailable before '+target)
 directory=OUT/target;directory.mkdir(exist_ok=False);raw=directory/'official-rendered.Dockerfile'
 subprocess.run(['python3',str(SNAP/'ci/compute.py'),'render','--pg','v17','--target',target,'--artifacts',str(STAGES),'--output',str(raw)],check=True,env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1'})
 df=overlay(raw.read_text(),target);(directory/'probe.Dockerfile').write_text(df)
 context=directory/'context.tar';context_tar(context,df)
 tag=PREFIX if target=='runtime' else PREFIX+'-'+target
 image=REG+('/compute-v17:' if target=='runtime' else '/compute-v17-stage:')+tag
 command=['docker','buildx','build','--builder','neon-data','--platform','linux/amd64','--network','host','--allow','network.host','--resource','memory='+str(memoryGiB)+'g','--resource','memory-swap='+str(memoryGiB)+'g','--resource','cpu-quota=200000','--progress','plain','--target',target,'--build-arg','PG_VERSION=v17','--build-arg','DEBIAN_VERSION=bookworm','--build-arg','EXTENSIONS=none','--build-arg','DSGA_RUNTIME_PROFILE=default-loader-v1','--build-arg','BUILD_JOBS=2','--build-arg','GIT_VERSION='+NEON,'--build-arg','BUILD_TAG='+PREFIX,'--build-arg','PROBE_MEMORY_MAX_BYTES='+str(memoryGiB<<30)]
 for key in ['HTTP_PROXY','HTTPS_PROXY','NO_PROXY','http_proxy','https_proxy','no_proxy']:command+=['--build-arg',key]
 command+=['--label','org.opencontainers.image.revision='+NEON,'--label','io.neon.pg17.revision='+PG,'--label','io.neon.test-only=true','--label','io.neon.extensions=none','--label','io.neon.probe.source-manifest-sha256='+META['sourceManifestSHA256'],'--metadata-file',str(directory/'metadata.json'),'--output','type=image,name='+image+',push=true,registry.insecure=true','-f','ci/pre-main-probe.Dockerfile','-']
 state={'target':target,'state':'RUNNING','startUTC':now(),'image':image,'command':command,'contextSHA256':sha(context),'dockerfileSHA256':hashlib.sha256(df.encode()).hexdigest(),'sourceManifestSHA256':META['sourceManifestSHA256'],'maxRUNMemoryBytes':memoryGiB<<30,'maxRUNSwapBytes':0,'MemAvailableBeforeBytes':available(),'TEST_ONLY':True}
 jwrite(directory/'result.json',state)
 with context.open('rb') as stdin:rc,seconds=run_logged(command,directory/'build.log',5400,stdin)
 state.update({'state':'PASS_BUILD' if rc==0 else 'FAIL_BUILD','exitCode':rc,'seconds':seconds,'endUTC':now(),'MemAvailableAfterBytes':available()})
 if rc==0:
  data=json.loads((directory/'metadata.json').read_text());digest=data['containerimage.digest'];state['digest']=digest
  record={'kind':'compute-stage' if target!='runtime' else 'runtime','pg':'v17','aliases':[target],'image':image,'digest':digest,'source_commit':NEON,'pg17_source_commit':PG,'sourceManifestSHA256':META['sourceManifestSHA256'],'target':target,'EXTENSIONS':'none','TEST_ONLY':True}
  jwrite(STAGES/(target+'.json'),record)
 jwrite(directory/'result.json',state)
 if rc:raise RuntimeError(target+' failed rc='+str(rc)+'; source/log/failure retained')
 return state
def resume_build(target,memoryGiB=8):
 recordPath=STAGES/(target+'.json')
 if recordPath.exists():
  rec=json.loads(recordPath.read_text())
  state=json.loads((OUT/target/'result.json').read_text())
  assert rec['source_commit']==NEON and rec['pg17_source_commit']==PG and rec['sourceManifestSHA256']==META['sourceManifestSHA256']
  assert state['state']=='PASS_BUILD' and state['exitCode']==0 and state['digest']==rec['digest']
  assert registry_digest('neon/compute-v17-stage',rec['image'].rsplit(':',1)[1])==rec['digest']
  jwrite(OUT/(target+'-resume-proof.json'),{'status':'REUSED_EXACT_SUCCESSFUL_STAGE_NOT_RECOMPILED','image':rec['image'],'digest':rec['digest'],'sameSourceManifest':True})
  return state
 return build(target,memoryGiB)

def main():
 assert (OUT/'proxy-health.rc').read_text().strip()=='0'
 proof=OUT/'resource-proof-swap-fresh'
 assert (OUT/'resource-proof-swap-fresh.rc').read_text().strip()=='0'
 assert (proof/'self-memory.max').read_text().strip()=='536870912'
 assert (proof/'self-memory.swap.max').read_text().strip()=='0'
 assert (proof/'self-cpu.max').read_text().strip()=='100000 100000'
 seed_dependency_record()
 # 两项独立且总上界5GiB；后面的重Rust构建显式串行，单RUN8GiB不叠加两个。
 with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
  futures=[pool.submit(resume_build,'pg-build',3),pool.submit(resume_build,'build-deps-with-cargo',2)]
  results=[f.result() for f in futures]
 resume_build('pg-build-with-cargo',2)
 resume_build('compute-tools',8)
 resume_build('neon-ext-build',8)
 runtime=resume_build('runtime',2)
 image=runtime['image']+'@'+runtime['digest'];rc,secs=run_logged(['docker','pull',image],OUT/'final-docker-pull.log',300)
 if rc:raise RuntimeError('final image built but Docker pull failed '+str(rc))
 subprocess.run(['docker','tag',image,runtime['image']],check=True)
 info=json.loads(subprocess.check_output(['docker','image','inspect',runtime['image']],text=True))[0]
 jwrite(OUT/'final-image.json',{'TEST_ONLY':True,'formalRelease':False,'normalQualification':False,'runtimeTag':runtime['image'],'runtimeDigest':runtime['digest'],'DockerImageID':info['Id'],'RepoDigests':info.get('RepoDigests'),'labels':info['Config'].get('Labels'),'bytes':info['Size'],'entrypoint':info['Config'].get('Entrypoint'),'user':info['Config'].get('User'),'actualTarget':'runtime','EXTENSIONS':'none','omittedCommunityExtensionStages':39,'neon':NEON,'PG17':PG,'seconds':time.monotonic()-start})
 jwrite(OUT/'pipeline-result.json',{'status':'PASS_TEST_ONLY_BUILD_DOCKER_VISIBLE','startUTC':START,'endUTC':now(),'seconds':time.monotonic()-start,'TEST_ONLY':True,'VMShrinkPermit':False})

START=now()
try:
 health=[]
 for label,url,want in [('GitHub','https://github.com','200'),('Rust1.88','https://static.rust-lang.org/dist/channel-rust-1.88.0.toml.sha256','200'),('DockerRegistryAuthChallenge','https://registry-1.docker.io/v2/','401')]:
  cp=subprocess.run(['curl','--proxy','http://127.0.0.1:7890','--connect-timeout','10','--max-time','30','-sS','-o','/dev/null','-w','%{http_code}',url],capture_output=True,text=True)
  health.append({'label':label,'curl_exit':cp.returncode,'HTTP_status':cp.stdout.strip(),'expected':want})
  jwrite(OUT/'runtime-proxy-health.json',health)
  if cp.returncode or cp.stdout.strip()!=want:raise RuntimeError('explicit Linux proxy health failed for '+label)
 resume_build('pg-build',3)
 resume_build('build-deps-with-cargo',2)
 resume_build('pg-build-with-cargo',2)
 resume_build('compute-tools',8)
 resume_build('neon-ext-build',8)
 runtime=resume_build('runtime',2)
 ref=runtime['image']+'@'+runtime['digest']
 rc,_=run_logged(['docker','pull',ref],OUT/'runtime-docker-pull.log',300)
 if rc:raise RuntimeError('runtime built but Docker pull failed')
 subprocess.run(['docker','tag',ref,runtime['image']],check=True)
 info=json.loads(subprocess.check_output(['docker','image','inspect',runtime['image']],text=True))[0]
 jwrite(OUT/'final-image.json',{'TEST_ONLY':True,'formalRelease':False,'normalQualification':False,'runtimeTag':runtime['image'],'runtimeDigest':runtime['digest'],'DockerImageID':info['Id'],'RepoDigests':info.get('RepoDigests',[]),'bytes':info['Size'],'neon':NEON,'PG17':PG,'EXTENSIONS':'none','omittedCommunityExtensions':39,'loaderProfile':'default-loader-v1','sourceManifestSHA256':META['sourceManifestSHA256'],'two_file_profile_overlay':META['loaderProfileOverlay'],'seconds':time.monotonic()-start})
 jwrite(OUT/'runtime-pipeline-result.json',{'status':'PASS_EXACT_NONE_RUNTIME_DOCKER_VISIBLE','formalRelease':False,'normalQualification':False,'startUTC':START,'endUTC':now(),'seconds':time.monotonic()-start})
except BaseException as e:
 jwrite(OUT/'runtime-pipeline-result.json',{'status':'FAIL_OR_ENV_BLOCK','error':type(e).__name__+': '+str(e),'startUTC':START,'endUTC':now(),'seconds':time.monotonic()-start,'formalRelease':False,'normalQualification':False})
 raise
