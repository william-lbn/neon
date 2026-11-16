CREATE OR REPLACE FUNCTION pg_temp.dsga_apply_catalog_role(
    project_id text, branch_id text, tenant_id text, timeline_id text,
    generation bigint, operation_id text, action text, target_name text,
    expected_oid oid, verifier text
) RETURNS oid
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog
AS $apply$
DECLARE
    scope_tag text := 'neon-control:catalog-role:' || project_id || ':' || branch_id || ':v1:';
    target_hash text := encode(sha256(convert_to(target_name, 'UTF8')), 'hex');
    marker_name text := 'dsga_catalog_role_' || substr(branch_id, 4) || '_' || substr(target_hash, 1, 20);
    role_tag text := 'neon-control/branch/' || branch_id;
    marker_row pg_authid%ROWTYPE;
    target_row pg_authid%ROWTYPE;
    marker_tag text;
    old_action text;
    old_generation bigint;
    old_operation text;
    old_oid oid;
    result_oid oid;
BEGIN
    IF session_user <> 'cloud_admin' OR current_user <> 'cloud_admin'
       OR pg_is_in_recovery()
       OR current_setting('neon.project_id', true) IS DISTINCT FROM project_id
       OR current_setting('neon.branch_id', true) IS DISTINCT FROM branch_id
       OR current_setting('neon.tenant_id', true) IS DISTINCT FROM tenant_id
       OR current_setting('neon.timeline_id', true) IS DISTINCT FROM timeline_id
       OR project_id !~ '^prj_[0-9a-f]{16}$' OR branch_id !~ '^br_[0-9a-f]{16}$'
       OR operation_id IS NULL OR operation_id !~ '^op_[0-9a-f]{24}$'
       OR generation IS NULL OR generation < 1 OR expected_oid IS NULL
       OR target_name IS NULL OR action IS NULL
       OR octet_length(target_name) NOT BETWEEN 1 AND 63 OR target_name <> btrim(target_name)
       OR target_name ~ '^/|[[:cntrl:]]'
       OR lower(target_name) IN ('app_owner', 'cloud_admin', 'postgres', 'zenith_admin', 'public', 'none')
       OR lower(target_name) ~ '^(pg_|neon_|control_|app_da_|fn_|rtp_|rtr_|dsga_)'
       OR action NOT IN ('create', 'rotate', 'delete')
       OR ((action = 'delete') IS DISTINCT FROM (verifier IS NULL))
       OR ((action = 'create') IS DISTINCT FROM (expected_oid = 0)) THEN
        RAISE insufficient_privilege;
    END IF;

    -- 与原服务登录使用同一锁序。对象、回执和身份检查在同一 PG 事务，
    -- 因此 HTTP 回包丢失后只能读回原执行，不能按同名接管另一角色。
    LOCK TABLE pg_authid IN SHARE ROW EXCLUSIVE MODE;
    LOCK TABLE pg_auth_members IN SHARE ROW EXCLUSIVE MODE;
    SELECT * INTO marker_row FROM pg_authid WHERE rolname=marker_name;
    IF FOUND THEN
        marker_tag := shobj_description(marker_row.oid, 'pg_authid');
        IF marker_tag IS NULL OR NOT starts_with(marker_tag, scope_tag)
           OR substr(marker_tag, length(scope_tag) + 1) !~
               '^[1-9][0-9]{0,18}:op_[0-9a-f]{24}:(create|rotate|delete):[1-9][0-9]{0,9}:[0-9a-f]{64}$'
           OR split_part(substr(marker_tag, length(scope_tag) + 1), ':', 5) <> target_hash
           OR marker_row.rolcanlogin OR marker_row.rolpassword IS NOT NULL
           OR marker_row.rolsuper OR marker_row.rolcreaterole OR marker_row.rolcreatedb
           OR marker_row.rolreplication OR marker_row.rolbypassrls OR marker_row.rolinherit
           OR EXISTS (SELECT FROM pg_auth_members WHERE member=marker_row.oid OR roleid=marker_row.oid) THEN
            RAISE insufficient_privilege;
        END IF;
        marker_tag := substr(marker_tag, length(scope_tag) + 1);
        old_generation := split_part(marker_tag, ':', 1)::bigint;
        old_operation := split_part(marker_tag, ':', 2);
        old_action := split_part(marker_tag, ':', 3);
        old_oid := split_part(marker_tag, ':', 4)::oid;
    END IF;

    SELECT * INTO target_row FROM pg_authid WHERE rolname=target_name;
    IF target_row.oid IS NOT NULL AND (
        shobj_description(target_row.oid, 'pg_authid') IS DISTINCT FROM role_tag
        OR target_row.rolsuper OR target_row.rolbypassrls OR target_row.rolreplication
        OR target_row.rolcreaterole OR target_row.rolcreatedb
        OR EXISTS (SELECT FROM pg_auth_members WHERE member=target_row.oid)
    ) THEN
        RAISE insufficient_privilege;
    END IF;
    IF old_generation > generation THEN
        RAISE insufficient_privilege;
    ELSIF old_generation = generation THEN
        IF old_operation <> operation_id OR old_action <> action OR (expected_oid <> 0 AND old_oid <> expected_oid)
           OR (action = 'delete' AND (target_row.oid IS NOT NULL OR EXISTS (SELECT FROM pg_authid WHERE oid=old_oid)))
           OR (action <> 'delete' AND (target_row.oid IS DISTINCT FROM old_oid
                OR target_row.rolpassword IS DISTINCT FROM verifier)) THEN
            RAISE insufficient_privilege;
        END IF;
        RETURN old_oid;
    END IF;

    IF action = 'create' THEN
        IF target_row.oid IS NOT NULL OR (old_action IS NOT NULL AND old_action <> 'delete') THEN
            RAISE insufficient_privilege;
        END IF;
        IF (SELECT count(*) FROM pg_authid WHERE rolcanlogin) >= 128 THEN
            RAISE insufficient_privilege;
        END IF;
        EXECUTE format('CREATE ROLE %I LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
                       target_name, verifier);
        EXECUTE format('COMMENT ON ROLE %I IS %L', target_name, role_tag);
        EXECUTE format('GRANT %I TO control_probe WITH INHERIT TRUE, SET TRUE', target_name);
        SELECT oid INTO STRICT result_oid FROM pg_authid WHERE rolname=target_name;
    ELSE
        IF target_row.oid IS DISTINCT FROM expected_oid OR old_oid IS DISTINCT FROM expected_oid
           OR old_action = 'delete' THEN
            RAISE insufficient_privilege;
        END IF;
        result_oid := target_row.oid;
        IF action = 'rotate' THEN
            EXECUTE format('ALTER ROLE %I PASSWORD %L', target_name, verifier);
        ELSE
            EXECUTE format('DROP ROLE %I', target_name);
        END IF;
    END IF;
    IF marker_row.oid IS NULL THEN
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT', marker_name);
    END IF;
    EXECUTE format('COMMENT ON ROLE %I IS %L', marker_name,
        scope_tag || generation || ':' || operation_id || ':' || action || ':' || result_oid || ':' || target_hash);
    RETURN result_oid;
END $apply$;
