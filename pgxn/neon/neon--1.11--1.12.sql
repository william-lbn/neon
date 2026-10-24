\echo Use "ALTER EXTENSION neon UPDATE TO '1.12'" to load this file. \quit

-- 同一boot的前台消息/命令/事务与进程生存期观察；不授权完整Quiesce或VM操作。
CREATE FUNCTION neon_dsg_work_state() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_work_state' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_work_state() FROM PUBLIC;
