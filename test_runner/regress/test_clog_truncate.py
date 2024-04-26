import os
import time

from fixtures.log_helper import log
from fixtures.neon_fixtures import NeonEnv
from fixtures.utils import query_scalar


#
# Test compute node start after clog truncation
#
def test_clog_truncate(neon_simple_env: NeonEnv):
    env = neon_simple_env
    env.neon_cli.create_branch("test_clog_truncate", "empty")

    # set aggressive autovacuum to make sure that truncation will happen
    config = [
        "autovacuum_max_workers=10",
        "autovacuum_vacuum_threshold=0",
        "autovacuum_vacuum_insert_threshold=0",
        "autovacuum_vacuum_cost_delay=0",
        "autovacuum_vacuum_cost_limit=10000",
        "autovacuum_naptime =1s",
        "autovacuum_freeze_max_age=100000",
    ]

    endpoint = env.endpoints.create_start("test_clog_truncate", config_lines=config)

    # Install extension containing function needed for test
    endpoint.safe_psql("CREATE EXTENSION neon_test_utils")

    # Consume many xids to advance clog
    with endpoint.cursor() as cur:
        cur.execute("select test_consume_xids(1000*1000*10);")
        log.info("xids consumed")

        # call a checkpoint to trigger TruncateSubtrans
        cur.execute("CHECKPOINT;")

        # ensure WAL flush
        cur.execute("select txid_current()")
        log.info(cur.fetchone())

    # wait for autovacuum to truncate the pg_xact
    # XXX Is it worth to add a timeout here?
    pg_xact_0000_path = os.path.join(endpoint.pg_xact_dir_path(), "0000")
    log.info(f"pg_xact_0000_path = {pg_xact_0000_path}")

    while os.path.isfile(pg_xact_0000_path):
        log.info(f"file exists. wait for truncation: {pg_xact_0000_path=}")
        time.sleep(5)

    # checkpoint to advance latest lsn
    with endpoint.cursor() as cur:
        cur.execute("CHECKPOINT;")
        lsn_after_truncation = query_scalar(cur, "select pg_current_wal_insert_lsn()")

    # create new branch after clog truncation and start a compute node on it
    log.info(f"create branch at lsn_after_truncation {lsn_after_truncation}")
    env.neon_cli.create_branch(
        "test_clog_truncate_new", "test_clog_truncate", ancestor_start_lsn=lsn_after_truncation
    )
    endpoint2 = env.endpoints.create_start("test_clog_truncate_new")

    # check that new node doesn't contain truncated segment
    pg_xact_0000_path_new = os.path.join(endpoint2.pg_xact_dir_path(), "0000")
    log.info(f"pg_xact_0000_path_new = {pg_xact_0000_path_new}")
    assert os.path.isfile(pg_xact_0000_path_new) is False
