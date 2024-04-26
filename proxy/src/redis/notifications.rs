use std::{convert::Infallible, sync::Arc};

use futures::StreamExt;
use pq_proto::CancelKeyData;
use redis::aio::PubSub;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    cache::project_info::ProjectInfoCache,
    cancellation::{CancelMap, CancellationHandler, NotificationsCancellationHandler},
    intern::{ProjectIdInt, RoleNameInt},
};

const CPLANE_CHANNEL_NAME: &str = "neondb-proxy-ws-updates";
pub(crate) const PROXY_CHANNEL_NAME: &str = "neondb-proxy-to-proxy-updates";
const RECONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const INVALIDATION_LAG: std::time::Duration = std::time::Duration::from_secs(20);

struct RedisConsumerClient {
    client: redis::Client,
}

impl RedisConsumerClient {
    pub fn new(url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self { client })
    }
    async fn try_connect(&self) -> anyhow::Result<PubSub> {
        let mut conn = self.client.get_async_connection().await?.into_pubsub();
        tracing::info!("subscribing to a channel `{CPLANE_CHANNEL_NAME}`");
        conn.subscribe(CPLANE_CHANNEL_NAME).await?;
        tracing::info!("subscribing to a channel `{PROXY_CHANNEL_NAME}`");
        conn.subscribe(PROXY_CHANNEL_NAME).await?;
        Ok(conn)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "topic", content = "data")]
pub(crate) enum Notification {
    #[serde(
        rename = "/allowed_ips_updated",
        deserialize_with = "deserialize_json_string"
    )]
    AllowedIpsUpdate {
        allowed_ips_update: AllowedIpsUpdate,
    },
    #[serde(
        rename = "/password_updated",
        deserialize_with = "deserialize_json_string"
    )]
    PasswordUpdate { password_update: PasswordUpdate },
    #[serde(rename = "/cancel_session")]
    Cancel(CancelSession),
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct AllowedIpsUpdate {
    project_id: ProjectIdInt,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct PasswordUpdate {
    project_id: ProjectIdInt,
    role_name: RoleNameInt,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct CancelSession {
    pub region_id: Option<String>,
    pub cancel_key_data: CancelKeyData,
    pub session_id: Uuid,
}

fn deserialize_json_string<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: for<'de2> serde::Deserialize<'de2>,
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(<D::Error as serde::de::Error>::custom)
}

struct MessageHandler<
    C: ProjectInfoCache + Send + Sync + 'static,
    H: NotificationsCancellationHandler + Send + Sync + 'static,
> {
    cache: Arc<C>,
    cancellation_handler: Arc<H>,
    region_id: String,
}

impl<
        C: ProjectInfoCache + Send + Sync + 'static,
        H: NotificationsCancellationHandler + Send + Sync + 'static,
    > MessageHandler<C, H>
{
    pub fn new(cache: Arc<C>, cancellation_handler: Arc<H>, region_id: String) -> Self {
        Self {
            cache,
            cancellation_handler,
            region_id,
        }
    }
    pub fn disable_ttl(&self) {
        self.cache.disable_ttl();
    }
    pub fn enable_ttl(&self) {
        self.cache.enable_ttl();
    }
    #[tracing::instrument(skip(self, msg), fields(session_id = tracing::field::Empty))]
    async fn handle_message(&self, msg: redis::Msg) -> anyhow::Result<()> {
        use Notification::*;
        let payload: String = msg.get_payload()?;
        tracing::debug!(?payload, "received a message payload");

        let msg: Notification = match serde_json::from_str(&payload) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!("broken message: {e}");
                return Ok(());
            }
        };
        tracing::debug!(?msg, "received a message");
        match msg {
            Cancel(cancel_session) => {
                tracing::Span::current().record(
                    "session_id",
                    &tracing::field::display(cancel_session.session_id),
                );
                if let Some(cancel_region) = cancel_session.region_id {
                    // If the message is not for this region, ignore it.
                    if cancel_region != self.region_id {
                        return Ok(());
                    }
                }
                // This instance of cancellation_handler doesn't have a RedisPublisherClient so it can't publish the message.
                match self
                    .cancellation_handler
                    .cancel_session_no_publish(cancel_session.cancel_key_data)
                    .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::error!("failed to cancel session: {e}");
                    }
                }
            }
            _ => {
                invalidate_cache(self.cache.clone(), msg.clone());
                // It might happen that the invalid entry is on the way to be cached.
                // To make sure that the entry is invalidated, let's repeat the invalidation in INVALIDATION_LAG seconds.
                // TODO: include the version (or the timestamp) in the message and invalidate only if the entry is cached before the message.
                let cache = self.cache.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(INVALIDATION_LAG).await;
                    invalidate_cache(cache, msg);
                });
            }
        }

        Ok(())
    }
}

