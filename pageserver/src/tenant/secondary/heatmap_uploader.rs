use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use crate::{
    metrics::SECONDARY_MODE,
    tenant::{
        config::AttachmentMode,
        mgr::TenantManager,
        remote_timeline_client::remote_heatmap_path,
        span::debug_assert_current_span_has_tenant_id,
        tasks::{warn_when_period_overrun, BackgroundLoopKind},
        Tenant,
    },
};

use futures::Future;
use md5;
use pageserver_api::shard::TenantShardId;
use rand::Rng;
use remote_storage::{GenericRemoteStorage, TimeoutOrCancel};

use super::{
    heatmap::HeatMapTenant,
    scheduler::{self, JobGenerator, RunningJob, SchedulingResult, TenantBackgroundJobs},
    CommandRequest, UploadCommand,
};
use tokio_util::sync::CancellationToken;
use tracing::{info_span, instrument, Instrument};
use utils::{backoff, completion::Barrier, yielding_loop::yielding_loop};

pub(super) async fn heatmap_uploader_task(
    tenant_manager: Arc<TenantManager>,
    remote_storage: GenericRemoteStorage,
    command_queue: tokio::sync::mpsc::Receiver<CommandRequest<UploadCommand>>,
    background_jobs_can_start: Barrier,
    cancel: CancellationToken,
) {
    let concurrency = tenant_manager.get_conf().heatmap_upload_concurrency;

    let generator = HeatmapUploader {
        tenant_manager,
        remote_storage,
        cancel: cancel.clone(),
        tenants: HashMap::new(),
    };
    let mut scheduler = Scheduler::new(generator, concurrency);

    scheduler
        .run(command_queue, background_jobs_can_start, cancel)
        .instrument(info_span!("heatmap_uploader"))
        .await
}

/// This type is owned by a single task ([`heatmap_uploader_task`]) which runs an event
/// handling loop and mutates it as needed: there are no locks here, because that event loop
/// can hold &mut references to this type throughout.
struct HeatmapUploader {
    tenant_manager: Arc<TenantManager>,
    remote_storage: GenericRemoteStorage,
    cancel: CancellationToken,

    tenants: HashMap<TenantShardId, UploaderTenantState>,
}

struct WriteInProgress {
    barrier: Barrier,
}

impl RunningJob for WriteInProgress {
    fn get_barrier(&self) -> Barrier {
        self.barrier.clone()
    }
}

struct UploadPending {
    tenant: Arc<Tenant>,
    last_digest: Option<md5::Digest>,
    target_time: Option<Instant>,
    period: Option<Duration>,
}

impl scheduler::PendingJob for UploadPending {
    fn get_tenant_shard_id(&self) -> &TenantShardId {
        self.tenant.get_tenant_shard_id()
    }
}

struct WriteComplete {
    tenant_shard_id: TenantShardId,
    completed_at: Instant,
    digest: Option<md5::Digest>,
    next_upload: Option<Instant>,
}

impl scheduler::Completion for WriteComplete {
    fn get_tenant_shard_id(&self) -> &TenantShardId {
        &self.tenant_shard_id
    }
}

/// The heatmap uploader keeps a little bit of per-tenant state, mainly to remember
/// when we last did a write.  We only populate this after doing at least one
/// write for a tenant -- this avoids holding state for tenants that have
/// uploads disabled.

struct UploaderTenantState {
    // This Weak only exists to enable culling idle instances of this type
    // when the Tenant has been deallocated.
    tenant: Weak<Tenant>,

    /// Digest of the serialized heatmap that we last successfully uploaded
    ///
    /// md5 is generally a bad hash.  We use it because it's convenient for interop with AWS S3's ETag,
    /// which is also an md5sum.
    last_digest: Option<md5::Digest>,

    /// When the last upload attempt completed (may have been successful or failed)
    last_upload: Option<Instant>,

    /// When should we next do an upload?  None means never.
    next_upload: Option<Instant>,
}

type Scheduler = TenantBackgroundJobs<
    HeatmapUploader,
    UploadPending,
    WriteInProgress,
    WriteComplete,
    UploadCommand,
>;

