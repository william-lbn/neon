//! Main authentication flow.

use super::{backend::ComputeCredentialKeys, AuthErrorImpl, PasswordHackPayload};
use crate::{
    config::TlsServerEndPoint,
    console::AuthSecret,
    context::RequestMonitoring,
    sasl, scram,
    stream::{PqStream, Stream},
};
use postgres_protocol::authentication::sasl::{SCRAM_SHA_256, SCRAM_SHA_256_PLUS};
use pq_proto::{BeAuthenticationSaslMessage, BeMessage, BeMessage as Be};
use std::io;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::info;

/// Every authentication selector is supposed to implement this trait.
pub trait AuthMethod {
    /// Any authentication selector should provide initial backend message
    /// containing auth method name and parameters, e.g. md5 salt.
    fn first_message(&self, channel_binding: bool) -> BeMessage<'_>;
}

/// Initial state of [`AuthFlow`].
pub struct Begin;

/// Use [SCRAM](crate::scram)-based auth in [`AuthFlow`].
pub struct Scram<'a>(pub &'a scram::ServerSecret, pub &'a mut RequestMonitoring);

impl AuthMethod for Scram<'_> {
    #[inline(always)]
    fn first_message(&self, channel_binding: bool) -> BeMessage<'_> {
        if channel_binding {
            Be::AuthenticationSasl(BeAuthenticationSaslMessage::Methods(scram::METHODS))
        } else {
            Be::AuthenticationSasl(BeAuthenticationSaslMessage::Methods(
                scram::METHODS_WITHOUT_PLUS,
            ))
        }
    }
}

/// Use an ad hoc auth flow (for clients which don't support SNI) proposed in
/// <https://github.com/neondatabase/cloud/issues/1620#issuecomment-1165332290>.
pub struct PasswordHack;

impl AuthMethod for PasswordHack {
    #[inline(always)]
    fn first_message(&self, _channel_binding: bool) -> BeMessage<'_> {
        Be::AuthenticationCleartextPassword
    }
}

/// Use clear-text password auth called `password` in docs
/// <https://www.postgresql.org/docs/current/auth-password.html>
pub struct CleartextPassword(pub AuthSecret);

impl AuthMethod for CleartextPassword {
    #[inline(always)]
    fn first_message(&self, _channel_binding: bool) -> BeMessage<'_> {
        Be::AuthenticationCleartextPassword
    }
}

/// This wrapper for [`PqStream`] performs client authentication.
#[must_use]
pub struct AuthFlow<'a, S, State> {
    /// The underlying stream which implements libpq's protocol.
    stream: &'a mut PqStream<Stream<S>>,
    /// State might contain ancillary data (see [`Self::begin`]).
    state: State,
    tls_server_end_point: TlsServerEndPoint,
}

/// Initial state of the stream wrapper.
impl<'a, S: AsyncRead + AsyncWrite + Unpin> AuthFlow<'a, S, Begin> {
    /// Create a new wrapper for client authentication.
    pub fn new(stream: &'a mut PqStream<Stream<S>>) -> Self {
        let tls_server_end_point = stream.get_ref().tls_server_end_point();

        Self {
            stream,
            state: Begin,
            tls_server_end_point,
        }
    }

    /// Move to the next step by sending auth method's name & params to client.
    pub async fn begin<M: AuthMethod>(self, method: M) -> io::Result<AuthFlow<'a, S, M>> {
        self.stream
            .write_message(&method.first_message(self.tls_server_end_point.supported()))
            .await?;

