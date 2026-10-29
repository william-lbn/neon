"""真实PG门禁回归：关闭可跨过未到达的开放，开放仍保持连续revision。

测试桥只翻译原生请求，不改共享状态；独立临时PG不依赖Linux/KVM能力。
测试不证明外层预算、已开始工作排空或物理内存释放。
"""
import json
import unittest

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要显式真实PG及配套测试库")
class DsgAdmissionClosure(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.pg.prepare_grow_exit_fixture()
        self.pg.install_functions()
        library = self.pg.quote(self.pg.library)
        self.pg.sql(
            f"CREATE FUNCTION admission(text,text,text,text,boolean,integer,boolean DEFAULT false) "
            f"RETURNS jsonb AS '{library}','test_dsg_control_admission' LANGUAGE C STRICT;"
            f"CREATE FUNCTION fence(text,text,boolean DEFAULT false) RETURNS jsonb "
            f"AS '{library}','test_dsg_fence_issuer' LANGUAGE C STRICT;"
        )
        # 测试桥返回signed bigint，生产协议使用完整uint64十进制身份。
        self.boot = str(int(self.pg.sql("SELECT dsg_boot()").stdout.strip()) & ((1 << 64) - 1))
        self.pg.sql(f"SELECT fence('{self.boot}','1')")

    def gate(self, revision, opened=False, *, epoch=1, generation=1, token=1, boot=None):
        return json.loads(self.pg.sql(
            f"SELECT admission('{self.boot if boot is None else boot}','{epoch}',"
            f"'{revision}','{generation}',{str(opened).lower()},{token})"
        ).stdout)

    def test_close_consumes_missing_open_and_retains_native_guards(self):
        # revision1开放从未发送，直接关闭2。不能先执行原开放以凑连续水位。
        self.assertEqual(self.gate(2), {
            "disposition": 0, "admission_open": False, "admission_revision": "2"})
        self.assertEqual(self.gate(1, True)["disposition"], 3)
        self.assertEqual(self.gate(2, True)["disposition"], 4)
        self.assertEqual(self.gate(2, token=2)["disposition"], 4)
        self.assertEqual(self.gate(2)["disposition"], 1)
        self.assertEqual(self.gate(4, True)["disposition"], 4)
        self.assertEqual(self.gate(5, generation=2)["disposition"], 3)
        self.assertEqual(self.gate(5, epoch=2)["disposition"], 3)
        self.assertEqual(self.gate(5, boot=str((int(self.boot) % ((1 << 64) - 1)) + 1))["disposition"], 3)
        # 原开放实际先到的确定顺序，同样由后继关闭覆盖；准入水位独立于resize。
        self.assertEqual(self.gate(3, True)["disposition"], 0)
        self.assertEqual(self.gate(4)["disposition"], 0)
        self.assertEqual(self.gate((1 << 64) - 1)["disposition"], 0)
        self.assertEqual(self.gate((1 << 64) - 1)["disposition"], 1)
        self.assertFalse(self.gate((1 << 64) - 1, True)["admission_open"])
