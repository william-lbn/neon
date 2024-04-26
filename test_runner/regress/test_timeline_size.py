import concurrent.futures
import math
import queue
import random
import threading
import time
from contextlib import closing
from pathlib import Path
from typing import Optional

import psycopg2.errors
import psycopg2.extras
import pytest
from fixtures.log_helper import log
from fixtures.neon_fixtures import (
    Endpoint,
    NeonEnv,
    NeonEnvBuilder,
    PgBin,
    VanillaPostgres,
    wait_for_last_flush_lsn,
)
from fixtures.pageserver.http import PageserverApiException
from fixtures.pageserver.utils import (
    assert_tenant_state,
    timeline_delete_wait_completed,
    wait_for_upload_queue_empty,
    wait_tenant_status_404,
    wait_until_tenant_active,
)
from fixtures.pg_version import PgVersion
from fixtures.port_distributor import PortDistributor
from fixtures.remote_storage import RemoteStorageKind
from fixtures.types import TenantId, TimelineId
from fixtures.utils import get_timeline_dir_size, wait_until


def test_timeline_size(neon_simple_env: NeonEnv):
    env = neon_simple_env
    new_timeline_id = env.neon_cli.create_branch("test_timeline_size", "empty")

    client = env.pageserver.http_client()
    client.timeline_wait_logical_size(env.initial_tenant, new_timeline_id)

    endpoint_main = env.endpoints.create_start("test_timeline_size")

    with closing(endpoint_main.connect()) as conn:
        with conn.cursor() as cur:
            cur.execute("CREATE TABLE foo (t text)")
            cur.execute(
                """
                INSERT INTO foo
                    SELECT 'long string to consume some space' || g
                    FROM generate_series(1, 10) g
            """
            )

            res = client.timeline_detail(
                env.initial_tenant, new_timeline_id, include_non_incremental_logical_size=True
            )
            assert res["current_logical_size"] == res["current_logical_size_non_incremental"]
            cur.execute("TRUNCATE foo")

            res = client.timeline_detail(
                env.initial_tenant, new_timeline_id, include_non_incremental_logical_size=True
            )
            assert res["current_logical_size"] == res["current_logical_size_non_incremental"]


def test_timeline_size_createdropdb(neon_simple_env: NeonEnv):
    env = neon_simple_env
    new_timeline_id = env.neon_cli.create_branch("test_timeline_size_createdropdb", "empty")

    client = env.pageserver.http_client()
    client.timeline_wait_logical_size(env.initial_tenant, new_timeline_id)
    timeline_details = client.timeline_detail(
        env.initial_tenant, new_timeline_id, include_non_incremental_logical_size=True
    )

    endpoint_main = env.endpoints.create_start("test_timeline_size_createdropdb")

    with closing(endpoint_main.connect()) as conn:
        with conn.cursor() as cur:
            res = client.timeline_detail(
                env.initial_tenant, new_timeline_id, include_non_incremental_logical_size=True
            )
            assert res["current_logical_size"] == res["current_logical_size_non_incremental"]
            assert (
                timeline_details["current_logical_size_non_incremental"]
                == res["current_logical_size_non_incremental"]
            ), "no writes should not change the incremental logical size"

            cur.execute("CREATE DATABASE foodb")
            with closing(endpoint_main.connect(dbname="foodb")) as conn:
                with conn.cursor() as cur2:
                    cur2.execute("CREATE TABLE foo (t text)")
                    cur2.execute(
                        """
                        INSERT INTO foo
                            SELECT 'long string to consume some space' || g
                            FROM generate_series(1, 10) g
                    """
                    )

                    res = client.timeline_detail(
                        env.initial_tenant,
                        new_timeline_id,
                        include_non_incremental_logical_size=True,
                    )
                    assert (
                        res["current_logical_size"] == res["current_logical_size_non_incremental"]
                    )

            cur.execute("DROP DATABASE foodb")

            res = client.timeline_detail(
                env.initial_tenant, new_timeline_id, include_non_incremental_logical_size=True
            )
            assert res["current_logical_size"] == res["current_logical_size_non_incremental"]


# wait until received_lsn_lag is 0
def wait_for_pageserver_catchup(endpoint_main: Endpoint, polling_interval=1, timeout=60):
    started_at = time.time()

    received_lsn_lag = 1
    while received_lsn_lag > 0:
        elapsed = time.time() - started_at
        if elapsed > timeout:
            raise RuntimeError(
                "timed out waiting for pageserver to reach pg_current_wal_flush_lsn()"
            )

        res = endpoint_main.safe_psql(
            """
            SELECT
                pg_size_pretty(neon.pg_cluster_size()),
                pg_wal_lsn_diff(pg_current_wal_flush_lsn(), received_lsn) as received_lsn_lag
            FROM neon.backpressure_lsns();
            """,
            dbname="postgres",
        )[0]
        log.info(f"pg_cluster_size = {res[0]}, received_lsn_lag = {res[1]}")
        received_lsn_lag = res[1]

        time.sleep(polling_interval)


