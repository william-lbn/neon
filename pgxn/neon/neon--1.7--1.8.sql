\echo Use "ALTER EXTENSION neon UPDATE TO '1.8'" to load this file. \quit

-- 独立issuer接管：PENDING不是已完成，保留原请求/未知效果；管理员会话和原始
-- 严格JSON由C层再核验。共享代次不随事务回滚，不认证外层租约/预算或VM许可。
CREATE FUNCTION neon_dsg_fence_issuer(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_fence_issuer' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_fence_issuer(text) FROM PUBLIC;
