use std::sync::Arc;

use anyhow::bail;
use futures::pin_mut;
use futures::StreamExt;
use hyper::body::HttpBody;
use hyper::header;
use hyper::http::HeaderName;
use hyper::http::HeaderValue;
use hyper::Response;
use hyper::StatusCode;
use hyper::{Body, HeaderMap, Request};
use serde_json::json;
use serde_json::Value;
use tokio::try_join;
use tokio_postgres::error::DbError;
use tokio_postgres::error::ErrorPosition;
use tokio_postgres::GenericClient;
use tokio_postgres::IsolationLevel;
use tokio_postgres::ReadyForQueryStatus;
use tokio_postgres::Transaction;
use tracing::error;
use tracing::info;
use tracing::instrument;
use url::Url;
use utils::http::error::ApiError;
use utils::http::json::json_response;

use crate::auth::backend::ComputeUserInfo;
use crate::auth::endpoint_sni;
use crate::auth::ComputeUserInfoParseError;
use crate::config::ProxyConfig;
use crate::config::TlsConfig;
use crate::context::RequestMonitoring;
use crate::metrics::HTTP_CONTENT_LENGTH;
use crate::metrics::NUM_CONNECTION_REQUESTS_GAUGE;
use crate::proxy::NeonOptions;
use crate::DbName;
use crate::RoleName;

use super::backend::PoolingBackend;
use super::conn_pool::ConnInfo;
use super::json::json_to_pg_text;
use super::json::pg_text_row_to_json;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryData {
    query: String,
    #[serde(deserialize_with = "bytes_to_pg_text")]
    params: Vec<Option<String>>,
    #[serde(default)]
    array_mode: Option<bool>,
}

#[derive(serde::Deserialize)]
struct BatchQueryData {
    queries: Vec<QueryData>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Payload {
    Single(QueryData),
    Batch(BatchQueryData),
}

const MAX_RESPONSE_SIZE: usize = 10 * 1024 * 1024; // 10 MiB
const MAX_REQUEST_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB

static RAW_TEXT_OUTPUT: HeaderName = HeaderName::from_static("neon-raw-text-output");
static ARRAY_MODE: HeaderName = HeaderName::from_static("neon-array-mode");
static ALLOW_POOL: HeaderName = HeaderName::from_static("neon-pool-opt-in");
static TXN_ISOLATION_LEVEL: HeaderName = HeaderName::from_static("neon-batch-isolation-level");
static TXN_READ_ONLY: HeaderName = HeaderName::from_static("neon-batch-read-only");
static TXN_DEFERRABLE: HeaderName = HeaderName::from_static("neon-batch-deferrable");

static HEADER_VALUE_TRUE: HeaderValue = HeaderValue::from_static("true");

fn bytes_to_pg_text<'de, D>(deserializer: D) -> Result<Vec<Option<String>>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    // TODO: consider avoiding the allocation here.
    let json: Vec<Value> = serde::de::Deserialize::deserialize(deserializer)?;
    Ok(json_to_pg_text(json))
}

