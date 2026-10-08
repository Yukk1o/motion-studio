# 无 AE 实例的 AEP 结构采集

`tools/ae_source_graph.py` 补充 [AE 采集基准](host-ae-project-audit.md)：通过固定版本的开源 [py-aep](https://github.com/forticheprod/py-aep) 读取合成、源引用、图层开关、片段、属性和关键帧。它用于确认还原范围和生成本地证据，尚未提供 Motion Studio 的 AEP 导入入口，也不会生成可播放的宿主工程。

## 运行

推荐 Python 3.11，在独立环境中安装 `tools/ae-source-requirements.txt`。依赖不进入 Android 包或 Rust 运行时。适配器要求 py-aep 0.18.1，其他版本直接报错，升级后需要重新验证。

```powershell
python -m venv .tools/ae-capture-env
.tools/ae-capture-env/Scripts/python.exe -m pip install -r tools/ae-source-requirements.txt
.tools/ae-capture-env/Scripts/python.exe -X utf8 tools/ae_source_graph.py X:/private-input/project.aep artifacts/ae-capture --root-id 2 --media-root X:/private-input --properties
```

`--root-name` 可以替代 `--root-id`，同名合成必须唯一。省略 `--properties` 时只采集合成、图层、效果记录和素材信息，适合先核对入口。输入只读，输出必须在输入文件所属目录之外；写入最终 JSON 前检查源哈希没有变化，报告采用原子替换。不要让输入目录包含输出目录。

**JSON 包含原素材路径、字体、源文本及表达式，属于本地私有材料。** 放在已忽略的 `artifacts/`；不提交 AEP、素材、采集结果或参考帧。工具不执行表达式、不打开 AE、不 relink 素材，不保存或渲染源工程。

## 报告契约

输出 `ae-source-capture.json`，`schema=motion-studio-ae-source-1`，`state=captured_static`，`restorationReady=false`。

| 字段 | 含义 |
| --- | --- |
| `source` / `parser` | 输入 SHA-256、大小、解析器版本及保存的 AE 版本 |
| `root.duration` | 合成本身的时长，保持秒，不转换为宿主整数帧 |
| `root.reference_range` | 保存的工作区起点与时长；作为候选对照范围，须与成片或 AE 渲染核实 |
| `source_graph` | 从所选入口经过图层源引用可达的合成、缺失源、环、最长嵌套边数及重复展开实例数 |
| `compositions[].layers[]` | 1 起始图层索引、AE ID、类型、父级、源、开关、片段、拉伸、混合与轨道遮罩 |
| `properties` | 开启 `--properties` 后保留属性地址、matchName、原值、范围、关键帧秒数、插值、时间缓动、空间切线和表达式源码 |
| `footages[].main_source` | 源类型、纯色源颜色、Alpha 解释、原生/解释帧率、静帧和循环设置；有启用代理时另报未采集警告 |
| `footages[].local_resolution` | 仅提供本地路径候选，`applied=false`；不会修改工程 |
| `warnings` / `opaque_regions` | 具体地址上的不可读字段/插件对象，以及基础 RIFF 解析器保留的未解释区域 |

AE ID 与宿主 ID 使用不同命名空间，后续导入需要显式映射。属性地址使用合成 ID、图层 ID 和 1 起始属性索引路径；同名属性或效果不会因此合并。py-aep 的图层索引从 0 起始，本报告转为 1 起始。

枚举保留 `{name,code}`，不根据显示名称猜测语义。数值、向量、颜色直接保存；Shape、Gradient、Curves 和 TextDocument 通过明确列出的 getter 采集选定字段。Mask 组另保留模式、反转、RotoBezier 和羽化衰减设置。字体名称是工程记录，不代表本机已安装字体。TextDocument 尚未采集完整逐字符样式；未知插件对象需要追加适配或 AE 对照。

### 数据来源与验证界限

py-aep 会补出 AE 合成默认属性，因此属性与效果记录带 `origin=stored/parser_synthesized/unknown`。合成记录数、保存属性数和默认属性数必须分别统计，不能把全部输出项当作工程作者实际设置的项。参数 `min_value/max_value` 可能来自解析器内置定义；未经真实 AE 验证不能直接写入“已核实有效范围”表。

`value` 读取版本固定的解析器所提供的原始静态值；动画另外保留全部 `keys`，表达式仅保存字符串与开关。适配器不调用表达式求值或依赖解析。Shape、Text、Curves 等复杂值只采集已列明的字段；`CUSTOM_VALUE` 未解码时有警告，不能用默认值掩盖。

源图包括禁用、隐藏和范围外图层。它没有解析效果图层输入、表达式引用或时间重映射，也不是“当前帧可见图”。`max_nested_edges` 以根到叶的引用边计数，宿主可能按节点深度计数，比较限制时须先统一定义。`expanded_composition_instances` 是静态全部路径的重复展开数，不能当作同时活跃的解码器或纹理数量。

图中有环、缺失合成源或不可读源时，`complete_source_graph=false`，深度与实例数返回 null。共享 DAG 使用记忆化统计，不展开指数级路径。文件最多 512 MiB，树深最多 64，属性/关键帧各最多 200,000，参数值元素最多 10,000,000，最终 JSON 最多 128 MiB；超出限制整体失败，不能变成成功响应中的缺省内容。

素材按原路径末尾的最长相同分量匹配，重复文件名无法唯一匹配时返回 `ambiguous` 并列出候选。没有候选返回 `missing`。这只是重连建议，正式导入仍需要共享素材 ID、文件哈希和冻结引用契约。

## 后续实现与验收

本采集器和实际 AE 采集器输出的证据级别不同。`captured_static` 不等于 AE 已验证、效果兼容或可编辑工程已还原。需要用自建小工程的 AE 导出逐项做差分，再按目标源的片段核对文本、图层顺序、关键帧、遮罩、效果及声音。

前后端后续共同接入：保留源 ID 到宿主 ID 的映射、项目工作区、共享素材和不可读状态；导入前展示依赖与能力问题，失败不能生成假成功工程。原生预览和导出需使用同一合成图、参数与时钟。验收包括可编辑内容、约定工作区的整片播放/输出、保存重开和撤销重做。

```powershell
.tools/ae-capture-env/Scripts/python.exe -X utf8 -m unittest discover -s tools -p test_ae_source_graph.py -v
```

测试覆盖禁用源、共享 DAG、环、缺失/重复身份、同名入口、素材歧义、只读与原子写入、预算失败，以及自建 AEP 的小数关键帧、非整数帧率和工作区。自建项目由解析器写出，仍不能代替 AE 自身生成的真实对照样本。