impl JobGenerator<UploadPending, WriteInProgress, WriteComplete, UploadCommand>
    for HeatmapUploader
{
    async fn schedule(&mut self) -> SchedulingResult<UploadPending> {
        // Cull any entries in self.tenants whose Arc<Tenant> is gone
        self.tenants
            .retain(|_k, v| v.tenant.upgrade().is_some() && v.next_upload.is_some());

        let now = Instant::now();

        let mut result = SchedulingResult {
            jobs: Vec::new(),
            want_interval: None,
        };

        let tenants = self.tenant_manager.get_attached_active_tenant_shards();

        yielding_loop(1000, &self.cancel, tenants.into_iter(), |tenant| {
            let period = match tenant.get_heatmap_period() {
                None => {
                    // Heatmaps are disabled for this tenant
                    return;
                }
                Some(period) => {
                    // If any tenant has asked for uploads more frequent than our scheduling interval,
                    // reduce it to match so that we can keep up.  This is mainly useful in testing, where
                    // we may set rather short intervals.
                    result.want_interval = match result.want_interval {
                        None => Some(period),
                        Some(existing) => Some(std::cmp::min(period, existing)),
                    };

                    period
                }
            };

            // Stale attachments do not upload anything: if we are in this state, there is probably some
            // other attachment in mode Single or Multi running on another pageserver, and we don't
            // want to thrash and overwrite their heatmap uploads.
            if tenant.get_attach_mode() == AttachmentMode::Stale {
                return;
            }

            // Create an entry in self.tenants if one doesn't already exist: this will later be updated
            // with the completion time in on_completion.
            let state = self
                .tenants
                .entry(*tenant.get_tenant_shard_id())
                .or_insert_with(|| {
                    let jittered_period = rand::thread_rng().gen_range(Duration::ZERO..period);

                    UploaderTenantState {
                        tenant: Arc::downgrade(&tenant),
                        last_upload: None,
                        next_upload: Some(now.checked_add(jittered_period).unwrap_or(now)),
                        last_digest: None,
                    }
                });

            // Decline to do the upload if insufficient time has passed
            if state.next_upload.map(|nu| nu > now).unwrap_or(false) {
                return;
            }

            let last_digest = state.last_digest;
            result.jobs.push(UploadPending {
                tenant,
                last_digest,
                target_time: state.next_upload,
                period: Some(period),
            });
        })
        .await
        .ok();

        result
    }

    fn spawn(
        &mut self,
        job: UploadPending,
    ) -> (
        WriteInProgress,
        Pin<Box<dyn Future<Output = WriteComplete> + Send>>,
    ) {
        let UploadPending {
            tenant,
            last_digest,
            target_time,
            period,
        } = job;

        let remote_storage = self.remote_storage.clone();
        let (completion, barrier) = utils::completion::channel();
        let tenant_shard_id = *tenant.get_tenant_shard_id();
        (WriteInProgress { barrier }, Box::pin(async move {
            // Guard for the barrier in [`WriteInProgress`]
            let _completion = completion;

            let started_at = Instant::now();
            let digest = match upload_tenant_heatmap(remote_storage, &tenant, last_digest).await {
                Ok(UploadHeatmapOutcome::Uploaded(digest)) => {
                    let duration = Instant::now().duration_since(started_at);
                    SECONDARY_MODE
                        .upload_heatmap_duration
                        .observe(duration.as_secs_f64());
                    SECONDARY_MODE.upload_heatmap.inc();
                    Some(digest)
                }
                Ok(UploadHeatmapOutcome::NoChange | UploadHeatmapOutcome::Skipped) => last_digest,
                Err(UploadHeatmapError::Upload(e)) => {
                    tracing::warn!(
                        "Failed to upload heatmap for tenant {}: {e:#}",
                        tenant.get_tenant_shard_id(),
                    );
                    let duration = Instant::now().duration_since(started_at);
                    SECONDARY_MODE
                        .upload_heatmap_duration
                        .observe(duration.as_secs_f64());
                    SECONDARY_MODE.upload_heatmap_errors.inc();
                    last_digest
                }
                Err(UploadHeatmapError::Cancelled) => {
                    tracing::info!("Cancelled heatmap upload, shutting down");
                    last_digest
                }
            };

            let now = Instant::now();

            // If the job had a target execution time, we may check our final execution
            // time against that for observability purposes.
            if let (Some(target_time), Some(period)) = (target_time, period) {
                // Elapsed time includes any scheduling lag as well as the execution of the job
                let elapsed = now.duration_since(target_time);

                warn_when_period_overrun(elapsed, period, BackgroundLoopKind::HeatmapUpload);
            }

            let next_upload = tenant
                .get_heatmap_period()
                .and_then(|period| now.checked_add(period));

            WriteComplete {
                    tenant_shard_id: *tenant.get_tenant_shard_id(),
                    completed_at: now,
                    digest,
                    next_upload,
                }
        }.instrument(info_span!(parent: None, "heatmap_upload", tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug()))))
    }

    fn on_command(&mut self, command: UploadCommand) -> anyhow::Result<UploadPending> {
        let tenant_shard_id = command.get_tenant_shard_id();

        tracing::info!(
            tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(),
            "Starting heatmap write on command");
        let tenant = self
            .tenant_manager
            .get_attached_tenant_shard(*tenant_shard_id, true)
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(UploadPending {
            // Ignore our state for last digest: this forces an upload even if nothing has changed
            last_digest: None,
            tenant,
            target_time: None,
            period: None,
        })
    }

    #[instrument(skip_all, fields(tenant_id=%completion.tenant_shard_id.tenant_id, shard_id=%completion.tenant_shard_id.shard_slug()))]
    fn on_completion(&mut self, completion: WriteComplete) {
        tracing::debug!("Heatmap upload completed");
        let WriteComplete {
            tenant_shard_id,
            completed_at,
            digest,
            next_upload,
        } = completion;
        use std::collections::hash_map::Entry;
        match self.tenants.entry(tenant_shard_id) {
            Entry::Vacant(_) => {
                // Tenant state was dropped, nothing to update.
            }
            Entry::Occupied(mut entry) => {
                entry.get_mut().last_upload = Some(completed_at);
                entry.get_mut().last_digest = digest;
                entry.get_mut().next_upload = next_upload
            }
        }
    }
}

