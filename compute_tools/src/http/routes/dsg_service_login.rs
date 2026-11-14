//! Scoped branch service login on the private PostgreSQL management connection.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use axum::{
    Extension,
    body::to_bytes,
    extract::{Request, State},
    response::Response,
};
use compute_api::{dsg_auth::DsgHttpClaims, dsg_control::DsgUint64, spec::ComputeMode};
use http::{
    StatusCode,
    header::{CONTENT_ENCODING, CONTENT_TYPE},
};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Client, Config, NoTls};

use crate::{compute::ComputeNode, http::JsonResponse};

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Service {
    #[default]
    ManagedAuth,
    DataApi,
    Function,
}

impl Service {
    fn is_managed_auth(&self) -> bool {
        *self == Self::ManagedAuth
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    protocol_version: u32,
    #[serde(default)]
    service: Service,
    project_id: String,
    branch_id: String,
    endpoint_id: String,
    pg_boot_id: DsgUint64,
    database: String,
    function_id: Option<String>,
    schema: Option<String>,
    generation: DsgUint64,
    // Explicit null disables the exact registered service authorization.
    verifier: serde_json::Value,
}

impl Login {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == 1
                && self.pg_boot_id.get() > 0
                && self.generation.get() > 0
                && self.generation.get() <= i64::MAX as u64
                && valid_id(&self.project_id, "prj_")
                && valid_id(&self.branch_id, "br_")
                && valid_id(&self.endpoint_id, "ep_")
                && !self.database.is_empty()
                && self.database.len() <= 63
                && self.database.as_bytes()[0].is_ascii_lowercase()
                && self
                    .database
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                && (self.verifier.is_null()
                    || self.verifier.as_str().is_some_and(crate::dsg_roles::scram)),
            "invalid service login request"
        );
        let valid_scope = match self.service {
            Service::Function => {
                self.function_id.as_deref().is_some_and(|id| valid_id(id, "fnc_"))
                    && self.schema.as_deref().is_some_and(valid_function_schema)
            }
            _ => self.function_id.is_none() && self.schema.is_none(),
        };
        ensure!(valid_scope, "invalid service login scope");
        Ok(())
    }
}

