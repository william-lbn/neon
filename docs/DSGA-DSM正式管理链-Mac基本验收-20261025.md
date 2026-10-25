# DSM1正式管理链：同点原生事实与严格消费者

本批仅PG17，沿四个既有服务版本分支演进。Native PG17仍为已发布
`5462f1de54a97b9a9235c69d64d31cb0825dcea5`，未重做原生实现；Neon升级1.15，
AS已先发布`d8c1f2ea2eb2d9db30469f2d279090a2ce6c03b4`，控制面绑定自有准确
伪版本`v0.0.0-20261025133115-d8c1f2ea2eb2`且Origin完整SHA一致，模块仅追加两条
校验和。三个消费者原分支分别提交，不rebase/强推，不合main，不构建镜像或准备部署。

## 实际生产接线

Neon正式成员 `neon_dsg_dsm_state()` 通过1.14→1.15升级创建、PUBLIC撤权，C体先
核对当前与原session双SU；误授EXECUTE、SU SECURITY DEFINER包装仍不能使弱
session越权。通过独立Native DSM1一次采样，序列化同点Prepared1/Connection1/
Work2/Buffer6，保留旧ABI。提取公共Prepared序列化helper后不再重复读取旧快照。
1.15→1.14删除新成员，旧PG/缺能力明确Unsupported，不能由宏或版本字符串冒充。

Rust正式零参数成员核验与完整Simple Query通过独立观察池读取，固定认证GET
`/dsg/v1/work/dsm`沿原JWT/current issuer/实例规则；不接受外部上报字节数。
Rust/Go响应强制全部字段，拒绝未知/重复/null；三类字节量为规范uint64字符串。
验证本boot范围、嵌套实例/boot、真实页幂和对齐、固定control成本、创建中/未知
保留子集、占用下界、GUC KB范围/持续cap及默认0的未限制语义。无效账本拒绝
消费，保持原未知预算。SDK与控制面固定GET不清Pending、不重放resize或准入。
OpenAPI严格schema/版本/权限及边界同步。

foregound_idle可与非零DSM或2PC共存；UNKNOWN依然在原预留内，不能补零。
main-region DSM已计固定main，不再算外部payload；control独立。当前cap为本boot
普通页payload，默认0只是未约束观察，**不能因此签发完整预算、跨boot清理或VM许可**。
所有响应vm_shrink_permit=false；accounting_valid失效的消费者返回不可用。

## Mac实际验证

- Rust严格协议1项通过。首轮仅测试fixture巨型json宏超递归限；分层构造fixture后
  替换通过，未提高编译器限额或改变产品逻辑。
- Go协议/SDK两项race通过、相关包vet通过：包括默认0、保留unknown、页/容量/
  计数/范围/实例拒绝和只读Pending。控制面受影响代理race去重2项通过；首轮
  新路由均实际转发，旧fixture硬编码15而应16，改用明确路径表长度后只替换该
  失败1项，未重跑已过的独立观察项。准确AS模块后readonly vet通过。
- 准确PG46与Neon1.15 C/Rust communicator重编通过。新严格正式HTTP1项通过0ignored：
  旧1.14成员缺失503；真实2MiB pin_segment与2PC槽1/前台idle共存、4MiBcap；
  匿名401；ACL及误授/definer原session42501；原测试协调者显式unpin后OS探针证实
  段消失且GET预留减少2MiB，旧Prepared1直接getter仍正确；降级1.14重新503。
- 权限首轮在关门下先收到55000，未抵达预期ACL/C体；改为本隔离夹具明确开门
  revision3验证权限，再以revision4关门。只改测试安排，不放宽产品准入或权限。
  失败原日志与替换记录均保留。
- 真正Go race→本机TLS→生产Router/JWT→正式SQL→准确PG的链1项通过，内层
  TestNativeDSMState通过：实际payload/cap/2PC/同点Memory、原Pending保留、零POST，
  不是mock响应body。二进制编译在AS提交前，收据按源码内容/产物哈希绑定，不
  冒充发布镜像VCS标签。参数/token仅经匿名stdin，未访问用户数据库或集群。
- 目标api/http clippy使用`--no-deps -D warnings`通过；依赖原27条remote_storage
  告警保留，不宣称全依赖零告警。新原生源未改，不重复Native8/core225/iso5或旧全套。
- index核对18二进制/4PG/39扩展阶段、实际5项绑定检查通过0skip；OpenAPI唯一键/
  固定路由/14强制字段与嵌套引用有效。

机器收据见[源码、产物和日志](DSGA-DSM正式管理链-Mac收据-20261025.json)。
原Native批次的Neon1.14二进制从原构建目录保留至
`build/dsga-dsm-native-artifacts/neon-1.14.dylib`，与旧收据SHA完全一致。准确PG
前缀目前安装新1.15，仅原路径下Neon库升级；历史收据未改写，不把新库当旧产物。

## 本批维护与下一步

fresh fetch的Neon fa504217、AS a4bce09e、NeonPG56692dfb、社区PGbd807012均已
包含。控制面主线af5e704 multipart维护32文件另以eb6829b完整合入并推送，Mac
新增/受影响19顶层/26含子项真实PG/race、readonly vet通过0skip。该主线附带
Linux/SDK/集群回执保留原文，不算本轮执行；Chart仅版本元数据，未重跑未改模板。

**整体功能仍未完成，Goal active。** 正式DSM观察链已接上，仍需实际后台/私有
保留状态完整Quiesce或持续上界、可信Prepare/Finalize和system outside生产者/
首次准入、Host/Scheduler最终Controller-Runner-QMP、公开worker和受控跨boot/
owner未知关闭代码。不能把这些代码缺口称为仅待Linux。Linux K8s/NeonVM/
HugeTLB/内核记账/物理/性能验收独立后置，默认关闭，无VM许可。

绑定阶段计数更正：本批初稿将新增两份SQL升级文件误加到构建阶段，写成41。
校验器实际输出始终为39，现以原日志为准；SQL迁移文件不增加Docker扩展构建阶段。
仅更正验收文字/收据，不重复已通过检查。
