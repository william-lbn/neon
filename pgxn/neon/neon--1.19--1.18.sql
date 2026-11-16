-- 移除只读 SQL 入口，不修改当前缓存或原执行器状态。
DROP FUNCTION neon_lfc_reclaim_state();
