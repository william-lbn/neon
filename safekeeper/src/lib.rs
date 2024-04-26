#![deny(clippy::undocumented_unsafe_blocks)]
use camino::Utf8PathBuf;
use once_cell::sync::Lazy;
use remote_storage::RemoteStorageConfig;
use tokio::runtime::Runtime;

use std::time::Duration;
use storage_broker::Uri;

use utils::{
    auth::SwappableJwtAuth,
    id::{NodeId, TenantId, TenantTimelineId},
};

mod auth;
pub mod broker;
pub mod control_file;
pub mod control_file_upgrade;
pub mod copy_timeline;
pub mod debug_dump;
pub mod handler;
pub mod http;
pub mod json_ctrl;
pub mod metrics;
pub mod patch_control_file;
pub mod pull_timeline;
pub mod receive_wal;
pub mod recovery;
pub mod remove_wal;
pub mod safekeeper;
pub mod send_wal;
pub mod state;
pub mod timeline;
pub mod wal_backup;
pub mod wal_service;
pub mod wal_storage;

mod timelines_global_map;
use std::sync::Arc;
pub use timelines_global_map::GlobalTimelines;
use utils::auth::JwtAuth;

pub mod defaults {
    pub use safekeeper_api::{
        DEFAULT_HTTP_LISTEN_ADDR, DEFAULT_HTTP_LISTEN_PORT, DEFAULT_PG_LISTEN_ADDR,
        DEFAULT_PG_LISTEN_PORT,
    };

    pub const DEFAULT_HEARTBEAT_TIMEOUT: &str = "5000ms";
    pub const DEFAULT_MAX_OFFLOADER_LAG_BYTES: u64 = 128 * (1 << 20);
}

#[derive(Debug, Clone)]
pub struct SafeKeeperConf {
    // Repository directory, relative to current working directory.
    // Normally, the safekeeper changes the current working directory
    // to the repository, and 'workdir' is always '.'. But we don't do
    // that during unit testing, because the current directory is global
    // to the process but different unit tests work on different
    // data directories to avoid clashing with each other.
    pub workdir: Utf8PathBuf,
    pub my_id: NodeId,
    pub listen_pg_addr: String,
    pub listen_pg_addr_tenant_only: Option<String>,
    pub listen_http_addr: String,
    pub advertise_pg_addr: Option<String>,
    pub availability_zone: Option<String>,
    pub no_sync: bool,
    pub broker_endpoint: Uri,
    pub broker_keepalive_interval: Duration,
    pub heartbeat_timeout: Duration,
    pub peer_recovery_enabled: bool,
    pub remote_storage: Option<RemoteStorageConfig>,
    pub max_offloader_lag_bytes: u64,
    pub backup_parallel_jobs: usize,
    pub wal_backup_enabled: bool,
    pub pg_auth: Option<Arc<JwtAuth>>,
    pub pg_tenant_only_auth: Option<Arc<JwtAuth>>,
    pub http_auth: Option<Arc<SwappableJwtAuth>>,
    pub current_thread_runtime: bool,
    pub walsenders_keep_horizon: bool,
}

impl SafeKeeperConf {
    pub fn tenant_dir(&self, tenant_id: &TenantId) -> Utf8PathBuf {
        self.workdir.join(tenant_id.to_string())
    }

    pub fn timeline_dir(&self, ttid: &TenantTimelineId) -> Utf8PathBuf {
        self.tenant_dir(&ttid.tenant_id)
            .join(ttid.timeline_id.to_string())
    }

    pub fn is_wal_backup_enabled(&self) -> bool {
        self.remote_storage.is_some() && self.wal_backup_enabled
    }
}

impl SafeKeeperConf {
    #[cfg(test)]
    fn dummy() -> Self {
        SafeKeeperConf {
            workdir: Utf8PathBuf::from("./"),
            no_sync: false,
            listen_pg_addr: defaults::DEFAULT_PG_LISTEN_ADDR.to_string(),
            listen_pg_addr_tenant_only: None,
            listen_http_addr: defaults::DEFAULT_HTTP_LISTEN_ADDR.to_string(),
            advertise_pg_addr: None,
            availability_zone: None,
            remote_storage: None,
            my_id: NodeId(0),
            broker_endpoint: storage_broker::DEFAULT_ENDPOINT
                .parse()
                .expect("failed to parse default broker endpoint"),
            broker_keepalive_interval: Duration::from_secs(5),
            peer_recovery_enabled: true,
            wal_backup_enabled: true,
            backup_parallel_jobs: 1,
            pg_auth: None,
            pg_tenant_only_auth: None,
            http_auth: None,
            heartbeat_timeout: Duration::new(5, 0),
            max_offloader_lag_bytes: defaults::DEFAULT_MAX_OFFLOADER_LAG_BYTES,
            current_thread_runtime: false,
            walsenders_keep_horizon: false,
        }
    }
}

// Tokio runtimes.
pub static WAL_SERVICE_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("WAL service worker")
        .enable_all()
        .build()
        .expect("Failed to create WAL service runtime")
});

pub static HTTP_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("HTTP worker")
        .enable_all()
        .build()
        .expect("Failed to create WAL service runtime")
});

pub static BROKER_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("broker worker")
        .worker_threads(2) // there are only 2 tasks, having more threads doesn't make sense
        .enable_all()
        .build()
        .expect("Failed to create broker runtime")
});

pub static WAL_REMOVER_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("WAL remover")
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("Failed to create broker runtime")
});

pub static WAL_BACKUP_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("WAL backup worker")
        .enable_all()
        .build()
        .expect("Failed to create WAL backup runtime")
});

pub static METRICS_SHIFTER_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("metric shifter")
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("Failed to create broker runtime")
});
