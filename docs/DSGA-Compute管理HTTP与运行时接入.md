# DSGA Compute 管理 HTTP 与运行时接入

日期：2026-10-20。按 V3 R09 接入现有 `compute_tools`，复用已验证的 Neon 扩展1.7
和 Buffer ABI5，不另建独立服务。当前为默认关闭的 PG/Provider 逻辑管理链；
**production_ready=false、vm_shrink_permit=false**。完整预算、持久执行者、Monitor/
Go Agent、最终 VM fencing 和其他 PG 版本仍未完成，不能称为生产在线缩容已交付。

## 路由与生命周期

| 方法与路径 | 职责 |
|---|---|
| GET `/dsg/v1/capabilities` | 返回协商能力前，先通过真实观察连接确认实例/boot仍有效 |
| GET `/dsg/v1/memory` | 返回当前PG/逐数组Provider事实，不拼接历史结果 |
| POST `/dsg/v1/resize` | 执行或完整原意图重放，只在Running派发 |
| POST `/dsg/v1/resize/status` | 按完整原key查询独立receipt和observed |
| POST `/dsg/v1/resize/cancel` | 独立通道请求业务取消；too_late不伪装取消成功 |

只挂载 external Router，internal 无此路由。认证在读取业务 body/连接数据库之前
执行；body最多16KiB，直接反序列化严格类型，拒绝重复/未知/缺失字段、非规范
uint64和错误媒体类型，不经中间 map 去重。所有路由响应 `Cache-Control: no-store`。
请求/key只放JSON body，不放URL；日志不记录token、连接串、body或内部SQL错误。

启用须有已验证的 `dynamic_shared_buffers` 启动契约。初始/空/已终止阶段拒绝管理；
配置、失败及终止准备阶段若PG仍可连接，则保留观察和业务取消。生命周期变化不
等于旧SQL已取消；新resize在实际派发前再次检查Running。原生测试已验证执行
阻塞→handler被丢弃→重配状态下拒绝新执行但仍可观察/取消→原意图重放。

## 独立严格认证

不使用通用 `Authorize` 的Hadron免认证分支、可选aud/exp或宽泛Admin授权，亦不
改变旧路由兼容行为。DSGA使用同一可信配置JWKS，但要求专用签名与claims：

```json
{
  "aud": "neon-dsga",
  "scope": "compute_ctl:dsga",
  "compute_id": "endpoint/compute-index",
  "vm_uid": "vm-uid-1",
  "compute_instance_id": "compute-instance-1",
  "issuer_epoch": "1",
  "nbf": 1700000000,
  "exp": 1700000120
}
```

示例时间已过期，仅说明结构。签名header必须有唯一且已配置的kid；JWK须声明
EdDSA或RS256，与header一致，且用途/操作允许验签。拒绝空/重复kid、算法混淆、
无匹配密钥及重复Authorization。两算法及混合JWKS均用真实签名测试，RSA测试
通过宿主openssl生成一次性测试密钥，仅内存管道传递，不提交或记录私钥。

claims拒绝未知/重复/缺失字段。aud/scope精确匹配，compute_id对应本节点；
VM/Compute实例与可信启动规格一致。issuer_epoch用无损规范字符串且非零。
标准JWT nbf/exp为整数秒，必须满足`0 < exp-nbf <= 300s`，校验当前时间且无
默认60秒宽限。旧compute/Admin/其他实例token即使签名有效也拒绝。

新执行要求body epoch等于授权epoch；新owner的凭据可查询/取消较旧epoch，不能
重新发起旧epoch执行。签名证明调用身份，**不是持久issuer租约或资源承诺**；
凭据过期也不撤销已发SQL、Provider动作或预算。跨owner的完整恢复/重放协议仍
需持久执行者实现，不能通过重启本客户端或换ID回避不确定性。

## 长期连接、不确定结果和恢复边界

`ComputeNode`的OnceCell持有同一个owned `DsgSqlClient`，执行栅栏跨HTTP请求
存活。只连接官方系统维护库`postgres`，使用真正`cloud_admin`；覆盖业务数据库/
role/options，5秒建立连接，70秒响应等待（PG协调器相对等待上限60秒）。不在这里
创建兼容角色或修改数据库权限。管理schema与五个C扩展函数继续由SDK核验。

