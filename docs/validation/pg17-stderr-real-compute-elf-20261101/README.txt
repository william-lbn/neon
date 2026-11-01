真实新compute_ctl输入产物，尚非Guest来源或发行版本。

8c9f1c在118实际用已通过8d718b准确Neon五源与相同Cargo上下文构建真实compute_ctl（不是DEV C standin）。cargo bin_build=0、strip-debug=0，178.47秒；保留完整未strip原产物的hash/大小，strip后ELF为71,147,152字节，SHA256 7569a12e36a19cb2f184d123e19ea8f52bd7294c914a1d918e4b5c8020af2af5。6CPU8GiB/Cargo4，实际CPU68.42秒、peak4,558,544,896B，无throttle；IO读1.165GB写0.817GB。原/data/Cargo缓存与ELF全部保留。

e31b59用相同ELF只读挂载，在准确原compute-v17-stage镜像、UID1000/无cap/NNP1/无网络/256MiB/1CPU下真实执行--help，退出0、0.404秒并打印compute_ctl实际Usage。这个smoke只证明本ELF能在该镜像加载和解析命令，未启动完整Compute业务、PG/Root RPC，不认证Guest Source或normal。

实际ELF无RPATH/RUNPATH，interpreter为/lib64/ld-linux-x86-64.so.2，四个DT_NEEDED为libgcc_s.so.1/libm.so.6/libc.so.6/ld-linux-x86-64.so.2。已经从同一准确镜像通过从未启动的临时container只读导出四个真实loader/DSO，记录原lookup路径、字节及sha；这些是后续Builder的真实输入，Guest canonical路径/alias与完整System3库存还未安装和验收。不能仅因有ELF就冒称原Root Compute lease或全部9服务/正常降限已通过。

镜像和发行锁不移动。大ELF与四运行库不入Git，external-raw-files逐sha定位/data原件，可直接供后继真链消费，避免重编译和传输Mac文件。
