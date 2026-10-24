\echo Use "ALTER EXTENSION neon UPDATE TO '1.13'" to load this file. \quit

-- 独立连接门禁应答及同点Work2；不是完整Quiesce、资源预算或VM许可。
CREATE FUNCTION neon_dsg_connection_state() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_connection_state' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_connection_state() FROM PUBLIC;
