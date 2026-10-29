-- 原生失联关闭不依赖外层WAL；此入口只供原预算核验后的内部管理调用。
CREATE FUNCTION neon_dsg_admission_lease(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_admission_lease' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_admission_lease(text) FROM PUBLIC;
