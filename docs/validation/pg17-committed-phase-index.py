#!/usr/bin/env python3
"""按Git已提交目录追加阶段索引，不混入WIP、不把回归/重跑相加。"""
from pathlib import Path
import datetime, hashlib, json, subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT.parent
OUTPUT = ROOT / 'docs/validation/PG17-已提交阶段增量索引.json'
REPOSITORIES = {
    'autoscaling': SOURCES / 'autoscaling',
    'neon': ROOT,
    'control-plane': SOURCES / 'control-plane',
    'PG17': ROOT / 'vendor/postgres-v17',
    'PG18': SOURCES / 'postgres-v18',
}


def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args])


def update():
    rows, versions = [], {}
    for label, repo in REPOSITORIES.items():
        commit = git(repo, 'rev-parse', 'HEAD').decode().strip()
        versions[label] = {'head': commit, 'branch': git(repo, 'branch', '--show-current').decode().strip()}
        names = git(repo, 'ls-tree', '-r', '-z', '--name-only', commit, '--', 'docs/validation', 'doc/dsga-validation').decode().split('\0')
        for name in sorted(n for n in names if n.endswith('/catalog.json') and n.count('/') == 3):
            raw = git(repo, 'show', commit + ':' + name)
            data = json.loads(raw)
            rows.append({'repository': label, 'commit_at_capture': commit, 'path': str(repo / name),
                         'git_path': name, 'sha256': hashlib.sha256(raw).hexdigest(),
                         'scope_from_original_catalog': data.get('scope', data.get('status', '按原目录精确证据解释')),
                         'included_in_base_aggregate_counts': False,
                         'counting_note': '含历史/直接回归/同ID重跑；逐目录查看，不作目录计数之和，不替代最终E'})
    assert len({(x['repository'], x['git_path']) for x in rows}) == len(rows)
    report = {'schema_version': 1, 'updated_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'scope': 'Git已提交阶段目录导航；包含原基表目录与后继增量，未统一累计新增根',
              'versions_at_capture': versions, 'catalogs': rows,
              'PG18_included_in_PG17_counts': False, 'normal_decrease_accepted': False,
              'real_Linux_or_K8s_E_pass': 0, 'uncommitted_production_chain_and_quota_excluded': True,
              'source_of_truth': '每项原Git对象及其源码/命令/资源/终态/失败收据；目录存在不是通过证据'}
    OUTPUT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    base_path = ROOT / 'docs/validation/PG17-测试用例总表.json'
    base = json.loads(base_path.read_text())
    base.setdefault('base_counts_updated_utc', base['updated_utc'])
    base['base_content_scope'] = '原计数与旧状态为基表时点快照；最新已提交阶段以committed_phase_index所指Git对象为准'
    base['counts_scope'] = 'counts保留原基表统计，未把此后阶段目录/直接回归/同ID重跑相加；最新导航见committed_phase_index'
    base['committed_phase_index'] = {'path': str(OUTPUT), 'sha256': hashlib.sha256(OUTPUT.read_bytes()).hexdigest()}
    base_path.write_text(json.dumps(base, ensure_ascii=False, indent=2) + '\n')
    txt_path = ROOT / 'docs/validation/PG17-测试用例总表.txt'
    original = txt_path.read_text()
    marker = '最新已提交阶段目录导航（原基表计数不含后继增量）：'
    if marker in original:
        original = original[:original.index(marker)].rstrip() + '\n'
    txt_path.write_text(original.rstrip() + '\n\n' + marker + '\n' + str(OUTPUT) + '\n'
                        '各阶段包含直接回归和原失败，不相加；PG18独立列出，不混PG17。\n'
                        '当前完整安全normal、Linux全链及三节点Helm K8s尚未验收，E0。\n')
    print(json.dumps({'committed_catalog_count': len(rows), 'output_sha256': hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
                      'base_counts_unchanged': base['counts'], 'E': 0}, ensure_ascii=False))


if __name__ == '__main__':
    update()
