# 效果空间边界回归验证

## 已验证的范围

本轮修复包含输出矩形原点传递、Core Effects 1.4.0 的 Shake / Transform Motion Blur / Polar Coordinates，以及显式保留参数升级。通用前端效果面板没有改动，接入约定见 [空间边界文档](../host-api/host-effect-spatial-bounds.md)。

Windows DX12 后端执行整个 workspace：**156 项测试通过，0 失败**。

```powershell
$env:WGPU_BACKEND='dx12'
cargo test --offline --locked --workspace --config profile.test.debug=0 --config profile.test.incremental=false -j1 -- --test-threads=1
```

| 检查 | 证据 |
| --- | --- |
| Shake 完整平面移动 | `motion-render/tests/spatial_effects.rs`：不损失超过 4% 的源 Alpha 覆盖，重心移动，原矩形之外出现非透明像素，原属性不变 |
| 随机寻帧与预览缩放 | 同一帧重复输出完全一致；半分辨率重心与原分辨率对齐误差小于 2 像素 |
| 效果叠加 | Shake 后叠加 Tint，Alpha 覆盖不变，原点不被丢弃 |
| 变换与效果透明度 | 完整平面按指定偏移移动；50% 效果透明度同时保留原像素与移动后的像素，原矩形以外不被边缘重复填满 |
| 极坐标 | 完整展开尾部在原矩形下方仍可见；顶部位置不重新居中；插值为零输出与无效果完全相同 |
| 3D 与非居中锚点 | 不对称输出矩形的四角符合原模型矩阵，包括 XYZ 旋转、非均匀缩放和非居中锚点 |
| 预算与边界算术 | 大偏移明确返回预算/尺寸错误；除零、负平方根、非有限/过大结果被拒绝；Select 惰性分支和 SDK 版本校验正确 |
| 旧版依赖 | 已发布 1.0–1.3 包及哈希保持不变；1.3.0 精确等于基准主分支的包字节；59 项参数定义不变 |
| 既有功能 | workspace 原有平面遮挡、粒子/镜头光效、效果库、视频 YUV、媒体、表达式和工程测试通过 |

## Android 验证入口

`SpatialEffectsTest` 覆盖新空间效果的 Android/GLES 编译、未编码 RGB/Alpha 对照、192×192 MP4 的 12 帧导出、0/7/11 帧对照，以及显式升级的关键帧/种子/启停/顺序/撤销重做/保存恢复。升级契约不相同时必须原子拒绝。

未编码输出阈值：RGB 和 Alpha MAE 均不超过 3/255；MP4 沿用全画面 RGB MAE 小于 6、前景小于 8 的阈值。测试在原图层矩形之外独立检查非透明像素，防止两端同时裁切而仅靠低误差通过。

构建使用 `tools/build_android.py --abis x86_64 --codegen-units 8 --effects-acceptance --task assembleDebug assembleDebugAndroidTest`。验收 App 使用独立 application ID `com.motionstudio.editor.effectsacceptance`，工程和报告位于测试私有目录。

API 35 x86_64 模拟器、Google SwiftShader GLES 3，执行 **3 项测试全部通过**，总耗时 107.26 秒：`SpatialEffectsTest` 的 2 项测试，加 `EffectsRuntimeTest#allFiftyNineEffectsCompileOnDeviceAndUnencodedGlesMatchesWgpu` 的旧版 59 效果回归。三个新版空间效果各导出 12 帧，合计 36 帧，对照其中 9 帧。

```powershell
adb -s emulator-5554 shell am instrument -w -r -e class 'com.motionstudio.editor.SpatialEffectsTest,com.motionstudio.editor.EffectsRuntimeTest#allFiftyNineEffectsCompileOnDeviceAndUnencodedGlesMatchesWgpu' com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

下表误差单位为 0–255；MP4 列取各效果 0/7/11 帧的最大 MAE。新空间测试输入为 32×20 不透明白色图像，原矩形外像素列统计第 7 帧 Alpha 大于 200 的像素；旧版 59 效果回归另使用彩色半透明渐变图。

| 效果 | 原矩形外像素 | 未编码 RGB MAE | 未编码 Alpha MAE | MP4 RGB MAE | MP4 前景 MAE |
| --- | ---: | ---: | ---: | ---: | ---: |
| Shake | 406 | <0.000001 | 0 | 0.01745 | 0.63282 |
| Transform Motion Blur | 252 | 0.22190 | 0.000513 | 0.00633 | 0.10313 |
| Polar Coordinates | 252 | <0.000001 | 0.002211 | 0.01973 | 0.31223 |

GLES 驱动兼容修复将 fullscreen/sprite 顶点的常量数组动态索引改为等价的位运算/条件选择；无 compute 支持的 GLES adapter 使用对应 downlevel 限制。host headless 初始化遵循 `WGPU_BACKEND`，使 DX12 和软件 GLES 验证能够选择实际支持的后端。以上均不改变渲染计划协议。

## 包及限制

- Core Effects 1.4.0 SHA-256：`0a0d38bf1c01aafca75416c93bbf0bd615cabd9246be2b8a041c3bdfde4c4ef9`。
- 保留的 Core Effects 1.3.0 SHA-256：`a2655482198ebc3a1e75b569a8d5e14d8232c9ad2445c24fc9badfb6274aa454`。
- `.msfx` 中的 60 个文件与当前 manifest / WGSL 源文件逐字节一致；manifest 为 252242 字节，保持原有 256 KiB 限制。
- 通用矩形投影修复影响宿主；三个空间效果的新边界和采样行为仅由 1.4.0 实例启用。旧工程仍要求显式升级，不自动改变参数和关键帧。
- 包内效果仍标为近似实现，本轮不是 AE 参考像素验收。极坐标中间插值仍是反向坐标近似。
- 保守外扩需要额外 GPU 纹理空间；极端合法参数仍可能超过 64 MiB 或设备尺寸。此类错误不会通过裁切隐藏。
- Android 软件 GLES 验证不代表 ARM 真机性能或连续 Surface 播放已测量；没有提供真机帧率结论。测试 APK 仅含 x86_64。
- 本地日志、图像、MP4、研究资料及编译产物不进入 Git。
