#!/usr/bin/env python3
"""执行有编号的PG17内存单服务用例；零用例/跳过/缺能力不能形成绿色收据。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import unittest

HERE = Path(__file__).resolve().parent

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, default=HERE / 'pg17_memory_cases.json')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--suite', default='core-smoke')
    args = parser.parse_args()
    if platform.system() != 'Linux' or os.geteuid() == 0:
        parser.error('Linux独占临时PG需以非root执行')
    catalog = json.loads(args.catalog.read_text())
    if catalog.get('protocol_version') != 1 or catalog.get('pg_major') != 17:
        parser.error('只接受准确PG17用例目录版本1')
    cases = [row for row in catalog['cases'] if row['suite'] == args.suite]
    names = [row['test'] for row in cases]
    ids = [row['id'] for row in cases]
    if not cases or len(names) != len(set(names)) or len(ids) != len(set(ids)) or any(row['layer'] != 'single-service' for row in cases):
        parser.error('单服务目录必须非空且测试名称/编号唯一')
    required = ['NEON_TEST_PG_BIN', 'NEON_TEST_DSG_LIBRARY', 'NEON_TEST_RMGR_LIBRARY', 'NEON_TEST_NEON_LIBRARY']
    if any(not os.environ.get(name) for name in required):
        parser.error('须显式指定准确PG及全部配套库，不能通过跳过缺失能力')
    pg_bin = Path(os.environ['NEON_TEST_PG_BIN'])
    configure = subprocess.check_output([str(pg_bin / 'pg_config'), '--configure'], text=True).strip()
    # 部分正式管理测试依赖原生协调器注入交错，缺注入点时直接报告能力不足。
    # 不临时删用例，也不把不支持平台降为skip；普通发行镜像不因此开启注入点。
    if '--enable-injection-points' not in configure:
        parser.error('此单服务套件需专用--enable-injection-points测试内核')
    binaries = {'postgres': pg_bin / 'postgres', **{name: Path(os.environ[name]) for name in required[1:]}}
    rows = []
    lookup = {row['test']: row['id'] for row in cases}

    class Result(unittest.TextTestResult):
        # 每个根只保存一条结果；任何subTest失败都回写其根，保留完整traceback。
        # 这样既可精确统计根用例，也不会将subTest失败藏在根的空结果里。
        def startTest(self, test):
            self.started = time.monotonic()
            self.current = {'id': lookup.get(test.id(), 'UNMAPPED'), 'test': test.id(), 'result': 'PASS', 'details': [], 'subtests': 0}
            super().startTest(test)
        def stopTest(self, test):
            self.current['elapsed_seconds'] = round(time.monotonic() - self.started, 6)
            rows.append(self.current)
            super().stopTest(test)
        def record_error(self, test, err, state):
            self.current['result'] = state
            self.current['details'].append(self._exc_info_to_string(err, test))
        def addError(self, test, err):
            super().addError(test, err)
            self.record_error(test, err, 'ERROR')
        def addFailure(self, test, err):
            super().addFailure(test, err)
            self.record_error(test, err, 'FAIL')
        def addSkip(self, test, reason):
            super().addSkip(test, reason)
            self.current['result'] = 'SKIP'
            self.current['details'].append(reason)
        def addSubTest(self, test, subtest, err):
            super().addSubTest(test, subtest, err)
            self.current['subtests'] += 1
            if err:
                self.record_error(subtest, err, 'FAIL')

    sys.path.insert(0, str(HERE))
    suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromName(name) for name in names)
    result = unittest.TextTestRunner(verbosity=2, resultclass=Result).run(suite)
    passed = result.testsRun == len(cases) and len(rows) == len(cases) and result.wasSuccessful() and all(row['result'] == 'PASS' and row['id'] != 'UNMAPPED' for row in rows)
    evidence = {'protocol_version': 1, 'layer': 'single-service', 'suite': args.suite, 'platform': platform.platform(),
                'python': platform.python_version(), 'pg_configure': configure,
                'postgres_version': subprocess.check_output([str(pg_bin / 'postgres'), '--version'], text=True).strip(),
                'binary_sha256': {name: {'path': str(path), 'sha256': digest(path)} for name, path in binaries.items()},
                'test_source_sha256': {str(path.relative_to(HERE)): digest(path) for path in sorted(HERE.glob('test_dsg*.py'))},
                'catalog_sha256': digest(args.catalog), 'runner_sha256': digest(__file__),
                'not_proven': ['KVM', 'guest cgroup/HugeTLB物理归还', 'owner到VM端到端', 'Kubernetes'],
                'tests': rows, 'tests_run': result.testsRun, 'expected_tests': len(cases),
                'passed': passed, 'skipped': len(result.skipped), 'errors': len(result.errors), 'failures': len(result.failures)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + '\n')
    return 0 if passed else 1

if __name__ == '__main__':
    raise SystemExit(main())
