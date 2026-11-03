#!/usr/bin/env python3
"""只读显示PG17回归的组件、Pod、worker和Compute层次；不读取Secret或改集群。"""
import argparse
import concurrent.futures
import json
import subprocess


def main():
    a = argparse.ArgumentParser(description=__doc__)
    a.add_argument('--kubeconfig', default='/root/.kube/config')
    a.add_argument('--compute', default='cp-d8d85ed796068ab5')
    a.add_argument('--output', help='可选：另存不含凭据的现场JSON')
    args = a.parse_args()
    kube = ['kubectl', '--kubeconfig', args.kubeconfig, '--request-timeout=10s']
    def get(namespace, kind, name=None):
        cmd = kube + ['get', kind, '-n', namespace]
        if name:
            cmd += [name, '--ignore-not-found']
        x = subprocess.run(cmd + ['-o', 'json'], capture_output=True, text=True, timeout=20)
        if x.returncode:
            raise RuntimeError('集群查询失败：' + namespace + '/' + kind)
        return json.loads(x.stdout) if x.stdout.strip() else None
    rows = []
    def pods(ns):
        result = []
        for p in get(ns, 'pods')['items']:
            name = p['metadata']['name']
            if ns == 'kube-system' and not name.startswith(('autoscaler-agent-', 'autoscale-scheduler-')):
                continue
            cs = p.get('status', {}).get('containerStatuses', [])
            result.append({'namespace': ns, 'pod': name, 'node': p['spec'].get('nodeName', '未调度'),
                'phase': p.get('status', {}).get('phase', '未知'),
                'ready': str(sum(bool(c.get('ready')) for c in cs)) + '/' + str(len(p['spec']['containers'])),
                'containers': [c['name'] for c in p['spec']['containers']],
                'owner_UIDs': [o['uid'] for o in p['metadata'].get('ownerReferences', [])]})
        return result
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        for group in pool.map(pods, ['neon', 'neonvm-system', 'kube-system']):
            rows.extend(group)
    vm = get('neon', 'virtualmachines.vm.neon.tech', args.compute)
    compute = None
    if vm:
        compute = {'namespace': 'neon', 'name': args.compute, 'UID': vm['metadata']['uid'],
            'phase': vm.get('status', {}).get('phase', '未知'),
            'status_node': vm.get('status', {}).get('node'),
            'last_runner_pod': vm.get('status', {}).get('podName'),
            'current_owned_pods': [p['pod'] for p in rows if vm['metadata']['uid'] in p['owner_UIDs']]}
    print('层次：worker服务器 → Compute Pod → neonvm-runner容器 → QEMU虚拟机 → PG17')
    print('管理网关、runner-image-loader和autoscaler-agent均不是PG17 Compute Pod。')
    print('Compute VM：' + (json.dumps(compute, ensure_ascii=False) if compute else '对象不存在'))
    print('命名空间\tPod\tworker\t状态\tReady\t容器')
    for p in sorted(rows, key=lambda x: (x['namespace'], x['pod'])):
        print('\t'.join([p['namespace'], p['pod'], p['node'], p['phase'], p['ready'], ','.join(p['containers'])]))
    if args.output:
        from pathlib import Path
        Path(args.output).write_text(json.dumps({'compute': compute, 'pods': rows}, ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    main()
