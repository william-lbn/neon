//! Main entry point for the Page Server executable.

use std::env::{var, VarError};
use std::sync::Arc;
use std::time::Duration;
use std::{env, ops::ControlFlow, str::FromStr};

use anyhow::{anyhow, Context};
use camino::Utf8Path;
use clap::{Arg, ArgAction, Command};

use metrics::launch_timestamp::{set_launch_timestamp_metric, LaunchTimestamp};
use pageserver::control_plane_client::ControlPlaneClient;
use pageserver::disk_usage_eviction_task::{self, launch_disk_usage_global_eviction_task};
use pageserver::metrics::{STARTUP_DURATION, STARTUP_IS_LOADING};
use pageserver::task_mgr::WALRECEIVER_RUNTIME;
use pageserver::tenant::{secondary, TenantSharedResources};
use remote_storage::GenericRemoteStorage;
use tokio::time::Instant;
use tracing::*;

use metrics::set_build_info_metric;
use pageserver::{
    config::{defaults::*, PageServerConf},
    context::{DownloadBehavior, RequestContext},
    deletion_queue::DeletionQueue,
    http, page_cache, page_service, task_mgr,
    task_mgr::TaskKind,
    task_mgr::{BACKGROUND_RUNTIME, COMPUTE_REQUEST_RUNTIME, MGMT_REQUEST_RUNTIME},
    tenant::mgr,
    virtual_file,
};
use postgres_backend::AuthType;
use utils::failpoint_support;
use utils::logging::TracingErrorLayerEnablement;
use utils::{
    auth::{JwtAuth, SwappableJwtAuth},
    logging, project_build_tag, project_git_version,
    sentry_init::init_sentry,
    tcp_listener,
};

project_git_version!(GIT_VERSION);
project_build_tag!(BUILD_TAG);

const PID_FILE_NAME: &str = "pageserver.pid";

const FEATURES: &[&str] = &[
    #[cfg(feature = "testing")]
    "testing",
];

fn version() -> String {
    format!(
        "{GIT_VERSION} failpoints: {}, features: {:?}",
        fail::has_failpoints(),
        FEATURES,
    )
}

fn main() -> anyhow::Result<()> {
    let launch_ts = Box::leak(Box::new(LaunchTimestamp::generate()));

    let arg_matches = cli().get_matches();

    if arg_matches.get_flag("enabled-features") {
        println!("{{\"features\": {FEATURES:?} }}");
        return Ok(());
    }

    let workdir = arg_matches
        .get_one::<String>("workdir")
        .map(Utf8Path::new)
        .unwrap_or_else(|| Utf8Path::new(".neon"));
    let workdir = workdir
        .canonicalize_utf8()
        .with_context(|| format!("Error opening workdir '{workdir}'"))?;

    let cfg_file_path = workdir.join("pageserver.toml");

    // Set CWD to workdir for non-daemon modes
    env::set_current_dir(&workdir)
        .with_context(|| format!("Failed to set application's current dir to '{workdir}'"))?;

    let conf = match initialize_config(&cfg_file_path, arg_matches, &workdir)? {
        ControlFlow::Continue(conf) => conf,
        ControlFlow::Break(()) => {
            info!("Pageserver config init successful");
            return Ok(());
        }
    };

    // Initialize logging.
    //
    // It must be initialized before the custom panic hook is installed below.
    //
    // Regarding tracing_error enablement: at this time, we only use the
    // tracing_error crate to debug_assert that log spans contain tenant and timeline ids.
    // See `debug_assert_current_span_has_tenant_and_timeline_id` in the timeline module
    let tracing_error_layer_enablement = if cfg!(debug_assertions) {
        TracingErrorLayerEnablement::EnableWithRustLogFilter
    } else {
        TracingErrorLayerEnablement::Disabled
    };
    logging::init(
        conf.log_format,
        tracing_error_layer_enablement,
        logging::Output::Stdout,
    )?;

    // mind the order required here: 1. logging, 2. panic_hook, 3. sentry.
    // disarming this hook on pageserver, because we never tear down tracing.
    logging::replace_panic_hook_with_tracing_panic_hook().forget();

    // initialize sentry if SENTRY_DSN is provided
    let _sentry_guard = init_sentry(
        Some(GIT_VERSION.into()),
        &[("node_id", &conf.id.to_string())],
    );

    let tenants_path = conf.tenants_path();
    if !tenants_path.exists() {
        utils::crashsafe::create_dir_all(conf.tenants_path())
            .with_context(|| format!("Failed to create tenants root dir at '{tenants_path}'"))?;
    }

    // Initialize up failpoints support
    let scenario = failpoint_support::init();

    // Basic initialization of things that don't change after startup
    virtual_file::init(conf.max_file_descriptors, conf.virtual_file_io_engine);
    page_cache::init(conf.page_cache_size);

    start_pageserver(launch_ts, conf).context("Failed to start pageserver")?;

    scenario.teardown();
    Ok(())
}

