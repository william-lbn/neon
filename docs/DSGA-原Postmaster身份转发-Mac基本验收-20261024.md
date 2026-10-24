# 原Postmaster身份转发

沿既有Neon V3服务分支，新增正式认证GET `/dsg/v1/resource/process`，转发到
既有固定root UDS。root原位exec前保存Birth，查询时验证实际PG文件、完整降权、
固定cgroup和启动时间，防止把请求自报PID或同组任意进程当原Postmaster。
不以PG boot或SQL断连取消旧资源承诺，不生成预算完成或VM许可。

Mac只执行新增路由用例1pass、0ignored，验证专用控制scope、准确实例绑定及
连接root之前的非法query拒绝；未访问Mac特权目录，没有重复旧五条资源路由。
compute_tools --no-deps --all-targets clippy通过。日志在工作区
build/dsga-process-birth-rust.log及clippy.log。PG17内核未改，三处PG锁仍cbabd5。

实际compute-node.Dockerfile原COPY把PG程序和目录归postgres。为了让原文件核验
在产品中成立，PG17最终层额外将/usr/local、/usr/local/bin及固定postgres程序
设为root拥有且不可由group/other写，避免通过父目录rename替换；其他版本的
权限不改，lib/share及扩展下载目录保持既有安排。仅修改产品构建源，未构建镜像。
Mac只做提取RUN的shell语法检查；最终安装属主、文件系统及动态扩展兼容仍须Linux
检查，不能以配方文字代替安装结果。

root出生记录4项Mac race通过，参数收紧后仅重跑受改项；Linux双架构目标编译
通过，不是实际内核验收，详见AS同批中文文档及其Linux6.12源码依据。
AS已先推送并核对a57bed0cd9b9b24f8447447681b9ffbfda2ec779，再绑定本仓发行锁；
暂存版本检查18个程序/4个锁定PG/39个扩展阶段通过，实际PG17检出/gitlink/两锁
一致，证据为工作区build/dsga-process-birth-version.log。
最终提交/远程状态以Git和恢复点
为准。当前Agent资源执行器、完整预算/准入/排空/Host/最终VM等业务仍未整体完成。
真实Linux SUID/进程/exe/cgroup/HugeTLB/NeonVM/K8s及物理性能后验，不构建镜像、
不准备/执行K8s部署、不合main。
