use super::{
    ComputeCredentialKeys, ComputeCredentials, ComputeUserInfo, ComputeUserInfoNoEndpoint,
};
use crate::{
    auth::{self, AuthFlow},
    console::AuthSecret,
    context::RequestMonitoring,
    sasl,
    stream::{self, Stream},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{info, warn};

/// Compared to [SCRAM](crate::scram), cleartext password auth saves
/// one round trip and *expensive* computations (>= 4096 HMAC iterations).
/// These properties are benefical for serverless JS workers, so we
/// use this mechanism for websocket connections.
pub async fn authenticate_cleartext(
    ctx: &mut RequestMonitoring,
    info: ComputeUserInfo,
    client: &mut stream::PqStream<Stream<impl AsyncRead + AsyncWrite + Unpin>>,
    secret: AuthSecret,
) -> auth::Result<ComputeCredentials> {
    warn!("cleartext auth flow override is enabled, proceeding");
    ctx.set_auth_method(crate::context::AuthMethod::Cleartext);

    // pause the timer while we communicate with the client
    let _paused = ctx.latency_timer.pause();

    let auth_outcome = AuthFlow::new(client)
        .begin(auth::CleartextPassword(secret))
        .await?
        .authenticate()
        .await?;

    let keys = match auth_outcome {
        sasl::Outcome::Success(key) => key,
        sasl::Outcome::Failure(reason) => {
            info!("auth backend failed with an error: {reason}");
            return Err(auth::AuthError::auth_failed(&*info.user));
        }
    };

    Ok(ComputeCredentials { info, keys })
}

/// Workaround for clients which don't provide an endpoint (project) name.
/// Similar to [`authenticate_cleartext`], but there's a specific password format,
/// and passwords are not yet validated (we don't know how to validate them!)
pub async fn password_hack_no_authentication(
    ctx: &mut RequestMonitoring,
    info: ComputeUserInfoNoEndpoint,
    client: &mut stream::PqStream<Stream<impl AsyncRead + AsyncWrite + Unpin>>,
) -> auth::Result<ComputeCredentials> {
    warn!("project not specified, resorting to the password hack auth flow");
    ctx.set_auth_method(crate::context::AuthMethod::Cleartext);

    // pause the timer while we communicate with the client
    let _paused = ctx.latency_timer.pause();

    let payload = AuthFlow::new(client)
        .begin(auth::PasswordHack)
        .await?
        .get_password()
        .await?;

    info!(project = &*payload.endpoint, "received missing parameter");

    // Report tentative success; compute node will check the password anyway.
    Ok(ComputeCredentials {
        info: ComputeUserInfo {
            user: info.user,
            options: info.options,
            endpoint: payload.endpoint,
        },
        keys: ComputeCredentialKeys::Password(payload.password),
    })
}
