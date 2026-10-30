"""真实外部DSM分配前额度/原生pin生命周期及失败预算，不是物理缩容证明。"""
import concurrent.futures
import json
import struct
import threading
import time
import unittest

import test_dsg_connection_gate as connections
import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要真实PG与配套DSGA库")
class DsgDsmLifecycle(unittest.TestCase):
    main_pool_mb = 0
    budget_mb = 4

    def setUp(self):
        self.control = connections.DsgConnectionGateLifecycle("runTest")
        self.control.extra_postgres_config = (
            f"dynamic_shared_buffers_dsm_budget='{self.budget_mb}MB'\n"
            f"min_dynamic_shared_memory='{self.main_pool_mb}MB'\n"
            "track_counts=off\nautovacuum=off\n")
        self.addCleanup(self.control.doCleanups)
        self.control.setUp()
        self.pg = self.control.pg
        lib = self.pg.quote(self.pg.library)
        declarations = {
            "dsg_dsm()": ("text", "snapshot"),
            "dsg_dsm_allocate(bigint,boolean,boolean)": ("bigint", "allocate"),
            "dsg_dsm_attach(bigint,boolean)": ("boolean", "attach"),
            "dsg_dsm_detach(bigint)": ("boolean", "detach"),
            "dsg_dsm_unpin(bigint)": ("boolean", "unpin"),
            "dsg_dsm_probe(bigint)": ("boolean", "probe"),
            "dsg_dsm_control(text)": ("int", "control"),
        }
        self.pg.sql("; ".join(
            f"CREATE FUNCTION {signature} RETURNS {typ} AS '{lib}', 'test_dsg_dsm_{entry}' "
            "LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE"
            for signature, (typ, entry) in declarations.items()))
        self.baseline = self.fact()
        self.assertTrue(self.baseline["accounting_valid"])
        self.assertEqual(self.baseline["limit_enforced"], bool(self.budget_mb))
        self.assertEqual(self.baseline["external_budget_bytes"], self.budget_mb * 1024**2)
        self.assertGreater(self.baseline["control_upper_bytes"], 0)

    def fact(self):
        messages = self.control.observer.exchange([(b"Q", b"SELECT dsg_dsm()\0")])
        result = json.loads(self.control.row(messages))
        for key in ("reserved_upper_bytes", "control_upper_bytes", "external_budget_bytes"):
            result[key] = int(result[key])
        self.assertLessEqual(result["pending_creations"] + result["unknown_segments"],
                             result["external_segments"])
        return result

    def wait(self, **expected):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            fact = self.fact()
            if all(fact[k] == v for k, v in expected.items()):
                return fact
            time.sleep(0.01)
        self.fail(f"actual DSM state did not reach {expected}: {fact}")

    def restored(self):
        return self.wait(**{k: self.baseline[k] for k in
                           ("reserved_upper_bytes", "external_segments", "pending_creations", "unknown_segments")})

    def peer(self):
        return self.control.peer(self.control.private, lifecycle.native_database_user())

    @staticmethod
    def query(peer, query):
        return peer.exchange([(b"Q", query.encode() + b"\0")])

    def success(self, messages):
        self.assertFalse(any(k == b"E" for k, _ in messages), messages)
        self.assertEqual(messages[-1], (b"Z", b"I"))

    def scalar(self, peer, query):
        messages = self.query(peer, query)
        self.success(messages)
        return self.control.row(messages)

    def error(self, messages, code):
        errors = [b for k, b in messages if k == b"E"]
        self.assertEqual(len(errors), 1, messages)
        self.assertIn(b"C" + code.encode() + b"\0", errors[0])

    def allocate(self, peer, size, mapping=True, segment=False):
        return int(self.scalar(peer, f"SELECT dsg_dsm_allocate({size},{str(mapping).lower()},{str(segment).lower()})"))

    def arm(self, peer, mode):
        self.pg.sql("SELECT dsg_dsm_control('attach')")
        self.scalar(peer, f"SELECT dsg_dsm_control('{mode}')")

    def test_pre_os_reservation_is_visible_and_actual_cancel_has_no_allocation(self):
        self.control.gate(True)
        peer = self.peer()
        self.arm(peer, "before_wait")
        body = b"SELECT dsg_dsm_allocate(2097152,true,false)\0"
        peer.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
        self.pg.wait_sql("SELECT dsg_dsm_control('state')", "1")
        self.control.gate(False)
        held = self.wait(pending_creations=self.baseline["pending_creations"] + 1)
        self.assertEqual(held["reserved_upper_bytes"], self.baseline["reserved_upper_bytes"] + 2097152)
        self.assertFalse(held["foreground_idle"])
        # 仅取消本次 BackendKeyData 的实际后端；回调在 OS 调用前检查中断。
        self.pg.sql(f"SELECT pg_cancel_backend({peer.backend_pid})")
        self.error(peer.receive_ready(), "57014")
        self.restored()
        self.pg.sql("SELECT dsg_dsm_control('detach')")
        handle = self.allocate(peer, 2097152)
        self.assertEqual(handle & 1, 0)
        self.assertEqual(self.scalar(peer, f"SELECT dsg_dsm_detach({handle})"), "t")
        self.restored()

    def test_error_after_real_create_destroys_unpublished_owned_mapping(self):
        peer = self.peer()
        self.arm(peer, "after_error")
        self.error(self.query(peer, "SELECT dsg_dsm_allocate(2097152,true,false)"), "XX000")
        self.restored()
        self.pg.sql("SELECT dsg_dsm_control('detach')")
        # 完整原额度可再次使用，不以清零夹具掩盖残留。
        handle = self.allocate(peer, 3145728)
        self.assertEqual(self.pg.sql(f"SELECT dsg_dsm_probe({handle})").stdout.strip(), "t")
        self.scalar(peer, f"SELECT dsg_dsm_detach({handle})")
        self.restored()
        self.assertEqual(self.pg.sql(f"SELECT dsg_dsm_probe({handle})").stdout.strip(), "f")

    def test_pinned_segment_survives_creator_exit_and_counts_backing_once(self):
        creator = self.peer()
        handle = self.allocate(creator, 2097152, mapping=False, segment=True)
        creator.close()
        self.wait(foreground_idle=True, reserved_upper_bytes=self.baseline["reserved_upper_bytes"] + 2097152)
        self.assertEqual(self.pg.sql(f"SELECT dsg_dsm_probe({handle})").stdout.strip(), "t")
        a, b = self.peer(), self.peer()
        for peer in (a, b):
            self.assertEqual(self.scalar(peer, f"SELECT dsg_dsm_attach({handle},true)"), "t")
        held = self.fact()
        self.assertEqual(held["reserved_upper_bytes"], self.baseline["reserved_upper_bytes"] + 2097152)
        self.pg.sql(f"SELECT dsg_dsm_unpin({handle})")
        self.scalar(a, f"SELECT dsg_dsm_detach({handle})")
        self.assertEqual(self.fact()["reserved_upper_bytes"], held["reserved_upper_bytes"])
        self.scalar(b, f"SELECT dsg_dsm_detach({handle})")
        self.restored()
        self.assertEqual(self.pg.sql(f"SELECT dsg_dsm_probe({handle})").stdout.strip(), "f")

    def test_concurrent_create_cannot_spend_same_last_budget_and_reuses_after_real_destroy(self):
        peers = [self.peer(), self.peer()]
        start = threading.Barrier(2)

        def allocate(peer):
            start.wait(timeout=4)
            return self.query(peer, "SELECT dsg_dsm_allocate(3145728,true,false)")

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            messages = list(pool.map(allocate, peers))
        winners = [i for i, m in enumerate(messages) if not any(k == b"E" for k, _ in m)]
        self.assertEqual(len(winners), 1, messages)
        winner = winners[0]
        self.error(messages[1 - winner], "53000")
        self.success(messages[winner])
        handle = int(self.control.row(messages[winner]))
        fact = self.fact()
        self.assertLessEqual(fact["reserved_upper_bytes"], fact["external_budget_bytes"])
        self.scalar(peers[winner], f"SELECT dsg_dsm_detach({handle})")
        self.restored()
        second = self.allocate(peers[1 - winner], 3145728)
        self.scalar(peers[1 - winner], f"SELECT dsg_dsm_detach({second})")
        self.restored()

    def test_failed_destroy_keeps_real_os_segment_and_unknown_budget(self):
        peer = self.peer()
        handle = self.allocate(peer, 2097152)
        self.arm(peer, "destroy_error")
        self.error(self.query(peer, f"SELECT dsg_dsm_detach({handle})"), "XX000")
        held = self.wait(unknown_segments=self.baseline["unknown_segments"] + 1)
        self.assertEqual(held["reserved_upper_bytes"], self.baseline["reserved_upper_bytes"] + 2097152)
        self.assertEqual(self.pg.sql(f"SELECT dsg_dsm_probe({handle})").stdout.strip(), "t")
        # 原生 moribund 不再可 attach，但 OS 对象仍存；不得清预算伪造回收。
        # 注入 ERROR 会保留原 backend 的待清理本地 descriptor；用另一真实
        # backend 检查 native moribund，不让“重复 attach 本地映射”保护误导断言。
        self.assertEqual(self.scalar(self.peer(), f"SELECT dsg_dsm_attach({handle},false)"), "f")
        self.error(self.query(peer, "SELECT dsg_dsm_allocate(3145728,true,false)"), "53000")
        self.assertEqual(self.fact()["reserved_upper_bytes"], held["reserved_upper_bytes"])
        self.pg.sql("SELECT dsg_dsm_control('detach')")

    def test_postmaster_guc_cannot_be_reduced_at_runtime(self):
        self.error(self.query(self.peer(), "SET dynamic_shared_buffers_dsm_budget='1MB'"), "55P02")
        self.assertEqual(self.fact()["external_budget_bytes"], 4194304)


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要真实PG与配套DSGA库")
class DsgDsmMainRegionLifecycle(unittest.TestCase):
    def test_main_region_payload_is_already_charged_in_fixed_main(self):
        # 组合复用夹具，避免继承另一TestCase而重复执行其全部测试。
        fixture = DsgDsmLifecycle("runTest")
        fixture.main_pool_mb = 2
        self.addCleanup(fixture.doCleanups)
        fixture.setUp()
        peer = fixture.peer()
        handle = fixture.allocate(peer, 65536)
        self.assertEqual(handle & 1, 1, "must actually allocate in preallocated main region")
        fact = fixture.fact()
        for key in ("reserved_upper_bytes", "external_segments", "control_upper_bytes"):
            self.assertEqual(fact[key], fixture.baseline[key])
        fixture.scalar(peer, f"SELECT dsg_dsm_detach({handle})")
        fixture.restored()


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要真实PG与配套DSGA库")
class DsgDsmUnboundedLifecycle(unittest.TestCase):
    def test_default_unenforced_budget_reports_fact_without_claiming_limit(self):
        fixture = DsgDsmLifecycle("runTest")
        fixture.budget_mb = 0
        self.addCleanup(fixture.doCleanups)
        fixture.setUp()
        peer = fixture.peer()
        handle = fixture.allocate(peer, 6291456)
        fact = fixture.fact()
        self.assertFalse(fact["limit_enforced"])
        self.assertEqual(fact["external_budget_bytes"], 0)
        self.assertEqual(fact["reserved_upper_bytes"], fixture.baseline["reserved_upper_bytes"] + 6291456)
        fixture.scalar(peer, f"SELECT dsg_dsm_detach({handle})")
        fixture.restored()
