//! This module contains functions to serve per-tenant background processes,
//! such as compaction and GC

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::context::{DownloadBehavior, RequestContext};
use crate::metrics::TENANT_TASK_EVENTS;
use crate::task_mgr;
use crate::task_mgr::{TaskKind, BACKGROUND_RUNTIME};
use crate::tenant::throttle::Stats;
use crate::tenant::timeline::CompactionError;
use crate::tenant::{Tenant, TenantState};
use tokio_util::sync::CancellationToken;
use tracing::*;
use utils::{backoff, completion};

static CONCURRENT_BACKGROUND_TASKS: once_cell::sync::Lazy<tokio::sync::Semaphore> =
    once_cell::sync::Lazy::new(|| {
        let total_threads = *task_mgr::BACKGROUND_RUNTIME_WORKER_THREADS;
        let permits = usize::max(
            1,
            // while a lot of the work is done on spawn_blocking, we still do
            // repartitioning in the async context. this should give leave us some workers
            // unblocked to be blocked on other work, hopefully easing any outside visible
            // effects of restarts.
            //
            // 6/8 is a guess; previously we ran with unlimited 8 and more from
            // spawn_blocking.
            (total_threads * 3).checked_div(4).unwrap_or(0),
        );
        assert_ne!(permits, 0, "we will not be adding in permits later");
        assert!(
            permits < total_threads,
            "need threads avail for shorter work"
        );
        tokio::sync::Semaphore::new(permits)
    });

#[derive(Debug, PartialEq, Eq, Clone, Copy, strum_macros::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum BackgroundLoopKind {
    Compaction,
    Gc,
    Eviction,
    ConsumptionMetricsCollectMetrics,
    ConsumptionMetricsSyntheticSizeWorker,
    InitialLogicalSizeCalculation,
    HeatmapUpload,
    SecondaryDownload,
}

impl BackgroundLoopKind {
    fn as_static_str(&self) -> &'static str {
        let s: &'static str = self.into();
        s
    }
}

/// Cancellation safe.
pub(crate) async fn concurrent_background_tasks_rate_limit_permit(
    loop_kind: BackgroundLoopKind,
    _ctx: &RequestContext,
) -> impl Drop {
    let _guard = crate::metrics::BACKGROUND_LOOP_SEMAPHORE_WAIT_GAUGE
        .with_label_values(&[loop_kind.as_static_str()])
        .guard();

    pausable_failpoint!(
        "initial-size-calculation-permit-pause",
        loop_kind == BackgroundLoopKind::InitialLogicalSizeCalculation
    );

    match CONCURRENT_BACKGROUND_TASKS.acquire().await {
        Ok(permit) => permit,
        Err(_closed) => unreachable!("we never close the semaphore"),
    }
}

/// Start per tenant background loops: compaction and gc.
pub fn start_background_loops(
    tenant: &Arc<Tenant>,
    background_jobs_can_start: Option<&completion::Barrier>,
) {
    let tenant_shard_id = tenant.tenant_shard_id;
    task_mgr::spawn(
        BACKGROUND_RUNTIME.handle(),
        TaskKind::Compaction,
        Some(tenant_shard_id),
        None,
        &format!("compactor for tenant {tenant_shard_id}"),
        false,
        {
            let tenant = Arc::clone(tenant);
            let background_jobs_can_start = background_jobs_can_start.cloned();
            async move {
                let cancel = task_mgr::shutdown_token();
                tokio::select! {
                    _ = cancel.cancelled() => { return Ok(()) },
                    _ = completion::Barrier::maybe_wait(background_jobs_can_start) => {}
                };
                compaction_loop(tenant, cancel)
                    .instrument(info_span!("compaction_loop", tenant_id = %tenant_shard_id.tenant_id, shard_id = %tenant_shard_id.shard_slug()))
                    .await;
                Ok(())
            }
        },
    );
    task_mgr::spawn(
        BACKGROUND_RUNTIME.handle(),
        TaskKind::GarbageCollector,
        Some(tenant_shard_id),
        None,
        &format!("garbage collector for tenant {tenant_shard_id}"),
        false,
        {
            let tenant = Arc::clone(tenant);
            let background_jobs_can_start = background_jobs_can_start.cloned();
            async move {
                let cancel = task_mgr::shutdown_token();
                tokio::select! {
                    _ = cancel.cancelled() => { return Ok(()) },
                    _ = completion::Barrier::maybe_wait(background_jobs_can_start) => {}
                };
                gc_loop(tenant, cancel)
                    .instrument(info_span!("gc_loop", tenant_id = %tenant_shard_id.tenant_id, shard_id = %tenant_shard_id.shard_slug()))
                    .await;
                Ok(())
            }
        },
    );
}