fn initialize_config(
    cfg_file_path: &Utf8Path,
    arg_matches: clap::ArgMatches,
    workdir: &Utf8Path,
) -> anyhow::Result<ControlFlow<(), &'static PageServerConf>> {
    let init = arg_matches.get_flag("init");
    let update_config = init || arg_matches.get_flag("update-config");

    let (mut toml, config_file_exists) = if cfg_file_path.is_file() {
        if init {
            anyhow::bail!(
                "Config file '{cfg_file_path}' already exists, cannot init it, use --update-config to update it",
            );
        }
        // Supplement the CLI arguments with the config file
        let cfg_file_contents = std::fs::read_to_string(cfg_file_path)
            .with_context(|| format!("Failed to read pageserver config at '{cfg_file_path}'"))?;
        (
            cfg_file_contents
                .parse::<toml_edit::Document>()
                .with_context(|| {
                    format!("Failed to parse '{cfg_file_path}' as pageserver config")
                })?,
            true,
        )
    } else if cfg_file_path.exists() {
        anyhow::bail!("Config file '{cfg_file_path}' exists but is not a regular file");
    } else {
        // We're initializing the tenant, so there's no config file yet
        (
            DEFAULT_CONFIG_FILE
                .parse::<toml_edit::Document>()
                .context("could not parse built-in config file")?,
            false,
        )
    };

    if let Some(values) = arg_matches.get_many::<String>("config-override") {
        for option_line in values {
            let doc = toml_edit::Document::from_str(option_line).with_context(|| {
                format!("Option '{option_line}' could not be parsed as a toml document")
            })?;

            for (key, item) in doc.iter() {
                if config_file_exists && update_config && key == "id" && toml.contains_key(key) {
                    anyhow::bail!("Pageserver config file exists at '{cfg_file_path}' and has node id already, it cannot be overridden");
                }
                toml.insert(key, item.clone());
            }
        }
    }

    debug!("Resulting toml: {toml}");
    let conf = PageServerConf::parse_and_validate(&toml, workdir)
        .context("Failed to parse pageserver configuration")?;

    if update_config {
        info!("Writing pageserver config to '{cfg_file_path}'");

        std::fs::write(cfg_file_path, toml.to_string())
            .with_context(|| format!("Failed to write pageserver config to '{cfg_file_path}'"))?;
        info!("Config successfully written to '{cfg_file_path}'")
    }

    Ok(if init {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(Box::leak(Box::new(conf)))
    })
}

struct WaitForPhaseResult<F: std::future::Future + Unpin> {
    timeout_remaining: Duration,
    skipped: Option<F>,
}

/// During startup, we apply a timeout to our waits for readiness, to avoid
/// stalling the whole service if one Tenant experiences some problem.  Each
/// phase may consume some of the timeout: this function returns the updated
/// timeout for use in the next call.
async fn wait_for_phase<F>(phase: &str, mut fut: F, timeout: Duration) -> WaitForPhaseResult<F>
where
    F: std::future::Future + Unpin,
{
    let initial_t = Instant::now();
    let skipped = match tokio::time::timeout(timeout, &mut fut).await {
        Ok(_) => None,
        Err(_) => {
            tracing::info!(
                timeout_millis = timeout.as_millis(),
                %phase,
                "Startup phase timed out, proceeding anyway"
            );
            Some(fut)
        }
    };

    WaitForPhaseResult {
        timeout_remaining: timeout
            .checked_sub(Instant::now().duration_since(initial_t))
            .unwrap_or(Duration::ZERO),
        skipped,
    }
}

fn startup_checkpoint(started_at: Instant, phase: &str, human_phase: &str) {
    let elapsed = started_at.elapsed();
    let secs = elapsed.as_secs_f64();
    STARTUP_DURATION.with_label_values(&[phase]).set(secs);

    info!(
        elapsed_ms = elapsed.as_millis(),
        "{human_phase} ({secs:.3}s since start)"
    )
}

