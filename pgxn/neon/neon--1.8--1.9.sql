\echo Use "ALTER EXTENSION neon UPDATE TO '1.9'" to load this file. \quit

-- 正式准入只向内部管理会话开放。C入口还核验session身份，错误GRANT或
-- SECURITY DEFINER不能把租户变成管理调用方；不随安装自动打开门禁。
CREATE FUNCTION neon_dsg_set_admission(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_set_admission' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_set_admission(text) FROM PUBLIC;
