#!/usr/bin/env python3
"""汇总各自准确快照的PG17证据，不将协议peer/环境门槛计为真实E2E。"""
import datetime
import hashlib
import json
import re
from pathlib import Path
import subprocess

BASE=Path('/data/neon-build')
SRC=BASE/'src'
rows=[]
evidence=[]
def read(path):
    path=Path(path)
    evidence.append({'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
    return json.loads(path.read_text())
def add(ident,layer,status,description,report,roots=1,**extra):
    rows.append({'id':ident,'layer':layer,'status':status,'description':description,
                 'evidence':str(report),'root_count':roots,**extra})

p=SRC/'autoscaling/docs/validation/pg17-p1-node-chain-linux-20261030/catalog.json'
p1=read(p)
for row in p1['cases']:
    add(row['id'],row['layer'],row['status'],row['root'],row['run_directory'],
        scope=row.get('scope',''),race=row.get('race'),overall_run_exit_code=row.get('run_outer_exit_code'))
p=SRC/'autoscaling/docs/validation/pg17-u-p2p3-linux-20261030/summary.json'
p2=read(p)
for row in p2['groups']:
    add(row['id'],'unit','PASS' if row['root_counts']['fail']==0 else 'FAIL',row['description'],p,len(row['roots']),root_names=row['roots'])
p=SRC/'autoscaling/docs/validation/pg17-u-p2p3-linux-20261030/native-service.json'
s=read(p)
add(s['id'],'single-service','PASS' if s['passed'] else 'FAIL',s['go_root'],s['raw_report'],scope=s['scope'])
p=SRC/'autoscaling/docs/validation/pg17-vm-placement-20261030/summary.json'
v=read(p)
for row in v['tests']:
    add(row['id'],'unit',row['result'],row['root'],v['raw_directory'])
for name,file in [('pg17-linux-native-20261030','single-service.json'),('pg17-linux-budget-20261030','result.json')]:
    p=SRC/'neon/docs/validation'/name/file
    result=read(p)
    for row in result['tests']:
        add(row['id'],'single-service',row['result'],row['test'],p,
            scope='准确Linux PG17与Neon原生服务；无QEMU/VM物理资源证明')
# 物理 kernel U 仅新增三根；原两根拒绝回归已在 P2 计数。
p=SRC/'autoscaling/docs/validation/pg17-physical-kernel-component-20261030/summary.json'
physical=read(p)
assert physical['new_roots']==3
for ident,root in [
 ('PG17-U-P2-004','TestPhysicalKernelObservationParsers'),
 ('PG17-U-P2-005','TestPhysicalKernelObservationMatchRejectsDrift'),
 ('PG17-U-P2-006','TestPhysicalKernelObservationRejectsSummedCapacityOverflow')]:
    add(ident,'unit','PASS',root,p)
p=SRC/'autoscaling/docs/validation/pg17-native-physical-kernel-component-20261030/result.json'
kernel=read(p)
assert kernel['status']=='PASS_COMPONENT_ONLY' and not kernel['actualObservation']['vm_shrink_permit']
add(kernel['id'],'single-service-component',kernel['status'],kernel['testRoot'],kernel['reportDirectory'],
    scope='真实KVM/准确kernel物理只读观察；无PG/normal写/VM伸缩/原出生封存')
p=SRC/'autoscaling/docs/validation/pg17-exec-environment-linux-20261031/summary.json'
execution=read(p)
for row in execution['cases']:
    add(row['id'],row['layer'],row['status'],row['root'],p)
# 同次首次归档的9根既有AS/source/WAL回归也独立列出；两个normal拒绝根不重复。
p=SRC/'autoscaling/docs/validation/pg17-exec-environment-linux-20261031/adopted-tests.json'
execution_events=read(p)
address_roots=sorted(x['Test'] for x in execution_events if x['Action']=='pass' and x['Test'].startswith('TestAddressSpace') and '/' not in x['Test'])
assert len(address_roots)==9
for i,root in enumerate(address_roots,1):
    add(f'PG17-U-AS-{i:03d}','unit','PASS',root,p,scope='受管启动直接关联的既有AS/原来源/WAL回归')
p=SRC/'autoscaling/docs/validation/pg17-native-birth-wire-linux-20261031/summary.json'
wire=read(p)
for row in wire['cases']:
    add(row['id'],row['layer'],row['status'],row['root'],p,scope='独立wire组件；无生产listener/maps/filter/入组/normal许可')
p=SRC/'autoscaling/docs/validation/pg17-physical-kernel-origin-20261031/summary.json'
origin=read(p)
for i,root in enumerate(['TestPhysicalKernelOriginCapturesAndKeepsOriginal','TestPhysicalKernelOriginRejectsSourceAndHistoryDrift','TestPhysicalKernelOriginPendingRequiresExplicitOriginalReplay','TestPhysicalKernelOriginSerializesWithOriginalResourceLock'],7):
    add(f'PG17-U-P2-{i:03d}','unit','PASS',root,p,scope='物理Origin组件U，真实flock/fsync但模型kernel/ISO；无生产接线')
p=SRC/'neon/vendor/postgres-v17/doc/dsga-validation/early-birth-linux-20261031/ci-definer-result.json'
birth=read(p)
assert birth['passed'] and birth['tests_run']==6 and birth['skipped']==0
for row in birth['tests']:
    add(row['id'],'unit-native' if row['layer']=='unit-native' else 'single-service',row['result'],row['test'],p,scope='真实原生PG test-v1；未绑定Root；非完整normal许可')
p=SRC/'neon/vendor/postgres-v17/doc/dsga-validation/early-birth-linux-20261031/v1-regression-result.json'
birth_reg=read(p)
# 既有格式拒绝根已是S-PG-019，不重复；其余4个直接启动/logger回归首次Linux归档。
for i,row in enumerate([x for x in birth_reg['tests'] if 'test_noncanonical_or_unbounded_values_fail_before_any_mode' not in x['test']],1):
    add(f'PG17-S-BIRTH-REG-{i:03d}','single-service',row['result'],row['test'],p,scope='原生v1准确快照直接回归；不冒称v2/当前纯HEAD重跑')
p=SRC/'neon/docs/validation/pg17-compute-entrypoint-permissions-20261031/summary.json'
permission=read(p)
add(permission['id'],'build-component',permission['status'],'正式Dockerfile v17条件与实际权限层执行',p,roots=None)
# 旧0ab Guest对wire fixture暴露竞态；Host修复后目标Guest正在新快照重验。
add('PG17-S-WIRE-001@kernel-6.12.26','platform-retest','RETEST_PENDING','旧KVM失败保留；仅fixture同步已修/Host PASS，待新准确Guest结果',SRC/'autoscaling/docs/validation/pg17-native-birth-wire-kvm-20261031/attempt-01/result.json',roots=None)
p=SRC/'neon-helm/docs/acceptance/pg17-helm-linux-20261030.json'
h=read(p)
for name in ['foundation-summary.json','dsga-summary.json']:
    path=p.with_suffix('')/name
    doc=read(path)
    for row in doc['results']:
        add(row['id'],'unit-gate' if '-U-' in row['id'] else 'configuration-contract',row['status'].upper(),row['description'],path,roots=None)
if h.get('nodeSourceCheck'):
    add(h['nodeSourceCheck']['id'],'unit-gate','PASS','仓库Node24源码/锁/10 chart静态策略',h['nodeSourceCheck']['evidence'],roots=None)
add('PG17-ENV-H01','environment','PASS','只预取准确模块，不运行测试',h['environmentPreparation']['evidence'],roots=None)
for ident,desc in h['notExecutedRuntimeGates'].items():
    add(ident,'runtime-prerequisite','NOT_READY',desc,p,roots=None)
add('PG17-S-NORMAL-001','single-service','REQUIRED_IN_PROGRESS','安全降低同一PG工作域normal硬上限；持续私有/后台、文件、kernel/IO及在途包络尚未完整实现',BASE/'plan/PG17至PG18-最终实施计划.txt',roots=None)
for ident,desc in [
 ('PG17-E-001','正式owner→PG→Guest资源→VM→Host→Finalize自动Grow/Shrink/Regrow'),
 ('PG17-E-002','实际Linux/KVM/cgroup/HugeTLB/normal下降及无新增OOM/重启'),
 ('PG17-E-003','仅117/118/119的Helm真实部署与故障恢复完整回归'),
 ('PG17-E-004','PG17 K8s通过后，以PG17后端接入届时最新控制面并补齐全部官方功能的联合验收'),
 ('PG18-E-001','独立并行PG18内核适配及相同上层合同全量联合回归'),
 ('PG16-E-001','后续PG16独立内核适配与全量联合回归')]:
    add(ident,'real-end-to-end','NOT_RUN',desc,BASE/'plan/PG17至PG18-最终实施计划.txt',roots=None)


# 2026-10-31按准确已执行cohort补充；只登记此前未有的根，不重复统计直接旧回归。
known_roots={name for row in rows for name in row.get('root_names',[])}
known_roots.update(row['description'] for row in rows if row.get('root_count')==1)
def unique_add(ident,layer,status,root,report,**extra):
    if root in known_roots or any(row['id']==ident for row in rows):
        return
    known_roots.add(root)
    add(ident,layer,status,root,report,**extra)
def go_roots(path,prefix,scope):
    raw=path.read_bytes()
    evidence.append({'path':str(path),'sha256':hashlib.sha256(raw).hexdigest()})
    events=[json.loads(line) for line in raw.decode().splitlines() if line.strip()]
    assert not any(x.get('Action') in ('fail','skip') for x in events)
    names=sorted(x['Test'] for x in events if x.get('Action')=='pass' and x.get('Test') and '/' not in x['Test'])
    for name in names:
        unique_add(prefix+'-'+hashlib.sha256(name.encode()).hexdigest()[:8],'unit','PASS',name,path,scope=scope,race=True)

p=SRC/'autoscaling/docs/validation/pg17-prebootstrap-system-20261031/validation.json'
for row in read(p)['cases']:
    unique_add(row['id'],'unit',row['status'],row['test'],p,scope='原552 PreSys模型kernel/ISO+真实flock/fsync；后续369修复另有快照，不冒称E')
go_roots(SRC/'autoscaling/docs/validation/physical-four-build-20261031/go/unit.json','PG17-U-PHYSBUILD','四个准确内核来源；已登记旧根自动去重；200实际节点包含直接回归')
go_roots(SRC/'autoscaling/docs/validation/kernel-bootstrap-20261031/go/unit.json','PG17-U-KERNELPREP','原内核准备模型kernel/ISO+真实持久WAL；原nonce与恢复')
p=SRC/'autoscaling/docs/validation/pg17-normal-split-topology-20261031/catalog.json'
new_split=read(p)
for row in new_split['unit']['cases']:
    unique_add(row['id'],'unit',row['status'],row['test'],p,scope='双子域模型kernel+真实flock/fsync；修复pending身份重fsync后的准确快照')
for ident in new_split['guest']['caseIDs']:
    unique_add(ident,'single-service-component','PASS',ident,p,scope='准确369真实cgroup/ext4/KVM；Source/ISO是明确夹具，无PG/normal许可')
p=SRC/'autoscaling/docs/validation/native-filter-audit-go-20261031/catalog.json'
for row in read(p)['cases']:
    unique_add(row['id'],'unit',row['result'],row['root'],p,scope='完整BPF解码/内核链比较模型U；实际特权读者路径另验')
p=SRC/'autoscaling/docs/validation/qmp-memory-geometry-20261031/catalog.json'
for row in read(p)['cases']:
    unique_add(row['id'],'unit' if row['class']=='U' else 'single-service-component',row['result'],row['name'],p,scope='真实QMP schema；S为CPU暂停QEMU/KVM，无Guest/PG，无normal许可')
p=SRC/'neon/docs/validation/pg17-native-root-gate-20261031/attempt-03/test-result.json'
new_root=read(p)
assert new_root['passed'] and new_root['testsRun']==9
for row in new_root['tests']:
    name=row['test']
    ident=name.split('test_',1)[1].split('_',5)[:4]
    ident='-'.join(ident)
    unique_add(ident,'unit-native' if '_U_' in name else 'single-service',row['result'],name,p,scope='真实PG Root停点/过滤器；Host6.8隔离root夹具，不是369Guest/生产WAL/normal/K8s',family='native-pg-root-component')
# 原kernel loader三个根继续保留；下列是另一准确新cohort的组件场景，不跨层相加。
for ident,desc,report in [
 ('PG17-S-PHYSBUILD-3B','原生只读物理读者准确3b构建',SRC/'autoscaling/docs/validation/physical-four-build-20261031/native'),
 ('PG17-S-PHYSBUILD-369','原生只读物理读者准确369构建',SRC/'autoscaling/docs/validation/physical-four-build-20261031/native'),
 ('PG17-S-KERNELPREP-POSITIVE','369真实ISO/ext4首次原内核准备',SRC/'autoscaling/docs/validation/kernel-bootstrap-20261031/native'),
 ('PG17-S-KERNELPREP-REPLAY','369真实写后错误与同意图重放',SRC/'autoscaling/docs/validation/kernel-bootstrap-20261031/native'),
 ('PG17-S-KERNELPREP-MODULES','369无原intent但已禁模块拒绝',SRC/'autoscaling/docs/validation/kernel-bootstrap-20261031/native'),
 ('PG17-S-KERNELPREP-MOVABLE','369初期已含Movable拒绝',SRC/'autoscaling/docs/validation/kernel-bootstrap-20261031/native')]:
    unique_add(ident,'single-service-component','PASS',desc,report,scope='真实独立KVM组件，未生产接线/PG/normal资格')

# 最新批次按稳定case ID归并；同根的不同源码/Host/Guest只补观察，不新增覆盖。
# 下列读取原始报告并核状态；不能只因catalog写PASS就登记。
extension_baseline_ids={r['id'] for r in rows}
def track(path):
    path=Path(path)
    item={'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
    if item not in evidence:evidence.append(item)
    return item
def actual_go_roots(path,allow_cap_prefix=False):
    track(path);events=[]
    for line in Path(path).read_text().splitlines():
        if not line.strip():continue
        if allow_cap_prefix and re.fullmatch(r'CapEff:\s*[0-9a-f]{16}',line):continue
        events.append(json.loads(line))
    assert not any(x.get('Action') in ('fail','skip') for x in events),path
    assert any(x.get('Action')=='pass' and not x.get('Test') for x in events),path
    return sorted({x['Test'] for x in events if x.get('Action')=='pass' and x.get('Test') and '/' not in x['Test']})
def record_case(ident,layer,names,report,scope,refs=(),**extra):
    names=[names] if isinstance(names,str) else list(names)
    assert names and len(names)==len(set(names))
    refs=[track(p) for p in [report,*refs]]
    observation={'evidence':str(report),'scope':scope,'source_and_receipt_files':refs,**extra}
    existing=next((row for row in rows if row['id']==ident),None)
    if existing is not None:
        existing.setdefault('observations',[]).append(observation)
        return
    fresh=[]
    for name in names:
        prior=next((row for row in rows if name==row['description'] or name in row.get('root_names',[])),None)
        if prior is None:fresh.append(name)
        else:prior.setdefault('observations',[]).append(observation)
    if not fresh:return
    add(ident,layer,'PASS',fresh[0] if len(fresh)==1 else ident+' 原始根组',report,
        roots=len(fresh),root_names=fresh,scope=scope,observations=[observation],**extra)
    known_roots.update(fresh)

# 同S-WIRE-001准确KVM复测已通过，旧失败不删除；环境行仍不计新增根。
d=SRC/'autoscaling/docs/validation/pg17-native-birth-wire-kvm-20261031'
w=read(d/'attempt-02/result.json')
assert w['GuestTestExitCode']==0 and w['hostQemuExitCode']==0 and w['coverageDelta']==0
record_case('PG17-S-WIRE-001','single-service-component','TestNativeBirthWireKernelPeerAndMalformedFrames',d/'attempt-02/result.json',
    '同wire原根准确0ab KVM复测；只新增环境证据，旧失败保留，无PG/normal许可',
    refs=[d/'attempt-02/source-proof.json',d/'attempt-01/result.json'],platforms=['ACCURATE_0AB_KVM'])
pending=next(r for r in rows if r['id']=='PG17-S-WIRE-001@kernel-6.12.26')
pending.update(status='PASS_COMPONENT_ONLY',description='原S-WIRE-001准确KVM复测PASS；旧attempt01失败保留，coverageDelta0',evidence=str(d/'attempt-02/result.json'))

# Root tracer：六个模型U根组成原稳定组；三个内核S根跨Host/369只登记一次。
d=SRC/'autoscaling/docs/validation/pg17-native-birth-tracer-20261031'
t=read(d/'acceptance.json');model=read(d/'model-v6-result.json')
actual=actual_go_roots(d/'model/model-test.jsonl');assert actual==sorted(model['roots']) and len(actual)==6
record_case('PG17-U-TRACER-001','unit',actual,d/'model-v6-result.json','模型ptrace/身份/取消/清理/复用U；不称真实内核',
    refs=[d/'acceptance.json',d/'cohort-410bcc-lock-v6.json',d/'model/model-test.jsonl'],race=True)
host=read(d/'host/result.json');guest=read(d/'guest-369/result.json')
assert host['passed'] and guest['passed'] and guest['queryKVM']['enabled'] and guest['hostQemuExitCode']==0 and guest['actualGuestExitCodes']==[0]
assert host['GoRootsPassed']==guest['GoRootsPassed'] and len(host['GoRootsPassed'])==3
for ident,name in zip(host['id'],host['GoRootsPassed']):
    record_case(ident,'single-service-component',name,d/'acceptance.json','真实Go+PG ptrace组件；Source/WAL为夹具，不是生产出生/normal/E',
        refs=[d/'native-binary-source-proof-v6.json',d/'host/result.json',d/'guest-369/result.json',d/'historical-attempts.json'],platforms=['HOST_DOCKER','ACCURATE369_KVM'],family='go-pg-tracer-component')
d=SRC/'autoscaling/docs/validation/pg17-tracer-home-node-deny-20261031'
retarget=read(d/'acceptance.json')
assert retarget['host']['exitCode']==0 and retarget['guest']['passed'] and retarget['guest']['hostQemuExitCode']==0
for ident,name in zip(host['id'],host['GoRootsPassed']):
    record_case(ident,'single-service-component',name,d/'acceptance.json','同三根用PG627新home_node BPF重验；不是新增根',
        refs=[d/'source-combination.json',d/'actual-pg-elf-proof.json',d/'guest-369-attempt-03/result.json'],platforms=['HOST_DOCKER','ACCURATE369_KVM'],coverage_delta=0)

# 原Source/Stage/ELF：保留原3个稳定组ID，按实际9根计数，不能把47子例算47根。
d=SRC/'autoscaling/docs/validation/native-birth-source-stage-20261031'
s=read(d/'acceptance.json');source_result=read(d/'result.json')
actual=actual_go_roots(d/'actual-copy-and-tests/model-test.jsonl')
assert actual==sorted(source_result['roots']) and len(actual)==9 and source_result['skip']==0
groups=[('PG17-U-BIRTHSOURCE-001',[x for x in actual if x.startswith('TestNativeBirthSource') and 'ELFDynamicMetadata' not in x]),
        ('PG17-U-BIRTHSTAGE-001',[x for x in actual if x.startswith('TestNativeBirthStage')]),
        ('PG17-U-BIRTHSOURCE-ELF-001',[x for x in actual if 'ELFDynamicMetadata' in x])]
assert [len(x[1])for x in groups]==[4,4,1]
for ident,names in groups:
    record_case(ident,'unit',names,d/'result.json','原字节/ABI/nonce与真实flock/fsync，kernel/ISO/PG身份为模型；ELF仅元数据，无执行/生产RO闭包S',
        refs=[d/'source-lock.json',d/'acceptance.json',d/'actual-copy-and-tests/model-test.jsonl'],race=True)

# 桥接新4根；25根cohort中其余21是直接回归，不再作为新增。
d=SRC/'autoscaling/docs/validation/bootstrap-presystem-bridge-20261031'
b=read(d/'catalog.json');bridge_roots=actual_go_roots(d/'unit/unit.json',allow_cap_prefix=True)
assert len(bridge_roots)==25 and b['new_unit_roots']==4
for case in b['cases']:
    assert case['status']=='PASS' and case['root'] in bridge_roots
    record_case(case['id'],'unit',case['root'],d/'catalog.json','PreSys→正式Bootstrap同原WAL引用；真实flock/fsync、模型内核；非normal许可',
        refs=[d/'source-proof.json',d/'unit/unit.json'],race=True)

# PreSys v2另新增4根（原5根/KernelPrep7/桥4为回归）；中央首次分配固定ID。
d=SRC/'autoscaling/docs/validation/presys-v2-20261031'
pre=read(d/'go/source-proof.json');pre_roots=actual_go_roots(d/'go/unit.json')
assert len(pre_roots)==20 and pre['actual_COPY_matches'] and pre['U_fail']==pre['U_skip']==0
pre_new=['TestPrebootstrapSystemProfileVersionsAndOriginalMarkers','TestPrebootstrapSystemSplitProfileSameOriginalRecovery',
         'TestPrebootstrapSystemCompletedPGRequiresAppliedOriginalBootstrap','TestPrebootstrapSystemKernelAdaptersKeepEarlyCensusAndRecursiveEmpty']
for i,name in enumerate(pre_new,1):
    assert name in pre_roots
    record_case(f'PG17-U-PRESYS-V2-{i:03d}','unit',name,d/'go/source-proof.json','PreSys v1/v2版本、split递归空、历史/原来源；模型kernel与真实WAL',
        refs=[d/'go/unit.json'],race=True,case_id_source='中央固定根名映射首次分配；后续保持')
pn=read(d/'native/result.json');assert pn['cases']==pn['passed']==3
for row in pn['results']:
    assert row['status']=='PASS_COMPONENT_ONLY' and row['host_qemu_exit']==0 and row['actual_qmp_kvm']['enabled']
    ident='PG17-S-PRESYS-V2-'+row['mode'].upper()
    record_case(ident,'single-service-component',row['mode'],d/'native/result.json','真实369内核/ISO/ext4、原Prepare→Origin→PreSys→Bootstrap API；角色为sleep夹具，无PG/normal/E',
        refs=[d/'native/source-lock.json',d/'go/source-proof.json',d/'failure-references.json'],platforms=['ACCURATE369_KVM'],case_id_source='中央按原固定mode首次分配；后续保持')

d=SRC/'autoscaling/docs/validation/guest-maximum-region-20261031'
maximum=read(d/'catalog.json');mproof=read(d/'source-proof.json');mr=actual_go_roots(d/'go/unit.json')
assert len(mr)==5 and mproof['actual_COPY_all_go_hashes_match']
for case in maximum['U']:
    assert case['root'] in mr
    record_case(case['id'],'unit',case['root'],d/'catalog.json','Guest不可变virtio region读者U，严格观察与几何比较，无资源写许可',
        refs=[d/'source-proof.json',d/'go/unit.json'],race=True)
mn=read(d/'native/result.json');assert mn['host_qemu_exit']==0 and mn['status']=='PASS_READONLY_INTERFACE_PROBE' and mproof['native']['Guest_exit']==0
for case in maximum['S']:
    record_case(case['id'],'single-service-component',case['root'],d/'native/result.json',case['scope'],
        refs=[d/'source-proof.json',d/'native/source-lock.json'],platforms=['ACCURATE369_KVM'])

# Runtime 只新增9 U根；同cohort旧3根仅回归，Darwin编译只算ENV。
d=SRC/'autoscaling/docs/validation/runtime-policy-templates-20261031'
rt=read(d/'acceptance-final.json');rr=actual_go_roots(d/'attempt04/test.json')
assert rt['testRC']==0 and rt['newRoots']==9 and len(rr)==12 and rt['skip']==0 and rt['actualCOPY']['exactInputMatchesActualCOPY']
for case in rt['newTests']:
    assert case['root'] in rr and case['status']=='PASS'
    record_case(case['id'],'unit',case['root'],d/'acceptance-final.json','原UID/Ready与严格v2模板纯生产者/渲染U；Fake Kube API、模型预算，不是实际ISO/Root/WAL/normal',
        refs=[d/'cohort-source-attempt04.json',d/'actual-copy-proof-final.json',d/'attempt04/test.json',d/'before-entry-fix/test.json'],race=True)
add('PG17-ENV-RUNTIME-TEMPLATES-001','environment','PASS_COMPILE_ONLY','Linux上Darwin/amd64交叉编译；不计U/S，未在Mac运行',d/'platform-darwin-final/build.rc',roots=None)
track(d/'platform-darwin-final/build.rc');track(d/'Dockerfile-platform-final')

# 原生Root9/early6复测按原ID归并；过滤器4S+1binary U旧总表漏登，补实际来源。
d=SRC/'neon/vendor/postgres-v17/doc/dsga-validation/filter-numa-deny-20261031'
fc=read(d/'catalog.json')
for filename in ['root9-result.json','compatibility11-result.json']:
    result=read(d/filename);assert result['passed'] and result['testsRun']==result['expectedRoots'] and all(x['result']=='PASS'for x in result['tests'])
    for case in result['tests']:
        match=re.search(r'test_(PG17_[US]_[A-Z0-9]+_[0-9]{3})_',case['test']);assert match,case
        ident=match.group(1).replace('_','-')
        record_case(ident,'unit-native' if '_U_' in case['test'] else 'single-service',case['test'],d/filename,
            '真实PG17 native TEST_ONLY，default-off/test-v1兼容与fork/logger/filter；无Root完整来源/normal/E',
            refs=[d/'catalog.json',d/'actual-image-source-and-elf.json'],family='native-pg-filter-component')
mechanism=read(d/'mechanism13-result.json');obs=read(d/'mechanism13-observations.json')
assert mechanism['pass'] and mechanism['roots']==1 and mechanism['skip']==0 and not mechanism['errors']
assert len(obs['actual_native_child']['calls'])==13 and all(x=={'result':-1,'errno':1}for x in obs['actual_native_child']['calls'].values())
record_case('PG17-S-MIGFILTER-001','single-service','test_PG17_S_MIGFILTER_001_actual_native_child_denies_migration_and_import',d/'mechanism13-result.json',
    '真实PG17 child的13个NUMA/迁移/namespace/uring/userfault导入调用拒绝；13机制场景共一根，Host不是KVM',
    refs=[d/'catalog.json',d/'mechanism13-runner.py',d/'mechanism13-observations.json'],family='native-pg-filter-component',syscall_subcases=13)

# 独立memcg/匿名map_files/inflight机制按原稳定case ID登记；不混入原生PG S或Go根。
d=SRC/'autoscaling/docs/validation/pg17-split-charge-kvm-component-20261031/attempt-01'
split_probe=read(d/'result.json');assert split_probe['passed'] and split_probe['queryKVM']['enabled'] and split_probe['hostQemuExitCode']==0 and split_probe['actualGuestExitCodes']==[0]
for ident,desc in zip(split_probe['id'],['真实shmem首次触页/洞fault分别收费arrays/private','真实pipe保活folio跨punch后收费保留直至释放']):
    record_case(ident,'single-service-component',desc,d/'result.json','原3b KVM独立memfd收费机制，无真实PG/降normal/生产Root',
        refs=[d/'source-and-kernel-lock.json',d/'actual-facts.json'],platforms=['ACCURATE3B_KVM'])
d=BASE/'validation/pg17-anonymous-mapfiles-kvm-attempt01-20261030T223019Z-23d6ef'
mapfiles=read(d/'catalog.json');assert mapfiles['runtime']['pass'] and mapfiles['runtime']['actualKVM']['enabled'] and mapfiles['runtime']['hostQemuExit']==0 and mapfiles['runtime']['GuestExit']==['0']
for case in mapfiles['cases']:
    assert case['result']=='PASS'
    record_case(case['id'],'single-service-component',case['detail'],d/'catalog.json','原3b KVM匿名SHARED map_files FD/代次/预触独立组件，无真实PG或memcg收费资格',
        refs=[d/'source-lock.json',d/'result.json'],platforms=['ACCURATE3B_KVM'])
d=SRC/'autoscaling/docs/validation/normal-inflight-source-20261031'
inflight=read(d/'native-retire/result.json');assert inflight['status']=='PASS_COMPONENT_ONLY' and inflight['actual_qmp_kvm']['enabled'] and inflight['host_qemu_exit']==inflight['gdb_exit']==0
record_case(inflight['id'],'single-service-component','准确369实际31-folio uncharge→retire窗口断点',d/'native-retire/result.json',inflight['scope'],
    refs=[d/'source-lock.json',d/'conditional-equations.json',d/'first-fixture-failure/result.json'],platforms=['ACCURATE369_KVM'])

# 原始失败/曾通过但源码已被替代的历史单列；最终PASS不会清除这些记录。
failure_history_catalogs=[]
for dirname,filename,key in [
 ('native-birth-source-stage-20261031','acceptance.json','historical'),
 ('runtime-policy-templates-20261031','acceptance-final.json','history'),
 ('presys-v2-20261031','failure-references.json',None),
 ('pg17-native-birth-tracer-20261031','historical-attempts.json',None)]:
    path=SRC/'autoscaling/docs/validation'/dirname/filename;doc=read(path)
    failure_history_catalogs.append({'catalog':track(path),'records':doc if key is None else doc[key]})
path=SRC/'neon/docs/validation/pg17-native-root-gate-20261031/catalog-final.json';doc=read(path)
failure_history_catalogs.append({'catalog':track(path),'records':doc['firstFailuresPreserved']})
for path in [SRC/'autoscaling/docs/validation/normal-inflight-source-20261031/first-fixture-failure/result.json',
             SRC/'autoscaling/docs/validation/guest-maximum-region-20261031/failures/native-evaluator-reevaluation.json',
             SRC/'autoscaling/docs/validation/pg17-native-birth-wire-kvm-20261031/attempt-01/result.json']:
    failure_history_catalogs.append({'catalog':track(path),'records':read(path)})

# 尚未交付的helper不增加正式覆盖；独立PG18档案另列，不混PG17任何计数。
helper_dir=BASE/'validation/array-populate-c-20261031T140854Z-875e0e'
development=[{'id':'PG17-DEV-ARRAY-HELPER','status':'IN_PROGRESS_NOT_COVERAGE','root_count':0,
    'scope':'C工具GNU12静态编译已过；共享tracer生命周期仍在接线，真实S未闭，不计新增U/S/E',
    'receipt':str(helper_dir),'production_qualification':False}]
pg18_archives=[]
for name in ['validation-first-leaf-v1.json','validation-arrays-core-v1.json','validation-resize-core-v1.json','validation-resize-failures-v1.json']:
    path=SRC/'postgres-v18/src/test/modules/test_dsg_region18'/name
    doc=read(path)
    pg18_archives.append({'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
        'scope':doc.get('scope',doc.get('stage',doc.get('status'))),'status':'ARCHIVED_EXACT_NATIVE_COMPONENT_NOT_RELEASE',
        'included_in_pg17_counts':False,'source_and_result_mapping':'see exact module archive; per-phase counts include direct regressions and must not be summed',
        'git_state':'see versions_at_index_capture; stage4 v3 freeze is pending owner review/commit at capture if still untracked'})
extension_new_ids=[row['id']for row in rows if row['id']not in extension_baseline_ids]
dedup_rules={'identity':'stable case ID first, then exact root name/root_names; original grouped IDs retain group root_count',
    'multi_platform':'same ID Host/369/newBPF appends observations, coverage delta0',
    'regression':'explicit old roots in latest cohorts attach evidence only; no new count',
    'layers':'unit Go roots, native-pg binary U, real PG S, isolated component case IDs, protocol fixtures and real E stay separate',
    'failure_history':'original failure/superseded receipts remain referenced; final PASS does not overwrite old files',
    'pg18':'module archives separate, no PG17 or E count'}

assert len({row['id'] for row in rows})==len(rows),'test IDs must be unique'

versions={}
for repo in ['neon','autoscaling','neon-helm','control-plane','postgres-v18']:
    path=SRC/repo
    versions[repo]={'head':subprocess.check_output(['git','-C',str(path),'rev-parse','HEAD'],text=True).strip(),
                    'branch':subprocess.check_output(['git','-C',str(path),'branch','--show-current'],text=True).strip(),
                    'worktree_status':subprocess.check_output(['git','-C',str(path),'status','--porcelain'],text=True).splitlines()}
report={'schema_version':1,'updated_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'meaning':'按每项实际源码快照归档，不是当前全部工作树或整体产品统一认证；原始失败与未执行保留',
        'release_gate':'首版安全normal降限；单机用于快速定位和回滚，最终117/118/119真实Helm K8s E2E；之后最新控制面以PG17后端补齐官方全部功能，PG18独立并行后联合回归',
        'versions_at_index_capture':versions,'evidence_files':list({(x['path'],x['sha256']):x for x in evidence}.values()),'cases':rows,'dedup_rules':dedup_rules,'latest_extension_case_ids':extension_new_ids,'latest_extension_delta':{layer:sum(row.get('root_count') or 0 for row in rows if row['id'] in extension_new_ids and row['layer']==layer) for layer in sorted({row['layer'] for row in rows if row['id'] in extension_new_ids})},'previous_index_counts':{'core_unit_roots_archived':127,'native_pg_single_service_roots':33,'native_pg_binary_unit_roots':5,'real_end_to_end_passed':0},'development_tracks':development,'failure_history_catalogs':failure_history_catalogs,'pg18_module_archives_separate':pg18_archives,
        'counts':{'core_unit_roots_archived':sum(row['root_count'] for row in rows if row['layer']=='unit' and row['status']=='PASS'),
                  'native_pg_single_service_roots':sum(row.get('root_count') or 0 for row in rows if row['layer']=='single-service' and row['status']=='PASS' and row['id']!='PG17-S-P2-004'),'native_pg_binary_unit_roots':sum(row.get('root_count') or 0 for row in rows if row['layer']=='unit-native' and row['status']=='PASS'),'independent_service_component_case_ids':sum(row['layer']=='single-service-component' and row['status'].startswith('PASS') for row in rows),'go_pg_tracer_component_roots':sum(row.get('root_count') or 0 for row in rows if row.get('family')=='go-pg-tracer-component'),'cross_language_pg_single_service_roots':1,'kernel_loader_wire_component_roots':3,
                  'peer_service_integration_roots':1,'peer_protocol_integration_roots':4,
                  'helm_unit_gates':sum(row['layer']=='unit-gate' and row['status']=='PASS' for row in rows),'helm_configuration_gates':4,'real_end_to_end_passed':0},
        'preliminary_control_plane_archive':{'head':'be30ba5e8d774781ff76194128720dd6f5d6e045','unit_roots':12,'metadata_service_roots':5,
          'reference':'control-plane/docs/validation/pg17-frozen-placement-linux-20261030','note':'前期最小VM修复；不是最终最新控制面对接基线，不计当前真实E'},
        'visible_images':'docker images | grep pg17-7bbabd0f9869 (30 old baseline images, not final release)',
        'not_release_certified':True}
jsonfile=BASE/'validation/PG17-测试用例总表.json'
jsonfile.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
lines=['PG17及后续内核：测试用例总表',report['updated_utc'],report['meaning'],report['release_gate'],'',
       f"已归档：PG17核心Go单元{report['counts']['core_unit_roots_archived']}根；原生PG单服务{report['counts']['native_pg_single_service_roots']}根、原生binary单元{report['counts']['native_pg_binary_unit_roots']}根；跨语言真实PG单服务1根；kernel/loader/wire独立组件3根；peer单服务1根、协议集成4根。",
       f"Helm：{report['counts']['helm_unit_gates']}个单元门槛、4个配置/类型门槛；真实PG/Guest/VM/K8s E2E通过=0。",
       '各类计数不混算；kernel/loader/wire三个根为早期类别，包含在下列独立组件统计中，不能重复相加；组件case ID不是Go根或PG单服务根。单元/配置通过不表示可封版。详细来源/命令/失败见各仓库收据。','']
for layer in dict.fromkeys(r['layer'] for r in rows):
    lines.extend([layer,'编号 | 结果 | 覆盖/用例 | 证据'])
    for row in rows:
        if row['layer']==layer:
            lines.append(' | '.join([row['id'],row['status'],row['description'],row['evidence']]))
    lines.append('')
lines.extend(['失败/曾通过但源码已替代的历史：见JSON failure_history_catalogs原路径与SHA，均未覆盖或删改。','新增独立组件按稳定case ID登记：'+str(report['counts']['independent_service_component_case_ids'])+'项；其中Go/PG tracer原生组件3根跨Host/369只计一次。',
              '开发状态（不增加覆盖）：'+development[0]['id']+' '+development[0]['status']+'；helper S与完整normal链尚未闭。',
              'PG18独立模块档案（不混PG17计数；各阶段包含回归，不可直接累加）：'])
for item in pg18_archives:lines.append(item['path']+' | '+item['status'])

(BASE/'validation/PG17-测试用例总表.txt').write_text('\n'.join(lines).rstrip()+'\n')
print(json.dumps(report['counts'],ensure_ascii=False))

# 同次生成准确产物副本；版本锁/gitlink不属于本脚本。
for name in ['PG17-测试用例总表.json','PG17-测试用例总表.txt']:
    (SRC/'neon/docs/validation'/name).write_bytes((BASE/'validation'/name).read_bytes())
