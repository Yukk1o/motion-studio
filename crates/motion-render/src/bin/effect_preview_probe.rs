//! Reproducible GPU-only effect playback probe. MediaCodec and display pacing
//! are deliberately outside this desktop measurement; use a phone for FPS.
use motion_core::{EffectInstance, Layer, Project, Scene, VideoSample};
use motion_render::{
    ChromaLayout, GpuTimer, Presenter, PreviewMode, PreviewPolicy, Renderer, Yuv420Frame,
};
use serde_json::{json, Value};
use std::{path::Path, time::Instant};

const SOURCE: [u32; 2] = [3840, 2160];
const SURFACE: [u32; 2] = [1080, 608];
const WARMUP: u64 = 4;
const FRAMES: u64 = 20;

fn fixture() -> Yuv420Frame {
    let [w, h] = SOURCE;
    let mut y = vec![0; (w * h) as usize];
    for row in 0..h {
        for col in 0..w {
            let dx = col as i32 - w as i32 / 3;
            let dy = row as i32 - h as i32 / 2;
            let value = if dx * dx + dy * dy < 160 * 160 {
                235
            } else if col > w * 2 / 3 && row > h / 2 {
                if (col / 24 + row / 24) % 2 == 0 {
                    220
                } else {
                    30
                }
            } else {
                16 + (col * 110 / w + row * 60 / h) as u8
            };
            y[(row * w + col) as usize] = value;
        }
    }
    Yuv420Frame {
        width: w,
        height: h,
        rotation: 0,
        standard: 1,
        range: 2,
        phase: [0, 0],
        chroma_layout: ChromaLayout::Uv,
        y,
        uv: [110, 155].repeat((w * h / 4) as usize),
    }
}

