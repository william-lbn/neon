# PG17初始化未确认的固定阶段诊断

实际Compute已运行，初始准入已打开，但`/dsg/v1/initialization`仍503。原handler对内层错误统一返回未确认，无法辨认第一个失败的检查。本次仅在原生产函数加入请求内诊断守卫，失败或取消时记录固定数字stage；原成功合同、认证、安全门、超时和公开响应完全保留。不能把本补丁当作503根因已修复。

固定阶段对应：1运行态；2准确PG大版本；3原创建与存储；4已发布认证回执；5第一次Native状态；6新私有观察连接；7实际PG版本及存储身份；8真实角色；9已加载HBA/ident策略；10再次Native状态；11配置未变；12认证回执代次未变；13最终响应合同。日志只有固定事件名与数字，不记录原错误、SQL、角色、路径、JWT、密钥、verifier或请求内容。守卫在原局部锁释放后析构，全部原检查通过后才停止记录。

Linux固定Rust1.88/ece1 SDK，以原7348源码加唯一`compute_tools/src/dsg.rs`（SHA e034a7ac203410165d7c50058b004a7fa6c9c0a7f0a925cb86b14e10ec633fc5）冻结4550个文件，复用原三个Cargo缓存。实际3.75Gi/2CPU/swap0、Cargo jobs1，四个准确测试名均通过、零失败零跳过；最终单Compute ELF与strip-debug成功，485.957秒，无OOM。准确名字、源码数量与ELF `c9b7061c70dba58b22c8f012bfe73ba658689db83d6475fc5645108f6240b95e`见收据。

固定缓存SDK缺rustfmt组件已留首次工具失败，不额外联网安装，也未将此项冒作通过。未改动的原生HBA ignored测试本轮未运行；四个测试不代表HBA、normal或端到端验收。构建前预算规格遗漏.service后缀的首次拒绝及最小修正也已保留。大日志、源码快照、实际cgroup与原Wait/terminal证据在118 `/data/neon-build/validation/native-initialization-fixed-stage-rust-PREPARED-20261104T033621Z-830a8b/`。

后续使用原Builder/Collector为新ELF重新生成System/Native闭包与Guest，不热替换正在运行的受信任文件。只有三节点准确镜像预载、原保数据退役/重建及真实阶段日志后，才能针对实际根因修复。当前安全normal降限、Grow/Regrow/自动策略和Proxy SQL仍待真实验收；PG17/PG18 gitlink与发行锁未改。
