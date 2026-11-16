import os
import random
import shlex
import struct
from contextlib import closing
from enum import StrEnum
from threading import Thread
from time import sleep
from typing import Any

import psutil
import psycopg2
import pytest
from fixtures.endpoint.http import EndpointHttpClient
from fixtures.log_helper import log
from fixtures.neon_fixtures import NeonEnv, NeonEnvBuilder
from fixtures.pg_version import PgVersion
from fixtures.utils import USE_LFC, wait_until
from prometheus_client.parser import text_string_to_metric_families as prom_parse_impl
from psycopg2.extensions import cursor as Cursor


class PrewarmMethod(StrEnum):
    POSTGRES = "postgres"
    COMPUTE_CTL = "compute-ctl"
    AUTOPREWARM = "autoprewarm"


PREWARM_LABEL = "compute_ctl_lfc_prewarms_total"
PREWARM_ERR_LABEL = "compute_ctl_lfc_prewarm_errors_total"
OFFLOAD_LABEL = "compute_ctl_lfc_offloads_total"
OFFLOAD_ERR_LABEL = "compute_ctl_lfc_offload_errors_total"
METHOD_VALUES = [e for e in PrewarmMethod]
METHOD_IDS = [e.value for e in PrewarmMethod]
AUTOOFFLOAD_INTERVAL_SECS = 2


def prom_parse(client: EndpointHttpClient) -> dict[str, float]:
    labels = PREWARM_LABEL, OFFLOAD_LABEL, PREWARM_ERR_LABEL, OFFLOAD_ERR_LABEL
    return {
        sample.name: int(sample.value)
        for family in prom_parse_impl(client.metrics())
        for sample in family.samples
        if sample.name in labels
    }


def offload_lfc(method: PrewarmMethod, client: EndpointHttpClient, cur: Cursor) -> Any:
    if method == PrewarmMethod.POSTGRES:
        cur.execute("select neon.get_local_cache_state()")
        return cur.fetchall()[0][0]

    if method == PrewarmMethod.AUTOPREWARM:
        # With autoprewarm, we need to be sure LFC was offloaded after all writes
        # finish, so we sleep. Otherwise we'll have less prewarmed pages than we want
        sleep(AUTOOFFLOAD_INTERVAL_SECS)
        offload_res = client.offload_lfc_wait()
        log.info(offload_res)
        return offload_res

    if method == PrewarmMethod.COMPUTE_CTL:
        status = client.prewarm_lfc_status()
        assert status["status"] == "not_prewarmed"
        assert "error" not in status
        offload_res = client.offload_lfc()
        log.info(offload_res)
        assert client.prewarm_lfc_status()["status"] == "not_prewarmed"

        parsed = prom_parse(client)
        desired = {OFFLOAD_LABEL: 1, PREWARM_LABEL: 0, OFFLOAD_ERR_LABEL: 0, PREWARM_ERR_LABEL: 0}
        assert parsed == desired, f"{parsed=} != {desired=}"

        return offload_res

    raise AssertionError(f"{method} not in PrewarmMethod")


def prewarm_endpoint(
    method: PrewarmMethod, client: EndpointHttpClient, cur: Cursor, lfc_state: str | None
):
    if method == PrewarmMethod.AUTOPREWARM:
        prewarm_res = client.prewarm_lfc_wait()
        log.info(prewarm_res)
    elif method == PrewarmMethod.COMPUTE_CTL:
        prewarm_res = client.prewarm_lfc()
        log.info(prewarm_res)
        return prewarm_res
    elif method == PrewarmMethod.POSTGRES:
        cur.execute("select neon.prewarm_local_cache(%s)", (lfc_state,))