fn project(effect: &str, composition: [u32; 2]) -> Project {
    let [w, h] = composition;
    let mut p = Project::new(w, h, 30, 180).unwrap();
    p.background = [0.0, 0.0, 0.0, 1.0];
    let mut l = Layer::solid(
        1,
        "synthetic 4K YUV video",
        [SOURCE[0] as f32, SOURCE[1] as f32],
        [w as f32 / 2.0, h as f32 / 2.0, 0.0],
        [1.0; 4],
    );
    let scale = w as f32 / SOURCE[0] as f32 * 100.0;
    l.transform.scale.value = [scale, scale, 100.0];
    let package = motion_effects::builtin::package().unwrap();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == effect)
        .unwrap();
    let mut instance = EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        l.size,
    );
    // Directional Blur has a neutral default; exercise the actual expensive pass.
    if effect == "directional_blur" {
        instance.params.get_mut("p0002").unwrap().track.value[0] = 100.0;
    }
    match effect {
        "gaussian_blur" | "fast_box_blur" => {
            instance.params.get_mut("p0001").unwrap().track.value[0] = 4.0
        }
        "unsharp_mask" => instance.params.get_mut("p0002").unwrap().track.value[0] = 4.0,
        "simple_choker" => instance.params.get_mut("choke").unwrap().track.value[0] = 4.0,
        _ => {}
    }
    if effect == "fast_box_blur" {
        instance.params.get_mut("p0002").unwrap().track.value[0] = 1.0;
    }
    l.effects.push(instance);
    p.layers.push(l);
    p.rebuild_plugin_dependencies();
    p
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn measure(
    renderer: &mut Renderer,
    p: &Project,
    optimized: bool,
    output: &Path,
) -> Result<(Value, Vec<u8>), String> {
    let mut policy = PreviewPolicy::default();
    policy.set_mode(PreviewMode::Balanced);
    let dims = if optimized {
        policy.render_dimensions(p.width, p.height, SURFACE[0], SURFACE[1])
    } else {
        policy.tier().dimensions(p.width, p.height)
    };
    let source = renderer
        .render_target(dims.0, dims.1)
        .map_err(|e| e.to_string())?;
    let target = renderer
        .capture_target(SURFACE[0], SURFACE[1])
        .map_err(|e| e.to_string())?;
    let presenter = Presenter::new(renderer, &source.view, renderer.target_format);
    let mut timer =
        GpuTimer::new(&renderer.device, &renderer.queue).ok_or("GPU timestamps unsupported")?;
    let mut scene = Scene::new(p);
    let mut video = fixture();
    let mut gpu = Vec::new();
    let mut cpu = Vec::new();
    let mut texture_bytes = None;
    let mut passes = 0;
    let start_uploads = renderer.video_uploads;
    let start_upload_bytes = renderer.video_upload_bytes;
    for i in 0..WARMUP + FRAMES {
        let started = Instant::now();
        scene.sample(p, i as f64, None).map_err(|e| e.to_string())?;
        scene.layers[0].video = Some(VideoSample {
            asset: 1,
            source_time_us: i * 1_000_000 / 30,
        });
        video.y[0] = 16 + i as u8;
        renderer
            .upload_video_yuv(1, 1, i * 33_333, &video)
            .map_err(|e| e.to_string())?;
        let slot = timer.begin(i).ok_or("GPU timer slot leaked")?;
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        let stats = if optimized {
            renderer.encode_preview(
                &scene,
                &source.view,
                dims.0,
                dims.1,
                &mut encoder,
                Some(timer.writes(slot, 0)),
            )
        } else {
            renderer.encode(
                &scene,
                &source.view,
                dims.0,
                dims.1,
                &mut encoder,
                Some(timer.writes(slot, 0)),
            )
        }
        .map_err(|e| e.to_string())?;
        if !renderer.effect_diagnostics.is_empty() {
            return Err(renderer.effect_diagnostics.join("; "));
        }
        presenter.encode(
            &mut encoder,
            &target.view,
            Some(timer.writes(slot, 1)),
            None,
            p.background,
        );
        timer.resolve(slot, &mut encoder);
        renderer.queue.submit(Some(encoder.finish()));
        timer.map(slot);
        let cpu_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        // Benchmark synchronization only. Runtime's frame loop uses nonblocking
        // Poll and skips busy slots; it never waits for these measurements.
        renderer.device.poll(wgpu::Maintain::Wait);
        let timing = timer
            .collect()
            .into_iter()
            .flatten()
            .next()
            .ok_or("missing GPU timing")?;
        if i >= WARMUP {
            cpu.push(cpu_us);
            gpu.push(timing.total_us);
            if texture_bytes.is_some_and(|n| n != stats.texture_bytes) {
                return Err("steady texture allocation changed".into());
            }
            texture_bytes = Some(stats.texture_bytes);
            passes = stats.draw_calls;
        }
    }
    let pixels = renderer.read_target(&target).map_err(|e| e.to_string())?;
    image::save_buffer(
        output,
        &pixels,
        SURFACE[0],
        SURFACE[1],
        image::ColorType::Rgba8,
    )
    .map_err(|e| e.to_string())?;
    renderer.check_health().map_err(|e| e.to_string())?;
    Ok((
        json!({"renderSize":[dims.0,dims.1],"gpuMedianUs":median(&mut gpu),"cpuMedianUs":median(&mut cpu),
        "textureBytes":texture_bytes,"renderDrawCalls":passes,"steadyTextureBytes":true,
        "uploadedFrames":renderer.video_uploads-start_uploads,"videoUploadBytes":renderer.video_upload_bytes-start_upload_bytes,
        "warmupFrames":WARMUP,"measuredFrames":FRAMES,"timedImageReadbackBytes":0}),
        pixels,
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("usage: effect_preview_probe <ignored-artifact-directory>")?;
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir)?;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let mut renderer = pollster::block_on(Renderer::new_profiled(
        &instance,
        None,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        true,
    ))?;
    let info = renderer.adapter.get_info();
    let mut cases = Vec::new();
    for composition in [SOURCE, [1920, 1080]] {
        for effect in [
            "rays",
            "directional_blur",
            "glow",
            "glow_edges",
            "streaks",
            "glint",
        ] {
            let p = project(effect, composition);
            let label = format!("{effect}-{}", composition[0]);
            let before = measure(
                &mut renderer,
                &p,
                false,
                &dir.join(format!("{label}-before.png")),
            );
            let after = measure(
                &mut renderer,
                &p,
                true,
                &dir.join(format!("{label}-after.png")),
            );
            let row = match (before, after) {
                (Ok((before, a)), Ok((after, b))) => {
                    let mae = a
                        .iter()
                        .zip(&b)
                        .map(|(x, y)| f64::from(x.abs_diff(*y)))
                        .sum::<f64>()
                        / a.len() as f64;
                    let speedup = before["gpuMedianUs"].as_f64().unwrap()
                        / after["gpuMedianUs"].as_f64().unwrap();
                    println!(
                        "{label}: {:.2} -> {:.2} GPU ms ({speedup:.2}x), RGBA MAE {mae:.3}",
                        before["gpuMedianUs"].as_f64().unwrap() / 1000.0,
                        after["gpuMedianUs"].as_f64().unwrap() / 1000.0
                    );
                    json!({"effect":effect,"composition":composition,"before":before,"after":after,"gpuSpeedup":speedup,"previewRgbaMae":mae})
                }
                (before, after) => json!({"effect":effect,"composition":composition,
                    "beforeError":before.err(),"afterError":after.err()}),
            };
            cases.push(row);
        }
    }
    // Compare package algorithms at identical preview density, independently
    // of the composition-sized versus projected-density measurements above.
    let previous = motion_effects::builtin::packages()?
        .into_iter()
        .find(|p| p.manifest.id == motion_effects::builtin::PLUGIN_ID && p.manifest.version == "1.4.0")
        .ok_or("published core 1.4.0 is missing")?;
    let mut loop_cases = Vec::new();
    for effect in [
        "gaussian_blur",
        "fast_box_blur",
        "unsharp_mask",
        "simple_choker",
    ] {
        let current = project(effect, [1920, 1080]);
        let mut old = current.clone();
        old.layers[0].effects[0].version = previous.manifest.version.clone();
        old.layers[0].effects[0].hash = previous.hash.clone();
        old.rebuild_plugin_dependencies();
        let (before, a) = measure(
            &mut renderer,
            &old,
            true,
            &dir.join(format!("{effect}-1.4.0.png")),
        )?;
        let (after, b) = measure(
            &mut renderer,
            &current,
            true,
            &dir.join(format!("{effect}-1.4.1.png")),
        )?;
        let speedup =
            before["gpuMedianUs"].as_f64().unwrap() / after["gpuMedianUs"].as_f64().unwrap();
        let identical = a == b;
        println!(
            "{effect} package loops: {:.2} -> {:.2} GPU ms ({speedup:.2}x), identical={identical}",
            before["gpuMedianUs"].as_f64().unwrap() / 1000.,
            after["gpuMedianUs"].as_f64().unwrap() / 1000.
        );
        if !identical {
            return Err(format!("{effect}: package loop output changed").into());
        }
        loop_cases.push(json!({"effect":effect,"before":before,"after":after,"gpuSpeedup":speedup,"identicalPixels":identical}));
    }
    let report = json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"deviceType":format!("{:?}",info.device_type),
        "source":SOURCE,"surface":SURFACE,"mode":"balanced","mediaCodecMeasured":false,"phoneFpsMeasured":false,
        "baseline":"previous composition-sized preview without projected layer density","cases":cases,
        "packageLoopComparison":{"beforeVersion":"1.4.0","afterVersion":"1.4.1","samePreviewDensity":true,"cases":loop_cases}});
    std::fs::write(dir.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    if report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["afterError"].is_string())
    {
        return Err("one or more preview cases failed; inspect report.json".into());
    }
    Ok(())
}
