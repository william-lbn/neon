//!
//! Timeline repository implementation that keeps old data in files on disk, and
//! the recent changes in memory. See tenant/*_layer.rs files.
//! The functions here are responsible for locating the correct layer for the
//! get/put call, walking back the timeline branching history as needed.
//!
//! The files are stored in the .neon/tenants/<tenant_id>/timelines/<timeline_id>
//! directory. See docs/pageserver-storage.md for how the files are managed.
//! In addition to the layer files, there is a metadata file in the same
//! directory that contains information about the timeline, in particular its
//! parent timeline, and the last LSN that has been written to disk.
//!

use anyhow::{bail, Context};
use camino::Utf8Path;
use camino::Utf8PathBuf;
use enumset::EnumSet;
use futures::stream::FuturesUnordered;
use futures::FutureExt;
use futures::StreamExt;
use pageserver_api::models;
use pageserver_api::models::TimelineState;
use pageserver_api::models::WalRedoManagerStatus;
use pageserver_api::shard::ShardIdentity;
use pageserver_api::shard::TenantShardId;
use remote_storage::DownloadError;
use remote_storage::GenericRemoteStorage;
use remote_storage::TimeoutOrCancel;
use std::fmt;
use storage_broker::BrokerClientChannel;
use tokio::io::BufReader;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::*;
use utils::backoff;
use utils::completion;
use utils::crashsafe::path_with_suffix_extension;
use utils::failpoint_support;
use utils::fs_ext;
use utils::sync::gate::Gate;
use utils::sync::gate::GateGuard;
use utils::timeout::timeout_cancellable;
use utils::timeout::TimeoutCancellableError;

use self::config::AttachedLocationConfig;
use self::config::AttachmentMode;
use self::config::LocationConf;
use self::config::TenantConf;
use self::delete::DeleteTenantFlow;
use self::metadata::TimelineMetadata;
use self::mgr::GetActiveTenantError;
use self::mgr::GetTenantError;
use self::mgr::TenantsMap;
use self::remote_timeline_client::upload::upload_index_part;
use self::remote_timeline_client::RemoteTimelineClient;
use self::timeline::uninit::TimelineExclusionError;
use self::timeline::uninit::TimelineUninitMark;
use self::timeline::uninit::UninitializedTimeline;
use self::timeline::EvictionTaskTenantState;
use self::timeline::TimelineResources;
use self::timeline::WaitLsnError;
use crate::config::PageServerConf;
use crate::context::{DownloadBehavior, RequestContext};
use crate::deletion_queue::DeletionQueueClient;
use crate::deletion_queue::DeletionQueueError;
use crate::import_datadir;
use crate::is_uninit_mark;
use crate::metrics::TENANT;
use crate::metrics::{
    remove_tenant_metrics, BROKEN_TENANTS_SET, TENANT_STATE_METRIC, TENANT_SYNTHETIC_SIZE_METRIC,
};
use crate::repository::GcResult;
use crate::task_mgr;
use crate::task_mgr::TaskKind;
use crate::tenant::config::LocationMode;
use crate::tenant::config::TenantConfOpt;
pub use crate::tenant::remote_timeline_client::index::IndexPart;
use crate::tenant::remote_timeline_client::remote_initdb_archive_path;
use crate::tenant::remote_timeline_client::MaybeDeletedIndexPart;
use crate::tenant::remote_timeline_client::INITDB_PATH;
use crate::tenant::storage_layer::DeltaLayer;
use crate::tenant::storage_layer::ImageLayer;
use crate::InitializationOrder;
use std::cmp::min;
use std::collections::hash_map::Entry;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::ops::Bound::Included;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::span;
use crate::tenant::timeline::delete::DeleteTimelineFlow;
use crate::tenant::timeline::uninit::cleanup_timeline_directory;
use crate::virtual_file::VirtualFile;
use crate::walredo::PostgresRedoManager;
use crate::TEMP_FILE_SUFFIX;
use once_cell::sync::Lazy;
pub use pageserver_api::models::TenantState;
use tokio::sync::Semaphore;

static INIT_DB_SEMAPHORE: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(8));
use toml_edit;
use utils::{
    crashsafe,
    generation::Generation,
    id::TimelineId,
    lsn::{Lsn, RecordLsn},
};

/// Declare a failpoint that can use the `pause` failpoint action.
/// We don't want to block the executor thread, hence, spawn_blocking + await.
macro_rules! pausable_failpoint {
    ($name:literal) => {
        if cfg!(feature = "testing") {
            tokio::task::spawn_blocking({
                let current = tracing::Span::current();
                move || {
                    let _entered = current.entered();
                    tracing::info!("at failpoint {}", $name);
                    fail::fail_point!($name);
                }
            })
            .await
            .expect("spawn_blocking");
        }
    };
    ($name:literal, $cond:expr) => {
        if cfg!(feature = "testing") {
            if $cond {
                pausable_failpoint!($name)
            }
        }
    };
}

pub mod blob_io;
pub mod block_io;

pub mod disk_btree;
pub(crate) mod ephemeral_file;
pub mod layer_map;

pub mod metadata;
mod par_fsync;
pub mod remote_timeline_client;
pub mod storage_layer;

pub mod config;
pub mod delete;
pub mod mgr;
pub mod secondary;
pub mod tasks;
pub mod upload_queue;

pub(crate) mod timeline;

pub mod size;

pub(crate) mod throttle;

pub(crate) use crate::span::debug_assert_current_span_has_tenant_and_timeline_id;
pub(crate) use timeline::{LogicalSizeCalculationCause, PageReconstructError, Timeline};

// re-export for use in walreceiver
pub use crate::tenant::timeline::WalReceiverInfo;

/// The "tenants" part of `tenants/<tenant>/timelines...`
pub const TENANTS_SEGMENT_NAME: &str = "tenants";

/// Parts of the `.neon/tenants/<tenant_id>/timelines/<timeline_id>` directory prefix.
pub const TIMELINES_SEGMENT_NAME: &str = "timelines";

pub const TENANT_DELETED_MARKER_FILE_NAME: &str = "deleted";

/// References to shared objects that are passed into each tenant, such
/// as the shared remote storage client and process initialization state.
#[derive(Clone)]
pub struct TenantSharedResources {
    pub broker_client: storage_broker::BrokerClientChannel,
    pub remote_storage: Option<GenericRemoteStorage>,
    pub deletion_queue_client: DeletionQueueClient,
}

/// A [`Tenant`] is really an _attached_ tenant.  The configuration
/// for an attached tenant is a subset of the [`LocationConf`], represented
/// in this struct.
pub(super) struct AttachedTenantConf {
    tenant_conf: TenantConfOpt,
    location: AttachedLocationConfig,
}

impl AttachedTenantConf {
    fn try_from(location_conf: LocationConf) -> anyhow::Result<Self> {
        match &location_conf.mode {
            LocationMode::Attached(attach_conf) => Ok(Self {
                tenant_conf: location_conf.tenant_conf,
                location: *attach_conf,
            }),
            LocationMode::Secondary(_) => {
                anyhow::bail!("Attempted to construct AttachedTenantConf from a LocationConf in secondary mode")
            }
        }
    }
}
struct TimelinePreload {
    timeline_id: TimelineId,
    client: RemoteTimelineClient,
    index_part: Result<MaybeDeletedIndexPart, DownloadError>,
}

pub(crate) struct TenantPreload {
    deleting: bool,
    timelines: HashMap<TimelineId, TimelinePreload>,
}

/// When we spawn a tenant, there is a special mode for tenant creation that
/// avoids trying to read anything from remote storage.
pub(crate) enum SpawnMode {
    Normal,
    Create,
}

///
/// Tenant consists of multiple timelines. Keep them in a hash table.
///
pub struct Tenant {
    // Global pageserver config parameters
    pub conf: &'static PageServerConf,

    /// The value creation timestamp, used to measure activation delay, see:
    /// <https://github.com/neondatabase/neon/issues/4025>
    constructed_at: Instant,

    state: watch::Sender<TenantState>,

    // Overridden tenant-specific config parameters.
    // We keep TenantConfOpt sturct here to preserve the information
    // about parameters that are not set.
    // This is necessary to allow global config updates.
    tenant_conf: Arc<RwLock<AttachedTenantConf>>,

    tenant_shard_id: TenantShardId,

    // The detailed sharding information, beyond the number/count in tenant_shard_id
    shard_identity: ShardIdentity,

    /// The remote storage generation, used to protect S3 objects from split-brain.
    /// Does not change over the lifetime of the [`Tenant`] object.
    ///
    /// This duplicates the generation stored in LocationConf, but that structure is mutable:
    /// this copy enforces the invariant that generatio doesn't change during a Tenant's lifetime.
    generation: Generation,

    timelines: Mutex<HashMap<TimelineId, Arc<Timeline>>>,

    /// During timeline creation, we first insert the TimelineId to the
    /// creating map, then `timelines`, then remove it from the creating map.
    /// **Lock order**: if acquring both, acquire`timelines` before `timelines_creating`
    timelines_creating: std::sync::Mutex<HashSet<TimelineId>>,

    // This mutex prevents creation of new timelines during GC.
    // Adding yet another mutex (in addition to `timelines`) is needed because holding
    // `timelines` mutex during all GC iteration
    // may block for a long time `get_timeline`, `get_timelines_state`,... and other operations
    // with timelines, which in turn may cause dropping replication connection, expiration of wait_for_lsn
    // timeout...
    gc_cs: tokio::sync::Mutex<()>,
    walredo_mgr: Option<Arc<WalRedoManager>>,

    // provides access to timeline data sitting in the remote storage
    pub(crate) remote_storage: Option<GenericRemoteStorage>,

    // Access to global deletion queue for when this tenant wants to schedule a deletion
    deletion_queue_client: DeletionQueueClient,

    /// Cached logical sizes updated updated on each [`Tenant::gather_size_inputs`].
    cached_logical_sizes: tokio::sync::Mutex<HashMap<(TimelineId, Lsn), u64>>,
    cached_synthetic_tenant_size: Arc<AtomicU64>,

    eviction_task_tenant_state: tokio::sync::Mutex<EvictionTaskTenantState>,

    /// If the tenant is in Activating state, notify this to encourage it
    /// to proceed to Active as soon as possible, rather than waiting for lazy
    /// background warmup.
    pub(crate) activate_now_sem: tokio::sync::Semaphore,

    pub(crate) delete_progress: Arc<tokio::sync::Mutex<DeleteTenantFlow>>,

    // Cancellation token fires when we have entered shutdown().  This is a parent of
    // Timelines' cancellation token.
    pub(crate) cancel: CancellationToken,

    // Users of the Tenant such as the page service must take this Gate to avoid
    // trying to use a Tenant which is shutting down.
    pub(crate) gate: Gate,

    /// Throttle applied at the top of [`Timeline::get`].
    /// All [`Tenant::timelines`] of a given [`Tenant`] instance share the same [`throttle::Throttle`] instance.
    pub(crate) timeline_get_throttle:
        Arc<throttle::Throttle<&'static crate::metrics::tenant_throttling::TimelineGet>>,
}

impl std::fmt::Debug for Tenant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.tenant_shard_id, self.current_state())
    }
}

pub(crate) enum WalRedoManager {
    Prod(PostgresRedoManager),
    #[cfg(test)]
    Test(harness::TestRedoManager),
}

impl From<PostgresRedoManager> for WalRedoManager {
    fn from(mgr: PostgresRedoManager) -> Self {
        Self::Prod(mgr)
    }
}

#[cfg(test)]
impl From<harness::TestRedoManager> for WalRedoManager {
    fn from(mgr: harness::TestRedoManager) -> Self {
        Self::Test(mgr)
    }
}

impl WalRedoManager {
    pub(crate) fn maybe_quiesce(&self, idle_timeout: Duration) {
        match self {
            Self::Prod(mgr) => mgr.maybe_quiesce(idle_timeout),
            #[cfg(test)]
            Self::Test(_) => {
                // Not applicable to test redo manager
            }
        }
    }

    /// # Cancel-Safety
    ///
    /// This method is cancellation-safe.
    pub async fn request_redo(
        &self,
        key: crate::repository::Key,
        lsn: Lsn,
        base_img: Option<(Lsn, bytes::Bytes)>,
        records: Vec<(Lsn, crate::walrecord::NeonWalRecord)>,
        pg_version: u32,
    ) -> anyhow::Result<bytes::Bytes> {
        match self {
            Self::Prod(mgr) => {
                mgr.request_redo(key, lsn, base_img, records, pg_version)
                    .await
            }
            #[cfg(test)]
            Self::Test(mgr) => {
                mgr.request_redo(key, lsn, base_img, records, pg_version)
                    .await
            }
        }
    }

