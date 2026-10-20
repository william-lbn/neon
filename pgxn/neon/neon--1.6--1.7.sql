\echo Use "ALTER EXTENSION neon UPDATE TO '1.7'" to load this file. \quit

-- 内部管理入口默认拒绝PUBLIC；C函数还同时核验当前角色和会话的真实superuser。
-- text必须保留原始JSON，不能在进入C前转jsonb并丢失重复字段证据。
-- PG共享状态不随SQL事务回滚，故函数为VOLATILE/PARALLEL UNSAFE。
CREATE FUNCTION neon_dsg_capabilities() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_capabilities' LANGUAGE C VOLATILE PARALLEL UNSAFE;
CREATE FUNCTION neon_dsg_memory_status() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_memory_status' LANGUAGE C VOLATILE PARALLEL UNSAFE;
CREATE FUNCTION neon_dsg_request_resize(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_request_resize' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
CREATE FUNCTION neon_dsg_resize_status(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_resize_status' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
CREATE FUNCTION neon_dsg_cancel_resize(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_cancel_resize' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;

REVOKE ALL ON FUNCTION neon_dsg_capabilities() FROM PUBLIC;
REVOKE ALL ON FUNCTION neon_dsg_memory_status() FROM PUBLIC;
REVOKE ALL ON FUNCTION neon_dsg_request_resize(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION neon_dsg_resize_status(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION neon_dsg_cancel_resize(text) FROM PUBLIC;
