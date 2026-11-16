-- 仅授权原监控角色读取有限回收见证；不开放缓存变更或资源降限权限。
CREATE FUNCTION neon_lfc_reclaim_state() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_lfc_reclaim_state'
LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_lfc_reclaim_state() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION neon_lfc_reclaim_state() TO pg_monitor;
