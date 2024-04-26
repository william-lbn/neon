use super::{ComputeCredentials, ComputeUserInfo};
use crate::{
    auth::{self, backend::ComputeCredentialKeys, AuthFlow},
    compute,
    config::AuthenticationConfig,
    console::AuthSecret,
    context::RequestMonitoring,
    sasl,
    stream::{PqStream, Stream},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{info, warn};

pub(super) async fn authenticate(
    ctx: &mut RequestMonitoring,
    creds: ComputeUserInfo,
    client: &mut PqStream<Stream<impl AsyncRead + AsyncWrite + Unpin>>,
    config: &'static AuthenticationConfig,
    secret: AuthSecret,
) -> auth::Result<ComputeCredentials> {
    let flow = AuthFlow::new(client);
    let scram_keys = match secret {
        #[cfg(any(test, feature = "testing"))]
        AuthSecret::Md5(_) => {
            info!("auth endpoint chooses MD5");
            return Err(auth::AuthError::bad_auth_method("MD5"));
        }
        AuthSecret::Scram(secret) => {
            info!("auth endpoint chooses SCRAM");
            let scram = auth::Scram(&secret, &mut *ctx);

            let auth_outcome = tokio::time::timeout(
                config.scram_protocol_timeout,
                async {

                    flow.begin(scram).await.map_err(|error| {
                        warn!(?error, "error sending scram acknowledgement");
                        error
                    })?.authenticate().await.map_err(|error| {
                        warn!(?error, "error processing scram messages");
                        error
                    })
                }
            )
            .await
            .map_err(|e| {
                warn!("error processing scram messages error = authentication timed out, execution time exeeded {} seconds", config.scram_protocol_timeout.as_secs());
                auth::AuthError::user_timeout(e)
            })??;

            let client_key = match auth_outcome {
                sasl::Outcome::Success(key) => key,
                sasl::Outcome::Failure(reason) => {
                    info!("auth backend failed with an error: {reason}");
                    return Err(auth::AuthError::auth_failed(&*creds.user));
                }
            };

            compute::ScramKeys {
                client_key: client_key.as_bytes(),
                server_key: secret.server_key.as_bytes(),
            }
        }
    };

    Ok(ComputeCredentials {
        info: creds,
        keys: ComputeCredentialKeys::AuthKeys(tokio_postgres::config::AuthKeys::ScramSha256(
            scram_keys,
        )),
    })
}
