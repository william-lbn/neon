use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use pageserver_api::keyspace::KeySpace;
use pageserver_api::models::{
    HistoricLayerInfo, LayerAccessKind, LayerResidenceEventReason, LayerResidenceStatus,
};
use pageserver_api::shard::ShardIndex;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::SystemTime;
use tracing::Instrument;
use utils::lsn::Lsn;
use utils::sync::heavier_once_cell;

use crate::config::PageServerConf;
use crate::context::RequestContext;
use crate::repository::Key;
use crate::span::debug_assert_current_span_has_tenant_and_timeline_id;
use crate::tenant::timeline::GetVectoredError;
use crate::tenant::{remote_timeline_client::LayerFileMetadata, Timeline};

use super::delta_layer::{self, DeltaEntry};
use super::image_layer;
use super::{
    AsLayerDesc, LayerAccessStats, LayerAccessStatsReset, LayerFileName, PersistentLayerDesc,
    ValueReconstructResult, ValueReconstructState, ValuesReconstructState,
};

use utils::generation::Generation;

/// A Layer contains all data in a "rectangle" consisting of a range of keys and
/// range of LSNs.
///
/// There are two kinds of layers, in-memory and on-disk layers. In-memory
/// layers are used to ingest incoming WAL, and provide fast access to the
/// recent page versions. On-disk layers are stored as files on disk, and are
/// immutable. This type represents the on-disk kind while in-memory kind are represented by
/// [`InMemoryLayer`].
///
/// Furthermore, there are two kinds of on-disk layers: delta and image layers.
/// A delta layer contains all modifications within a range of LSNs and keys.
/// An image layer is a snapshot of all the data in a key-range, at a single
/// LSN.
///
/// This type models the on-disk layers, which can be evicted and on-demand downloaded.
///
/// [`InMemoryLayer`]: super::inmemory_layer::InMemoryLayer
#[derive(Clone)]
pub(crate) struct Layer(Arc<LayerInner>);

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if matches!(self.0.generation, Generation::Broken) {
            write!(f, "{}-broken", self.layer_desc().short_id())
        } else {
            write!(
                f,
                "{}{}",
                self.layer_desc().short_id(),
                self.0.generation.get_suffix()
            )
        }
    }
}

impl std::fmt::Debug for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl AsLayerDesc for Layer {
    fn layer_desc(&self) -> &PersistentLayerDesc {
        self.0.layer_desc()
    }
}

impl Layer {
    /// Creates a layer value for a file we know to not be resident.
    pub(crate) fn for_evicted(
        conf: &'static PageServerConf,
        timeline: &Arc<Timeline>,
        file_name: LayerFileName,
        metadata: LayerFileMetadata,
    ) -> Self {
        let desc = PersistentLayerDesc::from_filename(
            timeline.tenant_shard_id,
            timeline.timeline_id,
            file_name,
            metadata.file_size(),
        );

        let access_stats = LayerAccessStats::for_loading_layer(LayerResidenceStatus::Evicted);

        let owner = Layer(Arc::new(LayerInner::new(
            conf,
            timeline,
            access_stats,
            desc,
            None,
            metadata.generation,
            metadata.shard,
        )));

        debug_assert!(owner.0.needs_download_blocking().unwrap().is_some());

        owner
    }

    /// Creates a Layer value for a file we know to be resident in timeline directory.
    pub(crate) fn for_resident(
        conf: &'static PageServerConf,
        timeline: &Arc<Timeline>,
        file_name: LayerFileName,
        metadata: LayerFileMetadata,
    ) -> ResidentLayer {
        let desc = PersistentLayerDesc::from_filename(
            timeline.tenant_shard_id,
            timeline.timeline_id,
            file_name,
            metadata.file_size(),
        );

        let access_stats = LayerAccessStats::for_loading_layer(LayerResidenceStatus::Resident);

        let mut resident = None;

        let owner = Layer(Arc::new_cyclic(|owner| {
            let inner = Arc::new(DownloadedLayer {
                owner: owner.clone(),
                kind: tokio::sync::OnceCell::default(),
                version: 0,
            });
            resident = Some(inner.clone());

            LayerInner::new(
                conf,
                timeline,
                access_stats,
                desc,
                Some(inner),
                metadata.generation,
                metadata.shard,
            )
        }));

        let downloaded = resident.expect("just initialized");

        debug_assert!(owner.0.needs_download_blocking().unwrap().is_none());

        timeline
            .metrics
            .resident_physical_size_add(metadata.file_size());

        ResidentLayer { downloaded, owner }
    }

    /// Creates a Layer value for freshly written out new layer file by renaming it from a
    /// temporary path.
    pub(crate) fn finish_creating(
        conf: &'static PageServerConf,
        timeline: &Arc<Timeline>,
        desc: PersistentLayerDesc,
        temp_path: &Utf8Path,
    ) -> anyhow::Result<ResidentLayer> {
        let mut resident = None;

        let owner = Layer(Arc::new_cyclic(|owner| {
            let inner = Arc::new(DownloadedLayer {
                owner: owner.clone(),
                kind: tokio::sync::OnceCell::default(),
                version: 0,
            });
            resident = Some(inner.clone());
            let access_stats = LayerAccessStats::empty_will_record_residence_event_later();
            access_stats.record_residence_event(
                LayerResidenceStatus::Resident,
                LayerResidenceEventReason::LayerCreate,
            );
            LayerInner::new(
                conf,
                timeline,
                access_stats,
                desc,
                Some(inner),
                timeline.generation,
                timeline.get_shard_index(),
            )
        }));

        let downloaded = resident.expect("just initialized");

        // if the rename works, the path is as expected
        std::fs::rename(temp_path, owner.local_path())
            .with_context(|| format!("rename temporary file as correct path for {owner}"))?;

        Ok(ResidentLayer { downloaded, owner })
    }

    /// Requests the layer to be evicted and waits for this to be done.
    ///
    /// If the file is not resident, an [`EvictionError::NotFound`] is returned.
    ///
    /// If for a bad luck or blocking of the executor, we miss the actual eviction and the layer is
    /// re-downloaded, [`EvictionError::Downloaded`] is returned.
    ///
    /// Technically cancellation safe, but cancelling might shift the viewpoint of what generation
    /// of download-evict cycle on retry.
    pub(crate) async fn evict_and_wait(&self) -> Result<(), EvictionError> {
        self.0.evict_and_wait().await
    }

    /// Delete the layer file when the `self` gets dropped, also try to schedule a remote index upload
    /// then.
    ///
    /// On drop, this will cause a call to [`crate::tenant::remote_timeline_client::RemoteTimelineClient::schedule_deletion_of_unlinked`].
    /// This means that the unlinking by [gc] or [compaction] must have happened strictly before
    /// the value this is called on gets dropped.
    ///
    /// This is ensured by both of those methods accepting references to Layer.
    ///
    /// [gc]: [`RemoteTimelineClient::schedule_gc_update`]
    /// [compaction]: [`RemoteTimelineClient::schedule_compaction_update`]
    pub(crate) fn delete_on_drop(&self) {
        self.0.delete_on_drop();
    }

