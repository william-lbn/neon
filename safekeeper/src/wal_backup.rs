use anyhow::{Context, Result};

use camino::{Utf8Path, Utf8PathBuf};
use futures::stream::FuturesOrdered;
use futures::StreamExt;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use utils::backoff;
use utils::id::NodeId;

use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use postgres_ffi::v14::xlog_utils::XLogSegNoOffsetToRecPtr;
use postgres_ffi::XLogFileName;
use postgres_ffi::{XLogSegNo, PG_TLI};
use remote_storage::{GenericRemoteStorage, RemotePath};
use tokio::fs::File;

use tokio::select;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::*;

use utils::{id::TenantTimelineId, lsn::Lsn};

use crate::metrics::{BACKED_UP_SEGMENTS, BACKUP_ERRORS};
use crate::timeline::{PeerInfo, Timeline};
use crate::{GlobalTimelines, SafeKeeperConf};

use once_cell::sync::OnceCell;

const UPLOAD_FAILURE_RETRY_MIN_MS: u64 = 10;
const UPLOAD_FAILURE_RETRY_MAX_MS: u64 = 5000;

/// Default buffer size when interfacing with [`tokio::fs::File`].
const BUFFER_SIZE: usize = 32 * 1024;

/// Check whether wal backup is required for timeline. If yes, mark that launcher is
/// aware of current status and return the timeline.
async fn is_wal_backup_required(ttid: TenantTimelineId) -> Option<Arc<Timeline>> {
    match GlobalTimelines::get(ttid).ok() {
        Some(tli) => {
            tli.wal_backup_attend().await;
            Some(tli)
        }
        None => None,
    }
}

struct WalBackupTaskHandle {
    shutdown_tx: Sender<()>,
    handle: JoinHandle<()>,
}

struct WalBackupTimelineEntry {
    timeline: Arc<Timeline>,
    handle: Option<WalBackupTaskHandle>,
}

async fn shut_down_task(ttid: TenantTimelineId, entry: &mut WalBackupTimelineEntry) {
    if let Some(wb_handle) = entry.handle.take() {
        // Tell the task to shutdown. Error means task exited earlier, that's ok.
        let _ = wb_handle.shutdown_tx.send(()).await;
        // Await the task itself. TODO: restart panicked tasks earlier.
        if let Err(e) = wb_handle.handle.await {
            warn!("WAL backup task for {} panicked: {}", ttid, e);
        }
    }
}

/// The goal is to ensure that normally only one safekeepers offloads. However,
/// it is fine (and inevitable, as s3 doesn't provide CAS) that for some short
/// time we have several ones as they PUT the same files. Also,
/// - frequently changing the offloader would be bad;
/// - electing seriously lagging safekeeper is undesirable;
/// So we deterministically choose among the reasonably caught up candidates.
/// TODO: take into account failed attempts to deal with hypothetical situation
/// where s3 is unreachable only for some sks.
fn determine_offloader(
    alive_peers: &[PeerInfo],
    wal_backup_lsn: Lsn,
    ttid: TenantTimelineId,
    conf: &SafeKeeperConf,
) -> (Option<NodeId>, String) {
    // TODO: remove this once we fill newly joined safekeepers since backup_lsn.
    let capable_peers = alive_peers
        .iter()
        .filter(|p| p.local_start_lsn <= wal_backup_lsn);
    match capable_peers.clone().map(|p| p.commit_lsn).max() {
        None => (None, "no connected peers to elect from".to_string()),
        Some(max_commit_lsn) => {
            let threshold = max_commit_lsn
                .checked_sub(conf.max_offloader_lag_bytes)
                .unwrap_or(Lsn(0));
            let mut caughtup_peers = capable_peers
                .clone()
                .filter(|p| p.commit_lsn >= threshold)
                .collect::<Vec<_>>();
            caughtup_peers.sort_by(|p1, p2| p1.sk_id.cmp(&p2.sk_id));

            // To distribute the load, shift by timeline_id.
            let offloader = caughtup_peers
                [(u128::from(ttid.timeline_id) % caughtup_peers.len() as u128) as usize]
                .sk_id;

            let mut capable_peers_dbg = capable_peers
                .map(|p| (p.sk_id, p.commit_lsn))
                .collect::<Vec<_>>();
            capable_peers_dbg.sort_by(|p1, p2| p1.0.cmp(&p2.0));
            (
                Some(offloader),
                format!(
                    "elected {} among {:?} peers, with {} of them being caughtup",
                    offloader,
                    capable_peers_dbg,
                    caughtup_peers.len()
                ),
            )
        }
    }
}

