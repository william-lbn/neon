\echo Use "ALTER EXTENSION neon UPDATE TO '1.10'" to load this file. \quit

-- 完整原意图的只读观察，不推进floor/revision、不修改门禁；缺失/淘汰不代表
-- 未执行。仍要求内部管理会话，PUBLIC撤权与C内session复核共同保护。
CREATE FUNCTION neon_dsg_admission_status(text) RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_admission_status' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_admission_status(text) FROM PUBLIC;
