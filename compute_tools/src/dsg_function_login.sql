CREATE OR REPLACE FUNCTION pg_temp.dsga_apply_function_login(
    project_id text, branch_id text, tenant_id text, timeline_id text,
    generation bigint, verifier text, function_id text, target_schema text
) RETURNS void
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog
AS $apply$
DECLARE
    target_db oid;
    target_role text := 'fn_' || substr(function_id,5);
    role_tag text := 'neon-control:function:' || project_id || ':' || branch_id || ':' || function_id;
    marker_name text;
    marker_tag text;
    request_tag text;
    stored_tag text;
    old_generation bigint;
    old_login_oid oid;
    login_record pg_authid%ROWTYPE;
    scope_record pg_authid%ROWTYPE;
BEGIN
    IF session_user <> 'cloud_admin' OR current_user <> 'cloud_admin'
       OR pg_is_in_recovery()
       OR current_setting('neon.project_id',true) IS DISTINCT FROM project_id
       OR current_setting('neon.branch_id',true) IS DISTINCT FROM branch_id
       OR current_setting('neon.tenant_id',true) IS DISTINCT FROM tenant_id
       OR current_setting('neon.timeline_id',true) IS DISTINCT FROM timeline_id
       OR project_id !~ '^prj_[0-9a-f]{16}$' OR branch_id !~ '^br_[0-9a-f]{16}$'
       OR function_id !~ '^fnc_[0-9a-f]{16}$'
       OR target_schema !~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'
       OR lower(target_schema) ~ '^(pg_|neon_|control_)' OR lower(target_schema)='information_schema'
       OR generation < 1 THEN
        RAISE insufficient_privilege;
    END IF;
    SELECT oid INTO STRICT target_db FROM pg_database
        WHERE datname=current_database() AND datallowconn AND NOT datistemplate;
    IF verifier IS NOT NULL AND (
        NOT has_database_privilege('control_probe',target_db,'CONNECT WITH GRANT OPTION')
        OR NOT EXISTS(SELECT FROM pg_namespace WHERE nspname=target_schema
            AND has_schema_privilege('control_probe',oid,'USAGE WITH GRANT OPTION'))
    ) THEN
        RAISE insufficient_privilege;
    END IF;

    LOCK TABLE pg_authid IN SHARE ROW EXCLUSIVE MODE;
    LOCK TABLE pg_auth_members IN SHARE ROW EXCLUSIVE MODE;
    marker_name := 'dsga_fn_login_' || substr(branch_id,4) || '_' || substr(function_id,5) || '_' || target_db::text;
    marker_tag := role_tag || ':' || target_db::text || ':v1:' || target_schema || ':';

    SELECT * INTO scope_record FROM pg_authid WHERE rolname=marker_name;
    IF FOUND THEN
        stored_tag := shobj_description(scope_record.oid,'pg_authid');
        IF stored_tag IS NULL OR NOT starts_with(stored_tag,marker_tag)
           OR substr(stored_tag,length(marker_tag)+1) !~ '^[0-9]{1,10}:[1-9][0-9]{0,18}:(enabled|disabled)$'
           OR scope_record.rolcanlogin OR scope_record.rolpassword IS NOT NULL
           OR scope_record.rolsuper OR scope_record.rolcreaterole OR scope_record.rolcreatedb
           OR scope_record.rolreplication OR scope_record.rolbypassrls
           OR EXISTS(SELECT FROM pg_auth_members WHERE member=scope_record.oid OR roleid=scope_record.oid) THEN
            RAISE insufficient_privilege;
        END IF;
        old_login_oid := split_part(substr(stored_tag,length(marker_tag)+1),':',1)::oid;
        old_generation := split_part(substr(stored_tag,length(marker_tag)+1),':',2)::bigint;
        IF old_generation > generation THEN
            RAISE insufficient_privilege;
        END IF;
    END IF;

    SELECT * INTO login_record FROM pg_authid WHERE rolname=target_role;
    IF FOUND THEN
        IF scope_record.oid IS NULL OR old_login_oid IS DISTINCT FROM login_record.oid
           OR shobj_description(login_record.oid,'pg_authid') IS DISTINCT FROM role_tag
           OR login_record.rolinherit OR login_record.rolsuper OR login_record.rolcreaterole
           OR login_record.rolcreatedb OR login_record.rolreplication OR login_record.rolbypassrls
           OR EXISTS (SELECT FROM pg_auth_members WHERE member=login_record.oid OR roleid=login_record.oid)
           OR (old_generation = generation AND login_record.rolpassword IS DISTINCT FROM verifier) THEN
            RAISE insufficient_privilege;
        END IF;
    ELSIF verifier IS NOT NULL THEN
        IF old_generation = generation THEN
            RAISE insufficient_privilege;
        END IF;
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT',target_role);
        EXECUTE format('COMMENT ON ROLE %I IS %L',target_role,role_tag);
        SELECT * INTO STRICT login_record FROM pg_authid WHERE rolname=target_role;
    ELSIF old_login_oid IS NOT NULL AND old_login_oid <> 0 THEN
        RAISE insufficient_privilege;
    END IF;

    request_tag := marker_tag || coalesce(login_record.oid,0)::text || ':' || generation::text || ':' ||
        CASE WHEN verifier IS NULL THEN 'disabled' ELSE 'enabled' END;
    IF old_generation = generation AND stored_tag <> request_tag THEN
        RAISE insufficient_privilege;
    END IF;

    IF scope_record.oid IS NULL THEN
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT',marker_name);
    END IF;
    IF stored_tag IS DISTINCT FROM request_tag THEN
        EXECUTE format('COMMENT ON ROLE %I IS %L',marker_name,request_tag);
    END IF;

    IF verifier IS NULL THEN
        IF login_record.oid IS NOT NULL THEN
            IF login_record.rolcanlogin OR login_record.rolpassword IS NOT NULL THEN
                EXECUTE format('ALTER ROLE %I NOLOGIN PASSWORD NULL',target_role);
            END IF;
            IF EXISTS (
                SELECT FROM pg_database d CROSS JOIN LATERAL aclexplode(d.datacl) a
                 WHERE d.oid=target_db AND a.grantee=login_record.oid AND a.privilege_type='CONNECT'
            ) THEN
                EXECUTE format('REVOKE CONNECT ON DATABASE %I FROM %I',current_database(),target_role);
            END IF;
        END IF;
    ELSE
        IF NOT login_record.rolcanlogin OR login_record.rolpassword IS DISTINCT FROM verifier
           OR login_record.rolconnlimit <> 4 THEN
            EXECUTE format('ALTER ROLE %I LOGIN PASSWORD %L CONNECTION LIMIT 4',target_role,verifier);
        END IF;
        IF NOT EXISTS (SELECT FROM pg_db_role_setting WHERE setrole=login_record.oid AND setdatabase=0
             AND 'idle_session_timeout=5s'=ANY(setconfig)
             AND format('search_path=%I, pg_catalog',target_schema)=ANY(setconfig)
             AND 'statement_timeout=30s'=ANY(setconfig)) THEN
            EXECUTE format('ALTER ROLE %I SET idle_session_timeout TO %L',target_role,'5s');
            EXECUTE format('ALTER ROLE %I SET search_path TO %I, pg_catalog',target_role,target_schema);
            EXECUTE format('ALTER ROLE %I SET statement_timeout TO %L',target_role,'30s');
        END IF;
        IF NOT EXISTS (
            SELECT FROM pg_database d CROSS JOIN LATERAL aclexplode(d.datacl) a
             WHERE d.oid=target_db AND a.grantee=login_record.oid AND a.privilege_type='CONNECT'
        ) THEN
            EXECUTE format('GRANT CONNECT ON DATABASE %I TO %I',current_database(),target_role);
        END IF;
    END IF;
END $apply$;
