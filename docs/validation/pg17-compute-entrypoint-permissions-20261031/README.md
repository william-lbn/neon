# PG17 Compute 固定程序权限接线

PG17-B-PERM-001：PASS_BUILD_LAYER_COMPONENT_ONLY。真实构建参数是v17，原正式Dockerfile比较17导致权限保护未执行。本次在源统一v17；ci/compute.py原生成器也真实render通过，无本地条件替换，原生成器输出SHA见summary。

隔离容器从正式Dockerfile提取原RUN块直接执行，起始三个路径均1000:1000/0777；v17实际读回/usr/local、/usr/local/bin和postgres为root:root/0755。v16对照仍保原值，未扩大其它版本行为。没有手写第二份条件或用静态字符串检查代替权限行为。原runtime-deps锁image、提取块SHA/完整命令上下文及stdout/stderr留Linux。

该容器只有权限占位文件，无实际PG/Compute，不是最终镜像、程序加载或normal/VM/K8s回归。最终Compute仍需重建并实际验证固定PG/库和受管exec。lib/share原权限未在本批改变，受限profile的完整不可变文件域另有硬门槛。
