\echo Use "ALTER EXTENSION neon UPDATE TO '1.9'" to load this file. \quit

-- 移除只读入口不改变准入、原回执或外层在途预算。
DROP FUNCTION neon_dsg_admission_status(text);
