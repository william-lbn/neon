use hyper::{Body, Request, Response, StatusCode, Uri};

use once_cell::sync::Lazy;
use postgres_ffi::WAL_SEGMENT_SIZE;
use safekeeper_api::models::{SkTimelineInfo, TimelineCopyRequest};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use storage_broker::proto::SafekeeperTimelineInfo;
use storage_broker::proto::TenantTimelineId as ProtoTenantTimelineId;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use utils::failpoint_support::failpoints_handler;
use utils::http::request::parse_query_param;

use std::io::Write as _;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info_span, Instrument};
use utils::http::endpoint::{request_span, ChannelWriter};

use crate::debug_dump::TimelineDigestRequest;
use crate::receive_wal::WalReceiverState;
use crate::safekeeper::Term;
use crate::safekeeper::{ServerInfo, TermLsn};
use crate::send_wal::WalSenderState;
use crate::timeline::PeerInfo;
use crate::{copy_timeline, debug_dump, patch_control_file, pull_timeline};

use crate::timelines_global_map::TimelineDeleteForceResult;
use crate::GlobalTimelines;
use crate::SafeKeeperConf;
use utils::{
    auth::SwappableJwtAuth,
    http::{
        endpoint::{self, auth_middleware, check_permission_with},
        error::ApiError,
        json::{json_request, json_response},
        request::{ensure_no_body, parse_request_param},
        RequestExt, RouterBuilder,
    },
    id::{NodeId, TenantId, TenantTimelineId, TimelineId},
    lsn::Lsn,
};

use super::models::TimelineCreateRequest;

#[derive(Debug, Serialize)]
struct SafekeeperStatus {
    id: NodeId,
}

/// Healthcheck handler.
async fn status_handler(request: Request<Body>) -> Result<Response<Body>, ApiError> {
    check_permission(&request, None)?;
    let conf = get_conf(&request);
    let status = SafekeeperStatus { id: conf.my_id };
    json_response(StatusCode::OK, status)
}

fn get_conf(request: &Request<Body>) -> &SafeKeeperConf {
    request
        .data::<Arc<SafeKeeperConf>>()
        .expect("unknown state type")
        .as_ref()
}

/// Same as TermLsn, but serializes LSN using display serializer
/// in Postgres format, i.e. 0/FFFFFFFF. Used only for the API response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TermSwitchApiEntry {
    pub term: Term,
    pub lsn: Lsn,
}

impl From<TermSwitchApiEntry> for TermLsn {
    fn from(api_val: TermSwitchApiEntry) -> Self {
        TermLsn {
            term: api_val.term,
            lsn: api_val.lsn,
        }
    }
}

/// Augment AcceptorState with epoch for convenience
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptorStateStatus {
    pub term: Term,
    pub epoch: Term,
    pub term_history: Vec<TermSwitchApiEntry>,
}

/// Info about timeline on safekeeper ready for reporting.
#[derive(Debug, Serialize, Deserialize)]
pub struct TimelineStatus {
    pub tenant_id: TenantId,
    pub timeline_id: TimelineId,
    pub acceptor_state: AcceptorStateStatus,
    pub pg_info: ServerInfo,
    pub flush_lsn: Lsn,
    pub timeline_start_lsn: Lsn,
    pub local_start_lsn: Lsn,
    pub commit_lsn: Lsn,
    pub backup_lsn: Lsn,
    pub peer_horizon_lsn: Lsn,
    pub remote_consistent_lsn: Lsn,
    pub peers: Vec<PeerInfo>,
    pub walsenders: Vec<WalSenderState>,
    pub walreceivers: Vec<WalReceiverState>,
}

fn check_permission(request: &Request<Body>, tenant_id: Option<TenantId>) -> Result<(), ApiError> {
    check_permission_with(request, |claims| {
        crate::auth::check_permission(claims, tenant_id)
    })
}