def check_prewarmed_contains(
    method: PrewarmMethod, client: EndpointHttpClient, desired_status: dict[str, str | int]
):
    if method == PrewarmMethod.AUTOPREWARM:
        prewarm_status = client.prewarm_lfc_status()
        for k in desired_status:
            assert desired_status[k] == prewarm_status[k]

        assert prom_parse(client)[PREWARM_LABEL] == 1
    elif method == PrewarmMethod.COMPUTE_CTL:
        prewarm_status = client.prewarm_lfc_status()
        for k in desired_status:
            assert desired_status[k] == prewarm_status[k]

        desired = {OFFLOAD_LABEL: 0, PREWARM_LABEL: 1, PREWARM_ERR_LABEL: 0, OFFLOAD_ERR_LABEL: 0}
        assert prom_parse(client) == desired


@pytest.mark.skipif(not USE_LFC, reason="LFC is disabled, skipping")
@pytest.mark.parametrize("method", METHOD_VALUES, ids=METHOD_IDS)
def test_lfc_prewarm(neon_simple_env: NeonEnv, method: PrewarmMethod):
    """
    Test we can offload endpoint's LFC cache to endpoint storage.
    Test we can prewarm endpoint with LFC cache loaded from endpoint storage.
    """
    env = neon_simple_env
    n_records = 1000000
    cfg = [
        "autovacuum = off",
        "shared_buffers=1MB",
        "neon.max_file_cache_size=1GB",
        "neon.file_cache_size_limit=1GB",
        "neon.file_cache_prewarm_limit=1000",
    ]

    if method == PrewarmMethod.AUTOPREWARM:
        endpoint = env.endpoints.create_start(
            branch_name="main",
            config_lines=cfg,
            autoprewarm=True,
            offload_lfc_interval_seconds=AUTOOFFLOAD_INTERVAL_SECS,
        )
    else:
        endpoint = env.endpoints.create_start(branch_name="main", config_lines=cfg)

    pg_conn = endpoint.connect()
    pg_cur = pg_conn.cursor()
    pg_cur.execute("create schema neon; create extension neon with schema neon")
    pg_cur.execute("create database lfc")

    lfc_conn = endpoint.connect(dbname="lfc")
    lfc_cur = lfc_conn.cursor()
    log.info(f"Inserting {n_records} rows")
    lfc_cur.execute("create table t(pk integer primary key, payload text default repeat('?', 128))")
    lfc_cur.execute(f"insert into t (pk) values (generate_series(1,{n_records}))")
    log.info(f"Inserted {n_records} rows")

    client = endpoint.http_client()
    lfc_state = offload_lfc(method, client, pg_cur)

    endpoint.stop()
    if method == PrewarmMethod.AUTOPREWARM:
        endpoint.start(autoprewarm=True, offload_lfc_interval_seconds=AUTOOFFLOAD_INTERVAL_SECS)
    else:
        endpoint.start()

    pg_conn = endpoint.connect()
    pg_cur = pg_conn.cursor()

    lfc_conn = endpoint.connect(dbname="lfc")
    lfc_cur = lfc_conn.cursor()
    prewarm_endpoint(method, client, pg_cur, lfc_state)

    pg_cur.execute(
        "select lfc_value from neon.neon_lfc_stats where lfc_key='file_cache_used_pages'"
    )
    lfc_used_pages = pg_cur.fetchall()[0][0]
    log.info(f"Used LFC size: {lfc_used_pages}")
    pg_cur.execute("select * from neon.get_prewarm_info()")
    total, prewarmed, skipped, _ = pg_cur.fetchall()[0]
    assert lfc_used_pages > 10000
    assert total > 0
    assert prewarmed > 0
    assert total == prewarmed + skipped

    lfc_cur.execute("select sum(pk) from t")
    assert lfc_cur.fetchall()[0][0] == n_records * (n_records + 1) / 2

    desired = {"status": "completed", "total": total, "prewarmed": prewarmed, "skipped": skipped}
    check_prewarmed_contains(method, client, desired)


