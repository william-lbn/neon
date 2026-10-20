"""实际加载Neon扩展的管理SQL契约测试，不以C测试桥代替产品入口。

独立临时PG，调用安装的neon--1.6--1.7 SQL和正式C函数；暂停夹具只安排真实
协调器交错。此处不构建镜像/部署，不证明资源承诺/外部控制链或物理VM完成。
"""

import copy
import json
import os
import time
import unittest
from pathlib import Path

import test_dsg_postgres as lifecycle

_ENV = (*lifecycle._REQUIRED_ENV, "NEON_TEST_NEON_LIBRARY")
if os.environ.get("NEON_TEST_NEON_LIBRARY") and not all(os.environ.get(name) for name in _ENV):
    raise RuntimeError("Neon管理接口测试环境不完整：" + ", ".join(_ENV))


@unittest.skipUnless(all(os.environ.get(name) for name in _ENV), "正式Neon接口需显式指定PG与配套库")
class DsgNeonControl(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        neon = self.pg.quote(Path(os.environ["NEON_TEST_NEON_LIBRARY"]).resolve())
        self.pg.config(
            f"shared_preload_libraries='{self.pg.quote(self.pg.library)},{neon}'\n"
            "neon.last_written_lsn_cache_size=1024\n"
            "neon.dsg_vm_uid='dsg-vm-1'\n"
            "neon.dsg_compute_instance_id='dsg-compute-1'\n"
        )
        self.pg.prepare_grow_exit_fixture()
        self.pg.sql("CREATE EXTENSION neon VERSION '1.6'; ALTER EXTENSION neon UPDATE TO '1.7'")
        self.capabilities = self.value("SELECT neon_dsg_capabilities()")

    def value(self, statement):
        return json.loads(self.pg.sql(statement).stdout)

    def request(self, *, revision=1, generation=1, target=1024, kind="grow"):
        return {
            "protocol_version": 1,
            "key": {
                "instance": self.capabilities["instance"].copy(),
                "issuer_epoch": "1",
                "revision": str(revision),
                "operation_id": {"high": "18364758544493064720", "low": str(revision)},
            },
            "kind": kind,
            "expected_capacity_generation": str(generation),
            "target_buffers": target,
            "target_vm_memory_bytes": "1073741824",
            "budget_commitment_id": "reservation-1",
            "deadline_unix_ms": "18446744073709551615",
        }

    @staticmethod
    def query(request):
        return {"protocol_version": 1, "key": copy.deepcopy(request["key"])}

    def statement(self, request, action="request_resize", *, raw=False):
        payload = request if raw else json.dumps(request)
        return f"SELECT neon_dsg_{action}('{self.pg.quote(payload)}')"

    def call(self, request, action="request_resize", *, raw=False):
        return self.value(self.statement(request, action, raw=raw))

    def state(self):
        return self.value("SELECT neon_dsg_memory_status()")["observed"]

    def arm(self, action):
        self.pg.sql(f"SELECT dsg_grow_control('{action}')")

    def paused(self, request):
        process = self.pg.background_sql(
            "SELECT dsg_grow_control('arm_pause'); " + self.statement(request), "neon-dsg-api"
        )
        self.pg.wait_sql("SELECT dsg_grow_control('prepare_waiting')", "yes")
        return process

    def finish(self, process):
        self.arm("release_prepare")
        output, errors = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, errors + self.pg.log.read_text()[-12000:])
        return json.loads(output.strip().splitlines()[-1])

    def test_capabilities_and_installed_sql_contract(self):
        self.assertEqual(self.capabilities["buffer_api_version"], 5)
        self.assertTrue(self.capabilities["supported"])
        self.assertTrue(self.capabilities["enabled"])
        self.assertTrue(self.capabilities["instance_bound"])
        self.assertFalse(self.capabilities["production_ready"])
        self.assertFalse(self.capabilities["vm_shrink_permit"])
        self.assertEqual(self.capabilities["history_capacity"], 16)
        self.assertEqual(
            self.pg.sql("SELECT extversion FROM pg_extension WHERE extname='neon'").stdout.strip(),
            "1.7",
        )
        state = self.state()
        self.assertEqual(state["instance"], self.capabilities["instance"])
        self.assertEqual(state["high_buffers"], 256)
        self.assertIsNone(state["operation"])
        for array in state["arrays"]:
            self.assertIsNone(array["release_key"])
            self.assertFalse(array["provider"]["has_result"])
            self.assertEqual(array["provider"]["status"], "unknown")

    def test_grow_shrink_regrow_and_historical_receipt(self):
        original = self.request()
        first = self.call(original)
        self.assertEqual(first["receipt"]["key"], original["key"])
        self.assertEqual(first["receipt"]["disposition"], "accepted")
        self.assertEqual(first["receipt"]["result"]["high_buffers"], 1024)
        self.assertEqual(len(first["receipt"]["result"]["operation"]["context_sha256"]), 64)
        self.call(self.request(revision=2, generation=2, target=256, kind="shrink"))
        self.call(self.request(revision=3, generation=3, target=2048))
        for action, request in (
            ("request_resize", original),
            ("resize_status", self.query(original)),
        ):
            result = self.call(request, action)
            self.assertEqual(result["receipt"]["disposition"], "replayed")
            self.assertEqual(result["receipt"]["result"]["high_buffers"], 1024)
            self.assertEqual(result["observed"]["high_buffers"], 2048)
            self.assertEqual(result["receipt"]["result"]["operation"]["key"], original["key"])
            self.assertFalse(result["vm_shrink_permit"])

    def test_full_outer_intent_is_immutable_for_same_operation(self):
        original = self.request()
        self.call(original)
        for field, value in (
            ("target_vm_memory_bytes", "2147483648"),
            ("budget_commitment_id", "reservation-2"),
            ("deadline_unix_ms", "18446744073709551614"),
            ("target_buffers", 2048),
            ("kind", "shrink"),
            ("expected_capacity_generation", "2"),
        ):
            with self.subTest(field=field):
                changed = copy.deepcopy(original)
                changed[field] = value
                result = self.call(changed)
                self.assertEqual(result["receipt"]["disposition"], "conflict")
                self.assertFalse(result["receipt"]["has_result"])
                self.assertEqual(self.state()["high_buffers"], 1024)

    def test_semantically_same_json_keeps_intent_binding(self):
        original = self.request()
        first = self.call(original)
        reordered = {key: original[key] for key in reversed(original)}
        raw = json.dumps(reordered, indent=2).replace("dsg-vm-1", "\\u0064sg-vm-1")
        second = self.call(raw, raw=True)
        self.assertEqual(second["receipt"]["disposition"], "replayed")
        self.assertEqual(
            first["receipt"]["result"]["operation"]["context_sha256"],
            second["receipt"]["result"]["operation"]["context_sha256"],
        )

    def test_duplicate_keys_at_each_level_are_rejected_before_normalization(self):
        source = json.dumps(self.request())
        for source_name, injected in (
            ('"protocol_version": 1', '"protocol_version": 1, "protocol_version": 1'),
            ('"issuer_epoch": "1"', '"issuer_epoch": "1", "issuer_epoch": "1"'),
            ('"vm_uid": "dsg-vm-1"', '"vm_uid": "dsg-vm-1", "vm_uid": "dsg-vm-1"'),
            ('"low": "1"', '"low": "1", "low": "1"'),
            ('"revision": "1"', '"revision": "1", "\\u0072evision": "1"'),
        ):
            with self.subTest(source_name=source_name):
                self.assertIn(source_name, source)
                result = self.pg.sql(
                    self.statement(source.replace(source_name, injected), raw=True), check=False
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("duplicate JSON object key", result.stderr)
        self.assertEqual(self.state()["high_buffers"], 256)

    def test_integer_lexemes_match_rust_u32_contract(self):
        source = json.dumps(self.request())
        for field, original, bad_values in (
            ("protocol_version", "1", ("1.0", "1e0", "-0", "true", '"1"')),
            ("target_buffers", "1024", ("1024.0", "1024e0", "-1024", "4294967296", "null")),
        ):
            for value in bad_values:
                with self.subTest(field=field, value=value):
                    raw = source.replace(f'"{field}": {original}', f'"{field}": {value}')
                    self.assertNotEqual(
                        self.pg.sql(self.statement(raw, raw=True), check=False).returncode, 0
                    )
        self.assertEqual(self.state()["high_buffers"], 256)

    def test_shared_rust_json_vector_has_lossless_control_identity(self):
        path = Path(__file__).resolve().parents[1] / "libs/compute_api/testdata/dsg_request_v1.json"
        request = json.loads(path.read_text())
        # 固定向量保留uint64极值/高位；只替换此次真实boot以便执行，而不是伪造内核身份。
        request["key"]["instance"]["pg_boot_id"] = self.capabilities["instance"]["pg_boot_id"]
        result = self.call(request)
        self.assertEqual(result["receipt"]["disposition"], "accepted")
        self.assertEqual(result["receipt"]["result"]["operation"]["key"], request["key"])
        self.assertEqual(self.call(request)["receipt"]["disposition"], "replayed")

    def test_stale_outer_instance_and_boot_cannot_execute_or_cancel(self):
        original = self.request()
        self.call(original)
        for field in ("vm_uid", "compute_instance_id", "pg_boot_id"):
            stale = copy.deepcopy(original)
            stale["key"]["instance"][field] = "1" if field == "pg_boot_id" else "another-instance"
            for action, request in (
                ("request_resize", stale),
                ("resize_status", self.query(stale)),
                ("cancel_resize", self.query(stale)),
            ):
                with self.subTest(field=field, action=action):
                    result = self.call(request, action)
                    self.assertEqual(result["receipt"]["key"], stale["key"])
                    self.assertEqual(result["receipt"]["disposition"], "stale")
                    self.assertFalse(result["receipt"]["has_result"])
                    self.assertEqual(result["observed"]["instance"], self.capabilities["instance"])
                    self.assertEqual(result["observed"]["operation"]["key"], original["key"])

    def test_unknown_missing_and_mistyped_nested_fields_are_rejected(self):
        for path in ((), ("key",), ("key", "instance"), ("key", "operation_id")):
            for change in ("unknown", "missing", "replace"):
                request = self.request()
                node = request
                for key in path:
                    node = node[key]
                if change == "unknown":
                    node["unexpected"] = "1"
                else:
                    key = next(iter(node))
                    del node[key]
                    if change == "replace":
                        node["unexpected"] = "1"
                with self.subTest(path=path, change=change):
                    self.assertNotEqual(
                        self.pg.sql(self.statement(request), check=False).returncode, 0
                    )
        for bad in (None, [], "request", 1, {"protocol_version": 1, "key": []}):
            self.assertNotEqual(self.pg.sql(self.statement(bad), check=False).returncode, 0)
        self.assertEqual(self.state()["high_buffers"], 256)

    def test_uint64_fields_never_accept_number_coercion_or_ambiguous_text(self):
        for value in (1, "01", "+1", "-1", " 1", "1e0", "１", "18446744073709551616", None):
            request = self.request()
            request["key"]["revision"] = value
            with self.subTest(value=value):
                self.assertNotEqual(self.pg.sql(self.statement(request), check=False).returncode, 0)
        for field in ("expected_capacity_generation", "target_vm_memory_bytes", "deadline_unix_ms"):
            request = self.request()
            request[field] = "0"
            self.assertNotEqual(self.pg.sql(self.statement(request), check=False).returncode, 0)
        self.assertEqual(self.state()["high_buffers"], 256)

    def test_expired_new_intent_is_rejected_but_existing_result_can_replay(self):
        request = self.request()
        request["deadline_unix_ms"] = "1"
        result = self.call(request)
        self.assertEqual(result["receipt"]["disposition"], "rejected")
        self.assertFalse(result["receipt"]["has_result"])
        request["deadline_unix_ms"] = str(int(time.time() * 1000) + 1500)
        self.assertEqual(self.call(request)["receipt"]["disposition"], "accepted")
        while time.time() * 1000 <= int(request["deadline_unix_ms"]):
            time.sleep(0.02)
        self.assertEqual(self.call(request)["receipt"]["disposition"], "replayed")
        changed = copy.deepcopy(request)
        changed["deadline_unix_ms"] = "18446744073709551615"
        self.assertEqual(self.call(changed)["receipt"]["disposition"], "conflict")

    def test_business_cancel_reaches_real_coordinator(self):
        request = self.request()
        running = self.paused(request)
        status = self.call(self.query(request), "resize_status")
        self.assertEqual(status["receipt"]["result"]["operation"]["phase"], "preparing")
        cancelled = self.call(self.query(request), "cancel_resize")
        self.assertEqual(cancelled["receipt"]["disposition"], "cancel_requested")
        result = self.finish(running)
        self.assertEqual(result["receipt"]["result"]["operation"]["reason"], "cancelled")
        self.assertEqual(result["observed"]["high_buffers"], 256)
        self.assertEqual(self.call(request)["receipt"]["disposition"], "replayed")

    def test_busy_receipt_has_own_key_and_running_query_remains_available(self):
        request = self.request()
        running = self.paused(request)
        competitor = self.request(revision=2)
        # 给真实psql启动留足余量，测试串行锁竞争，不把100ms进程启动性能当门禁。
        competitor["deadline_unix_ms"] = str(int(time.time() * 1000) + 2000)
        result = self.call(competitor)
        self.assertEqual(result["receipt"]["disposition"], "busy")
        self.assertEqual(result["receipt"]["key"], competitor["key"])
        self.assertIsNone(result["receipt"]["result"])
        self.assertEqual(result["observed"]["operation"]["key"], request["key"])
        current = self.call(self.query(request), "resize_status")
        self.assertEqual(current["receipt"]["result"]["operation"]["phase"], "preparing")
        self.assertEqual(
            self.finish(running)["receipt"]["result"]["operation"]["phase"], "committed"
        )

    def test_pending_provider_is_unknown_and_cancel_after_publish_is_too_late(self):
        self.call(self.request())
        self.arm("attach_release")
        request = self.request(revision=2, generation=2, target=256, kind="shrink")
        running = self.paused(request)
        result = self.call(self.query(request), "cancel_resize")
        self.assertEqual(result["receipt"]["disposition"], "too_late")
        self.assertEqual(result["observed"]["high_buffers"], 256)
        for array in result["observed"]["arrays"]:
            self.assertEqual(array["release_key"], request["key"])
            self.assertEqual(array["release_state"], "pending")
            self.assertFalse(array["provider"]["has_result"])
            self.assertEqual(array["provider"]["status"], "unknown")
            self.assertFalse(array["provider"]["physical_release_verified"])
        self.assertFalse(self.finish(running)["vm_shrink_permit"])

    def test_sql_rollback_and_extension_downgrade_do_not_undo_shared_facts(self):
        request = self.request()
        self.pg.sql("BEGIN; " + self.statement(request) + "; ROLLBACK")
        self.assertEqual(self.state()["high_buffers"], 1024)
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.6'")
        self.assertEqual(
            self.pg.sql("SELECT to_regprocedure('neon_dsg_capabilities()') IS NULL").stdout.strip(),
            "t",
        )
        self.pg.sql("ALTER EXTENSION neon UPDATE TO '1.7'")
        result = self.call(self.query(request), "resize_status")
        self.assertEqual(result["receipt"]["disposition"], "replayed")
        self.assertEqual(result["observed"]["high_buffers"], 1024)

    def test_error_after_publish_keeps_queryable_commit_and_pending_provider_facts(self):
        self.call(self.request())
        request = self.request(revision=2, generation=2, target=256, kind="shrink")
        self.arm("attach_shrink_publish")
        failed = self.pg.sql(
            "SELECT dsg_grow_control('arm_error'); " + self.statement(request), check=False
        )
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("DSGA test resize ERROR", failed.stderr)
        result = self.call(self.query(request), "resize_status")
        self.assertEqual(result["receipt"]["result"]["operation"]["phase"], "committed")
        self.assertEqual(result["observed"]["high_buffers"], 256)
        for array in result["observed"]["arrays"]:
            self.assertEqual(array["release_state"], "pending")
            self.assertFalse(array["provider"]["has_result"])
        self.assertEqual(self.call(request)["receipt"]["disposition"], "replayed")
        self.assertFalse(result["vm_shrink_permit"])

    def test_fatal_query_and_late_exit_callback_preserve_distinct_operation_results(self):
        request = self.request()
        old = self.pg.background_sql(
            "SELECT dsg_grow_control('arm_fatal'); " + self.statement(request), "neon-dsg-fatal"
        )
        self.pg.wait_sql("SELECT dsg_grow_control('exit_waiting')", "yes")
        result = self.call(self.query(request), "resize_status")
        self.assertEqual(result["receipt"]["result"]["operation"]["reason"], "interrupted")
        new = self.request(revision=2, target=2048)
        self.assertEqual(self.call(new)["receipt"]["disposition"], "accepted")
        self.arm("release_exit")
        _, errors = old.communicate(timeout=10)
        self.assertNotEqual(old.returncode, 0)
        self.assertIn("DSGA test Grow FATAL", errors)
        result = self.call(self.query(request), "resize_status")
        self.assertEqual(result["receipt"]["result"]["high_buffers"], 256)
        self.assertEqual(result["observed"]["operation"]["key"], new["key"])
        self.assertEqual(result["observed"]["high_buffers"], 2048)

    def test_public_acl_and_session_identity_both_enforce_management_boundary(self):
        signatures = (
            "neon_dsg_capabilities()",
            "neon_dsg_memory_status()",
            "neon_dsg_request_resize(text)",
            "neon_dsg_resize_status(text)",
            "neon_dsg_cancel_resize(text)",
        )
        self.pg.sql("CREATE ROLE dsg_api_user LOGIN")
        for signature in signatures:
            self.assertEqual(
                self.pg.sql(
                    f"SELECT has_function_privilege('dsg_api_user','{signature}','EXECUTE')"
                ).stdout.strip(),
                "f",
            )
        # 故意误授ACL和建立definer，仍必须由C入口拒绝普通会话。
        self.pg.sql(
            ";".join(
                f"GRANT EXECUTE ON FUNCTION {signature} TO dsg_api_user" for signature in signatures
            )
            + "; CREATE FUNCTION dsg_api_definer() RETURNS jsonb LANGUAGE SQL SECURITY DEFINER "
            "AS $$SELECT neon_dsg_capabilities()$$"
        )
        request = self.request()
        for statement in (
            "SELECT neon_dsg_capabilities()",
            "SELECT neon_dsg_memory_status()",
            self.statement(request),
            self.statement(self.query(request), "resize_status"),
            self.statement(self.query(request), "cancel_resize"),
            "SELECT dsg_api_definer()",
        ):
            result = self.pg.sql(
                "SET SESSION AUTHORIZATION dsg_api_user; " + statement, check=False
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("superuser session", result.stderr)

    def test_instance_gucs_are_immutable_within_postmaster(self):
        for name in ("neon.dsg_vm_uid", "neon.dsg_compute_instance_id"):
            result = self.pg.sql(f"SET {name}='another-instance'", check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("cannot be changed without restarting the server", result.stderr)

    def restart(self, config):
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
        self.pg.running = False
        self.pg.config(config)
        self.pg.start()

    def test_missing_binding_refuses_management_operations(self):
        self.restart("neon.dsg_vm_uid=''\nneon.dsg_compute_instance_id=''\n")
        self.assertFalse(self.value("SELECT neon_dsg_capabilities()")["instance_bound"])
        result = self.pg.sql(self.statement(self.request()), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("identity is not configured", result.stderr)

    def test_disabled_dynamic_mode_has_no_writable_management_fallback(self):
        self.restart("enable_dynamic_shared_buffers=off\n")
        self.assertFalse(self.value("SELECT neon_dsg_capabilities()")["enabled"])
        for statement in ("SELECT neon_dsg_memory_status()", self.statement(self.request())):
            result = self.pg.sql(statement, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("dynamic shared buffers are disabled", result.stderr)

    def test_bound_identity_is_json_escaped_without_changing_bytes(self):
        identity = 'compute-"quoted'
        self.restart(f"neon.dsg_compute_instance_id='{identity}'\n")
        self.capabilities = self.value("SELECT neon_dsg_capabilities()")
        self.assertEqual(self.capabilities["instance"]["compute_instance_id"], identity)
        result = self.call(self.request())
        self.assertEqual(result["receipt"]["key"]["instance"]["compute_instance_id"], identity)

    def test_command_line_binding_overrides_restored_auto_conf_and_last_include(self):
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
        self.pg.running = False
        (self.pg.data / "postgresql.auto.conf").write_text(
            "neon.dsg_vm_uid='invalid restored vm'\nneon.dsg_compute_instance_id='invalid restored compute'\n"
        )
        (self.pg.data / "compute_ctl_temp_override.conf").write_text(
            "neon.dsg_vm_uid='invalid override vm'\nneon.dsg_compute_instance_id='invalid override compute'\n"
        )
        self.pg.config("include_if_exists='compute_ctl_temp_override.conf'\n")
        self.pg.start(
            options="-c neon.dsg_vm_uid=dsg-vm-1 -c neon.dsg_compute_instance_id=dsg-compute-1"
        )
        self.capabilities = self.value("SELECT neon_dsg_capabilities()")
        self.assertEqual(self.capabilities["instance"]["vm_uid"], "dsg-vm-1")
        self.assertEqual(self.capabilities["instance"]["compute_instance_id"], "dsg-compute-1")
        self.assertEqual(self.call(self.request())["receipt"]["disposition"], "accepted")

    def test_invalid_startup_identity_is_rejected(self):
        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
        self.pg.running = False
        config = self.pg.data / "postgresql.conf"
        original = config.read_text()
        for identity in ("bad identity", "x" * 129, "实例"):
            with self.subTest(identity=identity):
                config.write_text(original + f"\nneon.dsg_vm_uid='{self.pg.quote(identity)}'\n")
                offset = len(self.pg.log.read_text())
                result = self.pg.start(check=False)
                try:
                    self.assertNotEqual(result.returncode, 0)
                    log = self.pg.log.read_text()[offset:]
                    self.assertIn("invalid DSGA managed startup identity", log)
                    self.assertIn("visible ASCII bytes", log)
                finally:
                    # 即使负向候选错误地启动成功，下一用例也须独立启动；不能把
                    # already-running误判成另一个非法身份被正确拒绝。
                    if self.pg.running:
                        self.pg.command("pg_ctl", "-D", self.pg.data, "-m", "fast", "-w", "stop")
                        self.pg.running = False
        config.write_text(original)

    def test_oversized_or_invalid_json_never_changes_capacity(self):
        for raw in ("{" + " " * 16384 + "}", "{", '{"protocol_version": 1,}'):
            result = self.pg.sql(self.statement(raw, raw=True), check=False)
            self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.state()["high_buffers"], 256)
