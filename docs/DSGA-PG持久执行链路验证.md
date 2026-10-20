# PG持久派发与原意图恢复的真实链路验证

日期：2026-10-21。依赖Autoscaling自有功能分支准确提交
`48f01e2aee8208ba1ab5392a518434e276086066`，已先推送并核验GitHub refs；
PG17仍为`6b7c3655ab74062e117eef7400953cd000679a49`/Neon1.8/ABI6，三处锁不变。
Neon/Autoscaling官方main再次fetch仍fa504217…/a4bce09e…且已完整包含。本批
Neon仅扩大跨仓源码状态记录及更新验证文档，Rust/C/PG业务未修改。

## 生产代码与实际测试链

新增Autoscaling `pgexec`将真实journal Store与Compute HTTPS Client接成PG步骤
适配器：固定owner/boot→Fence→确认Start条件提交→完整原意图Resize→完整回执
hash/当前floor/generation校验→Record条件提交。普通恢复只查询/业务取消；
显式ReplayOriginal限定本owner/boot、已Start且未Abort/删除的完整原命令。
已拒绝但无原生终态的步骤不可再次执行。成功重放不覆盖原终态或清外层预算。

逻辑ResizePG与ReleasePG分开；后者仅在同一当前操作/容量且PG执行者已退出时
核验四数组Provider完整范围及物理证明。unsupported/partial/未知成本/保留工作区
不生成成功许可：执行者仍在途或当前事实对不上则Unconfirmed，已退出且本操作
证明不足则Rejected并保存原成本交预算/准入清理，不能靠重复查询伪装推进。

本机真实链为：Go journal Store（生成clientset/HTTP/RFC6902）→Go pgexec→TLS
终结→生产Compute Router/JWT→长期双SQL→真实准确PG。只有VM API存储和非PG
前后领域使用明确test_only夹具；没有模拟PG回执。初次1024 grow先确认写前
日志再发送，PG真实完成后丢一次回复；重新构造适配器从持久原意图查询落账，
不重复POST。此后显式同原意图重放才第二次POST并解除Go SDK pending，原账本
终态不变。随后真实shrink/regrow、历史/当前分离、epoch2 Fence、600行大于
bootstrap数据校验及实际PG重启拒静默重绑均保留。

`ci/test-dsg-go-client.sh`现记录整个`pkg/dsga`的源码状态，避免漏记新增pgexec或
journal生产依赖。Go native测试改为外部测试包以使用生产pgexec，未引入循环依赖。
不增加独立假管理服务、不更改生产路由或放宽TLS/JWT/实例认证。

## Mac实际结果

- Autoscaling最终恢复后受影响race219个case pass事件（8个新顶层/26个子测试
  共34事件），0fail/用例skip；11测试包通过，9无测试文件包分列。
- native标签完整受影响组件lint0issues；无go.mod/go.sum或PG/版本锁变更。
- 正常及PG重启后两Go race进程、1 Rust外层门禁实际通过，候选7.08s。依赖提交
  后从准确48f01e2再次重编执行通过7.02s（Go正常2.99s/重启后0.01s），业务源码
  状态干净；准确结果见pinned日志。
- 四项实际反向：漏完整意图核验、漏物理释放证明、写前确认前派发、漏无原生
  终态的重放资格检查，均为断言失败；finally前后源码SHA256均
  b8355b965253bb97bd0fa936128320d783eefe4cf8c79c3e2130e2993047b42b，恢复后完整
  219组合重验通过，没有以编译错误计数。Bash/diff/准确版本绑定检查通过。
- 扩大Controller子目录envtest尝试因缺etcd启动失败并触发既有清理panic，0/2
  spec未执行，单列保留。此不是Mac天然限制；不冒充通过，本轮未准备K8s部署。

日志在工作区build：neon-pgexec-native-{release,pinned}.log，
autoscaling-pgexec-{publish.jsonl,publish.stderr,lint-publish.log,negative-final.log}；
扩大envtest的失败为autoscaling-pgexec-full.jsonl。复现仍用正式
`bash ci/test-dsg-go-client.sh PG_INSTALL_DIR AUTOSCALING_SOURCE_DIR`，PG/三处源码锁
和二进制SHA由脚本验证；Mac沿用Xcode14.4 SDK/clang/CGO、Rust1.88/Go1.25.13。

## 交付边界

这是真实PG步骤适配器和持久恢复测试，尚未将其接入Agent可信选主/持久协调循环。
Monitor、完整硬预算/quiesce及清理、Scheduler/Controller/最终Runner执行隔离、
受控跨boot恢复、其余PG、Driver/Helm及真实Neon存储E2E仍需继续代码/集成工作。
没有运行全compute_ctl启动、真实apiserver/etcd、VM、Host/Guest或Provider物理
释放验收。新模块默认关闭，vm_shrink_permit/production_ready仍false。
Linux/K8s/KVM/HugeTLB/真实物理回收/迁移/性能后验；Goal active，不构建镜像、
不准备部署、不合自有主分支、不发布生产。
