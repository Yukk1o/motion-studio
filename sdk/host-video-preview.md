# 视频预览后端与前端接入

本轮优化不改变工程格式、`.msfx`、SDK 3 或渲染计划协议 3，也不增加前端控件。

## 后端已实现

- 默认 SDR YUV420 解码结果保留为 Y + UV；CPU 只按 stride/pixelStride 打包，WGSL 在 GPU 上完成 BT.601/BT.709、full/limited range、crop 相位与 0/90/180/270 度旋转。保留原有整数转色和 clamp 规则。
- 视频输入纹理、参数 buffer 和转色 pipeline 复用；相同素材/PTS/尺寸跳过上传。紧密排列、偶数 crop 的 1080p YUV420 上传 3,110,400 字节，RGBA 原路径为 8,294,400 字节。这是载荷计算，不是帧率测量。
- 每个活动视频流缓存当前源帧和最多两个后续源帧。decode-ready 自动推进预取，不需要另一次 render 请求。1080p 默认 YUV 帧受字节上限限制，通常缓存当前帧及下一帧。
- 每流存储像素上限 8 MiB，每 reader 最多四流，因此源帧缓存上限 32 MiB。Codec/ImageReader、正在打包的帧、临时 JNI RGBA 和 GPU 纹理独立计量；32 MiB 不是整个应用内存上限。GPU 输入平面计入既有 128 MiB 图像/插件资源预算。
- 按实际 PTS 区间命中，支持 VFR；同一源帧内的组合时间不会反复取消解码。跳转、逆向、素材变化和释放会取消失效任务，保留准确寻帧。
- 每轮有界地喂入最多八个当前可用 Codec 输入，再排出输出；等待仍在解码 worker，不阻塞 UI。

ImageReader 的 RGB 格式兼容回退仍使用 CPU 像素搬运/旋转。预览 YUV 路径减少 CPU 转色，不是 AHardwareBuffer 零 CPU 拷贝；后续原生纹理接入仍需单独验证。

## 前端调用约定

现有 `NativeBridge.render(id, frame)` 可直接使用新预取，无需改变参数。冷启动、seek 或解码不足时仍可能返回 false，表示该目标尚未呈现；保持上一张完整组合画面，稍后重试，不要把 false 当成 GPU 故障。原有 pending 目标固定策略可以保留，避免无预取命中时反复取消任务。

`state.data.lastPresentedFrame` 仍是实际成功呈现的组合时间。时间轴的目标时间与该字段可以不同，不能把调用 render 的次数当成真实呈现帧数。不同图层不混用不同组合时间的像素，缺少准确源帧时仍保留整张上一结果。

暂停/精确静帧和 PNG capture 必须准确匹配目标，不能以旧画面代替；捕获未就绪时按原约定重试。`MediaBridge.request_video_frame`、`readVideoFrameInto` 和冻结导出接口保持原来的 sequence、精确 PTS 和 RGBA8 契约。CPU RGBA 只在这些显式像素消费者或缩略图请求时生成，MP4 导出继续按冻结工程准确取帧。冻结 reader 不启用前向预取，不共享编辑器的可变帧状态。

## 诊断字段

`NativeBridge.previewInfo(id)` 的 `data.video`，以及 `state.data.preview.video` 包含：

| 字段 | 含义 |
| --- | --- |
| `renderAttempts`, `pendingAttempts` | session 累计渲染尝试、因视频未就绪而未呈现的次数，后者也计入观察窗口 |
| `lastPrepareUs`, `lastUploadUs` | 最近一次供帧检查、CPU 上传调用耗时；上传调用返回不代表 GPU 已完成转色 |
| `cacheBytes`, `cacheFrames`, `cacheBudgetBytes` | 当前 reader 存储源帧的字节、数量与上限 |
| `cacheHits`, `cacheMisses` | 当前流集合中新目标请求的命中/缺失计数，同一目标重试不重复计算 |
| `decodedFrames`, `cancelledFrames`, `streams` | 当前流集合的解码结果、失效结果和流数量 |
| `uploadBytes`, `uploads`, `gpuConversions` | 当前 renderer 生命周期的像素上传载荷、上传次数及 GPU 转色次数 |

释放/替换 reader 或 Surface 后相应计数会重置，不同生命周期不能直接相减。不同视频图层取不同源时间时仍保有各自纹理。

取帧状态及 RGBA handoff 另含 `codec_us`、`transfer_us`、`pack_us`。`codec_us` 包括 Codec 输入 I/O 与输出等待，并非 VPU 纯执行耗时；`transfer_us` 包括 ImageReader 等待；`pack_us` 包括 plane 访问与打包，RGB 回退还包括其 CPU 转换。`decode_us` 保留本次 Codec 排队、取帧与打包阶段的总耗时，不含 decoder 创建和 seek/flush。导出时额外的 YUV→RGBA 和 JNI copy 不包含在这些取帧计时中。

现有 GPU timer 记录合成/效果 pass；视频转色在先行提交中，不能把该 timer 当作视频转换加合成的全部 GPU 成本。完整呈现期限、GPU 工作、PSS 与温控仍需 Perfetto/真机测量。

## 验证入口

- `cargo test --locked -p aem-render --test video_yuv --test video_instances`：矩阵/范围、四种旋转、奇数 crop、输入 stride、GPU/CPU 结果与纹理复用。
- `cargo test --locked -p aem-android --lib`：VFR 区间、窗口失效及字节上限。
- Android `VideoBackendApiTest`：真实 Codec 寻帧、冻结 reader、RGBA 输出；新增用例验证没有更多 render 请求时预取仍推进、下一张预取帧立即可呈现，以及逆向/跳转的 GPU 捕获一致性。
- `tools/validate_video.py`：Android 实际帧与项目生成素材的 FFmpeg 对照。

竞品 APK、反汇编、reference/refer 和本地测量材料不属于此文档的交付内容。
