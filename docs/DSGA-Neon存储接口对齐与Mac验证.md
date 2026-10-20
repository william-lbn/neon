# Neon 存储接口对齐与 Mac 验证

日期：2026-10-20。遵循 V3，功能默认关闭；不构建镜像、不部署 K8s、不合主分支。
本模块把完整 Neon 扩展接入实际 Neon PG17，不能以独立 PG 样例替代 Neon 产品。

## 修改范围与接口约定

| 组件 | 实际职责与改动 |
|---|---|
| Neon PG `storage/smgr.h` | 声明独立 SLRU 下载能力 v2，扩展落盘后核心重新打开；不按 PG 大版本猜 Neon 维护 ABI |
| `pgxn/neon/neon_lwlsncache.c` | 删除无调用者的旧 hook 注册；保留 smgr 的 block/vector/range 实际 LSN 更新，由扩展申请/绑定命名共享锁 |
| `pagestore_smgr.c`、`libpagestore.c` | 注册真正独立下载 hook；校验文件名与 segment 身份；幂等建立通信状态；保持原请求 LSN 与 Pageserver 协议 |
| `slru_download.c` | 完整私有缓冲区写入同目录独占临时文件，fsync 后原子 link；不暴露半文件，不覆盖其他 Backend 已发布或更新的目标 |
| `communicator.c` | 在乘 BLCKSZ 和复制之前检查远端 SLRU 页数，拒绝负数、超范围及截断报文；其他 transport 的结构仍独立校验 |
| `neon_test_utils` | 清缓存覆盖调用开始时真实 H，逐项短 Guard；复制值后放 Guard，再用原生 ReadRecentBuffer 复核过期提示，可能写回的阶段不持 Guard |
| 主仓测试与 CI | 增加完整扩展构建/加载的真实集群入口；核对准确 PG SHA，缺环境、零测试或 skip 直接失败 |

PG17 维护中的对应官方提交：旧 block hook 删除为 `a42a079b61c053e35c35e63cfd52449da92b5ddb`，
SLRU 接口重构为 `c93daf0889e17bf7cf9184d12c4aff9ed4d5084c`。已检查均为本功能头
的祖先；其他版本有对应 backpatch，不能只把公共主仓的旧扩展绑定到新内核就当作兼容。

DSGA 内核 ABI 仍为 4；独立 `NEON_SLRU_DOWNLOAD_API_VERSION=2` 只说明存储接口。
旧发行头未声明该能力时保留旧 f_smgr 接入；未知新版本明确编译拒绝，不做函数指针
强转或静默降级。PG18 的接口/segment 宽度及 AIO 必须另行适配。

## 锁、生命周期、失败与恢复

LSN hash/LRU 由同一个命名共享 LWLock 保护，在 request/startup 阶段申请/绑定；
不能用 Backend 私有锁或恢复过时的核心锁号。持锁不做页面 IO、不调用 Buffer API
或 Provider，避免与上层 content/buffer 锁形成反向依赖。新增固定请求也参与核心
真实共享布局计算；这不替代尚未完成的全进程私有/后台预算。

SLRU 发布不持 Buffer Guard 或新增共享锁，遵守调用方原生 SLRU 保护协议。文件
内容先完整落盘，link 遇 EEXIST 保留已有目标；不能 rename 覆盖可能已经更新的
文件。ERROR、取消、FATAL 只清理本次拥有的 transient fd/临时文件，绝不删除已
发布目标。清理上下文位于堆上，避免 longjmp 自动变量不确定性。SIGKILL 临时遗留
由新共享 boot 的 Postmaster 在子进程尚未运行时识别清理；合法文件和未知名称保留。

协议错误或落盘失败不能返回 true；0 页返回 false。发布后报错保留完整目标，后继
核心重开观察到的事实优先。这里没有任何 VM 许可，也不声称普通页/HugeTLB 物理释放。

## Mac 已执行的候选测试

