CREATE FUNCTION neon_logical_buffer_demand() RETURNS jsonb
AS 'MODULE_PATHNAME', 'neon_logical_buffer_demand'
LANGUAGE C STRICT VOLATILE PARALLEL UNSAFE;
REVOKE ALL ON FUNCTION neon_logical_buffer_demand() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION neon_logical_buffer_demand() TO pg_monitor;