def test_timeline_size_quota_on_startup(neon_env_builder: NeonEnvBuilder):
    env = neon_env_builder.init_start()
    client = env.pageserver.http_client()
    new_timeline_id = env.neon_cli.create_branch("test_timeline_size_quota_on_startup")

    client.timeline_wait_logical_size(env.initial_tenant, new_timeline_id)

    endpoint_main = env.endpoints.create(
        "test_timeline_size_quota_on_startup",
        # Set small limit for the test
        config_lines=["neon.max_cluster_size=30MB"],
    )
    endpoint_main.start()

    with closing(endpoint_main.connect()) as conn:
        with conn.cursor() as cur:
            cur.execute("CREATE TABLE foo (t text)")

            # Insert many rows. This query must fail because of space limit
            try:
                for _i in range(5000):
                    cur.execute(
                        """
                        INSERT INTO foo
                            SELECT 'long string to consume some space' || g
                            FROM generate_series(1, 100) g
                    """
                    )

                # If we get here, the timeline size limit failed
                log.error("Query unexpectedly succeeded")
                raise AssertionError()

            except psycopg2.errors.DiskFull as err:
                log.info(f"Query expectedly failed with: {err}")

    # Restart endpoint that reached the limit to ensure that it doesn't fail on startup
    # i.e. the size limit is not enforced during startup.
    endpoint_main.stop()
    # don't skip pg_catalog updates - it runs CREATE EXTENSION neon
    # which is needed for neon.pg_cluster_size() to work
    endpoint_main.respec(skip_pg_catalog_updates=False)
    endpoint_main.start()

    # ensure that the limit is enforced after startup
    with closing(endpoint_main.connect()) as conn:
        with conn.cursor() as cur:
            # This query must fail because of space limit
            try:
                cur.execute(
                    """
                    INSERT INTO foo
                        SELECT 'long string to consume some space' || g
                        FROM generate_series(1, 100000) g
                """
                )
                # If we get here, the timeline size limit failed
                log.error("Query unexpectedly succeeded")
                raise AssertionError()

            except psycopg2.errors.DiskFull as err:
                log.info(f"Query expectedly failed with: {err}")


def test_timeline_size_quota(neon_env_builder: NeonEnvBuilder):
    env = neon_env_builder.init_start()
    client = env.pageserver.http_client()
    new_timeline_id = env.neon_cli.create_branch("test_timeline_size_quota")

    client.timeline_wait_logical_size(env.initial_tenant, new_timeline_id)

    endpoint_main = env.endpoints.create(
        "test_timeline_size_quota",
        # Set small limit for the test
        config_lines=["neon.max_cluster_size=30MB"],
    )
    # don't skip pg_catalog updates - it runs CREATE EXTENSION neon
    # which is needed for pg_cluster_size() to work
    endpoint_main.respec(skip_pg_catalog_updates=False)
    endpoint_main.start()

    with closing(endpoint_main.connect()) as conn:
        with conn.cursor() as cur:
            cur.execute("CREATE TABLE foo (t text)")

            wait_for_pageserver_catchup(endpoint_main)

            # Insert many rows. This query must fail because of space limit
            try:
                cur.execute(
                    """
                    INSERT INTO foo
                        SELECT 'long string to consume some space' || g
                        FROM generate_series(1, 100000) g
                """
                )

                wait_for_pageserver_catchup(endpoint_main)

                cur.execute(
                    """
                    INSERT INTO foo
                        SELECT 'long string to consume some space' || g
                        FROM generate_series(1, 500000) g
                """
                )

                # If we get here, the timeline size limit failed
                log.error("Query unexpectedly succeeded")
                raise AssertionError()

            except psycopg2.errors.DiskFull as err:
                log.info(f"Query expectedly failed with: {err}")

            # drop table to free space
            cur.execute("DROP TABLE foo")

            wait_for_pageserver_catchup(endpoint_main)

            # create it again and insert some rows. This query must succeed
            cur.execute("CREATE TABLE foo (t text)")
            cur.execute(
                """
                INSERT INTO foo
                    SELECT 'long string to consume some space' || g
                    FROM generate_series(1, 10000) g
            """
            )

            wait_for_pageserver_catchup(endpoint_main)

            cur.execute("SELECT * from pg_size_pretty(neon.pg_cluster_size())")
            pg_cluster_size = cur.fetchone()
            log.info(f"pg_cluster_size = {pg_cluster_size}")

    new_res = client.timeline_detail(
        env.initial_tenant, new_timeline_id, include_non_incremental_logical_size=True
    )
    assert (
        new_res["current_logical_size"] == new_res["current_logical_size_non_incremental"]
    ), "after the WAL is streamed, current_logical_size is expected to be calculated and to be equal its non-incremental value"


