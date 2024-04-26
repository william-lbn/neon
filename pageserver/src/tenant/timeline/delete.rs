use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use anyhow::Context;
use pageserver_api::{models::TimelineState, shard::TenantShardId};
use tokio::sync::OwnedMutexGuard;
use tracing::{debug, error, info, instrument, Instrument};
use utils::{crashsafe, fs_ext, id::TimelineId};

use crate::{
    config::PageServerConf,
    deletion_queue::DeletionQueueClient,
    task_mgr::{self, TaskKind},
    tenant::{
        debug_assert_current_span_has_tenant_and_timeline_id,
        metadata::TimelineMetadata,
        remote_timeline_client::{
            self, PersistIndexPartWithDeletedFlagError, RemoteTimelineClient,
        },
        CreateTimelineCause, DeleteTimelineError, Tenant,
    },
};

use super::{Timeline, TimelineResources};

/// Now that the Timeline is in Stopping state, request all the related tasks to shut down.
async fn stop_tasks(timeline: &Timeline) -> Result<(), DeleteTimelineError> {
    debug_assert_current_span_has_tenant_and_timeline_id();
    // Notify any timeline work to drop out of loops/requests
    tracing::debug!("Cancelling CancellationToken");
    timeline.cancel.cancel();

    // Stop the walreceiver first.
    debug!("waiting for wal receiver to shutdown");
    let maybe_started_walreceiver = { timeline.walreceiver.lock().unwrap().take() };
    if let Some(walreceiver) = maybe_started_walreceiver {
        walreceiver.stop().await;
    }
    debug!("wal receiver shutdown confirmed");

    // Shut down the layer flush task before the remote client, as one depends on the other
    task_mgr::shutdown_tasks(
        Some(TaskKind::LayerFlushTask),
        Some(timeline.tenant_shard_id),
        Some(timeline.timeline_id),
    )
    .await;

    // Prevent new uploads from starting.
    if let Some(remote_client) = timeline.remote_client.as_ref() {
        let res = remote_client.stop();
        match res {
            Ok(()) => {}
            Err(e) => match e {
                remote_timeline_client::StopError::QueueUninitialized => {
                    // This case shouldn't happen currently because the
                    // load and attach code bails out if _any_ of the timeline fails to fetch its IndexPart.
                    // That is, before we declare the Tenant as Active.
                    // But we only allow calls to delete_timeline on Active tenants.
                    return Err(DeleteTimelineError::Other(anyhow::anyhow!("upload queue is uninitialized, likely the timeline was in Broken state prior to this call because it failed to fetch IndexPart during load or attach, check the logs")));
                }
            },
        }
    }

    // Stop & wait for the remaining timeline tasks, including upload tasks.
    // NB: This and other delete_timeline calls do not run as a task_mgr task,
    //     so, they are not affected by this shutdown_tasks() call.
    info!("waiting for timeline tasks to shutdown");
    task_mgr::shutdown_tasks(
        None,
        Some(timeline.tenant_shard_id),
        Some(timeline.timeline_id),
    )
    .await;

    fail::fail_point!("timeline-delete-before-index-deleted-at", |_| {
        Err(anyhow::anyhow!(
            "failpoint: timeline-delete-before-index-deleted-at"
        ))?
    });

    tracing::debug!("Waiting for gate...");
    timeline.gate.close().await;
    tracing::debug!("Shutdown complete");

    Ok(())
}

/// Mark timeline as deleted in S3 so we won't pick it up next time
/// during attach or pageserver restart.
/// See comment in persist_index_part_with_deleted_flag.
async fn set_deleted_in_remote_index(timeline: &Timeline) -> Result<(), DeleteTimelineError> {
    if let Some(remote_client) = timeline.remote_client.as_ref() {
        match remote_client.persist_index_part_with_deleted_flag().await {
            // If we (now, or already) marked it successfully as deleted, we can proceed
            Ok(()) | Err(PersistIndexPartWithDeletedFlagError::AlreadyDeleted(_)) => (),
            // Bail out otherwise
            //
            // AlreadyInProgress shouldn't happen, because the 'delete_lock' prevents
            // two tasks from performing the deletion at the same time. The first task
            // that starts deletion should run it to completion.
            Err(e @ PersistIndexPartWithDeletedFlagError::AlreadyInProgress(_))
            | Err(e @ PersistIndexPartWithDeletedFlagError::Other(_)) => {
                return Err(DeleteTimelineError::Other(anyhow::anyhow!(e)));
            }
        }
    }
    Ok(())
}