/// Based on peer information determine which safekeeper should offload; if it
/// is me, run (per timeline) task, if not yet. OTOH, if it is not me and task
/// is running, kill it.
async fn update_task(
    conf: &SafeKeeperConf,
    ttid: TenantTimelineId,
    entry: &mut WalBackupTimelineEntry,
) {
    let alive_peers = entry.timeline.get_peers(conf).await;
    let wal_backup_lsn = entry.timeline.get_wal_backup_lsn().await;
    let (offloader, election_dbg_str) =
        determine_offloader(&alive_peers, wal_backup_lsn, ttid, conf);
    let elected_me = Some(conf.my_id) == offloader;

    if elected_me != (entry.handle.is_some()) {
        if elected_me {
            info!("elected for backup: {}", election_dbg_str);

            let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
            let timeline_dir = conf.timeline_dir(&ttid);

            let handle = tokio::spawn(
                backup_task_main(
                    ttid,
                    timeline_dir,
                    conf.workdir.clone(),
                    conf.backup_parallel_jobs,
                    shutdown_rx,
                )
                .in_current_span(),
            );

            entry.handle = Some(WalBackupTaskHandle {
                shutdown_tx,
                handle,
            });
        } else {
            info!("stepping down from backup: {}", election_dbg_str);
            shut_down_task(ttid, entry).await;
        }
    }
}

static REMOTE_STORAGE: OnceCell<Option<GenericRemoteStorage>> = OnceCell::new();

// Storage must be configured and initialized when this is called.
fn get_configured_remote_storage() -> &'static GenericRemoteStorage {
    REMOTE_STORAGE
        .get()
        .expect("failed to get remote storage")
        .as_ref()
        .unwrap()
}

const CHECK_TASKS_INTERVAL_MSEC: u64 = 1000;

/// Sits on wal_backup_launcher_rx and starts/stops per timeline wal backup
/// tasks. Having this in separate task simplifies locking, allows to reap
/// panics and separate elections from offloading itself.
pub async fn wal_backup_launcher_task_main(
    conf: SafeKeeperConf,
    mut wal_backup_launcher_rx: Receiver<TenantTimelineId>,
) -> anyhow::Result<()> {
    info!(
        "WAL backup launcher started, remote config {:?}",
        conf.remote_storage
    );

    let conf_ = conf.clone();
    REMOTE_STORAGE.get_or_init(|| {
        conf_
            .remote_storage
            .as_ref()
            .map(|c| GenericRemoteStorage::from_config(c).expect("failed to create remote storage"))
    });

    // Presence in this map means launcher is aware s3 offloading is needed for
    // the timeline, but task is started only if it makes sense for to offload
    // from this safekeeper.
    let mut tasks: HashMap<TenantTimelineId, WalBackupTimelineEntry> = HashMap::new();

    let mut ticker = tokio::time::interval(Duration::from_millis(CHECK_TASKS_INTERVAL_MSEC));
    loop {
        tokio::select! {
            ttid = wal_backup_launcher_rx.recv() => {
                // channel is never expected to get closed
                let ttid = ttid.unwrap();
                if !conf.is_wal_backup_enabled() {
                    continue; /* just drain the channel and do nothing */
                }
                async {
                    let timeline = is_wal_backup_required(ttid).await;
                    // do we need to do anything at all?
                    if timeline.is_some() != tasks.contains_key(&ttid) {
                        if let Some(timeline) = timeline {
                            // need to start the task
                            let entry = tasks.entry(ttid).or_insert(WalBackupTimelineEntry {
                                timeline,
                                handle: None,
                            });
                            update_task(&conf, ttid, entry).await;
                        } else {
                            // need to stop the task
                            info!("stopping WAL backup task");
                            let mut entry = tasks.remove(&ttid).unwrap();
                            shut_down_task(ttid, &mut entry).await;
                        }
                    }
                }.instrument(info_span!("WAL backup", ttid = %ttid)).await;
            }
            // For each timeline needing offloading, check if this safekeeper
            // should do the job and start/stop the task accordingly.
            _ = ticker.tick() => {
                for (ttid, entry) in tasks.iter_mut() {
                    update_task(&conf, *ttid, entry)
                        .instrument(info_span!("WAL backup", ttid = %ttid))
                        .await;
                }
            }
        }
    }
}