    /// Return data needed to reconstruct given page at LSN.
    ///
    /// It is up to the caller to collect more data from the previous layer and
    /// perform WAL redo, if necessary.
    ///
    /// # Cancellation-Safety
    ///
    /// This method is cancellation-safe.
    pub(crate) async fn get_value_reconstruct_data(
        &self,
        key: Key,
        lsn_range: Range<Lsn>,
        reconstruct_data: &mut ValueReconstructState,
        ctx: &RequestContext,
    ) -> anyhow::Result<ValueReconstructResult> {
        use anyhow::ensure;

        let layer = self.0.get_or_maybe_download(true, Some(ctx)).await?;
        self.0
            .access_stats
            .record_access(LayerAccessKind::GetValueReconstructData, ctx);

        if self.layer_desc().is_delta {
            ensure!(lsn_range.start >= self.layer_desc().lsn_range.start);
            ensure!(self.layer_desc().key_range.contains(&key));
        } else {
            ensure!(self.layer_desc().key_range.contains(&key));
            ensure!(lsn_range.start >= self.layer_desc().image_layer_lsn());
            ensure!(lsn_range.end >= self.layer_desc().image_layer_lsn());
        }

        layer
            .get_value_reconstruct_data(key, lsn_range, reconstruct_data, &self.0, ctx)
            .instrument(tracing::debug_span!("get_value_reconstruct_data", layer=%self))
            .await
            .with_context(|| format!("get_value_reconstruct_data for layer {self}"))
    }

    pub(crate) async fn get_values_reconstruct_data(
        &self,
        keyspace: KeySpace,
        end_lsn: Lsn,
        reconstruct_data: &mut ValuesReconstructState,
        ctx: &RequestContext,
    ) -> Result<(), GetVectoredError> {
        let layer = self
            .0
            .get_or_maybe_download(true, Some(ctx))
            .await
            .map_err(|err| GetVectoredError::Other(anyhow::anyhow!(err)))?;

        self.0
            .access_stats
            .record_access(LayerAccessKind::GetValueReconstructData, ctx);

        layer
            .get_values_reconstruct_data(keyspace, end_lsn, reconstruct_data, &self.0, ctx)
            .instrument(tracing::debug_span!("get_values_reconstruct_data", layer=%self))
            .await
    }

    /// Download the layer if evicted.
    ///
    /// Will not error when the layer is already downloaded.
    pub(crate) async fn download(&self) -> anyhow::Result<()> {
        self.0.get_or_maybe_download(true, None).await?;
        Ok(())
    }

    /// Assuming the layer is already downloaded, returns a guard which will prohibit eviction
    /// while the guard exists.
    ///
    /// Returns None if the layer is currently evicted.
    pub(crate) async fn keep_resident(&self) -> anyhow::Result<Option<ResidentLayer>> {
        let downloaded = match self.0.get_or_maybe_download(false, None).await {
            Ok(d) => d,
            // technically there are a lot of possible errors, but in practice it should only be
            // DownloadRequired which is tripped up. could work to improve this situation
            // statically later.
            Err(DownloadError::DownloadRequired) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        Ok(Some(ResidentLayer {
            downloaded,
            owner: self.clone(),
        }))
    }

    /// Downloads if necessary and creates a guard, which will keep this layer from being evicted.
    pub(crate) async fn download_and_keep_resident(&self) -> anyhow::Result<ResidentLayer> {
        let downloaded = self.0.get_or_maybe_download(true, None).await?;

        Ok(ResidentLayer {
            downloaded,
            owner: self.clone(),
        })
    }

    pub(crate) fn info(&self, reset: LayerAccessStatsReset) -> HistoricLayerInfo {
        self.0.info(reset)
    }

    pub(crate) fn access_stats(&self) -> &LayerAccessStats {
        &self.0.access_stats
    }

    pub(crate) fn local_path(&self) -> &Utf8Path {
        &self.0.path
    }

    pub(crate) fn metadata(&self) -> LayerFileMetadata {
        self.0.metadata()
    }

    /// Traditional debug dumping facility
    #[allow(unused)]
    pub(crate) async fn dump(&self, verbose: bool, ctx: &RequestContext) -> anyhow::Result<()> {
        self.0.desc.dump();

        if verbose {
            // for now, unconditionally download everything, even if that might not be wanted.
            let l = self.0.get_or_maybe_download(true, Some(ctx)).await?;
            l.dump(&self.0, ctx).await?
        }

        Ok(())
    }

    /// Waits until this layer has been dropped (and if needed, local file deletion and remote
    /// deletion scheduling has completed).
    ///
    /// Does not start local deletion, use [`Self::delete_on_drop`] for that
    /// separatedly.
    #[cfg(feature = "testing")]
    pub(crate) fn wait_drop(&self) -> impl std::future::Future<Output = ()> + 'static {
        let mut rx = self.0.status.subscribe();

        async move {
            loop {
                if let Err(tokio::sync::broadcast::error::RecvError::Closed) = rx.recv().await {
                    break;
                }
            }
        }
    }
}

/// The download-ness ([`DownloadedLayer`]) can be either resident or wanted evicted.
///
/// However when we want something evicted, we cannot evict it right away as there might be current
/// reads happening on it. For example: it has been searched from [`LayerMap::search`] but not yet
/// read with [`Layer::get_value_reconstruct_data`].
///
/// [`LayerMap::search`]: crate::tenant::layer_map::LayerMap::search
#[derive(Debug)]
enum ResidentOrWantedEvicted {
    Resident(Arc<DownloadedLayer>),
    WantedEvicted(Weak<DownloadedLayer>, usize),
}

impl ResidentOrWantedEvicted {
    fn get_and_upgrade(&mut self) -> Option<(Arc<DownloadedLayer>, bool)> {
        match self {
            ResidentOrWantedEvicted::Resident(strong) => Some((strong.clone(), false)),
            ResidentOrWantedEvicted::WantedEvicted(weak, _) => match weak.upgrade() {
                Some(strong) => {
                    LAYER_IMPL_METRICS.inc_raced_wanted_evicted_accesses();

                    *self = ResidentOrWantedEvicted::Resident(strong.clone());

                    Some((strong, true))
                }
                None => None,
            },
        }
    }

    /// When eviction is first requested, drop down to holding a [`Weak`].
    ///
    /// Returns `Some` if this was the first time eviction was requested. Care should be taken to
    /// drop the possibly last strong reference outside of the mutex of
    /// heavier_once_cell::OnceCell.
    fn downgrade(&mut self) -> Option<Arc<DownloadedLayer>> {
        match self {
            ResidentOrWantedEvicted::Resident(strong) => {
                let weak = Arc::downgrade(strong);
                let mut temp = ResidentOrWantedEvicted::WantedEvicted(weak, strong.version);
                std::mem::swap(self, &mut temp);
                match temp {
                    ResidentOrWantedEvicted::Resident(strong) => Some(strong),
                    ResidentOrWantedEvicted::WantedEvicted(..) => unreachable!("just swapped"),
                }
            }
            ResidentOrWantedEvicted::WantedEvicted(..) => None,
        }
    }
}

struct LayerInner {
    /// Only needed to check ondemand_download_behavior_treat_error_as_warn and creation of
    /// [`Self::path`].
    conf: &'static PageServerConf,

    /// Full path to the file; unclear if this should exist anymore.
    path: Utf8PathBuf,

