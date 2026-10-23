//! DSGA 受管 Compute 的角色配置边界。旧 Neon 配置路径保持原语义。
//!
//! 这里保证配置中的应用角色不能成为超级用户或经成员关系取得内部管理身份，
//! 不代替控制面私有凭据签发、HBA/代理路由隔离及完整内存预算。只有这些门禁都
//! 接齐后才允许启用生产。尤其不能把“数据库显示 NOSUPERUSER”当作旧超级用户
//! 会话已被安全降权：发现既有越权角色时拒绝配置，交由受控重建/排空处理。

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::time::Duration;

use anyhow::{Result, bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use compute_api::spec::{ComputeMode, ComputeSpec, Role};
use tokio_postgres::{Client, NoTls};

use crate::pg_helpers::Escaping;

const ADMIN: &str = "cloud_admin";
const MAX_ROLES: usize = 512;
const MAX_SQL: usize = 1024 * 1024;

struct PlannedRole<'a> {
    role: &'a Role,
    options: BTreeMap<&'static str, String>,
}

struct RoleQuery {
    definition: String,
    passwords: Vec<Option<String>>,
}

fn ordinary_name(name: &str) -> bool {
    // PG17 HBA对以/开头的role token仍编译正则，即使token加了双引号。
    // 受管角色要进入字面白名单，故拒绝此无法用普通引号表达的前缀，防止
    // JWT回环trust白名单被/.*扩大为包括管理SU的任意角色。
    if name.is_empty()
        || name.len() > 63
        || name.starts_with('/')
        || name.chars().any(char::is_control)
    {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    !lower.starts_with("pg_")
        && ![
            ADMIN,
            "postgres",
            "zenith_admin",
            "neon_superuser",
            "public",
            "none",
        ]
        .contains(&lower.as_str())
}

/// 所有值最终仍转义。额外解析 SCRAM 是为了拒绝旧 MD5、畸形/超大 verifier 和
/// 将任意 SQL 偷渡到旧 RoleExt 的输入，不把字符串前缀当作可信密码格式。
fn scram(value: &str) -> bool {
    if value.len() > 512 {
        return false;
    }
    let Some(rest) = value.strip_prefix("SCRAM-SHA-256$") else {
        return false;
    };
    let Some((cost_salt, keys)) = rest.split_once('$') else {
        return false;
    };
    let Some((cost, salt)) = cost_salt.split_once(':') else {
        return false;
    };
    let Some((stored, server)) = keys.split_once(':') else {
        return false;
    };
    !cost.is_empty()
        && cost.bytes().all(|b| b.is_ascii_digit())
        && cost
            .parse::<u32>()
            .is_ok_and(|n| (4096..=1_000_000).contains(&n))
        && STANDARD
            .decode(salt)
            .is_ok_and(|s| (16..=64).contains(&s.len()))
        && [stored, server]
            .iter()
            .all(|s| STANDARD.decode(s).is_ok_and(|bytes| bytes.len() == 32))
}

fn literal(value: &str) -> String {
    // E-string 在 standard_conforming_strings 为 on/off 时语义相同；反斜线和
    // 单引号都处理，角色名允许引号/Unicode，不依赖数据库级 search_path/GUC。
    format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

fn plan(spec: &ComputeSpec) -> Result<Vec<PlannedRole<'_>>> {
    ensure!(
        !spec.cluster.roles.is_empty() && spec.cluster.roles.len() <= MAX_ROLES,
        "DSGA managed role count is outside the supported bound"
    );
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for role in &spec.cluster.roles {
        ensure!(seen.insert(&role.name), "duplicate DSGA role");
        // 与官方Compute约定一致：roles仅包含业务身份，cloud_admin由本地内部
        // 管理通道提供，绝不能把租户提交的口令写到超级用户。旧混用配置明确拒绝。
        ensure!(
            ordinary_name(&role.name),
            "reserved or invalid DSGA application role"
        );
        ensure!(
            role.encrypted_password.as_deref().is_none_or(scram),
            "DSGA application password must be a bounded SCRAM verifier"
        );
        let mut options = BTreeMap::new();
        ensure!(
            role.options.as_ref().is_none_or(|o| o.len() <= 12),
            "too many DSGA role options"
        );
        for option in role.options.iter().flatten() {
            ensure!(
                option.name.len() <= 32 && option.value.as_ref().is_none_or(|v| v.len() <= 256),
                "DSGA role option is too large"
            );
            // 不将 GenericOption 的 name/vartype 当作 SQL 片段使用。布尔标志和值
            // 分别解析，正/负标志共享同一键，重复及相互矛盾的权限意图都拒绝。
            let upper = option.name.to_ascii_uppercase();
            let (key, sql) = match upper.as_str() {
                "NOSUPERUSER" if option.value.is_none() => ("SUPERUSER", upper),
                "INHERIT" | "NOINHERIT" | "CREATEDB" | "NOCREATEDB" | "CREATEROLE"
                | "NOCREATEROLE" | "REPLICATION" | "NOREPLICATION" | "BYPASSRLS"
                | "NOBYPASSRLS" | "LOGIN" | "NOLOGIN"
                    if option.value.is_none() =>
                {
                    let key = match upper.trim_start_matches("NO") {
                        "INHERIT" => "INHERIT",
                        "CREATEDB" => "CREATEDB",
                        "CREATEROLE" => "CREATEROLE",
                        "REPLICATION" => "REPLICATION",
                        "BYPASSRLS" => "BYPASSRLS",
                        "LOGIN" => "LOGIN",
                        _ => unreachable!(),
                    };
                    (key, upper)
                }
                "CONNECTION LIMIT" => {
                    let n = option.value.as_deref().and_then(|s| s.parse::<i32>().ok());
                    ensure!(
                        n.is_some_and(|n| n >= -1),
                        "invalid DSGA role connection limit"
                    );
                    (
                        "CONNECTION LIMIT",
                        format!("CONNECTION LIMIT {}", n.unwrap()),
                    )
                }
                "VALID UNTIL" => {
                    let value = option.value.as_deref().unwrap_or_default();
                    ensure!(
                        value == "infinity" || chrono::DateTime::parse_from_rfc3339(value).is_ok(),
                        "invalid DSGA role validity"
                    );
                    ("VALID UNTIL", format!("VALID UNTIL {}", literal(value)))
                }
                _ => bail!("unsupported or unsafe DSGA role option"),
            };
            ensure!(
                options.insert(key, sql).is_none(),
                "duplicate DSGA role option"
            );
        }
        result.push(PlannedRole { role, options });
    }
    for database in &spec.cluster.databases {
        ensure!(
            ordinary_name(&database.owner) && seen.contains(&database.owner),
            "DSGA application database owner must be a declared ordinary role"
        );
    }
    for op in spec.delta_operations.iter().flatten() {
        if matches!(op.action.as_str(), "delete_role" | "rename_role") {
            ensure!(
                ordinary_name(&op.name),
                "DSGA cannot drop or rename a reserved role"
            );
            if op.action == "rename_role" {
                ensure!(
                    op.new_name.as_deref().is_some_and(ordinary_name),
                    "invalid DSGA role rename"
                );
            }
        }
    }
    ensure!(
        spec.local_proxy_config
            .as_ref()
            .and_then(|p| p.jwks.as_ref())
            .is_none_or(|j| j.len() <= 32),
        "too many DSGA JWT role mappings"
    );
    for jwks in spec
        .local_proxy_config
        .iter()
        .flat_map(|p| p.jwks.iter().flatten())
    {
        ensure!(
            !jwks.role_names.is_empty() && jwks.role_names.len() <= MAX_ROLES,
            "DSGA JWT requires an explicit role allowlist"
        );
        for name in &jwks.role_names {
            ensure!(
                ordinary_name(name) && seen.contains(name),
                "DSGA JWT cannot select an internal or undeclared role"
            );
        }
    }
    Ok(result)
}

pub(crate) fn validate_spec(spec: &ComputeSpec) -> Result<()> {
    if spec.dynamic_shared_buffers.is_some() {
        ensure!(
            spec.mode != ComputeMode::Primary || !spec.skip_pg_catalog_updates,
            "DSGA primary cannot skip mandatory role application"
        );
        plan(spec)?;
    }
    Ok(())
}

pub(crate) fn validate_parameters(spec: &ComputeSpec, weak: &str, lakebase: bool) -> Result<()> {
    if spec.dynamic_shared_buffers.is_none() {
        return Ok(());
    }
    // 旧 CreatePrivilegedRole 同时在 SQL 字符串/标识符中使用这个 CLI 配置。
    // 在其执行之前收窄到无转义歧义的内部名字，避免后面的租户门禁来得太晚。
    ensure!(
        !lakebase
            && spec.databricks_settings.is_none()
            && !weak.is_empty()
            && weak.len() <= 63
            && weak
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            && !weak.as_bytes()[0].is_ascii_digit()
            && !weak.starts_with("pg_")
            && ![ADMIN, "postgres", "zenith_admin", "public"].contains(&weak),
        "unsupported DSGA privileged role or lakebase mode"
    );
    ensure!(
        spec.cluster.roles.iter().all(|r| r.name != weak),
        "DSGA application cannot configure the internal group"
    );
    Ok(())
}

fn graph_guard(name: &str) -> String {
    format!(
        "IF EXISTS (WITH RECURSIVE ancestors(oid) AS (\
          SELECT oid FROM pg_catalog.pg_authid WHERE rolname = {} UNION \
          SELECT m.roleid FROM pg_catalog.pg_auth_members m JOIN ancestors a ON a.oid = m.member) \
          SELECT 1 FROM ancestors a JOIN pg_catalog.pg_authid r ON r.oid = a.oid \
          WHERE r.rolsuper OR r.rolname IN ('cloud_admin','postgres','zenith_admin',\
          'pg_read_server_files','pg_write_server_files','pg_execute_server_program')) THEN \
          RAISE EXCEPTION 'DSGA role has a protected privilege path' USING ERRCODE='42501'; END IF;\n",
        literal(name)
    )
}

fn option_conditions(options: &BTreeMap<&'static str, String>) -> Vec<String> {
    let mut conditions = Vec::new();
    for (key, sql) in options {
        let column = match *key {
            "SUPERUSER" => continue, // 权限图单独检查，不能用显式NOSUPERUSER代替。
            "INHERIT" => "rolinherit",
            "CREATEDB" => "rolcreatedb",
            "CREATEROLE" => "rolcreaterole",
            "REPLICATION" => "rolreplication",
            "BYPASSRLS" => "rolbypassrls",
            "LOGIN" => "rolcanlogin",
            "CONNECTION LIMIT" => "rolconnlimit",
            "VALID UNTIL" => "rolvaliduntil",
            _ => unreachable!(),
        };
        let value = match *key {
            "CONNECTION LIMIT" => sql.strip_prefix("CONNECTION LIMIT ").unwrap().to_owned(),
            "VALID UNTIL" => format!(
                "{}::pg_catalog.timestamptz",
                sql.strip_prefix("VALID UNTIL ").unwrap()
            ),
            _ => (!sql.starts_with("NO")).to_string(),
        };
        conditions.push(format!("{column} IS DISTINCT FROM {value}"));
    }
    conditions
}

fn query(spec: &ComputeSpec, weak: &str) -> Result<RoleQuery> {
    let roles = plan(spec)?;
    // 受管JWT身份只从同一份已验证spec派生，不接受旧phase传入的独立缓存集合。
    // 缓存漏项会把新JWT角色误当成普通owner，隐式授予BYPASSRLS/内部弱组权限。
    let jwks = spec
        .local_proxy_config
        .iter()
        .flat_map(|p| p.jwks.iter().flatten())
        .flat_map(|j| j.role_names.iter().map(String::as_str))
        .collect::<HashSet<_>>();
    let mut body = String::from(
        "BEGIN\nPERFORM pg_catalog.set_config('search_path','',true);\n\
         PERFORM pg_catalog.set_config('lock_timeout','2s',true);\n\
         LOCK TABLE pg_catalog.pg_authid IN SHARE ROW EXCLUSIVE MODE;\n\
         LOCK TABLE pg_catalog.pg_auth_members IN SHARE ROW EXCLUSIVE MODE;\n\
         IF session_user <> 'cloud_admin' OR current_user <> 'cloud_admin' OR NOT EXISTS \
         (SELECT 1 FROM pg_catalog.pg_authid WHERE rolname='cloud_admin' AND rolsuper AND rolcanlogin) THEN \
         RAISE EXCEPTION 'DSGA role apply requires the private administrator session' USING ERRCODE='42501'; END IF;\n\
         ",
    );
    // 锁序固定为 authid -> auth_members；锁覆盖读取、所有角色DDL及成员检查。
    // SHARE ROW EXCLUSIVE 与其他角色写入互斥，不能在检查后被另一条GRANT穿透。
    // UNION按OID去重，循环授权不会无限递归；不忽略NOINHERIT/SET标记的未来风险。
    writeln!(
        body,
        "IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_authid WHERE rolname={}) THEN RAISE EXCEPTION 'DSGA internal group is missing'; END IF;",
        literal(weak)
    )?;
    body.push_str(&graph_guard(weak));
    for role in &roles {
        body.push_str(&graph_guard(&role.role.name));
    }
    // 官方管理身份不使用PG口令。历史cloud_admin租户口令只能在同一原子事务
    // 内清除；失败须连同角色DDL回滚。清除口令不能撤销已认证超级用户会话，
    // 所以这不替代控制面受控重建/排空、HBA和原始启动来源验收。其他超级用户
    // 出现口令时拒绝，不能擅自替换未知运维凭据后宣称迁移安全。
    body.push_str("IF EXISTS (SELECT 1 FROM pg_catalog.pg_authid WHERE rolsuper AND rolname <> 'cloud_admin' AND rolpassword IS NOT NULL) THEN RAISE EXCEPTION 'DSGA privileged password requires controlled migration' USING ERRCODE='42501'; END IF;\nIF EXISTS (SELECT 1 FROM pg_catalog.pg_authid WHERE rolname='cloud_admin' AND rolpassword IS NOT NULL) THEN ALTER ROLE cloud_admin PASSWORD NULL; END IF;\n");
    let mut passwords = Vec::with_capacity(roles.len());
    for planned in roles {
        let role = planned.role;
        let name = role.name.pg_quote();
        let key = literal(&role.name);
        passwords.push(role.encrypted_password.clone());
        let password = format!("_passwords[{}]", passwords.len());
        let conditions = option_conditions(&planned.options);
        let flags = planned
            .options
            .iter()
            .filter(|(key, _)| **key != "SUPERUSER")
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let mut new_options = planned.options;
        new_options.insert("SUPERUSER", "NOSUPERUSER".into());
        for default in if jwks.contains(role.name.as_str()) {
            vec!["INHERIT", "LOGIN"]
        } else {
            vec![
                "INHERIT",
                "CREATEROLE",
                "CREATEDB",
                "BYPASSRLS",
                "REPLICATION",
                "LOGIN",
            ]
        } {
            new_options.entry(default).or_insert_with(|| default.into());
        }
        let new_flags = new_options.values().cloned().collect::<Vec<_>>().join(" ");
        let membership = if jwks.contains(role.name.as_str()) {
            String::new()
        } else {
            format!(" IN ROLE {}", weak.to_owned().pg_quote())
        };
        // 既有角色仅执行显式权限选项；密码不变也检查越权图，不因密码相同漏过
        // 漂移。旧角色不补授CREATEROLE/CREATEDB/组权限，保留官方特别警告的语义。
        writeln!(
            body,
            "IF EXISTS (SELECT 1 FROM pg_catalog.pg_authid WHERE rolname={key}) THEN"
        )?;
        if !conditions.is_empty() {
            writeln!(
                body,
                "IF EXISTS (SELECT 1 FROM pg_catalog.pg_authid WHERE rolname={key} AND ({})) THEN ALTER ROLE {name} NOSUPERUSER {flags}; END IF;",
                conditions.join(" OR ")
            )?;
        }
        writeln!(
            body,
            "\
            IF (SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname={key}) IS DISTINCT FROM {password} THEN\n\
            EXECUTE pg_catalog.format('ALTER ROLE %I %s PASSWORD %L',{key},{},{password}); END IF;\n\
            ELSE EXECUTE pg_catalog.format('CREATE ROLE %I %s%s PASSWORD %L',{key},{},{},{password}); END IF;",
            literal(new_options.get("LOGIN").unwrap()),
            literal(&new_flags),
            literal(&membership)
        )?;
        body.push_str(&graph_guard(&role.name));
        ensure!(body.len() <= MAX_SQL, "DSGA role plan is too large");
    }
    body.push_str("END;");
    // 临时SECURITY INVOKER函数正文不嵌入verifier；真正执行用扩展协议参数。
    // 这样pg_read_all_stats看不到私有hash，且单个函数调用仍原子回滚。不能在
    // 共享&Client上手动BEGIN。临时函数不提升调用身份，正文仍验证原session。
    Ok(RoleQuery {
        definition: format!(
            "CREATE OR REPLACE FUNCTION pg_temp.dsga_managed_role_apply(_passwords pg_catalog.text[]) RETURNS void LANGUAGE plpgsql SECURITY INVOKER AS {};",
            body.pg_quote_dollar().0
        ),
        passwords,
    })
}

pub(crate) async fn apply(client: &Client, spec: &ComputeSpec, weak: &str) -> Result<Vec<Role>> {
    ensure!(
        spec.dynamic_shared_buffers.is_some() && spec.mode == ComputeMode::Primary,
        "DSGA role apply requires managed primary mode"
    );
    let sql = query(spec, weak)?;
    execute_and_snapshot(client, &sql).await
}

/// RO/静态副本不修改角色，不以主库的成功回执推断本副本已回放。成员图、密码和
/// 属性在一个MVCC快照内读取；缺角色、回放滞后、源角色越权或内部身份不匹配均
/// 拒绝就绪。RO业务verifier来自源timeline，管理身份必须继承无PG口令状态；
/// 不能在副本虚构新密码，也不能把主库成功当作本副本已完成回放。
pub(crate) async fn attest_read_only(
    client: &Client,
    spec: &ComputeSpec,
    weak: &str,
) -> Result<()> {
    ensure!(
        spec.mode != ComputeMode::Primary,
        "DSGA read-only attestation requires replica mode"
    );
    attest(client, spec, weak).await
}

/// 主库初始化回执与RO回放核验共用只读目录证明，不在健康检查里执行ALTER ROLE。
/// 权限图/verifier/属性/内部无密码身份在同一MVCC快照内检查；实际恢复状态必须
/// 匹配冻结的模式。主库不能借RO的回执，RO也不能借主库角色应用成功越过回放。
pub(crate) async fn attest(client: &Client, spec: &ComputeSpec, weak: &str) -> Result<()> {
    ensure!(
        spec.dynamic_shared_buffers.is_some(),
        "DSGA role attestation requires managed mode"
    );
    validate_spec(spec)?;
    validate_parameters(spec, weak, false)?;
    let planned = plan(spec)?;
    let mut names = planned
        .iter()
        .map(|p| p.role.name.clone())
        .collect::<Vec<_>>();
    names.push(weak.to_owned());
    names.push(ADMIN.to_owned());
    let cases = planned
        .iter()
        .filter_map(|p| {
            let conditions = option_conditions(&p.options);
            (!conditions.is_empty()).then(|| {
                format!(
                    "WHEN r.rolname = {} THEN ({})",
                    literal(&p.role.name),
                    conditions.join(" OR ")
                )
            })
        })
        .collect::<Vec<_>>();
    let drift = if cases.is_empty() {
        "false".to_owned()
    } else {
        format!("CASE {} ELSE false END", cases.join(" "))
    };
    let sql = format!(
        "WITH RECURSIVE roots AS (SELECT * FROM pg_catalog.pg_authid WHERE rolname::pg_catalog.text = ANY($1::pg_catalog.text[])), \
        ancestors(root_oid,oid) AS (SELECT oid,oid FROM roots UNION SELECT a.root_oid,m.roleid FROM ancestors a JOIN pg_catalog.pg_auth_members m ON m.member=a.oid) \
        SELECT r.rolname,r.rolpassword,r.rolsuper,r.rolcanlogin,session_user::pg_catalog.text,current_user::pg_catalog.text,pg_catalog.pg_is_in_recovery(), \
        EXISTS(SELECT 1 FROM ancestors a JOIN pg_catalog.pg_authid p ON p.oid=a.oid WHERE a.root_oid=r.oid AND (p.rolsuper OR p.rolname IN ('cloud_admin','postgres','zenith_admin','pg_read_server_files','pg_write_server_files','pg_execute_server_program'))) AS protected, \
        {drift} AS drift, EXISTS(SELECT 1 FROM pg_catalog.pg_authid WHERE rolsuper AND rolpassword IS NOT NULL) AS privileged_password FROM roots r"
    );
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        // 专用管理连接仍主动归零search_path，避免数据库级默认值让同名运算符
        // 参与权限判断；SET不是目录写入，真实hot standby也允许。
        client.simple_query("SET search_path=''").await?;
        client.query(&sql, &[&names]).await
    })
    .await;
    let rows = match result {
        Ok(Ok(rows)) => rows,
        Ok(Err(_)) => bail!("DSGA read-only role snapshot unavailable"),
        Err(_) => {
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                client.cancel_token().cancel_query(NoTls),
            )
            .await;
            bail!("DSGA read-only role snapshot deadline exceeded");
        }
    };
    ensure!(
        rows.len() == names.len(),
        "DSGA replica role snapshot is incomplete"
    );
    for row in rows {
        let name: String = row.get(0);
        ensure!(
            row.get::<_, String>(4) == ADMIN
                && row.get::<_, String>(5) == ADMIN
                && row.get::<_, bool>(6) == (spec.mode != ComputeMode::Primary)
                && !row.get::<_, bool>(9),
            "DSGA replica requires its private administrator and actual recovery mode"
        );
        if name == ADMIN {
            ensure!(
                row.get::<_, bool>(2)
                    && row.get::<_, bool>(3)
                    && row.get::<_, Option<String>>(1).is_none(),
                "DSGA replica administrator is invalid"
            );
        } else {
            ensure!(
                !row.get::<_, bool>(7) && !row.get::<_, bool>(8),
                "DSGA replica role privilege drift"
            );
        }
        if let Some(p) = planned.iter().find(|p| p.role.name == name) {
            ensure!(
                row.get::<_, Option<String>>(1) == p.role.encrypted_password,
                "DSGA replica role verifier differs from frozen source"
            );
        }
    }
    Ok(())
}