    pub(crate) fn status(&self) -> Option<WalRedoManagerStatus> {
        match self {
            WalRedoManager::Prod(m) => m.status(),
            #[cfg(test)]
            WalRedoManager::Test(_) => None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GetTimelineError {
    #[error("Timeline {tenant_id}/{timeline_id} is not active, state: {state:?}")]
    NotActive {
        tenant_id: TenantShardId,
        timeline_id: TimelineId,
        state: TimelineState,
    },
    #[error("Timeline {tenant_id}/{timeline_id} was not found")]
    NotFound {
        tenant_id: TenantShardId,
        timeline_id: TimelineId,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LoadLocalTimelineError {
    #[error("FailedToLoad")]
    Load(#[source] anyhow::Error),
    #[error("FailedToResumeDeletion")]
    ResumeDeletion(#[source] anyhow::Error),
}

#[derive(thiserror::Error)]
pub enum DeleteTimelineError {
    #[error("NotFound")]
    NotFound,

    #[error("HasChildren")]
    HasChildren(Vec<TimelineId>),

    #[error("Timeline deletion is already in progress")]
    AlreadyInProgress(Arc<tokio::sync::Mutex<DeleteTimelineFlow>>),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Debug for DeleteTimelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "NotFound"),
            Self::HasChildren(c) => f.debug_tuple("HasChildren").field(c).finish(),
            Self::AlreadyInProgress(_) => f.debug_tuple("AlreadyInProgress").finish(),
            Self::Other(e) => f.debug_tuple("Other").field(e).finish(),
        }
    }
}

pub enum SetStoppingError {
    AlreadyStopping(completion::Barrier),
    Broken,
}

impl Debug for SetStoppingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyStopping(_) => f.debug_tuple("AlreadyStopping").finish(),
            Self::Broken => write!(f, "Broken"),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CreateTimelineError {
    #[error("creation of timeline with the given ID is in progress")]
    AlreadyCreating,
    #[error("timeline already exists with different parameters")]
    Conflict,
    #[error(transparent)]
    AncestorLsn(anyhow::Error),
    #[error("ancestor timeline is not active")]
    AncestorNotActive,
    #[error("tenant shutting down")]
    ShuttingDown,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
enum InitdbError {
    Other(anyhow::Error),
    Cancelled,
    Spawn(std::io::Result<()>),
    Failed(std::process::ExitStatus, Vec<u8>),
}

impl fmt::Display for InitdbError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InitdbError::Cancelled => write!(f, "Operation was cancelled"),
            InitdbError::Spawn(e) => write!(f, "Spawn error: {:?}", e),
            InitdbError::Failed(status, stderr) => write!(
                f,
                "Command failed with status {:?}: {}",
                status,
                String::from_utf8_lossy(stderr)
            ),
            InitdbError::Other(e) => write!(f, "Error: {:?}", e),
        }
    }
}

impl From<std::io::Error> for InitdbError {
    fn from(error: std::io::Error) -> Self {
        InitdbError::Spawn(Err(error))
    }
}

enum CreateTimelineCause {
    Load,
    Delete,
}

impl Tenant {
    /// Yet another helper for timeline initialization.
    ///
    /// - Initializes the Timeline struct and inserts it into the tenant's hash map
    /// - Scans the local timeline directory for layer files and builds the layer map
    /// - Downloads remote index file and adds remote files to the layer map
    /// - Schedules remote upload tasks for any files that are present locally but missing from remote storage.
    ///
    /// If the operation fails, the timeline is left in the tenant's hash map in Broken state. On success,
    /// it is marked as Active.
    #[allow(clippy::too_many_arguments)]
    async fn timeline_init_and_sync(
        &self,
        timeline_id: TimelineId,
        resources: TimelineResources,
        index_part: Option<IndexPart>,
        metadata: TimelineMetadata,
        ancestor: Option<Arc<Timeline>>,
        _ctx: &RequestContext,
    ) -> anyhow::Result<()> {
        let tenant_id = self.tenant_shard_id;

        let timeline = self.create_timeline_struct(
            timeline_id,
            &metadata,
            ancestor.clone(),
            resources,
            CreateTimelineCause::Load,
        )?;
        let disk_consistent_lsn = timeline.get_disk_consistent_lsn();
        anyhow::ensure!(
            disk_consistent_lsn.is_valid(),
            "Timeline {tenant_id}/{timeline_id} has invalid disk_consistent_lsn"
        );
        assert_eq!(
            disk_consistent_lsn,
            metadata.disk_consistent_lsn(),
            "these are used interchangeably"
        );

        if let Some(index_part) = index_part.as_ref() {
            timeline
                .remote_client
                .as_ref()
                .unwrap()
                .init_upload_queue(index_part)?;
        } else if self.remote_storage.is_some() {
            // No data on the remote storage, but we have local metadata file. We can end up
            // here with timeline_create being interrupted before finishing index part upload.
            // By doing what we do here, the index part upload is retried.
            // If control plane retries timeline creation in the meantime, the mgmt API handler
            // for timeline creation will coalesce on the upload we queue here.
            let rtc = timeline.remote_client.as_ref().unwrap();
            rtc.init_upload_queue_for_empty_remote(&metadata)?;
            rtc.schedule_index_upload_for_metadata_update(&metadata)?;
        }

        timeline
            .load_layer_map(disk_consistent_lsn, index_part)
            .await
            .with_context(|| {
                format!("Failed to load layermap for timeline {tenant_id}/{timeline_id}")
            })?;

        {
            // avoiding holding it across awaits
            let mut timelines_accessor = self.timelines.lock().unwrap();
            match timelines_accessor.entry(timeline_id) {
                Entry::Occupied(_) => {
                    // The uninit mark file acts as a lock that prevents another task from
                    // initializing the timeline at the same time.
                    unreachable!(
                        "Timeline {tenant_id}/{timeline_id} already exists in the tenant map"
                    );
                }
                Entry::Vacant(v) => {
                    v.insert(Arc::clone(&timeline));
                    timeline.maybe_spawn_flush_loop();
                }
            }
        };

        // Sanity check: a timeline should have some content.
        anyhow::ensure!(
            ancestor.is_some()
                || timeline
                    .layers
                    .read()
                    .await
                    .layer_map()
                    .iter_historic_layers()
                    .next()
                    .is_some(),
            "Timeline has no ancestor and no layer files"
        );

        Ok(())
    }

    /// Attach a tenant that's available in cloud storage.
    ///
    /// This returns quickly, after just creating the in-memory object
    /// Tenant struct and launching a background task to download
    /// the remote index files.  On return, the tenant is most likely still in
    /// Attaching state, and it will become Active once the background task
    /// finishes. You can use wait_until_active() to wait for the task to
    /// complete.
    ///
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn(
        conf: &'static PageServerConf,
        tenant_shard_id: TenantShardId,
        resources: TenantSharedResources,
        attached_conf: AttachedTenantConf,
        shard_identity: ShardIdentity,
        init_order: Option<InitializationOrder>,
        tenants: &'static std::sync::RwLock<TenantsMap>,
        mode: SpawnMode,
        ctx: &RequestContext,
    ) -> anyhow::Result<Arc<Tenant>> {
        let wal_redo_manager = Arc::new(WalRedoManager::from(PostgresRedoManager::new(
            conf,
            tenant_shard_id,
        )));

        let TenantSharedResources {
            broker_client,
            remote_storage,
            deletion_queue_client,
        } = resources;

        let attach_mode = attached_conf.location.attach_mode;
        let generation = attached_conf.location.generation;

        let tenant = Arc::new(Tenant::new(
            TenantState::Attaching,
            conf,
            attached_conf,
            shard_identity,
            Some(wal_redo_manager),
            tenant_shard_id,
            remote_storage.clone(),
            deletion_queue_client,
        ));

        // The attach task will carry a GateGuard, so that shutdown() reliably waits for it to drop out if
        // we shut down while attaching.
        let attach_gate_guard = tenant
            .gate
            .enter()
            .expect("We just created the Tenant: nothing else can have shut it down yet");

        // Do all the hard work in the background
        let tenant_clone = Arc::clone(&tenant);
        let ctx = ctx.detached_child(TaskKind::Attach, DownloadBehavior::Warn);
        task_mgr::spawn(
            &tokio::runtime::Handle::current(),
            TaskKind::Attach,
            Some(tenant_shard_id),
            None,
            "attach tenant",
            false,
            async move {

                info!(
                    ?attach_mode,
                    "Attaching tenant"
                );

                let _gate_guard = attach_gate_guard;

                // Is this tenant being spawned as part of process startup?
                let starting_up = init_order.is_some();
                scopeguard::defer! {
                    if starting_up {
                        TENANT.startup_complete.inc();
                    }
                }

                // Ideally we should use Tenant::set_broken_no_wait, but it is not supposed to be used when tenant is in loading state.
                let make_broken =
                    |t: &Tenant, err: anyhow::Error| {
                        error!("attach failed, setting tenant state to Broken: {err:?}");
                        t.state.send_modify(|state| {
                            // The Stopping case is for when we have passed control on to DeleteTenantFlow:
                            // if it errors, we will call make_broken when tenant is already in Stopping.
                            assert!(
                            matches!(*state, TenantState::Attaching | TenantState::Stopping { .. }),
                            "the attach task owns the tenant state until activation is complete"
                        );

                            *state = TenantState::broken_from_reason(err.to_string());
                        });
                    };

                let mut init_order = init_order;
                // take the completion because initial tenant loading will complete when all of
                // these tasks complete.
                let _completion = init_order
                    .as_mut()
                    .and_then(|x| x.initial_tenant_load.take());
                let remote_load_completion = init_order
                    .as_mut()
                    .and_then(|x| x.initial_tenant_load_remote.take());

                enum AttachType<'a> {
                    // During pageserver startup, we are attaching this tenant lazily in the background
                    Warmup(tokio::sync::SemaphorePermit<'a>),
                    // During pageserver startup, we are attaching this tenant as soon as we can,
                    // because a client tried to access it.
                    OnDemand,
                    // During normal operations after startup, we are attaching a tenant.
                    Normal,
                }

                // Before doing any I/O, wait for either or:
                // - A client to attempt to access to this tenant (on-demand loading)
                // - A permit to become available in the warmup semaphore (background warmup)
                //
                // Some-ness of init_order is how we know if we're attaching during startup or later
                // in process lifetime.
                let attach_type = if init_order.is_some() {
                    tokio::select!(
                        _ = tenant_clone.activate_now_sem.acquire() => {
                            tracing::info!("Activating tenant (on-demand)");
                            AttachType::OnDemand
                        },
                        permit_result = conf.concurrent_tenant_warmup.inner().acquire() => {
                            match permit_result {
                                Ok(p) => {
                                    tracing::info!("Activating tenant (warmup)");
                                    AttachType::Warmup(p)
                                }
                                Err(_) => {
                                    // This is unexpected: the warmup semaphore should stay alive
                                    // for the lifetime of init_order.  Log a warning and proceed.
                                    tracing::warn!("warmup_limit semaphore unexpectedly closed");
                                    AttachType::Normal
                                }
                            }

                        }
                        _ = tenant_clone.cancel.cancelled() => {
                            // This is safe, but should be pretty rare: it is interesting if a tenant
                            // stayed in Activating for such a long time that shutdown found it in
                            // that state.
                            tracing::info!(state=%tenant_clone.current_state(), "Tenant shut down before activation");
                            // Make the tenant broken so that set_stopping will not hang waiting for it to leave
                            // the Attaching state.  This is an over-reaction (nothing really broke, the tenant is
                            // just shutting down), but ensures progress.
                            make_broken(&tenant_clone, anyhow::anyhow!("Shut down while Attaching"));
                            return Ok(());
                        },
                    )
                } else {
                    AttachType::Normal
                };

                let preload = match (&mode, &remote_storage) {
                    (SpawnMode::Create, _) => {
                        None
                    },
                    (SpawnMode::Normal, Some(remote_storage)) => {
                        let _preload_timer = TENANT.preload.start_timer();
                        let res = tenant_clone
                            .preload(remote_storage, task_mgr::shutdown_token())
                            .await;
                        match res {
                            Ok(p) => Some(p),
                            Err(e) => {
                                make_broken(&tenant_clone, anyhow::anyhow!(e));
                                return Ok(());
                            }
                        }
                    }
                    (SpawnMode::Normal, None) => {
                        let _preload_timer = TENANT.preload.start_timer();
                        None
                    }
                };

                // Remote preload is complete.
                drop(remote_load_completion);

                let pending_deletion = {
                    match DeleteTenantFlow::should_resume_deletion(
                        conf,
                        preload.as_ref().map(|p| p.deleting).unwrap_or(false),
                        &tenant_clone,
                    )
                    .await
                    {
                        Ok(should_resume_deletion) => should_resume_deletion,
                        Err(err) => {
                            make_broken(&tenant_clone, anyhow::anyhow!(err));
                            return Ok(());
                        }
                    }
                };

                info!("pending_deletion {}", pending_deletion.is_some());

                if let Some(deletion) = pending_deletion {
                    // as we are no longer loading, signal completion by dropping
                    // the completion while we resume deletion
                    drop(_completion);
                    let background_jobs_can_start =
                        init_order.as_ref().map(|x| &x.background_jobs_can_start);
                    if let Some(background) = background_jobs_can_start {
                        info!("waiting for backgound jobs barrier");
                        background.clone().wait().await;
                        info!("ready for backgound jobs barrier");
                    }

                    let deleted = DeleteTenantFlow::resume_from_attach(
                        deletion,
                        &tenant_clone,
                        preload,
                        tenants,
                        &ctx,
                    )
                    .await;

                    if let Err(e) = deleted {
                        make_broken(&tenant_clone, anyhow::anyhow!(e));
                    }

                    return Ok(());
                }

                // We will time the duration of the attach phase unless this is a creation (attach will do no work)
                let attached = {
                    let _attach_timer = match mode {
                        SpawnMode::Create => None,
                        SpawnMode::Normal => {Some(TENANT.attach.start_timer())}
                    };
                    tenant_clone.attach(preload, mode, &ctx).await
                };

                match attached {
                    Ok(()) => {
                        info!("attach finished, activating");
                        tenant_clone.activate(broker_client, None, &ctx);
                    }
                    Err(e) => {
                        make_broken(&tenant_clone, anyhow::anyhow!(e));
                    }
                }

                // If we are doing an opportunistic warmup attachment at startup, initialize
                // logical size at the same time.  This is better than starting a bunch of idle tenants
                // with cold caches and then coming back later to initialize their logical sizes.
                //
                // It also prevents the warmup proccess competing with the concurrency limit on
                // logical size calculations: if logical size calculation semaphore is saturated,
                // then warmup will wait for that before proceeding to the next tenant.
                if let AttachType::Warmup(_permit) = attach_type {
                    let mut futs: FuturesUnordered<_> = tenant_clone.timelines.lock().unwrap().values().cloned().map(|t| t.await_initial_logical_size()).collect();
                    tracing::info!("Waiting for initial logical sizes while warming up...");
                    while futs.next().await.is_some() {}
                    tracing::info!("Warm-up complete");
                }

                Ok(())
            }
            .instrument(tracing::info_span!(parent: None, "attach", tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), gen=?generation)),
        );
        Ok(tenant)
    }

    #[instrument(skip_all)]
    pub(crate) async fn preload(
        self: &Arc<Tenant>,
        remote_storage: &GenericRemoteStorage,
        cancel: CancellationToken,
    ) -> anyhow::Result<TenantPreload> {
        span::debug_assert_current_span_has_tenant_id();
        // Get list of remote timelines
        // download index files for every tenant timeline
        info!("listing remote timelines");
        let (remote_timeline_ids, other_keys) = remote_timeline_client::list_remote_timelines(
            remote_storage,
            self.tenant_shard_id,
            cancel.clone(),
        )
        .await?;

        let deleting = other_keys.contains(TENANT_DELETED_MARKER_FILE_NAME);
        info!(
            "found {} timelines, deleting={}",
            remote_timeline_ids.len(),
            deleting
        );

        for k in other_keys {
            if k != TENANT_DELETED_MARKER_FILE_NAME {
                warn!("Unexpected non timeline key {k}");
            }
        }

        Ok(TenantPreload {
            deleting,
            timelines: self
                .load_timeline_metadata(remote_timeline_ids, remote_storage, cancel)
                .await?,
        })
    }

    ///
    /// Background task that downloads all data for a tenant and brings it to Active state.
    ///
    /// No background tasks are started as part of this routine.
    ///
    async fn attach(
        self: &Arc<Tenant>,
        preload: Option<TenantPreload>,
        mode: SpawnMode,
        ctx: &RequestContext,
    ) -> anyhow::Result<()> {
        span::debug_assert_current_span_has_tenant_id();

        failpoint_support::sleep_millis_async!("before-attaching-tenant");

        let preload = match (preload, mode) {
            (Some(p), _) => p,
            (None, SpawnMode::Create) => TenantPreload {
                deleting: false,
                timelines: HashMap::new(),
            },
            (None, SpawnMode::Normal) => {
                anyhow::bail!("local-only deployment is no longer supported, https://github.com/neondatabase/neon/issues/5624");
            }
        };

        let mut timelines_to_resume_deletions = vec![];

        let mut remote_index_and_client = HashMap::new();
        let mut timeline_ancestors = HashMap::new();
        let mut existent_timelines = HashSet::new();
        for (timeline_id, preload) in preload.timelines {
            let index_part = match preload.index_part {
                Ok(i) => {
                    debug!("remote index part exists for timeline {timeline_id}");
                    // We found index_part on the remote, this is the standard case.
                    existent_timelines.insert(timeline_id);
                    i
                }
                Err(DownloadError::NotFound) => {
                    // There is no index_part on the remote. We only get here
                    // if there is some prefix for the timeline in the remote storage.
                    // This can e.g. be the initdb.tar.zst archive, maybe a
                    // remnant from a prior incomplete creation or deletion attempt.
                    // Delete the local directory as the deciding criterion for a
                    // timeline's existence is presence of index_part.
                    info!(%timeline_id, "index_part not found on remote");
                    continue;
                }
                Err(e) => {
                    // Some (possibly ephemeral) error happened during index_part download.
                    // Pretend the timeline exists to not delete the timeline directory,
                    // as it might be a temporary issue and we don't want to re-download
                    // everything after it resolves.
                    warn!(%timeline_id, "Failed to load index_part from remote storage, failed creation? ({e})");

                    existent_timelines.insert(timeline_id);
                    continue;
                }
            };
            match index_part {
                MaybeDeletedIndexPart::IndexPart(index_part) => {
                    timeline_ancestors.insert(timeline_id, index_part.metadata.clone());
                    remote_index_and_client.insert(timeline_id, (index_part, preload.client));
                }
                MaybeDeletedIndexPart::Deleted(index_part) => {
                    info!(
                        "timeline {} is deleted, picking to resume deletion",
                        timeline_id
                    );
                    timelines_to_resume_deletions.push((timeline_id, index_part, preload.client));
                }
            }
        }

        // For every timeline, download the metadata file, scan the local directory,
        // and build a layer map that contains an entry for each remote and local
        // layer file.
        let sorted_timelines = tree_sort_timelines(timeline_ancestors, |m| m.ancestor_timeline())?;
        for (timeline_id, remote_metadata) in sorted_timelines {
            let (index_part, remote_client) = remote_index_and_client
                .remove(&timeline_id)
                .expect("just put it in above");

            // TODO again handle early failure
            self.load_remote_timeline(
                timeline_id,
                index_part,
                remote_metadata,
                TimelineResources {
                    remote_client: Some(remote_client),
                    deletion_queue_client: self.deletion_queue_client.clone(),
                    timeline_get_throttle: self.timeline_get_throttle.clone(),
                },
                ctx,
            )
            .await
            .with_context(|| {
                format!(
                    "failed to load remote timeline {} for tenant {}",
                    timeline_id, self.tenant_shard_id
                )
            })?;
        }

        // Walk through deleted timelines, resume deletion
        for (timeline_id, index_part, remote_timeline_client) in timelines_to_resume_deletions {
            remote_timeline_client
                .init_upload_queue_stopped_to_continue_deletion(&index_part)
                .context("init queue stopped")
                .map_err(LoadLocalTimelineError::ResumeDeletion)?;

            DeleteTimelineFlow::resume_deletion(
                Arc::clone(self),
                timeline_id,
                &index_part.metadata,
                Some(remote_timeline_client),
                self.deletion_queue_client.clone(),
            )
            .instrument(tracing::info_span!("timeline_delete", %timeline_id))
            .await
            .context("resume_deletion")
            .map_err(LoadLocalTimelineError::ResumeDeletion)?;
        }

        // The local filesystem contents are a cache of what's in the remote IndexPart;
        // IndexPart is the source of truth.
        self.clean_up_timelines(&existent_timelines)?;

        fail::fail_point!("attach-before-activate", |_| {
            anyhow::bail!("attach-before-activate");
        });
        failpoint_support::sleep_millis_async!("attach-before-activate-sleep", &self.cancel);

        info!("Done");

        Ok(())
    }

    /// Check for any local timeline directories that are temporary, or do not correspond to a
    /// timeline that still exists: this can happen if we crashed during a deletion/creation, or
    /// if a timeline was deleted while the tenant was attached to a different pageserver.
    fn clean_up_timelines(&self, existent_timelines: &HashSet<TimelineId>) -> anyhow::Result<()> {
        let timelines_dir = self.conf.timelines_path(&self.tenant_shard_id);

        let entries = match timelines_dir.read_dir_utf8() {
            Ok(d) => d,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(());
                } else {
                    return Err(e).context("list timelines directory for tenant");
                }
            }
        };

        for entry in entries {
            let entry = entry.context("read timeline dir entry")?;
            let entry_path = entry.path();

            let purge = if crate::is_temporary(entry_path)
                // TODO: uninit_mark isn't needed any more, since uninitialized timelines are already
                // covered by the check that the timeline must exist in remote storage.
                || is_uninit_mark(entry_path)
                || crate::is_delete_mark(entry_path)
            {
                true
            } else {
                match TimelineId::try_from(entry_path.file_name()) {
                    Ok(i) => {
                        // Purge if the timeline ID does not exist in remote storage: remote storage is the authority.
                        !existent_timelines.contains(&i)
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Unparseable directory in timelines directory: {entry_path}, ignoring ({e})"
                        );
                        // Do not purge junk: if we don't recognize it, be cautious and leave it for a human.
                        false
                    }
                }
            };

