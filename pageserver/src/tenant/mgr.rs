//! This module acts as a switchboard to access different repositories managed by this
//! page server.

use camino::{Utf8DirEntry, Utf8Path, Utf8PathBuf};
use futures::stream::StreamExt;
use itertools::Itertools;
use pageserver_api::key::Key;
use pageserver_api::models::ShardParameters;
use pageserver_api::shard::{ShardCount, ShardIdentity, ShardNumber, TenantShardId};
use rand::{distributions::Alphanumeric, Rng};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use utils::timeout::{timeout_cancellable, TimeoutCancellableError};

use anyhow::Context;
use once_cell::sync::Lazy;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::*;

use remote_storage::GenericRemoteStorage;
use utils::{completion, crashsafe};

use crate::config::PageServerConf;
use crate::context::{DownloadBehavior, RequestContext};
use crate::control_plane_client::{
    ControlPlaneClient, ControlPlaneGenerationsApi, RetryForeverError,
};
use crate::deletion_queue::DeletionQueueClient;
use crate::http::routes::ACTIVE_TENANT_TIMEOUT;
use crate::metrics::{TENANT, TENANT_MANAGER as METRICS};
use crate::task_mgr::{self, TaskKind};
use crate::tenant::config::{
    AttachedLocationConfig, AttachmentMode, LocationConf, LocationMode, SecondaryLocationConfig,
    TenantConfOpt,
};
use crate::tenant::delete::DeleteTenantFlow;
use crate::tenant::span::debug_assert_current_span_has_tenant_id;
use crate::tenant::{AttachedTenantConf, SpawnMode, Tenant, TenantState};
use crate::{InitializationOrder, IGNORED_TENANT_FILE_NAME, METADATA_FILE_NAME, TEMP_FILE_SUFFIX};

use utils::crashsafe::path_with_suffix_extension;
use utils::fs_ext::PathExt;
use utils::generation::Generation;
use utils::id::{TenantId, TimelineId};

use super::delete::DeleteTenantError;
use super::secondary::SecondaryTenant;
use super::TenantSharedResources;

/// For a tenant that appears in TenantsMap, it may either be
/// - `Attached`: has a full Tenant object, is elegible to service
///    reads and ingest WAL.
/// - `Secondary`: is only keeping a local cache warm.
///
/// Secondary is a totally distinct state rather than being a mode of a `Tenant`, because
/// that way we avoid having to carefully switch a tenant's ingestion etc on and off during
/// its lifetime, and we can preserve some important safety invariants like `Tenant` always
/// having a properly acquired generation (Secondary doesn't need a generation)
#[derive(Clone)]
pub(crate) enum TenantSlot {
    Attached(Arc<Tenant>),
    Secondary(Arc<SecondaryTenant>),
    /// In this state, other administrative operations acting on the TenantId should
    /// block, or return a retry indicator equivalent to HTTP 503.
    InProgress(utils::completion::Barrier),
}

impl std::fmt::Debug for TenantSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attached(tenant) => write!(f, "Attached({})", tenant.current_state()),
            Self::Secondary(_) => write!(f, "Secondary"),
            Self::InProgress(_) => write!(f, "InProgress"),
        }
    }
}

impl TenantSlot {
    /// Return the `Tenant` in this slot if attached, else None
    fn get_attached(&self) -> Option<&Arc<Tenant>> {
        match self {
            Self::Attached(t) => Some(t),
            Self::Secondary(_) => None,
            Self::InProgress(_) => None,
        }
    }
}

/// The tenants known to the pageserver.
/// The enum variants are used to distinguish the different states that the pageserver can be in.
pub(crate) enum TenantsMap {
    /// [`init_tenant_mgr`] is not done yet.
    Initializing,
    /// [`init_tenant_mgr`] is done, all on-disk tenants have been loaded.
    /// New tenants can be added using [`tenant_map_acquire_slot`].
    Open(BTreeMap<TenantShardId, TenantSlot>),
    /// The pageserver has entered shutdown mode via [`shutdown_all_tenants`].
    /// Existing tenants are still accessible, but no new tenants can be created.
    ShuttingDown(BTreeMap<TenantShardId, TenantSlot>),
}

pub(crate) enum TenantsMapRemoveResult {
    Occupied(TenantSlot),
    Vacant,
    InProgress(utils::completion::Barrier),
}

/// When resolving a TenantId to a shard, we may be looking for the 0th
/// shard, or we might be looking for whichever shard holds a particular page.
pub(crate) enum ShardSelector {
    /// Only return the 0th shard, if it is present.  If a non-0th shard is present,
    /// ignore it.
    Zero,
    /// Pick the first shard we find for the TenantId
    First,
    /// Pick the shard that holds this key
    Page(Key),
}

impl TenantsMap {
    /// Convenience function for typical usage, where we want to get a `Tenant` object, for
    /// working with attached tenants.  If the TenantId is in the map but in Secondary state,
    /// None is returned.
    pub(crate) fn get(&self, tenant_shard_id: &TenantShardId) -> Option<&Arc<Tenant>> {
        match self {
            TenantsMap::Initializing => None,
            TenantsMap::Open(m) | TenantsMap::ShuttingDown(m) => {
                m.get(tenant_shard_id).and_then(|slot| slot.get_attached())
            }
        }
    }

    /// A page service client sends a TenantId, and to look up the correct Tenant we must
    /// resolve this to a fully qualified TenantShardId.
    fn resolve_attached_shard(
        &self,
        tenant_id: &TenantId,
        selector: ShardSelector,
    ) -> Option<TenantShardId> {
        let mut want_shard = None;
        match self {
            TenantsMap::Initializing => None,
            TenantsMap::Open(m) | TenantsMap::ShuttingDown(m) => {
                for slot in m.range(TenantShardId::tenant_range(*tenant_id)) {
                    // Ignore all slots that don't contain an attached tenant
                    let tenant = match &slot.1 {
                        TenantSlot::Attached(t) => t,
                        _ => continue,
                    };

                    match selector {
                        ShardSelector::First => return Some(*slot.0),
                        ShardSelector::Zero if slot.0.shard_number == ShardNumber(0) => {
                            return Some(*slot.0)
                        }
                        ShardSelector::Page(key) => {
                            // First slot we see for this tenant, calculate the expected shard number
                            // for the key: we will use this for checking if this and subsequent
                            // slots contain the key, rather than recalculating the hash each time.
                            if want_shard.is_none() {
                                want_shard = Some(tenant.shard_identity.get_shard_number(&key));
                            }

                            if Some(tenant.shard_identity.number) == want_shard {
                                return Some(*slot.0);
                            }
                        }
                        _ => continue,
                    }
                }

                // Fall through: we didn't find an acceptable shard
                None
            }
        }
    }

    /// Only for use from DeleteTenantFlow.  This method directly removes a TenantSlot from the map.
    ///
    /// The normal way to remove a tenant is using a SlotGuard, which will gracefully remove the guarded
    /// slot if the enclosed tenant is shutdown.
    pub(crate) fn remove(&mut self, tenant_shard_id: TenantShardId) -> TenantsMapRemoveResult {
        use std::collections::btree_map::Entry;
        match self {
            TenantsMap::Initializing => TenantsMapRemoveResult::Vacant,
            TenantsMap::Open(m) | TenantsMap::ShuttingDown(m) => match m.entry(tenant_shard_id) {
                Entry::Occupied(entry) => match entry.get() {
                    TenantSlot::InProgress(barrier) => {
                        TenantsMapRemoveResult::InProgress(barrier.clone())
                    }
                    _ => TenantsMapRemoveResult::Occupied(entry.remove()),
                },
                Entry::Vacant(_entry) => TenantsMapRemoveResult::Vacant,
            },
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            TenantsMap::Initializing => 0,
            TenantsMap::Open(m) | TenantsMap::ShuttingDown(m) => m.len(),
        }
    }
}

/// This is "safe" in that that it won't leave behind a partially deleted directory
/// at the original path, because we rename with TEMP_FILE_SUFFIX before starting deleting
/// the contents.
///
/// This is pageserver-specific, as it relies on future processes after a crash to check
/// for TEMP_FILE_SUFFIX when loading things.
async fn safe_remove_tenant_dir_all(path: impl AsRef<Utf8Path>) -> std::io::Result<()> {
    let tmp_path = safe_rename_tenant_dir(path).await?;
    fs::remove_dir_all(tmp_path).await
}

async fn safe_rename_tenant_dir(path: impl AsRef<Utf8Path>) -> std::io::Result<Utf8PathBuf> {
    let parent = path
        .as_ref()
        .parent()
        // It is invalid to call this function with a relative path.  Tenant directories
        // should always have a parent.
        .ok_or(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Path must be absolute",
        ))?;
    let rand_suffix = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect::<String>()
        + TEMP_FILE_SUFFIX;
    let tmp_path = path_with_suffix_extension(&path, &rand_suffix);
    fs::rename(path.as_ref(), &tmp_path).await?;
    fs::File::open(parent).await?.sync_all().await?;
    Ok(tmp_path)
}

static TENANTS: Lazy<std::sync::RwLock<TenantsMap>> =
    Lazy::new(|| std::sync::RwLock::new(TenantsMap::Initializing));

/// The TenantManager is responsible for storing and mutating the collection of all tenants
/// that this pageserver process has state for.  Every Tenant and SecondaryTenant instance
/// lives inside the TenantManager.
///
/// The most important role of the TenantManager is to prevent conflicts: e.g. trying to attach
/// the same tenant twice concurrently, or trying to configure the same tenant into secondary
/// and attached modes concurrently.
pub struct TenantManager {
    conf: &'static PageServerConf,
    // TODO: currently this is a &'static pointing to TENANTs.  When we finish refactoring
    // out of that static variable, the TenantManager can own this.
    // See https://github.com/neondatabase/neon/issues/5796
    tenants: &'static std::sync::RwLock<TenantsMap>,
    resources: TenantSharedResources,
}

fn emergency_generations(
    tenant_confs: &HashMap<TenantShardId, anyhow::Result<LocationConf>>,
) -> HashMap<TenantShardId, Generation> {
    tenant_confs
        .iter()
        .filter_map(|(tid, lc)| {
            let lc = match lc {
                Ok(lc) => lc,
                Err(_) => return None,
            };
            let gen = match &lc.mode {
                LocationMode::Attached(alc) => Some(alc.generation),
                LocationMode::Secondary(_) => None,
            };

            gen.map(|g| (*tid, g))
        })
        .collect()
}

async fn init_load_generations(
    conf: &'static PageServerConf,
    tenant_confs: &HashMap<TenantShardId, anyhow::Result<LocationConf>>,
    resources: &TenantSharedResources,
    cancel: &CancellationToken,
) -> anyhow::Result<Option<HashMap<TenantShardId, Generation>>> {
    let generations = if conf.control_plane_emergency_mode {
        error!(
            "Emergency mode!  Tenants will be attached unsafely using their last known generation"
        );
        emergency_generations(tenant_confs)
    } else if let Some(client) = ControlPlaneClient::new(conf, cancel) {
        info!("Calling control plane API to re-attach tenants");
        // If we are configured to use the control plane API, then it is the source of truth for what tenants to load.
        match client.re_attach().await {
            Ok(tenants) => tenants,
            Err(RetryForeverError::ShuttingDown) => {
                anyhow::bail!("Shut down while waiting for control plane re-attach response")
            }
        }
    } else {
        info!("Control plane API not configured, tenant generations are disabled");
        return Ok(None);
    };

    // The deletion queue needs to know about the startup attachment state to decide which (if any) stored
    // deletion list entries may still be valid.  We provide that by pushing a recovery operation into
    // the queue. Sequential processing of te queue ensures that recovery is done before any new tenant deletions
    // are processed, even though we don't block on recovery completing here.
    //
    // Must only do this if remote storage is enabled, otherwise deletion queue
    // is not running and channel push will fail.
    if resources.remote_storage.is_some() {
        resources
            .deletion_queue_client
            .recover(generations.clone())?;
    }

    Ok(Some(generations))
}