fn start_pageserver(
    launch_ts: &'static LaunchTimestamp,
    conf: &'static PageServerConf,
) -> anyhow::Result<()> {
    // Monotonic time for later calculating startup duration
    let started_startup_at = Instant::now();

    // Print version and launch timestamp to the log,
    // and expose them as prometheus metrics.
    // A changed version string indicates changed software.
    // A changed launch timestamp indicates a pageserver restart.
    info!(
        "version: {} launch_timestamp: {} build_tag: {}",
        version(),
        launch_ts.to_string(),
        BUILD_TAG,
    );
    set_build_info_metric(GIT_VERSION, BUILD_TAG);
    set_launch_timestamp_metric(launch_ts);
    #[cfg(target_os = "linux")]
    metrics::register_internal(Box::new(metrics::more_process_metrics::Collector::new())).unwrap();
    metrics::register_internal(Box::new(
        pageserver::metrics::tokio_epoll_uring::Collector::new(),
    ))
    .unwrap();
    pageserver::preinitialize_metrics();

    // If any failpoints were set from FAILPOINTS environment variable,
    // print them to the log for debugging purposes
    let failpoints = fail::list();
    if !failpoints.is_empty() {
        info!(
            "started with failpoints: {}",
            failpoints
                .iter()
                .map(|(name, actions)| format!("{name}={actions}"))
                .collect::<Vec<String>>()
                .join(";")
        )
    }

    // Create and lock PID file. This ensures that there cannot be more than one
    // pageserver process running at the same time.
    let lock_file_path = conf.workdir.join(PID_FILE_NAME);
    let lock_file =
        utils::pid_file::claim_for_current_process(&lock_file_path).context("claim pid file")?;
    info!("Claimed pid file at {lock_file_path:?}");

    // Ensure that the lock file is held even if the main thread of the process panics.
    // We need to release the lock file only when the process exits.
    std::mem::forget(lock_file);

    // Bind the HTTP and libpq ports early, so that if they are in use by some other
    // process, we error out early.
    let http_addr = &conf.listen_http_addr;
    info!("Starting pageserver http handler on {http_addr}");
    let http_listener = tcp_listener::bind(http_addr)?;

    let pg_addr = &conf.listen_pg_addr;
    info!("Starting pageserver pg protocol handler on {pg_addr}");
    let pageserver_listener = tcp_listener::bind(pg_addr)?;

    // Launch broker client
    // The storage_broker::connect call needs to happen inside a tokio runtime thread.
    let broker_client = WALRECEIVER_RUNTIME
        .block_on(async {
            // Note: we do not attempt connecting here (but validate endpoints sanity).
            storage_broker::connect(conf.broker_endpoint.clone(), conf.broker_keepalive_interval)
        })
        .with_context(|| {
            format!(
                "create broker client for uri={:?} keepalive_interval={:?}",
                &conf.broker_endpoint, conf.broker_keepalive_interval,
            )
        })?;

    // Initialize authentication for incoming connections
    let http_auth;
    let pg_auth;
    if conf.http_auth_type == AuthType::NeonJWT || conf.pg_auth_type == AuthType::NeonJWT {
        // unwrap is ok because check is performed when creating config, so path is set and exists
        let key_path = conf.auth_validation_public_key_path.as_ref().unwrap();
        info!("Loading public key(s) for verifying JWT tokens from {key_path:?}");

        let jwt_auth = JwtAuth::from_key_path(key_path)?;
        let auth: Arc<SwappableJwtAuth> = Arc::new(SwappableJwtAuth::new(jwt_auth));

        http_auth = match &conf.http_auth_type {
            AuthType::Trust => None,
            AuthType::NeonJWT => Some(auth.clone()),
        };
        pg_auth = match &conf.pg_auth_type {
            AuthType::Trust => None,
            AuthType::NeonJWT => Some(auth),
        };
    } else {
        http_auth = None;
        pg_auth = None;
    }
    info!("Using auth for http API: {:#?}", conf.http_auth_type);
    info!("Using auth for pg connections: {:#?}", conf.pg_auth_type);

    match var("NEON_AUTH_TOKEN") {
        Ok(v) => {
            info!("Loaded JWT token for authentication with Safekeeper");
            pageserver::config::SAFEKEEPER_AUTH_TOKEN
                .set(Arc::new(v))
                .map_err(|_| anyhow!("Could not initialize SAFEKEEPER_AUTH_TOKEN"))?;
        }
        Err(VarError::NotPresent) => {
            info!("No JWT token for authentication with Safekeeper detected");
        }
        Err(e) => {
            return Err(e).with_context(|| {
                "Failed to either load to detect non-present NEON_AUTH_TOKEN environment variable"
            })
        }
    };

    // Top-level cancellation token for the process
    let shutdown_pageserver = tokio_util::sync::CancellationToken::new();

    // Set up remote storage client
    let remote_storage = create_remote_storage_client(conf)?;

    // Set up deletion queue
    let (deletion_queue, deletion_workers) = DeletionQueue::new(
        remote_storage.clone(),
        ControlPlaneClient::new(conf, &shutdown_pageserver),
        conf,
    );
    if let Some(deletion_workers) = deletion_workers {
        deletion_workers.spawn_with(BACKGROUND_RUNTIME.handle());
    }

    // Up to this point no significant I/O has been done: this should have been fast.  Record
    // duration prior to starting I/O intensive phase of startup.
    startup_checkpoint(started_startup_at, "initial", "Starting loading tenants");
    STARTUP_IS_LOADING.set(1);

    // Startup staging or optimizing:
    //
    // We want to minimize downtime for `page_service` connections, and trying not to overload
    // BACKGROUND_RUNTIME by doing initial compactions and initial logical sizes at the same time.
    //
    // init_done_rx will notify when all initial load operations have completed.
    //
    // background_jobs_can_start (same name used to hold off background jobs from starting at
    // consumer side) will be dropped once we can start the background jobs. Currently it is behind
    // completing all initial logical size calculations (init_logical_size_done_rx) and a timeout
    // (background_task_maximum_delay).
    let (init_remote_done_tx, init_remote_done_rx) = utils::completion::channel();
    let (init_done_tx, init_done_rx) = utils::completion::channel();

    let (background_jobs_can_start, background_jobs_barrier) = utils::completion::channel();

    let order = pageserver::InitializationOrder {
        initial_tenant_load_remote: Some(init_done_tx),
        initial_tenant_load: Some(init_remote_done_tx),
        background_jobs_can_start: background_jobs_barrier.clone(),
    };

    // Scan the local 'tenants/' directory and start loading the tenants
    let deletion_queue_client = deletion_queue.new_client();
    let tenant_manager = BACKGROUND_RUNTIME.block_on(mgr::init_tenant_mgr(
        conf,
        TenantSharedResources {
            broker_client: broker_client.clone(),
            remote_storage: remote_storage.clone(),
            deletion_queue_client,
        },
        order,
        shutdown_pageserver.clone(),
    ))?;
    let tenant_manager = Arc::new(tenant_manager);

    BACKGROUND_RUNTIME.spawn({
        let shutdown_pageserver = shutdown_pageserver.clone();
        let drive_init = async move {
            // NOTE: unlike many futures in pageserver, this one is cancellation-safe
            let guard = scopeguard::guard_on_success((), |_| {
                tracing::info!("Cancelled before initial load completed")
            });

            let timeout = conf.background_task_maximum_delay;

            let init_remote_done = std::pin::pin!(async {
                init_remote_done_rx.wait().await;
                startup_checkpoint(
                    started_startup_at,
                    "initial_tenant_load_remote",
                    "Remote part of initial load completed",
                );
            });

            let WaitForPhaseResult {
                timeout_remaining: timeout,
                skipped: init_remote_skipped,
            } = wait_for_phase("initial_tenant_load_remote", init_remote_done, timeout).await;

            let init_load_done = std::pin::pin!(async {
                init_done_rx.wait().await;
                startup_checkpoint(
                    started_startup_at,
                    "initial_tenant_load",
                    "Initial load completed",
                );
                STARTUP_IS_LOADING.set(0);
            });

            let WaitForPhaseResult {
                timeout_remaining: _timeout,
                skipped: init_load_skipped,
            } = wait_for_phase("initial_tenant_load", init_load_done, timeout).await;

            // initial logical sizes can now start, as they were waiting on init_done_rx.

            scopeguard::ScopeGuard::into_inner(guard);

            // allow background jobs to start: we either completed prior stages, or they reached timeout
            // and were skipped.  It is important that we do not let them block background jobs indefinitely,
            // because things like consumption metrics for billing are blocked by this barrier.
            drop(background_jobs_can_start);
            startup_checkpoint(
                started_startup_at,
                "background_jobs_can_start",
                "Starting background jobs",
            );

            // We are done. If we skipped any phases due to timeout, run them to completion here so that
            // they will eventually update their startup_checkpoint, and so that we do not declare the
            // 'complete' stage until all the other stages are really done.
            let guard = scopeguard::guard_on_success((), |_| {
                tracing::info!("Cancelled before waiting for skipped phases done")
            });
            if let Some(f) = init_remote_skipped {
                f.await;
            }
            if let Some(f) = init_load_skipped {
                f.await;
            }
            scopeguard::ScopeGuard::into_inner(guard);

            startup_checkpoint(started_startup_at, "complete", "Startup complete");
        };

        async move {
            let mut drive_init = std::pin::pin!(drive_init);
            // just race these tasks
            tokio::select! {
                _ = shutdown_pageserver.cancelled() => {},
                _ = &mut drive_init => {},
            }
        }
    });

    let secondary_controller = if let Some(remote_storage) = &remote_storage {
        secondary::spawn_tasks(
            tenant_manager.clone(),
            remote_storage.clone(),
            background_jobs_barrier.clone(),
            shutdown_pageserver.clone(),
        )
    } else {
        secondary::null_controller()
    };

    // shared state between the disk-usage backed eviction background task and the http endpoint
    // that allows triggering disk-usage based eviction manually. note that the http endpoint
    // is still accessible even if background task is not configured as long as remote storage has
    // been configured.
    let disk_usage_eviction_state: Arc<disk_usage_eviction_task::State> = Arc::default();

    if let Some(remote_storage) = &remote_storage {
        launch_disk_usage_global_eviction_task(
            conf,
            remote_storage.clone(),
            disk_usage_eviction_state.clone(),
            tenant_manager.clone(),
            background_jobs_barrier.clone(),
        )?;
    }

    // Start up the service to handle HTTP mgmt API request. We created the
    // listener earlier already.
    {
        let _rt_guard = MGMT_REQUEST_RUNTIME.enter();

        let router_state = Arc::new(
            http::routes::State::new(
                conf,
                tenant_manager,
                http_auth.clone(),
                remote_storage.clone(),
                broker_client.clone(),
                disk_usage_eviction_state,
                deletion_queue.new_client(),
                secondary_controller,
            )
            .context("Failed to initialize router state")?,
        );
        let router = http::make_router(router_state, launch_ts, http_auth.clone())?
            .build()
            .map_err(|err| anyhow!(err))?;
        let service = utils::http::RouterService::new(router).unwrap();
        let server = hyper::Server::from_tcp(http_listener)?
            .serve(service)
            .with_graceful_shutdown(task_mgr::shutdown_watcher());

        task_mgr::spawn(
            MGMT_REQUEST_RUNTIME.handle(),
            TaskKind::HttpEndpointListener,
            None,
            None,
            "http endpoint listener",
            true,
            async {
                server.await?;
                Ok(())
            },
        );
    }

    if let Some(metric_collection_endpoint) = &conf.metric_collection_endpoint {
        let metrics_ctx = RequestContext::todo_child(
            TaskKind::MetricsCollection,
            // This task itself shouldn't download anything.
            // The actual size calculation does need downloads, and
            // creates a child context with the right DownloadBehavior.
            DownloadBehavior::Error,
        );

        let local_disk_storage = conf.workdir.join("last_consumption_metrics.json");

        task_mgr::spawn(
            crate::BACKGROUND_RUNTIME.handle(),
            TaskKind::MetricsCollection,
            None,
            None,
            "consumption metrics collection",
            true,
            async move {
                // first wait until background jobs are cleared to launch.
                //
                // this is because we only process active tenants and timelines, and the
                // Timeline::get_current_logical_size will spawn the logical size calculation,
                // which will not be rate-limited.
                let cancel = task_mgr::shutdown_token();

                tokio::select! {
                    _ = cancel.cancelled() => { return Ok(()); },
                    _ = background_jobs_barrier.wait() => {}
                };

                pageserver::consumption_metrics::collect_metrics(
                    metric_collection_endpoint,
                    conf.metric_collection_interval,
                    conf.cached_metric_collection_interval,
                    conf.synthetic_size_calculation_interval,
                    conf.id,
                    local_disk_storage,
                    cancel,
                    metrics_ctx,
                )
                .instrument(info_span!("metrics_collection"))
                .await?;
                Ok(())
            },
        );
    }

    // Spawn a task to listen for libpq connections. It will spawn further tasks
    // for each connection. We created the listener earlier already.
    {
        let libpq_ctx = RequestContext::todo_child(
            TaskKind::LibpqEndpointListener,
            // listener task shouldn't need to download anything. (We will
            // create a separate sub-contexts for each connection, with their
            // own download behavior. This context is used only to listen and
            // accept connections.)
            DownloadBehavior::Error,
        );
        task_mgr::spawn(
            COMPUTE_REQUEST_RUNTIME.handle(),
            TaskKind::LibpqEndpointListener,
            None,
            None,
            "libpq endpoint listener",
            true,
            async move {
                page_service::libpq_listener_main(
                    conf,
                    broker_client,
                    pg_auth,
                    pageserver_listener,
                    conf.pg_auth_type,
                    libpq_ctx,
                    task_mgr::shutdown_token(),
                )
                .await
            },
        );
    }

    let mut shutdown_pageserver = Some(shutdown_pageserver.drop_guard());

    // All started up! Now just sit and wait for shutdown signal.
    {
        use signal_hook::consts::*;
        let signal_handler = BACKGROUND_RUNTIME.spawn_blocking(move || {
            let mut signals =
                signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGQUIT]).unwrap();
            return signals
                .forever()
                .next()
                .expect("forever() never returns None unless explicitly closed");
        });
        let signal = BACKGROUND_RUNTIME
            .block_on(signal_handler)
            .expect("join error");
        match signal {
            SIGQUIT => {
                info!("Got signal {signal}. Terminating in immediate shutdown mode",);
                std::process::exit(111);
            }
            SIGINT | SIGTERM => {
                info!("Got signal {signal}. Terminating gracefully in fast shutdown mode",);

                // This cancels the `shutdown_pageserver` cancellation tree.
                // Right now that tree doesn't reach very far, and `task_mgr` is used instead.
                // The plan is to change that over time.
                shutdown_pageserver.take();
                let bg_remote_storage = remote_storage.clone();
                let bg_deletion_queue = deletion_queue.clone();
                BACKGROUND_RUNTIME.block_on(pageserver::shutdown_pageserver(
                    bg_remote_storage.map(|_| bg_deletion_queue),
                    0,
                ));
                unreachable!()
            }
            _ => unreachable!(),
        }
    }
}

