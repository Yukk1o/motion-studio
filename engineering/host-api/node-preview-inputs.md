# 真实素材预览输入 API

用于桌面节点及其他效果创作工具。后端提供工程拥有的图片、文字栅格素材和视频图层原始输入，不改变工程、播放头、选择、撤销历史或渲染缓存。UI 使用以下接口选择和显示输入

## 宿主入口

`motion_host::ops::preview_inputs`：

| 方法 | 作用 |
| --- | --- |
| `choices(session)` | 返回输入目录、revision、最大边长与颜色契约 |
| `request(session, json, expected_revision)` | 提交请求，拒绝不同工程 revision |
| `poll(session, sequence)` | 轮询 `InputPoll::Pending(info)` 或 `Ready(packet)`，拒绝过期 revision/sequence |
| `upload(session, sequence, &mut texture)` | 在会话所属线程轮询并上传，返回 requested input 的 pending/ready JSON |
| `cancel(session)` | 取消输入并使尚未安装的结果失效 |

请求示例：

```json
{
  "sequence": 1,
  "source": {"kind":"layer","composition":"comp-main","object":8},
  "frame": 21.5,
  "max_edge": 512
}
```

也可使用 `{"kind":"image_asset","asset":1}`。`frame` 是所选合成的帧，支持小数；图片素材使用当前合成的时钟。请求上限为 16 KiB，字段严格解析，max_edge 范围为 1..=2048。序号需单调递增，取消及切换工程后也不要重置；可重复当前完全相同的请求，不能把同一序号分配给不同参数

后端原始输入只读取文件和缓存，不安装/修改效果包。隐藏图层可以作为输入；图层裁剪及视频源区间以外返回透明输入，精确片段结束时刻也是透明，不采样另一帧来假装仍在范围内

宿主存在未结束的工程编辑手势时暂不接受新输入或安装结果；UI 保留上一份有效纹理，手势提交后使用新 revision 重新请求。独立 reader 应传入不可变工程快照

## GPU 使用

创建一次独立输入纹理容器，使用已有预览 Device/Queue，不必新建 GPU 设备：

```rust
let mut texture = motion_render::preview_input::PreviewInputTexture::new(
    device.clone(), queue.clone(),
    motion_render::preview_input::DEFAULT_INPUT_BUDGET,
);
let status = motion_host::ops::preview_inputs::upload(session_id, sequence, &mut texture)?;
if status["state"] == "ready" {
    // texture.view() 直接用于 SDK 的输入/原始来源纹理绑定
    // texture.dimensions() 为真实栅格尺寸
}
```

独立 `PreviewInputReader` 也可通过 `new(Arc<dyn Platform>)`、`request(&project, root, revision, request)`、`poll(sequence, revision)` 使用；Ready 的 `PreparedInput::upload(&mut texture)` 与宿主入口相同。自建 reader 在项目切换时应调用 cancel，生命周期不要跨不同 Session 复用旧序号

返回的视图采样结果始终为 **linear / premultiplied RGBA**。图片复用已有解码器的线性预乘存储；straight codec RGBA 在 GPU 上预乘后滤波，透明像素颜色不会污染缩放边缘。YUV 视频通过已有 GPU 转换器直接生成受限栅格纹理，保留旋转、色度相位、矩阵与范围。正常路径没有 GPU→CPU 回读，也不通过 PNG 中转

默认输入纹理预算为 64 MiB，计入输出、RGBA 临时输入与 YUV planes；尺寸/预算/字节长度校验在修改当前纹理前执行。失败时当前有效输入仍可显示。临时纹理和视频 planes 随来源类型切换释放，同尺寸输出及视频 planes 复用。预算口径为该对象拥有的像素纹理分配，不是整机显存统计

## 尺寸、时间与颜色

`InputInfo` 包含：

- `sequence/revision/source`：结果归属，安装到预览前仍须检查当前请求
- `logical_size`：图片素材的原始尺寸，或图层的逻辑尺寸；供 shader 的 size/region 使用
- `source_size`：原始图片尺寸或旋转后视频显示尺寸
- `raster_size`：受 max_edge 约束并保留源比例的输入像素尺寸，透明输入为 1×1
- `clock.composition_frame/local_frame/seconds/fps`：合成帧、扣除片段 offset 的局部帧/秒及该合成 fps
- `clock.source_time_us`：视频源时间，包含 source_offset_us
- `video_pts_us/video_end_us`：Ready 视频帧实际覆盖区间，保留 VFR 源 PTS
- `working_space/alpha_mode`：分别为 linear 和 premultiplied，设置 SDK input mode 后由 SDK 转换到效果声明的工作空间
- `stage/active`：当前为 original；active 指片段与源时间是否在范围内，不把隐藏开关视为源失效

输出预览区域应按 logical_size 排布，不能强制方形。输入 raster_size 不是 shader 的逻辑分辨率。Time/FrameIndex 使用返回的局部时钟，不能继续把真实图层输入固定成 64×64 / 30 fps

当前目录覆盖原始图片、文字栅格和视频来源；效果后图层、整合成、外部未导入纹理等入口需独立资源契约，目录不会用占位结果标记它们已支持

## 验收

```sh
cargo test -p motion-host --locked --test preview_inputs -- --test-threads=1
cargo test -p motion-render --locked --test preview_input --test video_yuv -- --test-threads=1
```

用例覆盖真实透明 PNG/竖图、图层逻辑尺寸、小数帧与裁剪偏移、隐藏来源、VFR PTS、旋转、过期请求/取消、源文件变化，以及 GPU 预乘/缩放/透明边缘/预算保护。视频时序用确定性平台解码器隔离调度，实际 codec/PTS 兼容性还由现有 libav 套件验收