///
/// Compaction task's main loop
///
async fn compaction_loop(tenant: Arc<Tenant>, cancel: CancellationToken) {
    const MAX_BACKOFF_SECS: f64 = 300.0;
    // How many errors we have seen consequtively
    let mut error_run_count = 0;

    let mut last_throttle_flag_reset_at = Instant::now();

    TENANT_TASK_EVENTS.with_label_values(&["start"]).inc();
    async {
        let ctx = RequestContext::todo_child(TaskKind::Compaction, DownloadBehavior::Download);
        let mut first = true;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return;
                },
                tenant_wait_result = wait_for_active_tenant(&tenant) => match tenant_wait_result {
                    ControlFlow::Break(()) => return,
                    ControlFlow::Continue(()) => (),
                },
            }

            let period = tenant.get_compaction_period();

            // TODO: we shouldn't need to await to find tenant and this could be moved outside of
            // loop, #3501. There are also additional "allowed_errors" in tests.
            if first {
                first = false;
                if random_init_delay(period, &cancel).await.is_err() {
                    break;
                }
            }

            let started_at = Instant::now();

            let sleep_duration = if period == Duration::ZERO {
                #[cfg(not(feature = "testing"))]
                info!("automatic compaction is disabled");
                // check again in 10 seconds, in case it's been enabled again.
                Duration::from_secs(10)
            } else {
                // Run compaction
                if let Err(e) = tenant.compaction_iteration(&cancel, &ctx).await {
                    let wait_duration = backoff::exponential_backoff_duration_seconds(
                        error_run_count + 1,
                        1.0,
                        MAX_BACKOFF_SECS,
                    );
                    error_run_count += 1;
                    let wait_duration = Duration::from_secs_f64(wait_duration);
                    log_compaction_error(
                        &e,
                        error_run_count,
                        &wait_duration,
                        cancel.is_cancelled(),
                    );
                    wait_duration
                } else {
                    error_run_count = 0;
                    period
                }
            };

            warn_when_period_overrun(started_at.elapsed(), period, BackgroundLoopKind::Compaction);

            // Perhaps we did no work and the walredo process has been idle for some time:
            // give it a chance to shut down to avoid leaving walredo process running indefinitely.
            if let Some(walredo_mgr) = &tenant.walredo_mgr {
                walredo_mgr.maybe_quiesce(period * 10);
            }

            // TODO: move this (and walredo quiesce) to a separate task that isn't affected by the back-off,
            // so we get some upper bound guarantee on when walredo quiesce / this throttling reporting here happens.
            info_span!(parent: None, "timeline_get_throttle", tenant_id=%tenant.tenant_shard_id, shard_id=%tenant.tenant_shard_id.shard_slug()).in_scope(|| {
                let now = Instant::now();
                let prev = std::mem::replace(&mut last_throttle_flag_reset_at, now);
                let Stats { count_accounted, count_throttled, sum_throttled_usecs } = tenant.timeline_get_throttle.reset_stats();
                if count_throttled == 0 {
                    return;
                }
                let allowed_rps = tenant.timeline_get_throttle.steady_rps();
                let delta = now - prev;
                warn!(
                    n_seconds=%format_args!("{:.3}",
                    delta.as_secs_f64()),
                    count_accounted,
                    count_throttled,
                    sum_throttled_usecs,
                    allowed_rps=%format_args!("{allowed_rps:.0}"),
                    "shard was throttled in the last n_seconds")
            });

            // Sleep
            if tokio::time::timeout(sleep_duration, cancel.cancelled())
                .await
                .is_ok()
            {
                break;
            }
        }
    }
    .await;
    TENANT_TASK_EVENTS.with_label_values(&["stop"]).inc();
}

fn log_compaction_error(
    e: &CompactionError,
    error_run_count: u32,
    sleep_duration: &std::time::Duration,
    task_cancelled: bool,
) {
    use crate::tenant::upload_queue::NotInitialized;
    use crate::tenant::PageReconstructError;
    use CompactionError::*;

    enum LooksLike {
        Info,
        Error,
    }

    let decision = match e {
        ShuttingDown => None,
        _ if task_cancelled => Some(LooksLike::Info),
        Other(e) => {
            let root_cause = e.root_cause();

            let is_stopping = {
                let upload_queue = root_cause
                    .downcast_ref::<NotInitialized>()
                    .is_some_and(|e| e.is_stopping());

                let timeline = root_cause
                    .downcast_ref::<PageReconstructError>()
                    .is_some_and(|e| e.is_stopping());

                upload_queue || timeline
            };

            if is_stopping {
                Some(LooksLike::Info)
            } else {
                Some(LooksLike::Error)
            }
        }
    };

    match decision {
        Some(LooksLike::Info) => info!(
            "Compaction failed {error_run_count} times, retrying in {sleep_duration:?}: {e:#}",
        ),
        Some(LooksLike::Error) => error!(
            "Compaction failed {error_run_count} times, retrying in {sleep_duration:?}: {e:?}",
        ),
        None => {}
    }
}

