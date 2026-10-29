-- 原rearm结果独立只读查询；历史成功不授权当前服务。
CREATE FUNCTION neon_dsg_admission_rearm_status(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_admission_rearm_status' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_admission_rearm_status(text) FROM PUBLIC;