/// Report info about timeline.
async fn timeline_status_handler(request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "timeline_id")?,
    );
    check_permission(&request, Some(ttid.tenant_id))?;

    let tli = GlobalTimelines::get(ttid).map_err(ApiError::from)?;
    let (inmem, state) = tli.get_state().await;
    let flush_lsn = tli.get_flush_lsn().await;

    let epoch = state.acceptor_state.get_epoch(flush_lsn);
    let term_history = state
        .acceptor_state
        .term_history
        .0
        .into_iter()
        .map(|ts| TermSwitchApiEntry {
            term: ts.term,
            lsn: ts.lsn,
        })
        .collect();
    let acc_state = AcceptorStateStatus {
        term: state.acceptor_state.term,
        epoch,
        term_history,
    };

    let conf = get_conf(&request);
    // Note: we report in memory values which can be lost.
    let status = TimelineStatus {
        tenant_id: ttid.tenant_id,
        timeline_id: ttid.timeline_id,
        acceptor_state: acc_state,
        pg_info: state.server,
        flush_lsn,
        timeline_start_lsn: state.timeline_start_lsn,
        local_start_lsn: state.local_start_lsn,
        commit_lsn: inmem.commit_lsn,
        backup_lsn: inmem.backup_lsn,
        peer_horizon_lsn: inmem.peer_horizon_lsn,
        remote_consistent_lsn: inmem.remote_consistent_lsn,
        peers: tli.get_peers(conf).await,
        walsenders: tli.get_walsenders().get_all(),
        walreceivers: tli.get_walreceivers().get_all(),
    };
    json_response(StatusCode::OK, status)
}

async fn timeline_create_handler(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let request_data: TimelineCreateRequest = json_request(&mut request).await?;

    let ttid = TenantTimelineId {
        tenant_id: request_data.tenant_id,
        timeline_id: request_data.timeline_id,
    };
    check_permission(&request, Some(ttid.tenant_id))?;

    let server_info = ServerInfo {
        pg_version: request_data.pg_version,
        system_id: request_data.system_id.unwrap_or(0),
        wal_seg_size: request_data.wal_seg_size.unwrap_or(WAL_SEGMENT_SIZE as u32),
    };
    let local_start_lsn = request_data.local_start_lsn.unwrap_or_else(|| {
        request_data
            .commit_lsn
            .segment_lsn(server_info.wal_seg_size as usize)
    });
    GlobalTimelines::create(ttid, server_info, request_data.commit_lsn, local_start_lsn)
        .await
        .map_err(ApiError::InternalServerError)?;

    json_response(StatusCode::OK, ())
}

/// Pull timeline from peer safekeeper instances.
async fn timeline_pull_handler(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    check_permission(&request, None)?;

    let data: pull_timeline::Request = json_request(&mut request).await?;

    let resp = pull_timeline::handle_request(data)
        .await
        .map_err(ApiError::InternalServerError)?;
    json_response(StatusCode::OK, resp)
}

async fn timeline_copy_handler(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    check_permission(&request, None)?;

    let request_data: TimelineCopyRequest = json_request(&mut request).await?;
    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "source_timeline_id")?,
    );

    let source = GlobalTimelines::get(ttid)?;

    copy_timeline::handle_request(copy_timeline::Request{
        source,
        until_lsn: request_data.until_lsn,
        destination_ttid: TenantTimelineId::new(ttid.tenant_id, request_data.target_timeline_id),
    })
        .instrument(info_span!("copy_timeline", from=%ttid, to=%request_data.target_timeline_id, until_lsn=%request_data.until_lsn))
        .await
        .map_err(ApiError::InternalServerError)?;

    json_response(StatusCode::OK, ())
}

async fn timeline_digest_handler(request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "timeline_id")?,
    );
    check_permission(&request, Some(ttid.tenant_id))?;

    let from_lsn: Option<Lsn> = parse_query_param(&request, "from_lsn")?;
    let until_lsn: Option<Lsn> = parse_query_param(&request, "until_lsn")?;

    let request = TimelineDigestRequest {
        from_lsn: from_lsn.ok_or(ApiError::BadRequest(anyhow::anyhow!(
            "from_lsn is required"
        )))?,
        until_lsn: until_lsn.ok_or(ApiError::BadRequest(anyhow::anyhow!(
            "until_lsn is required"
        )))?,
    };

    let tli = GlobalTimelines::get(ttid).map_err(ApiError::from)?;

    let response = debug_dump::calculate_digest(&tli, request)
        .await
        .map_err(ApiError::InternalServerError)?;
    json_response(StatusCode::OK, response)
}

