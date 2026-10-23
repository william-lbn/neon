"""真实PG复制协议与物理槽边界；只创建本机独占临时集群，不连接已有数据库。

有意同时测试physical startup、logical连接中的物理命令、SQL/SECURITY DEFINER、
实际备份与逻辑COPY流。协议/权限通过不代表逻辑槽预算或在途流式quiesce已实现。
"""
import getpass
import os
import struct
import unittest

import test_dsg_postgres as lifecycle


class ReplicationWire(lifecycle.PgWireConnection):
    """保留实际原始登录身份；遇到COPY只读取模式头，不消费/输出任何物理数据。"""
    def __init__(self, directory, user, mode="database", options=None):
        import socket
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.connection.settimeout(15)
        self.connection.connect(str(directory / ".s.PGSQL.55432"))
        values = ["user", user, "database", "postgres", "replication", mode]
        if options:
            values += ["options", options]
        body = struct.pack("!I", 196608) + ("\0".join(values) + "\0\0").encode()
        self.connection.sendall(struct.pack("!I", len(body) + 4) + body)
        self.startup = self.read_until({b"Z", b"E"})

    def read_until(self, stops):
        result = []
        while True:
            kind = self.receive_exact(1)
            size = struct.unpack("!I", self.receive_exact(4))[0]
            if not 4 <= size <= 16 * 1024 * 1024:
                raise RuntimeError("invalid bounded replication frame")
            result.append((kind, self.receive_exact(size - 4)))
            if kind in stops:
                return result

    def query(self, sql):
        body = sql.encode() + b"\0"
        self.connection.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
        return self.read_until({b"Z", b"W", b"H", b"G"})

    @staticmethod
    def code(messages):
        for kind, body in messages:
            if kind == b"E":
                for field in body.split(b"\0"):
                    if field.startswith(b"C"):
                        return field[1:].decode()
        return None


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "真实复制专项需要准确PG及配套库")
class DsgReplicationBoundary(unittest.TestCase):
    def setUp(self):
        self.pg = lifecycle.DsgPostgresLifecycle("runTest")
        self.addCleanup(self.pg.doCleanups)
        self.pg.setUp()
        self.admin = getpass.getuser()

    def start(self, enabled=True, managed=False):
        self.pg.config("wal_level=logical\nmax_wal_senders=12\nmax_replication_slots=16\n")
        if not enabled:
            self.pg.config("enable_dynamic_shared_buffers=off\n")
        if managed:
            self.pg.config("dynamic_shared_buffers_require_admission=on\ndynamic_shared_buffers_startup_budget='64MB'\n")
        self.pg.start()
        self.pg.install_functions()
        self.pg.sql("CREATE ROLE dsg_repl LOGIN REPLICATION")
        if managed:
            lib = self.pg.quote(self.pg.library)
            self.pg.sql(f"CREATE FUNCTION dsg_admission(bigint,bigint,bigint,boolean) RETURNS boolean AS '{lib}','test_dsg_admission' LANGUAGE C STRICT")

    def peer(self, user="dsg_repl", mode="database", options=None):
        peer = ReplicationWire(self.pg.socket, user, mode, options)
        self.addCleanup(peer.close)
        return peer

    def assert_denied(self, peer, command, code="42501"):
        messages = peer.query(command)
        self.assertEqual(peer.code(messages), code, command)
        self.assertFalse(any(k in (b"W", b"H", b"G", b"d", b"D") for k, _ in messages), command)
        self.assertEqual(messages[-1][0], b"Z", "permission errors must leave a reusable connection")

    def set_admission(self, revision, opened):
        value = "true" if opened else "false"
        self.pg.sql(f"SELECT dsg_admission(dsg_boot(),{revision},1,{value})")

    def test_physical_startup_and_logical_physical_commands_are_rejected(self):
        self.start()
        physical = self.peer(mode="true")
        self.assertEqual(physical.code(physical.startup), "42501")
        logical = self.peer()
        self.assertEqual(logical.startup[-1][0], b"Z")
        for command in [
            "BASE_BACKUP (CHECKPOINT 'fast')", "UPLOAD_MANIFEST", "TIMELINE_HISTORY 1",
            "CREATE_REPLICATION_SLOT forbidden_physical PHYSICAL",
            "READ_REPLICATION_SLOT private_physical", "START_REPLICATION 0/0",
        ]:
            self.assert_denied(logical, command)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_replication_slots WHERE slot_name='forbidden_physical'").stdout.strip(), "0")
        self.assertEqual(logical.query("IDENTIFY_SYSTEM")[-1][0], b"Z")
        # 原始SU也不能在startup切到普通ROLE后沿用SU豁免；此时入口已完成
        # 原始认证，必须由命令级检查重新读取PG维护的当前权限标志。
        switched = self.peer(user=self.admin, mode="true", options="-c role=dsg_repl")
        self.assertEqual(switched.startup[-1][0], b"Z")
        self.assert_denied(switched, "BASE_BACKUP (CHECKPOINT 'fast')")

    def test_physical_slot_sql_protocol_definer_and_error_cleanup(self):
        self.start()
        self.pg.sql("SELECT pg_create_physical_replication_slot('private_physical',true)")
        before = self.pg.sql("SELECT restart_lsn::text FROM pg_replication_slots WHERE slot_name='private_physical'").stdout
        logical = self.peer()
        for command in [
            "SELECT pg_create_physical_replication_slot('forbidden_sql')",
            "SELECT pg_drop_replication_slot('private_physical')",
            "SELECT pg_replication_slot_advance('private_physical',pg_current_wal_lsn())",
            "SELECT pg_copy_physical_replication_slot('private_physical','forbidden_copy')",
            "DROP_REPLICATION_SLOT private_physical", "DROP_REPLICATION_SLOT private_physical WAIT",
            "ALTER_REPLICATION_SLOT private_physical (FAILOVER true)",
        ]:
            self.assert_denied(logical, command)
        self.pg.sql("CREATE FUNCTION public.definer_physical() RETURNS void LANGUAGE sql SECURITY DEFINER AS $$ SELECT pg_drop_replication_slot('private_physical') $$")
        self.assert_denied(logical, "SELECT public.definer_physical()")
        self.assertEqual(self.pg.sql("SELECT restart_lsn::text FROM pg_replication_slots WHERE slot_name='private_physical'").stdout, before)
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_replication_slots WHERE slot_name LIKE 'forbidden%'").stdout.strip(), "0")
        self.assertEqual(self.pg.sql("SELECT active_pid IS NULL FROM pg_replication_slots WHERE slot_name='private_physical'").stdout.strip(), "t")
        # 拒绝不认领槽、不保留ControlLock/CV登记；真实管理员紧接着删除并同名重建。
        self.pg.sql("SELECT pg_drop_replication_slot('private_physical'); SELECT pg_create_physical_replication_slot('private_physical',true)")
        self.assert_denied(logical, "DROP_REPLICATION_SLOT private_physical")
        self.pg.sql("SELECT pg_drop_replication_slot('private_physical')")

    def test_logical_slots_and_actual_stream_are_preserved(self):
        self.start()
        logical = self.peer()
        created = logical.query("CREATE_REPLICATION_SLOT tenant_logical LOGICAL test_decoding (SNAPSHOT 'nothing')")
        self.assertIsNone(logical.code(created))
        self.assertTrue(any(k == b"D" for k, _ in created))
        self.pg.sql("CREATE TABLE public.dsga_repl_data(id integer PRIMARY KEY); INSERT INTO public.dsga_repl_data VALUES(73921)")
        end = self.pg.sql("SELECT pg_current_wal_flush_lsn()::text").stdout.strip()
        output = self.pg.root / "logical-output"
        # pg_recvlogical使用实际COPY_BOTH与test_decoding；固定结束LSN，不以仅建槽
        # 或收到COPY头替代数据流成功。临时文件只包含本夹具公开的整数业务数据。
        self.pg.command("pg_recvlogical", "-h", self.pg.socket, "-p", "55432", "-U", "dsg_repl", "-d", "postgres", "--start", "--slot=tenant_logical", "--file", output, "--endpos", end, "--no-loop", "--status-interval=1")
        self.assertIn("73921", output.read_text())
        self.assertIsNone(logical.code(logical.query("ALTER_REPLICATION_SLOT tenant_logical (FAILOVER true)")))
        self.assertIsNone(logical.code(logical.query("DROP_REPLICATION_SLOT tenant_logical")))
        self.assertEqual(self.pg.sql("SELECT count(*) FROM pg_replication_slots").stdout.strip(), "0")

    def test_internal_physical_stream_and_actual_basebackup_remain_available(self):
        self.start()
        physical = self.peer(user=self.admin, mode="true")
        self.assertEqual(physical.startup[-1][0], b"Z")
        lsn = self.pg.sql("SELECT pg_current_wal_flush_lsn()::text").stdout.strip()
        # 只读取COPY_BOTH模式头，随即断开；不消费/输出任何物理WAL数据。
        streaming = physical.query(f"START_REPLICATION {lsn}")
        self.assertEqual(streaming[-1][0], b"W")
        physical.close()
        backup = self.pg.root / "private-backup"
        self.pg.command("pg_basebackup", "-h", self.pg.socket, "-p", "55432", "-U", self.admin, "-D", backup, "-X", "none", "-c", "fast", "--no-sync")
        self.assertTrue((backup / "PG_VERSION").is_file())
        authfile = self.pg.sql("SELECT pg_relation_filepath('pg_catalog.pg_authid')").stdout.strip()
        self.assertTrue((backup / authfile).is_file(), "actual private backup must include global catalog")

    def test_replication_admission_closes_existing_and_new_logical_commands(self):
        self.start(managed=True)
        logical = self.peer()
        self.assert_denied(logical, "IDENTIFY_SYSTEM", "55000")
        self.assert_denied(logical, "SELECT 1", "55000")
        self.set_admission(1, True)
        self.assertIsNone(logical.code(logical.query("IDENTIFY_SYSTEM")))
        self.assertIsNone(logical.code(logical.query("CREATE_REPLICATION_SLOT admitted_logical LOGICAL test_decoding (SNAPSHOT 'nothing')")))
        self.set_admission(2, False)
        self.assert_denied(logical, "DROP_REPLICATION_SLOT admitted_logical", "55000")
        other = self.peer()
        self.assert_denied(other, "IDENTIFY_SYSTEM", "55000")
        self.set_admission(3, True)
        self.assertIsNone(logical.code(logical.query("DROP_REPLICATION_SLOT admitted_logical")))
        # 这里只证明新命令/既有连接下条命令。已在COPY中的逻辑流仍需要完整quiesce，
        # 不能用本测试把运行时复制内存、维护任务或VM缩容认证为已完成。

    def test_disabled_mode_retains_upstream_replication_semantics(self):
        self.start(enabled=False)
        physical = self.peer(mode="true")
        self.assertEqual(physical.startup[-1][0], b"Z")
        created = physical.query("CREATE_REPLICATION_SLOT legacy_physical PHYSICAL")
        self.assertIsNone(physical.code(created))
        self.assertIsNone(physical.code(physical.query("DROP_REPLICATION_SLOT legacy_physical")))
        logical = self.peer()
        self.assertIsNone(logical.code(logical.query("SELECT pg_create_physical_replication_slot('legacy_sql')")))
        self.assertIsNone(logical.code(logical.query("SELECT pg_drop_replication_slot('legacy_sql')")))
