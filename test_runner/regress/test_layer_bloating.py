import os
import time

import pytest
from fixtures.log_helper import log
from fixtures.neon_fixtures import (
    NeonEnv,
    logical_replication_sync,
)
from fixtures.pg_version import PgVersion


def test_layer_bloating(neon_simple_env: NeonEnv, vanilla_pg):
    env = neon_simple_env

    if env.pg_version != PgVersion.V16:
        pytest.skip("pg_log_standby_snapshot() function is available only in PG16")

    timeline = env.neon_cli.create_branch("test_logical_replication", "empty")
    endpoint = env.endpoints.create_start(
        "test_logical_replication", config_lines=["log_statement=all"]
    )

    pg_conn = endpoint.connect()
    cur = pg_conn.cursor()

    # create table...
    cur.execute("create table t(pk integer primary key)")
    cur.execute("create publication pub1 for table t")
    # Create slot to hold WAL
    cur.execute("select pg_create_logical_replication_slot('my_slot', 'pgoutput')")

    # now start subscriber
    vanilla_pg.start()
    vanilla_pg.safe_psql("create table t(pk integer primary key)")

    connstr = endpoint.connstr().replace("'", "''")
    log.info(f"ep connstr is {endpoint.connstr()}, subscriber connstr {vanilla_pg.connstr()}")
    vanilla_pg.safe_psql(f"create subscription sub1 connection '{connstr}' publication pub1")

    cur.execute(
        """create or replace function create_snapshots(n integer) returns void as $$
                   declare
                     i integer;
                   begin
                     for i in 1..n loop
                       perform pg_log_standby_snapshot();
                     end loop;
                   end; $$ language plpgsql"""
    )
    cur.execute("set statement_timeout=0")
    cur.execute("select create_snapshots(10000)")
    # Wait logical replication to sync
    logical_replication_sync(vanilla_pg, endpoint)
    time.sleep(10)

    # Check layer file sizes
    timeline_path = "{}/tenants/{}/timelines/{}/".format(
        env.pageserver.workdir, env.initial_tenant, timeline
    )
    log.info(f"Check {timeline_path}")
    for filename in os.listdir(timeline_path):
        if filename.startswith("00000"):
            log.info(f"layer {filename} size is {os.path.getsize(timeline_path + filename)}")
            assert os.path.getsize(timeline_path + filename) < 512_000_000
