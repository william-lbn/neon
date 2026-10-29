//! DSGA管理HTTP承载PG/Provider及独立Guest资源事实，不发放VM许可，
//! 不借通用路由的认证豁免；root资源动作经固定UDS重新验签和持久fence。

use std::sync::Arc;

use axum::{
    Extension, Router,
    extract::{DefaultBodyLimit, State},
    response::Response,
    routing::{get, post},
};
use compute_api::dsg_admission::{
    DsgAdmissionCloseRequest, DsgAdmissionContinuationRequest, DsgAdmissionRequest,
};
use compute_api::dsg_auth::DsgHttpClaims;
use compute_api::dsg_client::{
    DsgAdmissionCallError, DsgCallError, DsgFenceCallError, DsgSqlClient,
};
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
        .route("/dsg/v1/budget/shared", get(shared_cost))
        .route("/dsg/v1/work/state", get(work_state))
        .route("/dsg/v1/connections/state", get(connection_state))
        .route("/dsg/v1/work/prepared", get(prepared_state))
        .route("/dsg/v1/work/dsm", get(dsm_state))
        .route("/dsg/v1/resource/identity", get(super::dsg_resource::proxy))
        .route("/dsg/v1/resource/process", get(super::dsg_resource::proxy))
        .route("/dsg/v1/resource/state", get(super::dsg_resource::proxy))
        .route("/dsg/v1/resource/fence", post(super::dsg_resource::proxy))
        .route("/dsg/v1/resource/apply", post(super::dsg_resource::proxy))
        .route("/dsg/v1/resource/observe", post(super::dsg_resource::proxy))
        .route(
            "/dsg/v1/resource/replay-original",
            post(super::dsg_resource::proxy),
        )
        // 独立显式恢复仅选择Root已持久的原约束摘要；网关不接受/推导
        // 新限额，Root仍重新验签、核原floor/出生并拒绝任何实际降限。
        .route(
            "/dsg/v1/resource/repair-held",
            post(super::dsg_resource::proxy),
        )
        // 取消仅转发原完整命令及当前owner签名。Root须持久禁止迟到Apply，
        // 网关的NotFound、超时或数值读回都不能构造NoEffect。
        .route(
            "/dsg/v1/resource/cancel-original",
            post(super::dsg_resource::proxy),
        )
        .route("/dsg/v1/resize", post(resize))
        .route("/dsg/v1/resize/status", post(status))
        .route("/dsg/v1/resize/cancel", post(cancel))
        .route("/dsg/v1/issuer/fence", post(fence))
        .route("/dsg/v1/admission", post(admission))
        .route("/dsg/v1/admission/status", post(admission_status))
        .route("/dsg/v1/admission/lease", post(admission_lease))
        .route(
            "/dsg/v1/admission/lease/rearm/status",
            post(admission_rearm_status),
        )
        .route("/dsg/v1/admission/close-original", post(admission_close))
        .route(
            "/dsg/v1/admission/close-original/status",
            post(admission_close_status),
        )
        .route("/dsg/v1/admission/continue-close", post(admission_continue))
        .route(
            "/dsg/v1/admission/continue-close/status",
            post(admission_continue_status),
        )
        // 原始有类型JSON直接反序列化，不经Value/map去重；不得放入URL query或日志。
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(AsyncRequireAuthorizationLayer::new(DsgAuthorize::new(
            compute_id, jwks,
        )))
        // 能力/观察结果与boot和当前事实绑定，禁止中间缓存重放旧状态；认证失败亦不缓存。
        .layer(axum::middleware::map_response(no_store))
}

// 固定当前签名owner，禁止只读创建凭据/旧owner借续租维持服务。失联后PG
// 自行关闭，网关错误不清SDK在途状态；新期限也不是任何资源动作的许可。
async fn admission_lease(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<compute_api::dsg_admission_lease::DsgAdmissionLeaseRequest>,
) -> Response {
    if request.validate().is_err() {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    let owner = DsgIssuerFenceRequest {
        protocol_version: 1,
        instance: request.instance.clone(),
        issuer_epoch: request.issuer_epoch,
    };
    if claims.authorize_fence(&owner).is_err() {
        return failure(StatusCode::FORBIDDEN, "wrong_instance_or_epoch", None);
    }
    match client(&compute, &claims).await {
        Ok(client) => match client.admission_lease(&request).await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(StatusCode::CONFLICT, "lease_unconfirmed", None),
        },
        Err(error) => error,
    }
}

// 更高签名owner只读原rearm；不会借查询调用setter/fence，HTTP/SQL双层
// 固定只读入口。缺回执是未知，不能补造NoEffect或接受当前deadline代替。
async fn admission_rearm_status(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<compute_api::dsg_admission_lease::DsgAdmissionLeaseRequest>,
) -> Response {
    if request.validate().is_err()
        || request.action != compute_api::dsg_admission_lease::DsgAdmissionLeaseAction::Rearm
    {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    if claims.authorize_rearm_status(&request).is_err() {
        return failure(StatusCode::FORBIDDEN, "wrong_instance_or_epoch", None);
    }
    match client(&compute, &claims).await {
        Ok(client) => match client.admission_rearm_status(&request).await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(StatusCode::CONFLICT, "rearm_status_unconfirmed", None),
        },
        Err(error) => error,
    }
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

async fn shared_cost(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        Ok(client) => match client.shared_cost().await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "shared_cost_unavailable",
                None,
            ),
        },
        Err(error) => error,
    }
}

