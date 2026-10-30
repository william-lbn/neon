# PG17原生出生数组的CI桥；真实测试仍随准确PG子模块维护。
import importlib.util
import os
from pathlib import Path

required=('NEON_TEST_PG_BIN','NEON_TEST_DSG_LIBRARY')
configured=[key for key in required if os.environ.get(key)]
if configured and len(configured)!=len(required):
    raise RuntimeError('PG17 early-birth test environment is incomplete')
if configured:
    source=Path(__file__).resolve().parent.parent/'vendor/postgres-v17/src/test/modules/test_dsg_region/t/test_early_arrays.py'
    if not source.is_file():
        raise RuntimeError('exact PG17 early-birth test source is absent')
    spec=importlib.util.spec_from_file_location('_pg17_native_early_arrays',source)
    native=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    EarlyArrays=native.EarlyArrays
    # 稳定catalog名称；不复制测试正文或把另一个PG版本装进当前模块。
    EarlyArrays.__module__=__name__
# 通用contracts无PG工具链时不造dummy绿色测试。真实shell入口严格要求6根，
# 编号runner先要求完整工具链，缺源码/环境/NNP前置均失败，不会skip取得资格。
