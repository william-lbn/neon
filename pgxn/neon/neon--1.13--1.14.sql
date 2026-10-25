\echo Use "ALTER EXTENSION neon UPDATE TO '1.14'" to load this file. \quit

-- 独立Prepared1与同点连接/工作；只阻止新运行期PREPARE，恢复仍保留旧意图。
CREATE FUNCTION neon_dsg_prepared_state() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_prepared_state' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_prepared_state() FROM PUBLIC;