@pytest.mark.skipif(not USE_LFC, reason="LFC is disabled, skipping")
def test_lfc_prewarm_respects_destination_limit(neon_simple_env: NeonEnv):
    endpoint = neon_simple_env.endpoints.create_start(
        branch_name="main",
        config_lines=[
            "autovacuum = off",
            "shared_buffers = 1MB",
            "neon.max_file_cache_size = 1GB",
            "neon.file_cache_size_limit = 1GB",
            "neon.file_cache_chunk_size = 8",
            "neon.file_cache_prewarm_limit = 1000",
        ],
    )
    with endpoint.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("CREATE SCHEMA neon; CREATE EXTENSION neon WITH SCHEMA neon")
            cur.execute(
                "CREATE TABLE prewarm_budget AS "
                "SELECT i, repeat('x', 128) AS payload FROM generate_series(1, 50000) AS g(i)"
            )
            cur.execute("SELECT sum(length(payload)) FROM prewarm_budget")
            assert cur.fetchone() == (6400000,)
            cur.execute("SELECT neon.get_local_cache_state(1000)")
            (state,) = cur.fetchone()
            assert state is not None
    endpoint.stop()
    endpoint.start()

    endpoint.safe_psql("ALTER SYSTEM SET neon.file_cache_prewarm_limit = 2")
    endpoint.safe_psql("SELECT pg_reload_conf()")

    def limit_installed():
        assert endpoint.safe_psql("SHOW neon.file_cache_prewarm_limit") == [("2",)]

    wait_until(limit_installed)
    with endpoint.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT neon.prewarm_local_cache(%s, 1)", (state,))
            cur.execute(
                "SELECT total_pages, prewarmed_pages, skipped_pages, active_workers "
                "FROM neon.get_prewarm_info()"
            )
            total, prewarmed, skipped, active = cur.fetchone()
            assert 0 < total <= 2 * 8
            assert prewarmed > 0
            assert total == prewarmed + skipped
            assert active == 0
            cur.execute("SELECT count(*), sum(length(payload)) FROM prewarm_budget")
            assert cur.fetchone() == (50000, 6400000)