    desc: PersistentLayerDesc,

    /// Timeline access is needed for remote timeline client and metrics.
    timeline: Weak<Timeline>,

    /// Cached knowledge of [`Timeline::remote_client`] being `Some`.
    have_remote_client: bool,

    access_stats: LayerAccessStats,

    /// This custom OnceCell is backed by std mutex, but only held for short time periods.
    /// Initialization and deinitialization are done while holding a permit.
    inner: heavier_once_cell::OnceCell<ResidentOrWantedEvicted>,

    /// Do we want to delete locally and remotely this when `LayerInner` is dropped
    wanted_deleted: AtomicBool,

    /// Do we want to evict this layer as soon as possible? After being set to `true`, all accesses
    /// will try to downgrade [`ResidentOrWantedEvicted`], which will eventually trigger
    /// [`LayerInner::on_downloaded_layer_drop`].
    wanted_evicted: AtomicBool,

    /// Version is to make sure we will only evict a specific download of a file.
    ///
    /// Incremented for each download, stored in `DownloadedLayer::version` or
    /// `ResidentOrWantedEvicted::WantedEvicted`.
    version: AtomicUsize,

    /// Allow subscribing to when the layer actually gets evicted.
    status: tokio::sync::broadcast::Sender<Status>,

    /// Counter for exponential backoff with the download
    consecutive_failures: AtomicUsize,

    /// The generation of this Layer.
    ///
    /// For loaded layers (resident or evicted) this comes from [`LayerFileMetadata::generation`],
    /// for created layers from [`Timeline::generation`].
    generation: Generation,

    /// The shard of this Layer.
    ///
    /// For layers created in this process, this will always be the [`ShardIndex`] of the
    /// current `ShardIdentity`` (TODO: add link once it's introduced).
    ///
    /// For loaded layers, this may be some other value if the tenant has undergone
    /// a shard split since the layer was originally written.
    shard: ShardIndex,

    last_evicted_at: std::sync::Mutex<Option<std::time::Instant>>,
}

impl std::fmt::Display for LayerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.layer_desc().short_id())
    }
}

impl AsLayerDesc for LayerInner {
    fn layer_desc(&self) -> &PersistentLayerDesc {
        &self.desc
    }
}

#[derive(Debug, Clone, Copy)]
enum Status {
    Evicted,
    Downloaded,
}

impl Drop for LayerInner {
    fn drop(&mut self) {
        if !*self.wanted_deleted.get_mut() {
            // should we try to evict if the last wish was for eviction?
            // feels like there's some hazard of overcrowding near shutdown near by, but we don't
            // run drops during shutdown (yet)
            return;
        }

        let span = tracing::info_span!(parent: None, "layer_delete", tenant_id = %self.layer_desc().tenant_shard_id.tenant_id, shard_id=%self.layer_desc().tenant_shard_id.shard_slug(), timeline_id = %self.layer_desc().timeline_id);

        let path = std::mem::take(&mut self.path);
        let file_name = self.layer_desc().filename();
        let file_size = self.layer_desc().file_size;
        let timeline = self.timeline.clone();
        let meta = self.metadata();
        let status = self.status.clone();

        crate::task_mgr::BACKGROUND_RUNTIME.spawn_blocking(move || {
            let _g = span.entered();

            // carry this until we are finished for [`Layer::wait_drop`] support
            let _status = status;

            let removed = match std::fs::remove_file(path) {
                Ok(()) => true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // until we no longer do detaches by removing all local files before removing the
                    // tenant from the global map, we will always get these errors even if we knew what
                    // is the latest state.
                    //
                    // we currently do not track the latest state, so we'll also end up here on evicted
                    // layers.
                    false
                }
                Err(e) => {
                    tracing::error!("failed to remove wanted deleted layer: {e}");
                    LAYER_IMPL_METRICS.inc_delete_removes_failed();
                    false
                }
            };

            if let Some(timeline) = timeline.upgrade() {
                if removed {
                    timeline.metrics.resident_physical_size_sub(file_size);
                }
                if let Some(remote_client) = timeline.remote_client.as_ref() {
                    let res = remote_client.schedule_deletion_of_unlinked(vec![(file_name, meta)]);

                    if let Err(e) = res {
                        // test_timeline_deletion_with_files_stuck_in_upload_queue is good at
                        // demonstrating this deadlock (without spawn_blocking): stop will drop
                        // queued items, which will have ResidentLayer's, and those drops would try
                        // to re-entrantly lock the RemoteTimelineClient inner state.
                        if !timeline.is_active() {
                            tracing::info!("scheduling deletion on drop failed: {e:#}");
                        } else {
                            tracing::warn!("scheduling deletion on drop failed: {e:#}");
                        }
                        LAYER_IMPL_METRICS.inc_deletes_failed(DeleteFailed::DeleteSchedulingFailed);
                    } else {
                        LAYER_IMPL_METRICS.inc_completed_deletes();
                    }
                }
            } else {
                // no need to nag that timeline is gone: under normal situation on
                // task_mgr::remove_tenant_from_memory the timeline is gone before we get dropped.
                LAYER_IMPL_METRICS.inc_deletes_failed(DeleteFailed::TimelineGone);
            }
        });
    }
}

impl LayerInner {
    fn new(
        conf: &'static PageServerConf,
        timeline: &Arc<Timeline>,
        access_stats: LayerAccessStats,
        desc: PersistentLayerDesc,
        downloaded: Option<Arc<DownloadedLayer>>,
        generation: Generation,
        shard: ShardIndex,
    ) -> Self {
        let path = conf
            .timeline_path(&timeline.tenant_shard_id, &timeline.timeline_id)
            .join(desc.filename().to_string());

        let (inner, version) = if let Some(inner) = downloaded {
            let version = inner.version;
            let resident = ResidentOrWantedEvicted::Resident(inner);
            (heavier_once_cell::OnceCell::new(resident), version)
        } else {
            (heavier_once_cell::OnceCell::default(), 0)
        };

        LayerInner {
            conf,
            path,
            desc,
            timeline: Arc::downgrade(timeline),
            have_remote_client: timeline.remote_client.is_some(),
            access_stats,
            wanted_deleted: AtomicBool::new(false),
            wanted_evicted: AtomicBool::new(false),
            inner,
            version: AtomicUsize::new(version),
            status: tokio::sync::broadcast::channel(1).0,
            consecutive_failures: AtomicUsize::new(0),
            generation,
            shard,
            last_evicted_at: std::sync::Mutex::default(),
        }
    }

    fn delete_on_drop(&self) {
        let res =
            self.wanted_deleted
                .compare_exchange(false, true, Ordering::Release, Ordering::Relaxed);

        if res.is_ok() {
            LAYER_IMPL_METRICS.inc_started_deletes();
        }
    }