            if purge {
                tracing::info!("Purging stale timeline dentry {entry_path}");
                if let Err(e) = match entry.file_type() {
                    Ok(t) => if t.is_dir() {
                        std::fs::remove_dir_all(entry_path)
                    } else {
                        std::fs::remove_file(entry_path)
                    }
                    .or_else(fs_ext::ignore_not_found),
                    Err(e) => Err(e),
                } {
                    tracing::warn!("Failed to purge stale timeline dentry {entry_path}: {e}");
                }
            }
        }

        Ok(())
    }

    /// Get sum of all remote timelines sizes
    ///
    /// This function relies on the index_part instead of listing the remote storage
    pub fn remote_size(&self) -> u64 {
        let mut size = 0;

        for timeline in self.list_timelines() {
            if let Some(remote_client) = &timeline.remote_client {
                size += remote_client.get_remote_physical_size();
            }
        }

        size
    }

    #[instrument(skip_all, fields(timeline_id=%timeline_id))]
    async fn load_remote_timeline(
        &self,
        timeline_id: TimelineId,
        index_part: IndexPart,
        remote_metadata: TimelineMetadata,
        resources: TimelineResources,
        ctx: &RequestContext,
    ) -> anyhow::Result<()> {
        span::debug_assert_current_span_has_tenant_id();

        info!("downloading index file for timeline {}", timeline_id);
        tokio::fs::create_dir_all(self.conf.timeline_path(&self.tenant_shard_id, &timeline_id))
            .await
            .context("Failed to create new timeline directory")?;

        let ancestor = if let Some(ancestor_id) = remote_metadata.ancestor_timeline() {
            let timelines = self.timelines.lock().unwrap();
            Some(Arc::clone(timelines.get(&ancestor_id).ok_or_else(
                || {
                    anyhow::anyhow!(
                        "cannot find ancestor timeline {ancestor_id} for timeline {timeline_id}"
                    )
                },
            )?))
        } else {
            None
        };

        self.timeline_init_and_sync(
            timeline_id,
            resources,
            Some(index_part),
            remote_metadata,
            ancestor,
            ctx,
        )
        .await
    }

    /// Create a placeholder Tenant object for a broken tenant
    pub fn create_broken_tenant(
        conf: &'static PageServerConf,
        tenant_shard_id: TenantShardId,
        reason: String,
    ) -> Arc<Tenant> {
        Arc::new(Tenant::new(
            TenantState::Broken {
                reason,
                backtrace: String::new(),
            },
            conf,
            AttachedTenantConf::try_from(LocationConf::default()).unwrap(),
            // Shard identity isn't meaningful for a broken tenant: it's just a placeholder
            // to occupy the slot for this TenantShardId.
            ShardIdentity::broken(tenant_shard_id.shard_number, tenant_shard_id.shard_count),
            None,
            tenant_shard_id,
            None,
            DeletionQueueClient::broken(),
        ))
    }

    async fn load_timeline_metadata(
        self: &Arc<Tenant>,
        timeline_ids: HashSet<TimelineId>,
        remote_storage: &GenericRemoteStorage,
        cancel: CancellationToken,
    ) -> anyhow::Result<HashMap<TimelineId, TimelinePreload>> {
        let mut part_downloads = JoinSet::new();
        for timeline_id in timeline_ids {
            let client = RemoteTimelineClient::new(
                remote_storage.clone(),
                self.deletion_queue_client.clone(),
                self.conf,
                self.tenant_shard_id,
                timeline_id,
                self.generation,
            );
            let cancel_clone = cancel.clone();
            part_downloads.spawn(
                async move {
                    debug!("starting index part download");

                    let index_part = client.download_index_file(&cancel_clone).await;

                    debug!("finished index part download");

                    Result::<_, anyhow::Error>::Ok(TimelinePreload {
                        client,
                        timeline_id,
                        index_part,
                    })
                }
                .map(move |res| {
                    res.with_context(|| format!("download index part for timeline {timeline_id}"))
                })
                .instrument(info_span!("download_index_part", %timeline_id)),
            );
        }

        let mut timeline_preloads: HashMap<TimelineId, TimelinePreload> = HashMap::new();

        loop {
            tokio::select!(
                next = part_downloads.join_next() => {
                    match next {
                        Some(result) => {
                            let preload_result = result.context("join preload task")?;
                            let preload = preload_result?;
                            timeline_preloads.insert(preload.timeline_id, preload);
                        },
                        None => {
                            break;
                        }
                    }
                },
                _ = cancel.cancelled() => {
                    anyhow::bail!("Cancelled while waiting for remote index download")
                }
            )
        }

        Ok(timeline_preloads)
    }

    pub(crate) fn tenant_shard_id(&self) -> TenantShardId {
        self.tenant_shard_id
    }

    /// Get Timeline handle for given Neon timeline ID.
    /// This function is idempotent. It doesn't change internal state in any way.
    pub fn get_timeline(
        &self,
        timeline_id: TimelineId,
        active_only: bool,
    ) -> Result<Arc<Timeline>, GetTimelineError> {
        let timelines_accessor = self.timelines.lock().unwrap();
        let timeline = timelines_accessor
            .get(&timeline_id)
            .ok_or(GetTimelineError::NotFound {
                tenant_id: self.tenant_shard_id,
                timeline_id,
            })?;

        if active_only && !timeline.is_active() {
            Err(GetTimelineError::NotActive {
                tenant_id: self.tenant_shard_id,
                timeline_id,
                state: timeline.current_state(),
            })
        } else {
            Ok(Arc::clone(timeline))
        }
    }

    /// Lists timelines the tenant contains.
    /// Up to tenant's implementation to omit certain timelines that ar not considered ready for use.
    pub fn list_timelines(&self) -> Vec<Arc<Timeline>> {
        self.timelines
            .lock()
            .unwrap()
            .values()
            .map(Arc::clone)
            .collect()
    }

    pub fn list_timeline_ids(&self) -> Vec<TimelineId> {
        self.timelines.lock().unwrap().keys().cloned().collect()
    }

    /// This is used to create the initial 'main' timeline during bootstrapping,
    /// or when importing a new base backup. The caller is expected to load an
    /// initial image of the datadir to the new timeline after this.
    ///
    /// Until that happens, the on-disk state is invalid (disk_consistent_lsn=Lsn(0))
    /// and the timeline will fail to load at a restart.
    ///
    /// That's why we add an uninit mark file, and wrap it together witht the Timeline
    /// in-memory object into UninitializedTimeline.
    /// Once the caller is done setting up the timeline, they should call
    /// `UninitializedTimeline::initialize_with_lock` to remove the uninit mark.
    ///
    /// For tests, use `DatadirModification::init_empty_test_timeline` + `commit` to setup the
    /// minimum amount of keys required to get a writable timeline.
    /// (Without it, `put` might fail due to `repartition` failing.)
    pub(crate) async fn create_empty_timeline(
        &self,
        new_timeline_id: TimelineId,
        initdb_lsn: Lsn,
        pg_version: u32,
        _ctx: &RequestContext,
    ) -> anyhow::Result<UninitializedTimeline> {
        anyhow::ensure!(
            self.is_active(),
            "Cannot create empty timelines on inactive tenant"
        );

        let timeline_uninit_mark = self.create_timeline_uninit_mark(new_timeline_id)?;
        let new_metadata = TimelineMetadata::new(
            // Initialize disk_consistent LSN to 0, The caller must import some data to
            // make it valid, before calling finish_creation()
            Lsn(0),
            None,
            None,
            Lsn(0),
            initdb_lsn,
            initdb_lsn,
            pg_version,
        );
        self.prepare_new_timeline(
            new_timeline_id,
            &new_metadata,
            timeline_uninit_mark,
            initdb_lsn,
            None,
        )
        .await
    }

    /// Helper for unit tests to create an empty timeline.
    ///
    /// The timeline is has state value `Active` but its background loops are not running.
    // This makes the various functions which anyhow::ensure! for Active state work in tests.
    // Our current tests don't need the background loops.
    #[cfg(test)]
    pub async fn create_test_timeline(
        &self,
        new_timeline_id: TimelineId,
        initdb_lsn: Lsn,
        pg_version: u32,
        ctx: &RequestContext,
    ) -> anyhow::Result<Arc<Timeline>> {
        let uninit_tl = self
            .create_empty_timeline(new_timeline_id, initdb_lsn, pg_version, ctx)
            .await?;
        let tline = uninit_tl.raw_timeline().expect("we just created it");
        assert_eq!(tline.get_last_record_lsn(), Lsn(0));

        // Setup minimum keys required for the timeline to be usable.
        let mut modification = tline.begin_modification(initdb_lsn);
        modification
            .init_empty_test_timeline()
            .context("init_empty_test_timeline")?;
        modification
            .commit(ctx)
            .await
            .context("commit init_empty_test_timeline modification")?;

        // Flush to disk so that uninit_tl's check for valid disk_consistent_lsn passes.
        tline.maybe_spawn_flush_loop();
        tline.freeze_and_flush().await.context("freeze_and_flush")?;

        // Make sure the freeze_and_flush reaches remote storage.
        tline
            .remote_client
            .as_ref()
            .unwrap()
            .wait_completion()
            .await
            .unwrap();

        let tl = uninit_tl.finish_creation()?;
        // The non-test code would call tl.activate() here.
        tl.set_state(TimelineState::Active);
        Ok(tl)
    }

    /// Create a new timeline.
    ///
    /// Returns the new timeline ID and reference to its Timeline object.
    ///
    /// If the caller specified the timeline ID to use (`new_timeline_id`), and timeline with
    /// the same timeline ID already exists, returns CreateTimelineError::AlreadyExists.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_timeline(
        &self,
        new_timeline_id: TimelineId,
        ancestor_timeline_id: Option<TimelineId>,
        mut ancestor_start_lsn: Option<Lsn>,
        pg_version: u32,
        load_existing_initdb: Option<TimelineId>,
        broker_client: storage_broker::BrokerClientChannel,
        ctx: &RequestContext,
    ) -> Result<Arc<Timeline>, CreateTimelineError> {
        if !self.is_active() {
            if matches!(self.current_state(), TenantState::Stopping { .. }) {
                return Err(CreateTimelineError::ShuttingDown);
            } else {
                return Err(CreateTimelineError::Other(anyhow::anyhow!(
                    "Cannot create timelines on inactive tenant"
                )));
            }
        }

        let _gate = self
            .gate
            .enter()
            .map_err(|_| CreateTimelineError::ShuttingDown)?;

        // Get exclusive access to the timeline ID: this ensures that it does not already exist,
        // and that no other creation attempts will be allowed in while we are working.  The
        // uninit_mark is a guard.
        let uninit_mark = match self.create_timeline_uninit_mark(new_timeline_id) {
            Ok(m) => m,
            Err(TimelineExclusionError::AlreadyCreating) => {
                // Creation is in progress, we cannot create it again, and we cannot
                // check if this request matches the existing one, so caller must try
                // again later.
                return Err(CreateTimelineError::AlreadyCreating);
            }
            Err(TimelineExclusionError::Other(e)) => {
                return Err(CreateTimelineError::Other(e));
            }
            Err(TimelineExclusionError::AlreadyExists(existing)) => {
                debug!("timeline {new_timeline_id} already exists");

                // Idempotency: creating the same timeline twice is not an error, unless
                // the second creation has different parameters.
                if existing.get_ancestor_timeline_id() != ancestor_timeline_id
                    || existing.pg_version != pg_version
                    || (ancestor_start_lsn.is_some()
                        && ancestor_start_lsn != Some(existing.get_ancestor_lsn()))
                {
                    return Err(CreateTimelineError::Conflict);
                }

                if let Some(remote_client) = existing.remote_client.as_ref() {
                    // Wait for uploads to complete, so that when we return Ok, the timeline
                    // is known to be durable on remote storage. Just like we do at the end of
                    // this function, after we have created the timeline ourselves.
                    //
                    // We only really care that the initial version of `index_part.json` has
                    // been uploaded. That's enough to remember that the timeline
                    // exists. However, there is no function to wait specifically for that so
                    // we just wait for all in-progress uploads to finish.
                    remote_client
                        .wait_completion()
                        .await
                        .context("wait for timeline uploads to complete")?;
                }

                return Ok(existing);
            }
        };

        let loaded_timeline = match ancestor_timeline_id {
            Some(ancestor_timeline_id) => {
                let ancestor_timeline = self
                    .get_timeline(ancestor_timeline_id, false)
                    .context("Cannot branch off the timeline that's not present in pageserver")?;

                // instead of waiting around, just deny the request because ancestor is not yet
                // ready for other purposes either.
                if !ancestor_timeline.is_active() {
                    return Err(CreateTimelineError::AncestorNotActive);
                }

                if let Some(lsn) = ancestor_start_lsn.as_mut() {
                    *lsn = lsn.align();

                    let ancestor_ancestor_lsn = ancestor_timeline.get_ancestor_lsn();
                    if ancestor_ancestor_lsn > *lsn {
                        // can we safely just branch from the ancestor instead?
                        return Err(CreateTimelineError::AncestorLsn(anyhow::anyhow!(
                            "invalid start lsn {} for ancestor timeline {}: less than timeline ancestor lsn {}",
                            lsn,
                            ancestor_timeline_id,
                            ancestor_ancestor_lsn,
                        )));
                    }

                    // Wait for the WAL to arrive and be processed on the parent branch up
                    // to the requested branch point. The repository code itself doesn't
                    // require it, but if we start to receive WAL on the new timeline,
                    // decoding the new WAL might need to look up previous pages, relation
                    // sizes etc. and that would get confused if the previous page versions
                    // are not in the repository yet.
                    ancestor_timeline
                        .wait_lsn(*lsn, ctx)
                        .await
                        .map_err(|e| match e {
                            e @ (WaitLsnError::Timeout(_) | WaitLsnError::BadState) => {
                                CreateTimelineError::AncestorLsn(anyhow::anyhow!(e))
                            }
                            WaitLsnError::Shutdown => CreateTimelineError::ShuttingDown,
                        })?;
                }

                self.branch_timeline(
                    &ancestor_timeline,
                    new_timeline_id,
                    ancestor_start_lsn,
                    uninit_mark,
                    ctx,
                )
                .await?
            }
            None => {
                self.bootstrap_timeline(
                    new_timeline_id,
                    pg_version,
                    load_existing_initdb,
                    uninit_mark,
                    ctx,
                )
                .await?
            }
        };

        // At this point we have dropped our guard on [`Self::timelines_creating`], and
        // the timeline is visible in [`Self::timelines`], but it is _not_ durable yet.  We must
        // not send a success to the caller until it is.  The same applies to handling retries,
        // see the handling of [`TimelineExclusionError::AlreadyExists`] above.
        if let Some(remote_client) = loaded_timeline.remote_client.as_ref() {
            let kind = ancestor_timeline_id
                .map(|_| "branched")
                .unwrap_or("bootstrapped");
            remote_client.wait_completion().await.with_context(|| {
                format!("wait for {} timeline initial uploads to complete", kind)
            })?;
        }

        loaded_timeline.activate(broker_client, None, ctx);

        Ok(loaded_timeline)
    }

    pub(crate) async fn delete_timeline(
        self: Arc<Self>,
        timeline_id: TimelineId,
    ) -> Result<(), DeleteTimelineError> {
        DeleteTimelineFlow::run(&self, timeline_id, false).await?;

        Ok(())
    }

    /// perform one garbage collection iteration, removing old data files from disk.
    /// this function is periodically called by gc task.
    /// also it can be explicitly requested through page server api 'do_gc' command.
    ///
    /// `target_timeline_id` specifies the timeline to GC, or None for all.
    ///
    /// The `horizon` an `pitr` parameters determine how much WAL history needs to be retained.
    /// Also known as the retention period, or the GC cutoff point. `horizon` specifies
    /// the amount of history, as LSN difference from current latest LSN on each timeline.
    /// `pitr` specifies the same as a time difference from the current time. The effective
    /// GC cutoff point is determined conservatively by either `horizon` and `pitr`, whichever
    /// requires more history to be retained.
    //
    pub async fn gc_iteration(
        &self,
        target_timeline_id: Option<TimelineId>,
        horizon: u64,
        pitr: Duration,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<GcResult> {
        // Don't start doing work during shutdown
        if let TenantState::Stopping { .. } = self.current_state() {
            return Ok(GcResult::default());
        }

        // there is a global allowed_error for this
        anyhow::ensure!(
            self.is_active(),
            "Cannot run GC iteration on inactive tenant"
        );

        {
            let conf = self.tenant_conf.read().unwrap();

            if !conf.location.may_delete_layers_hint() {
                info!("Skipping GC in location state {:?}", conf.location);
                return Ok(GcResult::default());
            }
        }

        self.gc_iteration_internal(target_timeline_id, horizon, pitr, cancel, ctx)
            .await
    }

    /// Perform one compaction iteration.
    /// This function is periodically called by compactor task.
    /// Also it can be explicitly requested per timeline through page server
    /// api's 'compact' command.
    async fn compaction_iteration(
        &self,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<(), timeline::CompactionError> {
        // Don't start doing work during shutdown, or when broken, we do not need those in the logs
        if !self.is_active() {
            return Ok(());
        }

        {
            let conf = self.tenant_conf.read().unwrap();
            if !conf.location.may_delete_layers_hint() || !conf.location.may_upload_layers_hint() {
                info!("Skipping compaction in location state {:?}", conf.location);
                return Ok(());
            }
        }

        // Scan through the hashmap and collect a list of all the timelines,
        // while holding the lock. Then drop the lock and actually perform the
        // compactions.  We don't want to block everything else while the
        // compaction runs.
        let timelines_to_compact = {
            let timelines = self.timelines.lock().unwrap();
            let timelines_to_compact = timelines
                .iter()
                .filter_map(|(timeline_id, timeline)| {
                    if timeline.is_active() {
                        Some((*timeline_id, timeline.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            drop(timelines);
            timelines_to_compact
        };

        for (timeline_id, timeline) in &timelines_to_compact {
            timeline
                .compact(cancel, EnumSet::empty(), ctx)
                .instrument(info_span!("compact_timeline", %timeline_id))
                .await?;
        }

        Ok(())
    }

    pub fn current_state(&self) -> TenantState {
        self.state.borrow().clone()
    }

    pub fn is_active(&self) -> bool {
        self.current_state() == TenantState::Active
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub(crate) fn wal_redo_manager_status(&self) -> Option<WalRedoManagerStatus> {
        self.walredo_mgr.as_ref().and_then(|mgr| mgr.status())
    }

    /// Changes tenant status to active, unless shutdown was already requested.
    ///
    /// `background_jobs_can_start` is an optional barrier set to a value during pageserver startup
    /// to delay background jobs. Background jobs can be started right away when None is given.
    fn activate(
        self: &Arc<Self>,
        broker_client: BrokerClientChannel,
        background_jobs_can_start: Option<&completion::Barrier>,
        ctx: &RequestContext,
    ) {
        span::debug_assert_current_span_has_tenant_id();

        let mut activating = false;
        self.state.send_modify(|current_state| {
            use pageserver_api::models::ActivatingFrom;
            match &*current_state {
                TenantState::Activating(_) | TenantState::Active | TenantState::Broken { .. } | TenantState::Stopping { .. } => {
                    panic!("caller is responsible for calling activate() only on Loading / Attaching tenants, got {state:?}", state = current_state);
                }
                TenantState::Loading => {
                    *current_state = TenantState::Activating(ActivatingFrom::Loading);
                }
                TenantState::Attaching => {
                    *current_state = TenantState::Activating(ActivatingFrom::Attaching);
                }
            }
            debug!(tenant_id = %self.tenant_shard_id.tenant_id, shard_id = %self.tenant_shard_id.shard_slug(), "Activating tenant");
            activating = true;
            // Continue outside the closure. We need to grab timelines.lock()
            // and we plan to turn it into a tokio::sync::Mutex in a future patch.
        });

        if activating {
            let timelines_accessor = self.timelines.lock().unwrap();
            let timelines_to_activate = timelines_accessor
                .values()
                .filter(|timeline| !(timeline.is_broken() || timeline.is_stopping()));

            // Spawn gc and compaction loops. The loops will shut themselves
            // down when they notice that the tenant is inactive.
            tasks::start_background_loops(self, background_jobs_can_start);

            let mut activated_timelines = 0;

            for timeline in timelines_to_activate {
                timeline.activate(broker_client.clone(), background_jobs_can_start, ctx);
                activated_timelines += 1;
            }

            self.state.send_modify(move |current_state| {
                assert!(
                    matches!(current_state, TenantState::Activating(_)),
                    "set_stopping and set_broken wait for us to leave Activating state",
                );
                *current_state = TenantState::Active;

                let elapsed = self.constructed_at.elapsed();
                let total_timelines = timelines_accessor.len();

                // log a lot of stuff, because some tenants sometimes suffer from user-visible
                // times to activate. see https://github.com/neondatabase/neon/issues/4025
                info!(
                    since_creation_millis = elapsed.as_millis(),
                    tenant_id = %self.tenant_shard_id.tenant_id,
                    shard_id = %self.tenant_shard_id.shard_slug(),
                    activated_timelines,
                    total_timelines,
                    post_state = <&'static str>::from(&*current_state),
                    "activation attempt finished"
                );

                TENANT.activation.observe(elapsed.as_secs_f64());
            });
        }
    }

    /// Shutdown the tenant and join all of the spawned tasks.
    ///
    /// The method caters for all use-cases:
    /// - pageserver shutdown (freeze_and_flush == true)
    /// - detach + ignore (freeze_and_flush == false)
    ///
    /// This will attempt to shutdown even if tenant is broken.
    ///
    /// `shutdown_progress` is a [`completion::Barrier`] for the shutdown initiated by this call.
    /// If the tenant is already shutting down, we return a clone of the first shutdown call's
    /// `Barrier` as an `Err`. This not-first caller can use the returned barrier to join with
    /// the ongoing shutdown.
    async fn shutdown(
        &self,
        shutdown_progress: completion::Barrier,
        freeze_and_flush: bool,
    ) -> Result<(), completion::Barrier> {
        span::debug_assert_current_span_has_tenant_id();

        // Set tenant (and its timlines) to Stoppping state.
        //
        // Since we can only transition into Stopping state after activation is complete,
        // run it in a JoinSet so all tenants have a chance to stop before we get SIGKILLed.
        //
        // Transitioning tenants to Stopping state has a couple of non-obvious side effects:
        // 1. Lock out any new requests to the tenants.
        // 2. Signal cancellation to WAL receivers (we wait on it below).
        // 3. Signal cancellation for other tenant background loops.
        // 4. ???
        //
        // The waiting for the cancellation is not done uniformly.
        // We certainly wait for WAL receivers to shut down.
        // That is necessary so that no new data comes in before the freeze_and_flush.
        // But the tenant background loops are joined-on in our caller.
        // It's mesed up.
        // we just ignore the failure to stop

        // If we're still attaching, fire the cancellation token early to drop out: this
        // will prevent us flushing, but ensures timely shutdown if some I/O during attach
        // is very slow.
        if matches!(self.current_state(), TenantState::Attaching) {
            self.cancel.cancel();
        }

        match self.set_stopping(shutdown_progress, false, false).await {
            Ok(()) => {}
            Err(SetStoppingError::Broken) => {
                // assume that this is acceptable
            }
            Err(SetStoppingError::AlreadyStopping(other)) => {
                // give caller the option to wait for this this shutdown
                info!("Tenant::shutdown: AlreadyStopping");
                return Err(other);
            }
        };

        let mut js = tokio::task::JoinSet::new();
        {
            let timelines = self.timelines.lock().unwrap();
            timelines.values().for_each(|timeline| {
                let timeline = Arc::clone(timeline);
                let timeline_id = timeline.timeline_id;

                let span =
                    tracing::info_span!("timeline_shutdown", %timeline_id, ?freeze_and_flush);
                js.spawn(async move {
                    if freeze_and_flush {
                        timeline.flush_and_shutdown().instrument(span).await
                    } else {
                        timeline.shutdown().instrument(span).await
                    }
                });
            })
        };
        // test_long_timeline_create_then_tenant_delete is leaning on this message
        tracing::info!("Waiting for timelines...");
        while let Some(res) = js.join_next().await {
            match res {
                Ok(()) => {}
                Err(je) if je.is_cancelled() => unreachable!("no cancelling used"),
                Err(je) if je.is_panic() => { /* logged already */ }
                Err(je) => warn!("unexpected JoinError: {je:?}"),
            }
        }

        // We cancel the Tenant's cancellation token _after_ the timelines have all shut down.  This permits
        // them to continue to do work during their shutdown methods, e.g. flushing data.
        tracing::debug!("Cancelling CancellationToken");
        self.cancel.cancel();

        // shutdown all tenant and timeline tasks: gc, compaction, page service
        // No new tasks will be started for this tenant because it's in `Stopping` state.
        //
        // this will additionally shutdown and await all timeline tasks.
        tracing::debug!("Waiting for tasks...");
        task_mgr::shutdown_tasks(None, Some(self.tenant_shard_id), None).await;

        // Wait for any in-flight operations to complete
        self.gate.close().await;

        Ok(())
    }

    /// Change tenant status to Stopping, to mark that it is being shut down.
    ///
    /// This function waits for the tenant to become active if it isn't already, before transitioning it into Stopping state.
    ///
    /// This function is not cancel-safe!
    ///
    /// `allow_transition_from_loading` is needed for the special case of loading task deleting the tenant.
    /// `allow_transition_from_attaching` is needed for the special case of attaching deleted tenant.
    async fn set_stopping(
        &self,
        progress: completion::Barrier,
        allow_transition_from_loading: bool,
        allow_transition_from_attaching: bool,
    ) -> Result<(), SetStoppingError> {
        let mut rx = self.state.subscribe();

        // cannot stop before we're done activating, so wait out until we're done activating
        rx.wait_for(|state| match state {
            TenantState::Attaching if allow_transition_from_attaching => true,
            TenantState::Activating(_) | TenantState::Attaching => {
                info!(
                    "waiting for {} to turn Active|Broken|Stopping",
                    <&'static str>::from(state)
                );
                false
            }
            TenantState::Loading => allow_transition_from_loading,
            TenantState::Active | TenantState::Broken { .. } | TenantState::Stopping { .. } => true,
        })
        .await
        .expect("cannot drop self.state while on a &self method");

        // we now know we're done activating, let's see whether this task is the winner to transition into Stopping
        let mut err = None;
        let stopping = self.state.send_if_modified(|current_state| match current_state {
            TenantState::Activating(_) => {
                unreachable!("1we ensured above that we're done with activation, and, there is no re-activation")
            }
            TenantState::Attaching => {
                if !allow_transition_from_attaching {
                    unreachable!("2we ensured above that we're done with activation, and, there is no re-activation")
                };
                *current_state = TenantState::Stopping { progress };
                true
            }
            TenantState::Loading => {
                if !allow_transition_from_loading {
                    unreachable!("3we ensured above that we're done with activation, and, there is no re-activation")
                };
                *current_state = TenantState::Stopping { progress };
                true
            }
            TenantState::Active => {
                // FIXME: due to time-of-check vs time-of-use issues, it can happen that new timelines
                // are created after the transition to Stopping. That's harmless, as the Timelines
                // won't be accessible to anyone afterwards, because the Tenant is in Stopping state.
                *current_state = TenantState::Stopping { progress };
                // Continue stopping outside the closure. We need to grab timelines.lock()
                // and we plan to turn it into a tokio::sync::Mutex in a future patch.
                true
            }
            TenantState::Broken { reason, .. } => {
                info!(
                    "Cannot set tenant to Stopping state, it is in Broken state due to: {reason}"
                );
                err = Some(SetStoppingError::Broken);
                false
            }
            TenantState::Stopping { progress } => {
                info!("Tenant is already in Stopping state");
                err = Some(SetStoppingError::AlreadyStopping(progress.clone()));
                false
            }
        });
        match (stopping, err) {
            (true, None) => {} // continue
            (false, Some(err)) => return Err(err),
            (true, Some(_)) => unreachable!(
                "send_if_modified closure must error out if not transitioning to Stopping"
            ),
            (false, None) => unreachable!(
                "send_if_modified closure must return true if transitioning to Stopping"
            ),
        }

        let timelines_accessor = self.timelines.lock().unwrap();
        let not_broken_timelines = timelines_accessor
            .values()
            .filter(|timeline| !timeline.is_broken());
        for timeline in not_broken_timelines {
            timeline.set_state(TimelineState::Stopping);
        }
        Ok(())
    }

    /// Method for tenant::mgr to transition us into Broken state in case of a late failure in
    /// `remove_tenant_from_memory`
    ///
    /// This function waits for the tenant to become active if it isn't already, before transitioning it into Stopping state.
    ///
    /// In tests, we also use this to set tenants to Broken state on purpose.
    pub(crate) async fn set_broken(&self, reason: String) {
        let mut rx = self.state.subscribe();

        // The load & attach routines own the tenant state until it has reached `Active`.
        // So, wait until it's done.
        rx.wait_for(|state| match state {
            TenantState::Activating(_) | TenantState::Loading | TenantState::Attaching => {
                info!(
                    "waiting for {} to turn Active|Broken|Stopping",
                    <&'static str>::from(state)
                );
                false
            }
            TenantState::Active | TenantState::Broken { .. } | TenantState::Stopping { .. } => true,
        })
        .await
        .expect("cannot drop self.state while on a &self method");

        // we now know we're done activating, let's see whether this task is the winner to transition into Broken
        self.set_broken_no_wait(reason)
    }

    pub(crate) fn set_broken_no_wait(&self, reason: impl Display) {
        let reason = reason.to_string();
        self.state.send_modify(|current_state| {
            match *current_state {
                TenantState::Activating(_) | TenantState::Loading | TenantState::Attaching => {
                    unreachable!("we ensured above that we're done with activation, and, there is no re-activation")
                }
                TenantState::Active => {
                    if cfg!(feature = "testing") {
                        warn!("Changing Active tenant to Broken state, reason: {}", reason);
                        *current_state = TenantState::broken_from_reason(reason);
                    } else {
                        unreachable!("not allowed to call set_broken on Active tenants in non-testing builds")
                    }
                }
                TenantState::Broken { .. } => {
                    warn!("Tenant is already in Broken state");
                }
                // This is the only "expected" path, any other path is a bug.
                TenantState::Stopping { .. } => {
                    warn!(
                        "Marking Stopping tenant as Broken state, reason: {}",
                        reason
                    );
                    *current_state = TenantState::broken_from_reason(reason);
                }
           }
        });
    }

    pub fn subscribe_for_state_updates(&self) -> watch::Receiver<TenantState> {
        self.state.subscribe()
    }

    /// The activate_now semaphore is initialized with zero units.  As soon as
    /// we add a unit, waiters will be able to acquire a unit and proceed.
    pub(crate) fn activate_now(&self) {
        self.activate_now_sem.add_permits(1);
    }

    pub(crate) async fn wait_to_become_active(
        &self,
        timeout: Duration,
    ) -> Result<(), GetActiveTenantError> {
        let mut receiver = self.state.subscribe();
        loop {
            let current_state = receiver.borrow_and_update().clone();
            match current_state {
                TenantState::Loading | TenantState::Attaching | TenantState::Activating(_) => {
                    // in these states, there's a chance that we can reach ::Active
                    self.activate_now();
                    match timeout_cancellable(timeout, &self.cancel, receiver.changed()).await {
                        Ok(r) => {
                            r.map_err(
                            |_e: tokio::sync::watch::error::RecvError|
                                // Tenant existed but was dropped: report it as non-existent
                                GetActiveTenantError::NotFound(GetTenantError::NotFound(self.tenant_shard_id.tenant_id))
                        )?
                        }
                        Err(TimeoutCancellableError::Cancelled) => {
                            return Err(GetActiveTenantError::Cancelled);
                        }
                        Err(TimeoutCancellableError::Timeout) => {
                            return Err(GetActiveTenantError::WaitForActiveTimeout {
                                latest_state: Some(self.current_state()),
                                wait_time: timeout,
                            });
                        }
                    }
                }
                TenantState::Active { .. } => {
                    return Ok(());
                }
                TenantState::Broken { .. } | TenantState::Stopping { .. } => {
                    // There's no chance the tenant can transition back into ::Active
                    return Err(GetActiveTenantError::WillNotBecomeActive(current_state));
                }
            }
        }
    }

    pub(crate) fn get_attach_mode(&self) -> AttachmentMode {
        self.tenant_conf.read().unwrap().location.attach_mode
    }

    /// For API access: generate a LocationConfig equivalent to the one that would be used to
    /// create a Tenant in the same state.  Do not use this in hot paths: it's for relatively
    /// rare external API calls, like a reconciliation at startup.
    pub(crate) fn get_location_conf(&self) -> models::LocationConfig {
        let conf = self.tenant_conf.read().unwrap();

        let location_config_mode = match conf.location.attach_mode {
            AttachmentMode::Single => models::LocationConfigMode::AttachedSingle,
            AttachmentMode::Multi => models::LocationConfigMode::AttachedMulti,
            AttachmentMode::Stale => models::LocationConfigMode::AttachedStale,
        };

        // We have a pageserver TenantConf, we need the API-facing TenantConfig.
        let tenant_config: models::TenantConfig = conf.tenant_conf.clone().into();

        models::LocationConfig {
            mode: location_config_mode,
            generation: self.generation.into(),
            secondary_conf: None,
            shard_number: self.shard_identity.number.0,
            shard_count: self.shard_identity.count.literal(),
            shard_stripe_size: self.shard_identity.stripe_size.0,
            tenant_conf: tenant_config,
        }
    }

    pub(crate) fn get_tenant_shard_id(&self) -> &TenantShardId {
        &self.tenant_shard_id
    }

    pub(crate) fn get_generation(&self) -> Generation {
        self.generation
    }

    /// This function partially shuts down the tenant (it shuts down the Timelines) and is fallible,
    /// and can leave the tenant in a bad state if it fails.  The caller is responsible for
    /// resetting this tenant to a valid state if we fail.
    pub(crate) async fn split_prepare(
        &self,
        child_shards: &Vec<TenantShardId>,
    ) -> anyhow::Result<()> {
        let timelines = self.timelines.lock().unwrap().clone();
        for timeline in timelines.values() {
            let Some(tl_client) = &timeline.remote_client else {
                anyhow::bail!("Remote storage is mandatory");
            };

            let Some(remote_storage) = &self.remote_storage else {
                anyhow::bail!("Remote storage is mandatory");
            };

            // We do not block timeline creation/deletion during splits inside the pageserver: it is up to higher levels
            // to ensure that they do not start a split if currently in the process of doing these.

            // Upload an index from the parent: this is partly to provide freshness for the
            // child tenants that will copy it, and partly for general ease-of-debugging: there will
            // always be a parent shard index in the same generation as we wrote the child shard index.
            tl_client.schedule_index_upload_for_file_changes()?;
            tl_client.wait_completion().await?;

            // Shut down the timeline's remote client: this means that the indices we write
            // for child shards will not be invalidated by the parent shard deleting layers.
            tl_client.shutdown().await?;

            // Download methods can still be used after shutdown, as they don't flow through the remote client's
            // queue.  In principal the RemoteTimelineClient could provide this without downloading it, but this
            // operation is rare, so it's simpler to just download it (and robustly guarantees that the index
            // we use here really is the remotely persistent one).
            let result = tl_client
                .download_index_file(&self.cancel)
                .instrument(info_span!("download_index_file", tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug(), timeline_id=%timeline.timeline_id))
                .await?;
            let index_part = match result {
                MaybeDeletedIndexPart::Deleted(_) => {
                    anyhow::bail!("Timeline deletion happened concurrently with split")
                }
                MaybeDeletedIndexPart::IndexPart(p) => p,
            };

            for child_shard in child_shards {
                upload_index_part(
                    remote_storage,
                    child_shard,
                    &timeline.timeline_id,
                    self.generation,
                    &index_part,
                    &self.cancel,
                )
                .await?;
            }
        }

        Ok(())
    }
}

/// Given a Vec of timelines and their ancestors (timeline_id, ancestor_id),
/// perform a topological sort, so that the parent of each timeline comes
/// before the children.
/// E extracts the ancestor from T
/// This allows for T to be different. It can be TimelineMetadata, can be Timeline itself, etc.
fn tree_sort_timelines<T, E>(
    timelines: HashMap<TimelineId, T>,
    extractor: E,
) -> anyhow::Result<Vec<(TimelineId, T)>>
where
    E: Fn(&T) -> Option<TimelineId>,
{
    let mut result = Vec::with_capacity(timelines.len());

    let mut now = Vec::with_capacity(timelines.len());
    // (ancestor, children)
    let mut later: HashMap<TimelineId, Vec<(TimelineId, T)>> =
        HashMap::with_capacity(timelines.len());

    for (timeline_id, value) in timelines {
        if let Some(ancestor_id) = extractor(&value) {
            let children = later.entry(ancestor_id).or_default();
            children.push((timeline_id, value));
        } else {
            now.push((timeline_id, value));
        }
    }

    while let Some((timeline_id, metadata)) = now.pop() {
        result.push((timeline_id, metadata));
        // All children of this can be loaded now
        if let Some(mut children) = later.remove(&timeline_id) {
            now.append(&mut children);
        }
    }

    // All timelines should be visited now. Unless there were timelines with missing ancestors.
    if !later.is_empty() {
        for (missing_id, orphan_ids) in later {
            for (orphan_id, _) in orphan_ids {
                error!("could not load timeline {orphan_id} because its ancestor timeline {missing_id} could not be loaded");
            }
        }
        bail!("could not load tenant because some timelines are missing ancestors");
    }

    Ok(result)
}

impl Tenant {
    pub fn tenant_specific_overrides(&self) -> TenantConfOpt {
        self.tenant_conf.read().unwrap().tenant_conf.clone()
    }

    pub fn effective_config(&self) -> TenantConf {
        self.tenant_specific_overrides()
            .merge(self.conf.default_tenant_conf.clone())
    }

    pub fn get_checkpoint_distance(&self) -> u64 {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .checkpoint_distance
            .unwrap_or(self.conf.default_tenant_conf.checkpoint_distance)
    }

    pub fn get_checkpoint_timeout(&self) -> Duration {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .checkpoint_timeout
            .unwrap_or(self.conf.default_tenant_conf.checkpoint_timeout)
    }

    pub fn get_compaction_target_size(&self) -> u64 {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .compaction_target_size
            .unwrap_or(self.conf.default_tenant_conf.compaction_target_size)
    }

    pub fn get_compaction_period(&self) -> Duration {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .compaction_period
            .unwrap_or(self.conf.default_tenant_conf.compaction_period)
    }

    pub fn get_compaction_threshold(&self) -> usize {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .compaction_threshold
            .unwrap_or(self.conf.default_tenant_conf.compaction_threshold)
    }

    pub fn get_gc_horizon(&self) -> u64 {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .gc_horizon
            .unwrap_or(self.conf.default_tenant_conf.gc_horizon)
    }

    pub fn get_gc_period(&self) -> Duration {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .gc_period
            .unwrap_or(self.conf.default_tenant_conf.gc_period)
    }

    pub fn get_image_creation_threshold(&self) -> usize {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .image_creation_threshold
            .unwrap_or(self.conf.default_tenant_conf.image_creation_threshold)
    }

    pub fn get_pitr_interval(&self) -> Duration {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .pitr_interval
            .unwrap_or(self.conf.default_tenant_conf.pitr_interval)
    }

    pub fn get_trace_read_requests(&self) -> bool {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .trace_read_requests
            .unwrap_or(self.conf.default_tenant_conf.trace_read_requests)
    }

    pub fn get_min_resident_size_override(&self) -> Option<u64> {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        tenant_conf
            .min_resident_size_override
            .or(self.conf.default_tenant_conf.min_resident_size_override)
    }

    pub fn get_heatmap_period(&self) -> Option<Duration> {
        let tenant_conf = self.tenant_conf.read().unwrap().tenant_conf.clone();
        let heatmap_period = tenant_conf
            .heatmap_period
            .unwrap_or(self.conf.default_tenant_conf.heatmap_period);
        if heatmap_period.is_zero() {
            None
        } else {
            Some(heatmap_period)
        }
    }

    pub fn set_new_tenant_config(&self, new_tenant_conf: TenantConfOpt) {
        self.tenant_conf.write().unwrap().tenant_conf = new_tenant_conf;
        self.tenant_conf_updated();
        // Don't hold self.timelines.lock() during the notifies.
        // There's no risk of deadlock right now, but there could be if we consolidate
        // mutexes in struct Timeline in the future.
        let timelines = self.list_timelines();
        for timeline in timelines {
            timeline.tenant_conf_updated();
        }
    }

    pub(crate) fn set_new_location_config(&self, new_conf: AttachedTenantConf) {
        *self.tenant_conf.write().unwrap() = new_conf;
        self.tenant_conf_updated();
        // Don't hold self.timelines.lock() during the notifies.
        // There's no risk of deadlock right now, but there could be if we consolidate
        // mutexes in struct Timeline in the future.
        let timelines = self.list_timelines();
        for timeline in timelines {
            timeline.tenant_conf_updated();
        }
    }

    fn get_timeline_get_throttle_config(
        psconf: &'static PageServerConf,
        overrides: &TenantConfOpt,
    ) -> throttle::Config {
        overrides
            .timeline_get_throttle
            .clone()
            .unwrap_or(psconf.default_tenant_conf.timeline_get_throttle.clone())
    }

    pub(crate) fn tenant_conf_updated(&self) {
        let conf = {
            let guard = self.tenant_conf.read().unwrap();
            Self::get_timeline_get_throttle_config(self.conf, &guard.tenant_conf)
        };
        self.timeline_get_throttle.reconfigure(conf)
    }

    /// Helper function to create a new Timeline struct.
    ///
    /// The returned Timeline is in Loading state. The caller is responsible for
    /// initializing any on-disk state, and for inserting the Timeline to the 'timelines'
    /// map.
    ///
    /// `validate_ancestor == false` is used when a timeline is created for deletion
    /// and we might not have the ancestor present anymore which is fine for to be
    /// deleted timelines.
    fn create_timeline_struct(
        &self,
        new_timeline_id: TimelineId,
        new_metadata: &TimelineMetadata,
        ancestor: Option<Arc<Timeline>>,
        resources: TimelineResources,
        cause: CreateTimelineCause,
    ) -> anyhow::Result<Arc<Timeline>> {
        let state = match cause {
            CreateTimelineCause::Load => {
                let ancestor_id = new_metadata.ancestor_timeline();
                anyhow::ensure!(
                    ancestor_id == ancestor.as_ref().map(|t| t.timeline_id),
                    "Timeline's {new_timeline_id} ancestor {ancestor_id:?} was not found"
                );
                TimelineState::Loading
            }
            CreateTimelineCause::Delete => TimelineState::Stopping,
        };

        let pg_version = new_metadata.pg_version();

        let timeline = Timeline::new(
            self.conf,
            Arc::clone(&self.tenant_conf),
            new_metadata,
            ancestor,
            new_timeline_id,
            self.tenant_shard_id,
            self.generation,
            self.shard_identity,
            self.walredo_mgr.as_ref().map(Arc::clone),
            resources,
            pg_version,
            state,
            self.cancel.child_token(),
        );

        Ok(timeline)
    }

    // Allow too_many_arguments because a constructor's argument list naturally grows with the
    // number of attributes in the struct: breaking these out into a builder wouldn't be helpful.
    #[allow(clippy::too_many_arguments)]
    fn new(
        state: TenantState,
        conf: &'static PageServerConf,
        attached_conf: AttachedTenantConf,
        shard_identity: ShardIdentity,
        walredo_mgr: Option<Arc<WalRedoManager>>,
        tenant_shard_id: TenantShardId,
        remote_storage: Option<GenericRemoteStorage>,
        deletion_queue_client: DeletionQueueClient,
    ) -> Tenant {
        let (state, mut rx) = watch::channel(state);

        tokio::spawn(async move {
            // reflect tenant state in metrics:
            // - global per tenant state: TENANT_STATE_METRIC
            // - "set" of broken tenants: BROKEN_TENANTS_SET
            //
            // set of broken tenants should not have zero counts so that it remains accessible for
            // alerting.

            let tid = tenant_shard_id.to_string();
            let shard_id = tenant_shard_id.shard_slug().to_string();
            let set_key = &[tid.as_str(), shard_id.as_str()][..];

            fn inspect_state(state: &TenantState) -> ([&'static str; 1], bool) {
                ([state.into()], matches!(state, TenantState::Broken { .. }))
            }

            let mut tuple = inspect_state(&rx.borrow_and_update());

            let is_broken = tuple.1;
            let mut counted_broken = if is_broken {
                // add the id to the set right away, there should not be any updates on the channel
                // after before tenant is removed, if ever
                BROKEN_TENANTS_SET.with_label_values(set_key).set(1);
                true
            } else {
                false
            };

            loop {
                let labels = &tuple.0;
                let current = TENANT_STATE_METRIC.with_label_values(labels);
                current.inc();

                if rx.changed().await.is_err() {
                    // tenant has been dropped
                    current.dec();
                    drop(BROKEN_TENANTS_SET.remove_label_values(set_key));
                    break;
                }

                current.dec();
                tuple = inspect_state(&rx.borrow_and_update());

                let is_broken = tuple.1;
                if is_broken && !counted_broken {
                    counted_broken = true;
                    // insert the tenant_id (back) into the set while avoiding needless counter
                    // access
                    BROKEN_TENANTS_SET.with_label_values(set_key).set(1);
                }
            }
        });

        Tenant {
            tenant_shard_id,
            shard_identity,
            generation: attached_conf.location.generation,
            conf,
            // using now here is good enough approximation to catch tenants with really long
            // activation times.
            constructed_at: Instant::now(),
            timelines: Mutex::new(HashMap::new()),
            timelines_creating: Mutex::new(HashSet::new()),
            gc_cs: tokio::sync::Mutex::new(()),
            walredo_mgr,
            remote_storage,
            deletion_queue_client,
            state,
            cached_logical_sizes: tokio::sync::Mutex::new(HashMap::new()),
            cached_synthetic_tenant_size: Arc::new(AtomicU64::new(0)),
            eviction_task_tenant_state: tokio::sync::Mutex::new(EvictionTaskTenantState::default()),
            activate_now_sem: tokio::sync::Semaphore::new(0),
            delete_progress: Arc::new(tokio::sync::Mutex::new(DeleteTenantFlow::default())),
            cancel: CancellationToken::default(),
            gate: Gate::default(),
            timeline_get_throttle: Arc::new(throttle::Throttle::new(
                Tenant::get_timeline_get_throttle_config(conf, &attached_conf.tenant_conf),
                &crate::metrics::tenant_throttling::TIMELINE_GET,
            )),
            tenant_conf: Arc::new(RwLock::new(attached_conf)),
        }
    }

    /// Locate and load config
    pub(super) fn load_tenant_config(
        conf: &'static PageServerConf,
        tenant_shard_id: &TenantShardId,
    ) -> anyhow::Result<LocationConf> {
        let legacy_config_path = conf.tenant_config_path(tenant_shard_id);
        let config_path = conf.tenant_location_config_path(tenant_shard_id);

        if config_path.exists() {
            // New-style config takes precedence
            let deserialized = Self::read_config(&config_path)?;
            Ok(toml_edit::de::from_document::<LocationConf>(deserialized)?)
        } else if legacy_config_path.exists() {
            // Upgrade path: found an old-style configuration only
            let deserialized = Self::read_config(&legacy_config_path)?;

            let mut tenant_conf = TenantConfOpt::default();
            for (key, item) in deserialized.iter() {
                match key {
                    "tenant_config" => {
                        tenant_conf = TenantConfOpt::try_from(item.to_owned()).context(format!("Failed to parse config from file '{legacy_config_path}' as pageserver config"))?;
                    }
                    _ => bail!(
                        "config file {legacy_config_path} has unrecognized pageserver option '{key}'"
                    ),
                }
            }

            // Legacy configs are implicitly in attached state, and do not support sharding
            Ok(LocationConf::attached_single(
                tenant_conf,
                Generation::none(),
                &models::ShardParameters::default(),
            ))
        } else {
            // FIXME If the config file is not found, assume that we're attaching
            // a detached tenant and config is passed via attach command.
            // https://github.com/neondatabase/neon/issues/1555
            // OR: we're loading after incomplete deletion that managed to remove config.
            info!(
                "tenant config not found in {} or {}",
                config_path, legacy_config_path
            );
            Ok(LocationConf::default())
        }
    }

    fn read_config(path: &Utf8Path) -> anyhow::Result<toml_edit::Document> {
        info!("loading tenant configuration from {path}");

        // load and parse file
        let config = fs::read_to_string(path)
            .with_context(|| format!("Failed to load config from path '{path}'"))?;

        config
            .parse::<toml_edit::Document>()
            .with_context(|| format!("Failed to parse config from file '{path}' as toml file"))
    }

    #[tracing::instrument(skip_all, fields(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug()))]
    pub(super) async fn persist_tenant_config(
        conf: &'static PageServerConf,
        tenant_shard_id: &TenantShardId,
        location_conf: &LocationConf,
    ) -> anyhow::Result<()> {
        let legacy_config_path = conf.tenant_config_path(tenant_shard_id);
        let config_path = conf.tenant_location_config_path(tenant_shard_id);

        Self::persist_tenant_config_at(
            tenant_shard_id,
            &config_path,
            &legacy_config_path,
            location_conf,
        )
        .await
    }

    #[tracing::instrument(skip_all, fields(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug()))]
    pub(super) async fn persist_tenant_config_at(
        tenant_shard_id: &TenantShardId,
        config_path: &Utf8Path,
        legacy_config_path: &Utf8Path,
        location_conf: &LocationConf,
    ) -> anyhow::Result<()> {
        // Forward compat: write out an old-style configuration that old versions can read, in case we roll back
        Self::persist_tenant_config_legacy(
            tenant_shard_id,
            legacy_config_path,
            &location_conf.tenant_conf,
        )
        .await?;

        if let LocationMode::Attached(attach_conf) = &location_conf.mode {
            // Once we use LocationMode, generations are mandatory.  If we aren't using generations,
            // then drop out after writing legacy-style config.
            if attach_conf.generation.is_none() {
                tracing::debug!("Running without generations, not writing new-style LocationConf");
                return Ok(());
            }
        }

        debug!("persisting tenantconf to {config_path}");

        let mut conf_content = r#"# This file contains a specific per-tenant's config.
#  It is read in case of pageserver restart.
"#
        .to_string();

        fail::fail_point!("tenant-config-before-write", |_| {
            anyhow::bail!("tenant-config-before-write");
        });

        // Convert the config to a toml file.
        conf_content += &toml_edit::ser::to_string_pretty(&location_conf)?;

        let temp_path = path_with_suffix_extension(config_path, TEMP_FILE_SUFFIX);

        let tenant_shard_id = *tenant_shard_id;
        let config_path = config_path.to_owned();
        let conf_content = conf_content.into_bytes();
        VirtualFile::crashsafe_overwrite(config_path.clone(), temp_path, conf_content)
            .await
            .with_context(|| format!("write tenant {tenant_shard_id} config to {config_path}"))?;

        Ok(())
    }

    #[tracing::instrument(skip_all, fields(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug()))]
    async fn persist_tenant_config_legacy(
        tenant_shard_id: &TenantShardId,
        target_config_path: &Utf8Path,
        tenant_conf: &TenantConfOpt,
    ) -> anyhow::Result<()> {
        debug!("persisting tenantconf to {target_config_path}");

        let mut conf_content = r#"# This file contains a specific per-tenant's config.
#  It is read in case of pageserver restart.

[tenant_config]
"#
        .to_string();

        // Convert the config to a toml file.
        conf_content += &toml_edit::ser::to_string(&tenant_conf)?;

        let temp_path = path_with_suffix_extension(target_config_path, TEMP_FILE_SUFFIX);

        let tenant_shard_id = *tenant_shard_id;
        let target_config_path = target_config_path.to_owned();
        let conf_content = conf_content.into_bytes();
        VirtualFile::crashsafe_overwrite(target_config_path.clone(), temp_path, conf_content)
            .await
            .with_context(|| {
                format!("write tenant {tenant_shard_id} config to {target_config_path}")
            })?;
        Ok(())
    }

    //
    // How garbage collection works:
    //
    //                    +--bar------------->
    //                   /
    //             +----+-----foo---------------->
    //            /
    // ----main--+-------------------------->
    //                \
    //                 +-----baz-------->
    //
    //
    // 1. Grab 'gc_cs' mutex to prevent new timelines from being created while Timeline's
    //    `gc_infos` are being refreshed
    // 2. Scan collected timelines, and on each timeline, make note of the
    //    all the points where other timelines have been branched off.
    //    We will refrain from removing page versions at those LSNs.
    // 3. For each timeline, scan all layer files on the timeline.
    //    Remove all files for which a newer file exists and which
    //    don't cover any branch point LSNs.
    //
    // TODO:
    // - if a relation has a non-incremental persistent layer on a child branch, then we
    //   don't need to keep that in the parent anymore. But currently
    //   we do.
    async fn gc_iteration_internal(
        &self,
        target_timeline_id: Option<TimelineId>,
        horizon: u64,
        pitr: Duration,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<GcResult> {
        let mut totals: GcResult = Default::default();
        let now = Instant::now();

        let gc_timelines = match self
            .refresh_gc_info_internal(target_timeline_id, horizon, pitr, cancel, ctx)
            .await
        {
            Ok(result) => result,
            Err(e) => {
                if let Some(PageReconstructError::Cancelled) =
                    e.downcast_ref::<PageReconstructError>()
                {
                    // Handle cancellation
                    totals.elapsed = now.elapsed();
                    return Ok(totals);
                } else {
                    // Propagate other errors
                    return Err(e);
                }
            }
        };

        failpoint_support::sleep_millis_async!("gc_iteration_internal_after_getting_gc_timelines");

        // If there is nothing to GC, we don't want any messages in the INFO log.
        if !gc_timelines.is_empty() {
            info!("{} timelines need GC", gc_timelines.len());
        } else {
            debug!("{} timelines need GC", gc_timelines.len());
        }

        // Perform GC for each timeline.
        //
        // Note that we don't hold the `Tenant::gc_cs` lock here because we don't want to delay the
        // branch creation task, which requires the GC lock. A GC iteration can run concurrently
        // with branch creation.
        //
        // See comments in [`Tenant::branch_timeline`] for more information about why branch
        // creation task can run concurrently with timeline's GC iteration.
        for timeline in gc_timelines {
            if task_mgr::is_shutdown_requested() || cancel.is_cancelled() {
                // We were requested to shut down. Stop and return with the progress we
                // made.
                break;
            }
            let result = timeline.gc().await?;
            totals += result;
        }

        totals.elapsed = now.elapsed();
        Ok(totals)
    }

    /// Refreshes the Timeline::gc_info for all timelines, returning the
    /// vector of timelines which have [`Timeline::get_last_record_lsn`] past
    /// [`Tenant::get_gc_horizon`].
    ///
    /// This is usually executed as part of periodic gc, but can now be triggered more often.
    pub async fn refresh_gc_info(
        &self,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<Vec<Arc<Timeline>>> {
        // since this method can now be called at different rates than the configured gc loop, it
        // might be that these configuration values get applied faster than what it was previously,
        // since these were only read from the gc task.
        let horizon = self.get_gc_horizon();
        let pitr = self.get_pitr_interval();

        // refresh all timelines
        let target_timeline_id = None;

        self.refresh_gc_info_internal(target_timeline_id, horizon, pitr, cancel, ctx)
            .await
    }

    async fn refresh_gc_info_internal(
        &self,
        target_timeline_id: Option<TimelineId>,
        horizon: u64,
        pitr: Duration,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<Vec<Arc<Timeline>>> {
        // grab mutex to prevent new timelines from being created here.
        let gc_cs = self.gc_cs.lock().await;

        // Scan all timelines. For each timeline, remember the timeline ID and
        // the branch point where it was created.
        let (all_branchpoints, timeline_ids): (BTreeSet<(TimelineId, Lsn)>, _) = {
            let timelines = self.timelines.lock().unwrap();
            let mut all_branchpoints = BTreeSet::new();
            let timeline_ids = {
                if let Some(target_timeline_id) = target_timeline_id.as_ref() {
                    if timelines.get(target_timeline_id).is_none() {
                        bail!("gc target timeline does not exist")
                    }
                };

                timelines
                    .iter()
                    .map(|(timeline_id, timeline_entry)| {
                        if let Some(ancestor_timeline_id) =
                            &timeline_entry.get_ancestor_timeline_id()
                        {
                            // If target_timeline is specified, we only need to know branchpoints of its children
                            if let Some(timeline_id) = target_timeline_id {
                                if ancestor_timeline_id == &timeline_id {
                                    all_branchpoints.insert((
                                        *ancestor_timeline_id,
                                        timeline_entry.get_ancestor_lsn(),
                                    ));
                                }
                            }
                            // Collect branchpoints for all timelines
                            else {
                                all_branchpoints.insert((
                                    *ancestor_timeline_id,
                                    timeline_entry.get_ancestor_lsn(),
                                ));
                            }
                        }

                        *timeline_id
                    })
                    .collect::<Vec<_>>()
            };
            (all_branchpoints, timeline_ids)
        };

        // Ok, we now know all the branch points.
        // Update the GC information for each timeline.
        let mut gc_timelines = Vec::with_capacity(timeline_ids.len());
        for timeline_id in timeline_ids {
            // Timeline is known to be local and loaded.
            let timeline = self
                .get_timeline(timeline_id, false)
                .with_context(|| format!("Timeline {timeline_id} was not found"))?;

            // If target_timeline is specified, ignore all other timelines
            if let Some(target_timeline_id) = target_timeline_id {
                if timeline_id != target_timeline_id {
                    continue;
                }
            }

            if let Some(cutoff) = timeline.get_last_record_lsn().checked_sub(horizon) {
                let branchpoints: Vec<Lsn> = all_branchpoints
                    .range((
                        Included((timeline_id, Lsn(0))),
                        Included((timeline_id, Lsn(u64::MAX))),
                    ))
                    .map(|&x| x.1)
                    .collect();
                timeline
                    .update_gc_info(branchpoints, cutoff, pitr, cancel, ctx)
                    .await?;

                gc_timelines.push(timeline);
            }
        }
        drop(gc_cs);
        Ok(gc_timelines)
    }

    /// A substitute for `branch_timeline` for use in unit tests.
    /// The returned timeline will have state value `Active` to make various `anyhow::ensure!()`
    /// calls pass, but, we do not actually call `.activate()` under the hood. So, none of the
    /// timeline background tasks are launched, except the flush loop.
    #[cfg(test)]
    async fn branch_timeline_test(
        &self,
        src_timeline: &Arc<Timeline>,
        dst_id: TimelineId,
        start_lsn: Option<Lsn>,
        ctx: &RequestContext,
    ) -> Result<Arc<Timeline>, CreateTimelineError> {
        let uninit_mark = self.create_timeline_uninit_mark(dst_id).unwrap();
        let tl = self
            .branch_timeline_impl(src_timeline, dst_id, start_lsn, uninit_mark, ctx)
            .await?;
        tl.set_state(TimelineState::Active);
        Ok(tl)
    }

    /// Branch an existing timeline.
    ///
    /// The caller is responsible for activating the returned timeline.
    async fn branch_timeline(
        &self,
        src_timeline: &Arc<Timeline>,
        dst_id: TimelineId,
        start_lsn: Option<Lsn>,
        timeline_uninit_mark: TimelineUninitMark<'_>,
        ctx: &RequestContext,
    ) -> Result<Arc<Timeline>, CreateTimelineError> {
        self.branch_timeline_impl(src_timeline, dst_id, start_lsn, timeline_uninit_mark, ctx)
            .await
    }

    async fn branch_timeline_impl(
        &self,
        src_timeline: &Arc<Timeline>,
        dst_id: TimelineId,
        start_lsn: Option<Lsn>,
        timeline_uninit_mark: TimelineUninitMark<'_>,
        _ctx: &RequestContext,
    ) -> Result<Arc<Timeline>, CreateTimelineError> {
        let src_id = src_timeline.timeline_id;

        // We will validate our ancestor LSN in this function.  Acquire the GC lock so that
        // this check cannot race with GC, and the ancestor LSN is guaranteed to remain
        // valid while we are creating the branch.
        let _gc_cs = self.gc_cs.lock().await;

        // If no start LSN is specified, we branch the new timeline from the source timeline's last record LSN
        let start_lsn = start_lsn.unwrap_or_else(|| {
            let lsn = src_timeline.get_last_record_lsn();
            info!("branching timeline {dst_id} from timeline {src_id} at last record LSN: {lsn}");
            lsn
        });

        // Ensure that `start_lsn` is valid, i.e. the LSN is within the PITR
        // horizon on the source timeline
        //
        // We check it against both the planned GC cutoff stored in 'gc_info',
        // and the 'latest_gc_cutoff' of the last GC that was performed.  The
        // planned GC cutoff in 'gc_info' is normally larger than
        // 'latest_gc_cutoff_lsn', but beware of corner cases like if you just
        // changed the GC settings for the tenant to make the PITR window
        // larger, but some of the data was already removed by an earlier GC
        // iteration.

        // check against last actual 'latest_gc_cutoff' first
        let latest_gc_cutoff_lsn = src_timeline.get_latest_gc_cutoff_lsn();
        src_timeline
            .check_lsn_is_in_scope(start_lsn, &latest_gc_cutoff_lsn)
            .context(format!(
                "invalid branch start lsn: less than latest GC cutoff {}",
                *latest_gc_cutoff_lsn,
            ))
            .map_err(CreateTimelineError::AncestorLsn)?;

        // and then the planned GC cutoff
        {
            let gc_info = src_timeline.gc_info.read().unwrap();
            let cutoff = min(gc_info.pitr_cutoff, gc_info.horizon_cutoff);
            if start_lsn < cutoff {
                return Err(CreateTimelineError::AncestorLsn(anyhow::anyhow!(
                    "invalid branch start lsn: less than planned GC cutoff {cutoff}"
                )));
            }
        }

        //
        // The branch point is valid, and we are still holding the 'gc_cs' lock
        // so that GC cannot advance the GC cutoff until we are finished.
        // Proceed with the branch creation.
        //

        // Determine prev-LSN for the new timeline. We can only determine it if
        // the timeline was branched at the current end of the source timeline.
        let RecordLsn {
            last: src_last,
            prev: src_prev,
        } = src_timeline.get_last_record_rlsn();
        let dst_prev = if src_last == start_lsn {
            Some(src_prev)
        } else {
            None
        };

        // Create the metadata file, noting the ancestor of the new timeline.
        // There is initially no data in it, but all the read-calls know to look
        // into the ancestor.
        let metadata = TimelineMetadata::new(
            start_lsn,
            dst_prev,
            Some(src_id),
            start_lsn,
            *src_timeline.latest_gc_cutoff_lsn.read(), // FIXME: should we hold onto this guard longer?
            src_timeline.initdb_lsn,
            src_timeline.pg_version,
        );

        let uninitialized_timeline = self
            .prepare_new_timeline(
                dst_id,
                &metadata,
                timeline_uninit_mark,
                start_lsn + 1,
                Some(Arc::clone(src_timeline)),
            )
            .await?;

        let new_timeline = uninitialized_timeline.finish_creation()?;

        // Root timeline gets its layers during creation and uploads them along with the metadata.
        // A branch timeline though, when created, can get no writes for some time, hence won't get any layers created.
        // We still need to upload its metadata eagerly: if other nodes `attach` the tenant and miss this timeline, their GC
        // could get incorrect information and remove more layers, than needed.
        // See also https://github.com/neondatabase/neon/issues/3865
        if let Some(remote_client) = new_timeline.remote_client.as_ref() {
            remote_client
                .schedule_index_upload_for_metadata_update(&metadata)
                .context("branch initial metadata upload")?;
        }

        Ok(new_timeline)
    }

    /// For unit tests, make this visible so that other modules can directly create timelines
    #[cfg(test)]
    #[tracing::instrument(skip_all, fields(tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug(), %timeline_id))]
    pub(crate) async fn bootstrap_timeline_test(
        &self,
        timeline_id: TimelineId,
        pg_version: u32,
        load_existing_initdb: Option<TimelineId>,
        ctx: &RequestContext,
    ) -> anyhow::Result<Arc<Timeline>> {
        let uninit_mark = self.create_timeline_uninit_mark(timeline_id).unwrap();
        self.bootstrap_timeline(
            timeline_id,
            pg_version,
            load_existing_initdb,
            uninit_mark,
            ctx,
        )
        .await
    }

    async fn upload_initdb(
        &self,
        timelines_path: &Utf8PathBuf,
        pgdata_path: &Utf8PathBuf,
        timeline_id: &TimelineId,
    ) -> anyhow::Result<()> {
        let Some(storage) = &self.remote_storage else {
            // No remote storage?  No upload.
            return Ok(());
        };

        let temp_path = timelines_path.join(format!(
            "{INITDB_PATH}.upload-{timeline_id}.{TEMP_FILE_SUFFIX}"
        ));

        scopeguard::defer! {
            if let Err(e) = fs::remove_file(&temp_path) {
                error!("Failed to remove temporary initdb archive '{temp_path}': {e}");
            }
        }

        let (pgdata_zstd, tar_zst_size) =
            import_datadir::create_tar_zst(pgdata_path, &temp_path).await?;

        pausable_failpoint!("before-initdb-upload");

        backoff::retry(
            || async {
                self::remote_timeline_client::upload_initdb_dir(
                    storage,
                    &self.tenant_shard_id.tenant_id,
                    timeline_id,
                    pgdata_zstd.try_clone().await?,
                    tar_zst_size,
                    &self.cancel,
                )
                .await
            },
            |_| false,
            3,
            u32::MAX,
            "persist_initdb_tar_zst",
            &self.cancel,
        )
        .await
        .ok_or_else(|| anyhow::Error::new(TimeoutOrCancel::Cancel))
        .and_then(|x| x)
    }

    /// - run initdb to init temporary instance and get bootstrap data
    /// - after initialization completes, tar up the temp dir and upload it to S3.
    ///
    /// The caller is responsible for activating the returned timeline.
    async fn bootstrap_timeline(
        &self,
        timeline_id: TimelineId,
        pg_version: u32,
        load_existing_initdb: Option<TimelineId>,
        timeline_uninit_mark: TimelineUninitMark<'_>,
        ctx: &RequestContext,
    ) -> anyhow::Result<Arc<Timeline>> {
        // create a `tenant/{tenant_id}/timelines/basebackup-{timeline_id}.{TEMP_FILE_SUFFIX}/`
        // temporary directory for basebackup files for the given timeline.

        let timelines_path = self.conf.timelines_path(&self.tenant_shard_id);
        let pgdata_path = path_with_suffix_extension(
            timelines_path.join(format!("basebackup-{timeline_id}")),
            TEMP_FILE_SUFFIX,
        );

        // an uninit mark was placed before, nothing else can access this timeline files
        // current initdb was not run yet, so remove whatever was left from the previous runs
        if pgdata_path.exists() {
            fs::remove_dir_all(&pgdata_path).with_context(|| {
                format!("Failed to remove already existing initdb directory: {pgdata_path}")
            })?;
        }
        // this new directory is very temporary, set to remove it immediately after bootstrap, we don't need it
        scopeguard::defer! {
            if let Err(e) = fs::remove_dir_all(&pgdata_path) {
                // this is unlikely, but we will remove the directory on pageserver restart or another bootstrap call
                error!("Failed to remove temporary initdb directory '{pgdata_path}': {e}");
            }
        }
        if let Some(existing_initdb_timeline_id) = load_existing_initdb {
            let Some(storage) = &self.remote_storage else {
                bail!("no storage configured but load_existing_initdb set to {existing_initdb_timeline_id}");
            };
            if existing_initdb_timeline_id != timeline_id {
                let source_path = &remote_initdb_archive_path(
                    &self.tenant_shard_id.tenant_id,
                    &existing_initdb_timeline_id,
                );
                let dest_path =
                    &remote_initdb_archive_path(&self.tenant_shard_id.tenant_id, &timeline_id);

                // if this fails, it will get retried by retried control plane requests
                storage
                    .copy_object(source_path, dest_path, &self.cancel)
                    .await
                    .context("copy initdb tar")?;
            }
            let (initdb_tar_zst_path, initdb_tar_zst) =
                self::remote_timeline_client::download_initdb_tar_zst(
                    self.conf,
                    storage,
                    &self.tenant_shard_id,
                    &existing_initdb_timeline_id,
                    &self.cancel,
                )
                .await
                .context("download initdb tar")?;

            scopeguard::defer! {
                if let Err(e) = fs::remove_file(&initdb_tar_zst_path) {
                    error!("Failed to remove temporary initdb archive '{initdb_tar_zst_path}': {e}");
                }
            }

            let buf_read =
                BufReader::with_capacity(remote_timeline_client::BUFFER_SIZE, initdb_tar_zst);
            import_datadir::extract_tar_zst(&pgdata_path, buf_read)
                .await
                .context("extract initdb tar")?;
        } else {
            // Init temporarily repo to get bootstrap data, this creates a directory in the `pgdata_path` path
            run_initdb(self.conf, &pgdata_path, pg_version, &self.cancel).await?;

            // Upload the created data dir to S3
            if self.tenant_shard_id().is_zero() {
                self.upload_initdb(&timelines_path, &pgdata_path, &timeline_id)
                    .await?;
            }
        }
        let pgdata_lsn = import_datadir::get_lsn_from_controlfile(&pgdata_path)?.align();

        // Import the contents of the data directory at the initial checkpoint
        // LSN, and any WAL after that.
        // Initdb lsn will be equal to last_record_lsn which will be set after import.
        // Because we know it upfront avoid having an option or dummy zero value by passing it to the metadata.
        let new_metadata = TimelineMetadata::new(
            Lsn(0),
            None,
            None,
            Lsn(0),
            pgdata_lsn,
            pgdata_lsn,
            pg_version,
        );
        let raw_timeline = self
            .prepare_new_timeline(
                timeline_id,
                &new_metadata,
                timeline_uninit_mark,
                pgdata_lsn,
                None,
            )
            .await?;

        let tenant_shard_id = raw_timeline.owning_tenant.tenant_shard_id;
        let unfinished_timeline = raw_timeline.raw_timeline()?;

        import_datadir::import_timeline_from_postgres_datadir(
            unfinished_timeline,
            &pgdata_path,
            pgdata_lsn,
            ctx,
        )
        .await
        .with_context(|| {
            format!("Failed to import pgdatadir for timeline {tenant_shard_id}/{timeline_id}")
        })?;

        // Flush the new layer files to disk, before we make the timeline as available to
        // the outside world.
        //
        // Flush loop needs to be spawned in order to be able to flush.
        unfinished_timeline.maybe_spawn_flush_loop();

        fail::fail_point!("before-checkpoint-new-timeline", |_| {
            anyhow::bail!("failpoint before-checkpoint-new-timeline");
        });

        unfinished_timeline
            .freeze_and_flush()
            .await
            .with_context(|| {
                format!(
                    "Failed to flush after pgdatadir import for timeline {tenant_shard_id}/{timeline_id}"
                )
            })?;

        // All done!
        let timeline = raw_timeline.finish_creation()?;

        Ok(timeline)
    }

    /// Call this before constructing a timeline, to build its required structures
    fn build_timeline_resources(&self, timeline_id: TimelineId) -> TimelineResources {
        let remote_client = if let Some(remote_storage) = self.remote_storage.as_ref() {
            let remote_client = RemoteTimelineClient::new(
                remote_storage.clone(),
                self.deletion_queue_client.clone(),
                self.conf,
                self.tenant_shard_id,
                timeline_id,
                self.generation,
            );
            Some(remote_client)
        } else {
            None
        };

        TimelineResources {
            remote_client,
            deletion_queue_client: self.deletion_queue_client.clone(),
            timeline_get_throttle: self.timeline_get_throttle.clone(),
        }
    }

    /// Creates intermediate timeline structure and its files.
    ///
    /// An empty layer map is initialized, and new data and WAL can be imported starting
    /// at 'disk_consistent_lsn'. After any initial data has been imported, call
    /// `finish_creation` to insert the Timeline into the timelines map and to remove the
    /// uninit mark file.
    async fn prepare_new_timeline<'a>(
        &'a self,
        new_timeline_id: TimelineId,
        new_metadata: &TimelineMetadata,
        uninit_mark: TimelineUninitMark<'a>,
        start_lsn: Lsn,
        ancestor: Option<Arc<Timeline>>,
    ) -> anyhow::Result<UninitializedTimeline> {
        let tenant_shard_id = self.tenant_shard_id;

        let resources = self.build_timeline_resources(new_timeline_id);
        if let Some(remote_client) = &resources.remote_client {
            remote_client.init_upload_queue_for_empty_remote(new_metadata)?;
        }

        let timeline_struct = self
            .create_timeline_struct(
                new_timeline_id,
                new_metadata,
                ancestor,
                resources,
                CreateTimelineCause::Load,
            )
            .context("Failed to create timeline data structure")?;

        timeline_struct.init_empty_layer_map(start_lsn);

        if let Err(e) = self.create_timeline_files(&uninit_mark.timeline_path).await {
            error!("Failed to create initial files for timeline {tenant_shard_id}/{new_timeline_id}, cleaning up: {e:?}");
            cleanup_timeline_directory(uninit_mark);
            return Err(e);
        }

        debug!(
            "Successfully created initial files for timeline {tenant_shard_id}/{new_timeline_id}"
        );

        Ok(UninitializedTimeline::new(
            self,
            new_timeline_id,
            Some((timeline_struct, uninit_mark)),
        ))
    }

    async fn create_timeline_files(&self, timeline_path: &Utf8Path) -> anyhow::Result<()> {
        crashsafe::create_dir(timeline_path).context("Failed to create timeline directory")?;

        fail::fail_point!("after-timeline-uninit-mark-creation", |_| {
            anyhow::bail!("failpoint after-timeline-uninit-mark-creation");
        });

        Ok(())
    }

    /// Attempts to create an uninit mark file for the timeline initialization.
    /// Bails, if the timeline is already loaded into the memory (i.e. initialized before), or the uninit mark file already exists.
    ///
    /// This way, we need to hold the timelines lock only for small amount of time during the mark check/creation per timeline init.
    fn create_timeline_uninit_mark(
        &self,
        timeline_id: TimelineId,
    ) -> Result<TimelineUninitMark, TimelineExclusionError> {
        let tenant_shard_id = self.tenant_shard_id;

        let uninit_mark_path = self
            .conf
            .timeline_uninit_mark_file_path(tenant_shard_id, timeline_id);
        let timeline_path = self.conf.timeline_path(&tenant_shard_id, &timeline_id);

        let uninit_mark = TimelineUninitMark::new(
            self,
            timeline_id,
            uninit_mark_path.clone(),
            timeline_path.clone(),
        )?;

        // At this stage, we have got exclusive access to in-memory state for this timeline ID
        // for creation.
        // A timeline directory should never exist on disk already:
        // - a previous failed creation would have cleaned up after itself
        // - a pageserver restart would clean up timeline directories that don't have valid remote state
        //
        // Therefore it is an unexpected internal error to encounter a timeline directory already existing here,
        // this error may indicate a bug in cleanup on failed creations.
        if timeline_path.exists() {
            return Err(TimelineExclusionError::Other(anyhow::anyhow!(
                "Timeline directory already exists! This is a bug."
            )));
        }

        // Create the on-disk uninit mark _after_ the in-memory acquisition of the tenant ID: guarantees
        // that during process runtime, colliding creations will be caught in-memory without getting
        // as far as failing to write a file.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&uninit_mark_path)
            .context("Failed to create uninit mark file")
            .and_then(|_| {
                crashsafe::fsync_file_and_parent(&uninit_mark_path)
                    .context("Failed to fsync uninit mark file")
            })
            .with_context(|| {
                format!("Failed to crate uninit mark for timeline {tenant_shard_id}/{timeline_id}")
            })?;

        Ok(uninit_mark)
    }

    /// Gathers inputs from all of the timelines to produce a sizing model input.
    ///
    /// Future is cancellation safe. Only one calculation can be running at once per tenant.
    #[instrument(skip_all, fields(tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug()))]
    pub async fn gather_size_inputs(
        &self,
        // `max_retention_period` overrides the cutoff that is used to calculate the size
        // (only if it is shorter than the real cutoff).
        max_retention_period: Option<u64>,
        cause: LogicalSizeCalculationCause,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<size::ModelInputs> {
        let logical_sizes_at_once = self
            .conf
            .concurrent_tenant_size_logical_size_queries
            .inner();

        // TODO: Having a single mutex block concurrent reads is not great for performance.
        //
        // But the only case where we need to run multiple of these at once is when we
        // request a size for a tenant manually via API, while another background calculation
        // is in progress (which is not a common case).
        //
        // See more for on the issue #2748 condenced out of the initial PR review.
        let mut shared_cache = self.cached_logical_sizes.lock().await;

        size::gather_inputs(
            self,
            logical_sizes_at_once,
            max_retention_period,
            &mut shared_cache,
            cause,
            cancel,
            ctx,
        )
        .await
    }

    /// Calculate synthetic tenant size and cache the result.
    /// This is periodically called by background worker.
    /// result is cached in tenant struct
    #[instrument(skip_all, fields(tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug()))]
    pub async fn calculate_synthetic_size(
        &self,
        cause: LogicalSizeCalculationCause,
        cancel: &CancellationToken,
        ctx: &RequestContext,
    ) -> anyhow::Result<u64> {
        let inputs = self.gather_size_inputs(None, cause, cancel, ctx).await?;

        let size = inputs.calculate()?;

        self.set_cached_synthetic_size(size);

        Ok(size)
    }

    /// Cache given synthetic size and update the metric value
    pub fn set_cached_synthetic_size(&self, size: u64) {
        self.cached_synthetic_tenant_size
            .store(size, Ordering::Relaxed);

        // Only shard zero should be calculating synthetic sizes
        debug_assert!(self.shard_identity.is_zero());

        TENANT_SYNTHETIC_SIZE_METRIC
            .get_metric_with_label_values(&[&self.tenant_shard_id.tenant_id.to_string()])
            .unwrap()
            .set(size);
    }

    pub fn cached_synthetic_size(&self) -> u64 {
        self.cached_synthetic_tenant_size.load(Ordering::Relaxed)
    }

    /// Flush any in-progress layers, schedule uploads, and wait for uploads to complete.
    ///
    /// This function can take a long time: callers should wrap it in a timeout if calling
    /// from an external API handler.
    ///
    /// Cancel-safety: cancelling this function may leave I/O running, but such I/O is
    /// still bounded by tenant/timeline shutdown.
    #[tracing::instrument(skip_all)]
    pub(crate) async fn flush_remote(&self) -> anyhow::Result<()> {
        let timelines = self.timelines.lock().unwrap().clone();

        async fn flush_timeline(_gate: GateGuard, timeline: Arc<Timeline>) -> anyhow::Result<()> {
            tracing::info!(timeline_id=%timeline.timeline_id, "Flushing...");
            timeline.freeze_and_flush().await?;
            tracing::info!(timeline_id=%timeline.timeline_id, "Waiting for uploads...");
            if let Some(client) = &timeline.remote_client {
                client.wait_completion().await?;
            }

            Ok(())
        }

        // We do not use a JoinSet for these tasks, because we don't want them to be
        // aborted when this function's future is cancelled: they should stay alive
        // holding their GateGuard until they complete, to ensure their I/Os complete
        // before Timeline shutdown completes.
        let mut results = FuturesUnordered::new();

        for (_timeline_id, timeline) in timelines {
            // Run each timeline's flush in a task holding the timeline's gate: this
            // means that if this function's future is cancelled, the Timeline shutdown
            // will still wait for any I/O in here to complete.
            let gate = match timeline.gate.enter() {
                Ok(g) => g,
                Err(_) => continue,
            };
            let jh = tokio::task::spawn(async move { flush_timeline(gate, timeline).await });
            results.push(jh);
        }

        while let Some(r) = results.next().await {
            if let Err(e) = r {
                if !e.is_cancelled() && !e.is_panic() {
                    tracing::error!("unexpected join error: {e:?}");
                }
            }
        }

        // The flushes we did above were just writes, but the Tenant might have had
        // pending deletions as well from recent compaction/gc: we want to flush those
        // as well.  This requires flushing the global delete queue.  This is cheap
        // because it's typically a no-op.
        match self.deletion_queue_client.flush_execute().await {
            Ok(_) => {}
            Err(DeletionQueueError::ShuttingDown) => {}
        }

        Ok(())
    }

    pub(crate) fn get_tenant_conf(&self) -> TenantConfOpt {
        self.tenant_conf.read().unwrap().tenant_conf.clone()
    }
}

/// Create the cluster temporarily in 'initdbpath' directory inside the repository
/// to get bootstrap data for timeline initialization.
async fn run_initdb(
    conf: &'static PageServerConf,
    initdb_target_dir: &Utf8Path,
    pg_version: u32,
    cancel: &CancellationToken,
) -> Result<(), InitdbError> {
    let initdb_bin_path = conf
        .pg_bin_dir(pg_version)
        .map_err(InitdbError::Other)?
        .join("initdb");
    let initdb_lib_dir = conf.pg_lib_dir(pg_version).map_err(InitdbError::Other)?;
    info!(
        "running {} in {}, libdir: {}",
        initdb_bin_path, initdb_target_dir, initdb_lib_dir,
    );

    let _permit = INIT_DB_SEMAPHORE.acquire().await;

    let initdb_command = tokio::process::Command::new(&initdb_bin_path)
        .args(["-D", initdb_target_dir.as_ref()])
        .args(["-U", &conf.superuser])
        .args(["-E", "utf8"])
        .arg("--no-instructions")
        .arg("--no-sync")
        .env_clear()
        .env("LD_LIBRARY_PATH", &initdb_lib_dir)
        .env("DYLD_LIBRARY_PATH", &initdb_lib_dir)
        .stdin(std::process::Stdio::null())
        // stdout invocation produces the same output every time, we don't need it
        .stdout(std::process::Stdio::null())
        // we would be interested in the stderr output, if there was any
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Ideally we'd select here with the cancellation token, but the problem is that
    // we can't safely terminate initdb: it launches processes of its own, and killing
    // initdb doesn't kill them. After we return from this function, we want the target
    // directory to be able to be cleaned up.
    // See https://github.com/neondatabase/neon/issues/6385
    let initdb_output = initdb_command.wait_with_output().await?;
    if !initdb_output.status.success() {
        return Err(InitdbError::Failed(
            initdb_output.status,
            initdb_output.stderr,
        ));
    }

    // This isn't true cancellation support, see above. Still return an error to
    // excercise the cancellation code path.
    if cancel.is_cancelled() {
        return Err(InitdbError::Cancelled);
    }

    Ok(())
}

impl Drop for Tenant {
    fn drop(&mut self) {
        remove_tenant_metrics(&self.tenant_shard_id);
    }
}
/// Dump contents of a layer file to stdout.
pub async fn dump_layerfile_from_path(
    path: &Utf8Path,
    verbose: bool,
    ctx: &RequestContext,
) -> anyhow::Result<()> {
    use std::os::unix::fs::FileExt;

    // All layer files start with a two-byte "magic" value, to identify the kind of
    // file.
    let file = File::open(path)?;
    let mut header_buf = [0u8; 2];
    file.read_exact_at(&mut header_buf, 0)?;

    match u16::from_be_bytes(header_buf) {
        crate::IMAGE_FILE_MAGIC => {
            ImageLayer::new_for_path(path, file)?
                .dump(verbose, ctx)
                .await?
        }
        crate::DELTA_FILE_MAGIC => {
            DeltaLayer::new_for_path(path, file)?
                .dump(verbose, ctx)
                .await?
        }
        magic => bail!("unrecognized magic identifier: {:?}", magic),
    }

    Ok(())
}

#[cfg(test)]
pub(crate) mod harness {
    use bytes::{Bytes, BytesMut};
    use camino::Utf8PathBuf;
    use once_cell::sync::OnceCell;
    use pageserver_api::models::ShardParameters;
    use pageserver_api::shard::ShardIndex;
    use std::fs;
    use std::sync::Arc;
    use utils::logging;
    use utils::lsn::Lsn;

    use crate::deletion_queue::mock::MockDeletionQueue;
    use crate::walredo::apply_neon;
    use crate::{
        config::PageServerConf, repository::Key, tenant::Tenant, walrecord::NeonWalRecord,
    };

    use super::*;
    use crate::tenant::config::{TenantConf, TenantConfOpt};
    use hex_literal::hex;
    use utils::id::{TenantId, TimelineId};

    pub const TIMELINE_ID: TimelineId =
        TimelineId::from_array(hex!("11223344556677881122334455667788"));
    pub const NEW_TIMELINE_ID: TimelineId =
        TimelineId::from_array(hex!("AA223344556677881122334455667788"));

    /// Convenience function to create a page image with given string as the only content
    pub fn test_img(s: &str) -> Bytes {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(s.as_bytes());
        buf.resize(64, 0);

        buf.freeze()
    }

    impl From<TenantConf> for TenantConfOpt {
        fn from(tenant_conf: TenantConf) -> Self {
            Self {
                checkpoint_distance: Some(tenant_conf.checkpoint_distance),
                checkpoint_timeout: Some(tenant_conf.checkpoint_timeout),
                compaction_target_size: Some(tenant_conf.compaction_target_size),
                compaction_period: Some(tenant_conf.compaction_period),
                compaction_threshold: Some(tenant_conf.compaction_threshold),
                gc_horizon: Some(tenant_conf.gc_horizon),
                gc_period: Some(tenant_conf.gc_period),
                image_creation_threshold: Some(tenant_conf.image_creation_threshold),
                pitr_interval: Some(tenant_conf.pitr_interval),
                walreceiver_connect_timeout: Some(tenant_conf.walreceiver_connect_timeout),
                lagging_wal_timeout: Some(tenant_conf.lagging_wal_timeout),
                max_lsn_wal_lag: Some(tenant_conf.max_lsn_wal_lag),
                trace_read_requests: Some(tenant_conf.trace_read_requests),
                eviction_policy: Some(tenant_conf.eviction_policy),
                min_resident_size_override: tenant_conf.min_resident_size_override,
                evictions_low_residence_duration_metric_threshold: Some(
                    tenant_conf.evictions_low_residence_duration_metric_threshold,
                ),
                gc_feedback: Some(tenant_conf.gc_feedback),
                heatmap_period: Some(tenant_conf.heatmap_period),
                lazy_slru_download: Some(tenant_conf.lazy_slru_download),
                timeline_get_throttle: Some(tenant_conf.timeline_get_throttle),
            }
        }
    }

    pub struct TenantHarness {
        pub conf: &'static PageServerConf,
        pub tenant_conf: TenantConf,
        pub tenant_shard_id: TenantShardId,
        pub generation: Generation,
        pub shard: ShardIndex,
        pub remote_storage: GenericRemoteStorage,
        pub remote_fs_dir: Utf8PathBuf,
        pub deletion_queue: MockDeletionQueue,
    }

    static LOG_HANDLE: OnceCell<()> = OnceCell::new();

    pub(crate) fn setup_logging() {
        LOG_HANDLE.get_or_init(|| {
            logging::init(
                logging::LogFormat::Test,
                // enable it in case the tests exercise code paths that use
                // debug_assert_current_span_has_tenant_and_timeline_id
                logging::TracingErrorLayerEnablement::EnableWithRustLogFilter,
                logging::Output::Stdout,
            )
            .expect("Failed to init test logging")
        });
    }

    impl TenantHarness {
        pub fn create(test_name: &'static str) -> anyhow::Result<Self> {
            setup_logging();

            let repo_dir = PageServerConf::test_repo_dir(test_name);
            let _ = fs::remove_dir_all(&repo_dir);
            fs::create_dir_all(&repo_dir)?;

            let conf = PageServerConf::dummy_conf(repo_dir);
            // Make a static copy of the config. This can never be free'd, but that's
            // OK in a test.
            let conf: &'static PageServerConf = Box::leak(Box::new(conf));

            // Disable automatic GC and compaction to make the unit tests more deterministic.
            // The tests perform them manually if needed.
            let tenant_conf = TenantConf {
                gc_period: Duration::ZERO,
                compaction_period: Duration::ZERO,
                ..TenantConf::default()
            };

            let tenant_id = TenantId::generate();
            let tenant_shard_id = TenantShardId::unsharded(tenant_id);
            fs::create_dir_all(conf.tenant_path(&tenant_shard_id))?;
            fs::create_dir_all(conf.timelines_path(&tenant_shard_id))?;

            use remote_storage::{RemoteStorageConfig, RemoteStorageKind};
            let remote_fs_dir = conf.workdir.join("localfs");
            std::fs::create_dir_all(&remote_fs_dir).unwrap();
            let config = RemoteStorageConfig {
                storage: RemoteStorageKind::LocalFs(remote_fs_dir.clone()),
                timeout: RemoteStorageConfig::DEFAULT_TIMEOUT,
            };
            let remote_storage = GenericRemoteStorage::from_config(&config).unwrap();
            let deletion_queue = MockDeletionQueue::new(Some(remote_storage.clone()));

            Ok(Self {
                conf,
                tenant_conf,
                tenant_shard_id,
                generation: Generation::new(0xdeadbeef),
                shard: ShardIndex::unsharded(),
                remote_storage,
                remote_fs_dir,
                deletion_queue,
            })
        }

        pub fn span(&self) -> tracing::Span {
            info_span!("TenantHarness", tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug())
        }

        pub(crate) async fn load(&self) -> (Arc<Tenant>, RequestContext) {
            let ctx = RequestContext::new(TaskKind::UnitTest, DownloadBehavior::Error);
            (
                self.do_try_load(&ctx)
                    .await
                    .expect("failed to load test tenant"),
                ctx,
            )
        }

        #[instrument(skip_all, fields(tenant_id=%self.tenant_shard_id.tenant_id, shard_id=%self.tenant_shard_id.shard_slug()))]
        pub(crate) async fn do_try_load(
            &self,
            ctx: &RequestContext,
        ) -> anyhow::Result<Arc<Tenant>> {
            let walredo_mgr = Arc::new(WalRedoManager::from(TestRedoManager));

            let tenant = Arc::new(Tenant::new(
                TenantState::Loading,
                self.conf,
                AttachedTenantConf::try_from(LocationConf::attached_single(
                    TenantConfOpt::from(self.tenant_conf.clone()),
                    self.generation,
                    &ShardParameters::default(),
                ))
                .unwrap(),
                // This is a legacy/test code path: sharding isn't supported here.
                ShardIdentity::unsharded(),
                Some(walredo_mgr),
                self.tenant_shard_id,
                Some(self.remote_storage.clone()),
                self.deletion_queue.new_client(),
            ));

            let preload = tenant
                .preload(&self.remote_storage, CancellationToken::new())
                .await?;
            tenant.attach(Some(preload), SpawnMode::Normal, ctx).await?;

            tenant.state.send_replace(TenantState::Active);
            for timeline in tenant.timelines.lock().unwrap().values() {
                timeline.set_state(TimelineState::Active);
            }
            Ok(tenant)
        }

        pub fn timeline_path(&self, timeline_id: &TimelineId) -> Utf8PathBuf {
            self.conf.timeline_path(&self.tenant_shard_id, timeline_id)
        }
    }

    // Mock WAL redo manager that doesn't do much
    pub(crate) struct TestRedoManager;

    impl TestRedoManager {
        /// # Cancel-Safety
        ///
        /// This method is cancellation-safe.
        pub async fn request_redo(
            &self,
            key: Key,
            lsn: Lsn,
            base_img: Option<(Lsn, Bytes)>,
            records: Vec<(Lsn, NeonWalRecord)>,
            _pg_version: u32,
        ) -> anyhow::Result<Bytes> {
            let records_neon = records.iter().all(|r| apply_neon::can_apply_in_neon(&r.1));
            if records_neon {
                // For Neon wal records, we can decode without spawning postgres, so do so.
                let base_img = base_img.expect("Neon WAL redo requires base image").1;
                let mut page = BytesMut::new();
                page.extend_from_slice(&base_img);
                for (_record_lsn, record) in records {
                    apply_neon::apply_in_neon(&record, key, &mut page)?;
                }
                Ok(page.freeze())
            } else {
                // We never spawn a postgres walredo process in unit tests: just log what we might have done.
                let s = format!(
                    "redo for {} to get to {}, with {} and {} records",
                    key,
                    lsn,
                    if base_img.is_some() {
                        "base image"
                    } else {
                        "no base image"
                    },
                    records.len()
                );
                println!("{s}");

                Ok(test_img(&s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyspace::KeySpaceAccum;
    use crate::repository::{Key, Value};
    use crate::tenant::harness::*;
    use crate::DEFAULT_PG_VERSION;
    use bytes::BytesMut;
    use hex_literal::hex;
    use once_cell::sync::Lazy;
    use pageserver_api::keyspace::KeySpace;
    use rand::{thread_rng, Rng};
    use tokio_util::sync::CancellationToken;

    static TEST_KEY: Lazy<Key> =
        Lazy::new(|| Key::from_slice(&hex!("010000000033333333444444445500000001")));

    #[tokio::test]
    async fn test_basic() -> anyhow::Result<()> {
        let (tenant, ctx) = TenantHarness::create("test_basic")?.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x08), DEFAULT_PG_VERSION, &ctx)
            .await?;

        let mut writer = tline.writer().await;
        writer
            .put(
                *TEST_KEY,
                Lsn(0x10),
                &Value::Image(test_img("foo at 0x10")),
                &ctx,
            )
            .await?;
        writer.finish_write(Lsn(0x10));
        drop(writer);

        let mut writer = tline.writer().await;
        writer
            .put(
                *TEST_KEY,
                Lsn(0x20),
                &Value::Image(test_img("foo at 0x20")),
                &ctx,
            )
            .await?;
        writer.finish_write(Lsn(0x20));
        drop(writer);

        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x10), &ctx).await?,
            test_img("foo at 0x10")
        );
        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x1f), &ctx).await?,
            test_img("foo at 0x10")
        );
        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x20), &ctx).await?,
            test_img("foo at 0x20")
        );

        Ok(())
    }

    #[tokio::test]
    async fn no_duplicate_timelines() -> anyhow::Result<()> {
        let (tenant, ctx) = TenantHarness::create("no_duplicate_timelines")?
            .load()
            .await;
        let _ = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;

        match tenant
            .create_empty_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await
        {
            Ok(_) => panic!("duplicate timeline creation should fail"),
            Err(e) => assert_eq!(e.to_string(), "Already exists".to_string()),
        }

        Ok(())
    }

    /// Convenience function to create a page image with given string as the only content
    pub fn test_value(s: &str) -> Value {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(s.as_bytes());
        Value::Image(buf.freeze())
    }

    ///
    /// Test branch creation
    ///
    #[tokio::test]
    async fn test_branch() -> anyhow::Result<()> {
        use std::str::from_utf8;

        let (tenant, ctx) = TenantHarness::create("test_branch")?.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;
        let mut writer = tline.writer().await;

        #[allow(non_snake_case)]
        let TEST_KEY_A: Key = Key::from_hex("110000000033333333444444445500000001").unwrap();
        #[allow(non_snake_case)]
        let TEST_KEY_B: Key = Key::from_hex("110000000033333333444444445500000002").unwrap();

        // Insert a value on the timeline
        writer
            .put(TEST_KEY_A, Lsn(0x20), &test_value("foo at 0x20"), &ctx)
            .await?;
        writer
            .put(TEST_KEY_B, Lsn(0x20), &test_value("foobar at 0x20"), &ctx)
            .await?;
        writer.finish_write(Lsn(0x20));

        writer
            .put(TEST_KEY_A, Lsn(0x30), &test_value("foo at 0x30"), &ctx)
            .await?;
        writer.finish_write(Lsn(0x30));
        writer
            .put(TEST_KEY_A, Lsn(0x40), &test_value("foo at 0x40"), &ctx)
            .await?;
        writer.finish_write(Lsn(0x40));

        //assert_current_logical_size(&tline, Lsn(0x40));

        // Branch the history, modify relation differently on the new timeline
        tenant
            .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x30)), &ctx)
            .await?;
        let newtline = tenant
            .get_timeline(NEW_TIMELINE_ID, true)
            .expect("Should have a local timeline");
        let mut new_writer = newtline.writer().await;
        new_writer
            .put(TEST_KEY_A, Lsn(0x40), &test_value("bar at 0x40"), &ctx)
            .await?;
        new_writer.finish_write(Lsn(0x40));

        // Check page contents on both branches
        assert_eq!(
            from_utf8(&tline.get(TEST_KEY_A, Lsn(0x40), &ctx).await?)?,
            "foo at 0x40"
        );
        assert_eq!(
            from_utf8(&newtline.get(TEST_KEY_A, Lsn(0x40), &ctx).await?)?,
            "bar at 0x40"
        );
        assert_eq!(
            from_utf8(&newtline.get(TEST_KEY_B, Lsn(0x40), &ctx).await?)?,
            "foobar at 0x20"
        );

        //assert_current_logical_size(&tline, Lsn(0x40));

        Ok(())
    }

    async fn make_some_layers(
        tline: &Timeline,
        start_lsn: Lsn,
        ctx: &RequestContext,
    ) -> anyhow::Result<()> {
        let mut lsn = start_lsn;
        {
            let mut writer = tline.writer().await;
            // Create a relation on the timeline
            writer
                .put(
                    *TEST_KEY,
                    lsn,
                    &Value::Image(test_img(&format!("foo at {}", lsn))),
                    ctx,
                )
                .await?;
            writer.finish_write(lsn);
            lsn += 0x10;
            writer
                .put(
                    *TEST_KEY,
                    lsn,
                    &Value::Image(test_img(&format!("foo at {}", lsn))),
                    ctx,
                )
                .await?;
            writer.finish_write(lsn);
            lsn += 0x10;
        }
        tline.freeze_and_flush().await?;
        {
            let mut writer = tline.writer().await;
            writer
                .put(
                    *TEST_KEY,
                    lsn,
                    &Value::Image(test_img(&format!("foo at {}", lsn))),
                    ctx,
                )
                .await?;
            writer.finish_write(lsn);
            lsn += 0x10;
            writer
                .put(
                    *TEST_KEY,
                    lsn,
                    &Value::Image(test_img(&format!("foo at {}", lsn))),
                    ctx,
                )
                .await?;
            writer.finish_write(lsn);
        }
        tline.freeze_and_flush().await
    }

    #[tokio::test]
    async fn test_prohibit_branch_creation_on_garbage_collected_data() -> anyhow::Result<()> {
        let (tenant, ctx) =
            TenantHarness::create("test_prohibit_branch_creation_on_garbage_collected_data")?
                .load()
                .await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;
        make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

        // this removes layers before lsn 40 (50 minus 10), so there are two remaining layers, image and delta for 31-50
        // FIXME: this doesn't actually remove any layer currently, given how the flushing
        // and compaction works. But it does set the 'cutoff' point so that the cross check
        // below should fail.
        tenant
            .gc_iteration(
                Some(TIMELINE_ID),
                0x10,
                Duration::ZERO,
                &CancellationToken::new(),
                &ctx,
            )
            .await?;

        // try to branch at lsn 25, should fail because we already garbage collected the data
        match tenant
            .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x25)), &ctx)
            .await
        {
            Ok(_) => panic!("branching should have failed"),
            Err(err) => {
                let CreateTimelineError::AncestorLsn(err) = err else {
                    panic!("wrong error type")
                };
                assert!(err.to_string().contains("invalid branch start lsn"));
                assert!(err
                    .source()
                    .unwrap()
                    .to_string()
                    .contains("we might've already garbage collected needed data"))
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_prohibit_branch_creation_on_pre_initdb_lsn() -> anyhow::Result<()> {
        let (tenant, ctx) =
            TenantHarness::create("test_prohibit_branch_creation_on_pre_initdb_lsn")?
                .load()
                .await;

        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x50), DEFAULT_PG_VERSION, &ctx)
            .await?;
        // try to branch at lsn 0x25, should fail because initdb lsn is 0x50
        match tenant
            .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x25)), &ctx)
            .await
        {
            Ok(_) => panic!("branching should have failed"),
            Err(err) => {
                let CreateTimelineError::AncestorLsn(err) = err else {
                    panic!("wrong error type");
                };
                assert!(&err.to_string().contains("invalid branch start lsn"));
                assert!(&err
                    .source()
                    .unwrap()
                    .to_string()
                    .contains("is earlier than latest GC horizon"));
            }
        }

        Ok(())
    }

    /*
    // FIXME: This currently fails to error out. Calling GC doesn't currently
    // remove the old value, we'd need to work a little harder
    #[tokio::test]
    async fn test_prohibit_get_for_garbage_collected_data() -> anyhow::Result<()> {
        let repo =
            RepoHarness::create("test_prohibit_get_for_garbage_collected_data")?
            .load();

        let tline = repo.create_empty_timeline(TIMELINE_ID, Lsn(0), DEFAULT_PG_VERSION)?;
        make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

        repo.gc_iteration(Some(TIMELINE_ID), 0x10, Duration::ZERO)?;
        let latest_gc_cutoff_lsn = tline.get_latest_gc_cutoff_lsn();
        assert!(*latest_gc_cutoff_lsn > Lsn(0x25));
        match tline.get(*TEST_KEY, Lsn(0x25)) {
            Ok(_) => panic!("request for page should have failed"),
            Err(err) => assert!(err.to_string().contains("not found at")),
        }
        Ok(())
    }
     */

    #[tokio::test]
    async fn test_get_branchpoints_from_an_inactive_timeline() -> anyhow::Result<()> {
        let (tenant, ctx) =
            TenantHarness::create("test_get_branchpoints_from_an_inactive_timeline")?
                .load()
                .await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;
        make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

        tenant
            .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x40)), &ctx)
            .await?;
        let newtline = tenant
            .get_timeline(NEW_TIMELINE_ID, true)
            .expect("Should have a local timeline");

        make_some_layers(newtline.as_ref(), Lsn(0x60), &ctx).await?;

        tline.set_broken("test".to_owned());

        tenant
            .gc_iteration(
                Some(TIMELINE_ID),
                0x10,
                Duration::ZERO,
                &CancellationToken::new(),
                &ctx,
            )
            .await?;

        // The branchpoints should contain all timelines, even ones marked
        // as Broken.
        {
            let branchpoints = &tline.gc_info.read().unwrap().retain_lsns;
            assert_eq!(branchpoints.len(), 1);
            assert_eq!(branchpoints[0], Lsn(0x40));
        }

        // You can read the key from the child branch even though the parent is
        // Broken, as long as you don't need to access data from the parent.
        assert_eq!(
            newtline.get(*TEST_KEY, Lsn(0x70), &ctx).await?,
            test_img(&format!("foo at {}", Lsn(0x70)))
        );

        // This needs to traverse to the parent, and fails.
        let err = newtline.get(*TEST_KEY, Lsn(0x50), &ctx).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("will not become active. Current state: Broken"));

        Ok(())
    }

    #[tokio::test]
    async fn test_retain_data_in_parent_which_is_needed_for_child() -> anyhow::Result<()> {
        let (tenant, ctx) =
            TenantHarness::create("test_retain_data_in_parent_which_is_needed_for_child")?
                .load()
                .await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;
        make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

        tenant
            .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x40)), &ctx)
            .await?;
        let newtline = tenant
            .get_timeline(NEW_TIMELINE_ID, true)
            .expect("Should have a local timeline");
        // this removes layers before lsn 40 (50 minus 10), so there are two remaining layers, image and delta for 31-50
        tenant
            .gc_iteration(
                Some(TIMELINE_ID),
                0x10,
                Duration::ZERO,
                &CancellationToken::new(),
                &ctx,
            )
            .await?;
        assert!(newtline.get(*TEST_KEY, Lsn(0x25), &ctx).await.is_ok());

        Ok(())
    }
    #[tokio::test]
    async fn test_parent_keeps_data_forever_after_branching() -> anyhow::Result<()> {
        let (tenant, ctx) =
            TenantHarness::create("test_parent_keeps_data_forever_after_branching")?
                .load()
                .await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;
        make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

        tenant
            .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x40)), &ctx)
            .await?;
        let newtline = tenant
            .get_timeline(NEW_TIMELINE_ID, true)
            .expect("Should have a local timeline");

        make_some_layers(newtline.as_ref(), Lsn(0x60), &ctx).await?;

        // run gc on parent
        tenant
            .gc_iteration(
                Some(TIMELINE_ID),
                0x10,
                Duration::ZERO,
                &CancellationToken::new(),
                &ctx,
            )
            .await?;

        // Check that the data is still accessible on the branch.
        assert_eq!(
            newtline.get(*TEST_KEY, Lsn(0x50), &ctx).await?,
            test_img(&format!("foo at {}", Lsn(0x40)))
        );

        Ok(())
    }

    #[tokio::test]
    async fn timeline_load() -> anyhow::Result<()> {
        const TEST_NAME: &str = "timeline_load";
        let harness = TenantHarness::create(TEST_NAME)?;
        {
            let (tenant, ctx) = harness.load().await;
            let tline = tenant
                .create_test_timeline(TIMELINE_ID, Lsn(0x7000), DEFAULT_PG_VERSION, &ctx)
                .await?;
            make_some_layers(tline.as_ref(), Lsn(0x8000), &ctx).await?;
            // so that all uploads finish & we can call harness.load() below again
            tenant
                .shutdown(Default::default(), true)
                .instrument(harness.span())
                .await
                .ok()
                .unwrap();
        }

        let (tenant, _ctx) = harness.load().await;
        tenant
            .get_timeline(TIMELINE_ID, true)
            .expect("cannot load timeline");

        Ok(())
    }

    #[tokio::test]
    async fn timeline_load_with_ancestor() -> anyhow::Result<()> {
        const TEST_NAME: &str = "timeline_load_with_ancestor";
        let harness = TenantHarness::create(TEST_NAME)?;
        // create two timelines
        {
            let (tenant, ctx) = harness.load().await;
            let tline = tenant
                .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
                .await?;

            make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

            let child_tline = tenant
                .branch_timeline_test(&tline, NEW_TIMELINE_ID, Some(Lsn(0x40)), &ctx)
                .await?;
            child_tline.set_state(TimelineState::Active);

            let newtline = tenant
                .get_timeline(NEW_TIMELINE_ID, true)
                .expect("Should have a local timeline");

            make_some_layers(newtline.as_ref(), Lsn(0x60), &ctx).await?;

            // so that all uploads finish & we can call harness.load() below again
            tenant
                .shutdown(Default::default(), true)
                .instrument(harness.span())
                .await
                .ok()
                .unwrap();
        }

        // check that both of them are initially unloaded
        let (tenant, _ctx) = harness.load().await;

        // check that both, child and ancestor are loaded
        let _child_tline = tenant
            .get_timeline(NEW_TIMELINE_ID, true)
            .expect("cannot get child timeline loaded");

        let _ancestor_tline = tenant
            .get_timeline(TIMELINE_ID, true)
            .expect("cannot get ancestor timeline loaded");

        Ok(())
    }

    #[tokio::test]
    async fn delta_layer_dumping() -> anyhow::Result<()> {
        use storage_layer::AsLayerDesc;
        let (tenant, ctx) = TenantHarness::create("test_layer_dumping")?.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;
        make_some_layers(tline.as_ref(), Lsn(0x20), &ctx).await?;

        let layer_map = tline.layers.read().await;
        let level0_deltas = layer_map
            .layer_map()
            .get_level0_deltas()?
            .into_iter()
            .map(|desc| layer_map.get_from_desc(&desc))
            .collect::<Vec<_>>();

        assert!(!level0_deltas.is_empty());

        for delta in level0_deltas {
            // Ensure we are dumping a delta layer here
            assert!(delta.layer_desc().is_delta);
            delta.dump(true, &ctx).await.unwrap();
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_images() -> anyhow::Result<()> {
        let (tenant, ctx) = TenantHarness::create("test_images")?.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x08), DEFAULT_PG_VERSION, &ctx)
            .await?;

        let mut writer = tline.writer().await;
        writer
            .put(
                *TEST_KEY,
                Lsn(0x10),
                &Value::Image(test_img("foo at 0x10")),
                &ctx,
            )
            .await?;
        writer.finish_write(Lsn(0x10));
        drop(writer);

        tline.freeze_and_flush().await?;
        tline
            .compact(&CancellationToken::new(), EnumSet::empty(), &ctx)
            .await?;

        let mut writer = tline.writer().await;
        writer
            .put(
                *TEST_KEY,
                Lsn(0x20),
                &Value::Image(test_img("foo at 0x20")),
                &ctx,
            )
            .await?;
        writer.finish_write(Lsn(0x20));
        drop(writer);

        tline.freeze_and_flush().await?;
        tline
            .compact(&CancellationToken::new(), EnumSet::empty(), &ctx)
            .await?;

        let mut writer = tline.writer().await;
        writer
            .put(
                *TEST_KEY,
                Lsn(0x30),
                &Value::Image(test_img("foo at 0x30")),
                &ctx,
            )
            .await?;
        writer.finish_write(Lsn(0x30));
        drop(writer);

        tline.freeze_and_flush().await?;
        tline
            .compact(&CancellationToken::new(), EnumSet::empty(), &ctx)
            .await?;

        let mut writer = tline.writer().await;
        writer
            .put(
                *TEST_KEY,
                Lsn(0x40),
                &Value::Image(test_img("foo at 0x40")),
                &ctx,
            )
            .await?;
        writer.finish_write(Lsn(0x40));
        drop(writer);

        tline.freeze_and_flush().await?;
        tline
            .compact(&CancellationToken::new(), EnumSet::empty(), &ctx)
            .await?;

        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x10), &ctx).await?,
            test_img("foo at 0x10")
        );
        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x1f), &ctx).await?,
            test_img("foo at 0x10")
        );
        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x20), &ctx).await?,
            test_img("foo at 0x20")
        );
        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x30), &ctx).await?,
            test_img("foo at 0x30")
        );
        assert_eq!(
            tline.get(*TEST_KEY, Lsn(0x40), &ctx).await?,
            test_img("foo at 0x40")
        );

        Ok(())
    }

    async fn bulk_insert_compact_gc(
        timeline: Arc<Timeline>,
        ctx: &RequestContext,
        mut lsn: Lsn,
        repeat: usize,
        key_count: usize,
    ) -> anyhow::Result<()> {
        let mut test_key = Key::from_hex("010000000033333333444444445500000000").unwrap();
        let mut blknum = 0;

        // Enforce that key range is monotonously increasing
        let mut keyspace = KeySpaceAccum::new();

        for _ in 0..repeat {
            for _ in 0..key_count {
                test_key.field6 = blknum;
                let mut writer = timeline.writer().await;
                writer
                    .put(
                        test_key,
                        lsn,
                        &Value::Image(test_img(&format!("{} at {}", blknum, lsn))),
                        ctx,
                    )
                    .await?;
                writer.finish_write(lsn);
                drop(writer);

                keyspace.add_key(test_key);

                lsn = Lsn(lsn.0 + 0x10);
                blknum += 1;
            }

            let cutoff = timeline.get_last_record_lsn();

            timeline
                .update_gc_info(
                    Vec::new(),
                    cutoff,
                    Duration::ZERO,
                    &CancellationToken::new(),
                    ctx,
                )
                .await?;
            timeline.freeze_and_flush().await?;
            timeline
                .compact(&CancellationToken::new(), EnumSet::empty(), ctx)
                .await?;
            timeline.gc().await?;
        }

        Ok(())
    }

    //
    // Insert 1000 key-value pairs with increasing keys, flush, compact, GC.
    // Repeat 50 times.
    //
    #[tokio::test]
    async fn test_bulk_insert() -> anyhow::Result<()> {
        let harness = TenantHarness::create("test_bulk_insert")?;
        let (tenant, ctx) = harness.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x08), DEFAULT_PG_VERSION, &ctx)
            .await?;

        let lsn = Lsn(0x10);
        bulk_insert_compact_gc(tline.clone(), &ctx, lsn, 50, 10000).await?;

        Ok(())
    }

    // Test the vectored get real implementation against a simple sequential implementation.
    //
    // The test generates a keyspace by repeatedly flushing the in-memory layer and compacting.
    // Projected to 2D the key space looks like below. Lsn grows upwards on the Y axis and keys
    // grow to the right on the X axis.
    //                       [Delta]
    //                 [Delta]
    //           [Delta]
    //    [Delta]
    // ------------ Image ---------------
    //
    // After layer generation we pick the ranges to query as follows:
    // 1. The beginning of each delta layer
    // 2. At the seam between two adjacent delta layers
    //
    // There's one major downside to this test: delta layers only contains images,
    // so the search can stop at the first delta layer and doesn't traverse any deeper.
    #[tokio::test]
    async fn test_get_vectored() -> anyhow::Result<()> {
        let harness = TenantHarness::create("test_get_vectored")?;
        let (tenant, ctx) = harness.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x08), DEFAULT_PG_VERSION, &ctx)
            .await?;

        let lsn = Lsn(0x10);
        bulk_insert_compact_gc(tline.clone(), &ctx, lsn, 50, 10000).await?;

        let guard = tline.layers.read().await;
        guard.layer_map().dump(true, &ctx).await?;

        let mut reads = Vec::new();
        let mut prev = None;
        guard.layer_map().iter_historic_layers().for_each(|desc| {
            if !desc.is_delta() {
                prev = Some(desc.clone());
                return;
            }

            let start = desc.key_range.start;
            let end = desc
                .key_range
                .start
                .add(Timeline::MAX_GET_VECTORED_KEYS.try_into().unwrap());
            reads.push(KeySpace {
                ranges: vec![start..end],
            });

            if let Some(prev) = &prev {
                if !prev.is_delta() {
                    return;
                }

                let first_range = Key {
                    field6: prev.key_range.end.field6 - 4,
                    ..prev.key_range.end
                }..prev.key_range.end;

                let second_range = desc.key_range.start..Key {
                    field6: desc.key_range.start.field6 + 4,
                    ..desc.key_range.start
                };

                reads.push(KeySpace {
                    ranges: vec![first_range, second_range],
                });
            };

            prev = Some(desc.clone());
        });

        drop(guard);

        // Pick a big LSN such that we query over all the changes.
        // Technically, u64::MAX - 1 is the largest LSN supported by the read path,
        // but there seems to be a bug on the non-vectored search path which surfaces
        // in that case.
        let reads_lsn = Lsn(u64::MAX - 1000);

        for read in reads {
            info!("Doing vectored read on {:?}", read);

            let vectored_res = tline.get_vectored_impl(read.clone(), reads_lsn, &ctx).await;
            tline
                .validate_get_vectored_impl(&vectored_res, read, reads_lsn, &ctx)
                .await;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_random_updates() -> anyhow::Result<()> {
        let harness = TenantHarness::create("test_random_updates")?;
        let (tenant, ctx) = harness.load().await;
        let tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;

        const NUM_KEYS: usize = 1000;

        let mut test_key = Key::from_hex("010000000033333333444444445500000000").unwrap();

        let mut keyspace = KeySpaceAccum::new();

        // Track when each page was last modified. Used to assert that
        // a read sees the latest page version.
        let mut updated = [Lsn(0); NUM_KEYS];

        let mut lsn = Lsn(0x10);
        #[allow(clippy::needless_range_loop)]
        for blknum in 0..NUM_KEYS {
            lsn = Lsn(lsn.0 + 0x10);
            test_key.field6 = blknum as u32;
            let mut writer = tline.writer().await;
            writer
                .put(
                    test_key,
                    lsn,
                    &Value::Image(test_img(&format!("{} at {}", blknum, lsn))),
                    &ctx,
                )
                .await?;
            writer.finish_write(lsn);
            updated[blknum] = lsn;
            drop(writer);

            keyspace.add_key(test_key);
        }

        for _ in 0..50 {
            for _ in 0..NUM_KEYS {
                lsn = Lsn(lsn.0 + 0x10);
                let blknum = thread_rng().gen_range(0..NUM_KEYS);
                test_key.field6 = blknum as u32;
                let mut writer = tline.writer().await;
                writer
                    .put(
                        test_key,
                        lsn,
                        &Value::Image(test_img(&format!("{} at {}", blknum, lsn))),
                        &ctx,
                    )
                    .await?;
                writer.finish_write(lsn);
                drop(writer);
                updated[blknum] = lsn;
            }

            // Read all the blocks
            for (blknum, last_lsn) in updated.iter().enumerate() {
                test_key.field6 = blknum as u32;
                assert_eq!(
                    tline.get(test_key, lsn, &ctx).await?,
                    test_img(&format!("{} at {}", blknum, last_lsn))
                );
            }

            // Perform a cycle of flush, compact, and GC
            let cutoff = tline.get_last_record_lsn();
            tline
                .update_gc_info(
                    Vec::new(),
                    cutoff,
                    Duration::ZERO,
                    &CancellationToken::new(),
                    &ctx,
                )
                .await?;
            tline.freeze_and_flush().await?;
            tline
                .compact(&CancellationToken::new(), EnumSet::empty(), &ctx)
                .await?;
            tline.gc().await?;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_traverse_branches() -> anyhow::Result<()> {
        let (tenant, ctx) = TenantHarness::create("test_traverse_branches")?
            .load()
            .await;
        let mut tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;

        const NUM_KEYS: usize = 1000;

        let mut test_key = Key::from_hex("010000000033333333444444445500000000").unwrap();

        let mut keyspace = KeySpaceAccum::new();

        // Track when each page was last modified. Used to assert that
        // a read sees the latest page version.
        let mut updated = [Lsn(0); NUM_KEYS];

        let mut lsn = Lsn(0x10);
        #[allow(clippy::needless_range_loop)]
        for blknum in 0..NUM_KEYS {
            lsn = Lsn(lsn.0 + 0x10);
            test_key.field6 = blknum as u32;
            let mut writer = tline.writer().await;
            writer
                .put(
                    test_key,
                    lsn,
                    &Value::Image(test_img(&format!("{} at {}", blknum, lsn))),
                    &ctx,
                )
                .await?;
            writer.finish_write(lsn);
            updated[blknum] = lsn;
            drop(writer);

            keyspace.add_key(test_key);
        }

        for _ in 0..50 {
            let new_tline_id = TimelineId::generate();
            tenant
                .branch_timeline_test(&tline, new_tline_id, Some(lsn), &ctx)
                .await?;
            tline = tenant
                .get_timeline(new_tline_id, true)
                .expect("Should have the branched timeline");

            for _ in 0..NUM_KEYS {
                lsn = Lsn(lsn.0 + 0x10);
                let blknum = thread_rng().gen_range(0..NUM_KEYS);
                test_key.field6 = blknum as u32;
                let mut writer = tline.writer().await;
                writer
                    .put(
                        test_key,
                        lsn,
                        &Value::Image(test_img(&format!("{} at {}", blknum, lsn))),
                        &ctx,
                    )
                    .await?;
                println!("updating {} at {}", blknum, lsn);
                writer.finish_write(lsn);
                drop(writer);
                updated[blknum] = lsn;
            }

            // Read all the blocks
            for (blknum, last_lsn) in updated.iter().enumerate() {
                test_key.field6 = blknum as u32;
                assert_eq!(
                    tline.get(test_key, lsn, &ctx).await?,
                    test_img(&format!("{} at {}", blknum, last_lsn))
                );
            }

            // Perform a cycle of flush, compact, and GC
            let cutoff = tline.get_last_record_lsn();
            tline
                .update_gc_info(
                    Vec::new(),
                    cutoff,
                    Duration::ZERO,
                    &CancellationToken::new(),
                    &ctx,
                )
                .await?;
            tline.freeze_and_flush().await?;
            tline
                .compact(&CancellationToken::new(), EnumSet::empty(), &ctx)
                .await?;
            tline.gc().await?;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_traverse_ancestors() -> anyhow::Result<()> {
        let (tenant, ctx) = TenantHarness::create("test_traverse_ancestors")?
            .load()
            .await;
        let mut tline = tenant
            .create_test_timeline(TIMELINE_ID, Lsn(0x10), DEFAULT_PG_VERSION, &ctx)
            .await?;

        const NUM_KEYS: usize = 100;
        const NUM_TLINES: usize = 50;

        let mut test_key = Key::from_hex("010000000033333333444444445500000000").unwrap();
        // Track page mutation lsns across different timelines.
        let mut updated = [[Lsn(0); NUM_KEYS]; NUM_TLINES];

        let mut lsn = Lsn(0x10);

        #[allow(clippy::needless_range_loop)]
        for idx in 0..NUM_TLINES {
            let new_tline_id = TimelineId::generate();
            tenant
                .branch_timeline_test(&tline, new_tline_id, Some(lsn), &ctx)
                .await?;
            tline = tenant
                .get_timeline(new_tline_id, true)
                .expect("Should have the branched timeline");

            for _ in 0..NUM_KEYS {
                lsn = Lsn(lsn.0 + 0x10);
                let blknum = thread_rng().gen_range(0..NUM_KEYS);
                test_key.field6 = blknum as u32;
                let mut writer = tline.writer().await;
                writer
                    .put(
                        test_key,
                        lsn,
                        &Value::Image(test_img(&format!("{} {} at {}", idx, blknum, lsn))),
                        &ctx,
                    )
                    .await?;
                println!("updating [{}][{}] at {}", idx, blknum, lsn);
                writer.finish_write(lsn);
                drop(writer);
                updated[idx][blknum] = lsn;
            }
        }

        // Read pages from leaf timeline across all ancestors.
        for (idx, lsns) in updated.iter().enumerate() {
            for (blknum, lsn) in lsns.iter().enumerate() {
                // Skip empty mutations.
                if lsn.0 == 0 {
                    continue;
                }
                println!("checking [{idx}][{blknum}] at {lsn}");
                test_key.field6 = blknum as u32;
                assert_eq!(
                    tline.get(test_key, *lsn, &ctx).await?,
                    test_img(&format!("{idx} {blknum} at {lsn}"))
                );
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_write_at_initdb_lsn_takes_optimization_code_path() -> anyhow::Result<()> {
        let (tenant, ctx) = TenantHarness::create("test_empty_test_timeline_is_usable")?
            .load()
            .await;

        let initdb_lsn = Lsn(0x20);
        let utline = tenant
            .create_empty_timeline(TIMELINE_ID, initdb_lsn, DEFAULT_PG_VERSION, &ctx)
            .await?;
        let tline = utline.raw_timeline().unwrap();

        // Spawn flush loop now so that we can set the `expect_initdb_optimization`
        tline.maybe_spawn_flush_loop();

        // Make sure the timeline has the minimum set of required keys for operation.
        // The only operation you can always do on an empty timeline is to `put` new data.
        // Except if you `put` at `initdb_lsn`.
        // In that case, there's an optimization to directly create image layers instead of delta layers.
        // It uses `repartition()`, which assumes some keys to be present.
        // Let's make sure the test timeline can handle that case.
        {
            let mut state = tline.flush_loop_state.lock().unwrap();
            assert_eq!(
                timeline::FlushLoopState::Running {
                    expect_initdb_optimization: false,
                    initdb_optimization_count: 0,
                },
                *state
            );
            *state = timeline::FlushLoopState::Running {
                expect_initdb_optimization: true,
                initdb_optimization_count: 0,
            };
        }

        // Make writes at the initdb_lsn. When we flush it below, it should be handled by the optimization.
        // As explained above, the optimization requires some keys to be present.
        // As per `create_empty_timeline` documentation, use init_empty to set them.
        // This is what `create_test_timeline` does, by the way.
        let mut modification = tline.begin_modification(initdb_lsn);
        modification
            .init_empty_test_timeline()
            .context("init_empty_test_timeline")?;
        modification
            .commit(&ctx)
            .await
            .context("commit init_empty_test_timeline modification")?;

        // Do the flush. The flush code will check the expectations that we set above.
        tline.freeze_and_flush().await?;

        // assert freeze_and_flush exercised the initdb optimization
        {
            let state = tline.flush_loop_state.lock().unwrap();
            let timeline::FlushLoopState::Running {
                expect_initdb_optimization,
                initdb_optimization_count,
            } = *state
            else {
                panic!("unexpected state: {:?}", *state);
            };
            assert!(expect_initdb_optimization);
            assert!(initdb_optimization_count > 0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_uninit_mark_crash() -> anyhow::Result<()> {
        let name = "test_uninit_mark_crash";
        let harness = TenantHarness::create(name)?;
        {
            let (tenant, ctx) = harness.load().await;
            let tline = tenant
                .create_empty_timeline(TIMELINE_ID, Lsn(0), DEFAULT_PG_VERSION, &ctx)
                .await?;
            // Keeps uninit mark in place
            let raw_tline = tline.raw_timeline().unwrap();
            raw_tline
                .shutdown()
                .instrument(info_span!("test_shutdown", tenant_id=%raw_tline.tenant_shard_id, shard_id=%raw_tline.tenant_shard_id.shard_slug(), timeline_id=%TIMELINE_ID))
                .await;
            std::mem::forget(tline);
        }

        let (tenant, _) = harness.load().await;
        match tenant.get_timeline(TIMELINE_ID, false) {
            Ok(_) => panic!("timeline should've been removed during load"),
            Err(e) => {
                assert_eq!(
                    e,
                    GetTimelineError::NotFound {
                        tenant_id: tenant.tenant_shard_id,
                        timeline_id: TIMELINE_ID,
                    }
                )
            }
        }

        assert!(!harness
            .conf
            .timeline_path(&tenant.tenant_shard_id, &TIMELINE_ID)
            .exists());

        assert!(!harness
            .conf
            .timeline_uninit_mark_file_path(tenant.tenant_shard_id, TIMELINE_ID)
            .exists());

        Ok(())
    }
}
