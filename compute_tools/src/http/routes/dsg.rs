//! DSGA管理HTTP只承载PG/Provider事实，不发放VM许可，不借通用路由的认证豁免。

use std::sync::Arc;

use axum::{
    Extension, Router,
    extract::{DefaultBodyLimit, State},
    response::Response,
    routing::{get, post},
};
use compute_api::dsg_auth::DsgHttpClaims;
use compute_api::dsg_client::{DsgCallError, DsgFenceCallError, DsgSqlClient};
use compute_api::dsg_control::{
    DsgIssuerFenceRequest, DsgOperationKey, DsgOperationQuery, DsgResizeRequest,
};
use compute_api::dsg_state::DsgControlResponse;
use http::{HeaderValue, StatusCode, header::CACHE_CONTROL};
use jsonwebtoken::jwk::JwkSet;
use serde::Serialize;
use tower_http::auth::AsyncRequireAuthorizationLayer;

use crate::compute::ComputeNode;
use crate::http::{JsonResponse, extract::Json, middleware::dsg_authorize::DsgAuthorize};

/// 只合并到external。单独layer保证即使Hadron也必须专用签名/短期/身份认证。
pub(in crate::http) fn router(compute_id: String, jwks: JwkSet) -> Router<Arc<ComputeNode>> {
    Router::new()
        .route("/dsg/v1/capabilities", get(capabilities))
        .route("/dsg/v1/initialization", get(initialization))
        .route("/dsg/v1/memory", get(memory))
        .route("/dsg/v1/resize", post(resize))
        .route("/dsg/v1/resize/status", post(status))
        .route("/dsg/v1/resize/cancel", post(cancel))
        .route("/dsg/v1/issuer/fence", post(fence))
        // 原始有类型JSON直接反序列化，不经Value/map去重；不得放入URL query或日志。
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(AsyncRequireAuthorizationLayer::new(DsgAuthorize::new(
            compute_id, jwks,
        )))
        // 能力/观察结果与boot和当前事实绑定，禁止中间缓存重放旧状态；认证失败亦不缓存。
        .layer(axum::middleware::map_response(no_store))
}

async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn client<'a>(
    compute: &'a ComputeNode,
    claims: &DsgHttpClaims,
) -> Result<&'a DsgSqlClient<'static>, Response> {
    let spec = compute
        .dsg_spec()
        .map_err(|_| failure(StatusCode::PRECONDITION_FAILED, "not_available", None))?;
    if claims.vm_uid != spec.vm_uid || claims.compute_instance_id != spec.compute_instance_id {
        return Err(failure(StatusCode::FORBIDDEN, "wrong_instance", None));
    }
    compute.dsg_management_client().await.map_err(|_| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "management_unavailable",
            None,
        )
    })
}

async fn capabilities(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        // 能力虽在bind固定，HTTP发现不能只返回缓存冒充存活实例。检查真实观察通道，
        // 断连/换boot即失败关闭；不通过自动重新初始化丢弃原执行栅栏。
        Ok(client) => match client.memory_status().await {
            Ok(_) => JsonResponse::success(StatusCode::OK, client.capabilities()),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "observation_unavailable",
                None,
            ),
        },
        Err(error) => error,
    }
}

async fn initialization(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    let spec = match compute.dsg_spec() {
        Ok(spec) => spec,
        Err(_) => return failure(StatusCode::PRECONDITION_FAILED, "not_available", None),
    };
    if claims.vm_uid != spec.vm_uid || claims.compute_instance_id != spec.compute_instance_id {
        return failure(StatusCode::FORBIDDEN, "wrong_instance", None);
    }
    match compute.dsg_initialization().await {
        Ok(receipt) => JsonResponse::success(StatusCode::OK, receipt),
        Err(_) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "initialization_unconfirmed",
            None,
        ),
    }
}

async fn memory(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        Ok(client) => match client.memory_status().await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "observation_unavailable",
                None,
            ),
        },
        Err(error) => error,
    }
}

async fn resize(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgResizeRequest>,
) -> Response {
    if request.validate().is_err() {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some(&request.key),
        );
    }
    if claims.authorize_key(&request.key, true).is_err() {
        return failure(
            StatusCode::FORBIDDEN,
            "wrong_instance_or_epoch",
            Some(&request.key),
        );
    }
    match client(&compute, &claims).await {
        Ok(client) => {
            if compute.verify_dsg_running().is_err() {
                return failure(
                    StatusCode::PRECONDITION_FAILED,
                    "not_available",
                    Some(&request.key),
                );
            }
            control_result(client.request_resize(&request).await, &request.key)
        }
        Err(error) => error,
    }
}

async fn status(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(query): Json<DsgOperationQuery>,
) -> Response {
    query_operation(&compute, &claims, query, false).await
}

async fn cancel(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(query): Json<DsgOperationQuery>,
) -> Response {
    query_operation(&compute, &claims, query, true).await
}

async fn fence(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgIssuerFenceRequest>,
) -> Response {
    if request.validate().is_err() {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    if claims.authorize_fence(&request).is_err() {
        return failure(StatusCode::FORBIDDEN, "wrong_instance_or_epoch", None);
    }
    match client(&compute, &claims).await {
        Ok(client) => match client.fence_issuer(&request).await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(DsgFenceCallError::NotSent(_)) => failure(StatusCode::CONFLICT, "not_sent", None),
            Err(DsgFenceCallError::Unconfirmed { .. }) => {
                failure(StatusCode::SERVICE_UNAVAILABLE, "unconfirmed", None)
            }
        },
        Err(error) => error,
    }
}

async fn query_operation(
    compute: &ComputeNode,
    claims: &DsgHttpClaims,
    query: DsgOperationQuery,
    cancel: bool,
) -> Response {
    if query.validate().is_err() {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", Some(&query.key));
    }
    if claims.authorize_key(&query.key, false).is_err() {
        return failure(
            StatusCode::FORBIDDEN,
            "wrong_instance_or_epoch",
            Some(&query.key),
        );
    }
    match client(compute, claims).await {
        Ok(client) => control_result(
            if cancel {
                client.cancel_resize(&query).await
            } else {
                client.resize_status(&query).await
            },
            &query.key,
        ),
        Err(error) => error,
    }
}

fn control_result(
    result: Result<DsgControlResponse, DsgCallError>,
    key: &DsgOperationKey,
) -> Response {
    match result {
        Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
        Err(DsgCallError::NotSent(_)) => failure(StatusCode::CONFLICT, "not_sent", Some(key)),
        // 不把SQL ERROR/网络故障写成DENIED，不回显PG详细错误/内部凭据。调用方必须
        // 保留同key完整意图并独立查询/重放；HTTP 503也不证明PG未执行。
        Err(DsgCallError::Unconfirmed { .. }) => {
            failure(StatusCode::SERVICE_UNAVAILABLE, "unconfirmed", Some(key))
        }
    }
}

fn failure(code: StatusCode, state: &'static str, key: Option<&DsgOperationKey>) -> Response {
    #[derive(Serialize)]
    struct Failure<'a> {
        protocol_version: u32,
        vm_shrink_permit: bool,
        state: &'static str,
        key: Option<&'a DsgOperationKey>,
    }
    // 与通用错误不同，此处返回机器可判定的发送状态，不泄露内部SQL错误详情。
    JsonResponse::create_response(
        code,
        Failure {
            protocol_version: 1,
            vm_shrink_permit: false,
            state,
            key,
        },
    )
}