fn valid_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|id| {
        id.len() == 16
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn valid_function_schema(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 63
        && bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !["pg_", "neon_", "control_"]
            .iter()
            .any(|prefix| name.to_ascii_lowercase().starts_with(prefix))
        && !name.eq_ignore_ascii_case("information_schema")
}

#[derive(Serialize)]
struct Receipt {
    protocol_version: u32,
    #[serde(skip_serializing_if = "Service::is_managed_auth")]
    service: Service,
    branch_id: String,
    pg_boot_id: DsgUint64,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    generation: DsgUint64,
    enabled: bool,
}

pub(crate) async fn reconcile(
    State(compute): State<Arc<ComputeNode>>,
    Extension(claims): Extension<DsgHttpClaims>,
    request: Request,
) -> Response {
    if request.uri().query().is_some()
        || request.headers().contains_key(CONTENT_ENCODING)
        || request.headers().get_all(CONTENT_TYPE).iter().count() != 1
        || request
            .headers()
            .get(CONTENT_TYPE)
            .is_none_or(|v| v != "application/json")
    {
        return JsonResponse::error(StatusCode::BAD_REQUEST, "invalid service login transport");
    }
    let Ok(body) = to_bytes(request.into_body(), 4096).await else {
        return JsonResponse::error(StatusCode::BAD_REQUEST, "invalid service login body");
    };
    if claims.authorize_service_login(&body).is_err() {
        return JsonResponse::error(StatusCode::FORBIDDEN, "unbound service login request");
    }
    let Ok(input) = serde_json::from_slice::<Login>(&body) else {
        return JsonResponse::error(StatusCode::BAD_REQUEST, "invalid service login document");
    };
    if input.validate().is_err() {
        return JsonResponse::error(StatusCode::BAD_REQUEST, "invalid service login fields");
    }
    let runtime = tokio::runtime::Handle::current();
    let task = tokio::task::spawn_blocking(move || {
        let _configuration = compute
            .dsg_configuration_lock
            .try_lock()
            .map_err(|_| anyhow::anyhow!("DSGA configuration unavailable"))?;
        let _lifecycle = compute.dsg_suspension.try_read()?;
        ensure!(!*_lifecycle, "Writer is draining");
        compute.verify_dsg_running()?;
        let original = {
            let state = compute
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Compute state poisoned"))?;
            state
                .pspec
                .clone()
                .context("Writer configuration unavailable")?
        };
        compute.verify_dsg_storage(&original)?;
        let spec = original.spec;
        let boot = compute
            .frozen_dsg_startup()
            .context("Writer boot unavailable")?;
        ensure!(
            spec.mode == ComputeMode::Primary
                && spec.project_id.as_deref() == Some(input.project_id.as_str())
                && spec.branch_id.as_deref() == Some(input.branch_id.as_str())
                && spec.endpoint_id.as_deref() == Some(input.endpoint_id.as_str())
                && claims.vm_uid == boot.vm_uid
                && claims.compute_instance_id == boot.compute_instance_id,
            "service login does not match the current Writer"
        );
        runtime
            .block_on(async {
                let reader = compute.dsg_sql_client().await?;
                let observation = reader.memory_status().await?;
                ensure!(
                    observation.observed.instance.pg_boot_id == input.pg_boot_id,
                    "service login does not match the current PostgreSQL boot"
                );
                Ok::<_, anyhow::Error>(())
            })
            .map_err(|_| {
                anyhow::anyhow!("service login boot observation unavailable or different")
            })?;
        let mut config = compute.get_tokio_conn_conf(Some("compute_ctl:DSGA_service_login"));
        config.dbname(&input.database);
        runtime.block_on(apply(
            &config,
            &input,
            &original.tenant_id.to_string(),
            &original.timeline_id.to_string(),
        ))?;
        compute.refresh_dsg_hba(&spec, true)?;
        Ok::<_, anyhow::Error>(Receipt {
            protocol_version: 1,
            service: input.service,
            branch_id: input.branch_id,
            pg_boot_id: input.pg_boot_id,
            function_id: input.function_id,
            schema: input.schema,
            generation: input.generation,
            enabled: !input.verifier.is_null(),
        })
    });
    match task.await {
        Ok(Ok(receipt)) => JsonResponse::success(StatusCode::OK, receipt),
        Ok(Err(error)) => {
            // apply redacts database error text and request credential values.
            tracing::warn!(stage=%error, "DSGA_SERVICE_LOGIN_UNCONFIRMED");
            JsonResponse::error(StatusCode::CONFLICT, "service login unconfirmed")
        }
        Err(_) => JsonResponse::error(StatusCode::SERVICE_UNAVAILABLE, "service login unavailable"),
    }
}

async fn connect(config: &Config) -> Result<Client> {
    let (client, driver) = config.connect(NoTls).await?;
    tokio::spawn(async move {
        let _ = driver.await;
    });
    Ok(client)
}

async fn apply(config: &Config, input: &Login, tenant: &str, timeline: &str) -> Result<()> {
    let generation = i64::try_from(input.generation.get())?;
    let verifier = input.verifier.as_str();
    let client = tokio::time::timeout(Duration::from_secs(3), connect(config))
        .await
        .map_err(|_| anyhow::anyhow!("service login connection deadline"))?
        .map_err(|_| anyhow::anyhow!("private service login connection unavailable"))?;
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        client.batch_execute(
            "SET search_path=''; SET statement_timeout='8s'; SET lock_timeout='2s';
             SET log_statement='none'; SET log_min_duration_statement=-1;
             SET log_min_duration_sample=-1; SET log_parameter_max_length=0;
             SET log_parameter_max_length_on_error=0; SET log_error_verbosity='terse';
             SET debug_print_parse=off; SET debug_print_plan=off; SET debug_print_rewritten=off;
             SET auto_explain.log_min_duration=-1; SET pgaudit.log='none'; SET pgaudit.role='';
             SET pgaudit.log_parameter=off; SET pg_stat_statements.track='none'; SET neon.forward_ddl=false;"
        ).await?;
        match input.service {
            Service::Function => {
                client.batch_execute(include_str!("../../dsg_function_login.sql")).await?;
                client.query_one("SELECT pg_temp.dsga_apply_function_login($1,$2,$3,$4,$5,$6,$7,$8)",
                    &[&input.project_id, &input.branch_id, &tenant, &timeline, &generation, &verifier,
                      &input.function_id, &input.schema]).await
            }
            service => {
                let (sql, function) = if service == Service::ManagedAuth {
                    (include_str!("../../dsg_service_login.sql"), "SELECT pg_temp.dsga_apply_auth_login($1,$2,$3,$4,$5,$6)")
                } else {
                    (include_str!("../../dsg_data_api_login.sql"), "SELECT pg_temp.dsga_apply_data_api_login($1,$2,$3,$4,$5,$6)")
                };
                client.batch_execute(sql).await?;
                client.query_one(function,
                    &[&input.project_id, &input.branch_id, &tenant, &timeline, &generation, &verifier]).await
            }
        }
    }).await;
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => anyhow::bail!(
            "service login SQLSTATE {}",
            error.code().map_or("connection", |code| code.code())
        ),
        Err(_) => {
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                client.cancel_token().cancel_query(NoTls),
            )
            .await;
            anyhow::bail!("service login SQL deadline")
        }
    }
}
