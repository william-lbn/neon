\echo Use "ALTER EXTENSION neon UPDATE TO '1.8'" to load this file. \quit

-- 仅移除SQL入口；降级不能变更当前boot准入或撤销在途预算。
DROP FUNCTION neon_dsg_set_admission(text);
