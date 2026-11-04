# PG17 当前7Gi首次真实内存准入闭环

本项只验收原资格来源接通后的首次准入，不代表Grow、安全normal Shrink、Regrow或自动策略通过。

- 真实Compute：`neon/cp-d8d85ed796068ab5-fb8mv`，`worker-117`；VM UID `0aa42803-c500-455f-9ed6-633221671351`，PG启动代次 `9013877430840229889`。
- 原Collector/Issuer产生的immutable资格CM通过自有Helm接入三节点Agent；原执行链自然选主并生成`initial_held`。
- 原Control API读取并经过Store.Load/Model验证：journal序号124、Owner epoch119、Admission phase为`settled`，满足原`InitialAdmissionReady`谓词。
- 同次原Native回执确认`admission_open=true`、revision1、capacity_generation1、issuer_floor_epoch119；请求/响应均绑定原VM和PG启动代次。本轮没有手工发送admission setter或任何内存Begin。
- 独立收据SHA256：`fdc958416d804e7815ffd073c311a49d3ec38cc98e937805f2646f76db646d1b`。大证据保留118 `/data/neon-k8s-pg17/runs/pg17-post-initial-admission-state-read-20261104T032102Z-86b5a0/`。

当前独立阻塞：`/dsg/v1/initialization`仍返回503，不能用本项成功代替其PG身份、角色与HBA证明。Proxy SQL基线和四个内存核心动作尚未正式验收，不可据本项封版或声称生产认证。原数据、凭据、镜像、缓存及失败证据保留；唯一configure请求不得重复。
