#![deny(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]
#[cfg(feature = "postgres-client")]
pub mod dsg_client;
pub mod dsg_control;
pub mod dsg_state;
pub mod dynamic_shared_buffers;
pub mod privilege;
pub mod requests;
pub mod responses;
pub mod spec;
