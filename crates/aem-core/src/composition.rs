//! Shared-resource composition graph and atomic composition edits.
use crate::{Camera, Content, Error, Layer, LayerTimeline, Project, PropertyExpression, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const MAIN_COMPOSITION: &str = "comp-main";
pub const MAX_COMPOSITIONS: usize = 256;
/// Number of nodes on a path, including the root.
pub const MAX_COMPOSITION_DEPTH: usize = 16;
/// All source-reference paths, including disabled and off-range layers.
pub const MAX_COMPOSITION_INSTANCES: usize = 1024;
/// Sampled nodes in one frame, including the root. GPU byte budgets are separate.
pub const MAX_RENDER_COMPOSITION_INSTANCES: usize = 64;
pub fn main_composition() -> String {
    MAIN_COMPOSITION.into()
}
fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub background: [f32; 4],
    pub camera: Camera,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub expressions: Vec<PropertyExpression>,
}

/// Borrow a node from an already validated document without copying its graph.
#[derive(Clone, Copy)]
pub struct CompositionView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub background: [f32; 4],
    pub camera: &'a Camera,
    pub layers: &'a [Layer],
    pub expressions: &'a [PropertyExpression],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionClip {
    pub composition: String,
    #[serde(default)]
    pub source_start_frame: i32,
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
}
#[derive(Clone, Debug)]
pub struct AudioVoice {
    pub asset: u64,
    pub begin_sample: u64,
    pub end_sample: u64,
    pub offset_sample: i64,
    pub source_offset_us: u64,
    pub volume: f32,
}
impl CompositionClip {
    pub fn new(composition: String) -> Self {
        Self {
            composition,
            source_start_frame: 0,
            volume: 1.,
            muted: false,
        }
    }
    pub fn source_frame(&self, local_frame: f64, parent_fps: u32, child_fps: u32) -> f64 {
        f64::from(self.source_start_frame)
            + local_frame * f64::from(child_fps) / f64::from(parent_fps)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSettings {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    #[serde(default = "seconds")]
    pub timing: String,
    #[serde(default = "reject")]
    pub shorten: String,
}
fn seconds() -> String {
    "preserve_seconds".into()
}
fn reject() -> String {
    "reject".into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionAction {
    Create {
        settings: CompositionSettings,
    },
    Reference {
        target: String,
        #[serde(default)]
        at_frame: u32,
    },
    Precompose {
        objects: Vec<u64>,
        name: String,
        #[serde(default = "full_range")]
        range: String,
    },
    Settings {
        settings: CompositionSettings,
    },
    Delete {
        target: String,
    },
    SetClip {
        object: u64,
        source_start_frame: i32,
        volume: f32,
        muted: bool,
    },
}
fn full_range() -> String {
    "composition".into()
}

#[derive(Clone, Debug, Serialize)]
pub struct CompositionError {
    pub code: String,
    pub composition: String,
    pub message: String,
    pub details: Value,
}
impl std::fmt::Display for CompositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "composition_error:{}",
            serde_json::to_string(self).map_err(|_| std::fmt::Error)?
        )
    }
}
impl std::error::Error for CompositionError {}
pub fn fail<T>(composition: &str, code: &str, message: &str, details: Value) -> Result<T> {
    Err(Error::Composition(CompositionError {
        code: code.into(),
        composition: composition.into(),
        message: message.into(),
        details,
    }))
}
impl Composition {
    fn from_project(p: &Project) -> Self {
        Self {
            id: p.composition_id.clone(),
            name: p.name.clone(),
            width: p.width,
            height: p.height,
            fps: p.fps,
            frames: p.frames,
            background: p.background,
            camera: p.camera.clone(),
            layers: p.layers.clone(),
            expressions: p.expressions.clone(),
        }
    }
    fn write_to(self, p: &mut Project) {
        p.composition_id = self.id;
        p.name = self.name;
        p.width = self.width;
        p.height = self.height;
        p.fps = self.fps;
        p.frames = self.frames;
        p.background = self.background;
        p.camera = self.camera;
        p.layers = self.layers;
        p.expressions = self.expressions;
    }
}
impl Project {
    pub fn composition_view(&self, id: &str) -> Result<CompositionView<'_>> {
        if id == self.composition_id {
            return Ok(CompositionView {
                id: &self.composition_id, name: &self.name, width: self.width, height: self.height,
                fps: self.fps, frames: self.frames, background: self.background, camera: &self.camera,
                layers: &self.layers, expressions: &self.expressions,
            });
        }
        let Some(c) = self.compositions.iter().find(|c| c.id == id) else {
            return fail(id, "composition_missing", "Composition does not exist", json!({}));
        };
        Ok(CompositionView {
            id: &c.id, name: &c.name, width: c.width, height: c.height, fps: c.fps, frames: c.frames,
            background: c.background, camera: &c.camera, layers: &c.layers, expressions: &c.expressions,
        })
    }

    /// Sampling view only. The caller keeps the validated document for graph lookups.
    /// Never save or validate this view as an independent document. Local
    /// validation must resolve references against the owning document.
    pub(crate) fn composition_frame_view(&self, id: &str) -> Result<Self> {
        let body = self.composition_view(id)?;
        Ok(Self {
            version: self.version,
            composition_id: body.id.into(),
            compositions: Vec::new(),
            name: body.name.into(),
            width: body.width,
            height: body.height,
            fps: body.fps,
            frames: body.frames,
            background: body.background,
            camera: body.camera.clone(),
            layers: body.layers.to_vec(),
            expressions: body.expressions.to_vec(),
            assets: self.assets.clone(),
            audio_assets: self.audio_assets.clone(),
            video_assets: self.video_assets.clone(),
            plugin_dependencies: Vec::new(),
        })
    }
    pub fn audio_voices(&self) -> Result<Vec<AudioVoice>> {
        // Preserve fractions until each absolute sample boundary is evaluated.
        // Dividing 48 kHz by fps first loses samples at e.g. 59 or 144 fps.
        #[derive(Clone, Copy)]
        struct Time {
            numerator: i128,
            denominator: i128,
        }
        impl Time {
            fn add_frames(self, frames: i64, fps: u32) -> Result<Self> {
                let overflow = || Error::Invalid("Audio timebase exceeds rational arithmetic budget".into());
                crate::ensure(fps > 0, "invalid audio frame rate")?;
                let mut a = self.denominator;
                let mut b = i128::from(fps);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                let multiplier = i128::from(fps) / a;
                let numerator = self.numerator.checked_mul(multiplier)
                    .and_then(|left| i128::from(frames).checked_mul(48_000)
                        .and_then(|v| v.checked_mul(self.denominator / a))
                        .and_then(|right| left.checked_add(right))).ok_or_else(overflow)?;
                let denominator = self.denominator.checked_mul(multiplier).ok_or_else(overflow)?;
                // Keep every intermediate rational reduced, including zero.
                let (mut x, mut y) = (numerator.unsigned_abs(), denominator as u128);
                while y != 0 { (x, y) = (y, x % y); }
                let divisor = x as i128;
                Ok(Self { numerator: numerator / divisor, denominator: denominator / divisor })
            }
            fn sample(self) -> i64 {
                self.numerator.div_euclid(self.denominator) as i64
            }
        }
        fn visit(
            document: &Project,
            p: CompositionView<'_>,
            origin: Time,
            begin: i64,
            end: i64,
            volume: f32,
            depth: usize,
            out: &mut Vec<AudioVoice>,
        ) -> Result<()> {
            crate::ensure(
                depth < MAX_COMPOSITION_DEPTH,
                "audio composition nesting too deep",
            )?;
            for l in p.layers {
                let add = |time: Time, frames: i64, fps: u32| time.add_frames(frames, fps).map_err(|e|
                    Error::Composition(CompositionError { code:"audio_time_limit".into(), composition:p.id.into(),
                        message:e.to_string(), details:json!({"layer":l.id,"frames":frames,"fps":fps}) }));
                let clip = l.clip(p.frames);
                let first = begin
                    .max(add(origin,i64::from(clip.in_frame),p.fps)?.sample())
                    .max(0);
                let last = end.min(add(origin,i64::from(clip.out_frame),p.fps)?.sample());
                if first >= last {
                    continue;
                }
                let offset = add(origin,i64::from(clip.offset_frame),p.fps)?;
                if let Content::Composition { clip } = &l.content {
                    if clip.muted || clip.volume == 0. {
                        continue;
                    }
                    let child = document.composition_view(&clip.composition)?;
                    let child_origin =
                        add(offset,-i64::from(clip.source_start_frame),child.fps)?;
                    visit(
                        document, child,
                        child_origin,
                        first.max(child_origin.sample()),
                        last.min(
                            add(child_origin,i64::from(child.frames),child.fps)?.sample(),
                        ),
                        volume * clip.volume,
                        depth + 1,
                        out,
                    )?;
                } else if let Some(audio) = document.layer_audio(l) {
                    if audio.muted || audio.volume == 0. {
                        continue;
                    }
                    out.push(AudioVoice {
                        asset: audio.asset,
                        begin_sample: first as u64,
                        end_sample: last as u64,
                        offset_sample: offset.sample(),
                        source_offset_us: audio.source_offset_us,
                        volume: volume * audio.volume,
                    });
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        visit(
            self,
            self.composition_view(&self.composition_id)?,
            Time {
                numerator: 0,
                denominator: 1,
            },
            0,
            i64::from(self.frames) * 48_000 / i64::from(self.fps),
            1.,
            0,
            &mut out,
        )?;
        Ok(out)
    }
    /// Switch only the view fields; shared assets and the graph remain in this document.
    pub fn activate_composition(&mut self, id: &str) -> Result<()> {
        if id == self.composition_id {
            return Ok(());
        }
        let Some(index) = self.compositions.iter().position(|c| c.id == id) else {
            return fail(
                id,
                "composition_missing",
                "Composition does not exist",
                json!({}),
            );
        };
        let current = Composition::from_project(self);
        let target = std::mem::replace(&mut self.compositions[index], current);
        target.write_to(self);
        Ok(())
    }
    pub fn composition(&self, id: &str) -> Result<Self> {
        let mut p = self.clone();
        p.activate_composition(id)?;
        Ok(p)
    }
    pub fn composition_ids(&self) -> Vec<String> {
        std::iter::once(self.composition_id.clone())
            .chain(self.compositions.iter().map(|c| c.id.clone()))
            .collect()
    }
    pub fn reachable_compositions(&self) -> Result<Vec<String>> {
        fn visit(p: &Project, id: &str, ids: &mut Vec<String>) -> Result<()> {
            if ids.iter().any(|v| v == id) {
                return Ok(());
            }
            ids.push(id.into());
            for l in p.composition_view(id)?.layers {
                if let Content::Composition { clip } = &l.content {
                    visit(p, &clip.composition, ids)?;
                }
            }
            Ok(())
        }
        let mut ids = Vec::new();
        visit(self, &self.composition_id, &mut ids)?;
        Ok(ids)
    }
    pub fn composition_references(&self, target: &str) -> Vec<Value> {
        self.composition_ids()
            .into_iter()
            .flat_map(|id| {
                let p = self.composition_view(&id).expect("known composition");
                p.layers.iter().filter_map(move |l| match &l.content {
                    Content::Composition { clip } if clip.composition == target => {
                        Some(json!({"composition":id,"object":l.id}))
                    }
                    _ => None,
                })
            })
            .collect()
    }
    pub fn composition_list(&self) -> Vec<Value> {
        let ids = self.composition_ids();
        let mut references: HashMap<&str, Vec<Value>> = HashMap::new();
        for id in &ids {
            for layer in self.composition_view(id).expect("known composition").layers {
                if let Content::Composition { clip } = &layer.content {
                    references.entry(&clip.composition).or_default()
                        .push(json!({"composition":id,"object":layer.id}));
                }
            }
        }
        ids.iter().map(|id| {
            let p = self.composition_view(id).expect("known composition");
            json!({"id":id,"name":p.name,"width":p.width,"height":p.height,"fps":p.fps,"frames":p.frames,
                "main":id==MAIN_COMPOSITION,"references":references.get(id.as_str()).cloned().unwrap_or_default(),
                "children":p.layers.iter().filter_map(|l| match &l.content {Content::Composition{clip}=>Some(json!({"object":l.id,"composition":clip.composition})),_=>None}).collect::<Vec<_>>(),
                "next_layer_id":p.layers.iter().map(|l|l.id).max().unwrap_or(0).checked_add(1)})
        }).collect()
    }
    pub(crate) fn validate_compositions(&self) -> Result<()> {
        if self.compositions.is_empty()
            && self.composition_id == MAIN_COMPOSITION
            && !self
                .layers
                .iter()
                .any(|l| matches!(l.content, Content::Composition { .. }))
        {
            return Ok(());
        }
        if self.version < 7 {
            return fail(
                &self.composition_id,
                "unsupported_format",
                "Nested compositions require project format seven",
                json!({}),
            );
        }
        let ids = self.composition_ids();
        let mut unique = HashSet::new();
        if ids.len() > MAX_COMPOSITIONS
            || !ids.iter().all(|id| {
                id.len() <= 64
                    && id.starts_with("comp-")
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && unique.insert(id)
            })
            || !ids.iter().any(|id| id == MAIN_COMPOSITION)
        {
            return fail(
                &self.composition_id,
                "resource_limit",
                "Invalid composition IDs or composition limit exceeded",
                json!({"max_compositions":MAX_COMPOSITIONS}),
            );
        }
        for id in &ids {
            if id != &self.composition_id {
                self.composition_frame_view(id)?.validate_one(self)?;
            }
        }
        self.check_composition_graph(None)?;
        Ok(())
    }
    fn check_composition_graph(&self, extra: Option<(&str, &str)>) -> Result<()> {
        fn visit<'a>(nodes: &HashMap<&'a str, CompositionView<'a>>, id: &'a str,
                     path: &mut Vec<&'a str>, memo: &mut HashMap<&'a str, (usize, usize)>, extra: Option<(&'a str, &'a str)>) -> Result<(usize, usize)> {
            if path.contains(&id) {
                return fail(
                    id,
                    "cycle",
                    "Composition cycle detected",
                    json!({"path":path}),
                );
            }
            if path.len() >= MAX_COMPOSITION_DEPTH {
                return fail(
                    id,
                    "resource_limit",
                    "Composition nesting too deep",
                    json!({"max_depth":MAX_COMPOSITION_DEPTH}),
                );
            }
            if let Some(&(depth, instances)) = memo.get(id) {
                if path.len() + depth > MAX_COMPOSITION_DEPTH {
                    return fail(id, "resource_limit", "Composition nesting too deep",
                                json!({"max_depth":MAX_COMPOSITION_DEPTH,"depth_includes_root":true}));
                }
                return Ok((depth, instances));
            }
            let node = nodes.get(id).ok_or_else(|| Error::Composition(CompositionError {
                code: "composition_missing".into(), composition: id.into(),
                message: "Composition does not exist".into(), details: json!({}),
            }))?;
            let (mut depth, mut instances) = (1, 1);
            path.push(id);
            for l in node.layers {
                if let Content::Composition { clip } = &l.content {
                    let (child_depth, child_instances) = visit(nodes, &clip.composition, path, memo, extra)?;
                    depth = depth.max(child_depth + 1);
                    instances += child_instances;
                    if instances > MAX_COMPOSITION_INSTANCES {
                        return fail(id, "resource_limit", "Composition source expansion limit exceeded",
                                    json!({"max_instances":MAX_COMPOSITION_INSTANCES,"requested_instances":instances}));
                    }
                }
            }
            if let Some((source, target)) = extra.filter(|(source, _)| *source == id) {
                let _ = source;
                let (child_depth, child_instances) = visit(nodes, target, path, memo, extra)?;
                depth = depth.max(child_depth + 1);
                instances += child_instances;
                if instances > MAX_COMPOSITION_INSTANCES {
                    return fail(id, "resource_limit", "Composition source expansion limit exceeded",
                                json!({"max_instances":MAX_COMPOSITION_INSTANCES,"requested_instances":instances}));
                }
            }
            path.pop();
            memo.insert(id, (depth, instances));
            Ok((depth, instances))
        }
        let ids = self.composition_ids();
        let nodes: HashMap<_, _> = ids.iter().map(|id| {
            let node = self.composition_view(id).expect("known composition");
            (node.id, node)
        }).collect();
        let mut memo = HashMap::new();
        for id in &ids { visit(&nodes, id, &mut Vec::new(), &mut memo, extra)?; }
        Ok(())
    }
    /// Structural candidates for a validated document. Does not instantiate an
    /// editor, clone media/animations or serialize an undo history per target.
    pub fn composition_reference_candidates(&self, source: &str) -> Result<Vec<String>> {
        let parent = self.composition_view(source)?;
        if parent.layers.len() >= crate::MAX_LAYERS
            || parent.layers.iter().map(|l| l.id).max() == Some(u64::MAX) {
            return Ok(vec![]);
        }
        Ok(self.composition_ids().into_iter().filter(|target|
            self.check_composition_graph(Some((source, target))).is_ok()).collect())
    }
    fn new_composition_id(&self) -> Result<String> {
        let ids = self.composition_ids();
        (1..=u32::MAX)
            .map(|n| format!("comp-{n}"))
            .find(|id| !ids.contains(id))
            .ok_or_else(|| Error::Invalid("composition ID space exhausted".into()))
    }
    fn add_reference(&mut self, target: &str, at_frame: u32) -> Result<u64> {
        let child = self.composition(target)?;
        if at_frame >= self.frames {
            return fail(
                &self.composition_id,
                "invalid_range",
                "Reference starts outside composition",
                json!({"at_frame":at_frame}),
            );
        }
        let id = self
            .layers
            .iter()
            .map(|l| l.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("layer ID space exhausted".into()))?;
        let mut l = Layer::solid(
            id,
            &child.name,
            [child.width as f32, child.height as f32],
            [self.width as f32 / 2., self.height as f32 / 2., 0.],
            [1.; 4],
        );
        l.content = Content::Composition {
            clip: CompositionClip::new(target.into()),
        };
        l.timeline = Some(LayerTimeline {
            in_frame: at_frame,
            out_frame: self.frames,
            offset_frame: at_frame as i32,
        });
        self.layers.push(l);
        Ok(id)
    }
    pub(crate) fn edit_composition(&mut self, action: CompositionAction) -> Result<Value> {
        match action {
            CompositionAction::Create { settings } => {
                if self.compositions.len() + 1 >= MAX_COMPOSITIONS {
                    return fail(
                        &self.composition_id,
                        "resource_limit",
                        "Composition limit exceeded",
                        json!({}),
                    );
                }
                let id = self.new_composition_id()?;
                let mut child = Project::new(
                    settings.width,
                    settings.height,
                    settings.fps,
                    settings.frames,
                )?;
                child.name = settings.name;
                child.composition_id = id.clone();
                child.background = [0.; 4];
                self.compositions.push(Composition::from_project(&child));
                Ok(json!({"kind":"create","composition":id}))
            }
            CompositionAction::Reference { target, at_frame } => {
                let object = self.add_reference(&target, at_frame)?;
                Ok(
                    json!({"kind":"reference","composition":self.composition_id,"object":object,"selection":[object]}),
                )
            }
            CompositionAction::Precompose {
                objects,
                name,
                range,
            } => self.precompose(objects, name, range),
            CompositionAction::Settings { settings } => self.apply_composition_settings(settings),
            CompositionAction::Delete { target } => {
                if target == MAIN_COMPOSITION || target == self.composition_id {
                    return fail(
                        &target,
                        "composition_in_use",
                        "Cannot delete the main or active composition",
                        json!({}),
                    );
                }
                self.composition(&target)?;
                let references = self.composition_references(&target);
                if !references.is_empty() {
                    return fail(
                        &target,
                        "composition_in_use",
                        "Composition is referenced",
                        json!({"references":references}),
                    );
                }
                self.compositions.retain(|c| c.id != target);
                Ok(json!({"kind":"delete","composition":target}))
            }
            CompositionAction::SetClip {
                object,
                source_start_frame,
                volume,
                muted,
            } => {
                let source = self.composition_id.clone();
                if !volume.is_finite()
                    || !(0.0..=4.).contains(&volume)
                    || source_start_frame.unsigned_abs() > crate::MAX_FRAMES
                {
                    return fail(
                        &source,
                        "invalid_range",
                        "Invalid composition source offset or volume",
                        json!({"object":object}),
                    );
                }
                let l = self.layer_mut(object)?;
                if l.locked {
                    return fail(
                        &source,
                        "locked_layer",
                        "Reference layer is locked",
                        json!({"object":object}),
                    );
                }
                let Content::Composition { clip } = &mut l.content else {
                    return fail(
                        &source,
                        "invalid_reference",
                        "Layer is not a composition reference",
                        json!({"object":object}),
                    );
                };
                clip.source_start_frame = source_start_frame;
                clip.volume = volume;
                clip.muted = muted;
                Ok(json!({"kind":"set_clip","object":object}))
            }
        }
    }
    fn precompose(&mut self, objects: Vec<u64>, name: String, range: String) -> Result<Value> {
        let source = self.composition_id.clone();
        if range != "composition" {
            return fail(
                &source,
                "unsupported_mode",
                "Only the full composition range is supported",
                json!({"range":range}),
            );
        }
        let selected: HashSet<_> = objects.iter().copied().collect();
        if selected.is_empty() || selected.len() != objects.len() || selected.contains(&0) {
            return fail(
                &source,
                "invalid_selection",
                "Select distinct non-camera layers",
                json!({}),
            );
        }
        let indices: Vec<_> = self
            .layers
            .iter()
            .enumerate()
            .filter_map(|(i, l)| selected.contains(&l.id).then_some(i))
            .collect();
        if indices.len() != objects.len() {
            return fail(
                &source,
                "cross_composition_selection",
                "Every selected layer must belong to the source composition",
                json!({"objects":objects}),
            );
        }
        if indices.last().unwrap() - indices[0] + 1 != indices.len() {
            return fail(
                &source,
                "noncontiguous_selection",
                "Precomposition requires contiguous stack positions",
                json!({"objects":objects}),
            );
        }
        for l in &self.layers {
            if selected.contains(&l.id) && l.locked {
                return fail(
                    &source,
                    "locked_layer",
                    "Selected layer is locked",
                    json!({"object":l.id}),
                );
            }
            if selected.contains(&l.id) && l.three_d {
                return fail(
                    &source,
                    "unsupported_mode",
                    "Precomposition currently supports flat layers only",
                    json!({"object":l.id}),
                );
            }
            if selected.contains(&l.id) && l.effects.iter().any(|e| e.scene.is_some()) {
                return fail(
                    &source,
                    "unsupported_mode",
                    "Scene generators can depend on layers outside the selection",
                    json!({"object":l.id}),
                );
            }
            if !selected.contains(&l.id)
                && l.effects.iter().any(|e| {
                    e.scene
                        .as_ref()
                        .and_then(|s| s.source_layer)
                        .is_some_and(|id| selected.contains(&id))
                })
            {
                return fail(
                    &source,
                    "external_reference",
                    "An unselected effect references a selected layer",
                    json!({"object":l.id}),
                );
            }
            if l.parent
                .as_ref()
                .and_then(|p| p.object)
                .is_some_and(|id| selected.contains(&id) != selected.contains(&l.id))
            {
                return fail(
                    &source,
                    "external_parent",
                    "Parent relationship crosses the selection",
                    json!({"object":l.id}),
                );
            }
        }
        if self
            .camera
            .parent
            .as_ref()
            .and_then(|p| p.object)
            .is_some_and(|id| selected.contains(&id))
        {
            return fail(
                &source,
                "external_parent",
                "Camera parent belongs to the selection",
                json!({}),
            );
        }
        for e in &self.expressions {
            if e.enabled && e.source.contains("index") {
                return fail(
                    &source,
                    "expression_context",
                    "Expression depends on a composition context changed by precomposition",
                    json!({"target":e.target}),
                );
            }
        }
        let child_id = self.new_composition_id()?;
        let insert = indices[0];
        let mut child = Composition::from_project(self);
        child.id = child_id.clone();
        child.name = name;
        child.background = [0.; 4];
        child.camera = Camera::new(self.width, self.height);
        child.camera.created = false;
        child.layers.retain(|l| selected.contains(&l.id));
        child
            .expressions
            .retain(|e| selected.contains(&e.target.object()));
        self.compositions.push(child);
        let reference = self.add_reference(&child_id, 0)?;
        let l = self.layers.pop().expect("reference just added");
        self.layers.retain(|l| !selected.contains(&l.id));
        self.layers.insert(insert, l);
        self.expressions
            .retain(|e| !selected.contains(&e.target.object()));
        Ok(
            json!({"kind":"precompose","source":source,"composition":child_id,"object":reference,"selection":[reference],
            "layer_mapping":objects.iter().map(|id|json!({"source_composition":source,"source_object":id,"composition":child_id,"object":id})).collect::<Vec<_>>()}),
        )
    }
    fn apply_composition_settings(&mut self, s: CompositionSettings) -> Result<Value> {
        let id = self.composition_id.clone();
        if !(1..=8192).contains(&s.width)
            || !(1..=8192).contains(&s.height)
            || !(1..=crate::MAX_COMPOSITION_FPS).contains(&s.fps)
            || !(1..=crate::MAX_FRAMES).contains(&s.frames)
            || s.name.len() > 1024
        {
            return fail(
                &id,
                "invalid_settings",
                "Invalid name, dimensions, frame rate or duration",
                json!({"width":s.width,"height":s.height,"fps":s.fps,"frames":s.frames}),
            );
        }
        if !matches!(s.timing.as_str(), "preserve_seconds" | "preserve_frames")
            || !matches!(s.shorten.as_str(), "reject" | "trim")
        {
            return fail(
                &id,
                "unsupported_mode",
                "Unsupported settings timing or shortening policy",
                json!({}),
            );
        }
        let old_fps = self.fps;
        let old_frames = self.frames;
        let old_size = [self.width, self.height];
        let mut body = serde_json::to_value(Composition::from_project(self))?;
        let mut impacts = Vec::new();
        fn retime(
            v: &mut Value,
            old: u32,
            new: u32,
            path: &str,
            impacts: &mut Vec<Value>,
        ) -> Result<()> {
            match v {
                Value::Object(o) => {
                    if let Some(keys) = o.get_mut("keys").and_then(Value::as_array_mut) {
                        let mut used = HashSet::new();
                        for k in keys {
                            if let Some(f) = k.get_mut("frame") {
                                let before = f
                                    .as_i64()
                                    .ok_or_else(|| Error::Invalid("invalid key time".into()))?;
                                let after = (before as f64 * f64::from(new) / f64::from(old))
                                    .round() as i64;
                                if !used.insert(after) {
                                    return Err(Error::Invalid("frame rate change merges keyframes; choose preserve_frames".into()));
                                }
                                if before != after {
                                    impacts.push(json!({"kind":"keyframe","path":path,"before":before,"after":after}));
                                }
                                *f = json!(after);
                            }
                        }
                    }
                    for (k, v) in o.iter_mut() {
                        if k == "keys" {
                            continue;
                        }
                        if matches!(k.as_str(), "in_frame" | "out_frame" | "offset_frame") {
                            if let Some(f) = v.as_i64() {
                                *v = json!(
                                    (f as f64 * f64::from(new) / f64::from(old)).round() as i64
                                );
                            }
                        } else {
                            retime(v, old, new, &format!("{path}/{k}"), impacts)?;
                        }
                    }
                }
                Value::Array(a) => {
                    for (i, v) in a.iter_mut().enumerate() {
                        retime(v, old, new, &format!("{path}/{i}"), impacts)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        if s.timing == "preserve_seconds" && old_fps != s.fps {
            if let Err(e) = retime(&mut body, old_fps, s.fps, "", &mut impacts) {
                return fail(
                    &id,
                    "keyframe_collision",
                    &e.to_string(),
                    json!({"impacts":impacts}),
                );
            }
        }
        let mut node: Composition = serde_json::from_value(body)?;
        node.name = s.name;
        node.width = s.width;
        node.height = s.height;
        node.fps = s.fps;
        node.frames = s.frames;
        let original: HashSet<_> = node.layers.iter().map(|l| l.id).collect();
        fn outside_keys(v: &Value, end: i64, path: &str, kind: &str, impacts: &mut Vec<Value>) {
            match v {
                Value::Object(o) => {
                    if let Some(a) = o.get("keys").and_then(Value::as_array) {
                        for k in a {
                            if k["frame"].as_i64().is_some_and(|f| f >= end) {
                                impacts.push(json!({"kind":kind,"path":path,"key":k}));
                            }
                        }
                    }
                    for (k, v) in o {
                        if k != "keys" {
                            outside_keys(v, end, &format!("{path}/{k}"), kind, impacts);
                        }
                    }
                }
                Value::Array(a) => {
                    for (i, v) in a.iter().enumerate() {
                        outside_keys(v, end, &format!("{path}/{i}"), kind, impacts);
                    }
                }
                _ => {}
            }
        }
        outside_keys(
            &serde_json::to_value(&node.camera)?,
            i64::from(s.frames),
            "/camera",
            "removed_camera_keyframe",
            &mut impacts,
        );
        for l in &node.layers {
            outside_keys(
                &serde_json::to_value(l)?,
                i64::from(s.frames) - i64::from(l.clip(s.frames).offset_frame),
                &format!("/layers/{}", l.id),
                "retained_outside_keyframe",
                &mut impacts,
            );
        }
        for l in &mut node.layers {
            let old = l.clip(if s.timing == "preserve_seconds" {
                (f64::from(old_frames) * f64::from(s.fps) / f64::from(old_fps)).round() as u32
            } else {
                old_frames
            });
            if old.out_frame > s.frames {
                impacts.push(json!({"kind":"clip","object":l.id,"before":old,"new_end":s.frames}));
                if old.in_frame < s.frames {
                    l.timeline = Some(LayerTimeline {
                        out_frame: s.frames,
                        ..old
                    });
                }
            } else if l.timeline.is_none() {
                l.timeline = Some(old);
            }
        }
        if s.shorten == "reject"
            && impacts
                .iter()
                .any(|v| v["kind"] == "clip" || v["kind"] == "removed_camera_keyframe")
        {
            return fail(
                &id,
                "invalid_range",
                "New duration clips layers; choose trim explicitly",
                json!({"impacts":impacts}),
            );
        }
        if s.shorten == "trim" {
            node.layers
                .retain(|l| l.timeline.map_or(0, |t| t.in_frame) < s.frames);
            let kept: HashSet<_> = node.layers.iter().map(|l| l.id).collect();
            if node.layers.iter().any(|l| {
                l.parent
                    .as_ref()
                    .and_then(|p| p.object)
                    .is_some_and(|p| !kept.contains(&p))
            }) || node
                .camera
                .parent
                .as_ref()
                .and_then(|p| p.object)
                .is_some_and(|p| !kept.contains(&p))
            {
                return fail(
                    &id,
                    "external_parent",
                    "Trimming would remove a required parent",
                    json!({}),
                );
            }
            node.expressions
                .retain(|e| e.target.object() == 0 || kept.contains(&e.target.object()));
            for removed in original.difference(&kept) {
                impacts.push(json!({"kind":"removed_layer","object":removed}));
            }
            let mut camera = serde_json::to_value(&node.camera)?;
            fn trim(v: &mut Value, end: u32) {
                match v {
                    Value::Object(o) => {
                        if let Some(a) = o.get_mut("keys").and_then(Value::as_array_mut) {
                            a.retain(|k| {
                                k["frame"]
                                    .as_i64()
                                    .is_some_and(|f| f >= 0 && f < i64::from(end))
                            });
                        }
                        for v in o.values_mut() {
                            trim(v, end);
                        }
                    }
                    Value::Array(a) => {
                        for v in a {
                            trim(v, end);
                        }
                    }
                    _ => {}
                }
            }
            trim(&mut camera, s.frames);
            node.camera = serde_json::from_value(camera)?;
        }
        node.write_to(self);
        // Source frame offsets belong to the target's clock. Preserve seconds on inbound references.
        for source in self.composition_ids() {
            let mut p = self.composition(&source)?;
            let mut changed = false;
            for l in &mut p.layers {
                if let Content::Composition { clip } = &mut l.content {
                    if clip.composition == id {
                        impacts.push(json!({"kind":"reference","composition":source,"object":l.id,"old_size":old_size,"new_size":[s.width,s.height]}));
                        l.size = [s.width as f32, s.height as f32];
                        if old_fps != s.fps && s.timing == "preserve_seconds" {
                            clip.source_start_frame = (f64::from(clip.source_start_frame)
                                * f64::from(s.fps)
                                / f64::from(old_fps))
                            .round() as i32;
                        }
                        changed = true;
                    }
                }
            }
            if changed {
                let node = Composition::from_project(&p);
                if source == self.composition_id {
                    node.write_to(self);
                } else {
                    *self
                        .compositions
                        .iter_mut()
                        .find(|c| c.id == source)
                        .unwrap() = node;
                }
            }
        }
        Ok(
            json!({"kind":"settings","composition":id,"impacts":impacts,"timing":s.timing,"shorten":s.shorten}),
        )
    }
}
