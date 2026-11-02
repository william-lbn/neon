PG18版本/FFI/control/WAL底层适配阶段归档；不移动PG17发行锁/gitlink。

9个生产源+2个直接测试源：真实PG18头生成FFI/bindings/常量，增加同一底层版本表和穷举dispatch，保默认PG17；control严格1800及原校验和，PG18 WAL按真实布局解码，必要compute版本解析，pageserver参数/结束恢复记录回写PG18 checkpoint.wal_level。未修改上层业务合同、compute_ctl两个路由或broker；未伪用PG17常量。

最终不可变afe920实际构建360.663s：reader build0、wal_decoder/pageserver/safekeeper/compute_tools四包check0、新wal_level unit1通过（265 filtered out，不计额外测试或skip）。继承fd870原versioninfo2+compute版本解析1直接U。实际内存峰值8GiB、memory.max触达667次、OOM0、CPU258.266s；wrapper 97MiB不能冒编译器峰值。原缓存保留。

最终e1a5f7真实PG18 native两个target通过3.8457s：实际PG server_version_num180002、SQL产生原WAL，control逐字roundtrip/CRC及CRC有效但外版本1700拒，实际WAL区间/SQL marker/通用记录解码与错误magic/内容CRC反例。新reader来自afe实际export，Driver来自同一不可变构建source。精确pg_control和5.59MB WAL输入保留/data evidence（仅hash/文本入Git），不是模型数据；没有正式Pageserver/Safekeeper timeline18服务或E验收。旧ecd115两个target被最终本轮替换，不相加为四。

重要发行缺口：当前默认Makefile POSTGRES_VERSIONS和.gitmodules/Dockerfile只有14–17；新FFI无条件读取18，因此afe显式使用准确PG18安装/头仅证明底层模块，不能据此声称标准PG17发行镜像构建闭合。Root正独立统筹vendor/postgres-v18准确gitlink及默认依赖生产者接线；未完成前不得将本批签为默认构建通过。冻结清单只含本批9+2，不含17 gitlink/发行锁/其它WIP。

最初Docker ignore/工具镜像旧头缺失等真实失败均在/data原nonce保存，original-failure-references锁原result及源码SHA；不覆盖失败、不以缺环境算用例通过。最终nonce/spec/原始日志/编译输入/真实头清单SHA/资源及终态均保留。阶段U4+原生组件2、正式PS/E0、normal/Source false。
