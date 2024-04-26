mod downloader;
pub mod heatmap;
mod heatmap_uploader;
mod scheduler;

use std::{sync::Arc, time::SystemTime};

use crate::{
    config::PageServerConf,
    disk_usage_eviction_task::DiskUsageEvictionInfo,
    task_mgr::{self, TaskKind, BACKGROUND_RUNTIME},
    virtual_file::MaybeFatalIo,
};

use self::{
    downloader::{downloader_task, SecondaryDetail},
    heatmap_uploader::heatmap_uploader_task,
};

use super::{
    config::{SecondaryLocationConfig, TenantConfOpt},
    mgr::TenantManager,
    span::debug_assert_current_span_has_tenant_id,
    storage_layer::LayerFileName,
};

use pageserver_api::{
    models,
    shard::{ShardIdentity, TenantShardId},
};
use remote_storage::GenericRemoteStorage;

use tokio_util::sync::CancellationToken;
use tracing::instrument;
use utils::{completion::Barrier, fs_ext, id::TimelineId, sync::gate::Gate};

enum DownloadCommand {
    Download(TenantShardId),
}
enum UploadCommand {
    Upload(TenantShardId),
}

impl UploadCommand {
    fn get_tenant_shard_id(&self) -> &TenantShardId {
        match self {
            Self::Upload(id) => id,
        }
    }
}

impl DownloadCommand {
    fn get_tenant_shard_id(&self) -> &TenantShardId {
        match self {
            Self::Download(id) => id,
        }
    }
}

struct CommandRequest<T> {
    payload: T,
    response_tx: tokio::sync::oneshot::Sender<CommandResponse>,
}

struct CommandResponse {
    result: anyhow::Result<()>,
}

// Whereas [`Tenant`] represents an attached tenant, this type represents the work
// we do for secondary tenant locations: where we are not serving clients or
// ingesting WAL, but we are maintaining a warm cache of layer files.
//
// This type is all about the _download_ path for secondary mode.  The upload path
// runs separately (see [`heatmap_uploader`]) while a regular attached `Tenant` exists.
//
// This structure coordinates TenantManager and SecondaryDownloader,
// so that the downloader can indicate which tenants it is currently
// operating on, and the manager can indicate when a particular
// secondary tenant should cancel any work in flight.
#[derive(Debug)]
pub(crate) struct SecondaryTenant {
    /// Carrying a tenant shard ID simplifies callers such as the downloader
    /// which need to organize many of these objects by ID.
    tenant_shard_id: TenantShardId,

    /// Cancellation token indicates to SecondaryDownloader that it should stop doing
    /// any work for this tenant at the next opportunity.
    pub(crate) cancel: CancellationToken,

    pub(crate) gate: Gate,

    // Secondary mode does not need the full shard identity or the TenantConfOpt.  However,
    // storing these enables us to report our full LocationConf, enabling convenient reconciliation
    // by the control plane (see [`Self::get_location_conf`])
    shard_identity: ShardIdentity,
    tenant_conf: std::sync::Mutex<TenantConfOpt>,

    detail: std::sync::Mutex<SecondaryDetail>,
}

impl SecondaryTenant {
    pub(crate) fn new(
        tenant_shard_id: TenantShardId,
        shard_identity: ShardIdentity,
        tenant_conf: TenantConfOpt,
        config: &SecondaryLocationConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            tenant_shard_id,
            // todo: shall we make this a descendent of the
            // main cancellation token, or is it sufficient that
            // on shutdown we walk the tenants and fire their
            // individual cancellations?
            cancel: CancellationToken::new(),
            gate: Gate::default(),

            shard_identity,
            tenant_conf: std::sync::Mutex::new(tenant_conf),

            detail: std::sync::Mutex::new(SecondaryDetail::new(config.clone())),
        })
    }

    pub(crate) async fn shutdown(&self) {
        self.cancel.cancel();

        // Wait for any secondary downloader work to complete
        self.gate.close().await;
    }

    pub(crate) fn set_config(&self, config: &SecondaryLocationConfig) {
        self.detail.lock().unwrap().config = config.clone();
    }

    pub(crate) fn set_tenant_conf(&self, config: &TenantConfOpt) {
        *(self.tenant_conf.lock().unwrap()) = config.clone();
    }

    /// For API access: generate a LocationConfig equivalent to the one that would be used to
    /// create a Tenant in the same state.  Do not use this in hot paths: it's for relatively
    /// rare external API calls, like a reconciliation at startup.
    pub(crate) fn get_location_conf(&self) -> models::LocationConfig {
        let conf = self.detail.lock().unwrap().config.clone();

        let conf = models::LocationConfigSecondary { warm: conf.warm };

        let tenant_conf = self.tenant_conf.lock().unwrap().clone();
        models::LocationConfig {
            mode: models::LocationConfigMode::Secondary,
            generation: None,
            secondary_conf: Some(conf),
            shard_number: self.tenant_shard_id.shard_number.0,
            shard_count: self.tenant_shard_id.shard_count.literal(),
            shard_stripe_size: self.shard_identity.stripe_size.0,
            tenant_conf: tenant_conf.into(),
        }
    }

    pub(crate) fn get_tenant_shard_id(&self) -> &TenantShardId {
        &self.tenant_shard_id
    }

    pub(crate) fn get_layers_for_eviction(self: &Arc<Self>) -> (DiskUsageEvictionInfo, usize) {
        self.detail.lock().unwrap().get_layers_for_eviction(self)
    }

    #[instrument(skip_all, fields(tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug(), timeline_id=%timeline_id, name=%name))]
    pub(crate) async fn evict_layer(
        &self,
        conf: &PageServerConf,
        timeline_id: TimelineId,
        name: LayerFileName,
    ) {
        debug_assert_current_span_has_tenant_id();

        let _guard = match self.gate.enter() {
            Ok(g) => g,
            Err(_) => {
                tracing::debug!("Dropping layer evictions, secondary tenant shutting down",);
                return;
            }
        };

        let now = SystemTime::now();

        let path = conf
            .timeline_path(&self.tenant_shard_id, &timeline_id)
            .join(name.file_name());

        // We tolerate ENOENT, because between planning eviction and executing
        // it, the secondary downloader could have seen an updated heatmap that
        // resulted in a layer being deleted.
        // Other local I/O errors are process-fatal: these should never happen.
        tokio::fs::remove_file(path)
            .await
            .or_else(fs_ext::ignore_not_found)
            .fatal_err("Deleting layer during eviction");

        // Update the timeline's state.  This does not have to be synchronized with
        // the download process, because:
        // - If downloader is racing with us to remove a file (e.g. because it is
        //   removed from heatmap), then our mutual .remove() operations will both
        //   succeed.
        // - If downloader is racing with us to download the object (this would require
        //   multiple eviction iterations to race with multiple download iterations), then
        //   if we remove it from the state, the worst that happens is the downloader
        //   downloads it again before re-inserting, or we delete the file but it remains
        //   in the state map (in which case it will be downloaded if this secondary
        //   tenant transitions to attached and tries to access it)
        //
        // The important assumption here is that the secondary timeline state does not
        // have to 100% match what is on disk, because it's a best-effort warming
        // of the cache.
        let mut detail = self.detail.lock().unwrap();
        if let Some(timeline_detail) = detail.timelines.get_mut(&timeline_id) {
            timeline_detail.on_disk_layers.remove(&name);
            timeline_detail.evicted_at.insert(name, now);
        }
    }
}

