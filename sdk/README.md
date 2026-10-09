# Motion Studio 插件 SDK

面向第三方开发者的效果包契约、WGSL 接口、原生 UI 声明、模板和打包工具。宿主 JNI、前端接入与历史验收说明位于 [engineering](../engineering/README.md)。

SDK 提供自包含的 [motion-studio-plugin skill](motion-studio-plugin/SKILL.md)。将整个目录复制到所用工具的 skills 目录，即可用 `$motion-studio-plugin` 创建或修改效果，参考资料、脚本和模板随技能提供。

复制 `motion-studio-plugin/assets/effect-template`，修改插件 ID、版本、参数和 shader，然后运行：

```sh
python sdk/motion-studio-plugin/scripts/pack_msfx.py ./my-effect ./out/my-effect.msfx
```

只需 Python 3.10+，无需重建 Rust、Android 或 APK。打包器检查结构、文件与大小；App 导入完成完整参数、PNG 和两端 shader 校验。修改算法后提升版本，显式升级测试实例，再比较预览与导出。发布普通 `.msfx` 不要求重编译 App。

已有 `effect_tool` 二进制时，可增加 `--validator /path/to/effect_tool` 做完整离线检查。新增宿主编译器、渲染器或原生控件实现时，仍需宿主构建和 CI。

- [包与参数](motion-studio-plugin/references/package-format.md)
- [WGSL 接口](motion-studio-plugin/references/wgsl.md)
- [原生 UI 与共享组件](motion-studio-plugin/references/native-ui.md)
- [开发、测试与发布](motion-studio-plugin/references/development.md)

当前主分支支持 SDK 1–5；原生编辑页使用 SDK 5 / 协议 1。声明所需的最低 SDK 和能力，不依赖未发布接口。
