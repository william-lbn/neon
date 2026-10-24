# 认证Guest资源转发

Compute正式DSGA Router新增五个固定资源路径，复用专用短JWT认证和冻结VM/CI
校验，向固定`/run/neon-dsga/resource.sock`转发原Bearer和请求字节。root独立验签，
资源身份和floor独立于PG boot。没有旧无认证TCP回退，不要求SQL连接或PG running。
16KiB请求、32KiB响应及20秒调用上限；不自动重试，取消时abort连接驱动。
root可能已写入，所有传输失败仍是未知，不表示NoEffect或VM许可。

Mac必要新增检查：真实Unix socket转发1项pass；正式Router五路认证和实例绑定
1项pass，均0ignored。后者在有效认证后用query负向拒绝，避免访问本机特权路径。
没有运行旧PG/SDK或全部Router回归。root在Linux平台的服务权限与物理效果未实测。

工具链Rust1.88，固定Xcode MacOSX14.4 SDK，锁依赖和离线模式；clippy
compute_tools --no-deps --all-targets -D warnings退出0。既有remote_storage依赖
27条warning单列。发现既有dsg.rs测试模块排在生产项之前，仅移至末尾并补中文
说明，未改其业务实现，也未增加lint allow。

证据：工作区build/dsga-resource-http-rust-uds-final.log、rust-routes-final.log和
clippy-final.log。早期Limited错误转换已修正；本机默认SDK27链接不兼容及sandbox
禁止Unix监听的失败日志保留；固定SDK并通过正常审批获得本机监听权限后成功，
没有绕过审批。早期失败和过滤掉的旧用例不计通过。

PG17子模块及两份PG锁仍cbabd5；本批未改PG内核。AS资源通道
a6d191af172fc9fb109b648145cbe6ba484ac94d已先提交、推送并核对远程，再更新
发行AS锁。首次自动审批额度失败导致命令未执行；所有者要求继续后，原审批
流程成功，未绕过审批。本仓最终提交/推送状态以实际Git/远程refs和恢复点为准。
暂存内容版本验证通过：18个程序、4个锁定PG版本、39个扩展阶段；实际PG17
检出、gitlink及两份锁一致。日志为工作区build/dsga-resource-http-version.log。
默认关闭，不合main、
不构建镜像、不准备或执行K8s部署。完整预算、准入、排空、Host/最终VM及受控恢复
仍需继续编码；真实Linux K8s/NeonVM/HugeTLB/物理及性能留目标环境验收。
