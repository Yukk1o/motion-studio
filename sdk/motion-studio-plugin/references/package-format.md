# .msfx 包与参数

`.msfx` 是 ZIP，根目录有 `manifest.json`，只包含声明引用的 WGSL、PNG 或旧版编辑器资源。路径相对包根，使用 `/`；禁止绝对路径、`..`、目录项、外部符号链接和忽略大小写后的重复名称。压缩包与展开总量各不超过 16 MiB，最多 256 项；manifest 与每个 shader 各不超过 256 KiB。

插件声明 `format_version:1`、最低 `sdk_version`、稳定 `id`、SemVer `version`、名称和 `effects`。每包 1–64 个效果，每效果最多 32 个参数、8 个 pass、4 个包内 PNG 资源。效果使用 `category` 分类，提供 `name` 与 `english_name`。

参数按 manifest 顺序对应 `fx.params[index]`，每项一个四浮点槽。支持 `float`、`integer`、`vector2`、`vector3`、`color`、`boolean`、`enum`、`curve_object`。声明四元素 `default`、有效 `min/max`、`step`、`units` 和 `animatable`；离散值应合法，连续值必须有限。颜色通常归一化，Alpha 是否可编辑取决于其范围；颜色通道增益不应误声明为颜色。

`curve_object` 是颜色传递曲线对象和宿主派生 LUT，区别于关键帧的时间缓动。协议 1 原生参数槽不能绑定它。

图像效果声明 `working_space`（`srgb` / `linear`）、`alpha_mode`（`straight` / `premultiplied`）、`edge_mode`（`transparent` / `clamp` / `repeat` / `mirror`）及 `required_capabilities`。宿主进行契约转换，不应默认所有算法使用线性预乘 Alpha。

完整例子见 [模板](../assets/effect-template/manifest.json)。模板使用 SDK 5 展示原生编辑页；只使用图像能力的效果可声明较低的最低 SDK。

插件版本和包 SHA-256 共同确定工程依赖。同版本同哈希重复安装幂等，同版本不同哈希拒绝安装；升级需显式执行，参数不会自动迁移。缺失依赖保留实例和动画；启用依赖失败时阻止正式导出。