struct WalBackupTask {
    timeline: Arc<Timeline>,
    timeline_dir: Utf8PathBuf,
    workspace_dir: Utf8PathBuf,
    wal_seg_size: usize,
    parallel_jobs: usize,
    commit_lsn_watch_rx: watch::Receiver<Lsn>,
}

/// Offload single timeline.
async fn backup_task_main(
    ttid: TenantTimelineId,
    timeline_dir: Utf8PathBuf,
    workspace_dir: Utf8PathBuf,
    parallel_jobs: usize,
    mut shutdown_rx: Receiver<()>,
) {
    info!("started");
    let res = GlobalTimelines::get(ttid);
    if let Err(e) = res {
        error!("backup error: {}", e);
        return;
    }
    let tli = res.unwrap();

    let mut wb = WalBackupTask {
        wal_seg_size: tli.get_wal_seg_size().await,
        commit_lsn_watch_rx: tli.get_commit_lsn_watch_rx(),
        timeline: tli,
        timeline_dir,
        workspace_dir,
        parallel_jobs,
    };

    // task is spinned up only when wal_seg_size already initialized
    assert!(wb.wal_seg_size > 0);

    let mut canceled = false;
    select! {
        _ = wb.run() => {}
        _ = shutdown_rx.recv() => {
            canceled = true;
        }
    }
    info!("task {}", if canceled { "canceled" } else { "terminated" });
}

impl WalBackupTask {
    async fn run(&mut self) {
        let mut backup_lsn = Lsn(0);

        let mut retry_attempt = 0u32;
        // offload loop
        loop {
            if retry_attempt == 0 {
                // wait for new WAL to arrive
                if let Err(e) = self.commit_lsn_watch_rx.changed().await {
                    // should never happen, as we hold Arc to timeline.
                    error!("commit_lsn watch shut down: {:?}", e);
                    return;
                }
            } else {
                // or just sleep if we errored previously
                let mut retry_delay = UPLOAD_FAILURE_RETRY_MAX_MS;
                if let Some(backoff_delay) = UPLOAD_FAILURE_RETRY_MIN_MS.checked_shl(retry_attempt)
                {
                    retry_delay = min(retry_delay, backoff_delay);
                }
                sleep(Duration::from_millis(retry_delay)).await;
            }

            let commit_lsn = *self.commit_lsn_watch_rx.borrow();

            // Note that backup_lsn can be higher than commit_lsn if we
            // don't have much local WAL and others already uploaded
            // segments we don't even have.
            if backup_lsn.segment_number(self.wal_seg_size)
                >= commit_lsn.segment_number(self.wal_seg_size)
            {
                retry_attempt = 0;
                continue; /* nothing to do, common case as we wake up on every commit_lsn bump */
            }
            // Perhaps peers advanced the position, check shmem value.
            backup_lsn = self.timeline.get_wal_backup_lsn().await;
            if backup_lsn.segment_number(self.wal_seg_size)
                >= commit_lsn.segment_number(self.wal_seg_size)
            {
                retry_attempt = 0;
                continue;
            }

            match backup_lsn_range(
                &self.timeline,
                &mut backup_lsn,
                commit_lsn,
                self.wal_seg_size,
                &self.timeline_dir,
                &self.workspace_dir,
                self.parallel_jobs,
            )
            .await
            {
                Ok(()) => {
                    retry_attempt = 0;
                }
                Err(e) => {
                    error!(
                        "failed while offloading range {}-{}: {:?}",
                        backup_lsn, commit_lsn, e
                    );

                    retry_attempt = retry_attempt.saturating_add(1);
                }
            }
        }
    }
}

