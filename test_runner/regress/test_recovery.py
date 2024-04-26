import time
from contextlib import closing

from fixtures.log_helper import log
from fixtures.neon_fixtures import NeonEnvBuilder


#
# Test pageserver recovery after crash
#
def test_pageserver_recovery(neon_env_builder: NeonEnvBuilder):
    # Override default checkpointer settings to run it more often
    neon_env_builder.pageserver_config_override = "tenant_config={checkpoint_distance = 1048576}"

    env = neon_env_builder.init_start()
    env.pageserver.is_testing_enabled_or_skip()

    # Create a branch for us
    env.neon_cli.create_branch("test_pageserver_recovery", "main")

    endpoint = env.endpoints.create_start("test_pageserver_recovery")

    with closing(endpoint.connect()) as conn:
        with conn.cursor() as cur:
            with env.pageserver.http_client() as pageserver_http:
                # Create and initialize test table
                cur.execute("CREATE TABLE foo(x bigint)")
                cur.execute("INSERT INTO foo VALUES (generate_series(1,100000))")

                # Sleep for some time to let checkpoint create image layers
                time.sleep(2)

                # Configure failpoints
                pageserver_http.configure_failpoints(
                    [
                        ("flush-frozen-pausable", "sleep(2000)"),
                        ("flush-frozen-exit", "exit"),
                    ]
                )

                # Do some updates until pageserver is crashed
                try:
                    while True:
                        cur.execute("update foo set x=x+1")
                except Exception as err:
                    log.info(f"Expected server crash {err}")

    log.info("Wait before server restart")
    env.pageserver.stop()
    env.pageserver.start()

    with closing(endpoint.connect()) as conn:
        with conn.cursor() as cur:
            cur.execute("select count(*) from foo")
            assert cur.fetchone() == (100000,)