///
/// GC task's main loop
///
async fn gc_loop(tenant: Arc<Tenant>, cancel: CancellationToken) {
    const MAX_BACKOFF_SECS: f64 = 300.0;
    // How many errors we have seen consequtively
    let mut error_run_count = 0;

    TENANT_TASK_EVENTS.with_label_values(&["start"]).inc();
    async {
        // GC might require downloading, to find the cutoff LSN that corresponds to the
        // cutoff specified as time.
        let ctx =
            RequestContext::todo_child(TaskKind::GarbageCollector, DownloadBehavior::Download);
        let mut first = true;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return;
                },
                tenant_wait_result = wait_for_active_tenant(&tenant) => match tenant_wait_result {
                    ControlFlow::Break(()) => return,
                    ControlFlow::Continue(()) => (),
                },
            }

            let period = tenant.get_gc_period();

            if first {
                first = false;
                if random_init_delay(period, &cancel).await.is_err() {
                    break;
                }
            }

            let started_at = Instant::now();

            let gc_horizon = tenant.get_gc_horizon();
            let sleep_duration = if period == Duration::ZERO || gc_horizon == 0 {
                #[cfg(not(feature = "testing"))]
                info!("automatic GC is disabled");
                // check again in 10 seconds, in case it's been enabled again.
                Duration::from_secs(10)
            } else {
                // Run gc
                let res = tenant
                    .gc_iteration(None, gc_horizon, tenant.get_pitr_interval(), &cancel, &ctx)
                    .await;
                if let Err(e) = res {
                    let wait_duration = backoff::exponential_backoff_duration_seconds(
                        error_run_count + 1,
                        1.0,
                        MAX_BACKOFF_SECS,
                    );
                    error_run_count += 1;
                    let wait_duration = Duration::from_secs_f64(wait_duration);
                    error!(
                        "Gc failed {error_run_count} times, retrying in {wait_duration:?}: {e:?}",
                    );
                    wait_duration
                } else {
                    error_run_count = 0;
                    period
                }
            };

            warn_when_period_overrun(started_at.elapsed(), period, BackgroundLoopKind::Gc);

            // Sleep
            if tokio::time::timeout(sleep_duration, cancel.cancelled())
                .await
                .is_ok()
            {
                break;
            }
        }
    }
    .await;
    TENANT_TASK_EVENTS.with_label_values(&["stop"]).inc();
}

async fn wait_for_active_tenant(tenant: &Arc<Tenant>) -> ControlFlow<()> {
    // if the tenant has a proper status already, no need to wait for anything
    if tenant.current_state() == TenantState::Active {
        ControlFlow::Continue(())
    } else {
        let mut tenant_state_updates = tenant.subscribe_for_state_updates();
        loop {
            match tenant_state_updates.changed().await {
                Ok(()) => {
                    let new_state = &*tenant_state_updates.borrow();
                    match new_state {
                        TenantState::Active => {
                            debug!("Tenant state changed to active, continuing the task loop");
                            return ControlFlow::Continue(());
                        }
                        state => {
                            debug!("Not running the task loop, tenant is not active: {state:?}");
                            continue;
                        }
                    }
                }
                Err(_sender_dropped_error) => {
                    return ControlFlow::Break(());
                }
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("cancelled")]
pub(crate) struct Cancelled;

/// Provide a random delay for background task initialization.
///
/// This delay prevents a thundering herd of background tasks and will likely keep them running on
/// different periods for more stable load.
pub(crate) async fn random_init_delay(
    period: Duration,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    use rand::Rng;

    if period == Duration::ZERO {
        return Ok(());
    }

    let d = {
        let mut rng = rand::thread_rng();
        rng.gen_range(Duration::ZERO..=period)
    };

    match tokio::time::timeout(d, cancel.cancelled()).await {
        Ok(_) => Err(Cancelled),
        Err(_) => Ok(()),
    }
}

/// Attention: the `task` and `period` beocme labels of a pageserver-wide prometheus metric.
pub(crate) fn warn_when_period_overrun(
    elapsed: Duration,
    period: Duration,
    task: BackgroundLoopKind,
) {
    // Duration::ZERO will happen because it's the "disable [bgtask]" value.
    if elapsed >= period && period != Duration::ZERO {
        // humantime does no significant digits clamping whereas Duration's debug is a bit more
        // intelligent. however it makes sense to keep the "configuration format" for period, even
        // though there's no way to output the actual config value.
        info!(
            ?elapsed,
            period = %humantime::format_duration(period),
            ?task,
            "task iteration took longer than the configured period"
        );
        crate::metrics::BACKGROUND_LOOP_PERIOD_OVERRUN_COUNT
            .with_label_values(&[task.as_static_str(), &format!("{}", period.as_secs())])
            .inc();
    }
}