async fn execute_and_snapshot(client: &Client, sql: &RoleQuery) -> Result<Vec<Role>> {
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        // 私有控制连接不供租户复用。先关闭该连接的SQL/参数/嵌套统计记录，再
        // 传入任何verifier；错误只返回SQLSTATE，PG日志terse也不输出动态DDL的
        // CONTEXT密码字面量。结构定义可以记录，但定义中没有密码。
        client.simple_query("SET log_statement='none'; SET log_min_duration_statement=-1; SET log_min_duration_sample=-1; SET log_parameter_max_length=0; SET log_parameter_max_length_on_error=0; SET log_error_verbosity='terse'; SET debug_print_parse=off; SET debug_print_plan=off; SET debug_print_rewritten=off; SET auto_explain.log_min_duration=-1; SET pgaudit.log='none'; SET pgaudit.role=''; SET pgaudit.log_parameter=off; SET pg_stat_statements.track='none'; SET neon.forward_ddl=false;").await?;
        client.simple_query(&sql.definition).await?;
        client.query_one("SELECT pg_temp.dsga_managed_role_apply($1::pg_catalog.text[])", &[&sql.passwords]).await
    }).await;
    match result {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => bail!(
            "DSGA role transaction failed (SQLSTATE {})",
            error.code().map(|c| c.code()).unwrap_or("connection")
        ),
        Err(_) => {
            // 截止期限后取消也有独立上界。取消丢回复/提交结果未知都返回失败，
            // 不发布本地ctx成功状态；上层只能重读同一配置事实，不能猜测降权已完成。
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                client.cancel_token().cancel_query(NoTls),
            )
            .await;
            bail!("DSGA role transaction deadline exceeded; result requires reconciliation");
        }
    }
    // 只有完整提交且权威读取成功后才能替换Rust缓存；不使用旧helper的row.ok()
    // 过滤来吞掉部分读取错误，也不在任何网络await期间持有ctx的RwLock。
    let rows = tokio::time::timeout(
        Duration::from_secs(5),
        client.query("SELECT rolname, rolpassword FROM pg_catalog.pg_authid", &[]),
    )
    .await;
    let rows = match rows {
        Ok(Ok(rows)) => rows,
        _ => bail!("DSGA committed role snapshot unavailable; reconciliation required"),
    };
    Ok(rows
        .into_iter()
        .map(|row| Role {
            name: row.get(0),
            encrypted_password: row.get(1),
            options: None,
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod tests;