fn invalidate_cache<C: ProjectInfoCache>(cache: Arc<C>, msg: Notification) {
    use Notification::*;
    match msg {
        AllowedIpsUpdate { allowed_ips_update } => {
            cache.invalidate_allowed_ips_for_project(allowed_ips_update.project_id)
        }
        PasswordUpdate { password_update } => cache.invalidate_role_secret_for_project(
            password_update.project_id,
            password_update.role_name,
        ),
        Cancel(_) => unreachable!("cancel message should be handled separately"),
    }
}

/// Handle console's invalidation messages.
#[tracing::instrument(name = "console_notifications", skip_all)]
pub async fn task_main<C>(
    url: String,
    cache: Arc<C>,
    cancel_map: CancelMap,
    region_id: String,
) -> anyhow::Result<Infallible>
where
    C: ProjectInfoCache + Send + Sync + 'static,
{
    cache.enable_ttl();
    let handler = MessageHandler::new(
        cache,
        Arc::new(CancellationHandler::new(cancel_map, None)),
        region_id,
    );

    loop {
        let redis = RedisConsumerClient::new(&url)?;
        let conn = match redis.try_connect().await {
            Ok(conn) => {
                handler.disable_ttl();
                conn
            }
            Err(e) => {
                tracing::error!(
                    "failed to connect to redis: {e}, will try to reconnect in {RECONNECT_TIMEOUT:#?}"
                );
                tokio::time::sleep(RECONNECT_TIMEOUT).await;
                continue;
            }
        };
        let mut stream = conn.into_on_message();
        while let Some(msg) = stream.next().await {
            match handler.handle_message(msg).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!("failed to handle message: {e}, will try to reconnect");
                    break;
                }
            }
        }
        handler.enable_ttl();
    }
}

#[cfg(test)]
mod tests {
    use crate::{ProjectId, RoleName};

    use super::*;
    use serde_json::json;

    #[test]
    fn parse_allowed_ips() -> anyhow::Result<()> {
        let project_id: ProjectId = "new_project".into();
        let data = format!("{{\"project_id\": \"{project_id}\"}}");
        let text = json!({
            "type": "message",
            "topic": "/allowed_ips_updated",
            "data": data,
            "extre_fields": "something"
        })
        .to_string();

        let result: Notification = serde_json::from_str(&text)?;
        assert_eq!(
            result,
            Notification::AllowedIpsUpdate {
                allowed_ips_update: AllowedIpsUpdate {
                    project_id: (&project_id).into()
                }
            }
        );

        Ok(())
    }

    #[test]
    fn parse_password_updated() -> anyhow::Result<()> {
        let project_id: ProjectId = "new_project".into();
        let role_name: RoleName = "new_role".into();
        let data = format!("{{\"project_id\": \"{project_id}\", \"role_name\": \"{role_name}\"}}");
        let text = json!({
            "type": "message",
            "topic": "/password_updated",
            "data": data,
            "extre_fields": "something"
        })
        .to_string();

        let result: Notification = serde_json::from_str(&text)?;
        assert_eq!(
            result,
            Notification::PasswordUpdate {
                password_update: PasswordUpdate {
                    project_id: (&project_id).into(),
                    role_name: (&role_name).into(),
                }
            }
        );

        Ok(())
    }
    #[test]
    fn parse_cancel_session() -> anyhow::Result<()> {
        let cancel_key_data = CancelKeyData {
            backend_pid: 42,
            cancel_key: 41,
        };
        let uuid = uuid::Uuid::new_v4();
        let msg = Notification::Cancel(CancelSession {
            cancel_key_data,
            region_id: None,
            session_id: uuid,
        });
        let text = serde_json::to_string(&msg)?;
        let result: Notification = serde_json::from_str(&text)?;
        assert_eq!(msg, result);

        let msg = Notification::Cancel(CancelSession {
            cancel_key_data,
            region_id: Some("region".to_string()),
            session_id: uuid,
        });
        let text = serde_json::to_string(&msg)?;
        let result: Notification = serde_json::from_str(&text)?;
        assert_eq!(msg, result,);

        Ok(())
    }
}
