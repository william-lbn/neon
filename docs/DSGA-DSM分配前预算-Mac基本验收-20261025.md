# DSM 分配前预算：原生实现与 Mac 基本验收

本批在 Neon 实际 `vendor/postgres-v17`、既有
`codex/dsga-pg17-v3-20261019` 分支实现，原生提交
`5462f1de54a97b9a9235c69d64d31cb0825dcea5` 已先推送并核验远程。不是另一套独立
PostgreSQL 工程。Neon gitlink/两份版本锁随后绑定同一 SHA。整体 Goal 继续，
**Native DSM1 不是完整伸缩交付；正式 DSM SQL/HTTP/消费者和完整预算仍须编码**。

## 为什么需要原生修改

DSM 可能在普通查询、并行/后台、共享统计/扩展中创建或固定；前台 idle、backend
退出、原生 refcnt 归1都不能代表 backing 已释放。原生外部创建先调 OS 再入控制表，
因此创建前窗口也要预算；不能等分配完成、RSS采样后才决定是否有额度。

新增固定主段登记表，成本进入真实 `CalculateShmemSize`；不改变官方 DSM 控制段
布局/magic、旧段恢复解码或 Buffer6/Work2/Connection1/Prepared1 ABI。固定容量复用
原生控制槽公式并加并发/未知余量；耗尽拒绝创建，不能覆盖未知记录。

- 外部 CREATE 在 OS 调用前按实读基础页大小预留，CREATING 也在账内。O_EXCL明确
  撞名时只撤销本次无效果预留；候选名称不等于对象所有权，不能提前写可清理列表。
- 原生发布成功后转驻留。attach、多会话 mapping、pin_segment 不重复加共享 payload。
  主段预分配 DSM 已包含在固定主段成本，不能再叠加；独立控制段另计固定成本。
- `dynamic_shared_buffers_dsm_budget` 是 postmaster-only 普通页 payload 预留额度，
  默认0明确未约束。正数对 SU/后台也有效，在创建 OS 对象前拒绝争用的超额请求。
  不把准入关闭解释为暂停全部统计/检查点/必要回收。
- 只有实际 DESTROY 成功且无先前解除映射未知才扣账。失败保留未知与原预留，
  即便原生引用已归1也不补零。创建后尚未发布的真实映射，由原 ResourceOwner
  在错误清理时销毁；没有真实成功事实时不盲删可能属于他人的候选名称。
- 固定锁序 DynamicSharedMemoryControlLock→status；汇总和 Connection/Work/2PC
  同点复制，观察不反取DSM锁。Postmaster和尚无PGPROC的控制段原无backend锁路径保留。
  元数据不一致/计数下溢撤销有效性，不伪造归零。

## 本轮实际 Mac 验证

候选新增 **8个不同场景、0skip**：

1. 实际 OS 调用前暂停，关门仍可同点读预留；取消本次 BackendKeyData 的 backend 后
   无 OS 效果预留被回收，后续原额度可正常使用。
2. 实际创建成功、原生控制表发布前 ERROR，真实 ResourceOwner 销毁未发布映射。
3. pin_segment 后创建者退出仍存；两后端实际 attach 读到同一 backing 标记，只算一份；
   unpin和最后真实detach后，独立OS探针确认段已不存在。
4. 两后端并发争最后3MiB额度：只有一个成功；实际销毁后另一后端可复用原额度。
5. 原生最后引用退出后、实际DESTROY前注入ERROR：OS探针确认段仍存，native
   moribund不可attach，UNKNOWN和原预算仍在，下一超额创建被拒绝。
6. 运行期SU也不能降低postmaster-only额度。
7. 命中真实main-region（odd handle）时外部账不变，固定主段没有被重复计费。
8. 默认0实际允许6MiB段，报告未限制且不谎报有限额度，销毁后账恢复。

