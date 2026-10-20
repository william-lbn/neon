# R09 内核回执与取消接入

日期：2026-10-20。PG17依赖提交`48633803b05c1b7bd9a40d749b5278df9aaf01ac`
已先推送并核验。详细职责、锁序、取消线性化、历史淘汰和恢复规则见实际子模块
[内核说明](../vendor/postgres-v17/DSGA-R09-回执历史与业务取消.md)。

这批新增`DsgBufferControlResize/Status/Cancel`受管C接口，回执明确区分本请求
result与当前observed；旧Grow/Shrink函数签名仍保留。Buffer ABI由4升级到5，
主仓必须重新绑定gitlink与两份锁、重编准确SHA、清理重编Neon扩展后实际加载。
Rust `compute_api::dsg_control`的wire结构仍需通过生产Neon端点接入，不能把C测试桥
当成用户接口，也不能把context token测试当成生产SHA-256、认证或预算验证。

## 实际本机用例

新增`ci/test_dsg_control.py`共21项，复用临时集群夹具而不继承原48项，避免重复
计数；严格`ci/test-dsg-postgres.sh`显式组合两个模块并拒绝零测试/任何skip。
真实执行Grow/Shrink/原生异常与锁，C桥仅转换参数/快照，不改写生产状态。

| 范围 | 关键断言 |
|---|---|
| 身份/历史 | G→S→G后查询旧G得到原结果；current可属于新操作；保留16条的精确边界 |
| 冲突/过期 | 同ID改完整意图拒绝；旧key淘汰/换boot不重新执行 |
| 换代/表示 | uint64最高位与最大值无损，较大epoch重置revision；拒绝模糊字符串 |
| 并发 | BUSY只属于竞争请求；当前运行中的重复查询不等待串行锁；历史取消不影响新操作 |
| 业务取消 | 发布前终止/恢复L并保数据；Grow预触成本保留；发布后TOO_LATE不回退H |
| 锁等待 | 读者真实持accessSH时，Grow/Shrink取消在原期限前自行结束，读者仍未退出 |
| 异常恢复 | SQL ROLLBACK不撤销已提交；ERROR/FATAL可查；迟到退出回调不覆盖新操作 |
| 边界/权限 | C入口拒绝反向锁序，原生ERROR释放Guard；测试桥显式GRANT/definer仍检查会话 |
| 回收归属 | 每数组epoch/revision/opID两半/generation完整，换代同revision不混淆 |

候选67项（原48+新19）通过122.538s；最后两项加入后新模块21项通过44.279s，
均零skip。准确SHA重编后69项组合通过127.064s，完整Neon扩展22项通过40.470s，均零skip。
off/on各基础81组/666项、48份TAP报告（231报告文件/18410报告断言，含skip）、
46PASS/2NOTESTS/零FAIL；pipeline70614退出0。prepare104项为13通过/91环境skip，
不能与实际业务通过混计。详见[当前状态](DSGA-当前实施状态.md)与
[恢复记录](DSGA-执行恢复记录.md)，日志`build/neon-r09-exact-*.log`。

## 不可越过的实现边界

有界PG日志只保证保留期内ID意图不可变、相同旧key不重执行；无限期UUID唯一与
跨boot历史必须由外层持久登记/唯一执行者处理。不能把已淘汰ID重配更大的epoch/
revision后声称内核必然能发现篡改。RESULT_EXPIRED不证明旧请求曾被接受。

CANCEL_REQUESTED不是取消终态/资源回收证明，COMMITTED也不是VM完成。外层预算
承诺、实例身份/issuer所有权、Provider/Guest/QMP/Host回执与最后执行fencing
仍须完整落实。正式capabilities/request/status/cancel/memory_status、生产
认证、C/Go/Rust交叉编码、全量预算/控制链与其他PG版本仍是代码剩余项，不能
列为Linux环境待测。Mac只证明已执行用例；真实Linux物理/VM/K8s/性能后验。
