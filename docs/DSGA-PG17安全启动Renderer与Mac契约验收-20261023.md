# PG17 安全启动 Renderer 与真实 Compute 解析契约

本批控制面代码 `7c5da7fee6c60c8c73270647f46f1c277d65d6cb`；Neon 门禁代码
`a9ce6453b4ff00bff61ecf511f8e447f61e95f91`。仅当前 PG17，其他 PG 版本暂缓。
沿用每仓既有 V3 汇总分支；PG 仍 fcada44b，Autoscaling 仍8e1e，源码未改。

安全 bootstrap Renderer 从原计划、实际 VM UID、业务 verifier 与管理公钥输出
确定性的完整 ComputeConfig，不复用旧实验模板的 cloud_admin 业务口令、MD5、
fsync=off 或未经登记的后台扩展。业务 app_owner/control_probe 显式负权限，
管理公钥仅进 JWKS；不生成盐/UUID/时间，不接收seed/明文密码，不读取 env。
角色、TLS路径、固定存储拓扑、兼容影响及剩余工作详见
[控制面完整记录](https://github.com/william-lbn/control-plane/blob/codex/dsga-control-plane-v3-20261020/docs/DSGA-PG17安全启动Renderer与Mac契约验收-20261023.md)。

## Mac 新增定向结果

`ci/test_dsg_renderer.py` 实际运行本次 Go Renderer，使用独占新文件导出主/只读
两份完整配置，交真实 ComputeConfig/ParsedSpec 生产入口消费。不是手写 JSON
或只校验七字段。新 unsafe 样本把普通身份改绑 cloud_admin，由同一真实解析器
拒绝；整数预算、强制准入、身份、TLS/JWK、角色和存储参数均检查。

| 范围 | 实际结果 |
|---|---|
| Go race 新增 | 4顶层/15 pass事件，3.191秒，0失败/跳过；主/只读确定性、9类非法输入、typed创建屏障与恢复、导出 |
| Rust 新鲜配置消费 | 1通过，0失败/ignored/filtered，2份完整模式，0.00秒 |
| 静态 | Go全vet及Linuxamd64 CGO0全部cmd交叉编译通过；受影响Rust clippy38.75秒通过；无新增compute_tools warning，依赖既有warning保留 |

本门禁不启动 PG/VM，创建屏障用本机TLS API夹具，不是实际K8s。旧PG/SDK/
Go/控制面业务测试未重跑，不改变旧证据的SHA范围。完整机器摘要、准确源码/
日志/契约SHA256见[验收记录](acceptance/dsga-pg17-renderer-mac-20261023.json)。
格式化后全部非空白token及字符串一致，保留前后SHA，不为纯格式重复功能测试。
最初clippy漏SDK/CC导致jemalloc C编译失败，补齐Mac环境后通过，原失败保留。

复验时从Neon仓库运行（环境提供相应Go/Rust/SDK/缓存）：

```sh
python3 ci/test_dsg_renderer.py /path/to/william-lbn/control-plane --report-dir /new/evidence/path
```

要求本次导出、4个Go顶层实际pass、零skip/fail、两模式及Rust1实际通过；源码
变化即拒绝。默认ignored不能计为通过，不覆盖历史导出文件。

## 仍须完成

本批是安全bootstrap核心，完整catalog/原凭据Secret UID及内容绑定、公开选择
与真实worker、VM模板/TLS来源挂载及原Operation/VM/Config/boot/镜像能力来源
回执仍未完成。连接上限/受限preload不等于完整运行时内存预算，未冻结catalog
被拒绝不能算最终支持；完整预算/quiesce、最终Runner fence及真实存储E2E继续
编码。不能把这些转记成仅Linux待测，也不将整个V3标为完成。

Linux另验实际CRD/RBAC/TokenReview/Lease/UID、Provider/HugeTLB/KVM/NeonVM/
HostGuest物理回收、迁移/故障/负载和性能长稳。默认关闭；本轮无镜像、部署准备/
执行、main合并或生产发布。
