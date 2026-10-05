# JNI 渲染计划协议 2

`renderPlanInfo(session)` 做依赖预检并返回 version=2、uniformBytes=624、passBytes=40、spriteBytes=48、bufferBytes（安全容量）、assetBytes（含白色纹理的素材字节数）和 programs。每个 program 为 key、glsl（vertex/fragment/blocks/textures）、resources（path/width/height）、sprite、additive。通过 `pluginPixels(session,programIndex,resourceIndex)` 一次读取RGBA8；图层素材继续用 assetPixels。相同包hash与path的PNG跨pass复用，素材与插件资源合计最多128 MiB。整个导出期间保留同一 session。

`sampleRenderPlanInto(session,integerFrame,directByteBuffer)` 返回写入字节数，失败为-1。缓冲区为 native endian、direct，容量至少 info.bufferBytes；样本不通过 JSON。应用不能把 header 中的版本/长度错误当成空画面。

Header 为16个u32，共64字节：

| word | 含义 |
|---|---|
| 0 / 1 | magic=0x46584d53 / version=2 |
| 2 / 3 | draw数 / pass数 |
| 4 / 5 / 6 / 7 | draw字节offset / pass字节offset / uniform字节offset / 总字节数 |
| 8 / 9 / 10 | 纹理池宽 / 高 / slot位图 |
| 11 / 12 | LUT字节offset / LUT数 |
| 13 / 14 / 15 | 实例区字节offset / 实例数量 / 实例大小48 |

Draw 为32个f32，共128字节。0～15列主序MVP，16～19线性色彩，20～21最终区域宽高，22图层透明度，23最终additive合成开关，24素材索引，25～26纹理UV比例，27处理结果slot（-1表示原素材），28～29本图层pass开始/结束索引。其它保留。slot处理和后续合成必须逐图层执行，不能先执行全部pass再合成；纹理池被不同图层复用。素材索引0是白色1×1，随后按工程assets顺序。

Pass 为10个u32，共40字节：programIndex、input（按i32解释）、source（i32）、outputSlot、实际宽、实际高、uniform字节offset、LUT字节offset（0xffffffff无LUT）、sprite_start、sprite_count。input/source负值 `-(assetIndex+1)` 指素材，非负值指纹理池slot。

实例位于LUT区之后，每项12个f32，共48字节：rect（NDC中心和完整宽高）、color（线性预乘）、style，各4个数字。仅sprite程序读取实例范围，普通图像程序使用全屏三角形。场景行为和style约定见 [场景运行时](../scene-effects/runtime.md) 与 [SDK 2](../scene-effects/sdk.md)。

池有slot0～7，slot0及4～6为RGBA8 sRGB目标，1～3为RGBA8 UNORM，7为RGBA16F线性累积；同尺寸高水位池受64 MiB限制，slot7按每像素8字节计费。pass按实际尺寸设置viewport、清空全目标；普通pass禁用blend，sprite pass按程序的alpha/additive模式混合。输入不能与当前输出相同。输出slot0最后由线性预乘合成器绘制，按draw[23]选alpha/additive，UV比例限制到有效区域。参数块布局见SDK；LUT固定256×1 RGBA8、1024字节。

GLSL 的 blocks 名称由反射返回，绑定到一个624字节UBO；sampler名称映射为[group,binding]，group1的texture bindings0/2/4是input/source/LUT，group2的0/2/4/6是四个PNG。宿主布局允许资源未使用；不要猜测Naga变量名称。Naga顶点代码包含GL坐标修正，不要额外翻转效果纹理；素材首行与处理纹理的logical Y=0一致。

已提供 `GlEffects.kt` 和更新后的 `VideoExporter.kt` 渲染适配，供前端复用；这两个文件负责GPU导出，不包含效果编辑界面。原sampleInto保留供无效果旧调用方使用，启用效果时明确失败。新增协议为后续可变数量参数和pass预留了版本检查。