@pytest.mark.parametrize("chunk_blocks", [1, 8, 128])
def test_lfc_prewarm_compact_dsm_and_allocation_recovery(
    neon_env_builder: NeonEnvBuilder, pg_config_configure: str, chunk_blocks: int
):
    """从原SQL入口检验紧凑DSM的真实字节、分配错误清理及同boot后继预热。"""
    # 通用regress可能使用未编译注入点的PG，先核能力再启动独占环境。
    # 专项入口显式设置严格开关；任何能力缺失必须失败，不能以skip充数。
    # 通用环境若确有注入能力，仍可直接选择本用例运行。
    required = os.environ.get("NEON_TEST_REQUIRE_LFC_INJECTION", "0")
    assert required in ("0", "1"), "invalid NEON_TEST_REQUIRE_LFC_INJECTION"
    unavailable = None
    if not USE_LFC:
        unavailable = "LFC is disabled"
    elif neon_env_builder.pg_version < PgVersion.V17:
        unavailable = "the injection_points extension requires PostgreSQL 17+"
    elif "--enable-injection-points" not in shlex.split(pg_config_configure):
        unavailable = "PostgreSQL was configured without --enable-injection-points"
    if unavailable is not None:
        if required == "1":
            pytest.fail(f"required LFC injection test unavailable: {unavailable}")
        pytest.skip(unavailable)
    env = neon_env_builder.init_start()
    endpoint = env.endpoints.create_start(
        branch_name="main",
        config_lines=[
            "autovacuum = off",
            "shared_buffers = 1MB",
            "neon.max_file_cache_size = 256MB",
            "neon.file_cache_size_limit = 256MB",
            f"neon.file_cache_chunk_size = {chunk_blocks}",
            "neon.file_cache_prewarm_limit = 1000",
            "dynamic_shared_memory_type = mmap",
            "min_dynamic_shared_memory = 0",
        ],
    )
    with closing(endpoint.connect()) as conn, conn.cursor() as cur:
        cur.execute("CREATE SCHEMA neon; CREATE EXTENSION neon WITH SCHEMA neon")
        # 此测试必须用启用injection-points的PG候选；缺模块/能力应明确失败，
        # 不能把未触及真实分配入口的运行记为负控通过。
        cur.execute("CREATE EXTENSION injection_points")
        cur.execute(
            "CREATE TABLE compact_prewarm AS "
            "SELECT i, repeat('x', 128) AS payload FROM generate_series(1, 50000) AS g(i)"
        )
        cur.execute("SELECT sum(length(payload)) FROM compact_prewarm")
        assert cur.fetchone() == (6400000,)
        cur.execute("SELECT neon.get_local_cache_state(1000)")
        state = bytes(cur.fetchone()[0])
    endpoint.stop()
    endpoint.start()

    # compute_ctl重启会重建PGDATA，必须在目标boot加载并实读新上限。
    endpoint.safe_psql("ALTER SYSTEM SET neon.file_cache_prewarm_limit = 2")
    endpoint.safe_psql("SELECT pg_reload_conf()")

    def target_limit_loaded():
        assert endpoint.safe_psql("SHOW neon.file_cache_prewarm_limit") == [("2",)]

    wait_until(target_limit_loaded)

    # bytea不包含C varlena头；这些offset对应发行中FileCacheState与BufferTag
    # 的实际布局。后续逐字节比较原chunk及原bitmap，避免只检查结果页数。
    magic, chunks, _pages, chunk_log = struct.unpack_from("=IIIH", state)
    assert magic == 0xFCFCFCFC and chunks > 2 and (1 << chunk_log) == chunk_blocks
    source_bitmap = state[16 + chunks * 20 :]
    selected_bits = 2 * chunk_blocks
    expected_bitmap = bytearray(source_bitmap[: (selected_bits + 7) // 8])
    if selected_bits % 8:
        expected_bitmap[-1] &= (1 << (selected_bits % 8)) - 1
    expected_pages = sum(value.bit_count() for value in expected_bitmap)
    assert expected_pages > 0

    dsm_dir = endpoint.pg_data_dir_path() / "pg_dynshmem"

    def dsm_files():
        return {
            path.name: (path.stat().st_ino, path.stat().st_size)
            for path in dsm_dir.glob("mmap.*")
        }

    def read_sql(sql, params=None):
        observer.execute(sql, params)
        return observer.fetchall()

    with (
        closing(endpoint.connect()) as conn,
        conn.cursor() as cur,
        closing(endpoint.connect()) as observer_conn,
        observer_conn.cursor() as observer,
    ):
        cur.execute("SELECT pg_postmaster_start_time(), pg_backend_pid()")
        boot, backend_pid = cur.fetchone()
        cur.execute("SELECT injection_points_set_local()")
        before = dsm_files()
        loaded_total = 0
        for point in (
            "neon-lfc-prewarm-before-dsm-create",
            "neon-lfc-prewarm-after-dsm-create",
        ):
            cur.execute("SELECT injection_points_attach(%s, 'error')", (point,))
            try:
                with pytest.raises(
                    psycopg2.Error, match=f"error triggered for injection point {point}"
                ) as failed:
                    cur.execute("SELECT neon.prewarm_local_cache(%s, 1)", (state,))
                assert failed.value.pgcode == "XX000"
            finally:
                cur.execute("SELECT injection_points_detach(%s)", (point,))
            assert dsm_files() == before
            assert endpoint.safe_psql("SELECT pg_postmaster_start_time()") == [(boot,)]
            cur.execute("SELECT neon.prewarm_local_cache(%s, 1)", (state,))
            cur.execute("SELECT * FROM neon.get_prewarm_info()")
            total, loaded, skipped, active = cur.fetchone()
            assert total == expected_pages == loaded + skipped and active == 0
            loaded_total += loaded
            assert dsm_files() == before

        # compute_ctl将pg_dynshmem指向共享/dev/shm，不能修改该目录权限。
        # 仅原子切换本夹具自己的链接到只读私有目录，让原OpenTransientFile
        # 返回EACCES；保留已有DSM映射，finally恢复原链接后再检验同boot恢复。
        assert dsm_dir.is_symlink()
        original_target = dsm_dir.readlink()
        denied_dir = endpoint.pg_data_dir_path() / "prewarm-denied-dsm"
        link_swap = endpoint.pg_data_dir_path() / "prewarm-dsm-link-swap"
        denied_dir.mkdir(mode=0o500)
        try:
            link_swap.symlink_to(denied_dir.name)
            link_swap.replace(dsm_dir)
            with pytest.raises(
                psycopg2.Error, match="could not open shared memory segment"
            ) as failed:
                cur.execute("SELECT neon.prewarm_local_cache(%s, 1)", (state,))
            assert failed.value.pgcode == "42501"
        finally:
            if link_swap.is_symlink():
                link_swap.unlink()
            link_swap.symlink_to(original_target)
            link_swap.replace(dsm_dir)
            denied_dir.rmdir()
        assert dsm_files() == before
        cur.execute("SELECT neon.prewarm_local_cache(%s, 1)", (state,))
        cur.execute("SELECT * FROM neon.get_prewarm_info()")
        total, loaded, skipped, active = cur.fetchone()
        assert total == expected_pages == loaded + skipped and active == 0
        loaded_total += loaded
        assert dsm_files() == before

        point = "neon-lfc-prewarm-after-dsm-create"
        # wait持原LFC排他锁，控制会话必须先处理reload、加载模块并预热目录。
        # 用同一会话的已执行prepared语句观察/唤醒，不能在持锁期间新建冷会话。
        assert read_sql("SHOW neon.file_cache_prewarm_limit") == [("2",)]
        observer.execute(
            "PREPARE c6_wait AS SELECT wait_event FROM pg_stat_activity WHERE pid = %s",
            (backend_pid,),
        )
        read_sql("EXECUTE c6_wait")
        observer.execute("PREPARE c6_detach(text) AS SELECT injection_points_detach($1)")
        with pytest.raises(psycopg2.Error, match="could not detach injection point"):
            read_sql("EXECUTE c6_detach('c6-unused-point')")
        observer.execute("PREPARE c6_wake(text) AS SELECT injection_points_wakeup($1)")
        with pytest.raises(psycopg2.Error, match="could not find injection point"):
            read_sql("EXECUTE c6_wake('c6-unused-point')")
        cur.execute("SELECT injection_points_attach(%s, 'wait')", (point,))
        errors = []

        def prewarm():
            try:
                cur.execute("SELECT neon.prewarm_local_cache(%s, 1)", (state,))
            except BaseException as error:
                errors.append(error)

        thread = Thread(target=prewarm)
        thread.start()
        try:

            def allocation_waiting():
                assert read_sql("EXECUTE c6_wait") == [(point,)]

            wait_until(allocation_waiting)
            allocated = dsm_files()
            added = allocated.keys() - before.keys()
            assert len(added) == 1
            raw = (dsm_dir / next(iter(added))).read_bytes()
            expected_size = 20 + 2 * 20 + len(expected_bitmap)
            assert len(raw) == expected_size < len(state) + 4
            assert int.from_bytes(raw[:4], "little") >> 2 == expected_size
            assert struct.unpack_from("=IIIH", raw, 4) == (magic, 2, expected_pages, chunk_log)
            assert raw[20:60] == state[16:56]
            assert raw[60:] == expected_bitmap
        finally:
            try:
                read_sql("EXECUTE c6_detach(%s)", (point,))
            finally:
                try:
                    read_sql("EXECUTE c6_wake(%s)", (point,))
                except psycopg2.Error:
                    # 若分配前已失败，线程会携带原错误返回；没有waiter可唤醒。
                    pass
                finally:
                    thread.join(timeout=30)
        assert not thread.is_alive()
        assert not errors, errors
        assert dsm_files() == before
        cur.execute("SELECT * FROM neon.get_prewarm_info()")
        total, prewarmed, skipped, active = cur.fetchone()
        assert total == expected_pages == prewarmed + skipped
        # 前面的恢复已可预热同一前缀，所以最后一轮可以全skip；单页chunk
        # 也可能在启动时即全部命中。其余粒度要求整个恢复序列实际取过页。
        assert (loaded_total + prewarmed > 0 or chunk_blocks == 1) and active == 0
        cur.execute("SELECT count(*), sum(length(payload)) FROM compact_prewarm")
        assert cur.fetchone() == (50000, 6400000)
        cur.execute("SELECT pg_postmaster_start_time(), pg_backend_pid()")
        assert cur.fetchone() == (boot, backend_pid)


@pytest.mark.skipif(not USE_LFC, reason="LFC is disabled, skipping")
def test_lfc_prewarm_cancel(neon_simple_env: NeonEnv):
    """
    Test we can cancel LFC prewarm and prewarm successfully after
    """
    env = neon_simple_env
    n_records = 1000000
    cfg = [
        "autovacuum = off",
        "shared_buffers=1MB",
        "neon.max_file_cache_size=1GB",
        "neon.file_cache_size_limit=1GB",
        "neon.file_cache_prewarm_limit=1000",
    ]
    endpoint = env.endpoints.create_start(branch_name="main", config_lines=cfg)

    pg_conn = endpoint.connect()
    pg_cur = pg_conn.cursor()
    pg_cur.execute("create schema neon; create extension neon with schema neon")
    pg_cur.execute("create database lfc")

    lfc_conn = endpoint.connect(dbname="lfc")
    lfc_cur = lfc_conn.cursor()
    log.info(f"Inserting {n_records} rows")
    lfc_cur.execute("create table t(pk integer primary key, payload text default repeat('?', 128))")
    lfc_cur.execute(f"insert into t (pk) values (generate_series(1,{n_records}))")
    log.info(f"Inserted {n_records} rows")

    client = endpoint.http_client()
    method = PrewarmMethod.COMPUTE_CTL
    offload_lfc(method, client, pg_cur)

    endpoint.stop()
    endpoint.start()

    thread = Thread(target=lambda: prewarm_endpoint(method, client, pg_cur, None))
    thread.start()
    pid_file = endpoint.pg_data_dir_path() / "postmaster.pid"
    postmaster = psutil.Process(int(pid_file.read_text().splitlines()[0]))

    def prewarm_workers() -> int:
        # LFC workers have no database session and may be absent from pg_stat_activity.
        count = 0
        for worker in postmaster.children():
            try:
                if any("LFC prewarm worker " in argument for argument in worker.cmdline()):
                    count += 1
            except psutil.NoSuchProcess:
                # The child may exit between discovery and reading its title.
                continue
        return count

    def assert_prewarmer_running():
        assert prewarm_workers() > 0

    try:
        wait_until(assert_prewarmer_running, interval=0.05)
    finally:
        client.cancel_prewarm_lfc()
        thread.join(timeout=30)
    assert not thread.is_alive()
    assert client.prewarm_lfc_status()["status"] == "cancelled"
    assert prewarm_workers() == 0

    prewarm_endpoint(method, client, pg_cur, None)
    assert client.prewarm_lfc_status()["status"] == "completed"


@pytest.mark.skipif(not USE_LFC, reason="LFC is disabled, skipping")
def test_lfc_prewarm_empty(neon_simple_env: NeonEnv):
    """
    Test there are no errors when trying to offload or prewarm endpoint without cache using compute_ctl.
    Endpoint without cache is simulated by turning off LFC manually, but in cloud/ setup this is
    also reproduced on fresh endpoints
    """
    env = neon_simple_env
    ep = env.endpoints.create_start("main", config_lines=["neon.file_cache_size_limit=0"])
    client = ep.http_client()
    conn = ep.connect()
    cur = conn.cursor()
    cur.execute("create schema neon; create extension neon with schema neon")
    method = PrewarmMethod.COMPUTE_CTL
    assert offload_lfc(method, client, cur)["status"] == "skipped"
    assert prewarm_endpoint(method, client, cur, None)["status"] == "skipped"


# autoprewarm isn't needed as we prewarm manually
WORKLOAD_VALUES = METHOD_VALUES[:-1]
WORKLOAD_IDS = METHOD_IDS[:-1]


@pytest.mark.skipif(not USE_LFC, reason="LFC is disabled, skipping")
@pytest.mark.parametrize("method", WORKLOAD_VALUES, ids=WORKLOAD_IDS)
def test_lfc_prewarm_under_workload(neon_simple_env: NeonEnv, method: PrewarmMethod):
    """
    Test continiously prewarming endpoint when there is a write-heavy workload going in parallel
    """
    env = neon_simple_env
    n_records = 10000
    n_threads = 4
    cfg = [
        "shared_buffers=1MB",
        "neon.max_file_cache_size=1GB",
        "neon.file_cache_size_limit=1GB",
        "neon.file_cache_prewarm_limit=1000000",
    ]
    endpoint = env.endpoints.create_start(branch_name="main", config_lines=cfg)

    pg_conn = endpoint.connect()
    pg_cur = pg_conn.cursor()
    pg_cur.execute("create schema neon; create extension neon with schema neon")
    pg_cur.execute("CREATE DATABASE lfc")

    lfc_conn = endpoint.connect(dbname="lfc")
    lfc_cur = lfc_conn.cursor()
    lfc_cur.execute(
        "create table accounts(id integer primary key, balance bigint default 0, payload text default repeat('?', 1000)) with (fillfactor=10)"
    )
    log.info(f"Inserting {n_records} rows")
    lfc_cur.execute(f"insert into accounts(id) values (generate_series(1,{n_records}))")
    log.info(f"Inserted {n_records} rows")

    http_client = endpoint.http_client()
    lfc_state = offload_lfc(method, http_client, pg_cur)
    running = True
    n_prewarms = 0

    def workload():
        lfc_conn = endpoint.connect(dbname="lfc")
        lfc_cur = lfc_conn.cursor()
        n_transfers = 0
        while running:
            src = random.randint(1, n_records)
            dst = random.randint(1, n_records)
            lfc_cur.execute(f"update accounts set balance=balance-100 where id={src}")
            lfc_cur.execute(f"update accounts set balance=balance+100 where id={dst}")
            n_transfers += 1
        log.info(f"Number of transfers: {n_transfers}")

    def prewarm():
        pg_conn = endpoint.connect()
        pg_cur = pg_conn.cursor()
        while running:
            pg_cur.execute("alter system set neon.file_cache_size_limit='1MB'")
            pg_cur.execute("select pg_reload_conf()")
            pg_cur.execute("alter system set neon.file_cache_size_limit='1GB'")
            pg_cur.execute("select pg_reload_conf()")
            prewarm_endpoint(method, http_client, pg_cur, lfc_state)
            nonlocal n_prewarms
            n_prewarms += 1
        log.info(f"Number of prewarms: {n_prewarms}")

    workload_threads = []
    for _ in range(n_threads):
        t = Thread(target=workload)
        workload_threads.append(t)
        t.start()

    prewarm_thread = Thread(target=prewarm)
    prewarm_thread.start()

    def prewarmed():
        assert n_prewarms > 3

    wait_until(prewarmed, timeout=40)  # debug builds don't finish in 20s

    running = False
    for t in workload_threads:
        t.join()
    prewarm_thread.join()

    lfc_cur.execute("select sum(balance) from accounts")
    total_balance = lfc_cur.fetchall()[0][0]
    assert total_balance == 0

    if method == PrewarmMethod.POSTGRES:
        return
    desired = {
        OFFLOAD_LABEL: 1,
        PREWARM_LABEL: n_prewarms,
        OFFLOAD_ERR_LABEL: 0,
        PREWARM_ERR_LABEL: 0,
    }
    assert prom_parse(http_client) == desired