@pytest.mark.parametrize("deletion_method", ["tenant_detach", "timeline_delete"])
def test_timeline_initial_logical_size_calculation_cancellation(
    neon_env_builder: NeonEnvBuilder, deletion_method: str
):
    env = neon_env_builder.init_start()
    client = env.pageserver.http_client()

    tenant_id = env.initial_tenant
    timeline_id = env.initial_timeline

    # load in some data
    endpoint = env.endpoints.create_start("main", tenant_id=tenant_id)
    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (x INTEGER)",
            "INSERT INTO foo SELECT g FROM generate_series(1, 10000) g",
        ]
    )
    wait_for_last_flush_lsn(env, endpoint, tenant_id, timeline_id)
    endpoint.stop()

    # restart with failpoint inside initial size calculation task
    env.pageserver.stop()
    env.pageserver.start(
        extra_env_vars={"FAILPOINTS": "timeline-calculate-logical-size-pause=pause"}
    )

    wait_until_tenant_active(client, tenant_id)

    # kick off initial size calculation task (the response we get here is the estimated size)
    def assert_size_calculation_not_done():
        details = client.timeline_detail(
            tenant_id, timeline_id, include_non_incremental_logical_size=True
        )
        assert details["current_logical_size"] != details["current_logical_size_non_incremental"]

    assert_size_calculation_not_done()
    # ensure we're really stuck
    time.sleep(5)
    assert_size_calculation_not_done()

    log.info(
        f"try to delete the timeline using {deletion_method}, this should cancel size computation tasks and wait for them to finish"
    )
    delete_timeline_success: queue.Queue[bool] = queue.Queue(maxsize=1)

    def delete_timeline_thread_fn():
        try:
            if deletion_method == "tenant_detach":
                client.tenant_detach(tenant_id)
            elif deletion_method == "timeline_delete":
                timeline_delete_wait_completed(client, tenant_id, timeline_id)
            delete_timeline_success.put(True)
        except PageserverApiException:
            delete_timeline_success.put(False)
            raise

    delete_timeline_thread = threading.Thread(target=delete_timeline_thread_fn)
    delete_timeline_thread.start()
    # give it some time to settle in the state where it waits for size computation task
    time.sleep(5)
    if not delete_timeline_success.empty():
        raise AssertionError(
            f"test is broken, the {deletion_method} should be stuck waiting for size computation task, got result {delete_timeline_success.get()}"
        )

    log.info(
        "resume the size calculation. The failpoint checks that the timeline directory still exists."
    )
    client.configure_failpoints(("timeline-calculate-logical-size-check-dir-exists", "return"))
    client.configure_failpoints(("timeline-calculate-logical-size-pause", "off"))

    log.info("wait for delete timeline thread to finish and assert that it succeeded")
    assert delete_timeline_success.get()

    # if the implementation is incorrect, the teardown would complain about an error log
    # message emitted by the code behind failpoint "timeline-calculate-logical-size-check-dir-exists"


def test_timeline_physical_size_init(neon_env_builder: NeonEnvBuilder):
    neon_env_builder.enable_pageserver_remote_storage(RemoteStorageKind.LOCAL_FS)

    env = neon_env_builder.init_start()

    new_timeline_id = env.neon_cli.create_branch("test_timeline_physical_size_init")
    endpoint = env.endpoints.create_start("test_timeline_physical_size_init")

    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (t text)",
            """INSERT INTO foo
           SELECT 'long string to consume some space' || g
           FROM generate_series(1, 1000) g""",
        ]
    )

    wait_for_last_flush_lsn(env, endpoint, env.initial_tenant, new_timeline_id)

    # restart the pageserer to force calculating timeline's initial physical size
    env.pageserver.stop()
    env.pageserver.start()

    # Wait for the tenant to be loaded
    client = env.pageserver.http_client()
    wait_until(
        number_of_iterations=5,
        interval=1,
        func=lambda: assert_tenant_state(client, env.initial_tenant, "Active"),
    )

    assert_physical_size_invariants(
        get_physical_size_values(env, env.initial_tenant, new_timeline_id),
    )