/// Grab the compaction and gc locks, and actually perform the deletion.
///
/// The locks prevent GC or compaction from running at the same time. The background tasks do not
/// register themselves with the timeline it's operating on, so it might still be running even
/// though we called `shutdown_tasks`.
///
/// Note that there are still other race conditions between
/// GC, compaction and timeline deletion. See
/// <https://github.com/neondatabase/neon/issues/2671>
///
/// No timeout here, GC & Compaction should be responsive to the
/// `TimelineState::Stopping` change.
// pub(super): documentation link
pub(super) async fn delete_local_timeline_directory(
    conf: &PageServerConf,
    tenant_shard_id: TenantShardId,
    timeline: &Timeline,
) -> anyhow::Result<()> {
    let guards = async { tokio::join!(timeline.gc_lock.lock(), timeline.compaction_lock.lock()) };
    let guards = crate::timed(
        guards,
        "acquire gc and compaction locks",
        std::time::Duration::from_secs(5),
    )
    .await;

    // NB: storage_sync upload tasks that reference these layers have been cancelled
    //     by the caller.

    let local_timeline_directory = conf.timeline_path(&tenant_shard_id, &timeline.timeline_id);

    fail::fail_point!("timeline-delete-before-rm", |_| {
        Err(anyhow::anyhow!("failpoint: timeline-delete-before-rm"))?
    });

    // NB: This need not be atomic because the deleted flag in the IndexPart
    // will be observed during tenant/timeline load. The deletion will be resumed there.
    //
    // Note that here we do not bail out on std::io::ErrorKind::NotFound.
    // This can happen if we're called a second time, e.g.,
    // because of a previous failure/cancellation at/after
    // failpoint timeline-delete-after-rm.
    //
    // ErrorKind::NotFound can also happen if we race with tenant detach, because,
    // no locks are shared.
    tokio::fs::remove_dir_all(local_timeline_directory)
        .await
        .or_else(fs_ext::ignore_not_found)
        .context("remove local timeline directory")?;

    // Make sure previous deletions are ordered before mark removal.
    // Otherwise there is no guarantee that they reach the disk before mark deletion.
    // So its possible for mark to reach disk first and for other deletions
    // to be reordered later and thus missed if a crash occurs.
    // Note that we dont need to sync after mark file is removed
    // because we can tolerate the case when mark file reappears on startup.
    let timeline_path = conf.timelines_path(&tenant_shard_id);
    crashsafe::fsync_async(timeline_path)
        .await
        .context("fsync_pre_mark_remove")?;

    info!("finished deleting layer files, releasing locks");
    drop(guards);

    fail::fail_point!("timeline-delete-after-rm", |_| {
        Err(anyhow::anyhow!("failpoint: timeline-delete-after-rm"))?
    });

    Ok(())
}

/// Removes remote layers and an index file after them.
async fn delete_remote_layers_and_index(timeline: &Timeline) -> anyhow::Result<()> {
    if let Some(remote_client) = &timeline.remote_client {
        remote_client.delete_all().await.context("delete_all")?
    };

    Ok(())
}

// This function removs remaining traces of a timeline on disk.
// Namely: metadata file, timeline directory, delete mark.
// Note: io::ErrorKind::NotFound are ignored for metadata and timeline dir.
// delete mark should be present because it is the last step during deletion.
// (nothing can fail after its deletion)
async fn cleanup_remaining_timeline_fs_traces(
    conf: &PageServerConf,
    tenant_shard_id: TenantShardId,
    timeline_id: TimelineId,
) -> anyhow::Result<()> {
    // Remove delete mark
    // TODO: once we are confident that no more exist in the field, remove this
    // line.  It cleans up a legacy marker file that might in rare cases be present.
    tokio::fs::remove_file(conf.timeline_delete_mark_file_path(tenant_shard_id, timeline_id))
        .await
        .or_else(fs_ext::ignore_not_found)
        .context("remove delete mark")
}

/// It is important that this gets called when DeletionGuard is being held.
/// For more context see comments in [`DeleteTimelineFlow::prepare`]
async fn remove_timeline_from_tenant(
    tenant: &Tenant,
    timeline_id: TimelineId,
    _: &DeletionGuard, // using it as a witness
) -> anyhow::Result<()> {
    // Remove the timeline from the map.
    let mut timelines = tenant.timelines.lock().unwrap();
    let children_exist = timelines
        .iter()
        .any(|(_, entry)| entry.get_ancestor_timeline_id() == Some(timeline_id));
    // XXX this can happen because `branch_timeline` doesn't check `TimelineState::Stopping`.
    // We already deleted the layer files, so it's probably best to panic.
    // (Ideally, above remove_dir_all is atomic so we don't see this timeline after a restart)
    if children_exist {
        panic!("Timeline grew children while we removed layer files");
    }

    timelines
        .remove(&timeline_id)
        .expect("timeline that we were deleting was concurrently removed from 'timelines' map");

    drop(timelines);

    Ok(())
}