// 同一路由认证/实例/current issuer核验，只返回Native事实，不写排空或VM许可。
async fn work_state(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        Ok(client) => match client.work_state().await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "work_observation_unavailable",
                None,
            ),
        },
        Err(error) => error,
    }
}

// 原认证/current issuer/实例检查及独立观察连接；同点work禁止从两个GET拼成证明。
async fn connection_state(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        Ok(client) => match client.connection_state().await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "connection_observation_unavailable",
                None,
            ),
        },
        Err(error) => error,
    }
}

// 固定只读路径复用原认证/current issuer/实例检查，不能由外部body报prepared计数。
async fn prepared_state(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        Ok(client) => match client.prepared_state().await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "prepared_observation_unavailable",
                None,
            ),
        },
        Err(error) => error,
    }
}

// 固定只读DSM路径；原JWT/current issuer/实例检查后才取独立观察池，不接受调用方报数。
async fn dsm_state(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
) -> Response {
    match client(&compute, &claims).await {
        Ok(client) => match client.dsm_state().await {
            Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
            Err(_) => failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "dsm_observation_unavailable",
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

async fn admission(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgAdmissionRequest>,
) -> Response {
    admission_operation(&compute, &claims, request, false).await
}

async fn admission_status(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgAdmissionRequest>,
) -> Response {
    admission_operation(&compute, &claims, request, true).await
}

async fn admission_operation(
    compute: &ComputeNode,
    claims: &DsgHttpClaims,
    request: DsgAdmissionRequest,
    query: bool,
) -> Response {
    if request.validate().is_err() {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    if claims.authorize_admission(&request, !query).is_err() {
        return failure(StatusCode::FORBIDDEN, "wrong_instance_or_epoch", None);
    }
    // 只读查询和关闭门禁可用于失败收口，不依赖外部Running标志；只有开放普通
    // SQL必须再次检查当前运行态。检查成功仍不等于持久预算/issuer租约有效。
    if !query && request.open && compute.verify_dsg_running().is_err() {
        return failure(StatusCode::PRECONDITION_FAILED, "not_available", None);
    }
    match client(compute, claims).await {
        Ok(client) => admission_result(if query {
            client.admission_status(&request).await
        } else {
            client.set_admission(&request).await
        }),
        Err(error) => error,
    }
}

async fn admission_close(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgAdmissionCloseRequest>,
) -> Response {
    admission_close_operation(&compute, &claims, request, false).await
}

async fn admission_close_status(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgAdmissionCloseRequest>,
) -> Response {
    admission_close_operation(&compute, &claims, request, true).await
}

async fn admission_close_operation(
    compute: &ComputeNode,
    claims: &DsgHttpClaims,
    request: DsgAdmissionCloseRequest,
    query: bool,
) -> Response {
    if request.validate().is_err() {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    // 接管者可以引用旧原意图，但任何新关闭写入必须使用当前签名epoch。关闭
    // 回执查询完全只读，允许新owner读取旧关闭回执，不能偷偷重放其setter。
    if claims
        .authorize_admission(&request.original, false)
        .is_err()
        || claims.authorize_admission(&request.close, !query).is_err()
    {
        return failure(StatusCode::FORBIDDEN, "wrong_instance_or_epoch", None);
    }
    match client(compute, claims).await {
        Ok(client) => admission_result(if query {
            client
                .admission_closure_status(&request.original, &request.close)
                .await
        } else {
            client
                .close_admission_over_original(&request.original, &request.close)
                .await
        }),
        Err(error) => error,
    }
}

async fn admission_continue(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgAdmissionContinuationRequest>,
) -> Response {
    admission_continue_operation(&compute, &claims, request, false).await
}

async fn admission_continue_status(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    Json(request): Json<DsgAdmissionContinuationRequest>,
) -> Response {
    admission_continue_operation(&compute, &claims, request, true).await
}

async fn admission_continue_operation(
    compute: &ComputeNode,
    claims: &DsgHttpClaims,
    request: DsgAdmissionContinuationRequest,
    query: bool,
) -> Response {
    if request.validate().is_err() {
        return failure(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    // 旧两份请求只读授权；新关闭必须恰为当前签名epoch。不能借未来floor或
    // 原owner签名发送接续；SDK还会先取得PG原生fence，并保留全部未知选择。
    if claims
        .authorize_admission(&request.original, false)
        .is_err()
        || claims
            .authorize_admission(&request.previous_close, false)
            .is_err()
        || claims.authorize_admission(&request.close, !query).is_err()
    {
        return failure(StatusCode::FORBIDDEN, "wrong_instance_or_epoch", None);
    }
    match client(compute, claims).await {
        Ok(client) => admission_result(if query {
            client.admission_continuation_status(&request).await
        } else {
            client.continue_admission_closure(&request).await
        }),
        Err(error) => error,
    }
}

fn admission_result<T: Serialize>(result: Result<T, DsgAdmissionCallError>) -> Response {
    match result {
        Ok(fact) => JsonResponse::success(StatusCode::OK, fact),
        Err(DsgAdmissionCallError::NotSent(_)) => failure(StatusCode::CONFLICT, "not_sent", None),
        // 请求已发送但未确认、busy或关闭未获完整证明时保留原意图，不能回显内部
        // SQL错误、凭据或称为NoEffect；即使关闭确认也始终不产生VM缩容许可。
        Err(DsgAdmissionCallError::Unconfirmed { .. }) => {
            failure(StatusCode::SERVICE_UNAVAILABLE, "unconfirmed", None)
        }
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
