PG17 原Rust stderr组件阶段归档，尚非Root来源或normal验收。

真实实现：Linux受管helper ABI1创建CLOEXEC双端pipe，仅登记前一次SET64KiB；两个原端点以固定16B/SCM_RIGHTS送Root，不允许自报PID/路径/预算/nonce。main PG与sync-safekeepers两入口注册成功才spawn，立即释放父Command所持write；旧0沿原piped，新1失败不降级。日志收集固定8KiB读块、16KiB行、64KiB组，保普通多行/空行/CRLF，长行分片；仅真实read0产生EOF receipt，错误不补成功。协议选择严格object，未知/空值/重复字段拒；Root仍必须独立认定完整来源。

验证边界：894768原4U；4893a0新[]负例发现serde空sequence被default接纳的生产缺口，已修object限制；c4b3c1新5U通过。8d718b准确Neon全crate编译并只执行本模块5U：5PASS、0失败、29原未相关测试排除，实际6CPU8GiB/4cargo/221.05s，峰值约5.62GiB，CPU91.68s，无配额throttle。COPY43.88MB0.9s、COPY步骤3.4s、RUN215.3s、Cargo210s；日志没有锁等待/联网下载，未记录足够分时数据，不能凭差额断言单一瓶颈。

40b42b为实际新Rust pipe+PG17 off/filter两PM组件：各约8.73MB无timestamp/4MiB长行日志、后续SQL323，max_line=16384/max_group=64520，唯一Wait成功后额外原write持200ms仍无EOF，关闭最后write后才真实EOF/HUP。该运行使用新增Root注册前的同一有界drain模块5b07，未调用Root注册client，不冒充新RootRPC/来源联调。旧d4b编译、8b权限126、109 loader缺失及f6 COPY忽略失败均原样保留；f6旧result的full_compute_compile=true只表示原worker意图，实际cargo未执行，不作为完成。

后继8c9f1c正在复用相同准确Cargo上下文构建真实compute_ctl ELF，另行记终态。真实同g Compute/Root Begin/NativeFinal/持续FD与回收、真正normal降限、Linux及三节点Helm E均未通过。PG17 filter另仓库81794a0已独立提交，本阶段不移动gitlink或发行锁。大ELF/17MB真实日志保留原/data，外部清单逐sha定位，不塞入Git。