    /// Cancellation safe, however dropping the future and calling this method again might result
    /// in a new attempt to evict OR join the previously started attempt.
    pub(crate) async fn evict_and_wait(&self) -> Result<(), EvictionError> {
        use tokio::sync::broadcast::error::RecvError;

        assert!(self.have_remote_client);

        let mut rx = self.status.subscribe();

        let strong = {
            match self.inner.get() {
                Some(mut either) => {
                    self.wanted_evicted.store(true, Ordering::Relaxed);
                    either.downgrade()
                }
                None => return Err(EvictionError::NotFound),
            }
        };

        if strong.is_some() {
            // drop the DownloadedLayer outside of the holding the guard
            drop(strong);
            LAYER_IMPL_METRICS.inc_started_evictions();
        }

        match rx.recv().await {
            Ok(Status::Evicted) => Ok(()),
            Ok(Status::Downloaded) => Err(EvictionError::Downloaded),
            Err(RecvError::Closed) => {
                unreachable!("sender cannot be dropped while we are in &self method")
            }
            Err(RecvError::Lagged(_)) => {
                // this is quite unlikely, but we are blocking a lot in the async context, so
                // we might be missing this because we are stuck on a LIFO slot on a thread
                // which is busy blocking for a 1TB database create_image_layers.
                //
                // use however late (compared to the initial expressing of wanted) as the
                // "outcome" now
                LAYER_IMPL_METRICS.inc_broadcast_lagged();
                match self.inner.get() {
                    Some(_) => Err(EvictionError::Downloaded),
                    None => Ok(()),
                }
            }
        }
    }

    /// Cancellation safe.
    async fn get_or_maybe_download(
        self: &Arc<Self>,
        allow_download: bool,
        ctx: Option<&RequestContext>,
    ) -> Result<Arc<DownloadedLayer>, DownloadError> {
        let mut init_permit = None;

        loop {
            let download = move |permit| {
                async move {
                    // disable any scheduled but not yet running eviction deletions for this
                    let next_version = 1 + self.version.fetch_add(1, Ordering::Relaxed);

                    // count cancellations, which currently remain largely unexpected
                    let init_cancelled =
                        scopeguard::guard((), |_| LAYER_IMPL_METRICS.inc_init_cancelled());

                    // no need to make the evict_and_wait wait for the actual download to complete
                    drop(self.status.send(Status::Downloaded));

                    let timeline = self
                        .timeline
                        .upgrade()
                        .ok_or_else(|| DownloadError::TimelineShutdown)?;

                    // FIXME: grab a gate

                    let can_ever_evict = timeline.remote_client.as_ref().is_some();

                    // check if we really need to be downloaded; could have been already downloaded by a
                    // cancelled previous attempt.
                    let needs_download = self
                        .needs_download()
                        .await
                        .map_err(DownloadError::PreStatFailed)?;

                    let permit = if let Some(reason) = needs_download {
                        if let NeedsDownload::NotFile(ft) = reason {
                            return Err(DownloadError::NotFile(ft));
                        }

                        // only reset this after we've decided we really need to download. otherwise it'd
                        // be impossible to mark cancelled downloads for eviction, like one could imagine
                        // we would like to do for prefetching which was not needed.
                        self.wanted_evicted.store(false, Ordering::Release);

                        if !can_ever_evict {
                            return Err(DownloadError::NoRemoteStorage);
                        }

                        if let Some(ctx) = ctx {
                            self.check_expected_download(ctx)?;
                        }

                        if !allow_download {
                            // this does look weird, but for LayerInner the "downloading" means also changing
                            // internal once related state ...
                            return Err(DownloadError::DownloadRequired);
                        }

                        tracing::info!(%reason, "downloading on-demand");

                        self.spawn_download_and_wait(timeline, permit).await?
                    } else {
                        // the file is present locally, probably by a previous but cancelled call to
                        // get_or_maybe_download. alternatively we might be running without remote storage.
                        LAYER_IMPL_METRICS.inc_init_needed_no_download();

                        permit
                    };

                    let since_last_eviction =
                        self.last_evicted_at.lock().unwrap().map(|ts| ts.elapsed());
                    if let Some(since_last_eviction) = since_last_eviction {
                        // FIXME: this will not always be recorded correctly until #6028 (the no
                        // download needed branch above)
                        LAYER_IMPL_METRICS.record_redownloaded_after(since_last_eviction);
                    }

                    let res = Arc::new(DownloadedLayer {
                        owner: Arc::downgrade(self),
                        kind: tokio::sync::OnceCell::default(),
                        version: next_version,
                    });

                    self.access_stats.record_residence_event(
                        LayerResidenceStatus::Resident,
                        LayerResidenceEventReason::ResidenceChange,
                    );

                    let waiters = self.inner.initializer_count();
                    if waiters > 0 {
                        tracing::info!(
                            waiters,
                            "completing the on-demand download for other tasks"
                        );
                    }

                    scopeguard::ScopeGuard::into_inner(init_cancelled);

                    Ok((ResidentOrWantedEvicted::Resident(res), permit))
                }
                .instrument(tracing::info_span!("get_or_maybe_download", layer=%self))
            };

            if let Some(init_permit) = init_permit.take() {
                // use the already held initialization permit because it is impossible to hit the
                // below paths anymore essentially limiting the max loop iterations to 2.
                let (value, init_permit) = download(init_permit).await?;
                let mut guard = self.inner.set(value, init_permit);
                let (strong, _upgraded) = guard
                    .get_and_upgrade()
                    .expect("init creates strong reference, we held the init permit");
                return Ok(strong);
            }

            let (weak, permit) = {
                let mut locked = self.inner.get_or_init(download).await?;

                if let Some((strong, upgraded)) = locked.get_and_upgrade() {
                    if upgraded {
                        // when upgraded back, the Arc<DownloadedLayer> is still available, but
                        // previously a `evict_and_wait` was received.
                        self.wanted_evicted.store(false, Ordering::Relaxed);

                        // error out any `evict_and_wait`
                        drop(self.status.send(Status::Downloaded));
                        LAYER_IMPL_METRICS
                            .inc_eviction_cancelled(EvictionCancelled::UpgradedBackOnAccess);
                    }

                    return Ok(strong);
                } else {
                    // path to here: the evict_blocking is stuck on spawn_blocking queue.
                    //
                    // reset the contents, deactivating the eviction and causing a
                    // EvictionCancelled::LostToDownload or EvictionCancelled::VersionCheckFailed.
                    locked.take_and_deinit()
                }
            };

            // unlock first, then drop the weak, but because upgrade failed, we
            // know it cannot be a problem.

            assert!(
                matches!(weak, ResidentOrWantedEvicted::WantedEvicted(..)),
                "unexpected {weak:?}, ResidentOrWantedEvicted::get_and_upgrade has a bug"
            );

            init_permit = Some(permit);

            LAYER_IMPL_METRICS.inc_retried_get_or_maybe_download();
        }
    }

