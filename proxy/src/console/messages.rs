use serde::Deserialize;
use std::fmt;

use crate::auth::IpPattern;

use crate::{BranchId, EndpointId, ProjectId};

/// Generic error response with human-readable description.
/// Note that we can't always present it to user as is.
#[derive(Debug, Deserialize)]
pub struct ConsoleError {
    pub error: Box<str>,
}

/// Response which holds client's auth secret, e.g. [`crate::scram::ServerSecret`].
/// Returned by the `/proxy_get_role_secret` API method.
#[derive(Deserialize)]
pub struct GetRoleSecret {
    pub role_secret: Box<str>,
    pub allowed_ips: Option<Vec<IpPattern>>,
    pub project_id: Option<ProjectId>,
}

// Manually implement debug to omit sensitive info.
impl fmt::Debug for GetRoleSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GetRoleSecret").finish_non_exhaustive()
    }
}

/// Response which holds compute node's `host:port` pair.
/// Returned by the `/proxy_wake_compute` API method.
#[derive(Debug, Deserialize)]
pub struct WakeCompute {
    pub address: Box<str>,
    pub aux: MetricsAuxInfo,
}

/// Async response which concludes the link auth flow.
/// Also known as `kickResponse` in the console.
#[derive(Debug, Deserialize)]
pub struct KickSession<'a> {
    /// Session ID is assigned by the proxy.
    pub session_id: &'a str,

    /// Compute node connection params.
    #[serde(deserialize_with = "KickSession::parse_db_info")]
    pub result: DatabaseInfo,
}

impl KickSession<'_> {
    fn parse_db_info<'de, D>(des: D) -> Result<DatabaseInfo, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum Wrapper {
            // Currently, console only reports `Success`.
            // `Failure(String)` used to be here... RIP.
            Success(DatabaseInfo),
        }

        Wrapper::deserialize(des).map(|x| match x {
            Wrapper::Success(info) => info,
        })
    }
}

/// Compute node connection params.
#[derive(Deserialize)]
pub struct DatabaseInfo {
    pub host: Box<str>,
    pub port: u16,
    pub dbname: Box<str>,
    pub user: Box<str>,
    /// Console always provides a password, but it might
    /// be inconvenient for debug with local PG instance.
    pub password: Option<Box<str>>,
    pub aux: MetricsAuxInfo,
}

// Manually implement debug to omit sensitive info.
impl fmt::Debug for DatabaseInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DatabaseInfo")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("dbname", &self.dbname)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

/// Various labels for prometheus metrics.
/// Also known as `ProxyMetricsAuxInfo` in the console.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct MetricsAuxInfo {
    pub endpoint_id: EndpointId,
    pub project_id: ProjectId,
    pub branch_id: BranchId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dummy_aux() -> serde_json::Value {
        json!({
            "endpoint_id": "endpoint",
            "project_id": "project",
            "branch_id": "branch",
        })
    }

    #[test]
    fn parse_kick_session() -> anyhow::Result<()> {
        // This is what the console's kickResponse looks like.
        let json = json!({
            "session_id": "deadbeef",
            "result": {
                "Success": {
                    "host": "localhost",
                    "port": 5432,
                    "dbname": "postgres",
                    "user": "john_doe",
                    "password": "password",
                    "aux": dummy_aux(),
                }
            }
        });
        let _: KickSession = serde_json::from_str(&json.to_string())?;

        Ok(())
    }

    #[test]
    fn parse_db_info() -> anyhow::Result<()> {
        // with password
        let _: DatabaseInfo = serde_json::from_value(json!({
            "host": "localhost",
            "port": 5432,
            "dbname": "postgres",
            "user": "john_doe",
            "password": "password",
            "aux": dummy_aux(),
        }))?;

        // without password
        let _: DatabaseInfo = serde_json::from_value(json!({
            "host": "localhost",
            "port": 5432,
            "dbname": "postgres",
            "user": "john_doe",
            "aux": dummy_aux(),
        }))?;

        // new field (forward compatibility)
        let _: DatabaseInfo = serde_json::from_value(json!({
            "host": "localhost",
            "port": 5432,
            "dbname": "postgres",
            "user": "john_doe",
            "project": "hello_world",
            "N.E.W": "forward compatibility check",
            "aux": dummy_aux(),
        }))?;

        Ok(())
    }

    #[test]
    fn parse_wake_compute() -> anyhow::Result<()> {
        let json = json!({
            "address": "0.0.0.0",
            "aux": dummy_aux(),
        });
        let _: WakeCompute = serde_json::from_str(&json.to_string())?;
        Ok(())
    }

    #[test]
    fn parse_get_role_secret() -> anyhow::Result<()> {
        // Empty `allowed_ips` field.
        let json = json!({
            "role_secret": "secret",
        });
        let _: GetRoleSecret = serde_json::from_str(&json.to_string())?;
        let json = json!({
            "role_secret": "secret",
            "allowed_ips": ["8.8.8.8"],
        });
        let _: GetRoleSecret = serde_json::from_str(&json.to_string())?;
        let json = json!({
            "role_secret": "secret",
            "allowed_ips": ["8.8.8.8"],
            "project_id": "project",
        });
        let _: GetRoleSecret = serde_json::from_str(&json.to_string())?;

        Ok(())
    }
}