/// Download a file from the timeline directory.
// TODO: figure out a better way to copy files between safekeepers
async fn timeline_files_handler(request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "timeline_id")?,
    );
    check_permission(&request, Some(ttid.tenant_id))?;

    let filename: String = parse_request_param(&request, "filename")?;

    let tli = GlobalTimelines::get(ttid).map_err(ApiError::from)?;

    let filepath = tli.timeline_dir.join(filename);
    let mut file = File::open(&filepath)
        .await
        .map_err(|e| ApiError::InternalServerError(e.into()))?;

    let mut content = Vec::new();
    // TODO: don't store files in memory
    file.read_to_end(&mut content)
        .await
        .map_err(|e| ApiError::InternalServerError(e.into()))?;

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .body(Body::from(content))
        .map_err(|e| ApiError::InternalServerError(e.into()))
}

/// Deactivates the timeline and removes its data directory.
async fn timeline_delete_handler(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "timeline_id")?,
    );
    let only_local = parse_query_param(&request, "only_local")?.unwrap_or(false);
    check_permission(&request, Some(ttid.tenant_id))?;
    ensure_no_body(&mut request).await?;
    // FIXME: `delete_force` can fail from both internal errors and bad requests. Add better
    // error handling here when we're able to.
    let resp = GlobalTimelines::delete(&ttid, only_local)
        .await
        .map_err(ApiError::InternalServerError)?;
    json_response(StatusCode::OK, resp)
}

/// Deactivates all timelines for the tenant and removes its data directory.
/// See `timeline_delete_handler`.
async fn tenant_delete_handler(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let tenant_id = parse_request_param(&request, "tenant_id")?;
    let only_local = parse_query_param(&request, "only_local")?.unwrap_or(false);
    check_permission(&request, Some(tenant_id))?;
    ensure_no_body(&mut request).await?;
    // FIXME: `delete_force_all_for_tenant` can return an error for multiple different reasons;
    // Using an `InternalServerError` should be fixed when the types support it
    let delete_info = GlobalTimelines::delete_force_all_for_tenant(&tenant_id, only_local)
        .await
        .map_err(ApiError::InternalServerError)?;
    json_response(
        StatusCode::OK,
        delete_info
            .iter()
            .map(|(ttid, resp)| (format!("{}", ttid.timeline_id), *resp))
            .collect::<HashMap<String, TimelineDeleteForceResult>>(),
    )
}

/// Used only in tests to hand craft required data.
async fn record_safekeeper_info(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "timeline_id")?,
    );
    check_permission(&request, Some(ttid.tenant_id))?;
    let sk_info: SkTimelineInfo = json_request(&mut request).await?;
    let proto_sk_info = SafekeeperTimelineInfo {
        safekeeper_id: 0,
        tenant_timeline_id: Some(ProtoTenantTimelineId {
            tenant_id: ttid.tenant_id.as_ref().to_owned(),
            timeline_id: ttid.timeline_id.as_ref().to_owned(),
        }),
        term: sk_info.term.unwrap_or(0),
        last_log_term: sk_info.last_log_term.unwrap_or(0),
        flush_lsn: sk_info.flush_lsn.0,
        commit_lsn: sk_info.commit_lsn.0,
        remote_consistent_lsn: sk_info.remote_consistent_lsn.0,
        peer_horizon_lsn: sk_info.peer_horizon_lsn.0,
        safekeeper_connstr: sk_info.safekeeper_connstr.unwrap_or_else(|| "".to_owned()),
        http_connstr: sk_info.http_connstr.unwrap_or_else(|| "".to_owned()),
        backup_lsn: sk_info.backup_lsn.0,
        local_start_lsn: sk_info.local_start_lsn.0,
        availability_zone: None,
    };

    let tli = GlobalTimelines::get(ttid).map_err(ApiError::from)?;
    tli.record_safekeeper_info(proto_sk_info)
        .await
        .map_err(ApiError::InternalServerError)?;

    json_response(StatusCode::OK, ())
}