#[derive(Debug, thiserror::Error)]
pub enum ConnInfoError {
    #[error("invalid header: {0}")]
    InvalidHeader(&'static str),
    #[error("invalid connection string: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("incorrect scheme")]
    IncorrectScheme,
    #[error("missing database name")]
    MissingDbName,
    #[error("invalid database name")]
    InvalidDbName,
    #[error("missing username")]
    MissingUsername,
    #[error("invalid username: {0}")]
    InvalidUsername(#[from] std::string::FromUtf8Error),
    #[error("missing password")]
    MissingPassword,
    #[error("missing hostname")]
    MissingHostname,
    #[error("invalid hostname: {0}")]
    InvalidEndpoint(#[from] ComputeUserInfoParseError),
    #[error("malformed endpoint")]
    MalformedEndpoint,
}

fn get_conn_info(
    ctx: &mut RequestMonitoring,
    headers: &HeaderMap,
    tls: &TlsConfig,
) -> Result<ConnInfo, ConnInfoError> {
    // HTTP only uses cleartext (for now and likely always)
    ctx.set_auth_method(crate::context::AuthMethod::Cleartext);

    let connection_string = headers
        .get("Neon-Connection-String")
        .ok_or(ConnInfoError::InvalidHeader("Neon-Connection-String"))?
        .to_str()
        .map_err(|_| ConnInfoError::InvalidHeader("Neon-Connection-String"))?;

    let connection_url = Url::parse(connection_string)?;

    let protocol = connection_url.scheme();
    if protocol != "postgres" && protocol != "postgresql" {
        return Err(ConnInfoError::IncorrectScheme);
    }

    let mut url_path = connection_url
        .path_segments()
        .ok_or(ConnInfoError::MissingDbName)?;

    let dbname: DbName = url_path.next().ok_or(ConnInfoError::InvalidDbName)?.into();
    ctx.set_dbname(dbname.clone());

    let username = RoleName::from(urlencoding::decode(connection_url.username())?);
    if username.is_empty() {
        return Err(ConnInfoError::MissingUsername);
    }
    ctx.set_user(username.clone());

    let password = connection_url
        .password()
        .ok_or(ConnInfoError::MissingPassword)?;
    let password = urlencoding::decode_binary(password.as_bytes());

    let hostname = connection_url
        .host_str()
        .ok_or(ConnInfoError::MissingHostname)?;

    let endpoint =
        endpoint_sni(hostname, &tls.common_names)?.ok_or(ConnInfoError::MalformedEndpoint)?;
    ctx.set_endpoint_id(endpoint.clone());

    let pairs = connection_url.query_pairs();

    let mut options = Option::None;

    for (key, value) in pairs {
        match &*key {
            "options" => {
                options = Some(NeonOptions::parse_options_raw(&value));
            }
            "application_name" => ctx.set_application(Some(value.into())),
            _ => {}
        }
    }

    let user_info = ComputeUserInfo {
        endpoint,
        user: username,
        options: options.unwrap_or_default(),
    };

    Ok(ConnInfo {
        user_info,
        dbname,
        password: match password {
            std::borrow::Cow::Borrowed(b) => b.into(),
            std::borrow::Cow::Owned(b) => b.into(),
        },
    })
}

// TODO: return different http error codes
pub async fn handle(
    config: &'static ProxyConfig,
    mut ctx: RequestMonitoring,
    request: Request<Body>,
    backend: Arc<PoolingBackend>,
) -> Result<Response<Body>, ApiError> {
    let result = tokio::time::timeout(
        config.http_config.request_timeout,
        handle_inner(config, &mut ctx, request, backend),
    )
    .await;
    let mut response = match result {
        Ok(r) => match r {
            Ok(r) => {
                ctx.set_success();
                r
            }
            Err(e) => {
                // TODO: ctx.set_error_kind(e.get_error_type());

                let mut message = format!("{:?}", e);
                let db_error = e
                    .downcast_ref::<tokio_postgres::Error>()
                    .and_then(|e| e.as_db_error());
                fn get<'a, T: serde::Serialize>(
                    db: Option<&'a DbError>,
                    x: impl FnOnce(&'a DbError) -> T,
                ) -> Value {
                    db.map(x)
                        .and_then(|t| serde_json::to_value(t).ok())
                        .unwrap_or_default()
                }

                if let Some(db_error) = db_error {
                    db_error.message().clone_into(&mut message);
                }

                let position = db_error.and_then(|db| db.position());
                let (position, internal_position, internal_query) = match position {
                    Some(ErrorPosition::Original(position)) => (
                        Value::String(position.to_string()),
                        Value::Null,
                        Value::Null,
                    ),
                    Some(ErrorPosition::Internal { position, query }) => (
                        Value::Null,
                        Value::String(position.to_string()),
                        Value::String(query.clone()),
                    ),
                    None => (Value::Null, Value::Null, Value::Null),
                };

                let code = get(db_error, |db| db.code().code());
                let severity = get(db_error, |db| db.severity());
                let detail = get(db_error, |db| db.detail());
                let hint = get(db_error, |db| db.hint());
                let where_ = get(db_error, |db| db.where_());
                let table = get(db_error, |db| db.table());
                let column = get(db_error, |db| db.column());
                let schema = get(db_error, |db| db.schema());
                let datatype = get(db_error, |db| db.datatype());
                let constraint = get(db_error, |db| db.constraint());
                let file = get(db_error, |db| db.file());
                let line = get(db_error, |db| db.line().map(|l| l.to_string()));
                let routine = get(db_error, |db| db.routine());

                error!(
                    ?code,
                    "sql-over-http per-client task finished with an error: {e:#}"
                );
                // TODO: this shouldn't always be bad request.
                json_response(
                    StatusCode::BAD_REQUEST,
                    json!({
                        "message": message,
                        "code": code,
                        "detail": detail,
                        "hint": hint,
                        "position": position,
                        "internalPosition": internal_position,
                        "internalQuery": internal_query,
                        "severity": severity,
                        "where": where_,
                        "table": table,
                        "column": column,
                        "schema": schema,
                        "dataType": datatype,
                        "constraint": constraint,
                        "file": file,
                        "line": line,
                        "routine": routine,
                    }),
                )?
            }
        },
        Err(_) => {
            // TODO: when http error classification is done, distinguish between
            // timeout on sql vs timeout in proxy/cplane
            // ctx.set_error_kind(crate::error::ErrorKind::RateLimit);

            let message = format!(
                "HTTP-Connection timed out, execution time exeeded {} seconds",
                config.http_config.request_timeout.as_secs()
            );
            error!(message);
            json_response(
                StatusCode::GATEWAY_TIMEOUT,
                json!({ "message": message, "code": StatusCode::GATEWAY_TIMEOUT.as_u16() }),
            )?
        }
    };

    response.headers_mut().insert(
        "Access-Control-Allow-Origin",
        hyper::http::HeaderValue::from_static("*"),
    );
    Ok(response)
}

#[instrument(
    name = "sql-over-http",
    skip_all,
    fields(
        pid = tracing::field::Empty,
        conn_id = tracing::field::Empty
    )
)]
async fn handle_inner(
    config: &'static ProxyConfig,
    ctx: &mut RequestMonitoring,
    request: Request<Body>,
    backend: Arc<PoolingBackend>,
) -> anyhow::Result<Response<Body>> {
    let _request_gauge = NUM_CONNECTION_REQUESTS_GAUGE
        .with_label_values(&[ctx.protocol])
        .guard();
    info!(
        protocol = ctx.protocol,
        "handling interactive connection from client"
    );

    //
    // Determine the destination and connection params
    //
    let headers = request.headers();
    // TLS config should be there.
    let conn_info = get_conn_info(ctx, headers, config.tls_config.as_ref().unwrap())?;
    info!(
        user = conn_info.user_info.user.as_str(),
        project = conn_info.user_info.endpoint.as_str(),
        "credentials"
    );

    // Determine the output options. Default behaviour is 'false'. Anything that is not
    // strictly 'true' assumed to be false.
    let raw_output = headers.get(&RAW_TEXT_OUTPUT) == Some(&HEADER_VALUE_TRUE);
    let default_array_mode = headers.get(&ARRAY_MODE) == Some(&HEADER_VALUE_TRUE);

    // Allow connection pooling only if explicitly requested
    // or if we have decided that http pool is no longer opt-in
    let allow_pool = !config.http_config.pool_options.opt_in
        || headers.get(&ALLOW_POOL) == Some(&HEADER_VALUE_TRUE);

    // isolation level, read only and deferrable

    let txn_isolation_level_raw = headers.get(&TXN_ISOLATION_LEVEL).cloned();
    let txn_isolation_level = match txn_isolation_level_raw {
        Some(ref x) => Some(match x.as_bytes() {
            b"Serializable" => IsolationLevel::Serializable,
            b"ReadUncommitted" => IsolationLevel::ReadUncommitted,
            b"ReadCommitted" => IsolationLevel::ReadCommitted,
            b"RepeatableRead" => IsolationLevel::RepeatableRead,
            _ => bail!("invalid isolation level"),
        }),
        None => None,
    };

    let txn_read_only = headers.get(&TXN_READ_ONLY) == Some(&HEADER_VALUE_TRUE);
    let txn_deferrable = headers.get(&TXN_DEFERRABLE) == Some(&HEADER_VALUE_TRUE);

    let request_content_length = match request.body().size_hint().upper() {
        Some(v) => v,
        None => MAX_REQUEST_SIZE + 1,
    };
    info!(request_content_length, "request size in bytes");
    HTTP_CONTENT_LENGTH.observe(request_content_length as f64);

    // we don't have a streaming request support yet so this is to prevent OOM
    // from a malicious user sending an extremely large request body
    if request_content_length > MAX_REQUEST_SIZE {
        return Err(anyhow::anyhow!(
            "request is too large (max is {MAX_REQUEST_SIZE} bytes)"
        ));
    }

    let fetch_and_process_request = async {
        let body = hyper::body::to_bytes(request.into_body())
            .await
            .map_err(anyhow::Error::from)?;
        info!(length = body.len(), "request payload read");
        let payload: Payload = serde_json::from_slice(&body)?;
        Ok::<Payload, anyhow::Error>(payload) // Adjust error type accordingly
    };

    let authenticate_and_connect = async {
        let keys = backend.authenticate(ctx, &conn_info).await?;
        let client = backend
            .connect_to_compute(ctx, conn_info, keys, !allow_pool)
            .await?;
        // not strictly necessary to mark success here,
        // but it's just insurance for if we forget it somewhere else
        ctx.latency_timer.success();
        Ok::<_, anyhow::Error>(client)
    };

    // Run both operations in parallel
    let (payload, mut client) = try_join!(fetch_and_process_request, authenticate_and_connect)?;

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json");

    //
    // Now execute the query and return the result
    //
    let mut size = 0;
    let result = match payload {
        Payload::Single(stmt) => {
            let (status, results) =
                query_to_json(&*client, stmt, &mut 0, raw_output, default_array_mode)
                    .await
                    .map_err(|e| {
                        client.discard();
                        e
                    })?;
            client.check_idle(status);
            results
        }
        Payload::Batch(statements) => {
            info!("starting transaction");
            let (inner, mut discard) = client.inner();
            let mut builder = inner.build_transaction();
            if let Some(isolation_level) = txn_isolation_level {
                builder = builder.isolation_level(isolation_level);
            }
            if txn_read_only {
                builder = builder.read_only(true);
            }
            if txn_deferrable {
                builder = builder.deferrable(true);
            }

            let transaction = builder.start().await.map_err(|e| {
                // if we cannot start a transaction, we should return immediately
                // and not return to the pool. connection is clearly broken
                discard.discard();
                e
            })?;

            let results = match query_batch(
                &transaction,
                statements,
                &mut size,
                raw_output,
                default_array_mode,
            )
            .await
            {
                Ok(results) => {
                    info!("commit");
                    let status = transaction.commit().await.map_err(|e| {
                        // if we cannot commit - for now don't return connection to pool
                        // TODO: get a query status from the error
                        discard.discard();
                        e
                    })?;
                    discard.check_idle(status);
                    results
                }
                Err(err) => {
                    info!("rollback");
                    let status = transaction.rollback().await.map_err(|e| {
                        // if we cannot rollback - for now don't return connection to pool
                        // TODO: get a query status from the error
                        discard.discard();
                        e
                    })?;
                    discard.check_idle(status);
                    return Err(err);
                }
            };

            if txn_read_only {
                response = response.header(
                    TXN_READ_ONLY.clone(),
                    HeaderValue::try_from(txn_read_only.to_string())?,
                );
            }
            if txn_deferrable {
                response = response.header(
                    TXN_DEFERRABLE.clone(),
                    HeaderValue::try_from(txn_deferrable.to_string())?,
                );
            }
            if let Some(txn_isolation_level) = txn_isolation_level_raw {
                response = response.header(TXN_ISOLATION_LEVEL.clone(), txn_isolation_level);
            }
            json!({ "results": results })
        }
    };

    let metrics = client.metrics();

    // how could this possibly fail
    let body = serde_json::to_string(&result).expect("json serialization should not fail");
    let len = body.len();
    let response = response
        .body(Body::from(body))
        // only fails if invalid status code or invalid header/values are given.
        // these are not user configurable so it cannot fail dynamically
        .expect("building response payload should not fail");

    // count the egress bytes - we miss the TLS and header overhead but oh well...
    // moving this later in the stack is going to be a lot of effort and ehhhh
    metrics.record_egress(len as u64);

    Ok(response)
}

async fn query_batch(
    transaction: &Transaction<'_>,
    queries: BatchQueryData,
    total_size: &mut usize,
    raw_output: bool,
    array_mode: bool,
) -> anyhow::Result<Vec<Value>> {
    let mut results = Vec::with_capacity(queries.queries.len());
    let mut current_size = 0;
    for stmt in queries.queries {
        // TODO: maybe we should check that the transaction bit is set here
        let (_, values) =
            query_to_json(transaction, stmt, &mut current_size, raw_output, array_mode).await?;
        results.push(values);
    }
    *total_size += current_size;
    Ok(results)
}

async fn query_to_json<T: GenericClient>(
    client: &T,
    data: QueryData,
    current_size: &mut usize,
    raw_output: bool,
    default_array_mode: bool,
) -> anyhow::Result<(ReadyForQueryStatus, Value)> {
    info!("executing query");
    let query_params = data.params;
    let row_stream = client.query_raw_txt(&data.query, query_params).await?;
    info!("finished executing query");

    // Manually drain the stream into a vector to leave row_stream hanging
    // around to get a command tag. Also check that the response is not too
    // big.
    pin_mut!(row_stream);
    let mut rows: Vec<tokio_postgres::Row> = Vec::new();
    while let Some(row) = row_stream.next().await {
        let row = row?;
        *current_size += row.body_len();
        rows.push(row);
        // we don't have a streaming response support yet so this is to prevent OOM
        // from a malicious query (eg a cross join)
        if *current_size > MAX_RESPONSE_SIZE {
            return Err(anyhow::anyhow!(
                "response is too large (max is {MAX_RESPONSE_SIZE} bytes)"
            ));
        }
    }

    let ready = row_stream.ready_status();

    // grab the command tag and number of rows affected
    let command_tag = row_stream.command_tag().unwrap_or_default();
    let mut command_tag_split = command_tag.split(' ');
    let command_tag_name = command_tag_split.next().unwrap_or_default();
    let command_tag_count = if command_tag_name == "INSERT" {
        // INSERT returns OID first and then number of rows
        command_tag_split.nth(1)
    } else {
        // other commands return number of rows (if any)
        command_tag_split.next()
    }
    .and_then(|s| s.parse::<i64>().ok());

    info!(
        rows = rows.len(),
        ?ready,
        command_tag,
        "finished reading rows"
    );

    let mut fields = vec![];
    let mut columns = vec![];

    for c in row_stream.columns() {
        fields.push(json!({
            "name": Value::String(c.name().to_owned()),
            "dataTypeID": Value::Number(c.type_().oid().into()),
            "tableID": c.table_oid(),
            "columnID": c.column_id(),
            "dataTypeSize": c.type_size(),
            "dataTypeModifier": c.type_modifier(),
            "format": "text",
        }));
        columns.push(client.get_type(c.type_oid()).await?);
    }

    let array_mode = data.array_mode.unwrap_or(default_array_mode);

    // convert rows to JSON
    let rows = rows
        .iter()
        .map(|row| pg_text_row_to_json(row, &columns, raw_output, array_mode))
        .collect::<Result<Vec<_>, _>>()?;

    // resulting JSON format is based on the format of node-postgres result
    Ok((
        ready,
        json!({
            "command": command_tag_name,
            "rowCount": command_tag_count,
            "rows": rows,
            "fields": fields,
            "rowAsArray": array_mode,
        }),
    ))
}