本批进一步补上NOINHERIT/SET权限边界：即使角色不继承CREATE，只要能`SET ROLE`
到超级角色，仍能替换管理对象，必须拒绝绑定。旧校验在真实PG上确定性漏拒绝，
新增用例先断言`has_schema_privilege=false`但`pg_has_role(...,'SET')=true`，再验证
绑定拒绝；修复后严格套件通过。保留反向日志
`build/neon-dsg-http-noinherit-negative.log`，不以权限名称代替实际成员边语义。

初始化未完成时可安全重试，因为此时尚未调用resize。初始化成功后连接断开、
backend FATAL或PG换boot，不自动清空OnceCell或创建新空栅栏。HTTP发现不能只
返回缓存能力冒充活实例。当前恢复策略为失败关闭；自动重连、跨进程持久意图与
唯一执行者恢复仍是后续**代码**，不是Linux待测项。观察到PG终态亦不自动证明
原执行连接的Provider后处理或VM动作已经结束。

成功HTTP200仅表示收到有类型事实；BUSY/STALE/CONFLICT/DENIED等依receipt字段
判断，不当作VM许可。错误语义：401认证失败；403实例/epoch不符；412不可派发；
409本次未发送（原在途意图可仍未知）；503管理/观察不可用，或请求可能已发送后的
`state=unconfirmed`并带原key。后者绝不能解释为DENIED。结构/媒体/大小错误分别
422/415/413，语义错误400；认证/解析错误沿用GenericError，运行时错误不回显SQL
详情。接口的完整字段定义在同目录`dsg_openapi.yaml`，由主OpenAPI五个路径引用。

## 实际验证与结论

使用 Mac arm64 与准确 PG17 `48633803b05c1b7bd9a40d749b5278df9aaf01ac` 的
debug/assert/injection二进制、已安装完整Neon扩展1.7。测试共享独立0700临时集群
夹具，失败保留日志/数据；不连接现有数据库。TCP测试只监听临时localhost端口，
HTTP客户端明确不使用代理。完整业务请求经正式Router和owned SDK进入正式SQL，
不注入生产响应或直接改操作phase。

| 检查 | 实际结果/边界 |
|---|---|
| 两包普通全量 | compute_api32项、compute_tools30项通过；另SDK8项/HTTP4项默认ignored，另行严格实测 |
| 严格SDK | 8项真实集群实际执行12.27s，零ignored/filtered |
| 严格Compute HTTP | 11项实际执行6.09s：7项认证/输入门禁、4项原生链路，零ignored/filtered |
| 两包本地Clippy | `--no-deps --all-targets -D warnings`退出0；依赖既有27条warnings保留 |
| 其他 | Rustfmt1.88、Bash、diff、版本锁绑定；OpenAPI YAML解析及所有本地/跨文件引用核对 |

Python prepare仍129项：13通过/116环境skip，日志`build/neon-dsg-http-prepare-final.log`。
原生SDK/HTTP已经严格另验，不能混计或把prepare skip算通过。

原生HTTP四项覆盖TCP Grow→Shrink→Grow、实际大于bootstrap的数据与全payload
MD5校验/历史，真实访问Guard阻塞时丢弃handler并独立取消，后端真实FATAL后的
unconfirmed/拒绝换连接，以及PG实际重启后拒绝缓存发现/旧意图STALE不解除栅栏。
普通门禁同时验证Hadron旧路由仍工作而DSGA不豁免、EdDSA/RS256混合集、过期/
未来nbf/错误aud/scope/Compute/重复claims、重复头、角色与epoch、输入边界。

最初测试夹具复用旧spec时含故意重复的SK地址，被生产ParsedSpec正确拒绝；仅修正
新夹具为合法规格，没有放宽生产校验。夹具拆分漏import及带引号schema查询问题
已修复，保留过程日志，不当作业务缺陷反向证明。最终严格脚本对两个suite分别校验
非零、列出数等于实际通过数且零ignored/filtered；PR宿主CI同步执行，未构建镜像。

复现：按既有PG/完整Neon安装门禁准备准确二进制后，在主仓运行
`bash ci/test-dsg-rust-client.sh <准确PG17安装前缀>`。日志相对工作区根：
`build/neon-dsg-http-{packages-final,strict-final,clippy-final}.log`。
本批没有远程LinuxCI执行证据；不是Pageserver/Safekeeper/RW/RO/branch完整存储
端到端，也不证明Guest/Host物理回收、VM迁移、K8s或目标性能。PG/C代码与三处
版本锁未变，不重复将之前69项内核、47项Neon或off/on world算作新增验证。