enum UploadHeatmapOutcome {
    /// We successfully wrote to remote storage, with this digest.
    Uploaded(md5::Digest),
    /// We did not upload because the heatmap digest was unchanged since the last upload
    NoChange,
    /// We skipped the upload for some reason, such as tenant/timeline not ready
    Skipped,
}

#[derive(thiserror::Error, Debug)]
enum UploadHeatmapError {
    #[error("Cancelled")]
    Cancelled,

    #[error(transparent)]
    Upload(#[from] anyhow::Error),
}

/// The inner upload operation.  This will skip if `last_digest` is Some and matches the digest
/// of the object we would have uploaded.
async fn upload_tenant_heatmap(
    remote_storage: GenericRemoteStorage,
    tenant: &Arc<Tenant>,
    last_digest: Option<md5::Digest>,
) -> Result<UploadHeatmapOutcome, UploadHeatmapError> {
    debug_assert_current_span_has_tenant_id();

    let generation = tenant.get_generation();
    if generation.is_none() {
        // We do not expect this: generations were implemented before heatmap uploads.  However,
        // handle it so that we don't have to make the generation in the heatmap an Option<>
        // (Generation::none is not serializable)
        tracing::warn!("Skipping heatmap upload for tenant with generation==None");
        return Ok(UploadHeatmapOutcome::Skipped);
    }

    let mut heatmap = HeatMapTenant {
        timelines: Vec::new(),
        generation,
    };
    let timelines = tenant.timelines.lock().unwrap().clone();

    // Ensure that Tenant::shutdown waits for any upload in flight: this is needed because otherwise
    // when we delete a tenant, we might race with an upload in flight and end up leaving a heatmap behind
    // in remote storage.
    let _guard = match tenant.gate.enter() {
        Ok(g) => g,
        Err(_) => {
            tracing::info!("Skipping heatmap upload for tenant which is shutting down");
            return Err(UploadHeatmapError::Cancelled);
        }
    };

    for (timeline_id, timeline) in timelines {
        let heatmap_timeline = timeline.generate_heatmap().await;
        match heatmap_timeline {
            None => {
                tracing::debug!(
                    "Skipping heatmap upload because timeline {timeline_id} is not ready"
                );
                return Ok(UploadHeatmapOutcome::Skipped);
            }
            Some(heatmap_timeline) => {
                heatmap.timelines.push(heatmap_timeline);
            }
        }
    }

    // Serialize the heatmap
    let bytes = serde_json::to_vec(&heatmap).map_err(|e| anyhow::anyhow!(e))?;
    let bytes = bytes::Bytes::from(bytes);
    let size = bytes.len();

    // Drop out early if nothing changed since our last upload
    let digest = md5::compute(&bytes);
    if Some(digest) == last_digest {
        return Ok(UploadHeatmapOutcome::NoChange);
    }

    let path = remote_heatmap_path(tenant.get_tenant_shard_id());

    let cancel = &tenant.cancel;

    tracing::debug!("Uploading {size} byte heatmap to {path}");
    if let Err(e) = backoff::retry(
        || async {
            let bytes = futures::stream::once(futures::future::ready(Ok(bytes.clone())));
            remote_storage
                .upload_storage_object(bytes, size, &path, cancel)
                .await
        },
        TimeoutOrCancel::caused_by_cancel,
        3,
        u32::MAX,
        "Uploading heatmap",
        cancel,
    )
    .await
    .ok_or_else(|| anyhow::anyhow!("Shutting down"))
    .and_then(|x| x)
    {
        if cancel.is_cancelled() {
            return Err(UploadHeatmapError::Cancelled);
        } else {
            return Err(e.into());
        }
    }

    tracing::info!("Successfully uploaded {size} byte heatmap to {path}");

    Ok(UploadHeatmapOutcome::Uploaded(digest))
}
