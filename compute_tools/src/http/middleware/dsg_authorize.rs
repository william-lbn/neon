//! DSGA独立授权层。既有通用路由的Hadron绕过、可选exp/aud及宽泛Admin不能沿用。

use anyhow::{Result, ensure};
use axum::{RequestExt, body::Body};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use compute_api::dsg_auth::DsgHttpClaims;
use futures::future::BoxFuture;
use http::{Request, Response, StatusCode, header::AUTHORIZATION};
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
};
use tower_http::auth::AsyncAuthorizeRequest;

use crate::http::JsonResponse;

#[derive(Clone)]
pub(in crate::http) struct DsgAuthorize {
    compute_id: String,
    jwks: JwkSet,
}

impl DsgAuthorize {
    pub fn new(compute_id: String, jwks: JwkSet) -> Self {
        Self { compute_id, jwks }
    }

    fn verify(&self, token: &str) -> Result<DsgHttpClaims> {
        ensure!(token.len() <= 8192, "oversized DSGA credential");
        let header = decode_header(token)?;
        let kid = header
            .kid
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| anyhow::anyhow!("DSGA credential requires kid"))?;
        let mut matching = self
            .jwks
            .keys
            .iter()
            .filter(|key| key.common.key_id.as_deref() == Some(kid));
        let key = matching
            .next()
            .ok_or_else(|| anyhow::anyhow!("unknown DSGA signing key"))?;
        ensure!(matching.next().is_none(), "ambiguous DSGA signing key");
        let algorithm = match key.common.key_algorithm {
            Some(KeyAlgorithm::EdDSA) => Algorithm::EdDSA,
            Some(KeyAlgorithm::RS256) => Algorithm::RS256,
            _ => anyhow::bail!("unsupported DSGA signing algorithm"),
        };
        ensure!(header.alg == algorithm, "DSGA signing algorithm mismatch");
        ensure!(
            key.common
                .public_key_use
                .as_ref()
                .is_none_or(|use_| *use_ == PublicKeyUse::Signature),
            "DSGA key is not for signatures"
        );
        ensure!(
            key.common
                .key_operations
                .as_ref()
                .is_none_or(|operations| operations.contains(&KeyOperations::Verify)),
            "DSGA key does not permit verification"
        );
        let mut validation = Validation::new(algorithm);
        validation.leeway = 0;
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.validate_aud = true;
        validation.set_audience(&[compute_api::dsg_auth::DSG_HTTP_AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "nbf", "aud"]);
        let claims =
            decode::<DsgHttpClaims>(token, &DecodingKey::from_jwk(key)?, &validation)?.claims;
        claims.validate_for_compute(&self.compute_id)?;
        Ok(claims)
    }
}

impl AsyncAuthorizeRequest<Body> for DsgAuthorize {
    type RequestBody = Body;
    type ResponseBody = Body;
    type Future = BoxFuture<'static, Result<Request<Body>, Response<Body>>>;

    fn authorize(&mut self, mut request: Request<Body>) -> Self::Future {
        let policy = self.clone();
        Box::pin(async move {
            // 不记录token、签名失败细节、连接凭据；空JWKS和重复Authorization都失败关闭。
            let deny = || JsonResponse::error(StatusCode::UNAUTHORIZED, "invalid DSGA credential");
            if request.headers().get_all(AUTHORIZATION).iter().count() != 1 {
                return Err(deny());
            }
            let TypedHeader(Authorization(bearer)) = request
                .extract_parts::<TypedHeader<Authorization<Bearer>>>()
                .await
                .map_err(|_| deny())?;
            let claims = policy.verify(bearer.token()).map_err(|_| deny())?;
            request.extensions_mut().insert(claims);
            Ok(request)
        })
    }
}