    /// Nag or fail per RequestContext policy
    fn check_expected_download(&self, ctx: &RequestContext) -> Result<(), DownloadError> {
        use crate::context::DownloadBehavior::*;
        let b = ctx.download_behavior();
        match b {
            Download => Ok(()),
            Warn | Error => {
                tracing::info!(
                    "unexpectedly on-demand downloading for task kind {:?}",
                    ctx.task_kind()
                );
                crate::metrics::UNEXPECTED_ONDEMAND_DOWNLOADS.inc();

                let really_error =
                    matches!(b, Error) && !self.conf.ondemand_download_behavior_treat_error_as_warn;

                if really_error {
                    // this check is only probablistic, seems like flakyness footgun
                    Err(DownloadError::ContextAndConfigReallyDeniesDownloads)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Actual download, at most one is executed at the time.
    async fn spawn_download_and_wait(
        self: &Arc<Self>,
        timeline: Arc<Timeline>,
        permit: heavier_once_cell::InitPermit,
    ) -> Result<heavier_once_cell::InitPermit, DownloadError> {
        debug_assert_current_span_has_tenant_and_timeline_id();

        let task_name = format!("download layer {}", self);

        let (tx, rx) = tokio::sync::oneshot::channel();

        // this is sadly needed because of task_mgr::shutdown_tasks, otherwise we cannot
        // block tenant::mgr::remove_tenant_from_memory.

        let this: Arc<Self> = self.clone();

        crate::task_mgr::spawn(
            &tokio::runtime::Handle::current(),
            crate::task_mgr::TaskKind::RemoteDownloadTask,
            Some(self.desc.tenant_shard_id),
            Some(self.desc.timeline_id),
            &task_name,
            false,
            async move {

                let client = timeline
                    .remote_client
                    .as_ref()
                    .expect("checked above with have_remote_client");

                let result = client.download_layer_file(
                    &this.desc.filename(),
                    &this.metadata(),
                    &crate::task_mgr::shutdown_token()
                )
                .await;

                let result = match result {
                    Ok(size) => {
                        timeline.metrics.resident_physical_size_add(size);
                        Ok(())
                    }
                    Err(e) => {
                        let consecutive_failures =
                            this.consecutive_failures.fetch_add(1, Ordering::Relaxed);

                        let backoff = utils::backoff::exponential_backoff_duration_seconds(
                            consecutive_failures.min(u32::MAX as usize) as u32,
                            1.5,
                            60.0,
                        );

                        let backoff = std::time::Duration::from_secs_f64(backoff);

                        tokio::select! {
                            _ = tokio::time::sleep(backoff) => {},
                            _ = crate::task_mgr::shutdown_token().cancelled_owned() => {},
                            _ = timeline.cancel.cancelled() => {},
                        };

                        Err(e)
                    }
                };

                if let Err(res) = tx.send((result, permit)) {
                    match res {
                        (Ok(()), _) => {
                            // our caller is cancellation safe so this is fine; if someone
                            // else requests the layer, they'll find it already downloaded.
                            //
                            // See counter [`LayerImplMetrics::inc_init_needed_no_download`]
                            //
                            // FIXME(#6028): however, could be that we should consider marking the
                            // layer for eviction? alas, cannot: because only DownloadedLayer will
                            // handle that.
                        },
                        (Err(e), _) => {
                            // our caller is cancellation safe, but we might be racing with
                            // another attempt to initialize. before we have cancellation
                            // token support: these attempts should converge regardless of
                            // their completion order.
                            tracing::error!("layer file download failed, and additionally failed to communicate this to caller: {e:?}");
                            LAYER_IMPL_METRICS.inc_download_failed_without_requester();
                        }
                    }
                }

                Ok(())
            }
            .in_current_span(),
        );
        match rx.await {
            Ok((Ok(()), permit)) => {
                if let Some(reason) = self
                    .needs_download()
                    .await
                    .map_err(DownloadError::PostStatFailed)?
                {
                    // this is really a bug in needs_download or remote timeline client
                    panic!("post-condition failed: needs_download returned {reason:?}");
                }

                self.consecutive_failures.store(0, Ordering::Relaxed);
                tracing::info!("on-demand download successful");

                Ok(permit)
            }
            Ok((Err(e), _permit)) => {
                // sleep already happened in the spawned task, if it was not cancelled
                let consecutive_failures = self.consecutive_failures.load(Ordering::Relaxed);

                match e.downcast_ref::<remote_storage::DownloadError>() {
                    // If the download failed due to its cancellation token,
                    // propagate the cancellation error upstream.
                    Some(remote_storage::DownloadError::Cancelled) => {
                        Err(DownloadError::DownloadCancelled)
                    }
                    _ => {
                        tracing::error!(consecutive_failures, "layer file download failed: {e:#}");
                        Err(DownloadError::DownloadFailed)
                    }
                }
            }
            Err(_gone) => Err(DownloadError::DownloadCancelled),
        }
    }

    async fn needs_download(&self) -> Result<Option<NeedsDownload>, std::io::Error> {
        match tokio::fs::metadata(&self.path).await {
            Ok(m) => Ok(self.is_file_present_and_good_size(&m).err()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Some(NeedsDownload::NotFound)),
            Err(e) => Err(e),
        }
    }

    fn needs_download_blocking(&self) -> Result<Option<NeedsDownload>, std::io::Error> {
        match self.path.metadata() {
            Ok(m) => Ok(self.is_file_present_and_good_size(&m).err()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Some(NeedsDownload::NotFound)),
            Err(e) => Err(e),
        }
    }

    fn is_file_present_and_good_size(&self, m: &std::fs::Metadata) -> Result<(), NeedsDownload> {
        // in future, this should include sha2-256 validation of the file.
        if !m.is_file() {
            Err(NeedsDownload::NotFile(m.file_type()))
        } else if m.len() != self.desc.file_size {
            Err(NeedsDownload::WrongSize {
                actual: m.len(),
                expected: self.desc.file_size,
            })
        } else {
            Ok(())
        }
    }

    fn info(&self, reset: LayerAccessStatsReset) -> HistoricLayerInfo {
        let layer_file_name = self.desc.filename().file_name();

        // this is not accurate: we could have the file locally but there was a cancellation
        // and now we are not in sync, or we are currently downloading it.
        let remote = self.inner.get().is_none();

        let access_stats = self.access_stats.as_api_model(reset);

        if self.desc.is_delta {
            let lsn_range = &self.desc.lsn_range;

            HistoricLayerInfo::Delta {
                layer_file_name,
                layer_file_size: self.desc.file_size,
                lsn_start: lsn_range.start,
                lsn_end: lsn_range.end,
                remote,
                access_stats,
            }
        } else {
            let lsn = self.desc.image_layer_lsn();

            HistoricLayerInfo::Image {
                layer_file_name,
                layer_file_size: self.desc.file_size,
                lsn_start: lsn,
                remote,
                access_stats,
            }
        }
    }

    /// `DownloadedLayer` is being dropped, so it calls this method.
    fn on_downloaded_layer_drop(self: Arc<LayerInner>, version: usize) {
        let delete = self.wanted_deleted.load(Ordering::Acquire);
        let evict = self.wanted_evicted.load(Ordering::Acquire);
        let can_evict = self.have_remote_client;

        if delete {
            // do nothing now, only in LayerInner::drop -- this was originally implemented because
            // we could had already scheduled the deletion at the time.
            //
            // FIXME: this is not true anymore, we can safely evict wanted deleted files.
        } else if can_evict && evict {
            let span = tracing::info_span!(parent: None, "layer_evict", tenant_id = %self.desc.tenant_shard_id.tenant_id, shard_id = %self.desc.tenant_shard_id.shard_slug(), timeline_id = %self.desc.timeline_id, layer=%self, %version);

            // downgrade for queueing, in case there's a tear down already ongoing we should not
            // hold it alive.
            let this = Arc::downgrade(&self);
            drop(self);

            // NOTE: this scope *must* never call `self.inner.get` because evict_and_wait might
            // drop while the `self.inner` is being locked, leading to a deadlock.

            crate::task_mgr::BACKGROUND_RUNTIME.spawn_blocking(move || {
                let _g = span.entered();

                // if LayerInner is already dropped here, do nothing because the delete on drop
                // has already ran while we were in queue
                let Some(this) = this.upgrade() else {
                    LAYER_IMPL_METRICS.inc_eviction_cancelled(EvictionCancelled::LayerGone);
                    return;
                };
                match this.evict_blocking(version) {
                    Ok(()) => LAYER_IMPL_METRICS.inc_completed_evictions(),
                    Err(reason) => LAYER_IMPL_METRICS.inc_eviction_cancelled(reason),
                }
            });
        }
    }

    fn evict_blocking(&self, only_version: usize) -> Result<(), EvictionCancelled> {
        // deleted or detached timeline, don't do anything.
        let Some(timeline) = self.timeline.upgrade() else {
            return Err(EvictionCancelled::TimelineGone);
        };

        // to avoid starting a new download while we evict, keep holding on to the
        // permit.
        let _permit = {
            let maybe_downloaded = self.inner.get();

            let (_weak, permit) = match maybe_downloaded {
                Some(mut guard) => {
                    if let ResidentOrWantedEvicted::WantedEvicted(_weak, version) = &*guard {
                        if *version == only_version {
                            guard.take_and_deinit()
                        } else {
                            // this was not for us; maybe there's another eviction job
                            // TODO: does it make any sense to stall here? unique versions do not
                            // matter, we only want to make sure not to evict a resident, which we
                            // are not doing.
                            return Err(EvictionCancelled::VersionCheckFailed);
                        }
                    } else {
                        return Err(EvictionCancelled::AlreadyReinitialized);
                    }
                }
                None => {
                    // already deinitialized, perhaps get_or_maybe_download did this and is
                    // currently waiting to reinitialize it
                    return Err(EvictionCancelled::LostToDownload);
                }
            };

            permit
        };

        // now accesses to inner.get_or_init wait on the semaphore or the `_permit`

        self.access_stats.record_residence_event(
            LayerResidenceStatus::Evicted,
            LayerResidenceEventReason::ResidenceChange,
        );

        let res = match capture_mtime_and_remove(&self.path) {
            Ok(local_layer_mtime) => {
                let duration = SystemTime::now().duration_since(local_layer_mtime);
                match duration {
                    Ok(elapsed) => {
                        timeline
                            .metrics
                            .evictions_with_low_residence_duration
                            .read()
                            .unwrap()
                            .observe(elapsed);
                        tracing::info!(
                            residence_millis = elapsed.as_millis(),
                            "evicted layer after known residence period"
                        );
                    }
                    Err(_) => {
                        tracing::info!("evicted layer after unknown residence period");
                    }
                }
                timeline.metrics.evictions.inc();
                timeline
                    .metrics
                    .resident_physical_size_sub(self.desc.file_size);

                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::error!(
                    layer_size = %self.desc.file_size,
                    "failed to evict layer from disk, it was already gone (metrics will be inaccurate)"
                );
                Err(EvictionCancelled::FileNotFound)
            }
            Err(e) => {
                tracing::error!("failed to evict file from disk: {e:#}");
                Err(EvictionCancelled::RemoveFailed)
            }
        };

        // we are still holding the permit, so no new spawn_download_and_wait can happen
        drop(self.status.send(Status::Evicted));

        *self.last_evicted_at.lock().unwrap() = Some(std::time::Instant::now());

        res
    }

    fn metadata(&self) -> LayerFileMetadata {
        LayerFileMetadata::new(self.desc.file_size, self.generation, self.shard)
    }
}

fn capture_mtime_and_remove(path: &Utf8Path) -> Result<SystemTime, std::io::Error> {
    let m = path.metadata()?;
    let local_layer_mtime = m.modified()?;
    std::fs::remove_file(path)?;
    Ok(local_layer_mtime)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum EvictionError {
    #[error("layer was already evicted")]
    NotFound,

    /// Evictions must always lose to downloads in races, and this time it happened.
    #[error("layer was downloaded instead")]
    Downloaded,
}

/// Error internal to the [`LayerInner::get_or_maybe_download`]
#[derive(Debug, thiserror::Error)]
pub(crate) enum DownloadError {
    #[error("timeline has already shutdown")]
    TimelineShutdown,
    #[error("no remote storage configured")]
    NoRemoteStorage,
    #[error("context denies downloading")]
    ContextAndConfigReallyDeniesDownloads,
    #[error("downloading is really required but not allowed by this method")]
    DownloadRequired,
    #[error("layer path exists, but it is not a file: {0:?}")]
    NotFile(std::fs::FileType),
    /// Why no error here? Because it will be reported by page_service. We should had also done
    /// retries already.
    #[error("downloading evicted layer file failed")]
    DownloadFailed,
    #[error("downloading failed, possibly for shutdown")]
    DownloadCancelled,
    #[error("pre-condition: stat before download failed")]
    PreStatFailed(#[source] std::io::Error),
    #[error("post-condition: stat after download failed")]
    PostStatFailed(#[source] std::io::Error),
}

#[derive(Debug, PartialEq)]
pub(crate) enum NeedsDownload {
    NotFound,
    NotFile(std::fs::FileType),
    WrongSize { actual: u64, expected: u64 },
}

impl std::fmt::Display for NeedsDownload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NeedsDownload::NotFound => write!(f, "file was not found"),
            NeedsDownload::NotFile(ft) => write!(f, "path is not a file; {ft:?}"),
            NeedsDownload::WrongSize { actual, expected } => {
                write!(f, "file size mismatch {actual} vs. {expected}")
            }
        }
    }
}

/// Existence of `DownloadedLayer` means that we have the file locally, and can later evict it.
pub(crate) struct DownloadedLayer {
    owner: Weak<LayerInner>,
    // Use tokio OnceCell as we do not need to deinitialize this, it'll just get dropped with the
    // DownloadedLayer
    kind: tokio::sync::OnceCell<anyhow::Result<LayerKind>>,
    version: usize,
}

impl std::fmt::Debug for DownloadedLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadedLayer")
            // owner omitted because it is always "Weak"
            .field("kind", &self.kind)
            .field("version", &self.version)
            .finish()
    }
}

impl Drop for DownloadedLayer {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.on_downloaded_layer_drop(self.version);
        } else {
            // no need to do anything, we are shutting down
        }
    }
}

