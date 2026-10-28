//! 固定root资源通道的透明转发。资源生命周期独立于PG boot/SQL连接；这里不生成
//! 预算/执行回执，不重试未知写，不向无认证neonvmd端口降级。

use std::{sync::Arc, time::Duration};

use axum::{
    Extension,
    body::{Body, Bytes},
    extract::State,
    response::Response,
};
use compute_api::dsg_auth::DsgHttpClaims;
use http::{
    Request, StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use http_body_util::{BodyExt, Full, Limited};
use hyper_util::rt::TokioIo;

use crate::{compute::ComputeNode, http::JsonResponse};

const SOCKET: &str = "/run/neon-dsga/resource.sock";

pub(super) async fn proxy(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    request: Request<Body>,
) -> Response {
    let deny = |status| JsonResponse::error(status, "resource_unconfirmed");
    let Ok(spec) = compute.dsg_spec() else {
        return deny(StatusCode::PRECONDITION_FAILED);
    };
    if claims.vm_uid != spec.vm_uid || claims.compute_instance_id != spec.compute_instance_id {
        return deny(StatusCode::FORBIDDEN);
    }
    // 不调用dsg_management_client/verify_dsg_running：PG断连不能阻止旧Guest约束
    // 的原查询及资源floor接管。原签名直接交root重新验签，不信任自报claims头。
    if request.uri().query().is_some()
        || request
            .headers()
            .contains_key(http::header::CONTENT_ENCODING)
    {
        return deny(StatusCode::BAD_REQUEST);
    }
    let (parts, body) = request.into_parts();
    let Some(bearer) = parts.headers.get(AUTHORIZATION).cloned() else {
        return deny(StatusCode::UNAUTHORIZED);
    };
    let Ok(body) = axum::body::to_bytes(body, 16 * 1024).await else {
        return deny(StatusCode::BAD_REQUEST);
    };
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        forward(SOCKET, parts.method, parts.uri.path(), bearer, body),
    )
    .await;
    match result {
        Ok(Ok(response)) => response,
        _ => deny(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn forward(
    socket: &str,
    method: http::Method,
    path: &str,
    bearer: http::HeaderValue,
    body: Bytes,
) -> anyhow::Result<Response> {
    let stream = tokio::net::UnixStream::connect(socket).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    // 连接任务与本次调用同生共死；超时/取消会drop guard并abort驱动，绝不把
    // 后台HTTP请求遗留为下一次请求的隐式重试。root正在执行的同步内核写仍由其
    // 自己的文件锁/原意图保护，连接关闭不表示NoEffect。
    let task = tokio::spawn(connection);
    struct Abort(tokio::task::JoinHandle<Result<(), hyper::Error>>);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _abort = Abort(task);
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::HOST, "dsga-resource")
        .header(AUTHORIZATION, bearer)
        .header(CONTENT_TYPE, "application/json")
        .header(http::header::CONNECTION, "close")
        .body(Full::new(body))?;
    let response = sender.send_request(request).await?;
    // 只接受root服务约定的状态和限长JSON；不转发Location/cookies/任意header，
    // 不追踪重定向，不让来自root故障页的敏感内容流入外部日志。
    anyhow::ensure!(
        [200, 400, 401, 403, 404, 409].contains(&response.status().as_u16()),
        "resource response unconfirmed"
    );
    anyhow::ensure!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .is_some_and(|v| v == "application/json")
            && response
                .headers()
                .get(http::header::CACHE_CONTROL)
                .is_some_and(|v| v == "no-store"),
        "resource response unconfirmed"
    );
    let status = response.status();
    let body = Limited::new(response.into_body(), 32 * 1024)
        .collect()
        .await
        .map_err(|_| anyhow::anyhow!("resource body unconfirmed"))?
        .to_bytes();
    Ok(Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixed_resource_proxy_preserves_original_and_closes_connection() {
        check_fixed_forward("/dsg/v1/resource/observe").await;
        check_fixed_forward("/dsg/v1/resource/repair-held").await;
    }

    // 转发保持原字节/原签名，不在Rust根据JSON重新生成恢复选择。
    // 此处UDS peer为夹具；完整Root请求及鉴权由Go生产handler另验。
    async fn check_fixed_forward(route: &'static str) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("guard.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    TokioIo::new(stream),
                    hyper::service::service_fn(
                        move |r: Request<hyper::body::Incoming>| async move {
                            assert_eq!(r.uri(), route);
                            assert_eq!(r.headers()[AUTHORIZATION], "Bearer original-signature");
                            assert!(!r.headers().contains_key("x-issuer-epoch"));
                            assert_eq!(
                                r.into_body().collect().await.unwrap().to_bytes(),
                                r#"{"issuer_epoch":"9007199254740993"}"#
                            );
                            Ok::<_, std::convert::Infallible>(
                                http::Response::builder()
                                    .header(CONTENT_TYPE, "application/json")
                                    .header(http::header::CACHE_CONTROL, "no-store")
                                    .header("set-cookie", "must-not-forward")
                                    .body(Full::new(Bytes::from_static(b"{}")))
                                    .unwrap(),
                            )
                        },
                    ),
                )
                .await
                .unwrap();
        });
        let response = forward(
            path.to_str().unwrap(),
            http::Method::POST,
            route,
            http::HeaderValue::from_static("Bearer original-signature"),
            Bytes::from_static(br#"{"issuer_epoch":"9007199254740993"}"#),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("set-cookie"));
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "{}"
        );
        server.await.unwrap();
    }
}
