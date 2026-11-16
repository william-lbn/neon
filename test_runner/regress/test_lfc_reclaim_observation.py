"""原 LFC 回收事实只读回归：计数、路径和原回收序列均不是降限授权。"""
from contextlib import closing

import psycopg2
import pytest
from fixtures.neon_fixtures import NeonEnv
from fixtures.utils import USE_LFC, wait_until


@pytest.mark.skipif(not USE_LFC, reason="LFC is disabled")
def test_lfc_reclaim_observation(neon_simple_env: NeonEnv):
    env = neon_simple_env
    endpoint = env.endpoints.create_start(
        branch_name="main",
        config_lines=[
            "shared_buffers=2MB",
            "autovacuum=off",
            "neon.max_file_cache_size=64MB",
            "neon.file_cache_size_limit=32MB",
        ],
    )
    with closing(endpoint.connect()) as conn:
        conn.autocommit = True
        with conn.cursor() as cur:
            cur.execute("CREATE SCHEMA IF NOT EXISTS neon; CREATE EXTENSION IF NOT EXISTS neon WITH SCHEMA neon")
            cur.execute("ALTER EXTENSION neon UPDATE")
            cur.execute("CREATE TABLE reclaim_rows AS SELECT n,repeat(md5(n::text),8) payload FROM generate_series(1,50000) n")
            query = "SELECT count(*),sum(length(payload)) FROM reclaim_rows"
            for _ in range(3):
                cur.execute(query)
                assert cur.fetchone() == (50000, 12800000)

            def state():
                cur.execute("SELECT neon.neon_lfc_reclaim_state()")
                value = cur.fetchone()[0]
                assert value["old_references"] == "unknown"
                assert value["release_proven"] is False
                assert value["metadata_scope"] == "point_in_time_not_io_barrier"
                return value

            before = state()
            assert before["available"] and before["used_chunks"] > 0
            assert before["max_bytes"] == str(64 * 1024**2)
            cur.execute("SELECT pg_postmaster_start_time(),pg_backend_pid()")
            identity = cur.fetchone()

            def change(limit):
                cur.execute(f"ALTER SYSTEM SET neon.file_cache_size_limit='{limit}MB'")
                cur.execute("SELECT pg_reload_conf()")

                def installed():
                    value = state()
                    assert value["shared_limit_bytes"] == str(limit * 1024**2)
                    assert value["configured_limit_bytes"] == str(limit * 1024**2)

                wait_until(installed)
                return state()

            smaller = change(16)
            assert smaller["generation"] == before["generation"]
            assert int(smaller["reclaim_sequence"]) > int(before["reclaim_sequence"])
            assert smaller["last_reclaim"]["target_bytes"] == str(16 * 1024**2)
            old_sequence = smaller["reclaim_sequence"]
            disabled = change(0)
            assert int(disabled["generation"]) > int(smaller["generation"])
            assert int(disabled["reclaim_sequence"]) > int(old_sequence)
            assert disabled["last_reclaim"]["sequence"] != old_sequence
            assert disabled["last_reclaim"]["unlink_status"] == "succeeded"
            assert disabled["last_reclaim"]["recreate_status"] == "succeeded"
            assert disabled["current_path"]["available"]
            assert disabled["current_path"]["allocated_bytes"] == "0"
            cur.execute(query)
            assert cur.fetchone() == (50000, 12800000)
            restored = change(32)
            assert int(restored["reclaim_sequence"]) > int(disabled["reclaim_sequence"])
            cur.execute(query)
            assert cur.fetchone() == (50000, 12800000)
            cur.execute("SELECT pg_postmaster_start_time(),pg_backend_pid()")
            assert cur.fetchone() == identity

            # 原普通租户没有此元数据权限，授予 pg_monitor 后才可读取；不授予写缓存能力。
            cur.execute("CREATE ROLE lfc_observation_reader; GRANT USAGE ON SCHEMA neon TO lfc_observation_reader; SET ROLE lfc_observation_reader")
            with pytest.raises(psycopg2.errors.InsufficientPrivilege):
                cur.execute("SELECT neon.neon_lfc_reclaim_state()")
            cur.execute("RESET ROLE; GRANT pg_monitor TO lfc_observation_reader; SET ROLE lfc_observation_reader")
            state()
            cur.execute("RESET ROLE")

    endpoint.stop()
    disabled_endpoint = env.endpoints.create_start(
        branch_name="main", config_lines=["neon.max_file_cache_size=0", "neon.file_cache_size_limit=0"]
    )
    with closing(disabled_endpoint.connect()) as conn, conn.cursor() as cur:
        cur.execute("SELECT neon.neon_lfc_reclaim_state()")
        value = cur.fetchone()[0]
        assert value["available"] is False and value["enabled"] is False
        assert value["last_reclaim"] is None
        assert value["current_path"]["available"] is False
        assert value["release_proven"] is False