        Ok(AuthFlow {
            stream: self.stream,
            state: method,
            tls_server_end_point: self.tls_server_end_point,
        })
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AuthFlow<'_, S, PasswordHack> {
    /// Perform user authentication. Raise an error in case authentication failed.
    pub async fn get_password(self) -> super::Result<PasswordHackPayload> {
        let msg = self.stream.read_password_message().await?;
        let password = msg
            .strip_suffix(&[0])
            .ok_or(AuthErrorImpl::MalformedPassword("missing terminator"))?;

        let payload = PasswordHackPayload::parse(password)
            // If we ended up here and the payload is malformed, it means that
            // the user neither enabled SNI nor resorted to any other method
            // for passing the project name we rely on. We should show them
            // the most helpful error message and point to the documentation.
            .ok_or(AuthErrorImpl::MissingEndpointName)?;

        Ok(payload)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AuthFlow<'_, S, CleartextPassword> {
    /// Perform user authentication. Raise an error in case authentication failed.
    pub async fn authenticate(self) -> super::Result<sasl::Outcome<ComputeCredentialKeys>> {
        let msg = self.stream.read_password_message().await?;
        let password = msg
            .strip_suffix(&[0])
            .ok_or(AuthErrorImpl::MalformedPassword("missing terminator"))?;

        let outcome = validate_password_and_exchange(password, self.state.0)?;

        if let sasl::Outcome::Success(_) = &outcome {
            self.stream.write_message_noflush(&Be::AuthenticationOk)?;
        }

        Ok(outcome)
    }
}

/// Stream wrapper for handling [SCRAM](crate::scram) auth.
impl<S: AsyncRead + AsyncWrite + Unpin> AuthFlow<'_, S, Scram<'_>> {
    /// Perform user authentication. Raise an error in case authentication failed.
    pub async fn authenticate(self) -> super::Result<sasl::Outcome<scram::ScramKey>> {
        let Scram(secret, ctx) = self.state;

        // pause the timer while we communicate with the client
        let _paused = ctx.latency_timer.pause();

        // Initial client message contains the chosen auth method's name.
        let msg = self.stream.read_password_message().await?;
        let sasl = sasl::FirstMessage::parse(&msg)
            .ok_or(AuthErrorImpl::MalformedPassword("bad sasl message"))?;

        // Currently, the only supported SASL method is SCRAM.
        if !scram::METHODS.contains(&sasl.method) {
            return Err(super::AuthError::bad_auth_method(sasl.method));
        }

        match sasl.method {
            SCRAM_SHA_256 => ctx.auth_method = Some(crate::context::AuthMethod::ScramSha256),
            SCRAM_SHA_256_PLUS => {
                ctx.auth_method = Some(crate::context::AuthMethod::ScramSha256Plus)
            }
            _ => {}
        }
        info!("client chooses {}", sasl.method);

        let outcome = sasl::SaslStream::new(self.stream, sasl.message)
            .authenticate(scram::Exchange::new(
                secret,
                rand::random,
                self.tls_server_end_point,
            ))
            .await?;

        if let sasl::Outcome::Success(_) = &outcome {
            self.stream.write_message_noflush(&Be::AuthenticationOk)?;
        }

        Ok(outcome)
    }
}

pub(crate) fn validate_password_and_exchange(
    password: &[u8],
    secret: AuthSecret,
) -> super::Result<sasl::Outcome<ComputeCredentialKeys>> {
    match secret {
        #[cfg(any(test, feature = "testing"))]
        AuthSecret::Md5(_) => {
            // test only
            Ok(sasl::Outcome::Success(ComputeCredentialKeys::Password(
                password.to_owned(),
            )))
        }
        // perform scram authentication as both client and server to validate the keys
        AuthSecret::Scram(scram_secret) => {
            use postgres_protocol::authentication::sasl::{ChannelBinding, ScramSha256};
            let sasl_client = ScramSha256::new(password, ChannelBinding::unsupported());
            let outcome = crate::scram::exchange(
                &scram_secret,
                sasl_client,
                crate::config::TlsServerEndPoint::Undefined,
            )?;

            let client_key = match outcome {
                sasl::Outcome::Success(client_key) => client_key,
                sasl::Outcome::Failure(reason) => return Ok(sasl::Outcome::Failure(reason)),
            };

            let keys = crate::compute::ScramKeys {
                client_key: client_key.as_bytes(),
                server_key: scram_secret.server_key.as_bytes(),
            };

            Ok(sasl::Outcome::Success(ComputeCredentialKeys::AuthKeys(
                tokio_postgres::config::AuthKeys::ScramSha256(keys),
            )))
        }
    }
}