def test_timeline_physical_size_post_checkpoint(neon_env_builder: NeonEnvBuilder):
    neon_env_builder.enable_pageserver_remote_storage(RemoteStorageKind.LOCAL_FS)

    env = neon_env_builder.init_start()

    pageserver_http = env.pageserver.http_client()
    new_timeline_id = env.neon_cli.create_branch("test_timeline_physical_size_post_checkpoint")
    endpoint = env.endpoints.create_start("test_timeline_physical_size_post_checkpoint")

    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (t text)",
            """INSERT INTO foo
           SELECT 'long string to consume some space' || g
           FROM generate_series(1, 1000) g""",
        ]
    )

    wait_for_last_flush_lsn(env, endpoint, env.initial_tenant, new_timeline_id)
    pageserver_http.timeline_checkpoint(env.initial_tenant, new_timeline_id)

    def check():
        assert_physical_size_invariants(
            get_physical_size_values(env, env.initial_tenant, new_timeline_id),
        )

    wait_until(10, 1, check)


def test_timeline_physical_size_post_compaction(neon_env_builder: NeonEnvBuilder):
    neon_env_builder.enable_pageserver_remote_storage(RemoteStorageKind.LOCAL_FS)

    # Disable background compaction as we don't want it to happen after `get_physical_size` request
    # and before checking the expected size on disk, which makes the assertion failed
    neon_env_builder.pageserver_config_override = (
        "tenant_config={checkpoint_distance=100000, compaction_period='10m'}"
    )

    env = neon_env_builder.init_start()
    pageserver_http = env.pageserver.http_client()

    new_timeline_id = env.neon_cli.create_branch("test_timeline_physical_size_post_compaction")
    endpoint = env.endpoints.create_start("test_timeline_physical_size_post_compaction")

    # We don't want autovacuum to run on the table, while we are calculating the
    # physical size, because that could cause a new layer to be created and a
    # mismatch between the incremental and non-incremental size. (If that still
    # happens, because of some other background activity or autovacuum on other
    # tables, we could simply retry the size calculations. It's unlikely that
    # that would happen more than once.)
    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (t text) WITH (autovacuum_enabled = off)",
            """INSERT INTO foo
           SELECT 'long string to consume some space' || g
           FROM generate_series(1, 100000) g""",
        ]
    )

    wait_for_last_flush_lsn(env, endpoint, env.initial_tenant, new_timeline_id)

    # shutdown safekeepers to prevent new data from coming in
    endpoint.stop()  # We can't gracefully stop after safekeepers die
    for sk in env.safekeepers:
        sk.stop()

    pageserver_http.timeline_checkpoint(env.initial_tenant, new_timeline_id)
    pageserver_http.timeline_compact(env.initial_tenant, new_timeline_id)

    wait_for_upload_queue_empty(pageserver_http, env.initial_tenant, new_timeline_id)

    assert_physical_size_invariants(
        get_physical_size_values(env, env.initial_tenant, new_timeline_id),
    )


def test_timeline_physical_size_post_gc(neon_env_builder: NeonEnvBuilder):
    neon_env_builder.enable_pageserver_remote_storage(RemoteStorageKind.LOCAL_FS)

    # Disable background compaction and GC as we don't want it to happen after `get_physical_size` request
    # and before checking the expected size on disk, which makes the assertion failed
    neon_env_builder.pageserver_config_override = "tenant_config={checkpoint_distance=100000, compaction_period='0s', gc_period='0s', pitr_interval='1s'}"

    env = neon_env_builder.init_start()
    pageserver_http = env.pageserver.http_client()

    new_timeline_id = env.neon_cli.create_branch("test_timeline_physical_size_post_gc")
    endpoint = env.endpoints.create_start("test_timeline_physical_size_post_gc")

    # Like in test_timeline_physical_size_post_compaction, disable autovacuum
    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (t text) WITH (autovacuum_enabled = off)",
            """INSERT INTO foo
           SELECT 'long string to consume some space' || g
           FROM generate_series(1, 100000) g""",
        ]
    )

    wait_for_last_flush_lsn(env, endpoint, env.initial_tenant, new_timeline_id)
    pageserver_http.timeline_checkpoint(env.initial_tenant, new_timeline_id)

    endpoint.safe_psql(
        """
        INSERT INTO foo
            SELECT 'long string to consume some space' || g
            FROM generate_series(1, 100000) g
    """
    )

    wait_for_last_flush_lsn(env, endpoint, env.initial_tenant, new_timeline_id)
    pageserver_http.timeline_checkpoint(env.initial_tenant, new_timeline_id)
    pageserver_http.timeline_gc(env.initial_tenant, new_timeline_id, gc_horizon=None)

    wait_for_upload_queue_empty(pageserver_http, env.initial_tenant, new_timeline_id)

    assert_physical_size_invariants(
        get_physical_size_values(env, env.initial_tenant, new_timeline_id),
    )


