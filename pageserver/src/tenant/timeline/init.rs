use crate::{
    is_temporary,
    tenant::{
        ephemeral_file::is_ephemeral_file,
        remote_timeline_client::{
            self,
            index::{IndexPart, LayerFileMetadata},
        },
        storage_layer::LayerFileName,
        Generation,
    },
    METADATA_FILE_NAME,
};
use anyhow::Context;
use camino::Utf8Path;
use pageserver_api::shard::ShardIndex;
use std::{collections::HashMap, str::FromStr};
use utils::lsn::Lsn;

/// Identified files in the timeline directory.
pub(super) enum Discovered {
    /// The only one we care about
    Layer(LayerFileName, u64),
    /// Old ephmeral files from previous launches, should be removed
    Ephemeral(String),
    /// Old temporary timeline files, unsure what these really are, should be removed
    Temporary(String),
    /// Temporary on-demand download files, should be removed
    TemporaryDownload(String),
    /// "metadata" file we persist locally and include in `index_part.json`
    Metadata,
    /// Backup file from previously future layers
    IgnoredBackup,
    /// Unrecognized, warn about these
    Unknown(String),
}

/// Scans the timeline directory for interesting files.
pub(super) fn scan_timeline_dir(path: &Utf8Path) -> anyhow::Result<Vec<Discovered>> {
    let mut ret = Vec::new();

    for direntry in path.read_dir_utf8()? {
        let direntry = direntry?;
        let file_name = direntry.file_name().to_string();

        let discovered = match LayerFileName::from_str(&file_name) {
            Ok(file_name) => {
                let file_size = direntry.metadata()?.len();
                Discovered::Layer(file_name, file_size)
            }
            Err(_) => {
                if file_name == METADATA_FILE_NAME {
                    Discovered::Metadata
                } else if file_name.ends_with(".old") {
                    // ignore these
                    Discovered::IgnoredBackup
                } else if remote_timeline_client::is_temp_download_file(direntry.path()) {
                    Discovered::TemporaryDownload(file_name)
                } else if is_ephemeral_file(&file_name) {
                    Discovered::Ephemeral(file_name)
                } else if is_temporary(direntry.path()) {
                    Discovered::Temporary(file_name)
                } else {
                    Discovered::Unknown(file_name)
                }
            }
        };

        ret.push(discovered);
    }

    Ok(ret)
}

/// Decision on what to do with a layer file after considering its local and remote metadata.
#[derive(Clone, Debug)]
pub(super) enum Decision {
    /// The layer is not present locally.
    Evicted(LayerFileMetadata),
    /// The layer is present locally, but local metadata does not match remote; we must
    /// delete it and treat it as evicted.
    UseRemote {
        local: LayerFileMetadata,
        remote: LayerFileMetadata,
    },
    /// The layer is present locally, and metadata matches.
    UseLocal(LayerFileMetadata),
}

/// A layer needs to be left out of the layer map.
#[derive(Debug)]
pub(super) enum DismissedLayer {
    /// The related layer is is in future compared to disk_consistent_lsn, it must not be loaded.
    Future {
        /// The local metadata. `None` if the layer is only known through [`IndexPart`].
        local: Option<LayerFileMetadata>,
    },
    /// The layer only exists locally.
    ///
    /// In order to make crash safe updates to layer map, we must dismiss layers which are only
    /// found locally or not yet included in the remote `index_part.json`.
    LocalOnly(LayerFileMetadata),
}

/// Merges local discoveries and remote [`IndexPart`] to a collection of decisions.
pub(super) fn reconcile(
    discovered: Vec<(LayerFileName, u64)>,
    index_part: Option<&IndexPart>,
    disk_consistent_lsn: Lsn,
    generation: Generation,
    shard: ShardIndex,
) -> Vec<(LayerFileName, Result<Decision, DismissedLayer>)> {
    use Decision::*;

    // name => (local, remote)
    type Collected = HashMap<LayerFileName, (Option<LayerFileMetadata>, Option<LayerFileMetadata>)>;

    let mut discovered = discovered
        .into_iter()
        .map(|(name, file_size)| {
            (
                name,
                // The generation and shard here will be corrected to match IndexPart in the merge below, unless
                // it is not in IndexPart, in which case using our current generation makes sense
                // because it will be uploaded in this generation.
                (
                    Some(LayerFileMetadata::new(file_size, generation, shard)),
                    None,
                ),
            )
        })
        .collect::<Collected>();

    // merge any index_part information, when available
    index_part
        .as_ref()
        .map(|ip| ip.layer_metadata.iter())
        .into_iter()
        .flatten()
        .map(|(name, metadata)| (name, LayerFileMetadata::from(metadata)))
        .for_each(|(name, metadata)| {
            if let Some(existing) = discovered.get_mut(name) {
                existing.1 = Some(metadata);
            } else {
                discovered.insert(name.to_owned(), (None, Some(metadata)));
            }
        });

    discovered
        .into_iter()
        .map(|(name, (local, remote))| {
            let decision = if name.is_in_future(disk_consistent_lsn) {
                Err(DismissedLayer::Future { local })
            } else {
                match (local, remote) {
                    (Some(local), Some(remote)) if local != remote => {
                        Ok(UseRemote { local, remote })
                    }
                    (Some(x), Some(_)) => Ok(UseLocal(x)),
                    (None, Some(x)) => Ok(Evicted(x)),
                    (Some(x), None) => Err(DismissedLayer::LocalOnly(x)),
                    (None, None) => {
                        unreachable!("there must not be any non-local non-remote files")
                    }
                }
            };

            (name, decision)
        })
        .collect::<Vec<_>>()
}

pub(super) fn cleanup(path: &Utf8Path, kind: &str) -> anyhow::Result<()> {
    let file_name = path.file_name().expect("must be file path");
    tracing::debug!(kind, ?file_name, "cleaning up");
    std::fs::remove_file(path).with_context(|| format!("failed to remove {kind} at {path}"))
}

pub(super) fn cleanup_local_file_for_remote(
    path: &Utf8Path,
    local: &LayerFileMetadata,
    remote: &LayerFileMetadata,
) -> anyhow::Result<()> {
    let local_size = local.file_size();
    let remote_size = remote.file_size();

    let file_name = path.file_name().expect("must be file path");
    tracing::warn!("removing local file {file_name:?} because it has unexpected length {local_size}; length in remote index is {remote_size}");
    if let Err(err) = crate::tenant::timeline::rename_to_backup(path) {
        assert!(
            path.exists(),
            "we would leave the local_layer without a file if this does not hold: {path}",
        );
        Err(err)
    } else {
        Ok(())
    }
}

pub(super) fn cleanup_future_layer(
    path: &Utf8Path,
    name: &LayerFileName,
    disk_consistent_lsn: Lsn,
) -> anyhow::Result<()> {
    // future image layers are allowed to be produced always for not yet flushed to disk
    // lsns stored in InMemoryLayer.
    let kind = name.kind();
    tracing::info!("found future {kind} layer {name} disk_consistent_lsn is {disk_consistent_lsn}");
    std::fs::remove_file(path)?;
    Ok(())
}

pub(super) fn cleanup_local_only_file(
    path: &Utf8Path,
    name: &LayerFileName,
    local: &LayerFileMetadata,
) -> anyhow::Result<()> {
    let kind = name.kind();
    tracing::info!("found local-only {kind} layer {name}, metadata {local:?}");
    std::fs::remove_file(path)?;
    Ok(())
}