impl DownloadedLayer {
    /// Initializes the `DeltaLayerInner` or `ImageLayerInner` within [`LayerKind`], or fails to
    /// initialize it permanently.
    ///
    /// `owner` parameter is a strong reference at the same `LayerInner` as the
    /// `DownloadedLayer::owner` would be when upgraded. Given how this method ends up called,
    /// we will always have the LayerInner on the callstack, so we can just use it.
    async fn get<'a>(
        &'a self,
        owner: &Arc<LayerInner>,
        ctx: &RequestContext,
    ) -> anyhow::Result<&'a LayerKind> {
        let init = || async {
            assert_eq!(
                Weak::as_ptr(&self.owner),
                Arc::as_ptr(owner),
                "these are the same, just avoiding the upgrade"
            );

            let res = if owner.desc.is_delta {
                let summary = Some(delta_layer::Summary::expected(
                    owner.desc.tenant_shard_id.tenant_id,
                    owner.desc.timeline_id,
                    owner.desc.key_range.clone(),
                    owner.desc.lsn_range.clone(),
                ));
                delta_layer::DeltaLayerInner::load(&owner.path, summary, ctx)
                    .await
                    .map(|res| res.map(LayerKind::Delta))
            } else {
                let lsn = owner.desc.image_layer_lsn();
                let summary = Some(image_layer::Summary::expected(
                    owner.desc.tenant_shard_id.tenant_id,
                    owner.desc.timeline_id,
                    owner.desc.key_range.clone(),
                    lsn,
                ));
                image_layer::ImageLayerInner::load(&owner.path, lsn, summary, ctx)
                    .await
                    .map(|res| res.map(LayerKind::Image))
            };

            match res {
                Ok(Ok(layer)) => Ok(Ok(layer)),
                Ok(Err(transient)) => Err(transient),
                Err(permanent) => {
                    LAYER_IMPL_METRICS.inc_permanent_loading_failures();
                    // TODO(#5815): we are not logging all errors, so temporarily log them **once**
                    // here as well
                    let permanent = permanent.context("load layer");
                    tracing::error!("layer loading failed permanently: {permanent:#}");
                    Ok(Err(permanent))
                }
            }
        };
        self.kind
            .get_or_try_init(init)
            // return transient errors using `?`
            .await?
            .as_ref()
            .map_err(|e| {
                // errors are not clonabled, cannot but stringify
                // test_broken_timeline matches this string
                anyhow::anyhow!("layer loading failed: {e:#}")
            })
    }

    async fn get_value_reconstruct_data(
        &self,
        key: Key,
        lsn_range: Range<Lsn>,
        reconstruct_data: &mut ValueReconstructState,
        owner: &Arc<LayerInner>,
        ctx: &RequestContext,
    ) -> anyhow::Result<ValueReconstructResult> {
        use LayerKind::*;

        match self.get(owner, ctx).await? {
            Delta(d) => {
                d.get_value_reconstruct_data(key, lsn_range, reconstruct_data, ctx)
                    .await
            }
            Image(i) => {
                i.get_value_reconstruct_data(key, reconstruct_data, ctx)
                    .await
            }
        }
    }

    async fn get_values_reconstruct_data(
        &self,
        keyspace: KeySpace,
        end_lsn: Lsn,
        reconstruct_data: &mut ValuesReconstructState,
        owner: &Arc<LayerInner>,
        ctx: &RequestContext,
    ) -> Result<(), GetVectoredError> {
        use LayerKind::*;

        match self.get(owner, ctx).await.map_err(GetVectoredError::from)? {
            Delta(d) => {
                d.get_values_reconstruct_data(keyspace, end_lsn, reconstruct_data, ctx)
                    .await
            }
            Image(i) => {
                i.get_values_reconstruct_data(keyspace, reconstruct_data, ctx)
                    .await
            }
        }
    }

    async fn dump(&self, owner: &Arc<LayerInner>, ctx: &RequestContext) -> anyhow::Result<()> {
        use LayerKind::*;
        match self.get(owner, ctx).await? {
            Delta(d) => d.dump(ctx).await?,
            Image(i) => i.dump(ctx).await?,
        }

        Ok(())
    }
}