fn create_remote_storage_client(
    conf: &'static PageServerConf,
) -> anyhow::Result<Option<GenericRemoteStorage>> {
    let config = if let Some(config) = &conf.remote_storage_config {
        config
    } else {
        tracing::warn!("no remote storage configured, this is a deprecated configuration");
        return Ok(None);
    };

    // Create the client
    let mut remote_storage = GenericRemoteStorage::from_config(config)?;

    // If `test_remote_failures` is non-zero, wrap the client with a
    // wrapper that simulates failures.
    if conf.test_remote_failures > 0 {
        if !cfg!(feature = "testing") {
            anyhow::bail!("test_remote_failures option is not available because pageserver was compiled without the 'testing' feature");
        }
        info!(
            "Simulating remote failures for first {} attempts of each op",
            conf.test_remote_failures
        );
        remote_storage =
            GenericRemoteStorage::unreliable_wrapper(remote_storage, conf.test_remote_failures);
    }

    Ok(Some(remote_storage))
}

fn cli() -> Command {
    Command::new("Neon page server")
        .about("Materializes WAL stream to pages and serves them to the postgres")
        .version(version())
        .arg(
            Arg::new("init")
                .long("init")
                .action(ArgAction::SetTrue)
                .help("Initialize pageserver with all given config overrides"),
        )
        .arg(
            Arg::new("workdir")
                .short('D')
                .long("workdir")
                .help("Working directory for the pageserver"),
        )
        // See `settings.md` for more details on the extra configuration patameters pageserver can process
        .arg(
            Arg::new("config-override")
                .short('c')
                .num_args(1)
                .action(ArgAction::Append)
                .help("Additional configuration overrides of the ones from the toml config file (or new ones to add there). \
                Any option has to be a valid toml document, example: `-c=\"foo='hey'\"` `-c=\"foo={value=1}\"`"),
        )
        .arg(
            Arg::new("update-config")
                .long("update-config")
                .action(ArgAction::SetTrue)
                .help("Update the config file when started"),
        )
        .arg(
            Arg::new("enabled-features")
                .long("enabled-features")
                .action(ArgAction::SetTrue)
                .help("Show enabled compile time features"),
        )
}

#[test]
fn verify_cli() {
    cli().debug_assert();
}