/// Given a directory discovered in the pageserver's tenants/ directory, attempt
/// to load a tenant config from it.
///
/// If file is missing, return Ok(None)
fn load_tenant_config(
    conf: &'static PageServerConf,
    dentry: Utf8DirEntry,
) -> anyhow::Result<Option<(TenantShardId, anyhow::Result<LocationConf>)>> {
    let tenant_dir_path = dentry.path().to_path_buf();
    if crate::is_temporary(&tenant_dir_path) {
        info!("Found temporary tenant directory, removing: {tenant_dir_path}");
        // No need to use safe_remove_tenant_dir_all because this is already
        // a temporary path
        if let Err(e) = std::fs::remove_dir_all(&tenant_dir_path) {
            error!(
                "Failed to remove temporary directory '{}': {:?}",
                tenant_dir_path, e
            );
        }
        return Ok(None);
    }

    // This case happens if we crash during attachment before writing a config into the dir
    let is_empty = tenant_dir_path
        .is_empty_dir()
        .with_context(|| format!("Failed to check whether {tenant_dir_path:?} is an empty dir"))?;
    if is_empty {
        info!("removing empty tenant directory {tenant_dir_path:?}");
        if let Err(e) = std::fs::remove_dir(&tenant_dir_path) {
            error!(
                "Failed to remove empty tenant directory '{}': {e:#}",
                tenant_dir_path
            )
        }
        return Ok(None);
    }

    let tenant_shard_id = match tenant_dir_path
        .file_name()
        .unwrap_or_default()
        .parse::<TenantShardId>()
    {
        Ok(id) => id,
        Err(_) => {
            warn!("Invalid tenant path (garbage in our repo directory?): {tenant_dir_path}",);
            return Ok(None);
        }
    };

    // Clean up legacy `metadata` files.
    // Doing it here because every single tenant directory is visited here.
    // In any later code, there's different treatment of tenant dirs
    // ... depending on whether the tenant is in re-attach response or not
    // ... epending on whether the tenant is ignored or not
    assert_eq!(
        &conf.tenant_path(&tenant_shard_id),
        &tenant_dir_path,
        "later use of conf....path() methods would be dubious"
    );
    let timelines: Vec<TimelineId> = match conf.timelines_path(&tenant_shard_id).read_dir_utf8() {
        Ok(iter) => {
            let mut timelines = Vec::new();
            for res in iter {
                let p = res?;
                let Some(timeline_id) = p.file_name().parse::<TimelineId>().ok() else {
                    // skip any entries that aren't TimelineId, such as
                    // - *.___temp dirs
                    // - unfinished initdb uploads (test_non_uploaded_root_timeline_is_deleted_after_restart)
                    continue;
                };
                timelines.push(timeline_id);
            }
            timelines
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(anyhow::anyhow!(e)),
    };
    for timeline_id in timelines {
        let timeline_path = &conf.timeline_path(&tenant_shard_id, &timeline_id);
        let metadata_path = timeline_path.join(METADATA_FILE_NAME);
        match std::fs::remove_file(&metadata_path) {
            Ok(()) => {
                crashsafe::fsync(timeline_path)
                    .context("fsync timeline dir after removing legacy metadata file")?;
                info!("removed legacy metadata file at {metadata_path}");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // something removed the file earlier, or it was never there
                // We don't care, this software version doesn't write it again, so, we're good.
            }
            Err(e) => {
                anyhow::bail!("remove legacy metadata file: {e}: {metadata_path}");
            }
        }
    }

    let tenant_ignore_mark_file = tenant_dir_path.join(IGNORED_TENANT_FILE_NAME);
    if tenant_ignore_mark_file.exists() {
        info!("Found an ignore mark file {tenant_ignore_mark_file:?}, skipping the tenant");
        return Ok(None);
    }

    Ok(Some((
        tenant_shard_id,
        Tenant::load_tenant_config(conf, &tenant_shard_id),
    )))
}

/// Initial stage of load: walk the local tenants directory, clean up any temp files,
/// and load configurations for the tenants we found.
///
/// Do this in parallel, because we expect 10k+ tenants, so serial execution can take
/// seconds even on reasonably fast drives.
async fn init_load_tenant_configs(
    conf: &'static PageServerConf,
) -> anyhow::Result<HashMap<TenantShardId, anyhow::Result<LocationConf>>> {
    let tenants_dir = conf.tenants_path();

    let dentries = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Utf8DirEntry>> {
        let dir_entries = tenants_dir
            .read_dir_utf8()
            .with_context(|| format!("Failed to list tenants dir {tenants_dir:?}"))?;

        Ok(dir_entries.collect::<Result<Vec<_>, std::io::Error>>()?)
    })
    .await??;

    let mut configs = HashMap::new();

    let mut join_set = JoinSet::new();
    for dentry in dentries {
        join_set.spawn_blocking(move || load_tenant_config(conf, dentry));
    }

    while let Some(r) = join_set.join_next().await {
        if let Some((tenant_id, tenant_config)) = r?? {
            configs.insert(tenant_id, tenant_config);
        }
    }

    Ok(configs)
}

/// Initialize repositories with locally available timelines.
/// Timelines that are only partially available locally (remote storage has more data than this pageserver)
/// are scheduled for download and added to the tenant once download is completed.
#[instrument(skip_all)]
pub async fn init_tenant_mgr(
    conf: &'static PageServerConf,
    resources: TenantSharedResources,
    init_order: InitializationOrder,
    cancel: CancellationToken,
) -> anyhow::Result<TenantManager> {
    let mut tenants = BTreeMap::new();

    let ctx = RequestContext::todo_child(TaskKind::Startup, DownloadBehavior::Warn);

    // Scan local filesystem for attached tenants
    let tenant_configs = init_load_tenant_configs(conf).await?;

    // Determine which tenants are to be attached
    let tenant_generations =
        init_load_generations(conf, &tenant_configs, &resources, &cancel).await?;

    tracing::info!(
        "Attaching {} tenants at startup, warming up {} at a time",
        tenant_configs.len(),
        conf.concurrent_tenant_warmup.initial_permits()
    );
    TENANT.startup_scheduled.inc_by(tenant_configs.len() as u64);

    // Construct `Tenant` objects and start them running
    for (tenant_shard_id, location_conf) in tenant_configs {
        let tenant_dir_path = conf.tenant_path(&tenant_shard_id);

        let mut location_conf = match location_conf {
            Ok(l) => l,
            Err(e) => {
                warn!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), "Marking tenant broken, failed to {e:#}");

                tenants.insert(
                    tenant_shard_id,
                    TenantSlot::Attached(Tenant::create_broken_tenant(
                        conf,
                        tenant_shard_id,
                        format!("{}", e),
                    )),
                );
                continue;
            }
        };

        let generation = if let Some(generations) = &tenant_generations {
            // We have a generation map: treat it as the authority for whether
            // this tenant is really attached.
            if let Some(gen) = generations.get(&tenant_shard_id) {
                if let LocationMode::Attached(attached) = &location_conf.mode {
                    if attached.generation > *gen {
                        tracing::error!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(),
                            "Control plane gave decreasing generation ({gen:?}) in re-attach response for tenant that was attached in generation {:?}, demoting to secondary",
                            attached.generation
                        );

                        // We cannot safely attach this tenant given a bogus generation number, but let's avoid throwing away
                        // local disk content: demote to secondary rather than detaching.
                        tenants.insert(
                            tenant_shard_id,
                            TenantSlot::Secondary(SecondaryTenant::new(
                                tenant_shard_id,
                                location_conf.shard,
                                location_conf.tenant_conf.clone(),
                                &SecondaryLocationConfig { warm: false },
                            )),
                        );
                    }
                }
                *gen
            } else {
                match &location_conf.mode {
                    LocationMode::Secondary(secondary_config) => {
                        // We do not require the control plane's permission for secondary mode
                        // tenants, because they do no remote writes and hence require no
                        // generation number
                        info!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), "Loaded tenant in secondary mode");
                        tenants.insert(
                            tenant_shard_id,
                            TenantSlot::Secondary(SecondaryTenant::new(
                                tenant_shard_id,
                                location_conf.shard,
                                location_conf.tenant_conf,
                                secondary_config,
                            )),
                        );
                    }
                    LocationMode::Attached(_) => {
                        // TODO: augment re-attach API to enable the control plane to
                        // instruct us about secondary attachments.  That way, instead of throwing
                        // away local state, we can gracefully fall back to secondary here, if the control
                        // plane tells us so.
                        // (https://github.com/neondatabase/neon/issues/5377)
                        info!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), "Detaching tenant, control plane omitted it in re-attach response");
                        if let Err(e) = safe_remove_tenant_dir_all(&tenant_dir_path).await {
                            error!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(),
                                "Failed to remove detached tenant directory '{tenant_dir_path}': {e:?}",
                            );
                        }
                    }
                };

                continue;
            }
        } else {
            // Legacy mode: no generation information, any tenant present
            // on local disk may activate
            info!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), "Starting tenant in legacy mode, no generation",);
            Generation::none()
        };

        // Presence of a generation number implies attachment: attach the tenant
        // if it wasn't already, and apply the generation number.
        location_conf.attach_in_generation(generation);
        Tenant::persist_tenant_config(conf, &tenant_shard_id, &location_conf).await?;

        let shard_identity = location_conf.shard;
        match tenant_spawn(
            conf,
            tenant_shard_id,
            &tenant_dir_path,
            resources.clone(),
            AttachedTenantConf::try_from(location_conf)?,
            shard_identity,
            Some(init_order.clone()),
            &TENANTS,
            SpawnMode::Normal,
            &ctx,
        ) {
            Ok(tenant) => {
                tenants.insert(tenant_shard_id, TenantSlot::Attached(tenant));
            }
            Err(e) => {
                error!(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), "Failed to start tenant: {e:#}");
            }
        }
    }

    info!("Processed {} local tenants at startup", tenants.len());

    let mut tenants_map = TENANTS.write().unwrap();
    assert!(matches!(&*tenants_map, &TenantsMap::Initializing));
    METRICS.tenant_slots.set(tenants.len() as u64);
    *tenants_map = TenantsMap::Open(tenants);

    Ok(TenantManager {
        conf,
        tenants: &TENANTS,
        resources,
    })
}

/// Wrapper for Tenant::spawn that checks invariants before running, and inserts
/// a broken tenant in the map if Tenant::spawn fails.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tenant_spawn(
    conf: &'static PageServerConf,
    tenant_shard_id: TenantShardId,
    tenant_path: &Utf8Path,
    resources: TenantSharedResources,
    location_conf: AttachedTenantConf,
    shard_identity: ShardIdentity,
    init_order: Option<InitializationOrder>,
    tenants: &'static std::sync::RwLock<TenantsMap>,
    mode: SpawnMode,
    ctx: &RequestContext,
) -> anyhow::Result<Arc<Tenant>> {
    anyhow::ensure!(
        tenant_path.is_dir(),
        "Cannot load tenant from path {tenant_path:?}, it either does not exist or not a directory"
    );
    anyhow::ensure!(
        !crate::is_temporary(tenant_path),
        "Cannot load tenant from temporary path {tenant_path:?}"
    );
    anyhow::ensure!(
        !tenant_path.is_empty_dir().with_context(|| {
            format!("Failed to check whether {tenant_path:?} is an empty dir")
        })?,
        "Cannot load tenant from empty directory {tenant_path:?}"
    );

    let tenant_ignore_mark = conf.tenant_ignore_mark_file_path(&tenant_shard_id);
    anyhow::ensure!(
        !conf.tenant_ignore_mark_file_path(&tenant_shard_id).exists(),
        "Cannot load tenant, ignore mark found at {tenant_ignore_mark:?}"
    );

    let tenant = match Tenant::spawn(
        conf,
        tenant_shard_id,
        resources,
        location_conf,
        shard_identity,
        init_order,
        tenants,
        mode,
        ctx,
    ) {
        Ok(tenant) => tenant,
        Err(e) => {
            error!("Failed to spawn tenant {tenant_shard_id}, reason: {e:#}");
            Tenant::create_broken_tenant(conf, tenant_shard_id, format!("{e:#}"))
        }
    };

    Ok(tenant)
}

