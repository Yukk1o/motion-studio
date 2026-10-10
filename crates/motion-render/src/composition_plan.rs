//! Versioned, postorder GPU composition bundle. Child pixels stay on the GPU.
use crate::effect_plan::PlanBuilder;
use crate::Scene;
pub const MAGIC: u32 = 0x4243534d;
pub const VERSION: u32 = 1;
pub const HEADER: usize = 32;
pub const NODE: usize = 80;
pub const VIDEO: usize = 40;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
struct Entry<'a> {
    scene: &'a Scene,
    reference: u64,
    parent: usize,
    slot: i32,
}
fn entries<'a>(s: &'a Scene, reference: u64, slot: i32, out: &mut Vec<Entry<'a>>, depth: usize, visited: &mut usize) -> Result<usize, String> {
    if depth >= motion_model::composition::MAX_COMPOSITION_DEPTH || *visited >= motion_model::composition::MAX_RENDER_COMPOSITION_INSTANCES {
        return Err("composition frame exceeds depth or active instance limit".into());
    }
    *visited += 1;
    let children = s
        .nested
        .iter()
        .map(|n| {
            let layer = s
                .layers
                .iter()
                .find(|l| l.id == n.layer)
                .expect("sampled reference");
            entries(&n.scene, n.layer, -(layer.order as i32 + 1), out, depth + 1, visited)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let index = out.len();
    out.push(Entry {
        scene: s,
        reference,
        parent: usize::MAX,
        slot,
    });
    for child in children {
        out[child].parent = index;
    }
    Ok(index)
}
fn word(out: &mut [u8], offset: usize, v: u32) {
    out[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
}
fn long(out: &mut [u8], offset: usize, v: u64) {
    out[offset..offset + 8].copy_from_slice(&v.to_le_bytes());
}
pub fn build(
    builder: &mut PlanBuilder,
    scene: &Scene,
    project: &motion_model::Project,
    assets: &[u64],
    out: &mut Vec<u8>,
) -> Result<(), String> {
    let mut nodes = Vec::new();
    out.clear();
    let root = entries(scene, 0, 0, &mut nodes, 0, &mut 0)?;
    out.resize(HEADER + nodes.len() * NODE, 0);
    for (i, e) in nodes.iter().enumerate() {
        let plan = builder
            .build(e.scene, assets, e.scene.width, e.scene.height, true)
            .map_err(|error| e.scene.diagnostic(&error))?;
        let size = plan.buffer_bytes(e.scene);
        let start = out.len();
        let videos = e
            .scene
            .layers
            .iter()
            .filter_map(|l| l.video.as_ref().map(|s| (l, s)))
            .collect::<Vec<_>>();
        if start + size + videos.len() * VIDEO > MAX_BYTES {
            return Err("composition frame bundle exceeds 32 MiB".into());
        }
        out.resize(start + size + videos.len() * VIDEO, 0);
        plan.write(e.scene, &mut out[start..start + size])?;
        let d = HEADER + i * NODE;
        long(out, d, e.reference);
        word(out, d + 8, e.parent as u32);
        word(out, d + 12, e.slot as u32);
        word(out, d + 16, e.scene.width);
        word(out, d + 20, e.scene.height);
        for c in 0..4 {
            word(out, d + 24 + c * 4, e.scene.background[c].to_bits());
        }
        word(out, d + 40, start as u32);
        word(out, d + 44, size as u32);
        word(out, d + 48, (start + size) as u32);
        word(out, d + 52, videos.len() as u32);
        long(out, d + 56, e.scene.frame.to_bits());
        word(out, d + 64, e.scene.fps);
        for (j, (l, s)) in videos.iter().enumerate() {
            let v = start + size + j * VIDEO;
            long(out, v, l.id);
            long(out, v + 8, s.asset);
            long(out, v + 16, s.source_time_us);
            let a = project
                .video_assets
                .iter()
                .find(|a| a.id == s.asset)
                .ok_or("video asset missing")?;
            word(out, v + 24, (-(l.order as i32 + 1)) as u32);
            word(out, v + 28, a.display_width);
            word(out, v + 32, a.display_height);
        }
    }
    word(out, 0, MAGIC);
    word(out, 4, VERSION);
    word(out, 8, nodes.len() as u32);
    let len = out.len();
    word(out, 12, len as u32);
    word(out, 16, root as u32);
    word(out, 20, HEADER as u32);
    word(out, 24, NODE as u32);
    word(out, 28, VIDEO as u32);
    Ok(())
}
