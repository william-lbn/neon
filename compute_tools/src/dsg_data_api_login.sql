CREATE OR REPLACE FUNCTION pg_temp.dsga_apply_data_api_login(
    project_id text, branch_id text, tenant_id text, timeline_id text,
    generation bigint, verifier text
) RETURNS void
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog
AS $apply$
DECLARE
    target_db oid;
    login_name text := 'control_da_' || substr(branch_id,4);
    app_name text := 'app_da_' || substr(branch_id,4);
    role_tag text := 'neon-control:data-api:' || project_id || ':' || branch_id;
    marker_name text;
    marker_prefix text;
    stored_tag text;
    request_tag text;
    parts text[];
    old_generation bigint;
    old_login_oid oid;
    old_app_oid oid;
    login_record pg_authid%ROWTYPE;
    app_record pg_authid%ROWTYPE;
    marker pg_authid%ROWTYPE;
BEGIN
    IF session_user <> 'cloud_admin' OR current_user <> 'cloud_admin'
       OR pg_is_in_recovery()
       OR current_setting('neon.project_id',true) IS DISTINCT FROM project_id
       OR current_setting('neon.branch_id',true) IS DISTINCT FROM branch_id
       OR current_setting('neon.tenant_id',true) IS DISTINCT FROM tenant_id
       OR current_setting('neon.timeline_id',true) IS DISTINCT FROM timeline_id
       OR project_id !~ '^prj_[0-9a-f]{16}$' OR branch_id !~ '^br_[0-9a-f]{16}$'
       OR generation < 1 THEN
        RAISE insufficient_privilege;
    END IF;
    SELECT oid INTO STRICT target_db FROM pg_database
        WHERE datname=current_database() AND datallowconn AND NOT datistemplate;
    IF verifier IS NOT NULL AND (
        NOT has_database_privilege('control_probe',target_db,'CREATE')
        OR NOT has_database_privilege('control_probe',target_db,'CONNECT WITH GRANT OPTION')
    ) THEN
        RAISE insufficient_privilege;
    END IF;

    LOCK TABLE pg_authid IN SHARE ROW EXCLUSIVE MODE;
    LOCK TABLE pg_auth_members IN SHARE ROW EXCLUSIVE MODE;
    marker_name := 'dsga_data_api_login_' || substr(branch_id,4) || '_' || target_db::text;
    marker_prefix := role_tag || ':' || target_db::text || ':v1:';

    SELECT * INTO marker FROM pg_authid WHERE rolname=marker_name;
    IF FOUND THEN
        stored_tag := shobj_description(marker.oid,'pg_authid');
        IF stored_tag IS NULL OR NOT starts_with(stored_tag,marker_prefix)
           OR substr(stored_tag,length(marker_prefix)+1) !~ '^[0-9]{1,10}:[0-9]{1,10}:[1-9][0-9]{0,18}:(enabled|disabled)$'
           OR marker.rolcanlogin OR marker.rolpassword IS NOT NULL
           OR marker.rolsuper OR marker.rolcreaterole OR marker.rolcreatedb
           OR marker.rolreplication OR marker.rolbypassrls
           OR EXISTS(SELECT FROM pg_auth_members WHERE member=marker.oid OR roleid=marker.oid) THEN
            RAISE insufficient_privilege;
        END IF;
        parts := string_to_array(substr(stored_tag,length(marker_prefix)+1),':');
        old_login_oid := parts[1]::oid;
        old_app_oid := parts[2]::oid;
        old_generation := parts[3]::bigint;
        IF old_generation > generation THEN
            RAISE insufficient_privilege;
        END IF;
    END IF;

    SELECT * INTO login_record FROM pg_authid WHERE rolname=login_name;
    SELECT * INTO app_record FROM pg_authid WHERE rolname=app_name;
    IF login_record.oid IS NOT NULL OR app_record.oid IS NOT NULL THEN
        IF login_record.oid IS NULL OR app_record.oid IS NULL
           OR old_login_oid IS DISTINCT FROM login_record.oid OR old_app_oid IS DISTINCT FROM app_record.oid
           OR shobj_description(login_record.oid,'pg_authid') IS DISTINCT FROM role_tag
           OR shobj_description(app_record.oid,'pg_authid') IS DISTINCT FROM role_tag
           OR login_record.rolinherit OR login_record.rolsuper OR login_record.rolcreaterole
           OR login_record.rolcreatedb OR login_record.rolreplication OR login_record.rolbypassrls
           OR app_record.rolcanlogin OR app_record.rolpassword IS NOT NULL OR app_record.rolinherit
           OR app_record.rolsuper OR app_record.rolcreaterole OR app_record.rolcreatedb
           OR app_record.rolreplication OR app_record.rolbypassrls
           OR (SELECT count(*) FROM pg_auth_members
                 WHERE member=login_record.oid OR roleid=login_record.oid
                    OR member=app_record.oid OR roleid=app_record.oid) <> 1
           OR NOT EXISTS (SELECT FROM pg_auth_members WHERE member=login_record.oid
                 AND roleid=app_record.oid AND NOT inherit_option AND set_option AND NOT admin_option)
           OR (old_generation = generation AND login_record.rolpassword IS DISTINCT FROM verifier) THEN
            RAISE insufficient_privilege;
        END IF;
    ELSIF coalesce(old_login_oid,0) <> 0 OR coalesce(old_app_oid,0) <> 0 THEN
        RAISE insufficient_privilege;
    ELSIF verifier IS NOT NULL THEN
        IF old_generation = generation THEN
            RAISE insufficient_privilege;
        END IF;
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT',login_name);
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT',app_name);
        EXECUTE format('COMMENT ON ROLE %I IS %L',login_name,role_tag);
        EXECUTE format('COMMENT ON ROLE %I IS %L',app_name,role_tag);
        EXECUTE format('GRANT %I TO %I WITH INHERIT FALSE, SET TRUE, ADMIN FALSE',app_name,login_name);
        SELECT * INTO STRICT login_record FROM pg_authid WHERE rolname=login_name;
        SELECT * INTO STRICT app_record FROM pg_authid WHERE rolname=app_name;
    END IF;

    request_tag := marker_prefix || coalesce(login_record.oid,0)::text || ':' ||
        coalesce(app_record.oid,0)::text || ':' || generation::text || ':' ||
        CASE WHEN verifier IS NULL THEN 'disabled' ELSE 'enabled' END;
    IF old_generation = generation AND stored_tag <> request_tag THEN
        RAISE insufficient_privilege;
    END IF;

    IF marker.oid IS NULL THEN
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT',marker_name);
    END IF;
    IF stored_tag IS DISTINCT FROM request_tag THEN
        EXECUTE format('COMMENT ON ROLE %I IS %L',marker_name,request_tag);
    END IF;

    IF verifier IS NULL THEN
        IF login_record.oid IS NOT NULL THEN
            IF login_record.rolcanlogin OR login_record.rolpassword IS NOT NULL THEN
                EXECUTE format('ALTER ROLE %I NOLOGIN PASSWORD NULL',login_name);
            END IF;
            IF EXISTS (
                SELECT FROM pg_database d CROSS JOIN LATERAL aclexplode(d.datacl) a
                 WHERE d.oid=target_db AND a.grantee=login_record.oid AND a.privilege_type='CONNECT'
            ) THEN
                EXECUTE format('REVOKE CONNECT ON DATABASE %I FROM %I',current_database(),login_name);
            END IF;
        END IF;
    ELSE
        IF NOT login_record.rolcanlogin OR login_record.rolpassword IS DISTINCT FROM verifier
           OR login_record.rolconnlimit <> 4 THEN
            EXECUTE format('ALTER ROLE %I LOGIN PASSWORD %L CONNECTION LIMIT 4',login_name,verifier);
        END IF;
        IF NOT EXISTS (SELECT FROM pg_db_role_setting WHERE setrole=login_record.oid AND setdatabase=0
             AND 'idle_session_timeout=5s'=ANY(setconfig)) THEN
            EXECUTE format('ALTER ROLE %I SET idle_session_timeout TO %L',login_name,'5s');
        END IF;
        IF NOT EXISTS (
            SELECT FROM pg_database d CROSS JOIN LATERAL aclexplode(d.datacl) a
             WHERE d.oid=target_db AND a.grantee=login_record.oid AND a.privilege_type='CONNECT'
        ) THEN
            EXECUTE format('GRANT CONNECT ON DATABASE %I TO %I',current_database(),login_name);
        END IF;
    END IF;
END $apply$;