///
/// Shut down all tenants. This runs as part of pageserver shutdown.
///
/// NB: We leave the tenants in the map, so that they remain accessible through
/// the management API until we shut it down. If we removed the shut-down tenants
/// from the tenants map, the management API would return 404 for these tenants,
/// because TenantsMap::get() now returns `None`.
/// That could be easily misinterpreted by control plane, the consumer of the
/// management API. For example, it could attach the tenant on a different pageserver.
/// We would then be in split-brain once this pageserver restarts.
#[instrument(skip_all)]
pub(crate) async fn shutdown_all_tenants() {
    shutdown_all_tenants0(&TENANTS).await
}

async fn shutdown_all_tenants0(tenants: &std::sync::RwLock<TenantsMap>) {
    let mut join_set = JoinSet::new();

    // Atomically, 1. create the shutdown tasks and 2. prevent creation of new tenants.
    let (total_in_progress, total_attached) = {
        let mut m = tenants.write().unwrap();
        match &mut *m {
            TenantsMap::Initializing => {
                *m = TenantsMap::ShuttingDown(BTreeMap::default());
                info!("tenants map is empty");
                return;
            }
            TenantsMap::Open(tenants) => {
                let mut shutdown_state = BTreeMap::new();
                let mut total_in_progress = 0;
                let mut total_attached = 0;

                for (tenant_shard_id, v) in std::mem::take(tenants).into_iter() {
                    match v {
                        TenantSlot::Attached(t) => {
                            shutdown_state.insert(tenant_shard_id, TenantSlot::Attached(t.clone()));
                            join_set.spawn(
                                async move {
                                    let freeze_and_flush = true;

                                    let res = {
                                        let (_guard, shutdown_progress) = completion::channel();
                                        t.shutdown(shutdown_progress, freeze_and_flush).await
                                    };

                                    if let Err(other_progress) = res {
                                        // join the another shutdown in progress
                                        other_progress.wait().await;
                                    }

                                    // we cannot afford per tenant logging here, because if s3 is degraded, we are
                                    // going to log too many lines
                                    debug!("tenant successfully stopped");
                                }
                                .instrument(info_span!("shutdown", tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug())),
                            );

                            total_attached += 1;
                        }
                        TenantSlot::Secondary(state) => {
                            // We don't need to wait for this individually per-tenant: the
                            // downloader task will be waited on eventually, this cancel
                            // is just to encourage it to drop out if it is doing work
                            // for this tenant right now.
                            state.cancel.cancel();

                            shutdown_state.insert(tenant_shard_id, TenantSlot::Secondary(state));
                        }
                        TenantSlot::InProgress(notify) => {
                            // InProgress tenants are not visible in TenantsMap::ShuttingDown: we will
                            // wait for their notifications to fire in this function.
                            join_set.spawn(async move {
                                notify.wait().await;
                            });

                            total_in_progress += 1;
                        }
                    }
                }
                *m = TenantsMap::ShuttingDown(shutdown_state);
                (total_in_progress, total_attached)
            }
            TenantsMap::ShuttingDown(_) => {
                error!("already shutting down, this function isn't supposed to be called more than once");
                return;
            }
        }
    };

    let started_at = std::time::Instant::now();

    info!(
        "Waiting for {} InProgress tenants and {} Attached tenants to shut down",
        total_in_progress, total_attached
    );

    let total = join_set.len();
    let mut panicked = 0;
    let mut buffering = true;
    const BUFFER_FOR: std::time::Duration = std::time::Duration::from_millis(500);
    let mut buffered = std::pin::pin!(tokio::time::sleep(BUFFER_FOR));

    while !join_set.is_empty() {
        tokio::select! {
            Some(joined) = join_set.join_next() => {
                match joined {
                    Ok(()) => {},
                    Err(join_error) if join_error.is_cancelled() => {
                        unreachable!("we are not cancelling any of the tasks");
                    }
                    Err(join_error) if join_error.is_panic() => {
                        // cannot really do anything, as this panic is likely a bug
                        panicked += 1;
                    }
                    Err(join_error) => {
                        warn!("unknown kind of JoinError: {join_error}");
                    }
                }
                if !buffering {
                    // buffer so that every 500ms since the first update (or starting) we'll log
                    // how far away we are; this is because we will get SIGKILL'd at 10s, and we
                    // are not able to log *then*.
                    buffering = true;
                    buffered.as_mut().reset(tokio::time::Instant::now() + BUFFER_FOR);
                }
            },
            _ = &mut buffered, if buffering => {
                buffering = false;
                info!(remaining = join_set.len(), total, elapsed_ms = started_at.elapsed().as_millis(), "waiting for tenants to shutdown");
            }
        }
    }

    if panicked > 0 {
        warn!(
            panicked,
            total, "observed panicks while shutting down tenants"
        );
    }

    // caller will log how long we took
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SetNewTenantConfigError {
    #[error(transparent)]
    GetTenant(#[from] GetTenantError),
    #[error(transparent)]
    Persist(anyhow::Error),
    #[error(transparent)]
    Other(anyhow::Error),
}

pub(crate) async fn set_new_tenant_config(
    conf: &'static PageServerConf,
    new_tenant_conf: TenantConfOpt,
    tenant_id: TenantId,
) -> Result<(), SetNewTenantConfigError> {
    // Legacy API: does not support sharding
    let tenant_shard_id = TenantShardId::unsharded(tenant_id);

    info!("configuring tenant {tenant_id}");
    let tenant = get_tenant(tenant_shard_id, true)?;

    if !tenant.tenant_shard_id().shard_count.is_unsharded() {
        // Note that we use ShardParameters::default below.
        return Err(SetNewTenantConfigError::Other(anyhow::anyhow!(
            "This API may only be used on single-sharded tenants, use the /location_config API for sharded tenants"
        )));
    }

    // This is a legacy API that only operates on attached tenants: the preferred
    // API to use is the location_config/ endpoint, which lets the caller provide
    // the full LocationConf.
    let location_conf = LocationConf::attached_single(
        new_tenant_conf.clone(),
        tenant.generation,
        &ShardParameters::default(),
    );

    Tenant::persist_tenant_config(conf, &tenant_shard_id, &location_conf)
        .await
        .map_err(SetNewTenantConfigError::Persist)?;
    tenant.set_new_tenant_config(new_tenant_conf);
    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum UpsertLocationError {
    #[error("Bad config request: {0}")]
    BadRequest(anyhow::Error),

    #[error("Cannot change config in this state: {0}")]
    Unavailable(#[from] TenantMapError),

    #[error("Tenant is already being modified")]
    InProgress,

    #[error("Failed to flush: {0}")]
    Flush(anyhow::Error),

    #[error("Internal error: {0}")]
    Other(#[from] anyhow::Error),
}

impl TenantManager {
    /// Convenience function so that anyone with a TenantManager can get at the global configuration, without
    /// having to pass it around everywhere as a separate object.
    pub(crate) fn get_conf(&self) -> &'static PageServerConf {
        self.conf
    }

    /// Gets the attached tenant from the in-memory data, erroring if it's absent, in secondary mode, or is not fitting to the query.
    /// `active_only = true` allows to query only tenants that are ready for operations, erroring on other kinds of tenants.
    pub(crate) fn get_attached_tenant_shard(
        &self,
        tenant_shard_id: TenantShardId,
        active_only: bool,
    ) -> Result<Arc<Tenant>, GetTenantError> {
        let locked = self.tenants.read().unwrap();

        let peek_slot = tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Read)?;

        match peek_slot {
            Some(TenantSlot::Attached(tenant)) => match tenant.current_state() {
                TenantState::Broken {
                    reason,
                    backtrace: _,
                } if active_only => Err(GetTenantError::Broken(reason)),
                TenantState::Active => Ok(Arc::clone(tenant)),
                _ => {
                    if active_only {
                        Err(GetTenantError::NotActive(tenant_shard_id))
                    } else {
                        Ok(Arc::clone(tenant))
                    }
                }
            },
            Some(TenantSlot::InProgress(_)) => Err(GetTenantError::NotActive(tenant_shard_id)),
            None | Some(TenantSlot::Secondary(_)) => {
                Err(GetTenantError::NotFound(tenant_shard_id.tenant_id))
            }
        }
    }

    pub(crate) fn get_secondary_tenant_shard(
        &self,
        tenant_shard_id: TenantShardId,
    ) -> Option<Arc<SecondaryTenant>> {
        let locked = self.tenants.read().unwrap();

        let peek_slot = tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Read)
            .ok()
            .flatten();

        match peek_slot {
            Some(TenantSlot::Secondary(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Whether the `TenantManager` is responsible for the tenant shard
    pub(crate) fn manages_tenant_shard(&self, tenant_shard_id: TenantShardId) -> bool {
        let locked = self.tenants.read().unwrap();

        let peek_slot = tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Read)
            .ok()
            .flatten();

        peek_slot.is_some()
    }

    #[instrument(skip_all, fields(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug()))]
    pub(crate) async fn upsert_location(
        &self,
        tenant_shard_id: TenantShardId,
        new_location_config: LocationConf,
        flush: Option<Duration>,
        mut spawn_mode: SpawnMode,
        ctx: &RequestContext,
    ) -> Result<Option<Arc<Tenant>>, UpsertLocationError> {
        debug_assert_current_span_has_tenant_id();
        info!("configuring tenant location to state {new_location_config:?}");

        enum FastPathModified {
            Attached(Arc<Tenant>),
            Secondary(Arc<SecondaryTenant>),
        }

        // Special case fast-path for updates to existing slots: if our upsert is only updating configuration,
        // then we do not need to set the slot to InProgress, we can just call into the
        // existng tenant.
        let fast_path_taken = {
            let locked = self.tenants.read().unwrap();
            let peek_slot =
                tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Write)?;
            match (&new_location_config.mode, peek_slot) {
                (LocationMode::Attached(attach_conf), Some(TenantSlot::Attached(tenant))) => {
                    match attach_conf.generation.cmp(&tenant.generation) {
                        Ordering::Equal => {
                            // A transition from Attached to Attached in the same generation, we may
                            // take our fast path and just provide the updated configuration
                            // to the tenant.
                            tenant.set_new_location_config(
                                AttachedTenantConf::try_from(new_location_config.clone())
                                    .map_err(UpsertLocationError::BadRequest)?,
                            );

                            Some(FastPathModified::Attached(tenant.clone()))
                        }
                        Ordering::Less => {
                            return Err(UpsertLocationError::BadRequest(anyhow::anyhow!(
                                "Generation {:?} is less than existing {:?}",
                                attach_conf.generation,
                                tenant.generation
                            )));
                        }
                        Ordering::Greater => {
                            // Generation advanced, fall through to general case of replacing `Tenant` object
                            None
                        }
                    }
                }
                (
                    LocationMode::Secondary(secondary_conf),
                    Some(TenantSlot::Secondary(secondary_tenant)),
                ) => {
                    secondary_tenant.set_config(secondary_conf);
                    secondary_tenant.set_tenant_conf(&new_location_config.tenant_conf);
                    Some(FastPathModified::Secondary(secondary_tenant.clone()))
                }
                _ => {
                    // Not an Attached->Attached transition, fall through to general case
                    None
                }
            }
        };

        // Fast-path continued: having dropped out of the self.tenants lock, do the async
        // phase of writing config and/or waiting for flush, before returning.
        match fast_path_taken {
            Some(FastPathModified::Attached(tenant)) => {
                Tenant::persist_tenant_config(self.conf, &tenant_shard_id, &new_location_config)
                    .await?;

                // Transition to AttachedStale means we may well hold a valid generation
                // still, and have been requested to go stale as part of a migration.  If
                // the caller set `flush`, then flush to remote storage.
                if let LocationMode::Attached(AttachedLocationConfig {
                    generation: _,
                    attach_mode: AttachmentMode::Stale,
                }) = &new_location_config.mode
                {
                    if let Some(flush_timeout) = flush {
                        match tokio::time::timeout(flush_timeout, tenant.flush_remote()).await {
                            Ok(Err(e)) => {
                                return Err(UpsertLocationError::Flush(e));
                            }
                            Ok(Ok(_)) => return Ok(Some(tenant)),
                            Err(_) => {
                                tracing::warn!(
                                timeout_ms = flush_timeout.as_millis(),
                                "Timed out waiting for flush to remote storage, proceeding anyway."
                            )
                            }
                        }
                    }
                }

                return Ok(Some(tenant));
            }
            Some(FastPathModified::Secondary(_secondary_tenant)) => {
                Tenant::persist_tenant_config(self.conf, &tenant_shard_id, &new_location_config)
                    .await?;

                return Ok(None);
            }
            None => {
                // Proceed with the general case procedure, where we will shutdown & remove any existing
                // slot contents and replace with a fresh one
            }
        };

        // General case for upserts to TenantsMap, excluding the case above: we will substitute an
        // InProgress value to the slot while we make whatever changes are required.  The state for
        // the tenant is inaccessible to the outside world while we are doing this, but that is sensible:
        // the state is ill-defined while we're in transition.  Transitions are async, but fast: we do
        // not do significant I/O, and shutdowns should be prompt via cancellation tokens.
        let mut slot_guard = tenant_map_acquire_slot(&tenant_shard_id, TenantSlotAcquireMode::Any)
            .map_err(|e| match e {
                TenantSlotError::AlreadyExists(_, _) | TenantSlotError::NotFound(_) => {
                    unreachable!("Called with mode Any")
                }
                TenantSlotError::InProgress => UpsertLocationError::InProgress,
                TenantSlotError::MapState(s) => UpsertLocationError::Unavailable(s),
            })?;

        match slot_guard.get_old_value() {
            Some(TenantSlot::Attached(tenant)) => {
                // The case where we keep a Tenant alive was covered above in the special case
                // for Attached->Attached transitions in the same generation.  By this point,
                // if we see an attached tenant we know it will be discarded and should be
                // shut down.
                let (_guard, progress) = utils::completion::channel();

                match tenant.get_attach_mode() {
                    AttachmentMode::Single | AttachmentMode::Multi => {
                        // Before we leave our state as the presumed holder of the latest generation,
                        // flush any outstanding deletions to reduce the risk of leaking objects.
                        self.resources.deletion_queue_client.flush_advisory()
                    }
                    AttachmentMode::Stale => {
                        // If we're stale there's not point trying to flush deletions
                    }
                };

                info!("Shutting down attached tenant");
                match tenant.shutdown(progress, false).await {
                    Ok(()) => {}
                    Err(barrier) => {
                        info!("Shutdown already in progress, waiting for it to complete");
                        barrier.wait().await;
                    }
                }
                slot_guard.drop_old_value().expect("We just shut it down");

                // Edge case: if we were called with SpawnMode::Create, but a Tenant already existed, then
                // the caller thinks they're creating but the tenant already existed.  We must switch to
                // Normal mode so that when starting this Tenant we properly probe remote storage for timelines,
                // rather than assuming it to be empty.
                spawn_mode = SpawnMode::Normal;
            }
            Some(TenantSlot::Secondary(state)) => {
                info!("Shutting down secondary tenant");
                state.shutdown().await;
            }
            Some(TenantSlot::InProgress(_)) => {
                // This should never happen: acquire_slot should error out
                // if the contents of a slot were InProgress.
                return Err(UpsertLocationError::Other(anyhow::anyhow!(
                    "Acquired an InProgress slot, this is a bug."
                )));
            }
            None => {
                // Slot was vacant, nothing needs shutting down.
            }
        }

        let tenant_path = self.conf.tenant_path(&tenant_shard_id);
        let timelines_path = self.conf.timelines_path(&tenant_shard_id);

        // Directory structure is the same for attached and secondary modes:
        // create it if it doesn't exist.  Timeline load/creation expects the
        // timelines/ subdir to already exist.
        //
        // Does not need to be fsync'd because local storage is just a cache.
        tokio::fs::create_dir_all(&timelines_path)
            .await
            .with_context(|| format!("Creating {timelines_path}"))?;

        // Before activating either secondary or attached mode, persist the
        // configuration, so that on restart we will re-attach (or re-start
        // secondary) on the tenant.
        Tenant::persist_tenant_config(self.conf, &tenant_shard_id, &new_location_config).await?;

        let new_slot = match &new_location_config.mode {
            LocationMode::Secondary(secondary_config) => {
                let shard_identity = new_location_config.shard;
                TenantSlot::Secondary(SecondaryTenant::new(
                    tenant_shard_id,
                    shard_identity,
                    new_location_config.tenant_conf,
                    secondary_config,
                ))
            }
            LocationMode::Attached(_attach_config) => {
                let shard_identity = new_location_config.shard;

                // Testing hack: if we are configured with no control plane, then drop the generation
                // from upserts.  This enables creating generation-less tenants even though neon_local
                // always uses generations when calling the location conf API.
                let attached_conf = if cfg!(feature = "testing") {
                    let mut conf = AttachedTenantConf::try_from(new_location_config)?;
                    if self.conf.control_plane_api.is_none() {
                        conf.location.generation = Generation::none();
                    }
                    conf
                } else {
                    AttachedTenantConf::try_from(new_location_config)?
                };

                let tenant = tenant_spawn(
                    self.conf,
                    tenant_shard_id,
                    &tenant_path,
                    self.resources.clone(),
                    attached_conf,
                    shard_identity,
                    None,
                    self.tenants,
                    spawn_mode,
                    ctx,
                )?;

                TenantSlot::Attached(tenant)
            }
        };

        let attached_tenant = if let TenantSlot::Attached(tenant) = &new_slot {
            Some(tenant.clone())
        } else {
            None
        };

        match slot_guard.upsert(new_slot) {
            Err(TenantSlotUpsertError::InternalError(e)) => {
                Err(UpsertLocationError::Other(anyhow::anyhow!(e)))
            }
            Err(TenantSlotUpsertError::MapState(e)) => Err(UpsertLocationError::Unavailable(e)),
            Err(TenantSlotUpsertError::ShuttingDown((new_slot, _completion))) => {
                // If we just called tenant_spawn() on a new tenant, and can't insert it into our map, then
                // we must not leak it: this would violate the invariant that after shutdown_all_tenants, all tenants
                // are shutdown.
                //
                // We must shut it down inline here.
                match new_slot {
                    TenantSlot::InProgress(_) => {
                        // Unreachable because we never insert an InProgress
                        unreachable!()
                    }
                    TenantSlot::Attached(tenant) => {
                        let (_guard, progress) = utils::completion::channel();
                        info!("Shutting down just-spawned tenant, because tenant manager is shut down");
                        match tenant.shutdown(progress, false).await {
                            Ok(()) => {
                                info!("Finished shutting down just-spawned tenant");
                            }
                            Err(barrier) => {
                                info!("Shutdown already in progress, waiting for it to complete");
                                barrier.wait().await;
                            }
                        }
                    }
                    TenantSlot::Secondary(secondary_tenant) => {
                        secondary_tenant.shutdown().await;
                    }
                }

                Err(UpsertLocationError::Unavailable(
                    TenantMapError::ShuttingDown,
                ))
            }
            Ok(()) => Ok(attached_tenant),
        }
    }

    /// Resetting a tenant is equivalent to detaching it, then attaching it again with the same
    /// LocationConf that was last used to attach it.  Optionally, the local file cache may be
    /// dropped before re-attaching.
    ///
    /// This is not part of a tenant's normal lifecycle: it is used for debug/support, in situations
    /// where an issue is identified that would go away with a restart of the tenant.
    ///
    /// This does not have any special "force" shutdown of a tenant: it relies on the tenant's tasks
    /// to respect the cancellation tokens used in normal shutdown().
    #[instrument(skip_all, fields(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), %drop_cache))]
    pub(crate) async fn reset_tenant(
        &self,
        tenant_shard_id: TenantShardId,
        drop_cache: bool,
        ctx: &RequestContext,
    ) -> anyhow::Result<()> {
        let mut slot_guard = tenant_map_acquire_slot(&tenant_shard_id, TenantSlotAcquireMode::Any)?;
        let Some(old_slot) = slot_guard.get_old_value() else {
            anyhow::bail!("Tenant not found when trying to reset");
        };

        let Some(tenant) = old_slot.get_attached() else {
            slot_guard.revert();
            anyhow::bail!("Tenant is not in attached state");
        };

        let (_guard, progress) = utils::completion::channel();
        match tenant.shutdown(progress, false).await {
            Ok(()) => {
                slot_guard.drop_old_value()?;
            }
            Err(_barrier) => {
                slot_guard.revert();
                anyhow::bail!("Cannot reset Tenant, already shutting down");
            }
        }

        let tenant_path = self.conf.tenant_path(&tenant_shard_id);
        let timelines_path = self.conf.timelines_path(&tenant_shard_id);
        let config = Tenant::load_tenant_config(self.conf, &tenant_shard_id)?;

        if drop_cache {
            tracing::info!("Dropping local file cache");

            match tokio::fs::read_dir(&timelines_path).await {
                Err(e) => {
                    tracing::warn!("Failed to list timelines while dropping cache: {}", e);
                }
                Ok(mut entries) => {
                    while let Some(entry) = entries.next_entry().await? {
                        tokio::fs::remove_dir_all(entry.path()).await?;
                    }
                }
            }
        }

        let shard_identity = config.shard;
        let tenant = tenant_spawn(
            self.conf,
            tenant_shard_id,
            &tenant_path,
            self.resources.clone(),
            AttachedTenantConf::try_from(config)?,
            shard_identity,
            None,
            self.tenants,
            SpawnMode::Normal,
            ctx,
        )?;

        slot_guard.upsert(TenantSlot::Attached(tenant))?;

        Ok(())
    }

    pub(crate) fn get_attached_active_tenant_shards(&self) -> Vec<Arc<Tenant>> {
        let locked = self.tenants.read().unwrap();
        match &*locked {
            TenantsMap::Initializing => Vec::new(),
            TenantsMap::Open(map) | TenantsMap::ShuttingDown(map) => map
                .values()
                .filter_map(|slot| {
                    slot.get_attached()
                        .and_then(|t| if t.is_active() { Some(t.clone()) } else { None })
                })
                .collect(),
        }
    }
    // Do some synchronous work for all tenant slots in Secondary state.  The provided
    // callback should be small and fast, as it will be called inside the global
    // TenantsMap lock.
    pub(crate) fn foreach_secondary_tenants<F>(&self, mut func: F)
    where
        // TODO: let the callback return a hint to drop out of the loop early
        F: FnMut(&TenantShardId, &Arc<SecondaryTenant>),
    {
        let locked = self.tenants.read().unwrap();

        let map = match &*locked {
            TenantsMap::Initializing | TenantsMap::ShuttingDown(_) => return,
            TenantsMap::Open(m) => m,
        };

        for (tenant_id, slot) in map {
            if let TenantSlot::Secondary(state) = slot {
                // Only expose secondary tenants that are not currently shutting down
                if !state.cancel.is_cancelled() {
                    func(tenant_id, state)
                }
            }
        }
    }

    /// Total list of all tenant slots: this includes attached, secondary, and InProgress.
    pub(crate) fn list(&self) -> Vec<(TenantShardId, TenantSlot)> {
        let locked = self.tenants.read().unwrap();
        match &*locked {
            TenantsMap::Initializing => Vec::new(),
            TenantsMap::Open(map) | TenantsMap::ShuttingDown(map) => {
                map.iter().map(|(k, v)| (*k, v.clone())).collect()
            }
        }
    }

    pub(crate) async fn delete_tenant(
        &self,
        tenant_shard_id: TenantShardId,
        activation_timeout: Duration,
    ) -> Result<(), DeleteTenantError> {
        super::span::debug_assert_current_span_has_tenant_id();
        // We acquire a SlotGuard during this function to protect against concurrent
        // changes while the ::prepare phase of DeleteTenantFlow executes, but then
        // have to return the Tenant to the map while the background deletion runs.
        //
        // TODO: refactor deletion to happen outside the lifetime of a Tenant.
        // Currently, deletion requires a reference to the tenants map in order to
        // keep the Tenant in the map until deletion is complete, and then remove
        // it at the end.
        //
        // See https://github.com/neondatabase/neon/issues/5080

        let slot_guard =
            tenant_map_acquire_slot(&tenant_shard_id, TenantSlotAcquireMode::MustExist)?;

        // unwrap is safe because we used MustExist mode when acquiring
        let tenant = match slot_guard.get_old_value().as_ref().unwrap() {
            TenantSlot::Attached(tenant) => tenant.clone(),
            _ => {
                // Express "not attached" as equivalent to "not found"
                return Err(DeleteTenantError::NotAttached);
            }
        };

        match tenant.current_state() {
            TenantState::Broken { .. } | TenantState::Stopping { .. } => {
                // If a tenant is broken or stopping, DeleteTenantFlow can
                // handle it: broken tenants proceed to delete, stopping tenants
                // are checked for deletion already in progress.
            }
            _ => {
                tenant
                    .wait_to_become_active(activation_timeout)
                    .await
                    .map_err(|e| match e {
                        GetActiveTenantError::WillNotBecomeActive(_) => {
                            DeleteTenantError::InvalidState(tenant.current_state())
                        }
                        GetActiveTenantError::Cancelled => DeleteTenantError::Cancelled,
                        GetActiveTenantError::NotFound(_) => DeleteTenantError::NotAttached,
                        GetActiveTenantError::WaitForActiveTimeout {
                            latest_state: _latest_state,
                            wait_time: _wait_time,
                        } => DeleteTenantError::InvalidState(tenant.current_state()),
                    })?;
            }
        }

        let result = DeleteTenantFlow::run(
            self.conf,
            self.resources.remote_storage.clone(),
            &TENANTS,
            tenant,
        )
        .await;

        // The Tenant goes back into the map in Stopping state, it will eventually be removed by DeleteTenantFLow
        slot_guard.revert();
        result
    }

    #[instrument(skip_all, fields(tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), new_shard_count=%new_shard_count.literal()))]
    pub(crate) async fn shard_split(
        &self,
        tenant_shard_id: TenantShardId,
        new_shard_count: ShardCount,
        ctx: &RequestContext,
    ) -> anyhow::Result<Vec<TenantShardId>> {
        let tenant = get_tenant(tenant_shard_id, true)?;

        // Plan: identify what the new child shards will be
        if new_shard_count.count() <= tenant_shard_id.shard_count.count() {
            anyhow::bail!("Requested shard count is not an increase");
        }
        let expansion_factor = new_shard_count.count() / tenant_shard_id.shard_count.count();
        if !expansion_factor.is_power_of_two() {
            anyhow::bail!("Requested split is not a power of two");
        }

        let parent_shard_identity = tenant.shard_identity;
        let parent_tenant_conf = tenant.get_tenant_conf();
        let parent_generation = tenant.generation;

        let child_shards = tenant_shard_id.split(new_shard_count);
        tracing::info!(
            "Shard {} splits into: {}",
            tenant_shard_id.to_index(),
            child_shards
                .iter()
                .map(|id| format!("{}", id.to_index()))
                .join(",")
        );

        // Phase 1: Write out child shards' remote index files, in the parent tenant's current generation
        if let Err(e) = tenant.split_prepare(&child_shards).await {
            // If [`Tenant::split_prepare`] fails, we must reload the tenant, because it might
            // have been left in a partially-shut-down state.
            tracing::warn!("Failed to prepare for split: {e}, reloading Tenant before returning");
            self.reset_tenant(tenant_shard_id, false, ctx).await?;
            return Err(e);
        }

        self.resources.deletion_queue_client.flush_advisory();

        // Phase 2: Put the parent shard to InProgress and grab a reference to the parent Tenant
        drop(tenant);
        let mut parent_slot_guard =
            tenant_map_acquire_slot(&tenant_shard_id, TenantSlotAcquireMode::Any)?;
        let parent = match parent_slot_guard.get_old_value() {
            Some(TenantSlot::Attached(t)) => t,
            Some(TenantSlot::Secondary(_)) => anyhow::bail!("Tenant location in secondary mode"),
            Some(TenantSlot::InProgress(_)) => {
                // tenant_map_acquire_slot never returns InProgress, if a slot was InProgress
                // it would return an error.
                unreachable!()
            }
            None => {
                // We don't actually need the parent shard to still be attached to do our work, but it's
                // a weird enough situation that the caller probably didn't want us to continue working
                // if they had detached the tenant they requested the split on.
                anyhow::bail!("Detached parent shard in the middle of split!")
            }
        };

        // Optimization: hardlink layers from the parent into the children, so that they don't have to
        // re-download & duplicate the data referenced in their initial IndexPart
        self.shard_split_hardlink(parent, child_shards.clone())
            .await?;

        // Take a snapshot of where the parent's WAL ingest had got to: we will wait for
        // child shards to reach this point.
        let mut target_lsns = HashMap::new();
        for timeline in parent.timelines.lock().unwrap().clone().values() {
            target_lsns.insert(timeline.timeline_id, timeline.get_last_record_lsn());
        }

        // TODO: we should have the parent shard stop its WAL ingest here, it's a waste of resources
        // and could slow down the children trying to catch up.

        // Phase 3: Spawn the child shards
        for child_shard in &child_shards {
            let mut child_shard_identity = parent_shard_identity;
            child_shard_identity.count = child_shard.shard_count;
            child_shard_identity.number = child_shard.shard_number;

            let child_location_conf = LocationConf {
                mode: LocationMode::Attached(AttachedLocationConfig {
                    generation: parent_generation,
                    attach_mode: AttachmentMode::Single,
                }),
                shard: child_shard_identity,
                tenant_conf: parent_tenant_conf.clone(),
            };

            self.upsert_location(
                *child_shard,
                child_location_conf,
                None,
                SpawnMode::Normal,
                ctx,
            )
            .await?;
        }

        // Phase 4: wait for child chards WAL ingest to catch up to target LSN
        for child_shard_id in &child_shards {
            let child_shard_id = *child_shard_id;
            let child_shard = {
                let locked = TENANTS.read().unwrap();
                let peek_slot =
                    tenant_map_peek_slot(&locked, &child_shard_id, TenantSlotPeekMode::Read)?;
                peek_slot.and_then(|s| s.get_attached()).cloned()
            };
            if let Some(t) = child_shard {
                // Wait for the child shard to become active: this should be very quick because it only
                // has to download the index_part that we just uploaded when creating it.
                if let Err(e) = t.wait_to_become_active(ACTIVE_TENANT_TIMEOUT).await {
                    // This is not fatal: we have durably created the child shard.  It just makes the
                    // split operation less seamless for clients, as we will may detach the parent
                    // shard before the child shards are fully ready to serve requests.
                    tracing::warn!("Failed to wait for shard {child_shard_id} to activate: {e}");
                    continue;
                }

                let timelines = t.timelines.lock().unwrap().clone();
                for timeline in timelines.values() {
                    let Some(target_lsn) = target_lsns.get(&timeline.timeline_id) else {
                        continue;
                    };

                    tracing::info!(
                        "Waiting for child shard {}/{} to reach target lsn {}...",
                        child_shard_id,
                        timeline.timeline_id,
                        target_lsn
                    );
                    if let Err(e) = timeline.wait_lsn(*target_lsn, ctx).await {
                        // Failure here might mean shutdown, in any case this part is an optimization
                        // and we shouldn't hold up the split operation.
                        tracing::warn!(
                            "Failed to wait for timeline {} to reach lsn {target_lsn}: {e}",
                            timeline.timeline_id
                        );
                    } else {
                        tracing::info!(
                            "Child shard {}/{} reached target lsn {}",
                            child_shard_id,
                            timeline.timeline_id,
                            target_lsn
                        );
                    }
                }
            }
        }

        // Phase 5: Shut down the parent shard, and erase it from disk
        let (_guard, progress) = completion::channel();
        match parent.shutdown(progress, false).await {
            Ok(()) => {}
            Err(other) => {
                other.wait().await;
            }
        }
        let local_tenant_directory = self.conf.tenant_path(&tenant_shard_id);
        let tmp_path = safe_rename_tenant_dir(&local_tenant_directory)
            .await
            .with_context(|| format!("local tenant directory {local_tenant_directory:?} rename"))?;
        task_mgr::spawn(
            task_mgr::BACKGROUND_RUNTIME.handle(),
            TaskKind::MgmtRequest,
            None,
            None,
            "tenant_files_delete",
            false,
            async move {
                fs::remove_dir_all(tmp_path.as_path())
                    .await
                    .with_context(|| format!("tenant directory {:?} deletion", tmp_path))
            },
        );

        parent_slot_guard.drop_old_value()?;

        // Phase 6: Release the InProgress on the parent shard
        drop(parent_slot_guard);

        Ok(child_shards)
    }

    /// Part of [`Self::shard_split`]: hard link parent shard layers into child shards, as an optimization
    /// to avoid the children downloading them again.
    ///
    /// For each resident layer in the parent shard, we will hard link it into all of the child shards.
    async fn shard_split_hardlink(
        &self,
        parent_shard: &Tenant,
        child_shards: Vec<TenantShardId>,
    ) -> anyhow::Result<()> {
        debug_assert_current_span_has_tenant_id();

        let parent_path = self.conf.tenant_path(parent_shard.get_tenant_shard_id());
        let (parent_timelines, parent_layers) = {
            let mut parent_layers = Vec::new();
            let timelines = parent_shard.timelines.lock().unwrap().clone();
            let parent_timelines = timelines.keys().cloned().collect::<Vec<_>>();
            for timeline in timelines.values() {
                let timeline_layers = timeline
                    .layers
                    .read()
                    .await
                    .resident_layers()
                    .collect::<Vec<_>>()
                    .await;
                for layer in timeline_layers {
                    let relative_path = layer
                        .local_path()
                        .strip_prefix(&parent_path)
                        .context("Removing prefix from parent layer path")?;
                    parent_layers.push(relative_path.to_owned());
                }
            }
            debug_assert!(
                !parent_layers.is_empty(),
                "shutdown cannot empty the layermap"
            );
            (parent_timelines, parent_layers)
        };

        let mut child_prefixes = Vec::new();
        let mut create_dirs = Vec::new();

        for child in child_shards {
            let child_prefix = self.conf.tenant_path(&child);
            create_dirs.push(child_prefix.clone());
            create_dirs.extend(
                parent_timelines
                    .iter()
                    .map(|t| self.conf.timeline_path(&child, t)),
            );

            child_prefixes.push(child_prefix);
        }

        // Since we will do a large number of small filesystem metadata operations, batch them into
        // spawn_blocking calls rather than doing each one as a tokio::fs round-trip.
        let jh = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            for dir in &create_dirs {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    // Ignore AlreadyExists errors, drop out on all other errors
                    match e.kind() {
                        std::io::ErrorKind::AlreadyExists => {}
                        _ => {
                            return Err(anyhow::anyhow!(e).context(format!("Creating {dir}")));
                        }
                    }
                }
            }

            for child_prefix in child_prefixes {
                for relative_layer in &parent_layers {
                    let parent_path = parent_path.join(relative_layer);
                    let child_path = child_prefix.join(relative_layer);
                    if let Err(e) = std::fs::hard_link(&parent_path, &child_path) {
                        match e.kind() {
                            std::io::ErrorKind::AlreadyExists => {}
                            std::io::ErrorKind::NotFound => {
                                tracing::info!(
                                    "Layer {} not found during hard-linking, evicted during split?",
                                    relative_layer
                                );
                            }
                            _ => {
                                return Err(anyhow::anyhow!(e).context(format!(
                                    "Hard linking {relative_layer} into {child_prefix}"
                                )))
                            }
                        }
                    }
                }
            }

            // Durability is not required for correctness, but if we crashed during split and
            // then came restarted with empty timeline dirs, it would be very inefficient to
            // re-populate from remote storage.
            for dir in create_dirs {
                if let Err(e) = crashsafe::fsync(&dir) {
                    // Something removed a newly created timeline dir out from underneath us?  Extremely
                    // unexpected, but not worth panic'ing over as this whole function is just an
                    // optimization.
                    tracing::warn!("Failed to fsync directory {dir}: {e}")
                }
            }

            Ok(parent_layers.len())
        });

        match jh.await {
            Ok(Ok(layer_count)) => {
                tracing::info!(count = layer_count, "Hard linked layers into child shards");
            }
            Ok(Err(e)) => {
                // This is an optimization, so we tolerate failure.
                tracing::warn!("Error hard-linking layers, proceeding anyway: {e}")
            }
            Err(e) => {
                // This is something totally unexpected like a panic, so bail out.
                anyhow::bail!("Error joining hard linking task: {e}");
            }
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GetTenantError {
    /// NotFound is a TenantId rather than TenantShardId, because this error type is used from
    /// getters that use a TenantId and a ShardSelector, not just getters that target a specific shard.
    #[error("Tenant {0} not found")]
    NotFound(TenantId),

    #[error("Tenant {0} is not active")]
    NotActive(TenantShardId),
    /// Broken is logically a subset of NotActive, but a distinct error is useful as
    /// NotActive is usually a retryable state for API purposes, whereas Broken
    /// is a stuck error state
    #[error("Tenant is broken: {0}")]
    Broken(String),

    // Initializing or shutting down: cannot authoritatively say whether we have this tenant
    #[error("Tenant map is not available: {0}")]
    MapState(#[from] TenantMapError),
}

/// Gets the tenant from the in-memory data, erroring if it's absent or is not fitting to the query.
/// `active_only = true` allows to query only tenants that are ready for operations, erroring on other kinds of tenants.
///
/// This method is cancel-safe.
pub(crate) fn get_tenant(
    tenant_shard_id: TenantShardId,
    active_only: bool,
) -> Result<Arc<Tenant>, GetTenantError> {
    let locked = TENANTS.read().unwrap();

    let peek_slot = tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Read)?;

    match peek_slot {
        Some(TenantSlot::Attached(tenant)) => match tenant.current_state() {
            TenantState::Broken {
                reason,
                backtrace: _,
            } if active_only => Err(GetTenantError::Broken(reason)),
            TenantState::Active => Ok(Arc::clone(tenant)),
            _ => {
                if active_only {
                    Err(GetTenantError::NotActive(tenant_shard_id))
                } else {
                    Ok(Arc::clone(tenant))
                }
            }
        },
        Some(TenantSlot::InProgress(_)) => Err(GetTenantError::NotActive(tenant_shard_id)),
        None | Some(TenantSlot::Secondary(_)) => {
            Err(GetTenantError::NotFound(tenant_shard_id.tenant_id))
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum GetActiveTenantError {
    /// We may time out either while TenantSlot is InProgress, or while the Tenant
    /// is in a non-Active state
    #[error(
        "Timed out waiting {wait_time:?} for tenant active state. Latest state: {latest_state:?}"
    )]
    WaitForActiveTimeout {
        latest_state: Option<TenantState>,
        wait_time: Duration,
    },

    /// The TenantSlot is absent, or in secondary mode
    #[error(transparent)]
    NotFound(#[from] GetTenantError),

    /// Cancellation token fired while we were waiting
    #[error("cancelled")]
    Cancelled,

    /// Tenant exists, but is in a state that cannot become active (e.g. Stopping, Broken)
    #[error("will not become active.  Current state: {0}")]
    WillNotBecomeActive(TenantState),
}

/// Get a [`Tenant`] in its active state. If the tenant_id is currently in [`TenantSlot::InProgress`]
/// state, then wait for up to `timeout`.  If the [`Tenant`] is not currently in [`TenantState::Active`],
/// then wait for up to `timeout` (minus however long we waited for the slot).
pub(crate) async fn get_active_tenant_with_timeout(
    tenant_id: TenantId,
    shard_selector: ShardSelector,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Arc<Tenant>, GetActiveTenantError> {
    enum WaitFor {
        Barrier(utils::completion::Barrier),
        Tenant(Arc<Tenant>),
    }

    let wait_start = Instant::now();
    let deadline = wait_start + timeout;

    let (wait_for, tenant_shard_id) = {
        let locked = TENANTS.read().unwrap();

        // Resolve TenantId to TenantShardId
        let tenant_shard_id = locked
            .resolve_attached_shard(&tenant_id, shard_selector)
            .ok_or(GetActiveTenantError::NotFound(GetTenantError::NotFound(
                tenant_id,
            )))?;

        let peek_slot = tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Read)
            .map_err(GetTenantError::MapState)?;
        match peek_slot {
            Some(TenantSlot::Attached(tenant)) => {
                match tenant.current_state() {
                    TenantState::Active => {
                        // Fast path: we don't need to do any async waiting.
                        return Ok(tenant.clone());
                    }
                    _ => {
                        tenant.activate_now();
                        (WaitFor::Tenant(tenant.clone()), tenant_shard_id)
                    }
                }
            }
            Some(TenantSlot::Secondary(_)) => {
                return Err(GetActiveTenantError::NotFound(GetTenantError::NotActive(
                    tenant_shard_id,
                )))
            }
            Some(TenantSlot::InProgress(barrier)) => {
                (WaitFor::Barrier(barrier.clone()), tenant_shard_id)
            }
            None => {
                return Err(GetActiveTenantError::NotFound(GetTenantError::NotFound(
                    tenant_id,
                )))
            }
        }
    };

    let tenant = match wait_for {
        WaitFor::Barrier(barrier) => {
            tracing::debug!("Waiting for tenant InProgress state to pass...");
            timeout_cancellable(
                deadline.duration_since(Instant::now()),
                cancel,
                barrier.wait(),
            )
            .await
            .map_err(|e| match e {
                TimeoutCancellableError::Timeout => GetActiveTenantError::WaitForActiveTimeout {
                    latest_state: None,
                    wait_time: wait_start.elapsed(),
                },
                TimeoutCancellableError::Cancelled => GetActiveTenantError::Cancelled,
            })?;
            {
                let locked = TENANTS.read().unwrap();
                let peek_slot =
                    tenant_map_peek_slot(&locked, &tenant_shard_id, TenantSlotPeekMode::Read)
                        .map_err(GetTenantError::MapState)?;
                match peek_slot {
                    Some(TenantSlot::Attached(tenant)) => tenant.clone(),
                    _ => {
                        return Err(GetActiveTenantError::NotFound(GetTenantError::NotActive(
                            tenant_shard_id,
                        )))
                    }
                }
            }
        }
        WaitFor::Tenant(tenant) => tenant,
    };

    tracing::debug!("Waiting for tenant to enter active state...");
    tenant
        .wait_to_become_active(deadline.duration_since(Instant::now()))
        .await?;
    Ok(tenant)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeleteTimelineError {
    #[error("Tenant {0}")]
    Tenant(#[from] GetTenantError),

    #[error("Timeline {0}")]
    Timeline(#[from] crate::tenant::DeleteTimelineError),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TenantStateError {
    #[error("Tenant {0} is stopping")]
    IsStopping(TenantShardId),
    #[error(transparent)]
    SlotError(#[from] TenantSlotError),
    #[error(transparent)]
    SlotUpsertError(#[from] TenantSlotUpsertError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub(crate) async fn detach_tenant(
    conf: &'static PageServerConf,
    tenant_shard_id: TenantShardId,
    detach_ignored: bool,
    deletion_queue_client: &DeletionQueueClient,
) -> Result<(), TenantStateError> {
    let tmp_path = detach_tenant0(
        conf,
        &TENANTS,
        tenant_shard_id,
        detach_ignored,
        deletion_queue_client,
    )
    .await?;
    // Although we are cleaning up the tenant, this task is not meant to be bound by the lifetime of the tenant in memory.
    // After a tenant is detached, there are no more task_mgr tasks for that tenant_id.
    let task_tenant_id = None;
    task_mgr::spawn(
        task_mgr::BACKGROUND_RUNTIME.handle(),
        TaskKind::MgmtRequest,
        task_tenant_id,
        None,
        "tenant_files_delete",
        false,
        async move {
            fs::remove_dir_all(tmp_path.as_path())
                .await
                .with_context(|| format!("tenant directory {:?} deletion", tmp_path))
        },
    );
    Ok(())
}

async fn detach_tenant0(
    conf: &'static PageServerConf,
    tenants: &std::sync::RwLock<TenantsMap>,
    tenant_shard_id: TenantShardId,
    detach_ignored: bool,
    deletion_queue_client: &DeletionQueueClient,
) -> Result<Utf8PathBuf, TenantStateError> {
    let tenant_dir_rename_operation = |tenant_id_to_clean: TenantShardId| async move {
        let local_tenant_directory = conf.tenant_path(&tenant_id_to_clean);
        safe_rename_tenant_dir(&local_tenant_directory)
            .await
            .with_context(|| format!("local tenant directory {local_tenant_directory:?} rename"))
    };

    let removal_result = remove_tenant_from_memory(
        tenants,
        tenant_shard_id,
        tenant_dir_rename_operation(tenant_shard_id),
    )
    .await;

    // Flush pending deletions, so that they have a good chance of passing validation
    // before this tenant is potentially re-attached elsewhere.
    deletion_queue_client.flush_advisory();

    // Ignored tenants are not present in memory and will bail the removal from memory operation.
    // Before returning the error, check for ignored tenant removal case — we only need to clean its local files then.
    if detach_ignored
        && matches!(
            removal_result,
            Err(TenantStateError::SlotError(TenantSlotError::NotFound(_)))
        )
    {
        let tenant_ignore_mark = conf.tenant_ignore_mark_file_path(&tenant_shard_id);
        if tenant_ignore_mark.exists() {
            info!("Detaching an ignored tenant");
            let tmp_path = tenant_dir_rename_operation(tenant_shard_id)
                .await
                .with_context(|| {
                    format!("Ignored tenant {tenant_shard_id} local directory rename")
                })?;
            return Ok(tmp_path);
        }
    }

    removal_result
}

pub(crate) async fn load_tenant(
    conf: &'static PageServerConf,
    tenant_id: TenantId,
    generation: Generation,
    broker_client: storage_broker::BrokerClientChannel,
    remote_storage: Option<GenericRemoteStorage>,
    deletion_queue_client: DeletionQueueClient,
    ctx: &RequestContext,
) -> Result<(), TenantMapInsertError> {
    // This is a legacy API (replaced by `/location_conf`).  It does not support sharding
    let tenant_shard_id = TenantShardId::unsharded(tenant_id);

    let slot_guard =
        tenant_map_acquire_slot(&tenant_shard_id, TenantSlotAcquireMode::MustNotExist)?;
    let tenant_path = conf.tenant_path(&tenant_shard_id);

    let tenant_ignore_mark = conf.tenant_ignore_mark_file_path(&tenant_shard_id);
    if tenant_ignore_mark.exists() {
        std::fs::remove_file(&tenant_ignore_mark).with_context(|| {
            format!(
                "Failed to remove tenant ignore mark {tenant_ignore_mark:?} during tenant loading"
            )
        })?;
    }

    let resources = TenantSharedResources {
        broker_client,
        remote_storage,
        deletion_queue_client,
    };

    let mut location_conf =
        Tenant::load_tenant_config(conf, &tenant_shard_id).map_err(TenantMapInsertError::Other)?;
    location_conf.attach_in_generation(generation);

    Tenant::persist_tenant_config(conf, &tenant_shard_id, &location_conf).await?;

    let shard_identity = location_conf.shard;
    let new_tenant = tenant_spawn(
        conf,
        tenant_shard_id,
        &tenant_path,
        resources,
        AttachedTenantConf::try_from(location_conf)?,
        shard_identity,
        None,
        &TENANTS,
        SpawnMode::Normal,
        ctx,
    )
    .with_context(|| format!("Failed to schedule tenant processing in path {tenant_path:?}"))?;

    slot_guard.upsert(TenantSlot::Attached(new_tenant))?;
    Ok(())
}

pub(crate) async fn ignore_tenant(
    conf: &'static PageServerConf,
    tenant_id: TenantId,
) -> Result<(), TenantStateError> {
    ignore_tenant0(conf, &TENANTS, tenant_id).await
}

#[instrument(skip_all, fields(shard_id))]
async fn ignore_tenant0(
    conf: &'static PageServerConf,
    tenants: &std::sync::RwLock<TenantsMap>,
    tenant_id: TenantId,
) -> Result<(), TenantStateError> {
    // This is a legacy API (replaced by `/location_conf`).  It does not support sharding
    let tenant_shard_id = TenantShardId::unsharded(tenant_id);
    tracing::Span::current().record(
        "shard_id",
        tracing::field::display(tenant_shard_id.shard_slug()),
    );

    remove_tenant_from_memory(tenants, tenant_shard_id, async {
        let ignore_mark_file = conf.tenant_ignore_mark_file_path(&tenant_shard_id);
        fs::File::create(&ignore_mark_file)
            .await
            .context("Failed to create ignore mark file")
            .and_then(|_| {
                crashsafe::fsync_file_and_parent(&ignore_mark_file)
                    .context("Failed to fsync ignore mark file")
            })
            .with_context(|| format!("Failed to crate ignore mark for tenant {tenant_shard_id}"))?;
        Ok(())
    })
    .await
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TenantMapListError {
    #[error("tenant map is still initiailizing")]
    Initializing,
}

///
/// Get list of tenants, for the mgmt API
///
pub(crate) async fn list_tenants(
) -> Result<Vec<(TenantShardId, TenantState, Generation)>, TenantMapListError> {
    let tenants = TENANTS.read().unwrap();
    let m = match &*tenants {
        TenantsMap::Initializing => return Err(TenantMapListError::Initializing),
        TenantsMap::Open(m) | TenantsMap::ShuttingDown(m) => m,
    };
    Ok(m.iter()
        .filter_map(|(id, tenant)| match tenant {
            TenantSlot::Attached(tenant) => {
                Some((*id, tenant.current_state(), tenant.generation()))
            }
            TenantSlot::Secondary(_) => None,
            TenantSlot::InProgress(_) => None,
        })
        .collect())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TenantMapInsertError {
    #[error(transparent)]
    SlotError(#[from] TenantSlotError),
    #[error(transparent)]
    SlotUpsertError(#[from] TenantSlotUpsertError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Superset of TenantMapError: issues that can occur when acquiring a slot
/// for a particular tenant ID.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TenantSlotError {
    /// When acquiring a slot with the expectation that the tenant already exists.
    #[error("Tenant {0} not found")]
    NotFound(TenantShardId),

    /// When acquiring a slot with the expectation that the tenant does not already exist.
    #[error("tenant {0} already exists, state: {1:?}")]
    AlreadyExists(TenantShardId, TenantState),

    // Tried to read a slot that is currently being mutated by another administrative
    // operation.
    #[error("tenant has a state change in progress, try again later")]
    InProgress,

    #[error(transparent)]
    MapState(#[from] TenantMapError),
}

/// Superset of TenantMapError: issues that can occur when using a SlotGuard
/// to insert a new value.
#[derive(thiserror::Error)]
pub(crate) enum TenantSlotUpsertError {
    /// An error where the slot is in an unexpected state, indicating a code bug
    #[error("Internal error updating Tenant")]
    InternalError(Cow<'static, str>),

    #[error(transparent)]
    MapState(TenantMapError),

    // If we encounter TenantManager shutdown during upsert, we must carry the Completion
    // from the SlotGuard, so that the caller can hold it while they clean up: otherwise
    // TenantManager shutdown might race ahead before we're done cleaning up any Tenant that
    // was protected by the SlotGuard.
    #[error("Shutting down")]
    ShuttingDown((TenantSlot, utils::completion::Completion)),
}

impl std::fmt::Debug for TenantSlotUpsertError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InternalError(reason) => write!(f, "Internal Error {reason}"),
            Self::MapState(map_error) => write!(f, "Tenant map state: {map_error:?}"),
            Self::ShuttingDown(_completion) => write!(f, "Tenant map shutting down"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum TenantSlotDropError {
    /// It is only legal to drop a TenantSlot if its contents are fully shut down
    #[error("Tenant was not shut down")]
    NotShutdown,
}

/// Errors that can happen any time we are walking the tenant map to try and acquire
/// the TenantSlot for a particular tenant.
#[derive(Debug, thiserror::Error)]
pub enum TenantMapError {
    // Tried to read while initializing
    #[error("tenant map is still initializing")]
    StillInitializing,

    // Tried to read while shutting down
    #[error("tenant map is shutting down")]
    ShuttingDown,
}

/// Guards a particular tenant_id's content in the TenantsMap.  While this
/// structure exists, the TenantsMap will contain a [`TenantSlot::InProgress`]
/// for this tenant, which acts as a marker for any operations targeting
/// this tenant to retry later, or wait for the InProgress state to end.
///
/// This structure enforces the important invariant that we do not have overlapping
/// tasks that will try use local storage for a the same tenant ID: we enforce that
/// the previous contents of a slot have been shut down before the slot can be
/// left empty or used for something else
///
/// Holders of a SlotGuard should explicitly dispose of it, using either `upsert`
/// to provide a new value, or `revert` to put the slot back into its initial
/// state.  If the SlotGuard is dropped without calling either of these, then
/// we will leave the slot empty if our `old_value` is already shut down, else
/// we will replace the slot with `old_value` (equivalent to doing a revert).
///
/// The `old_value` may be dropped before the SlotGuard is dropped, by calling
/// `drop_old_value`.  It is an error to call this without shutting down
/// the conents of `old_value`.
pub struct SlotGuard {
    tenant_shard_id: TenantShardId,
    old_value: Option<TenantSlot>,
    upserted: bool,

    /// [`TenantSlot::InProgress`] carries the corresponding Barrier: it will
    /// release any waiters as soon as this SlotGuard is dropped.
    completion: utils::completion::Completion,
}

impl SlotGuard {
    fn new(
        tenant_shard_id: TenantShardId,
        old_value: Option<TenantSlot>,
        completion: utils::completion::Completion,
    ) -> Self {
        Self {
            tenant_shard_id,
            old_value,
            upserted: false,
            completion,
        }
    }

    /// Get any value that was present in the slot before we acquired ownership
    /// of it: in state transitions, this will be the old state.
    fn get_old_value(&self) -> &Option<TenantSlot> {
        &self.old_value
    }

    /// Emplace a new value in the slot.  This consumes the guard, and after
    /// returning, the slot is no longer protected from concurrent changes.
    fn upsert(mut self, new_value: TenantSlot) -> Result<(), TenantSlotUpsertError> {
        if !self.old_value_is_shutdown() {
            // This is a bug: callers should never try to drop an old value without
            // shutting it down
            return Err(TenantSlotUpsertError::InternalError(
                "Old TenantSlot value not shut down".into(),
            ));
        }

        let replaced = {
            let mut locked = TENANTS.write().unwrap();

            if let TenantSlot::InProgress(_) = new_value {
                // It is never expected to try and upsert InProgress via this path: it should
                // only be written via the tenant_map_acquire_slot path.  If we hit this it's a bug.
                return Err(TenantSlotUpsertError::InternalError(
                    "Attempt to upsert an InProgress state".into(),
                ));
            }

            let m = match &mut *locked {
                TenantsMap::Initializing => {
                    return Err(TenantSlotUpsertError::MapState(
                        TenantMapError::StillInitializing,
                    ))
                }
                TenantsMap::ShuttingDown(_) => {
                    return Err(TenantSlotUpsertError::ShuttingDown((
                        new_value,
                        self.completion.clone(),
                    )));
                }
                TenantsMap::Open(m) => m,
            };

            let replaced = m.insert(self.tenant_shard_id, new_value);
            self.upserted = true;

            METRICS.tenant_slots.set(m.len() as u64);

            replaced
        };

        // Sanity check: on an upsert we should always be replacing an InProgress marker
        match replaced {
            Some(TenantSlot::InProgress(_)) => {
                // Expected case: we find our InProgress in the map: nothing should have
                // replaced it because the code that acquires slots will not grant another
                // one for the same TenantId.
                Ok(())
            }
            None => {
                METRICS.unexpected_errors.inc();
                error!(
                    tenant_shard_id = %self.tenant_shard_id,
                    "Missing InProgress marker during tenant upsert, this is a bug."
                );
                Err(TenantSlotUpsertError::InternalError(
                    "Missing InProgress marker during tenant upsert".into(),
                ))
            }
            Some(slot) => {
                METRICS.unexpected_errors.inc();
                error!(tenant_shard_id=%self.tenant_shard_id, "Unexpected contents of TenantSlot during upsert, this is a bug.  Contents: {:?}", slot);
                Err(TenantSlotUpsertError::InternalError(
                    "Unexpected contents of TenantSlot".into(),
                ))
            }
        }
    }

    /// Replace the InProgress slot with whatever was in the guard when we started
    fn revert(mut self) {
        if let Some(value) = self.old_value.take() {
            match self.upsert(value) {
                Err(TenantSlotUpsertError::InternalError(_)) => {
                    // We already logged the error, nothing else we can do.
                }
                Err(
                    TenantSlotUpsertError::MapState(_) | TenantSlotUpsertError::ShuttingDown(_),
                ) => {
                    // If the map is shutting down, we need not replace anything
                }
                Ok(()) => {}
            }
        }
    }

    /// We may never drop our old value until it is cleanly shut down: otherwise we might leave
    /// rogue background tasks that would write to the local tenant directory that this guard
    /// is responsible for protecting
    fn old_value_is_shutdown(&self) -> bool {
        match self.old_value.as_ref() {
            Some(TenantSlot::Attached(tenant)) => tenant.gate.close_complete(),
            Some(TenantSlot::Secondary(secondary_tenant)) => secondary_tenant.gate.close_complete(),
            Some(TenantSlot::InProgress(_)) => {
                // A SlotGuard cannot be constructed for a slot that was already InProgress
                unreachable!()
            }
            None => true,
        }
    }

    /// The guard holder is done with the old value of the slot: they are obliged to already
    /// shut it down before we reach this point.
    fn drop_old_value(&mut self) -> Result<(), TenantSlotDropError> {
        if !self.old_value_is_shutdown() {
            Err(TenantSlotDropError::NotShutdown)
        } else {
            self.old_value.take();
            Ok(())
        }
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if self.upserted {
            return;
        }
        // Our old value is already shutdown, or it never existed: it is safe
        // for us to fully release the TenantSlot back into an empty state

        let mut locked = TENANTS.write().unwrap();

        let m = match &mut *locked {
            TenantsMap::Initializing => {
                // There is no map, this should never happen.
                return;
            }
            TenantsMap::ShuttingDown(_) => {
                // When we transition to shutdown, InProgress elements are removed
                // from the map, so we do not need to clean up our Inprogress marker.
                // See [`shutdown_all_tenants0`]
                return;
            }
            TenantsMap::Open(m) => m,
        };

        use std::collections::btree_map::Entry;
        match m.entry(self.tenant_shard_id) {
            Entry::Occupied(mut entry) => {
                if !matches!(entry.get(), TenantSlot::InProgress(_)) {
                    METRICS.unexpected_errors.inc();
                    error!(tenant_shard_id=%self.tenant_shard_id, "Unexpected contents of TenantSlot during drop, this is a bug.  Contents: {:?}", entry.get());
                }

                if self.old_value_is_shutdown() {
                    entry.remove();
                } else {
                    entry.insert(self.old_value.take().unwrap());
                }
            }
            Entry::Vacant(_) => {
                METRICS.unexpected_errors.inc();
                error!(
                    tenant_shard_id = %self.tenant_shard_id,
                    "Missing InProgress marker during SlotGuard drop, this is a bug."
                );
            }
        }

        METRICS.tenant_slots.set(m.len() as u64);
    }
}

enum TenantSlotPeekMode {
    /// In Read mode, peek will be permitted to see the slots even if the pageserver is shutting down
    Read,
    /// In Write mode, trying to peek at a slot while the pageserver is shutting down is an error
    Write,
}

fn tenant_map_peek_slot<'a>(
    tenants: &'a std::sync::RwLockReadGuard<'a, TenantsMap>,
    tenant_shard_id: &TenantShardId,
    mode: TenantSlotPeekMode,
) -> Result<Option<&'a TenantSlot>, TenantMapError> {
    match tenants.deref() {
        TenantsMap::Initializing => Err(TenantMapError::StillInitializing),
        TenantsMap::ShuttingDown(m) => match mode {
            TenantSlotPeekMode::Read => Ok(Some(
                // When reading in ShuttingDown state, we must translate None results
                // into a ShuttingDown error, because absence of a tenant shard ID in the map
                // isn't a reliable indicator of the tenant being gone: it might have been
                // InProgress when shutdown started, and cleaned up from that state such
                // that it's now no longer in the map.  Callers will have to wait until
                // we next start up to get a proper answer.  This avoids incorrect 404 API responses.
                m.get(tenant_shard_id).ok_or(TenantMapError::ShuttingDown)?,
            )),
            TenantSlotPeekMode::Write => Err(TenantMapError::ShuttingDown),
        },
        TenantsMap::Open(m) => Ok(m.get(tenant_shard_id)),
    }
}

enum TenantSlotAcquireMode {
    /// Acquire the slot irrespective of current state, or whether it already exists
    Any,
    /// Return an error if trying to acquire a slot and it doesn't already exist
    MustExist,
    /// Return an error if trying to acquire a slot and it already exists
    MustNotExist,
}

fn tenant_map_acquire_slot(
    tenant_shard_id: &TenantShardId,
    mode: TenantSlotAcquireMode,
) -> Result<SlotGuard, TenantSlotError> {
    tenant_map_acquire_slot_impl(tenant_shard_id, &TENANTS, mode)
}

fn tenant_map_acquire_slot_impl(
    tenant_shard_id: &TenantShardId,
    tenants: &std::sync::RwLock<TenantsMap>,
    mode: TenantSlotAcquireMode,
) -> Result<SlotGuard, TenantSlotError> {
    use TenantSlotAcquireMode::*;
    METRICS.tenant_slot_writes.inc();

    let mut locked = tenants.write().unwrap();
    let span = tracing::info_span!("acquire_slot", tenant_id=%tenant_shard_id.tenant_id, shard_id = %tenant_shard_id.shard_slug());
    let _guard = span.enter();

    let m = match &mut *locked {
        TenantsMap::Initializing => return Err(TenantMapError::StillInitializing.into()),
        TenantsMap::ShuttingDown(_) => return Err(TenantMapError::ShuttingDown.into()),
        TenantsMap::Open(m) => m,
    };

    use std::collections::btree_map::Entry;

    let entry = m.entry(*tenant_shard_id);

    match entry {
        Entry::Vacant(v) => match mode {
            MustExist => {
                tracing::debug!("Vacant && MustExist: return NotFound");
                Err(TenantSlotError::NotFound(*tenant_shard_id))
            }
            _ => {
                let (completion, barrier) = utils::completion::channel();
                v.insert(TenantSlot::InProgress(barrier));
                tracing::debug!("Vacant, inserted InProgress");
                Ok(SlotGuard::new(*tenant_shard_id, None, completion))
            }
        },
        Entry::Occupied(mut o) => {
            // Apply mode-driven checks
            match (o.get(), mode) {
                (TenantSlot::InProgress(_), _) => {
                    tracing::debug!("Occupied, failing for InProgress");
                    Err(TenantSlotError::InProgress)
                }
                (slot, MustNotExist) => match slot {
                    TenantSlot::Attached(tenant) => {
                        tracing::debug!("Attached && MustNotExist, return AlreadyExists");
                        Err(TenantSlotError::AlreadyExists(
                            *tenant_shard_id,
                            tenant.current_state(),
                        ))
                    }
                    _ => {
                        // FIXME: the AlreadyExists error assumes that we have a Tenant
                        // to get the state from
                        tracing::debug!("Occupied & MustNotExist, return AlreadyExists");
                        Err(TenantSlotError::AlreadyExists(
                            *tenant_shard_id,
                            TenantState::Broken {
                                reason: "Present but not attached".to_string(),
                                backtrace: "".to_string(),
                            },
                        ))
                    }
                },
                _ => {
                    // Happy case: the slot was not in any state that violated our mode
                    let (completion, barrier) = utils::completion::channel();
                    let old_value = o.insert(TenantSlot::InProgress(barrier));
                    tracing::debug!("Occupied, replaced with InProgress");
                    Ok(SlotGuard::new(
                        *tenant_shard_id,
                        Some(old_value),
                        completion,
                    ))
                }
            }
        }
    }
}

/// Stops and removes the tenant from memory, if it's not [`TenantState::Stopping`] already, bails otherwise.
/// Allows to remove other tenant resources manually, via `tenant_cleanup`.
/// If the cleanup fails, tenant will stay in memory in [`TenantState::Broken`] state, and another removal
/// operation would be needed to remove it.
async fn remove_tenant_from_memory<V, F>(
    tenants: &std::sync::RwLock<TenantsMap>,
    tenant_shard_id: TenantShardId,
    tenant_cleanup: F,
) -> Result<V, TenantStateError>
where
    F: std::future::Future<Output = anyhow::Result<V>>,
{
    let mut slot_guard =
        tenant_map_acquire_slot_impl(&tenant_shard_id, tenants, TenantSlotAcquireMode::MustExist)?;

    // allow pageserver shutdown to await for our completion
    let (_guard, progress) = completion::channel();

    // The SlotGuard allows us to manipulate the Tenant object without fear of some
    // concurrent API request doing something else for the same tenant ID.
    let attached_tenant = match slot_guard.get_old_value() {
        Some(TenantSlot::Attached(tenant)) => {
            // whenever we remove a tenant from memory, we don't want to flush and wait for upload
            let freeze_and_flush = false;

            // shutdown is sure to transition tenant to stopping, and wait for all tasks to complete, so
            // that we can continue safely to cleanup.
            match tenant.shutdown(progress, freeze_and_flush).await {
                Ok(()) => {}
                Err(_other) => {
                    // if pageserver shutdown or other detach/ignore is already ongoing, we don't want to
                    // wait for it but return an error right away because these are distinct requests.
                    slot_guard.revert();
                    return Err(TenantStateError::IsStopping(tenant_shard_id));
                }
            }
            Some(tenant)
        }
        Some(TenantSlot::Secondary(secondary_state)) => {
            tracing::info!("Shutting down in secondary mode");
            secondary_state.shutdown().await;
            None
        }
        Some(TenantSlot::InProgress(_)) => {
            // Acquiring a slot guarantees its old value was not InProgress
            unreachable!();
        }
        None => None,
    };

    match tenant_cleanup
        .await
        .with_context(|| format!("Failed to run cleanup for tenant {tenant_shard_id}"))
    {
        Ok(hook_value) => {
            // Success: drop the old TenantSlot::Attached.
            slot_guard
                .drop_old_value()
                .expect("We just called shutdown");

            Ok(hook_value)
        }
        Err(e) => {
            // If we had a Tenant, set it to Broken and put it back in the TenantsMap
            if let Some(attached_tenant) = attached_tenant {
                attached_tenant.set_broken(e.to_string()).await;
            }
            // Leave the broken tenant in the map
            slot_guard.revert();

            Err(TenantStateError::Other(e))
        }
    }
}

use {
    crate::repository::GcResult, pageserver_api::models::TimelineGcRequest,
    utils::http::error::ApiError,
};

pub(crate) async fn immediate_gc(
    tenant_shard_id: TenantShardId,
    timeline_id: TimelineId,
    gc_req: TimelineGcRequest,
    cancel: CancellationToken,
    ctx: &RequestContext,
) -> Result<tokio::sync::oneshot::Receiver<Result<GcResult, anyhow::Error>>, ApiError> {
    let guard = TENANTS.read().unwrap();

    let tenant = guard
        .get(&tenant_shard_id)
        .map(Arc::clone)
        .with_context(|| format!("tenant {tenant_shard_id}"))
        .map_err(|e| ApiError::NotFound(e.into()))?;

    let gc_horizon = gc_req.gc_horizon.unwrap_or_else(|| tenant.get_gc_horizon());
    // Use tenant's pitr setting
    let pitr = tenant.get_pitr_interval();

    // Run in task_mgr to avoid race with tenant_detach operation
    let ctx = ctx.detached_child(TaskKind::GarbageCollector, DownloadBehavior::Download);
    let (task_done, wait_task_done) = tokio::sync::oneshot::channel();
    // TODO: spawning is redundant now, need to hold the gate
    task_mgr::spawn(
        &tokio::runtime::Handle::current(),
        TaskKind::GarbageCollector,
        Some(tenant_shard_id),
        Some(timeline_id),
        &format!("timeline_gc_handler garbage collection run for tenant {tenant_shard_id} timeline {timeline_id}"),
        false,
        async move {
            fail::fail_point!("immediate_gc_task_pre");

            #[allow(unused_mut)]
            let mut result = tenant
                .gc_iteration(Some(timeline_id), gc_horizon, pitr, &cancel, &ctx)
                .instrument(info_span!("manual_gc", tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(), %timeline_id))
                .await;
                // FIXME: `gc_iteration` can return an error for multiple reasons; we should handle it
                // better once the types support it.

            #[cfg(feature = "testing")]
            {
                if let Ok(result) = result.as_mut() {
                    // why not futures unordered? it seems it needs very much the same task structure
                    // but would only run on single task.
                    let mut js = tokio::task::JoinSet::new();
                    for layer in std::mem::take(&mut result.doomed_layers) {
                        js.spawn(layer.wait_drop());
                    }
                    tracing::info!(total = js.len(), "starting to wait for the gc'd layers to be dropped");
                    while let Some(res) = js.join_next().await {
                        res.expect("wait_drop should not panic");
                    }
                }

                let timeline = tenant.get_timeline(timeline_id, false).ok();
                let rtc = timeline.as_ref().and_then(|x| x.remote_client.as_ref());

                if let Some(rtc) = rtc {
                    // layer drops schedule actions on remote timeline client to actually do the
                    // deletions; don't care just exit fast about the shutdown error
                    drop(rtc.wait_completion().await);
                }
            }

            match task_done.send(result) {
                Ok(_) => (),
                Err(result) => error!("failed to send gc result: {result:?}"),
            }
            Ok(())
        }
    );

    // drop the guard until after we've spawned the task so that timeline shutdown will wait for the task
    drop(guard);

    Ok(wait_task_done)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use tracing::Instrument;

    use crate::tenant::mgr::TenantSlot;

    use super::{super::harness::TenantHarness, TenantsMap};

    #[tokio::test(start_paused = true)]
    async fn shutdown_awaits_in_progress_tenant() {
        // Test that if an InProgress tenant is in the map during shutdown, the shutdown will gracefully
        // wait for it to complete before proceeding.

        let h = TenantHarness::create("shutdown_awaits_in_progress_tenant").unwrap();
        let (t, _ctx) = h.load().await;

        // harness loads it to active, which is forced and nothing is running on the tenant

        let id = t.tenant_shard_id();

        // tenant harness configures the logging and we cannot escape it
        let span = h.span();
        let _e = span.enter();

        let tenants = BTreeMap::from([(id, TenantSlot::Attached(t.clone()))]);
        let tenants = Arc::new(std::sync::RwLock::new(TenantsMap::Open(tenants)));

        // Invoke remove_tenant_from_memory with a cleanup hook that blocks until we manually
        // permit it to proceed: that will stick the tenant in InProgress

        let (until_cleanup_completed, can_complete_cleanup) = utils::completion::channel();
        let (until_cleanup_started, cleanup_started) = utils::completion::channel();
        let mut remove_tenant_from_memory_task = {
            let jh = tokio::spawn({
                let tenants = tenants.clone();
                async move {
                    let cleanup = async move {
                        drop(until_cleanup_started);
                        can_complete_cleanup.wait().await;
                        anyhow::Ok(())
                    };
                    super::remove_tenant_from_memory(&tenants, id, cleanup).await
                }
                .instrument(h.span())
            });

            // now the long cleanup should be in place, with the stopping state
            cleanup_started.wait().await;
            jh
        };

        let mut shutdown_task = {
            let (until_shutdown_started, shutdown_started) = utils::completion::channel();

            let shutdown_task = tokio::spawn(async move {
                drop(until_shutdown_started);
                super::shutdown_all_tenants0(&tenants).await;
            });

            shutdown_started.wait().await;
            shutdown_task
        };

        let long_time = std::time::Duration::from_secs(15);
        tokio::select! {
            _ = &mut shutdown_task => unreachable!("shutdown should block on remove_tenant_from_memory completing"),
            _ = &mut remove_tenant_from_memory_task => unreachable!("remove_tenant_from_memory_task should not complete until explicitly unblocked"),
            _ = tokio::time::sleep(long_time) => {},
        }

        drop(until_cleanup_completed);

        // Now that we allow it to proceed, shutdown should complete immediately
        remove_tenant_from_memory_task.await.unwrap().unwrap();
        shutdown_task.await.unwrap();
    }
}
