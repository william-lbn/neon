#![deny(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]
pub mod dsg_admission;
pub mod dsg_auth;
#[cfg(feature = "postgres-client")]
pub mod dsg_client;
pub mod dsg_connection;
pub mod dsg_control;
pub mod dsg_initialization;
pub mod dsg_state;
pub mod dsg_work;
pub mod dynamic_shared_buffers;
pub mod privilege;
pub mod requests;
pub mod responses;
pub mod spec;
