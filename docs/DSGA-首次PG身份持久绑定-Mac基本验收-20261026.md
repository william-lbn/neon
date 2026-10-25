# 首次PG持久绑定：Neon来源与Mac验收

自有AS `da2aa56a1efaf6241ebf85c7814542e41e757a66` 已先推送并核验，
Neon原codex/dsga-neon-v3-20261019锁定同SHA；PG三处保持b812、扩展1.15
不变。控制面原分支准确模块v0.0.0-20261025192811-da2aa56a1efa已核Origin
完整SHA、最小两条sum和readonly vet。各服务沿既有版本分支，不重建/强推或合main。

改码前真实Owner/SDK/TLS负向复现：无Resize/HWM时接受boot11后又接受12。
新首次能力协商后只读取得PG身份，必须原VM UID/RV CAS持久固定才返回可用
worker；丢回复保留原绑定，不自动取新boot。显式boot和错误资源参数在凭据/
HTTP前拒绝。Clone/接管/历史淘汰保留；旧HWM沿原身份恢复、不补字段，新
请求不能先改HWM收养新boot；原未知Busy/历史可读/同boot推进保留。

创建仍Model v1，首次绑定与升为Model v2同CAS，必带PGInstance，外层信封
仍v1；不同于最终Resume审计v2。实际旧读者对原v1通过、完整v2新字段拒绝、
正确checksum的v2旧字段投影版本拒绝。最初误以为外层json.Unmarshal会忽略
字段，实测Model自定义严格解码纠正了这一判断；错误断言日志和注释修订已
留证，不当成旧产品缺陷。所有读者先升级再启用，不删除字段或重造日志回退。

Mac去重36顶层/71含子项race通过0测试skip，覆盖模型寿命、实际typed Store/
RFC6902/UIDRV CAS、Owner/SDK/TLS发现与未知恢复。完整journal首轮53过/1
旧合同失败后，只替换该项及补历史断言，不重复旧53；通用Cleanup不足以
证明跨boot，独立受控恢复仍须代码。初窄组、全量、替换均按用例去重。最终
vet通过；Agent Linux amd64/arm64仅交叉编译通过，制品来自提交前最终业务
源码，不冒充发布VCS标签。Kube/PG能力为明确协议夹具，不是目标内核/集群。

控制面新增专用TLS签发器读v2场景1PASS0skip，实际Store/CAS写入后可签发原
绑定的有效签名token，账本和凭据不写、不能换boot；Kube/TokenReview为夹具。
初次测试库引出额外依赖导致setup未运行，改用已有标准testing后单项通过，
未引入额外依赖，初源码和日志保留。不重跑未改PG/DSM/nativeHTTP/发行单测5；
本批只对新发行索引输入核验，PG准确四制品哈希保持。

机器证据见[本模块收据](DSGA-首次PG身份持久绑定-Mac收据-20261026.json)，
实现详见[AS模块文档](https://github.com/william-lbn/autoscaling/blob/da2aa56a1efaf6241ebf85c7814542e41e757a66/docs/DSGA-首次PG身份持久绑定-Mac验收-20261026.md)。
本批fresh fetch相关完整维护均missing0，没有空维护提交。

本项没有开放业务或发VM许可。完整可信Prepare/Finalize/SystemOutsidePG、
安全降normal/后台私有上界、首次预算/开放、Host最终VM/公开worker/受控跨boot
仍缺代码，不包装成Linux待测。Linux内核/HugeTLB/物理释放/目标E2E/性能另验。
只PG17、Goal active/incomplete、默认关闭，不构建镜像、不准备/执行部署、不合main。