/// Wrapper around an actual layer implementation.
#[derive(Debug)]
enum LayerKind {
    Delta(delta_layer::DeltaLayerInner),
    Image(image_layer::ImageLayerInner),
}

/// Guard for forcing a layer be resident while it exists.
#[derive(Clone)]
pub(crate) struct ResidentLayer {
    owner: Layer,
    downloaded: Arc<DownloadedLayer>,
}

impl std::fmt::Display for ResidentLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.owner)
    }
}

impl std::fmt::Debug for ResidentLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.owner)
    }
}

impl ResidentLayer {
    /// Release the eviction guard, converting back into a plain [`Layer`].
    ///
    /// You can access the [`Layer`] also by using `as_ref`.
    pub(crate) fn drop_eviction_guard(self) -> Layer {
        self.into()
    }

    /// Loads all keys stored in the layer. Returns key, lsn and value size.
    #[tracing::instrument(skip_all, fields(layer=%self))]
    pub(crate) async fn load_keys<'a>(
        &'a self,
        ctx: &RequestContext,
    ) -> anyhow::Result<Vec<DeltaEntry<'a>>> {
        use LayerKind::*;

        let owner = &self.owner.0;

        match self.downloaded.get(owner, ctx).await? {
            Delta(ref d) => {
                owner
                    .access_stats
                    .record_access(LayerAccessKind::KeyIter, ctx);

                // this is valid because the DownloadedLayer::kind is a OnceCell, not a
                // Mutex<OnceCell>, so we cannot go and deinitialize the value with OnceCell::take
                // while it's being held.
                delta_layer::DeltaLayerInner::load_keys(d, ctx)
                    .await
                    .context("Layer index is corrupted")
            }
            Image(_) => anyhow::bail!("cannot load_keys on a image layer"),
        }
    }

    pub(crate) fn local_path(&self) -> &Utf8Path {
        &self.owner.0.path
    }

    pub(crate) fn metadata(&self) -> LayerFileMetadata {
        self.owner.metadata()
    }
}

impl AsLayerDesc for ResidentLayer {
    fn layer_desc(&self) -> &PersistentLayerDesc {
        self.owner.layer_desc()
    }
}

impl AsRef<Layer> for ResidentLayer {
    fn as_ref(&self) -> &Layer {
        &self.owner
    }
}

/// Drop the eviction guard.
impl From<ResidentLayer> for Layer {
    fn from(value: ResidentLayer) -> Self {
        value.owner
    }
}

use metrics::IntCounter;

pub(crate) struct LayerImplMetrics {
    started_evictions: IntCounter,
    completed_evictions: IntCounter,
    cancelled_evictions: enum_map::EnumMap<EvictionCancelled, IntCounter>,

    started_deletes: IntCounter,
    completed_deletes: IntCounter,
    failed_deletes: enum_map::EnumMap<DeleteFailed, IntCounter>,

    rare_counters: enum_map::EnumMap<RareEvent, IntCounter>,
    inits_cancelled: metrics::core::GenericCounter<metrics::core::AtomicU64>,
    redownload_after: metrics::Histogram,
}