async fn backup_lsn_range(
    timeline: &Arc<Timeline>,
    backup_lsn: &mut Lsn,
    end_lsn: Lsn,
    wal_seg_size: usize,
    timeline_dir: &Utf8Path,
    workspace_dir: &Utf8Path,
    parallel_jobs: usize,
) -> Result<()> {
    if parallel_jobs < 1 {
        anyhow::bail!("parallel_jobs must be >= 1");
    }

    let start_lsn = *backup_lsn;
    let segments = get_segments(start_lsn, end_lsn, wal_seg_size);

    // Pool of concurrent upload tasks. We use `FuturesOrdered` to
    // preserve order of uploads, and update `backup_lsn` only after
    // all previous uploads are finished.
    let mut uploads = FuturesOrdered::new();
    let mut iter = segments.iter();

    loop {
        let added_task = match iter.next() {
            Some(s) => {
                uploads.push_back(backup_single_segment(s, timeline_dir, workspace_dir));
                true
            }
            None => false,
        };

        // Wait for the next segment to upload if we don't have any more segments,
        // or if we have too many concurrent uploads.
        if !added_task || uploads.len() >= parallel_jobs {
            let next = uploads.next().await;
            if let Some(res) = next {
                // next segment uploaded
                let segment = res?;
                let new_backup_lsn = segment.end_lsn;
                timeline
                    .set_wal_backup_lsn(new_backup_lsn)
                    .await
                    .context("setting wal_backup_lsn")?;
                *backup_lsn = new_backup_lsn;
            } else {
                // no more segments to upload
                break;
            }
        }
    }

    info!(
        "offloaded segnos {:?} up to {}, previous backup_lsn {}",
        segments.iter().map(|&s| s.seg_no).collect::<Vec<_>>(),
        end_lsn,
        start_lsn,
    );
    Ok(())
}

async fn backup_single_segment(
    seg: &Segment,
    timeline_dir: &Utf8Path,
    workspace_dir: &Utf8Path,
) -> Result<Segment> {
    let segment_file_path = seg.file_path(timeline_dir)?;
    let remote_segment_path = segment_file_path
        .strip_prefix(workspace_dir)
        .context("Failed to strip workspace dir prefix")
        .and_then(RemotePath::new)
        .with_context(|| {
            format!(
                "Failed to resolve remote part of path {segment_file_path:?} for base {workspace_dir:?}",
            )
        })?;

    let res = backup_object(&segment_file_path, &remote_segment_path, seg.size()).await;
    if res.is_ok() {
        BACKED_UP_SEGMENTS.inc();
    } else {
        BACKUP_ERRORS.inc();
    }
    res?;
    debug!("Backup of {} done", segment_file_path);

    Ok(*seg)
}

#[derive(Debug, Copy, Clone)]
pub struct Segment {
    seg_no: XLogSegNo,
    start_lsn: Lsn,
    end_lsn: Lsn,
}

impl Segment {
    pub fn new(seg_no: u64, start_lsn: Lsn, end_lsn: Lsn) -> Self {
        Self {
            seg_no,
            start_lsn,
            end_lsn,
        }
    }

    pub fn object_name(self) -> String {
        XLogFileName(PG_TLI, self.seg_no, self.size())
    }

    pub fn file_path(self, timeline_dir: &Utf8Path) -> Result<Utf8PathBuf> {
        Ok(timeline_dir.join(self.object_name()))
    }

    pub fn size(self) -> usize {
        (u64::from(self.end_lsn) - u64::from(self.start_lsn)) as usize
    }
}

fn get_segments(start: Lsn, end: Lsn, seg_size: usize) -> Vec<Segment> {
    let first_seg = start.segment_number(seg_size);
    let last_seg = end.segment_number(seg_size);

    let res: Vec<Segment> = (first_seg..last_seg)
        .map(|s| {
            let start_lsn = XLogSegNoOffsetToRecPtr(s, 0, seg_size);
            let end_lsn = XLogSegNoOffsetToRecPtr(s + 1, 0, seg_size);
            Segment::new(s, Lsn::from(start_lsn), Lsn::from(end_lsn))
        })
        .collect();
    res
}

async fn backup_object(
    source_file: &Utf8Path,
    target_file: &RemotePath,
    size: usize,
) -> Result<()> {
    let storage = get_configured_remote_storage();

    let file = File::open(&source_file)
        .await
        .with_context(|| format!("Failed to open file {source_file:?} for wal backup"))?;

    let file = tokio_util::io::ReaderStream::with_capacity(file, BUFFER_SIZE);

    let cancel = CancellationToken::new();

    storage
        .upload_storage_object(file, size, target_file, &cancel)
        .await
}

pub async fn read_object(
    file_path: &RemotePath,
    offset: u64,
) -> anyhow::Result<Pin<Box<dyn tokio::io::AsyncRead + Send + Sync>>> {
    let storage = REMOTE_STORAGE
        .get()
        .context("Failed to get remote storage")?
        .as_ref()
        .context("No remote storage configured")?;

    info!("segment download about to start from remote path {file_path:?} at offset {offset}");

    let cancel = CancellationToken::new();

    let download = storage
        .download_storage_object(Some((offset, None)), file_path, &cancel)
        .await
        .with_context(|| {
            format!("Failed to open WAL segment download stream for remote path {file_path:?}")
        })?;

    let reader = tokio_util::io::StreamReader::new(download.download_stream);

    let reader = tokio::io::BufReader::with_capacity(BUFFER_SIZE, reader);

    Ok(Box::pin(reader))
}