完整 `neon`（含 Rust communicator 静态库）、`neon_test_utils`、rmgr 与真实 PG17
已构建和实际加载。最终 22 项独立临时集群测试通过，41.137s，零 skip，包含：

- 多 Backend LSN 单调性、共享 hash/LRU 淘汰后的保守上界。
- 真实扩大的 Buffer 尾部缓存清理；暂停在有效尾部页面提示，放 Guard 后实际
  Shrink，再恢复使用失效提示；验证缩容不被阻塞且数据 hash 重载一致。
- 完整/部分 segment、拒绝非法路径/页数、两个发布者竞争保留先发布目标。
- 用本 Backend 的真实 RLIMIT_FSIZE 制造短写；同连接随后正常写入，验证限额恢复。
- 发布前/后 ERROR、取消、FATAL、SIGKILL 以及新 boot 清理，核对实际文件与临时文件。
- 生产解析器接收合法/负数/过大/截断报文；普通角色被测试桥内部再次拒绝。
- 本地 PG/COPY 网络 fixture 驱动真实通信器和下载 hook，验证有数据、0 页、超大响应
  三条路径的请求身份、LSN、解析及落盘；没有替换 page_server 函数指针。
- 对不支持的 SLRU（包括长名 pg_subtrans）直接返回 false；受支持目录的文件名与
  segment 不符则报错；两种路径都不建立远端连接，不误发 CLOG 请求。

该网络 fixture 没有 Pageserver 的存储/WAL 语义，不能称为真实 Pageserver 端到端。
用于测试的 C 桥只在独立集群显式建函数，不改变已发行 extension SQL；每个入口仍
检查 superuser，不复制生产实现或伪造 Buffer/物理回收状态。

最终完整入口日志 `build/neon-integrated-storage-abi-extension-tail-final.log`；早期
候选日志 `build/neon-slru-extension-all.log`、`build/neon-slru-hook-first.log` 和
`build/neon-pgxn-s3-*.log`。PG14/15/16 已用各自真实发行头对8个受影响 C 文件
做语法/API兼容检查，24次通过；这不是其他版本 DSGA 功能或完整链接运行验收。

## 最终身份与持续门禁

PG 依赖已先推送至 `864d32d9a8cfb23c71ea73caccc2c80360fb18bb`，包含独立 SLRU
能力标识、关机 restartpoint 持久化修复 `e02a09ad8c` 及 TAP fixture 适配。
Neon gitlink/两份锁同时绑定该提交。最终 SHA 重编及两套专项日志：
`build/neon-integrated-storage-abi-{build,lifecycle,extension}.log`；结果以提交时实际
完成状态为准。本批已完成：准确SHA重编后45项内核专项通过86.415s；完整扩展
入口最终重新clean/build/install后22项通过41.137s，两套均零skip。发行锁、gitlink、
实际源码与postgres --version中的完整SHA一致。

```sh
bash ci/test-dsg-postgres.sh "$pg_build" "$pg_install"
bash ci/test-dsg-neon-extension.sh "$pg_build" "$pg_install"
```

调用方提供准确 PG 构建和安装前缀，配置需含 injection-points。完整扩展脚本
先验证源码/二进制 SHA、清理旧 ABI 对象再重编，显式运行且拒绝 skip；Cargo 使用
锁定依赖。新增 PR CI 是宿主编译/测试，不打包镜像，当前没有远程 Linux 执行证据。

## 尚未完成

最初 TAP 的8个失败文件中，7个已有针对性通过证据；`pg_upgrade/003_logical_slots`
仍待处理，首次整体失败后未执行的其余 TAP 也须完整复跑。不能仅因基础全量和专项
通过关闭发布门槛。正式生产 API/独立回执/历史/取消、全部预算准入、Monitor/Agent/
Scheduler/Controller/Runner fencing、其他 PG 版本、control-plane Driver 与 Helm
接入仍在持续目标内。真实 Neon 存储链本地可行部分继续做；目标 Linux 物理回收/
HugeTLB/KVM/迁移/K8s/性能与长稳后验，不能用 Mac 或协议 fixture 结果代替。
