---
name: motion-studio-plugin
description: 为 Motion Studio 创建、修改和测试 WGSL 效果包、参数与原生编辑页，生成可导入的 .msfx。适用于插件开发；宿主 Rust、Android 和内部 JNI 实现属于独立工作。
---

# Motion Studio 插件开发

交付可导入 App 的效果包、明确参数契约与验证结果。技能自包含：模板在 `assets/effect-template`，打包器在 `scripts/pack_msfx.py`；不依赖宿主源码或内部研究目录。

先按用户目标选择已有宿主能力，确认所用 App 的 SDK 与能力版本。当前公开版本为 SDK 1–5，原生 UI 使用 SDK 5 / 协议 1。读取 [包与参数](references/package-format.md)，使用稳定 ID，明确含义、单位、默认值、有效范围和动画标记。一个颜色用一个 `color` 参数；独立通道运算保留自身语义。

实现算法时读取 [WGSL](references/wgsl.md)。使用图层局部像素和宿主采样函数，声明色彩、Alpha、边缘与输出边界。随机效果固定种子、使用局部时钟，支持倒序和随机寻帧。不要通过缩小合法参数或裁切来隐藏资源失败。

专属页读取 [原生 UI](references/native-ui.md)：声明预览、共享时间轴与参数分组，由宿主复用颜色和参数控件，页内外修改同一份轨道。不要生成 WebView/JS 代替原生组件，也不要编造未发布的槽名。缺少能力时说明具体缺口，同时完成不依赖它的效果工作。

普通效果按 [开发流程](references/development.md) 打包：

```sh
python <skill目录>/scripts/pack_msfx.py <插件源码目录> <输出文件.msfx>
```

无需 Rust/JDK/Android 构建。有现成 `effect_tool` 时增加 `--validator <可执行文件>` 做完整离线检查。App 导入仍检查参数、资源和两端 shader；打包成功不能代替画面验收。修改后提升版本并显式升级测试实例，不覆盖同版本不同哈希的包。

纯 WGSL/描述/资源修改无需宿主 APK 重编译或完整 Android CI。宿主编译器、渲染器、新能力或原生控件实现改动应独立检查。发布 `.msfx` 不要求重编译 App，外部发布按用户已授权范围执行。

验证默认、代表与边界参数、半透明边缘、链顺序、动画、倒序与随机寻帧，比较预览和未编码输出。按实际设备与普通工作分辨率记录性能；效果身份、版本与色彩条件未匹配时，不将同名或相似画面写成已还原。交付包路径、版本、能力要求、实际验证范围和已知差异。
