#!/usr/bin/env python3
"""汇总各自准确快照的PG17证据，不将协议peer/环境门槛计为真实E2E。"""
import datetime
import hashlib
import json
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
        'versions_at_index_capture':versions,'evidence_files':evidence,'cases':rows,
        'counts':{'core_unit_roots_archived':sum(row['root_count'] for row in rows if row['layer']=='unit' and row['status']=='PASS'),
                  'native_pg_single_service_roots':27+sum(row.get('family')=='native-pg-root-component' and row['layer']=='single-service' for row in rows),'native_pg_binary_unit_roots':2+sum(row.get('family')=='native-pg-root-component' and row['layer']=='unit-native' for row in rows),'cross_language_pg_single_service_roots':1,'kernel_loader_wire_component_roots':3,
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
       '各类计数不混算；kernel/loader/wire三个根为早期类别，新增逐项single-service-component另列，不把场景数与Go根混算。单元/配置通过不表示可封版。详细来源/命令/失败见各仓库收据。','']
for layer in dict.fromkeys(r['layer'] for r in rows):
    lines.extend([layer,'编号 | 结果 | 覆盖/用例 | 证据'])
    for row in rows:
        if row['layer']==layer:
            lines.append(' | '.join([row['id'],row['status'],row['description'],row['evidence']]))
    lines.append('')
(BASE/'validation/PG17-测试用例总表.txt').write_text('\n'.join(lines).rstrip()+'\n')
print(json.dumps(report['counts'],ensure_ascii=False))
