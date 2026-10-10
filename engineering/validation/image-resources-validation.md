# 大图片资源验收记录

基线：`origin/main` 的 `0ca9665285197c0d35c22ac3b5d4d1b57ef118e4`。独立分支 `codex/image-demand-proxies-20261007`；不叠加其他未合并 PR。

## 初版验收（`25b357c`）

| 验证 | 结果 | 实际范围 |
| --- | --- | --- |
| Rust 全工作区 | 202 通过，0 失败，2 忽略 | 包含既有 GPU 效果、视频 YUV、合成、矢量与存储测试；本地大图用例独立执行 |
| 图片解码与原图测试 | 9 通过，0 失败，0 忽略 | 真实本地 PNG、合成的大 PNG、奇数尺寸、透明 RGB、PNG 色型/16 位/Adam7、损坏缓存、源变化、路径和磁盘预算 |
| WGPU 工作集 | 1 通过 | 总原图 192 MiB 的素材库，仅上传活动代理；图层/子合成共享；嵌套 Tint；随机寻帧；原图切换、超预算报错、缓存复用与视频 stamp 失效 |
| Android JVM | 10 通过，0 失败 | 现有应用单元测试 |
| Android 图片 API | 4 通过，0 失败，0 忽略 | 流式 PNG 导入、完整解码验证、只读/非 Direct/不足缓冲区拒绝、GLES 按需/释放/复用、真实大图原图上传与非空画面 |
| 视频后导入图片/文字 | 4 通过 | `MediaImportOrderTest`，包含批量编辑、撤销重做与重新打开 |
| WGPU/GLES 原始像素对照 | 4 个帧通过 | 透明图片叠加、片段切换、倒序寻帧；RGB MAE 约 `9.47e-15`、Alpha MAE `0`，均低于 `3/255`（按 0–255 数值统计时阈值为 3） |
| Android 构建/静态检查 | 通过 | ARM64、x86_64、debug APK、instrumentation APK、`lintDebug` |

原始像素测试先采集全部 WGPU PNG，再创建 GLES 输出上下文，避免在同一线程交替创建 headless WGPU 与 GLES 时改变 EGL 当前上下文。首轮测试因此出现空帧，修正测试顺序后全部通过；未降低对照阈值。

## 真实 7952×3273 PNG

仅使用用户本地素材，没有把原图、参考工程或参考目录提交到 Git。

- 编码文件：20,770,401 字节；导入后 SHA-256 与输入一致。
- 原图 RGBA：104,107,584 字节（约 99.3 MiB）。
- 预览代理：2048×843，6,905,856 字节（约 6.6 MiB），约为原图的 6.6%。
- 主机 debug 解码测试：冷缓存约 5.7 秒、命中约 0.82 秒，包含源 SHA-256/代理校验；不作为手机指标。
- Android API 35、x86_64 软件模拟器：导入及代理准备约 1,184 ms；原图 GLES 加载/绘制约 4,274 ms。
- 同一原图用于两个图层，初始化后驻留只有 4 字节白色纹理，实际采样后为 104,107,588 字节；重复准备没有第二次原图上传。素材库中另一个未引用同尺寸 PNG 未上传。
- 大图绘制读取的像素不是空画面。上述原图上传是正式输出路径，**不是代理预览每帧开销**。
- 输出后 PSS 快照约 221,046 KiB、native heap 快照约 36,453,568 字节。没有测峰值，也没有测 vivo Y300 Pro 真机帧率。

## 近期缓存与预取补充验收

- 最终 Rust 全工作区：206 通过，0 失败，2 忽略。忽略项沿用需要外部素材/特定条件的规则，初版的真实 PNG 验证记录保留在上文。
- 最终 Android ARM64/x86_64 构建、debug/instrumentation APK、`lintDebug` 通过；JVM 10 通过。API 35 软件模拟器上：新增预览 2 项、图片 API 4 项、视频后导入/原有视频预取 5 项，共 11 项全部通过，没有跳过大图片用例。
- 新增 `image_preview_cache` 两项 GPU 回归：预取片段首次进入立即就绪；同素材倒序寻帧无需再次上传；坏的未来 PNG 不影响当前帧、不逐帧重试，实际进入画面后明确报错。
- LRU 压力场景使用四份 2048² 自制 PNG，闲置纹理上限 32 MiB；4K 视频输出及 YUV 平面共需 88 MiB 时，先淘汰闲置图片，保留活动图片并使资源总量不超过 128 MiB。正式原图准备会释放所有闲置代理。
- 同一闲置纹理被公开 upload API 替换为更大纹理时，保留旧对象用于正确计算替换成本，淘汰其他闲置图片。该回归防止重复扣减旧纹理字节或低估总量。
- 嵌套时间预取覆盖 30/60 fps、片段 offset、子合成 source start、负源时间、可见性与半开区间边界；只查询依赖，不采样表达式。
- 单解码任务测试覆盖正在运行时拒绝启动第二任务、取消后丢弃旧结果与重新请求成功。

