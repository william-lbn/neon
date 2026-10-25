\echo Use "ALTER EXTENSION neon UPDATE TO '1.15'" to load this file. \quit

-- DSM1只读本boot分配前预留与同点工作；不替代完整资源预算/物理释放证据。
CREATE FUNCTION neon_dsg_dsm_state() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_dsg_dsm_state' LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_dsg_dsm_state() FROM PUBLIC;