/// Delete WAL files for the given timeline. Remote storage must be configured
/// when called.
pub async fn delete_timeline(ttid: &TenantTimelineId) -> Result<()> {
    let storage = get_configured_remote_storage();
    let ttid_path = Utf8Path::new(&ttid.tenant_id.to_string()).join(ttid.timeline_id.to_string());
    let remote_path = RemotePath::new(&ttid_path)?;

    // see DEFAULT_MAX_KEYS_PER_LIST_RESPONSE
    // const Option unwrap is not stable, otherwise it would be const.
    let batch_size: NonZeroU32 = NonZeroU32::new(1000).unwrap();

    // A backoff::retry is used here for two reasons:
    // - To provide a backoff rather than busy-polling the API on errors
    // - To absorb transient 429/503 conditions without hitting our error
    //   logging path for issues deleting objects.
    //
    // Note: listing segments might take a long time if there are many of them.
    // We don't currently have http requests timeout cancellation, but if/once
    // we have listing should get streaming interface to make progress.

    let cancel = CancellationToken::new(); // not really used
    backoff::retry(
        || async {
            // Do list-delete in batch_size batches to make progress even if there a lot of files.
            // Alternatively we could make list_files return iterator, but it is more complicated and
            // I'm not sure deleting while iterating is expected in s3.
            loop {
                let files = storage
                    .list_files(Some(&remote_path), Some(batch_size), &cancel)
                    .await?;
                if files.is_empty() {
                    return Ok(()); // done
                }
                // (at least) s3 results are sorted, so can log min/max:
                // "List results are always returned in UTF-8 binary order."
                info!(
                    "deleting batch of {} WAL segments [{}-{}]",
                    files.len(),
                    files.first().unwrap().object_name().unwrap_or(""),
                    files.last().unwrap().object_name().unwrap_or("")
                );
                storage.delete_objects(&files, &cancel).await?;
            }
        },
        // consider TimeoutOrCancel::caused_by_cancel when using cancellation
        |_| false,
        3,
        10,
        "executing WAL segments deletion batch",
        &cancel,
    )
    .await
    .ok_or_else(|| anyhow::anyhow!("canceled"))
    .and_then(|x| x)?;

    Ok(())
}

/// Copy segments from one timeline to another. Used in copy_timeline.
pub async fn copy_s3_segments(
    wal_seg_size: usize,
    src_ttid: &TenantTimelineId,
    dst_ttid: &TenantTimelineId,
    from_segment: XLogSegNo,
    to_segment: XLogSegNo,
) -> Result<()> {
    const SEGMENTS_PROGRESS_REPORT_INTERVAL: u64 = 1024;

    let storage = REMOTE_STORAGE
        .get()
        .expect("failed to get remote storage")
        .as_ref()
        .unwrap();

    let relative_dst_path =
        Utf8Path::new(&dst_ttid.tenant_id.to_string()).join(dst_ttid.timeline_id.to_string());

    let remote_path = RemotePath::new(&relative_dst_path)?;

    let cancel = CancellationToken::new();

    let files = storage
        .list_files(Some(&remote_path), None, &cancel)
        .await?;

    let uploaded_segments = &files
        .iter()
        .filter_map(|file| file.object_name().map(ToOwned::to_owned))
        .collect::<HashSet<_>>();

    debug!(
        "these segments have already been uploaded: {:?}",
        uploaded_segments
    );

    let relative_src_path =
        Utf8Path::new(&src_ttid.tenant_id.to_string()).join(src_ttid.timeline_id.to_string());

    for segno in from_segment..to_segment {
        if segno % SEGMENTS_PROGRESS_REPORT_INTERVAL == 0 {
            info!("copied all segments from {} until {}", from_segment, segno);
        }

        let segment_name = XLogFileName(PG_TLI, segno, wal_seg_size);
        if uploaded_segments.contains(&segment_name) {
            continue;
        }
        debug!("copying segment {}", segment_name);

        let from = RemotePath::new(&relative_src_path.join(&segment_name))?;
        let to = RemotePath::new(&relative_dst_path.join(&segment_name))?;

        storage.copy_object(&from, &to, &cancel).await?;
    }

    info!(
        "finished copying segments from {} until {}",
        from_segment, to_segment
    );
    Ok(())
}