impl Default for LayerImplMetrics {
    fn default() -> Self {
        use enum_map::Enum;

        // reminder: these will be pageserver_layer_* with "_total" suffix

        let started_evictions = metrics::register_int_counter!(
            "pageserver_layer_started_evictions",
            "Evictions started in the Layer implementation"
        )
        .unwrap();
        let completed_evictions = metrics::register_int_counter!(
            "pageserver_layer_completed_evictions",
            "Evictions completed in the Layer implementation"
        )
        .unwrap();

        let cancelled_evictions = metrics::register_int_counter_vec!(
            "pageserver_layer_cancelled_evictions_count",
            "Different reasons for evictions to have been cancelled or failed",
            &["reason"]
        )
        .unwrap();

        let cancelled_evictions = enum_map::EnumMap::from_array(std::array::from_fn(|i| {
            let reason = EvictionCancelled::from_usize(i);
            let s = reason.as_str();
            cancelled_evictions.with_label_values(&[s])
        }));

        let started_deletes = metrics::register_int_counter!(
            "pageserver_layer_started_deletes",
            "Deletions on drop pending in the Layer implementation"
        )
        .unwrap();
        let completed_deletes = metrics::register_int_counter!(
            "pageserver_layer_completed_deletes",
            "Deletions on drop completed in the Layer implementation"
        )
        .unwrap();

        let failed_deletes = metrics::register_int_counter_vec!(
            "pageserver_layer_failed_deletes_count",
            "Different reasons for deletions on drop to have failed",
            &["reason"]
        )
        .unwrap();

        let failed_deletes = enum_map::EnumMap::from_array(std::array::from_fn(|i| {
            let reason = DeleteFailed::from_usize(i);
            let s = reason.as_str();
            failed_deletes.with_label_values(&[s])
        }));

        let rare_counters = metrics::register_int_counter_vec!(
            "pageserver_layer_assumed_rare_count",
            "Times unexpected or assumed rare event happened",
            &["event"]
        )
        .unwrap();

        let rare_counters = enum_map::EnumMap::from_array(std::array::from_fn(|i| {
            let event = RareEvent::from_usize(i);
            let s = event.as_str();
            rare_counters.with_label_values(&[s])
        }));

        let inits_cancelled = metrics::register_int_counter!(
            "pageserver_layer_inits_cancelled_count",
            "Times Layer initialization was cancelled",
        )
        .unwrap();

        let redownload_after = {
            let minute = 60.0;
            let hour = 60.0 * minute;
            metrics::register_histogram!(
                "pageserver_layer_redownloaded_after",
                "Time between evicting and re-downloading.",
                vec![
                    10.0,
                    30.0,
                    minute,
                    5.0 * minute,
                    15.0 * minute,
                    30.0 * minute,
                    hour,
                    12.0 * hour,
                ]
            )
            .unwrap()
        };

        Self {
            started_evictions,
            completed_evictions,
            cancelled_evictions,

            started_deletes,
            completed_deletes,
            failed_deletes,

            rare_counters,
            inits_cancelled,
            redownload_after,
        }
    }
}

impl LayerImplMetrics {
    fn inc_started_evictions(&self) {
        self.started_evictions.inc();
    }
    fn inc_completed_evictions(&self) {
        self.completed_evictions.inc();
    }
    fn inc_eviction_cancelled(&self, reason: EvictionCancelled) {
        self.cancelled_evictions[reason].inc()
    }

    fn inc_started_deletes(&self) {
        self.started_deletes.inc();
    }
    fn inc_completed_deletes(&self) {
        self.completed_deletes.inc();
    }
    fn inc_deletes_failed(&self, reason: DeleteFailed) {
        self.failed_deletes[reason].inc();
    }

    /// Counted separatedly from failed layer deletes because we will complete the layer deletion
    /// attempt regardless of failure to delete local file.
    fn inc_delete_removes_failed(&self) {
        self.rare_counters[RareEvent::RemoveOnDropFailed].inc();
    }

    /// Expected rare because requires a race with `evict_blocking` and `get_or_maybe_download`.
    fn inc_retried_get_or_maybe_download(&self) {
        self.rare_counters[RareEvent::RetriedGetOrMaybeDownload].inc();
    }

    /// Expected rare because cancellations are unexpected, and failures are unexpected
    fn inc_download_failed_without_requester(&self) {
        self.rare_counters[RareEvent::DownloadFailedWithoutRequester].inc();
    }

    /// The Weak in ResidentOrWantedEvicted::WantedEvicted was successfully upgraded.
    ///
    /// If this counter is always zero, we should replace ResidentOrWantedEvicted type with an
    /// Option.
    fn inc_raced_wanted_evicted_accesses(&self) {
        self.rare_counters[RareEvent::UpgradedWantedEvicted].inc();
    }

    /// These are only expected for [`Self::inc_init_cancelled`] amount when
    /// running with remote storage.
    fn inc_init_needed_no_download(&self) {
        self.rare_counters[RareEvent::InitWithoutDownload].inc();
    }

    /// Expected rare because all layer files should be readable and good
    fn inc_permanent_loading_failures(&self) {
        self.rare_counters[RareEvent::PermanentLoadingFailure].inc();
    }

    fn inc_broadcast_lagged(&self) {
        self.rare_counters[RareEvent::EvictAndWaitLagged].inc();
    }

    fn inc_init_cancelled(&self) {
        self.inits_cancelled.inc()
    }

    fn record_redownloaded_after(&self, duration: std::time::Duration) {
        self.redownload_after.observe(duration.as_secs_f64())
    }
}

#[derive(enum_map::Enum)]
enum EvictionCancelled {
    LayerGone,
    TimelineGone,
    VersionCheckFailed,
    FileNotFound,
    RemoveFailed,
    AlreadyReinitialized,
    /// Not evicted because of a pending reinitialization
    LostToDownload,
    /// After eviction, there was a new layer access which cancelled the eviction.
    UpgradedBackOnAccess,
}

impl EvictionCancelled {
    fn as_str(&self) -> &'static str {
        match self {
            EvictionCancelled::LayerGone => "layer_gone",
            EvictionCancelled::TimelineGone => "timeline_gone",
            EvictionCancelled::VersionCheckFailed => "version_check_fail",
            EvictionCancelled::FileNotFound => "file_not_found",
            EvictionCancelled::RemoveFailed => "remove_failed",
            EvictionCancelled::AlreadyReinitialized => "already_reinitialized",
            EvictionCancelled::LostToDownload => "lost_to_download",
            EvictionCancelled::UpgradedBackOnAccess => "upgraded_back_on_access",
        }
    }
}

#[derive(enum_map::Enum)]
enum DeleteFailed {
    TimelineGone,
    DeleteSchedulingFailed,
}

impl DeleteFailed {
    fn as_str(&self) -> &'static str {
        match self {
            DeleteFailed::TimelineGone => "timeline_gone",
            DeleteFailed::DeleteSchedulingFailed => "delete_scheduling_failed",
        }
    }
}

#[derive(enum_map::Enum)]
enum RareEvent {
    RemoveOnDropFailed,
    RetriedGetOrMaybeDownload,
    DownloadFailedWithoutRequester,
    UpgradedWantedEvicted,
    InitWithoutDownload,
    PermanentLoadingFailure,
    EvictAndWaitLagged,
}

impl RareEvent {
    fn as_str(&self) -> &'static str {
        use RareEvent::*;

        match self {
            RemoveOnDropFailed => "remove_on_drop_failed",
            RetriedGetOrMaybeDownload => "retried_gomd",
            DownloadFailedWithoutRequester => "download_failed_without",
            UpgradedWantedEvicted => "raced_wanted_evicted",
            InitWithoutDownload => "init_needed_no_download",
            PermanentLoadingFailure => "permanent_loading_failure",
            EvictAndWaitLagged => "broadcast_lagged",
        }
    }
}

pub(crate) static LAYER_IMPL_METRICS: once_cell::sync::Lazy<LayerImplMetrics> =
    once_cell::sync::Lazy::new(LayerImplMetrics::default);