/// The SecondaryController is a pseudo-rpc client for administrative control of secondary mode downloads,
/// and heatmap uploads.  This is not a hot data path: it's primarily a hook for tests,
/// where we want to immediately upload/download for a particular tenant.  In normal operation
/// uploads & downloads are autonomous and not driven by this interface.
pub struct SecondaryController {
    upload_req_tx: tokio::sync::mpsc::Sender<CommandRequest<UploadCommand>>,
    download_req_tx: tokio::sync::mpsc::Sender<CommandRequest<DownloadCommand>>,
}

impl SecondaryController {
    async fn dispatch<T>(
        &self,
        queue: &tokio::sync::mpsc::Sender<CommandRequest<T>>,
        payload: T,
    ) -> anyhow::Result<()> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        queue
            .send(CommandRequest {
                payload,
                response_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Receiver shut down"))?;

        let response = response_rx
            .await
            .map_err(|_| anyhow::anyhow!("Request dropped"))?;

        response.result
    }

    pub async fn upload_tenant(&self, tenant_shard_id: TenantShardId) -> anyhow::Result<()> {
        self.dispatch(&self.upload_req_tx, UploadCommand::Upload(tenant_shard_id))
            .await
    }
    pub async fn download_tenant(&self, tenant_shard_id: TenantShardId) -> anyhow::Result<()> {
        self.dispatch(
            &self.download_req_tx,
            DownloadCommand::Download(tenant_shard_id),
        )
        .await
    }
}

pub fn spawn_tasks(
    tenant_manager: Arc<TenantManager>,
    remote_storage: GenericRemoteStorage,
    background_jobs_can_start: Barrier,
    cancel: CancellationToken,
) -> SecondaryController {
    let mgr_clone = tenant_manager.clone();
    let storage_clone = remote_storage.clone();
    let cancel_clone = cancel.clone();
    let bg_jobs_clone = background_jobs_can_start.clone();

    let (download_req_tx, download_req_rx) =
        tokio::sync::mpsc::channel::<CommandRequest<DownloadCommand>>(16);
    let (upload_req_tx, upload_req_rx) =
        tokio::sync::mpsc::channel::<CommandRequest<UploadCommand>>(16);

    task_mgr::spawn(
        BACKGROUND_RUNTIME.handle(),
        TaskKind::SecondaryDownloads,
        None,
        None,
        "secondary tenant downloads",
        false,
        async move {
            downloader_task(
                mgr_clone,
                storage_clone,
                download_req_rx,
                bg_jobs_clone,
                cancel_clone,
            )
            .await;

            Ok(())
        },
    );

    task_mgr::spawn(
        BACKGROUND_RUNTIME.handle(),
        TaskKind::SecondaryUploads,
        None,
        None,
        "heatmap uploads",
        false,
        async move {
            heatmap_uploader_task(
                tenant_manager,
                remote_storage,
                upload_req_rx,
                background_jobs_can_start,
                cancel,
            )
            .await;

            Ok(())
        },
    );

    SecondaryController {
        download_req_tx,
        upload_req_tx,
    }
}

/// For running with remote storage disabled: a SecondaryController that is connected to nothing.
pub fn null_controller() -> SecondaryController {
    let (download_req_tx, _download_req_rx) =
        tokio::sync::mpsc::channel::<CommandRequest<DownloadCommand>>(16);
    let (upload_req_tx, _upload_req_rx) =
        tokio::sync::mpsc::channel::<CommandRequest<UploadCommand>>(16);
    SecondaryController {
        upload_req_tx,
        download_req_tx,
    }
}