首轮为5过/2失败，均为测试把通用资源不足SQLSTATE `53000` 写成OOM `53200`；
产品未修改。只替换该2项，并新增默认0场景1项，通过，不重复先前5项。
收口把无用Postmaster包装调用还原原路径、补清楚注释后，只替换创建前取消与
真实销毁失败2项，通过；不增加不同用例计数。准确SHA重编关键3项另作替换，
不重复计算成新增覆盖。准确重编关键3项通过；另在准确新PG与重编Neon1.14上，仅复核受同点getter改动
影响的旧Prepared1正式HTTP兼容1项，通过且0ignored。这不是尚未实现的DSM
正式消费者验证。见[机器收据](DSGA-DSM分配前预算-Mac收据-20261025.json)。

新入口会影响所有并行/共享统计/DSA，故执行一次本批启用DSGA的官方core **225项**
全部通过（包含实际select/write/vacuum parallel）；并行/统计专门隔离 **5项**
全部通过，不重复未改的其他隔离123项。通用回归使用1GiB静态payload cap及1MiB
main预分配，额度争用由独立4MiB夹具测，不让无关容量不足改变官方预期。
6个C翻译单元语法、新账本和入口静态分析通过；正常项目告警级别无新增告警。
初次额外Wextra的官方GUC既有告警及草稿缺头文件修正日志保留。

代码写在真实Neon子模块，CI显式要求新增8项及官方锁修复专项1项存在、运行无skip；
本地本批没有重复运行包含旧全部业务的整套CI。新增shell/Python语法和实际收集
单独核验，不能把收集数当执行数。index核对18二进制/4PG/39扩展及实际5项
绑定检查通过0skip；第一次误写test_compute_manifest匹配0项，未计为通过，已用
真实test_compute.py且显式要求5项替换。

## 官方基线和维护事实

本批审批服务先因额度拒绝fresh fetch（动作未执行，非不安全判定）；按提示12:22
恢复后原命令成功。Neon fa504217、AS a4bce09e、NeonPG17 56692dfb已包含；社区
PG17前进bd807012，独立维护merge `bfd84a98d456d4cfb092a43342ff0fbcfd4aed0c`，
真实三级分区祖先AX锁/并发DDL1项通过，旧准确26e80ac同探针按预期实读0锁失败。
PG tablecmds.c与官方头一致，旧作者注释保留。两份维护都在原分支推送核验。

控制面完整维护main577bb3b另由 `b9f9be7cf2f8e21be18b170f5137a2078350a70a`
合入同一既有控制面分支。Mac真实PG/race新增及受影响19项、vet与本地模板3项
通过，详见该仓库 `docs/DSGA-主线同步-577bb3b-Mac验证-20261025.md`。
AS未改业务，不制造空提交；准确AS5b49依赖保留，其他PG版本暂缓。

## 不能从本批推断的事实

这是同一boot、经dsm.c创建的**普通页payload承诺**。不是RSS、PTE、内核元数据、
THP/HugeTLB、Guest池、Host总量、物理释放或VM许可。固定cap只是后续持续预算
的一项来源，不能单独输出完整Quiesce。直接绕过dsm.c的扩展须另有生产清单与
真实聚合约束；Linux页记账/THP/释放性能仍待目标验证。

新boot表0不证明旧OS段消失；原root Held/Original和未知栅栏继续保留。尤其OS创建
至原生发布前SIGKILL可能留下无法证明所有权的段，不因本模块重建而结清跨boot
旧预算。必须由后续受控资源来源闭合，不能为可用性盲unlink或给VM许可。

正式DSM SQL/HTTP/Rust/Go消费者、后台/私有保留状态完整Quiesce或持续上界、可信
Prepare/Finalize与system outside/首次准入、Host/最终Controller-Runner-QMP、公开
worker及受控跨boot关闭仍是**代码待办**。不是把这些统称Linux测试。Linux
K8s/NeonVM/HugeTLB物理/性能验收另外后置。默认关闭，不构建镜像、不准备部署、
不合自有main；沿既有各服务分支继续实现。
