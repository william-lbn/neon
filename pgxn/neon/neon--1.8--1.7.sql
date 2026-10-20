\echo Use "ALTER EXTENSION neon UPDATE TO '1.7'" to load this file. \quit

-- 只撤下SQL入口，不能撤销已提升的floor或清掉旧操作/预算；旧客户端必须按实际
-- ABI/功能集合失败关闭。发行构建须同时重编匹配的PG/Neon，不热混用旧结构ABI。
DROP FUNCTION neon_dsg_fence_issuer(text);