# The timeline logical and physical sizes are also exposed as prometheus metrics.
# Test the metrics.
def test_timeline_size_metrics(
    neon_simple_env: NeonEnv,
    test_output_dir: Path,
    port_distributor: PortDistributor,
    pg_distrib_dir: Path,
    pg_version: PgVersion,
):
    env = neon_simple_env
    pageserver_http = env.pageserver.http_client()

    new_timeline_id = env.neon_cli.create_branch("test_timeline_size_metrics")
    endpoint = env.endpoints.create_start("test_timeline_size_metrics")

    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (t text)",
            """INSERT INTO foo
           SELECT 'long string to consume some space' || g
           FROM generate_series(1, 100000) g""",
        ]
    )

    wait_for_last_flush_lsn(env, endpoint, env.initial_tenant, new_timeline_id)
    pageserver_http.timeline_checkpoint(env.initial_tenant, new_timeline_id)

    # get the metrics and parse the metric for the current timeline's physical size
    metrics = env.pageserver.http_client().get_metrics()
    tl_physical_size_metric = metrics.query_one(
        name="pageserver_resident_physical_size",
        filter={
            "tenant_id": str(env.initial_tenant),
            "timeline_id": str(new_timeline_id),
        },
    ).value

    # assert that the physical size metric matches the actual physical size on disk
    timeline_path = env.pageserver.timeline_dir(env.initial_tenant, new_timeline_id)
    assert tl_physical_size_metric == get_timeline_dir_size(timeline_path)

    # Check that the logical size metric is sane, and matches
    tl_logical_size_metric = metrics.query_one(
        name="pageserver_current_logical_size",
        filter={
            "tenant_id": str(env.initial_tenant),
            "timeline_id": str(new_timeline_id),
        },
    ).value

    pgdatadir = test_output_dir / "pgdata-vanilla"
    pg_bin = PgBin(test_output_dir, pg_distrib_dir, pg_version)
    port = port_distributor.get_port()
    with VanillaPostgres(pgdatadir, pg_bin, port) as vanilla_pg:
        vanilla_pg.start()

        # Create database based on template0 because we can't connect to template0
        vanilla_pg.safe_psql("CREATE TABLE foo (t text)")
        vanilla_pg.safe_psql(
            """INSERT INTO foo
                                SELECT 'long string to consume some space' || g
                                FROM generate_series(1, 100000) g"""
        )
        vanilla_size_sum = vanilla_pg.safe_psql(
            "select sum(pg_database_size(oid)) from pg_database"
        )[0][0]

    # Compare the size with Vanilla postgres.
    # Allow some slack, because the logical size metric includes some things like
    # the SLRUs that are not included in pg_database_size().
    assert math.isclose(tl_logical_size_metric, vanilla_size_sum, abs_tol=2 * 1024 * 1024)

    # The sum of the sizes of all databases, as seen by pg_database_size(), should also
    # be close. Again allow some slack, the logical size metric includes some things like
    # the SLRUs that are not included in pg_database_size().
    dbsize_sum = endpoint.safe_psql("select sum(pg_database_size(oid)) from pg_database")[0][0]
    assert math.isclose(dbsize_sum, tl_logical_size_metric, abs_tol=2 * 1024 * 1024)


