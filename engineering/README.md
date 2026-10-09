# Motion Studio 宿主开发资料

这里的接入说明与历史验证记录供软件维护者使用，不属于第三方插件 API。

- `host-api/`：Android/Rust、JNI、编辑与资源接口、宿主实现说明。
- `validation/`：具体版本、设备和分支的验证范围与限制；历史记录不能证明当前版本通过验收。

插件开发者从 [公开 SDK](../sdk/README.md) 开始，通过 `.msfx`、WGSL 和受控原生 UI 插槽使用能力，不直接调用内部 JNI。