/// Orchestrates timeline shut down of all timeline tasks, removes its in-memory structures,
/// and deletes its data from both disk and s3.
/// The sequence of steps:
/// 1. Set deleted_at in remote index part.
/// 2. Create local mark file.
/// 3. Delete local files except metadata (it is simpler this way, to be able to reuse timeline initialization code that expects metadata)
/// 4. Delete remote layers
/// 5. Delete index part
/// 6. Delete meta, timeline directory
/// 7. Delete mark file
/// It is resumable from any step in case a crash/restart occurs.
/// There are three entrypoints to the process:
/// 1. [`DeleteTimelineFlow::run`] this is the main one called by a management api handler.
/// 2. [`DeleteTimelineFlow::resume_deletion`] is called during restarts when local metadata is still present
/// and we possibly neeed to continue deletion of remote files.
/// 3. [`DeleteTimelineFlow::cleanup_remaining_timeline_fs_traces`] is used when we deleted remote
/// index but still have local metadata, timeline directory and delete mark.
/// Note the only other place that messes around timeline delete mark is the logic that scans directory with timelines during tenant load.
#[derive(Default)]
pub enum DeleteTimelineFlow {
    #[default]
    NotStarted,
    InProgress,
    Finished,
}

impl DeleteTimelineFlow {
    // These steps are run in the context of management api request handler.
    // Long running steps are continued to run in the background.
    // NB: If this fails half-way through, and is retried, the retry will go through
    // all the same steps again. Make sure the code here is idempotent, and don't
    // error out if some of the shutdown tasks have already been completed!
    #[instrument(skip_all, fields(%inplace))]
    pub async fn run(
        tenant: &Arc<Tenant>,
        timeline_id: TimelineId,
        inplace: bool,
    ) -> Result<(), DeleteTimelineError> {
        super::debug_assert_current_span_has_tenant_and_timeline_id();

        let (timeline, mut guard) = Self::prepare(tenant, timeline_id)?;

        guard.mark_in_progress()?;

        stop_tasks(&timeline).await?;

        set_deleted_in_remote_index(&timeline).await?;

        fail::fail_point!("timeline-delete-before-schedule", |_| {
            Err(anyhow::anyhow!(
                "failpoint: timeline-delete-before-schedule"
            ))?
        });

        if inplace {
            Self::background(guard, tenant.conf, tenant, &timeline).await?
        } else {
            Self::schedule_background(guard, tenant.conf, Arc::clone(tenant), timeline);
        }

        Ok(())
    }

    fn mark_in_progress(&mut self) -> anyhow::Result<()> {
        match self {
            Self::Finished => anyhow::bail!("Bug. Is in finished state"),
            Self::InProgress { .. } => { /* We're in a retry */ }
            Self::NotStarted => { /* Fresh start */ }
        }

        *self = Self::InProgress;

        Ok(())
    }

    /// Shortcut to create Timeline in stopping state and spawn deletion task.
    /// See corresponding parts of [`crate::tenant::delete::DeleteTenantFlow`]
    #[instrument(skip_all, fields(%timeline_id))]
    pub async fn resume_deletion(
        tenant: Arc<Tenant>,
        timeline_id: TimelineId,
        local_metadata: &TimelineMetadata,
        remote_client: Option<RemoteTimelineClient>,
        deletion_queue_client: DeletionQueueClient,
    ) -> anyhow::Result<()> {
        // Note: here we even skip populating layer map. Timeline is essentially uninitialized.
        // RemoteTimelineClient is the only functioning part.
        let timeline = tenant
            .create_timeline_struct(
                timeline_id,
                local_metadata,
                None, // Ancestor is not needed for deletion.
                TimelineResources {
                    remote_client,
                    deletion_queue_client,
                    timeline_get_throttle: tenant.timeline_get_throttle.clone(),
                },
                // Important. We dont pass ancestor above because it can be missing.
                // Thus we need to skip the validation here.
                CreateTimelineCause::Delete,
            )
            .context("create_timeline_struct")?;

        let mut guard = DeletionGuard(
            Arc::clone(&timeline.delete_progress)
                .try_lock_owned()
                .expect("cannot happen because we're the only owner"),
        );

        // We meed to do this because when console retries delete request we shouldnt answer with 404
        // because 404 means successful deletion.
        {
            let mut locked = tenant.timelines.lock().unwrap();
            locked.insert(timeline_id, Arc::clone(&timeline));
        }

        guard.mark_in_progress()?;

        Self::schedule_background(guard, tenant.conf, tenant, timeline);

        Ok(())
    }

