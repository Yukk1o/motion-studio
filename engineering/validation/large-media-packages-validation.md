# 大媒体与工程包验证

基准：`origin/main` 的 `0ca9665`；分支 `codex/large-media-resources-20261007`。源工程、参考目录和用户素材只在本地读取，不进入提交。

## 主机事务与资源验证

Windows 主机，Rust stable 1.97.1，运行：

```powershell
cargo test --locked --workspace --features motion-android/diagnostics -- --test-threads=1
```

全工作区结果：201 通过、0 失败、2 个大型文件用例默认忽略；其中新的大文件用例已按下文另行运行。补充源文件句柄尺寸检查后，直接受影响的 10 个工程包／视频事务用例再次通过。

新增用例覆盖：

- 快照建立后编辑 JSON、替换原素材路径，导出的描述和已打开源文件仍对应启动时内容。
- 64 KiB 复制中途取消、放弃 prepared 包、重命名失败和源长度变化，均清理任务暂存且保留已有输出。
- 工程包解压取消不发布目的目录，不改动原工程。
- 包任务进度、冻结 revision、终态查询、取消与发布顺序、任务记录释放。
- 视频声明长度超过旧 512 MiB 后继续执行长度校验；2 GiB 可接受，超过 2 GiB 拒绝；失败清理导入暂存。
- 视频、原声及片段共享源文件；备份中一个源 entry，恢复和缓存重建保持一致。

## 本地大源文件

对本地 1,885,147,688 字节 MP4/AAC 执行忽略默认 CI 的大文件测试。数据不会复制到仓库；测试临时目录结束后自行释放。主机视频探测为注入实现，实际 AAC 解码、复制、保存、ZIP CRC、恢复和完整内容对照均执行。

```powershell
$env:MOTION_LARGE_VIDEO_SOURCE='E:/local-input/large-mp4-with-aac.mp4'
$env:MOTION_LARGE_VIDEO_REPORT='E:/local-output/host-large-source.json'
cargo test --locked -p motion-media --test video_transactions large_source_roundtrip -- --ignored --nocapture --test-threads=1
```

输入需要 512 MiB 以上、2 GiB 以内，音轨索引 1 为可解码 AAC；另需约源文件三倍的临时磁盘空间。注入的视频探测沿用合成测试图的元数据，不能据此判断真实视频尺寸、帧率、MediaCodec 支持或预览画质。

| 检查 | 本地结果 |
| --- | --- |
| 源文件大小 | 1,885,147,688 字节 |
| 导入与 AAC 缓存 | 16,786 ms |
| Stored/ZIP64 打包 | 10,349 ms |
| 解压并验证工程 | 10,127 ms |
| 最大源读取请求 | 65,536 字节 |
| 包内文件数量 | 2：工程 JSON + 共享视频／原声源 |
| 完整恢复文件与原文件 | 相同，固定缓冲逐字节对照 |

耗时为该 Windows 主机单次测量，不是手机指标。64 KiB 是源复制缓冲，JSON、解码器、PCM 波形和 ZIP 中央目录另有各自资源，不代表总进程内存只有 64 KiB。

## Android 接口

构建和仪器测试命令：

```powershell
python tools/build_android.py --abis arm64-v8a,x86_64 --effects-acceptance --task assembleDebug assembleDebugAndroidTest testDebugUnitTest lintDebug
adb -s emulator-5554 shell am instrument -w -e class com.motionstudio.editor.PackageBackendApiTest com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

`PackageBackendApiTest` 使用项目原创测试素材验证独立上限查询、与能力声明一致、真实 MediaCodec 小视频导入、原声共享、打包期间编辑及切换工程、Java ZIP64 读取、恢复，以及取消清理与释放。arm64-v8a 和 x86_64 构建、10 个 JVM 单元测试、lintDebug 已通过；最终构建后的四个接口用例全部通过（包括可选真实大视频），0 失败、0 忽略。补充的 7 个媒体模块单元测试也通过，覆盖扫描等待预算边界。

另外提供可选的真实大视频导入用例，默认没有数据时跳过。用独立的本地可读路径输入，测试 APK 不包含用户素材：

```powershell
adb -s emulator-5554 push E:/local-input/large-mp4-with-aac.mp4 /data/local/tmp/motion-large-video.mp4
adb -s emulator-5554 shell am instrument -w -e class com.motionstudio.editor.PackageBackendApiTest#optionalLargeVideoImportsThroughTheRealAndroidDecoder -e large_video_path /data/local/tmp/motion-large-video.mp4 com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner
```

该用例执行真实 MediaExtractor、首帧 MediaCodec 解码、AAC 缓存、提交、共享源和重开，记录导入总耗时及提交后的 PSS／原生堆；这些内存快照不是峰值测量。最终 Android 15 / API 35 x86_64 软件解码模拟器实测通过：1,885,147,688 字节、3840×2160、60 fps、3470 帧；启动请求小于 1 ms（毫秒取整为 0），导入与提交 68,947 ms，提交后 PSS 126,008 KiB、原生堆 25,264,912 字节，重开成功。此前单独运行得到 96,326 ms、PSS 52,131 KiB；缓存、GC 和运行顺序会影响这些单次数字。该测量不代表手机性能或大视频预览验证。完成后删除该用例导入的独占源文件，报告和可重建缓存保留在测试私有目录。大视频播放与正式输出另需专门验证。

复跑曾暴露大文件时间戳扫描固定 30 秒的误判超时；已按源大小设置有界等待时间，并保留逐样本取消与索引限制。另一个字符串比较误报通过按 JSON 结构及 f32 参数语义对照修正，不改变工程序列化格式。

## 尚未验证或未交付

- Android 真机导入、手机耗时和预览连续播放；上述主机磁盘／模拟器导入测试不代替真机结果。
- 7952×3273 等大图片的低峰值解码、按需上传、预览代理；保持下一批独立实现。
- 前端工程输出按钮接入后台包任务；这里只提供接口和交接文档，旧输出按钮仍用同步打包。
- 完整 AE 工程打开、参数采集、画面还原与 AE 对照验收。
