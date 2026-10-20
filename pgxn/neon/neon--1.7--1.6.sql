\echo Use "ALTER EXTENSION neon UPDATE TO '1.6'" to load this file. \quit

-- 只移除SQL入口；不伪造撤销已经发生的容量变化，不清除共享历史/预算事实。
DROP FUNCTION neon_dsg_capabilities();
DROP FUNCTION neon_dsg_memory_status();
DROP FUNCTION neon_dsg_request_resize(text);
DROP FUNCTION neon_dsg_resize_status(text);
DROP FUNCTION neon_dsg_cancel_resize(text);
