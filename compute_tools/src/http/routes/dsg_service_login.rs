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
    Realtime,
    CatalogRole,
    CatalogRoleRefresh,
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
    // 只有 Reader 刷新必须携带原 Writer/当前 branch 的存储身份；不能仅自证
    // 目标 Reader 自己的 Timeline，从而误收养同 branch 历史切点的物理角色。
    tenant_id: Option<String>,
    timeline_id: Option<String>,
    pg_boot_id: DsgUint64,
    database: String,
    function_id: Option<String>,
    schema: Option<String>,
    realtime_id: Option<String>,
    relations: Option<Vec<RealtimeRelation>>,
    catalog_role: Option<CatalogRole>,
    generation: DsgUint64,
    // Explicit null disables the exact registered service authorization.
    verifier: serde_json::Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RealtimeRelation {
    schema: String,
    name: String,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CatalogAction {
    Create,
    Rotate,
    Delete,
}

impl CatalogAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Rotate => "rotate",
            Self::Delete => "delete",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRole {
    operation_id: String,
    action: CatalogAction,
    name: String,
    expected_oid: u32,
}

#[derive(Serialize)]
struct CatalogRoleReceipt {
    operation_id: String,
    action: CatalogAction,
    role_oid: u32,
}

fn valid_catalog_role_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && name.len() <= 63
        && name.trim() == name
        && !name.starts_with('/')
        && !name.chars().any(char::is_control)
        && ![
            "app_owner",
            "cloud_admin",
            "postgres",
            "zenith_admin",
            "public",
            "none",
        ]
        .contains(&lower.as_str())
        && ![
            "pg_", "neon_", "control_", "app_da_", "fn_", "rtp_", "rtr_", "dsga_",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

impl Login {
    fn accepts_mode(&self, mode: &ComputeMode) -> bool {
        if self.service == Service::CatalogRoleRefresh {
            *mode == ComputeMode::Replica
        } else {
            *mode == ComputeMode::Primary
        }
    }

    fn matches_storage(&self, tenant: &str, timeline: &str) -> bool {
        self.service != Service::CatalogRoleRefresh
            || (self.tenant_id.as_deref() == Some(tenant)
                && self.timeline_id.as_deref() == Some(timeline))
    }

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
        ensure!(
            matches!(
                self.service,
                Service::CatalogRole | Service::CatalogRoleRefresh
            ) == self.catalog_role.is_some(),
            "invalid catalog role action scope"
        );
        ensure!(
            if self.service == Service::CatalogRoleRefresh {
                [self.tenant_id.as_deref(), self.timeline_id.as_deref()]
                    .iter()
                    .all(|value| {
                        value.is_some_and(|value| {
                            value.len() == 32
                                && value
                                    .bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        })
                    })
            } else {
                self.tenant_id.is_none() && self.timeline_id.is_none()
            },
            "invalid Reader catalog storage scope"
        );
        let valid_scope = match self.service {
            Service::Function => {
                self.function_id
                    .as_deref()
                    .is_some_and(|id| valid_id(id, "fnc_"))
                    && self.schema.as_deref().is_some_and(valid_function_schema)
                    && self.realtime_id.is_none()
                    && self.relations.is_none()
            }
            Service::Realtime => {
                self.function_id.is_none()
                    && self.schema.is_none()
                    && self
                        .realtime_id
                        .as_deref()
                        .zip(self.relations.as_deref())
                        .is_some_and(|(id, relations)| {
                            (valid_realtime_id(id, "rts_") && relations.is_empty())
                                || (valid_realtime_id(id, "rtq_")
                                    && (1..=32).contains(&relations.len())
                                    && relations.iter().all(|relation| {
                                        valid_function_schema(&relation.schema)
                                            && valid_identifier(&relation.name)
                                    }))
                        })
            }
            Service::CatalogRole | Service::CatalogRoleRefresh => {
                self.function_id.is_none()
                    && self.schema.is_none()
                    && self.realtime_id.is_none()
                    && self.relations.is_none()
                    && self.catalog_role.as_ref().is_some_and(|role| {
                        valid_realtime_id(&role.operation_id, "op_")
                            && valid_catalog_role_name(&role.name)
                            && (if self.service == Service::CatalogRoleRefresh {
                                self.database == "postgres" && role.expected_oid > 0
                            } else {
                                (role.action == CatalogAction::Create) == (role.expected_oid == 0)
                            })
                            && (role.action == CatalogAction::Delete) == self.verifier.is_null()
                    })
            }
            _ => {
                self.function_id.is_none()
                    && self.schema.is_none()
                    && self.realtime_id.is_none()
                    && self.relations.is_none()
            }
        };
        ensure!(valid_scope, "invalid service login scope");
        Ok(())
    }
}

fn valid_realtime_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|id| {
        id.len() == 24
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn valid_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|id| {
        id.len() == 16
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn valid_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 63
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn valid_function_schema(name: &str) -> bool {
    valid_identifier(name)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeline_id: Option<String>,
    pg_boot_id: DsgUint64,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    realtime_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_role: Option<CatalogRoleReceipt>,
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
        ensure!(!*_lifecycle, "Compute is draining");
        compute.verify_dsg_running()?;
        let original = {
            let state = compute
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Compute state poisoned"))?;
            state
                .pspec
                .clone()
                .context("Compute configuration unavailable")?
        };
        compute.verify_dsg_storage(&original)?;
        let spec = original.spec;
        let boot = compute
            .frozen_dsg_startup()
            .context("Compute boot unavailable")?;
        ensure!(
            input.accepts_mode(&spec.mode)
                && input.matches_storage(
                    &original.tenant_id.to_string(),
                    &original.timeline_id.to_string()
                )
                && spec.project_id.as_deref() == Some(input.project_id.as_str())
                && spec.branch_id.as_deref() == Some(input.branch_id.as_str())
                && spec.endpoint_id.as_deref() == Some(input.endpoint_id.as_str())
                && claims.vm_uid == boot.vm_uid
                && claims.compute_instance_id == boot.compute_instance_id,
            "service login does not match the current Compute scope"
        );
        runtime
            .block_on(async {
                let observed = observe_boot(&compute, input.pg_boot_id);
                if input.service == Service::CatalogRoleRefresh {
                    // 配置锁下的 Reader 只读观察单次最多 3 秒；不继承资源协调
                    // 请求可等待 70 秒的 SDK 上界，避免挂住普通配置生命周期。
                    tokio::time::timeout(Duration::from_secs(3), observed)
                        .await
                        .map_err(|_| anyhow::anyhow!("Reader boot observation deadline"))??;
                    Ok(())
                } else {
                    observed.await
                }
            })
            .map_err(|_: anyhow::Error| {
                anyhow::anyhow!("service login boot observation unavailable or different")
            })?;
        let mut config = compute.get_tokio_conn_conf(Some("compute_ctl:DSGA_service_login"));
        config.dbname(&input.database);
        // Reader 只能确认原 Writer 事务已复制；显式分支不进入 apply，因此不会
        // 创建临时 DDL 函数、修改角色/密码/登记标记，也不会借此开放 Admission。
        let catalog_oid = if input.service == Service::CatalogRoleRefresh {
            let role = input
                .catalog_role
                .as_ref()
                .context("Reader catalog source missing")?;
            compute.refresh_dsg_reader_catalog(
                &spec,
                &crate::dsg_hba::ReaderCatalogRole {
                    project: &input.project_id,
                    branch: &input.branch_id,
                    tenant: &original.tenant_id.to_string(),
                    timeline: &original.timeline_id.to_string(),
                    generation: input.generation.get(),
                    operation: &role.operation_id,
                    action: role.action.as_str(),
                    name: &role.name,
                    oid: role.expected_oid,
                    verifier: input.verifier.as_str(),
                },
            )?;
            // HBA nonce 只能证明原文件已经加载；等待之后还必须核对原 PG boot。
            // 若重启或读失败，删除本地成功回执，不能将跨 boot 的观察拼接成成功。
            let confirmed = runtime.block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(3),
                    observe_boot(&compute, input.pg_boot_id),
                )
                .await
                .map_err(|_| anyhow::anyhow!("Reader boot observation deadline"))?
            });
            if confirmed.is_err() {
                *compute
                    .dsg_authentication_receipt
                    .write()
                    .map_err(|_| anyhow::anyhow!("DSGA authentication state poisoned"))? = None;
                anyhow::bail!("Reader catalog boot unconfirmed after HBA refresh");
            }
            Some(role.expected_oid)
        } else {
            runtime.block_on(apply(
                &config,
                &input,
                &original.tenant_id.to_string(),
                &original.timeline_id.to_string(),
            ))?
        };
        let catalog_role = match (input.catalog_role, catalog_oid) {
            (Some(role), Some(role_oid)) if role_oid > 0 => Some(CatalogRoleReceipt {
                operation_id: role.operation_id,
                action: role.action,
                role_oid,
            }),
            (None, None) => None,
            _ => anyhow::bail!("catalog role receipt unavailable"),
        };
        if input.service != Service::CatalogRoleRefresh {
            compute.refresh_dsg_hba(&spec, true)?;
        }
        Ok::<_, anyhow::Error>(Receipt {
            protocol_version: 1,
            service: input.service,
            branch_id: input.branch_id,
            tenant_id: input.tenant_id,
            timeline_id: input.timeline_id,
            pg_boot_id: input.pg_boot_id,
            function_id: input.function_id,
            schema: input.schema,
            realtime_id: input.realtime_id,
            catalog_role,
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

/// 只读原生 boot 观察与配置中的 VM/Compute 冻结来源绑定；它不设置门禁。
async fn observe_boot(compute: &ComputeNode, expected: DsgUint64) -> Result<()> {
    let reader = compute.dsg_sql_client().await?;
    let observation = reader.memory_status().await?;
    ensure!(
        observation.observed.instance.pg_boot_id == expected,
        "service login does not match the current PostgreSQL boot"
    );
    Ok(())
}

async fn connect(config: &Config) -> Result<Client> {
    let (client, driver) = config.connect(NoTls).await?;
    tokio::spawn(async move {
        let _ = driver.await;
    });
    Ok(client)
}

async fn apply(
    config: &Config,
    input: &Login,
    tenant: &str,
    timeline: &str,
) -> Result<Option<u32>> {
    ensure!(
        input.service != Service::CatalogRoleRefresh,
        "Reader catalog refresh cannot apply catalog writes"
    );
    let generation = i64::try_from(input.generation.get())?;
    let verifier = input.verifier.as_str();
    let relations = input
        .relations
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
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
        if let Some(role) = &input.catalog_role {
            client.batch_execute(include_str!("../../dsg_catalog_role.sql")).await?;
            let row = client.query_one(
                "SELECT pg_temp.dsga_apply_catalog_role($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                &[&input.project_id, &input.branch_id, &tenant, &timeline, &generation,
                  &role.operation_id, &role.action.as_str(), &role.name, &role.expected_oid, &verifier],
            ).await?;
            return Ok(Some(row.get(0)));
        }
        match input.service {
            Service::Realtime => {
                client.batch_execute(include_str!("../../dsg_realtime_login.sql")).await?;
                client.query_one("SELECT pg_temp.dsga_apply_realtime_login($1,$2,$3,$4,$5,$6,$7,$8::text::jsonb)",
                    &[&input.project_id, &input.branch_id, &tenant, &timeline, &generation, &verifier,
                      &input.realtime_id, &relations]).await
            }
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
        }.map(|_| None)
    }).await;
    match result {
        Ok(Ok(oid)) => Ok(oid),
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

#[cfg(test)]
mod tests {
    use super::Login;
    use serde_json::{Value, json};

    #[test]
    fn catalog_role_authorizes_only_its_original_scope_and_oid() {
        let input = json!({
            "protocol_version": 1, "service": "catalog_role",
            "project_id": "prj_0123456789abcdef", "branch_id": "br_0123456789abcdef",
            "endpoint_id": "ep_0123456789abcdef", "pg_boot_id": "42",
            "catalog_role": {"operation_id": "op_000000000000000000000004",
                             "action": "delete", "name": "customer test", "expected_oid": 16395},
            "database": "postgres", "generation": "3", "verifier": null
        });
        let valid =
            |v: Value| serde_json::from_value::<Login>(v).is_ok_and(|req| req.validate().is_ok());
        assert!(valid(input.clone()));
        for (pointer, delta) in [
            ("/service", json!("function")),
            ("/catalog_role/action", json!("create")),
            ("/catalog_role/expected_oid", json!(0)),
            ("/catalog_role/name", json!("/.*")),
            ("/catalog_role/name", json!("dsga_catalog_role_marker")),
            ("/catalog_role/operation_id", json!("op_wrong")),
        ] {
            let mut modified = input.clone();
            *modified.pointer_mut(pointer).unwrap() = delta;
            assert!(!valid(modified), "accepted {pointer}");
        }
    }

    #[test]
    fn reader_catalog_refresh_requires_original_oid_and_keeps_writer_isolation() {
        use super::{ComputeMode, Service};
        let verifier = format!(
            "SCRAM-SHA-256$4096:{}${}:{}",
            "AQEBAQEBAQEBAQEBAQEBAQ==",
            "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=",
            "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM="
        );
        let base = json!({
            "protocol_version":1,"service":"catalog_role_refresh",
            "project_id":"prj_0123456789abcdef","branch_id":"br_0123456789abcdef",
            "endpoint_id":"ep_0123456789abcdef","pg_boot_id":"42",
            "tenant_id":"11111111111111111111111111111111",
            "timeline_id":"22222222222222222222222222222222",
            "catalog_role":{"operation_id":"op_000000000000000000000004",
                "action":"create","name":"customer test","expected_oid":16395},
            "database":"postgres","generation":"3","verifier":verifier
        });
        let valid =
            |v: Value| serde_json::from_value::<Login>(v).is_ok_and(|r| r.validate().is_ok());
        for action in ["create", "rotate", "delete"] {
            let mut value = base.clone();
            value["catalog_role"]["action"] = json!(action);
            if action == "delete" {
                value["verifier"] = Value::Null;
            }
            let request: Login = serde_json::from_value(value).unwrap();
            request.validate().unwrap();
            assert!(request.accepts_mode(&ComputeMode::Replica));
            assert!(!request.accepts_mode(&ComputeMode::Primary));
            assert!(request.matches_storage(
                "11111111111111111111111111111111",
                "22222222222222222222222222222222"
            ));
            assert!(!request.matches_storage(
                "11111111111111111111111111111111",
                "33333333333333333333333333333333"
            ));
            assert!(!request.matches_storage(
                "44444444444444444444444444444444",
                "22222222222222222222222222222222"
            ));
        }
        for (pointer, change) in [
            ("/catalog_role/expected_oid", json!(0)),
            ("/catalog_role/operation_id", json!("op_other")),
            ("/catalog_role/name", json!("/.*")),
            ("/catalog_role/action", json!("observe")),
            ("/database", json!("tenant_db")),
            ("/generation", json!("0")),
            ("/pg_boot_id", json!("0")),
            ("/tenant_id", Value::Null),
            ("/timeline_id", Value::Null),
            ("/tenant_id", json!("unknown")),
            ("/timeline_id", json!("abcdef")),
            ("/verifier", Value::Null),
            ("/service", json!("catalog_role")),
        ] {
            let mut value = base.clone();
            *value.pointer_mut(pointer).unwrap() = change;
            assert!(!valid(value), "accepted {pointer}");
        }
        let mut writer = base.clone();
        writer.as_object_mut().unwrap().remove("tenant_id");
        writer.as_object_mut().unwrap().remove("timeline_id");
        writer["service"] = json!("catalog_role");
        writer["catalog_role"]["expected_oid"] = json!(0);
        let writer: Login = serde_json::from_value(writer).unwrap();
        writer.validate().unwrap();
        assert!(writer.service == Service::CatalogRole);
        assert!(writer.accepts_mode(&ComputeMode::Primary));
        assert!(!writer.accepts_mode(&ComputeMode::Replica));
    }

    #[tokio::test]
    async fn reader_catalog_refresh_cannot_reach_mutating_apply_even_without_connection() {
        let request: Login = serde_json::from_value(json!({
            "protocol_version":1,"service":"catalog_role_refresh",
            "project_id":"prj_0123456789abcdef","branch_id":"br_0123456789abcdef",
            "endpoint_id":"ep_0123456789abcdef","pg_boot_id":"42",
            "tenant_id":"11111111111111111111111111111111",
            "timeline_id":"22222222222222222222222222222222",
            "catalog_role":{"operation_id":"op_000000000000000000000004",
                "action":"delete","name":"customer","expected_oid":16395},
            "database":"postgres","generation":"3","verifier":null
        }))
        .unwrap();
        request.validate().unwrap();
        let error = super::apply(
            &tokio_postgres::Config::new(),
            &request,
            "tenant",
            "timeline",
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Reader catalog refresh cannot apply catalog writes"
        );
    }
}
