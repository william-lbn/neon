CREATE OR REPLACE FUNCTION pg_temp.dsga_apply_realtime_login(
    project_id text, branch_id text, tenant_id text, timeline_id text,
    generation bigint, verifier text, identity text, relations jsonb
) RETURNS void
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog
AS $apply$
DECLARE
    target_db oid;
    replication boolean := starts_with(identity,'rts_');
    target_role text := (CASE WHEN replication THEN 'rtp_' ELSE 'rtr_' END) || substr(identity,5);
    role_tag text := 'neon-control:realtime:' || project_id || ':' || branch_id || ':' || identity;
    marker_name text;
    marker_tag text;
    stored_tag text;
    desired_tag text;
    old_generation bigint;
    old_login_oid oid;
    login_record pg_authid%ROWTYPE;
    marker_record pg_authid%ROWTYPE;
    binding text;
    item record;
BEGIN
    IF session_user <> 'cloud_admin' OR current_user <> 'cloud_admin'
       OR pg_is_in_recovery()
       OR current_setting('neon.project_id',true) IS DISTINCT FROM project_id
       OR current_setting('neon.branch_id',true) IS DISTINCT FROM branch_id
       OR current_setting('neon.tenant_id',true) IS DISTINCT FROM tenant_id
       OR current_setting('neon.timeline_id',true) IS DISTINCT FROM timeline_id
       OR project_id !~ '^prj_[0-9a-f]{16}$' OR branch_id !~ '^br_[0-9a-f]{16}$'
       OR identity !~ '^rt[sq]_[0-9a-f]{24}$' OR generation < 1
       OR jsonb_typeof(relations) IS DISTINCT FROM 'array' THEN
        RAISE insufficient_privilege;
    END IF;
    IF (replication AND jsonb_array_length(relations)<>0)
       OR (NOT replication AND jsonb_array_length(relations) NOT BETWEEN 1 AND 32)
       OR EXISTS (SELECT 1 FROM jsonb_array_elements(relations) r(value)
                   WHERE jsonb_typeof(value) IS DISTINCT FROM 'object'
                     OR coalesce(value->>'schema','') !~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'
                     OR coalesce(value->>'name','') !~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'
                     OR lower(value->>'schema') ~ '^(pg_|neon_|control_)'
                     OR lower(value->>'schema')='information_schema')
       OR jsonb_array_length(relations) <> (SELECT count(DISTINCT value) FROM jsonb_array_elements(relations) r(value)) THEN
        RAISE insufficient_privilege;
    END IF;
    SELECT oid INTO STRICT target_db FROM pg_database
      WHERE datname=current_database() AND datallowconn AND NOT datistemplate;
    IF verifier IS NOT NULL AND (
        NOT has_database_privilege('control_probe',target_db,'CONNECT WITH GRANT OPTION')
        OR EXISTS (
            SELECT FROM jsonb_to_recordset(relations) AS r(schema text,name text)
            LEFT JOIN pg_namespace n ON n.nspname=r.schema
            LEFT JOIN pg_class c ON c.relnamespace=n.oid AND c.relname=r.name AND c.relkind IN ('r','p')
            WHERE c.oid IS NULL OR NOT has_schema_privilege('control_probe',n.oid,'USAGE WITH GRANT OPTION')
               OR NOT has_table_privilege('control_probe',c.oid,'SELECT WITH GRANT OPTION'))) THEN
        RAISE insufficient_privilege;
    END IF;

    -- The two matching role receipts freeze the original scope and login OID.
    -- A colliding user role or an inherited fork can only be rejected.
    LOCK TABLE pg_authid IN SHARE ROW EXCLUSIVE MODE;
    LOCK TABLE pg_auth_members IN SHARE ROW EXCLUSIVE MODE;
    marker_name := 'dsga_rtl_' || substr(branch_id,4) || '_' || substr(identity,3,1) || substr(identity,5) || '_' || target_db::text;
    binding := encode(sha256(convert_to(relations::text,'UTF8')),'hex');
    marker_tag := role_tag || ':' || target_db::text || ':v1:' || binding || ':';
    SELECT * INTO marker_record FROM pg_authid WHERE rolname=marker_name;
    IF FOUND THEN
        stored_tag := shobj_description(marker_record.oid,'pg_authid');
        IF stored_tag IS NULL OR NOT starts_with(stored_tag,marker_tag)
           OR substr(stored_tag,length(marker_tag)+1) !~ '^[0-9]{1,10}:[1-9][0-9]{0,18}:(enabled|disabled)$'
           OR marker_record.rolcanlogin OR marker_record.rolpassword IS NOT NULL OR marker_record.rolinherit
           OR marker_record.rolsuper OR marker_record.rolcreaterole OR marker_record.rolcreatedb
           OR marker_record.rolreplication OR marker_record.rolbypassrls
           OR EXISTS (SELECT FROM pg_auth_members WHERE member=marker_record.oid OR roleid=marker_record.oid) THEN
            RAISE insufficient_privilege;
        END IF;
        old_login_oid := split_part(substr(stored_tag,length(marker_tag)+1),':',1)::oid;
        old_generation := split_part(substr(stored_tag,length(marker_tag)+1),':',2)::bigint;
        IF old_generation > generation THEN RAISE insufficient_privilege; END IF;
    END IF;
    SELECT * INTO login_record FROM pg_authid WHERE rolname=target_role;
    IF FOUND THEN
        IF old_login_oid IS DISTINCT FROM login_record.oid
           OR shobj_description(login_record.oid,'pg_authid') IS DISTINCT FROM role_tag
           OR login_record.rolinherit OR login_record.rolsuper OR login_record.rolcreaterole
           OR login_record.rolcreatedb OR login_record.rolreplication IS DISTINCT FROM replication OR login_record.rolbypassrls
           OR EXISTS (SELECT FROM pg_auth_members WHERE member=login_record.oid OR roleid=login_record.oid)
           OR (old_generation = generation AND login_record.rolpassword IS DISTINCT FROM verifier) THEN
            RAISE insufficient_privilege;
        END IF;
    ELSIF verifier IS NOT NULL THEN
        IF old_generation IS NOT NULL THEN RAISE insufficient_privilege; END IF;
        EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT %s',target_role,CASE WHEN replication THEN 'REPLICATION' ELSE 'NOREPLICATION' END);
        EXECUTE format('COMMENT ON ROLE %I IS %L',target_role,role_tag);
        SELECT * INTO STRICT login_record FROM pg_authid WHERE rolname=target_role;
    ELSIF old_login_oid IS NOT NULL AND old_login_oid<>0 THEN
        RAISE insufficient_privilege;
    END IF;

    desired_tag := marker_tag || coalesce(login_record.oid,0)::text || ':' || generation::text || ':' ||
       CASE WHEN verifier IS NULL THEN 'disabled' ELSE 'enabled' END;
    IF old_generation = generation AND stored_tag IS DISTINCT FROM desired_tag THEN RAISE insufficient_privilege; END IF;
    IF marker_record.oid IS NULL THEN EXECUTE format('CREATE ROLE %I NOLOGIN NOINHERIT',marker_name); END IF;
    IF stored_tag IS DISTINCT FROM desired_tag THEN EXECUTE format('COMMENT ON ROLE %I IS %L',marker_name,desired_tag); END IF;

    IF login_record.oid IS NULL THEN RETURN; END IF;
    IF verifier IS NULL THEN
        EXECUTE format('ALTER ROLE %I NOLOGIN PASSWORD NULL',target_role);
        EXECUTE format('REVOKE CONNECT ON DATABASE %I FROM %I',current_database(),target_role);
        PERFORM pg_terminate_backend(pid) FROM pg_stat_activity WHERE usesysid=login_record.oid AND datid=target_db;
        RETURN;
    END IF;
    EXECUTE format('ALTER ROLE %I LOGIN PASSWORD %L CONNECTION LIMIT %s',target_role,verifier,
       CASE WHEN replication THEN 2 ELSE 8 END);
    EXECUTE format('GRANT CONNECT ON DATABASE %I TO %I',current_database(),target_role);
    FOR item IN SELECT * FROM jsonb_to_recordset(relations) AS r(schema text,name text) LOOP
        EXECUTE format('GRANT USAGE ON SCHEMA %I TO %I',item.schema,target_role);
        EXECUTE format('GRANT SELECT ON TABLE %I.%I TO %I',item.schema,item.name,target_role);
    END LOOP;
END $apply$;
