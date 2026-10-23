\echo Use "ALTER EXTENSION neon UPDATE TO '1.11'" to load this file. \quit

-- 原生固定主段/DSG数组的去重上界；不执行Provider，不修改准入，不是Guest完整预算。
CREATE FUNCTION neon_dsg_shared_cost() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_shared_cost' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_shared_cost() FROM PUBLIC;
