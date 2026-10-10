# Motion Studio MCP

同一个程序提供两种 stdio 启动方式。

| 参数 | 行为 |
| --- | --- |
| `--mcp` | 无窗口工程会话，适合脚本与批处理 |
| `--mcp-ui` | 打开桌面窗口，MCP 与用户共享同一工程、时间轴、撤销历史和渲染会话 |
| `--mcp-read-only` | 工具只允许读取状态，禁止编辑、跳帧、历史操作和保存 |

已有独立编辑器打开同一工程时，新的进程会报告工程占用。`--mcp-ui` 由客户端启动一个共享编辑器实例，不附着到另一个已运行的进程。

## 客户端配置

将路径替换为自己的程序与工程目录：

```json
{
  "mcpServers": {
    "motion-studio": {
      "command": "C:/MotionStudio/motion-studio.exe",
      "args": ["--mcp-ui", "--project", "E:/MotionProjects/FirstProject"]
    }
  }
}
```

批处理时将 `--mcp-ui` 换为 `--mcp`。应用日志写入 stderr，stdout 只输出 UTF-8 JSON-RPC 消息。

## 工具

| 名称 | 参数 | 结果 |
| --- | --- | --- |
| `motion_state` | `{}` | 工程、revision、当前帧、组合属性、时间轴与桌面会话信息 |
| `motion_edit` | `commands`, `expectedRevision` | 原子命令批次；一次批次对应一次撤销 |
| `motion_seek` | `frame` | 在 `[0, frames)` 内定位，允许小数帧 |
| `motion_history` | `operation: "undo" / "redo"` | 撤销或重做 |
| `motion_save` | `{}` | 校验资源并保存当前工程 |

先调用 `motion_state`，将结果的 `revision` 原样传给 `motion_edit`。版本不一致时拒绝整个批次，请重新读取状态后再决定修改。用户的鼠标编辑手势进行中时，agent 编辑同样会被拒绝。

```json
{
  "commands": [
    {
      "op": "add_shape",
      "id": 1,
      "name": "Rectangle",
      "shape": "rectangle",
      "size": [400, 240],
      "position": [960, 540, 0]
    }
  ],
  "expectedRevision": 0
}
```

`expectedRevision` 与图层 ID 必须使用刚读取的实际值。其他命令沿用共享核心 JSON API，包括关键帧、独立轴、曲线、父级、片段与效果参数。参数和命令名保持语言无关；工具结果同时提供文本与 `structuredContent`，操作错误返回 `isError`。

本实现使用 [MCP stdio 传输](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、[初始化流程](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)及[工具协议](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)，协商 `2025-11-25` 或 `2025-06-18`。单条输入限制 256 KiB；未初始化会话、未知工具、未知参数与超限输入都有明确错误。没有提供 HTTP 监听或任意系统命令工具。

## 内置 agent 预留

`ToolRouter` 可直接绑定窗口现有的 `Arc<Engine>`，用 `Access::ReadOnly` 或 `Access::Edit` 选择权限。MCP 的 stdio 只是适配器；后续内置 agent 无需启动第二份工程或绕过编辑核心。

共享窗口模式通过 winit 用户事件通知界面刷新。GPU 呈现与窗口尺寸更新异步提交，界面线程持续处理操作系统消息；效果与参数修改仍在同一个 session worker 上顺序执行。
