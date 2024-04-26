import time

from fixtures.log_helper import log
from fixtures.neon_fixtures import NeonEnv, NeonEnvBuilder
from fixtures.types import Lsn, TenantId


# Checks that pageserver's walreceiver state is printed in the logs during WAL wait timeout.
# Ensures that walreceiver does not run without any data inserted and only starts after the insertion.
def test_pageserver_lsn_wait_error_start(neon_env_builder: NeonEnvBuilder):
    # Trigger WAL wait timeout faster
    neon_env_builder.pageserver_config_override = "wait_lsn_timeout = '1s'"
    env = neon_env_builder.init_start()
    env.pageserver.http_client()

    tenant_id, timeline_id = env.neon_cli.create_tenant()
    expected_timeout_error = f"Timed out while waiting for WAL record at LSN {future_lsn} to arrive"
    env.pageserver.allowed_errors.append(f".*{expected_timeout_error}.*")

    try:
        trigger_wait_lsn_timeout(env, tenant_id)
    except Exception as e:
        exception_string = str(e)
        assert expected_timeout_error in exception_string, "Should time out during waiting for WAL"
        assert (
            "WalReceiver status: Not active" in exception_string
        ), "Walreceiver should not be active before any data writes"

    insert_test_elements(env, tenant_id, start=0, count=1_000)
    try:
        trigger_wait_lsn_timeout(env, tenant_id)
    except Exception as e:
        exception_string = str(e)
        assert expected_timeout_error in exception_string, "Should time out during waiting for WAL"
        assert (
            "WalReceiver status: Not active" not in exception_string
        ), "Should not be inactive anymore after INSERTs are made"
        assert "WalReceiver status" in exception_string, "But still should have some other status"


# Checks that all active safekeepers are shown in pageserver's walreceiver state printed on WAL wait timeout.
# Kills one of the safekeepers and ensures that only the active ones are printed in the state.
def test_pageserver_lsn_wait_error_safekeeper_stop(neon_env_builder: NeonEnvBuilder):
    # Trigger WAL wait timeout faster
    neon_env_builder.pageserver_config_override = """
        wait_lsn_timeout = "1s"
        tenant_config={walreceiver_connect_timeout = "2s", lagging_wal_timeout = "2s"}
    """
    # Have notable SK ids to ensure we check logs for their presence, not some other random numbers
    neon_env_builder.safekeepers_id_start = 12345
    neon_env_builder.num_safekeepers = 3
    env = neon_env_builder.init_start()
    env.pageserver.http_client()

    tenant_id, timeline_id = env.neon_cli.create_tenant()

    elements_to_insert = 1_000_000
    expected_timeout_error = f"Timed out while waiting for WAL record at LSN {future_lsn} to arrive"
    env.pageserver.allowed_errors.append(f".*{expected_timeout_error}.*")

    insert_test_elements(env, tenant_id, start=0, count=elements_to_insert)

    try:
        trigger_wait_lsn_timeout(env, tenant_id)
    except Exception as e:
        exception_string = str(e)
        assert expected_timeout_error in exception_string, "Should time out during waiting for WAL"

        for safekeeper in env.safekeepers:
            assert (
                str(safekeeper.id) in exception_string
            ), f"Should have safekeeper {safekeeper.id} printed in walreceiver state after WAL wait timeout"

    stopped_safekeeper = env.safekeepers[-1]
    stopped_safekeeper_id = stopped_safekeeper.id
    log.info(f"Stopping safekeeper {stopped_safekeeper.id}")
    stopped_safekeeper.stop()
    # sleep until stopped safekeeper is removed from candidates
    time.sleep(2)

    # Spend some more time inserting, to ensure SKs report updated statuses and walreceiver in PS have time to update its connection stats.
    insert_test_elements(env, tenant_id, start=elements_to_insert + 1, count=elements_to_insert)

    try:
        trigger_wait_lsn_timeout(env, tenant_id)
    except Exception as e:
        # Strip out the part before stdout, as it contains full command with the list of all safekeepers
        exception_string = str(e).split("stdout", 1)[-1]
        assert expected_timeout_error in exception_string, "Should time out during waiting for WAL"

        for safekeeper in env.safekeepers:
            if safekeeper.id == stopped_safekeeper_id:
                assert (
                    str(safekeeper.id) not in exception_string
                ), f"Should not have stopped safekeeper {safekeeper.id} printed in walreceiver state after 2nd WAL wait timeout"
            else:
                assert (
                    str(safekeeper.id) in exception_string
                ), f"Should have safekeeper {safekeeper.id} printed in walreceiver state after 2nd WAL wait timeout"


def insert_test_elements(env: NeonEnv, tenant_id: TenantId, start: int, count: int):
    first_element_id = start
    last_element_id = first_element_id + count
    with env.endpoints.create_start("main", tenant_id=tenant_id) as endpoint:
        with endpoint.cursor() as cur:
            cur.execute("CREATE TABLE IF NOT EXISTS t(key serial primary key, value text)")
            cur.execute(
                f"INSERT INTO t SELECT i, CONCAT('payload_', i) FROM generate_series({first_element_id},{last_element_id}) as i"
            )


future_lsn = Lsn("0/FFFFFFFF")


def trigger_wait_lsn_timeout(env: NeonEnv, tenant_id: TenantId):
    with env.endpoints.create_start(
        "main",
        tenant_id=tenant_id,
        lsn=future_lsn,
    ) as endpoint:
        with endpoint.cursor() as cur:
            cur.execute("SELECT 1")