    #[instrument(skip_all, fields(%timeline_id))]
    pub async fn cleanup_remaining_timeline_fs_traces(
        tenant: &Tenant,
        timeline_id: TimelineId,
    ) -> anyhow::Result<()> {
        let r =
            cleanup_remaining_timeline_fs_traces(tenant.conf, tenant.tenant_shard_id, timeline_id)
                .await;
        info!("Done");
        r
    }

    fn prepare(
        tenant: &Tenant,
        timeline_id: TimelineId,
    ) -> Result<(Arc<Timeline>, DeletionGuard), DeleteTimelineError> {
        // Note the interaction between this guard and deletion guard.
        // Here we attempt to lock deletion guard when we're holding a lock on timelines.
        // This is important because when you take into account `remove_timeline_from_tenant`
        // we remove timeline from memory when we still hold the deletion guard.
        // So here when timeline deletion is finished timeline wont be present in timelines map at all
        // which makes the following sequence impossible:
        // T1: get preempted right before the try_lock on `Timeline::delete_progress`
        // T2: do a full deletion, acquire and drop `Timeline::delete_progress`
        // T1: acquire deletion lock, do another `DeleteTimelineFlow::run`
        // For more context see this discussion: `https://github.com/neondatabase/neon/pull/4552#discussion_r1253437346`
        let timelines = tenant.timelines.lock().unwrap();

        let timeline = match timelines.get(&timeline_id) {
            Some(t) => t,
            None => return Err(DeleteTimelineError::NotFound),
        };

        // Ensure that there are no child timelines **attached to that pageserver**,
        // because detach removes files, which will break child branches
        let children: Vec<TimelineId> = timelines
            .iter()
            .filter_map(|(id, entry)| {
                if entry.get_ancestor_timeline_id() == Some(timeline_id) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        if !children.is_empty() {
            return Err(DeleteTimelineError::HasChildren(children));
        }

        // Note that using try_lock here is important to avoid a deadlock.
        // Here we take lock on timelines and then the deletion guard.
        // At the end of the operation we're holding the guard and need to lock timelines map
        // to remove the timeline from it.
        // Always if you have two locks that are taken in different order this can result in a deadlock.

        let delete_progress = Arc::clone(&timeline.delete_progress);
        let delete_lock_guard = match delete_progress.try_lock_owned() {
            Ok(guard) => DeletionGuard(guard),
            Err(_) => {
                // Unfortunately if lock fails arc is consumed.
                return Err(DeleteTimelineError::AlreadyInProgress(Arc::clone(
                    &timeline.delete_progress,
                )));
            }
        };

        timeline.set_state(TimelineState::Stopping);

        Ok((Arc::clone(timeline), delete_lock_guard))
    }

    fn schedule_background(
        guard: DeletionGuard,
        conf: &'static PageServerConf,
        tenant: Arc<Tenant>,
        timeline: Arc<Timeline>,
    ) {
        let tenant_shard_id = timeline.tenant_shard_id;
        let timeline_id = timeline.timeline_id;

        task_mgr::spawn(
            task_mgr::BACKGROUND_RUNTIME.handle(),
            TaskKind::TimelineDeletionWorker,
            Some(tenant_shard_id),
            Some(timeline_id),
            "timeline_delete",
            false,
            async move {
                if let Err(err) = Self::background(guard, conf, &tenant, &timeline).await {
                    error!("Error: {err:#}");
                    timeline.set_broken(format!("{err:#}"))
                };
                Ok(())
            }
            .instrument(tracing::info_span!(parent: None, "delete_timeline", tenant_id=%tenant_shard_id.tenant_id, shard_id=%tenant_shard_id.shard_slug(),timeline_id=%timeline_id)),
        );
    }

    async fn background(
        mut guard: DeletionGuard,
        conf: &PageServerConf,
        tenant: &Tenant,
        timeline: &Timeline,
    ) -> Result<(), DeleteTimelineError> {
        delete_local_timeline_directory(conf, tenant.tenant_shard_id, timeline).await?;

        delete_remote_layers_and_index(timeline).await?;

        pausable_failpoint!("in_progress_delete");

        remove_timeline_from_tenant(tenant, timeline.timeline_id, &guard).await?;

        *guard = Self::Finished;

        Ok(())
    }

    pub(crate) fn is_finished(&self) -> bool {
        matches!(self, Self::Finished)
    }
}

struct DeletionGuard(OwnedMutexGuard<DeleteTimelineFlow>);

impl Deref for DeletionGuard {
    type Target = DeleteTimelineFlow;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DeletionGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
