修改原因：Root资源代理错误复用PG Init拒绝门，真实身份/状态查询412。
主要变化：资源专用getter只取原冻结并验证的DSGA身份，允许Init；PG SQL/resize的原Running门及JWT/VM-CI/Root校验保持。
验证结果：2个getter直接测试和原proxy机制测试通过，真实Compute构建0，独立审查无阻断；原日志和完整构建记录仅保留/data。
待验收项：真实normal/VM与K8s；rustfmt组件缺失未执行，不冒整体通过。\n\n所有原始日志、构建配置、源码锁和二进制均在上述/data运行目录。此目录仅保存人工限定字段的小收据，不包含凭据或运行原文。