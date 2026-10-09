# 核心效果成本报告

报告由 `crates/aem-render/tests/effect_cost.rs` 生成，范围为当前核心包 manifest 的全部效果（目前 59 项、4 个 profile），使用 3840×2160 单层工程、第 0 帧、按工程尺寸初始化的默认参数。场景/粒子包、历史核心包、自定义插件及非默认参数场景不属于这份快照。

此快照使用 PlanBuilder 的保守默认预算 64 MiB，明确作为可重复的基线；不读取手机 RAM，也不代表设备分档后的实际预览或导出。设备预算仍使用主分支的配置接口和协议 v6，诊断记录不会把它改回固定 64 MiB。

```powershell
$env:MOTION_EFFECT_REPORT = 'E:/Dev/aem/artifacts/effect-costs'
cargo test --locked -p aem-render --test effect_cost -- --nocapture
```

环境变量表示目录，测试写入 `effect-costs.json` 和 `effect-costs.md`。未设置时只输出表格与告警，Rust 测试框架会默认捕获成功测试的输出。本报告仅进行 CPU 侧正式规划，不创建 GPU device，也不分配 GPU scratch。

## 数据口径

JSON 为 schemaVersion=1、metricKind=`portable_validation_product_v1`、scope=`current_core_manifest`。记录包 ID、版本、SHA-256、画面尺寸、参数场景、可选 GITHUB_SHA；effects 按 ID 排序，Markdown 按 loopWorkMax 降序并以 ID 排序打破同值。

| 字段 | 含义 |
| --- | --- |
| passCosts[].loopWork | Registry → PlanBuilder 对应声明 pass 的 CompiledShader.loop_work |
| loopWorkMax / loopWorkSum | 声明 pass 的最大值/总和，包含两次引用同一 shader 的两个 pass |
| passes / plannedPasses | manifest 声明数/实际计划数；实际数包含宿主转换与源物化 pass |
| scratch4kBytes / scratch4kSizes | 成功正式规划的 pool 容量及 8 个 slot 尺寸；失败为 null |
| scratch4kRequiredBytes / scratch4kRequiredSizes | 最后一次效果 pool 检查的候选需求，包括被预算拒绝的候选 |
| scratchBudgetBytes | 当前行实际采用的规划预算，不推断设备可用显存 |
| status / diagnostics | planned、budgetRejected 或 planningError，以及原规划器错误 |
| workingSpace / alphaMode | manifest 声明的工作空间和 alpha 模式 |
| padding / outputBounds | manifest 表达式；对应 Value 字段为默认参数、实际源矩形下的求值 |
| parameterValues | 当前 fixture 经 Scene 采样后的数值，包含中心和相对尺寸默认值 |
| paramsUnimplemented | manifest 明确标记 implemented=false 的参数 ID/名称 |
| warnings | 超循环指标、超 scratch 预算、未实现参数或其他规划错误 |

`loop_work` 直接导出已有 portable_source 校验值：各源码循环的静态迭代数相乘，串行和嵌套循环均按现有算法处理。无循环或零次循环因既有规则计为 1；辅助函数中未执行的循环也可能被包含。它不是纹理采样次数、实际循环执行次数或 GPU 时长。

scratch 容量调用运行时同一个 `scratch_capacity_bytes()`：slot 0–6 按 4 bytes/px，slot 7 按 8 bytes/px。`PlanBuilder.last_scratch_request` 在检查前保存候选尺寸、当时的实际预算及拒绝类型，每次 build 或替换 Registry 都重置。预览缓存命中不会重新检查，也不会为了记录指标重建计划，观察值为 None；当前有效容量仍可从 frame.scratch_sizes 读取。候选尚未通过检查时，不会提交到 frame.scratch_sizes 或分配 GPU 纹理；拒绝行不会因为读取旧 frame 而被误报为零。

此容量仅统计效果 pool。素材纹理、视频 plane、LUT、插件资源、矢量 MSAA、调整图层累积纹理、遮罩、预览目标与驱动内部内存各有独立口径，不是此表的总量。默认参数不代表最坏场景，也不能单凭静态排序宣称某个效果更慢。

## 告警与 CI

任一声明 pass 的 loopWork>2048、scratch 容量或候选需求>64 MiB、非空 paramsUnimplemented 会产生 WARN；默认不阻断报告测试。既有 shader 1024 次单循环、65536 work 上限以及渲染器的硬预算拒绝行为保留。缺少 shader、无效工程、序列化或输出目录错误仍会使测试失败。

`MOTION_EFFECT_STRICT=1` 可显式将报告告警升级为测试失败，报告先写出再返回失败。CI 初期不设置它；当前历史告警未归零，直接启用会阻断现有基线。其他值或未设置均为默认告警模式。

CI 的 checks job 在原 workspace 测试中生成报告，随后上传 effect-costs artifact 并写入 Job Summary，保留 30 天。测试编译阶段失败时没有报告文件，上传步骤只警告缺失，不覆盖原失败。JSON/Markdown 仍在忽略的 artifacts 目录，不提交快照到 Git。首版没有自动与 main 的基线作增量比较，也没有 App 内开发者面板。
