# 设备效果临时纹理预算（宿主协议 6）

调整图层累加器已与效果临时纹理分开。累加器仍使用独立 128 MiB 检查，蒙版与效果临时纹理共用设备策略的 scratch 预算；其他素材、矢量及嵌套合成资源检查继续保留。工程和效果包格式不变。

Android 创建编辑会话时从 ActivityManager 读取物理内存与低内存标志，调用 `NativeBridge.configureMemory(id, totalMem, guarded)`。系统已报告 `lowMemory` 时也启用保护档；读取失败或未知内存时使用保护值。PNG、插件预览、主预览与冻结视频导出应用同一策略。视频导出在任务创建时捕获 profile，之后编辑会话的变化不会修改已创建的导出会话。

| 条件 | scratch 上限 |
|---|---:|
| guarded 或 totalMem 未知 | 64 MiB |
| 未保护，≤3 GiB | 96 MiB |
| 未保护，≤6 GiB | 192 MiB |
| 未保护，>6 GiB | 384 MiB |

这些数字是应用策略的初始分档，不是显存容量、可分配内存保证或驱动能力测量。还需要真机校准；384 MiB 尤其不能当作所有高内存手机的已验证安全值。物理内存、当前可用内存、进程限制、解码器缓冲、预览/导出并行及驱动占用是不同口径；本次没有把所有资源合并为一个新的进程总预算，也没有新增运行中的内存压力自适应。仍保留设备纹理尺寸检查，超出合法布局或策略预算时明确失败。

`motion_effects::SCRATCH_BUDGET_FLOOR` 表示保守默认值。旧公开名字 `SCRATCH_BUDGET` 保留为该值的别名，避免破坏已有 SDK 代码；它不再代表所有设备的实际上限。

帧计划保持 128 字节头，版本从 5 升到 6。原保留字段 word 19（byte 76）为 `u32 scratch_budget_bytes`。Rust 检查并写入该值；GLES 的效果池、调整图层源和蒙版池从同一头字段读取，拒绝旧版本或失效预算。`renderPlanInfo` 和 `previewInfo` 均返回 `scratchBudgetBytes`。预算改变会失效已有预览规划缓存；设备策略不会改变参数、颜色、Alpha、采样或包哈希。

```json
{"ok":true,"data":{"scratchBudgetBytes":201326592,"policyVersion":1}}
```

验证用例只生成 4K 计划，不分配 4K GPU 纹理。实际容量决定结果：4K Tint 的 94.92 MiB 可在 96 MiB 档通过；Glow Edges 单项在 192 MiB 档通过；默认 Glow Edges + Gaussian Blur 涉及线性与 sRGB 两组工作槽，约需 225.90 MiB，192 MiB 档应拒绝，384 MiB 档可通过规划。不能把提高预算描述成“所有 4K 效果链都通过”。