- `ImagePreviewCacheTest` 实际 JNI/Surface 预览：两份 32×16 PNG 只解码/上传各一次（合计 4,096 字节），预取启动 1 次；进入下一片段再倒序寻帧命中驻留纹理 2 次，总上传字节不变。切到高质量时闲置字节归零。
- 混合输入的首次 render：图片解码尚未完成（`imageDecodes=0`），视频已经创建 stream 并发出请求（`streams=1`、`cacheMisses=1`）。就绪后视频只转换一次，后续同帧不重新上传图片。
- 最新真实大图复验：原 PNG 字节/尺寸与 SHA-256 保持一致，代理仍为 2048×843。模拟器导入/代理准备约 801 ms，原图 GLES 上传/绘制约 3,524 ms；驻留 104,107,588 字节、两个图层共用一次上传。PSS 快照约 222,205 KiB，native heap 快照约 36,452,480 字节。四帧未编码 RGB MAE 仍约 `9.47e-15`、Alpha MAE `0`。

以上证明缓存及依赖准备行为；没有据此宣称真机播放达到某个 FPS，也不把两轮模拟器耗时差异作为速度提升指标。原始日志与报告保持在本地 artifacts 中。

## 复现

```powershell
cargo test --locked --workspace --features motion-android/diagnostics -- --test-threads=1
cargo test --locked -p motion-render --test image_working_set -- --nocapture
$env:MOTION_IMAGE_SOURCE = '<本地 PNG 的绝对路径>'
cargo test --locked -p motion-render --test image_resources -- --include-ignored --test-threads=1 --nocapture
```

`optional_local_original_image_probe` 默认忽略，需要本地文件环境变量；本次已显式执行。合成 fixture 的大 PNG、181 字节 Adam7 样本均由测试生成，不包含用户图片。

```powershell
python tools/build_android.py --abis arm64-v8a,x86_64 --effects-acceptance --task assembleDebug assembleDebugAndroidTest testDebugUnitTest lintDebug
adb push '<本地 7952×3273 PNG>' /data/local/tmp/motion-large-image-test.png
adb shell am instrument -w -r -e class com.motionstudio.editor.ImageResourcesTest -e largeImagePath /data/local/tmp/motion-large-image-test.png com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
adb shell am instrument -w -r -e class com.motionstudio.editor.MediaImportOrderTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
adb shell am instrument -w -r -e class com.motionstudio.editor.ImagePreviewCacheTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

`optionalLargeOriginalPng` 默认通过 Assume 跳过；本次实际传入本地原图，4 项均执行。测试包使用独立的 `com.motionstudio.editor.effectsacceptance` ID。

## 交付边界

本批移除了打开工程时整库图片解码/上传和大 PNG 的 Bitmap 64 MiB 限制，提供代理、DirectByteBuffer 输出及后续补充的近期纹理复用/片段预取。原图/活动纹理的 128 MiB 预算、效果临时纹理 64 MiB 预算及设备尺寸限制仍会影响复杂正式输出。超过预算时仍然明确失败；没有实现超大效果层的分块渲染、跨 Renderer 共享、非 sRGB/HDR 色彩管理或任意大图 JPEG 流式解码。0.5 秒的预取窗口不能保证任意冷缓存素材都在片段开始前准备好。

测试 APK 包含主分支及本 PR；2 GiB 视频/异步工程包能力来自另一个独立 PR，需合并后进行组合验证。接口与前端接入规则见 [host-image-resources.md](../host-api/host-image-resources.md)。本地原始日志、报告与安装包位于工作区 `artifacts/image-demand-resources/`，该目录被 Git 忽略。
