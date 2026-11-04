# 托管SQL组件部署与Compute首启失败阶段归档

本阶段继续聚焦PG内存自动扩缩容及直接依赖组件。控制面必要接线补丁33f1cbc已提交原分支；39个去重组件测试根来自三份精确源码/函数边界一致的证据，不是单次39根执行，更不是真实SQL或内存端到端。旧FAIL与修复后结果全部保留。

Adapter使用自有neon-helm：预检5个对象，只替换单镜像并给原SQL Service和原配置Secret追加两项精确GET。118构建的一份OCI经SSH预载117/118/119实际k8s.io；Helm实际退出0，新Pod fmfsk在117 Ready/零重启，实际digest与IfNotPresent均读回一致。19个真实SubjectAccessReview为2允许、17拒绝，凭据Secret、其它配置、list/write及Node权限仍拒绝。此授权检查不是业务SQL接口成功。

旧0aa物理实例按原状态正式退役，原tenant/timeline、MinIO、PVC和凭据保留。新7410以新incarnation完成原hold/render/21项Runner权限检查/seal；首次journal初始化未确认后，仅沿已确认UID接续原流程成功，没有重新CREATE或重置。新Pod hm2mn在117启动22秒后失败，Guest仅进入initial-boot-enter，没有wait_spec、PG、configure或内存Begin。原日志中的固定错误是private QMP write unconfirmed: i/o timeout，并join QEMU signal:killed；主动错误清理也会产生SIGKILL，不能据此称OOM。

OOM独立只读窗口没有强绑定内核见证，保持Unknown。Birth和StateClaim均绑定真实新VM/Pod/Runtime/Config，但公开State三个null不包含私有WAL分派阶段，不能冒作未发送或NoEffect。完整私有日志、Node和元数据证据留118 /data，不复制进仓库。失败7410保留，未执行新的configure或重建。

初启QMP源码发现同连接绝对2秒期限覆盖了本地peer可执行文件校验、Kubernetes授权读取和WAL落盘。未修模型注入一次成功授权读取延迟后，父ctx仍有效却命中后继写入超时；模型qom-set计数0、may-have-sent文件仍保留。此为确定可复现的机制缺口，不能直接认定历史现场唯一原因。候选只调整初启完整命令I/O边界、保留原总ctx/2秒/身份/授权/WAL/Unknown及唯一写入，普通默认路径不扩；尚待GREEN及真实新冷启动验收。首GREEN编译重复函数与SDK校验工具问题均已中途发现、保留失败、无Runner产物。

阶段小用例归档位于/data/neon-k8s-pg17/cases/component-api/PG17-阶段归档-ManagedSQL与Compute首启-20261104T050505Z-44ff21（中文说明和JSON）。正式托管路由CLI已安装为/data/neon-k8s-pg17/tools/neon-control-managed-route-5b39b87ee058，仅安装未执行；真实初始化未通过时必须拒绝发布。Proxy SQL/512行有序校验和未过，K8s Grow、安全normal Shrink、Regrow、统一owner自动策略仍0/4。PG17/PG18 gitlink与发行锁未改；PG18与完整最新控制面仍按最终计划独立推进，不能据本阶段封版。