fn parse_kv_str<E: fmt::Display, T: FromStr<Err = E>>(k: &str, v: &str) -> Result<T, ApiError> {
    v.parse()
        .map_err(|e| ApiError::BadRequest(anyhow::anyhow!("cannot parse {k}: {e}")))
}

/// Dump debug info about all available safekeeper state.
async fn dump_debug_handler(mut request: Request<Body>) -> Result<Response<Body>, ApiError> {
    check_permission(&request, None)?;
    ensure_no_body(&mut request).await?;

    let mut dump_all: Option<bool> = None;
    let mut dump_control_file: Option<bool> = None;
    let mut dump_memory: Option<bool> = None;
    let mut dump_disk_content: Option<bool> = None;
    let mut dump_term_history: Option<bool> = None;
    let mut tenant_id: Option<TenantId> = None;
    let mut timeline_id: Option<TimelineId> = None;

    let query = request.uri().query().unwrap_or("");
    let mut values = url::form_urlencoded::parse(query.as_bytes());

    for (k, v) in &mut values {
        match k.as_ref() {
            "dump_all" => dump_all = Some(parse_kv_str(&k, &v)?),
            "dump_control_file" => dump_control_file = Some(parse_kv_str(&k, &v)?),
            "dump_memory" => dump_memory = Some(parse_kv_str(&k, &v)?),
            "dump_disk_content" => dump_disk_content = Some(parse_kv_str(&k, &v)?),
            "dump_term_history" => dump_term_history = Some(parse_kv_str(&k, &v)?),
            "tenant_id" => tenant_id = Some(parse_kv_str(&k, &v)?),
            "timeline_id" => timeline_id = Some(parse_kv_str(&k, &v)?),
            _ => Err(ApiError::BadRequest(anyhow::anyhow!(
                "Unknown query parameter: {}",
                k
            )))?,
        }
    }

    let dump_all = dump_all.unwrap_or(false);
    let dump_control_file = dump_control_file.unwrap_or(dump_all);
    let dump_memory = dump_memory.unwrap_or(dump_all);
    let dump_disk_content = dump_disk_content.unwrap_or(dump_all);
    let dump_term_history = dump_term_history.unwrap_or(true);

    let args = debug_dump::Args {
        dump_all,
        dump_control_file,
        dump_memory,
        dump_disk_content,
        dump_term_history,
        tenant_id,
        timeline_id,
    };

    let resp = debug_dump::build(args)
        .await
        .map_err(ApiError::InternalServerError)?;

    let started_at = std::time::Instant::now();

    let (tx, rx) = mpsc::channel(1);

    let body = Body::wrap_stream(ReceiverStream::new(rx));

    let mut writer = ChannelWriter::new(128 * 1024, tx);

    let response = Response::builder()
        .status(200)
        .header(hyper::header::CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .unwrap();

    let span = info_span!("blocking");
    tokio::task::spawn_blocking(move || {
        let _span = span.entered();

        let res = serde_json::to_writer(&mut writer, &resp)
            .map_err(std::io::Error::from)
            .and_then(|_| writer.flush());

        match res {
            Ok(()) => {
                tracing::info!(
                    bytes = writer.flushed_bytes(),
                    elapsed_ms = started_at.elapsed().as_millis(),
                    "responded /v1/debug_dump"
                );
            }
            Err(e) => {
                tracing::warn!("failed to write out /v1/debug_dump response: {e:#}");
                // semantics of this error are quite... unclear. we want to error the stream out to
                // abort the response to somehow notify the client that we failed.
                //
                // though, most likely the reason for failure is that the receiver is already gone.
                drop(
                    writer
                        .tx
                        .blocking_send(Err(std::io::ErrorKind::BrokenPipe.into())),
                );
            }
        }
    });

    Ok(response)
}

async fn patch_control_file_handler(
    mut request: Request<Body>,
) -> Result<Response<Body>, ApiError> {
    check_permission(&request, None)?;

    let ttid = TenantTimelineId::new(
        parse_request_param(&request, "tenant_id")?,
        parse_request_param(&request, "timeline_id")?,
    );

    let tli = GlobalTimelines::get(ttid).map_err(ApiError::from)?;

    let patch_request: patch_control_file::Request = json_request(&mut request).await?;
    let response = patch_control_file::handle_request(tli, patch_request)
        .await
        .map_err(ApiError::InternalServerError)?;

    json_response(StatusCode::OK, response)
}

/// Safekeeper http router.
pub fn make_router(conf: SafeKeeperConf) -> RouterBuilder<hyper::Body, ApiError> {
    let mut router = endpoint::make_router();
    if conf.http_auth.is_some() {
        router = router.middleware(auth_middleware(|request| {
            #[allow(clippy::mutable_key_type)]
            static ALLOWLIST_ROUTES: Lazy<HashSet<Uri>> = Lazy::new(|| {
                ["/v1/status", "/metrics"]
                    .iter()
                    .map(|v| v.parse().unwrap())
                    .collect()
            });
            if ALLOWLIST_ROUTES.contains(request.uri()) {
                None
            } else {
                // Option<Arc<SwappableJwtAuth>> is always provided as data below, hence unwrap().
                request
                    .data::<Option<Arc<SwappableJwtAuth>>>()
                    .unwrap()
                    .as_deref()
            }
        }))
    }

    // NB: on any changes do not forget to update the OpenAPI spec
    // located nearby (/safekeeper/src/http/openapi_spec.yaml).
    let auth = conf.http_auth.clone();
    router
        .data(Arc::new(conf))
        .data(auth)
        .get("/v1/status", |r| request_span(r, status_handler))
        .put("/v1/failpoints", |r| {
            request_span(r, move |r| async {
                let cancel = CancellationToken::new();
                failpoints_handler(r, cancel).await
            })
        })
        // Will be used in the future instead of implicit timeline creation
        .post("/v1/tenant/timeline", |r| {
            request_span(r, timeline_create_handler)
        })
        .get("/v1/tenant/:tenant_id/timeline/:timeline_id", |r| {
            request_span(r, timeline_status_handler)
        })
        .delete("/v1/tenant/:tenant_id/timeline/:timeline_id", |r| {
            request_span(r, timeline_delete_handler)
        })
        .delete("/v1/tenant/:tenant_id", |r| {
            request_span(r, tenant_delete_handler)
        })
        .post("/v1/pull_timeline", |r| {
            request_span(r, timeline_pull_handler)
        })
        .get(
            "/v1/tenant/:tenant_id/timeline/:timeline_id/file/:filename",
            |r| request_span(r, timeline_files_handler),
        )
        .post(
            "/v1/tenant/:tenant_id/timeline/:source_timeline_id/copy",
            |r| request_span(r, timeline_copy_handler),
        )
        .patch(
            "/v1/tenant/:tenant_id/timeline/:timeline_id/control_file",
            |r| request_span(r, patch_control_file_handler),
        )
        // for tests
        .post("/v1/record_safekeeper_info/:tenant_id/:timeline_id", |r| {
            request_span(r, record_safekeeper_info)
        })
        .get("/v1/debug_dump", |r| request_span(r, dump_debug_handler))
        .get("/v1/tenant/:tenant_id/timeline/:timeline_id/digest", |r| {
            request_span(r, timeline_digest_handler)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_term_switch_entry_api_serialize() {
        let state = AcceptorStateStatus {
            term: 1,
            epoch: 1,
            term_history: vec![TermSwitchApiEntry {
                term: 1,
                lsn: Lsn(0x16FFDDDD),
            }],
        };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            json,
            "{\"term\":1,\"epoch\":1,\"term_history\":[{\"term\":1,\"lsn\":\"0/16FFDDDD\"}]}"
        );
    }
}