def test_tenant_physical_size(neon_env_builder: NeonEnvBuilder):
    random.seed(100)

    neon_env_builder.enable_pageserver_remote_storage(RemoteStorageKind.LOCAL_FS)

    env = neon_env_builder.init_start()

    pageserver_http = env.pageserver.http_client()
    client = env.pageserver.http_client()

    tenant, timeline = env.neon_cli.create_tenant()

    def get_timeline_resident_physical_size(timeline: TimelineId):
        sizes = get_physical_size_values(env, tenant, timeline)
        assert_physical_size_invariants(sizes)
        return sizes.prometheus_resident_physical

    timeline_total_resident_physical_size = get_timeline_resident_physical_size(timeline)
    for i in range(10):
        n_rows = random.randint(100, 1000)

        timeline = env.neon_cli.create_branch(f"test_tenant_physical_size_{i}", tenant_id=tenant)
        endpoint = env.endpoints.create_start(f"test_tenant_physical_size_{i}", tenant_id=tenant)

        endpoint.safe_psql_many(
            [
                "CREATE TABLE foo (t text)",
                f"INSERT INTO foo SELECT 'long string to consume some space' || g FROM generate_series(1, {n_rows}) g",
            ]
        )

        wait_for_last_flush_lsn(env, endpoint, tenant, timeline)
        pageserver_http.timeline_checkpoint(tenant, timeline)

        wait_for_upload_queue_empty(pageserver_http, tenant, timeline)

        timeline_total_resident_physical_size += get_timeline_resident_physical_size(timeline)

        endpoint.stop()

    # ensure that tenant_status current_physical size reports sum of timeline current_physical_size
    tenant_current_physical_size = int(
        client.tenant_status(tenant_id=tenant)["current_physical_size"]
    )
    assert tenant_current_physical_size == sum(
        [tl["current_physical_size"] for tl in client.timeline_list(tenant_id=tenant)]
    )
    # since we don't do layer eviction, current_physical_size is identical to resident physical size
    assert timeline_total_resident_physical_size == tenant_current_physical_size


class TimelinePhysicalSizeValues:
    api_current_physical: int
    prometheus_resident_physical: float
    prometheus_remote_physical: Optional[float] = None
    python_timelinedir_layerfiles_physical: int
    layer_map_file_size_sum: int


def get_physical_size_values(
    env: NeonEnv,
    tenant_id: TenantId,
    timeline_id: TimelineId,
) -> TimelinePhysicalSizeValues:
    res = TimelinePhysicalSizeValues()

    client = env.pageserver.http_client()

    res.layer_map_file_size_sum = sum(
        layer.layer_file_size or 0
        for layer in client.layer_map_info(tenant_id, timeline_id).historic_layers
    )

    metrics = client.get_metrics()
    metrics_filter = {"tenant_id": str(tenant_id), "timeline_id": str(timeline_id)}
    res.prometheus_resident_physical = metrics.query_one(
        "pageserver_resident_physical_size", metrics_filter
    ).value
    res.prometheus_remote_physical = metrics.query_one(
        "pageserver_remote_physical_size", metrics_filter
    ).value

    detail = client.timeline_detail(
        tenant_id, timeline_id, include_timeline_dir_layer_file_size_sum=True
    )
    res.api_current_physical = detail["current_physical_size"]

    timeline_path = env.pageserver.timeline_dir(tenant_id, timeline_id)
    res.python_timelinedir_layerfiles_physical = get_timeline_dir_size(timeline_path)

    return res


def assert_physical_size_invariants(sizes: TimelinePhysicalSizeValues):
    # resident phyiscal size is defined as
    assert sizes.python_timelinedir_layerfiles_physical == sizes.prometheus_resident_physical
    assert sizes.python_timelinedir_layerfiles_physical == sizes.layer_map_file_size_sum

    # we don't do layer eviction, so, all layers are resident
    assert sizes.api_current_physical == sizes.prometheus_resident_physical
    assert sizes.prometheus_resident_physical == sizes.prometheus_remote_physical
    # XXX would be nice to assert layer file physical storage utilization here as well, but we can only do that for LocalFS


