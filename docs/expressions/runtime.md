# 运行时与工程契约

`Project.expressions` 是可选有序列表，每项保存稳定 target、source、enabled、seed 和 profile。源码使用 UTF-8，最多 8192 字节；每工程最多 256 条。无表达式时不序列化该字段，并保留原 Scene 无分配采样路径。带表达式时格式提升至 3，旧宿主因此明确拒绝未知工程格式，避免忽略表达式后错误导出。

每线程持有自己的 QuickJS runtime：64 MiB JS 堆上限、512 KiB 栈上限、单表达式 20 ms、一帧全部表达式 100 ms。时间预算是故障限制，不是性能目标。宿主轨道复制、JSON/AST 的 Rust 分配不计入 JS 堆上限；源长度、AST 128 层/4096 节点和工程模型约束提供额外边界。达到限制报错，不降低算法精度。`evaluated_at_cancellable(frame, Arc<AtomicBool>)` 支持取消；Android 现有导出取消在帧之间生效，帧内受执行预算约束。

第一次使用源码时解析、适配并编译，生成 QuickJS 模块字节码。正常重用最多 256 项线程缓存，满后重建整个 runtime；缓存不写入工程，也不加载外部字节码。模块 bytecode 的 ROM_DATA 在 runtime 析构前保持分配稳定，避免失效引用。新增源码首次采样或缓存重建仍有冷编译成本，不声称任意编辑场景逐帧从不编译。

每次求值建立独立 JS context，以免全局变量、原型修改或随机状态在帧之间残留。Date、Promise、SharedArrayBuffer、Atomics 被禁用，没有文件、网络、模块加载器、计时器和 JNI/应用操作对象。异步语法拒绝；动态产生的未完成任务会使 runtime 重置并报错，不能带入下一帧。

先采样原关键帧，再运行表达式。当前属性 valueAtTime/key 读取原工程。结果写入当前帧的临时 Project，原 Project、轨道和缓动不变。多个不同轴表达式累积在临时向量；整属性与轴表达式不允许重叠。暂不允许跨属性引用，因此表达式执行顺序不产生属性依赖。

Scene 使用临时值执行父子关系、摄影机、图层局部空间效果和合成。wgpu PNG/预览、JNI GLES 参数块、MP4 均经过同一个 Rust Scene。无表达式工程走原路径；有表达式时会复制 Project 并创建 context/参数数据，尚未优化为每属性缓存采样，需按设备与工程复杂度评估实时性能。

所有结果必须为有限数字或对应维度数组，拒绝字符串、函数、对象、Promise、NaN、Infinity。除图层透明度已有的显示限制外，不静默截断超范围的效果参数或变换。错误包含 target、frame、原因；逐帧失败不修改原工程。

SDK 不新增 Android 方法签名。添加/替换和移除分别使用 `Command::SetExpression`、`Command::RemoveExpression`；通过现有 Engine 批处理、gesture 和历史保证原子性。导入错误源码可以保留；开启时进行编译及指定帧预检；运行时仍逐帧验证。

Windows Android 构建使用 NDK clang/llvm-ar 编译 QuickJS C 源码，bindgen 按各目标及 API 29 生成 ABI。缺少 libclang 时构建脚本只将固定 `libclang==18.1.1` wheel 安装到忽略的 `.tools/python-libclang`，不修改系统环境。继续保留 16 KiB ELF page size 配置。初次构建需要 Cargo/PyPI 网络访问。