def test_ondemand_activation(neon_env_builder: NeonEnvBuilder):
    """
    Tenants warmuping up opportunistically will wait for one another's logical size calculations to complete
    before proceeding.  However, they skip this if a client is actively trying to access them.

    This test is not purely about logical sizes, but logical size calculation is the phase that we
    use as a proxy for "warming up" in this test: it happens within the semaphore guard used
    to limit concurrent tenant warm-up.
    """

    # We will run with the limit set to 1, so that once we have one tenant stuck
    # in a pausable failpoint, the rest are prevented from proceeding through warmup.
    neon_env_builder.pageserver_config_override = "concurrent_tenant_warmup = '1'"

    env = neon_env_builder.init_start()
    pageserver_http = env.pageserver.http_client()

    # Create some tenants
    n_tenants = 10
    tenant_ids = {env.initial_tenant}
    for _i in range(0, n_tenants - 1):
        tenant_id = TenantId.generate()
        env.neon_cli.create_tenant(tenant_id)
        tenant_ids.add(tenant_id)

    # Restart pageserver with logical size calculations paused
    env.pageserver.stop()
    env.pageserver.start(
        extra_env_vars={"FAILPOINTS": "timeline-calculate-logical-size-pause=pause"}
    )

    def get_tenant_states():
        states = {}
        log.info(f"Tenant ids: {tenant_ids}")
        for tenant_id in tenant_ids:
            tenant = pageserver_http.tenant_status(tenant_id=tenant_id)
            states[tenant_id] = tenant["state"]["slug"]
        log.info(f"Tenant states: {states}")
        return states

    def at_least_one_active():
        assert "Active" in set(get_tenant_states().values())

    # One tenant should activate, then get stuck in their logical size calculation
    wait_until(10, 1, at_least_one_active)

    # Wait some walltime to gain confidence that other tenants really are stuck and not proceeding to activate
    time.sleep(5)

    # We should see one tenant win the activation race, and enter logical size calculation.  The rest
    # will stay in Attaching state, waiting for the "warmup_limit" semaphore
    expect_activated = 1
    states = get_tenant_states()
    assert len([s for s in states.values() if s == "Active"]) == expect_activated
    assert len([s for s in states.values() if s == "Attaching"]) == n_tenants - expect_activated

    assert (
        pageserver_http.get_metric_value("pageserver_tenant_startup_scheduled_total") == n_tenants
    )

    # This is zero, and subsequent checks are expect_activated - 1, because this counter does not
    # count how may tenants are Active, it counts how many have finished warmup.  The first tenant
    # that reached Active is still stuck in its local size calculation, and has therefore not finished warmup.
    assert pageserver_http.get_metric_value("pageserver_tenant_startup_complete_total") == 0

    # If a client accesses one of the blocked tenants, it should skip waiting for warmup and
    # go active as fast as it can.
    stuck_tenant_id = list(
        [(tid, s) for (tid, s) in get_tenant_states().items() if s == "Attaching"]
    )[0][0]

    endpoint = env.endpoints.create_start(branch_name="main", tenant_id=stuck_tenant_id)
    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (x INTEGER)",
            "INSERT INTO foo SELECT g FROM generate_series(1, 10) g",
        ]
    )
    endpoint.stop()

    # That one that we successfully accessed is now Active
    expect_activated += 1
    assert pageserver_http.tenant_status(tenant_id=stuck_tenant_id)["state"]["slug"] == "Active"
    assert (
        pageserver_http.get_metric_value("pageserver_tenant_startup_complete_total")
        == expect_activated - 1
    )

    # The ones we didn't touch are still in Attaching
    assert (
        len([s for s in get_tenant_states().values() if s == "Attaching"])
        == n_tenants - expect_activated
    )

    # Timeline creation operations also wake up Attaching tenants
    stuck_tenant_id = list(
        [(tid, s) for (tid, s) in get_tenant_states().items() if s == "Attaching"]
    )[0][0]
    pageserver_http.timeline_create(env.pg_version, stuck_tenant_id, TimelineId.generate())
    expect_activated += 1
    assert pageserver_http.tenant_status(tenant_id=stuck_tenant_id)["state"]["slug"] == "Active"
    assert (
        len([s for s in get_tenant_states().values() if s == "Attaching"])
        == n_tenants - expect_activated
    )

    assert (
        pageserver_http.get_metric_value("pageserver_tenant_startup_complete_total")
        == expect_activated - 1
    )

    # When we unblock logical size calculation, all tenants should proceed to active state via
    # the warmup route.
    pageserver_http.configure_failpoints(("timeline-calculate-logical-size-pause", "off"))

    def all_active():
        assert all(s == "Active" for s in get_tenant_states().values())

    wait_until(10, 1, all_active)

    # Final control check: restarting with no failpoints at all results in all tenants coming active
    # without being prompted by client I/O
    env.pageserver.stop()
    env.pageserver.start()
    wait_until(10, 1, all_active)

    assert (
        pageserver_http.get_metric_value("pageserver_tenant_startup_scheduled_total") == n_tenants
    )
    assert pageserver_http.get_metric_value("pageserver_tenant_startup_complete_total") == n_tenants

    # Check that tenant deletion/detach proactively wakes tenants: this is done separately to the main
    # body of the test because it will disrupt tenant counts
    env.pageserver.stop()
    env.pageserver.start(
        extra_env_vars={"FAILPOINTS": "timeline-calculate-logical-size-pause=pause"}
    )

    wait_until(10, 1, at_least_one_active)

    detach_tenant_id = list(
        [(tid, s) for (tid, s) in get_tenant_states().items() if s == "Attaching"]
    )[0][0]
    delete_tenant_id = list(
        [(tid, s) for (tid, s) in get_tenant_states().items() if s == "Attaching"]
    )[1][0]

    # Detaching a stuck tenant should proceed promptly
    # (reproducer for https://github.com/neondatabase/neon/pull/6430)
    env.pageserver.http_client().tenant_detach(detach_tenant_id, timeout_secs=10)
    tenant_ids.remove(detach_tenant_id)
    # FIXME: currently the mechanism for cancelling attach is to set state to broken, which is reported spuriously at error level
    env.pageserver.allowed_errors.append(
        ".*attach failed, setting tenant state to Broken: Shut down while Attaching"
    )

    # Deleting a stuck tenant should prompt it to go active
    with concurrent.futures.ThreadPoolExecutor() as executor:
        log.info("Starting background delete")

        def delete_tenant():
            env.pageserver.http_client().tenant_delete(delete_tenant_id)

        background_delete = executor.submit(delete_tenant)

        # Deletion itself won't complete due to our failpoint: Tenant::shutdown can't complete while calculating
        # logical size is paused in a failpoint.  So instead we will use a log observation to check that
        # on-demand activation was triggered by the tenant deletion
        log_match = f".*attach{{tenant_id={delete_tenant_id} shard_id=0000 gen=[0-9a-f]+}}: Activating tenant \\(on-demand\\).*"

        def activated_on_demand():
            assert env.pageserver.log_contains(log_match) is not None

        log.info(f"Waiting for activation message '{log_match}'")
        try:
            wait_until(10, 1, activated_on_demand)
        finally:
            log.info("Clearing failpoint")
            pageserver_http.configure_failpoints(("timeline-calculate-logical-size-pause", "off"))

        # Deletion should complete successfully now that failpoint is unblocked
        log.info("Joining background delete")
        background_delete.result(timeout=10)

        # Poll for deletion to complete
        wait_tenant_status_404(pageserver_http, tenant_id=delete_tenant_id, iterations=40)
        tenant_ids.remove(delete_tenant_id)

    # Check that all the stuck tenants proceed to active (apart from the one that deletes, and the one
    # we detached)
    wait_until(10, 1, all_active)
    assert len(get_tenant_states()) == n_tenants - 2


def test_timeline_logical_size_task_priority(neon_env_builder: NeonEnvBuilder):
    """
    /v1/tenant/:tenant_shard_id/timeline and /v1/tenant/:tenant_shard_id
    should not bump the priority of the initial logical size computation
    background task, unless the force-await-initial-logical-size query param
    is set to true.

    This test verifies the invariant stated above. A couple of tricks are involved:
    1. Detach the tenant and re-attach it after the page server is restarted. This circumvents
    the warm-up which forces the initial logical size calculation.
    2. A fail point (initial-size-calculation-permit-pause) is used to block the initial
    computation of the logical size until forced.
    3. A fail point (walreceiver-after-ingest) is used to pause the walreceiver since
    otherwise it would force the logical size computation.
    """
    env = neon_env_builder.init_start()
    client = env.pageserver.http_client()

    tenant_id = env.initial_tenant
    timeline_id = env.initial_timeline

    # load in some data
    endpoint = env.endpoints.create_start("main", tenant_id=tenant_id)
    endpoint.safe_psql_many(
        [
            "CREATE TABLE foo (x INTEGER)",
            "INSERT INTO foo SELECT g FROM generate_series(1, 10000) g",
        ]
    )
    wait_for_last_flush_lsn(env, endpoint, tenant_id, timeline_id)

    # restart with failpoint inside initial size calculation task
    log.info(f"Detaching tenant {tenant_id} and stopping pageserver...")

    endpoint.stop()
    env.pageserver.tenant_detach(tenant_id)
    env.pageserver.stop()
    env.pageserver.start(
        extra_env_vars={
            "FAILPOINTS": "initial-size-calculation-permit-pause=pause;walreceiver-after-ingest=pause"
        }
    )

    log.info(f"Re-attaching tenant {tenant_id}...")
    env.pageserver.tenant_attach(tenant_id)

    # kick off initial size calculation task (the response we get here is the estimated size)
    def assert_initial_logical_size_not_prioritised():
        details = client.timeline_detail(tenant_id, timeline_id)
        assert details["current_logical_size_is_accurate"] is False

    assert_initial_logical_size_not_prioritised()

    # ensure that's actually the case
    time.sleep(2)
    assert_initial_logical_size_not_prioritised()

    details = client.timeline_detail(tenant_id, timeline_id, force_await_initial_logical_size=True)
    assert details["current_logical_size_is_accurate"] is True

    client.configure_failpoints(
        [("initial-size-calculation-permit-pause", "off"), ("walreceiver-after-ingest", "off")]
    )
