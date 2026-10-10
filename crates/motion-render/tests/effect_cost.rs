//! CPU-only planning snapshots; the metric is not GPU time or sample count.
use motion_core::{EffectInstance, Layer, Project, Scene};
use motion_effects::{
    AlphaMode, BoundsExpr, EffectDefinition, EffectPackage, OutputBounds, Registry, WorkingSpace,
};
use motion_render::effect_plan::{scratch_capacity_bytes, PlanBuilder, ScratchRejection};
use serde::Serialize;
use std::{collections::BTreeMap, fmt::Write, path::Path, sync::Arc};

const WIDTH: u32 = 3840;
const HEIGHT: u32 = 2160;
const LOOP_WARNING: u64 = 2048;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PassCost {
    index: usize,
    shader: String,
    entry: String,
    loop_work: u64,
}
#[derive(Debug, Serialize)]
struct Parameter {
    id: String,
    name: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Warning {
    code: &'static str,
    pass: Option<usize>,
    detail: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectCost {
    id: String,
    name: String,
    compatibility_profile: String,
    passes: usize,
    planned_passes: Option<usize>,
    pass_costs: Vec<PassCost>,
    loop_work_max: u64,
    loop_work_sum: u64,
    scratch_4k_bytes: Option<u64>,
    scratch_4k_sizes: Option<[[u32; 2]; 8]>,
    scratch_4k_required_bytes: Option<u64>,
    scratch_4k_required_sizes: Option<[[u32; 2]; 8]>,
    scratch_budget_bytes: u64,
    status: &'static str,
    diagnostics: Vec<String>,
    working_space: WorkingSpace,
    alpha_mode: AlphaMode,
    padding: BoundsExpr,
    padding_value: f32,
    output_bounds: Option<OutputBounds>,
    output_bounds_value: Option<[f32; 4]>,
    parameter_values: BTreeMap<String, [f32; 4]>,
    params_unimplemented: Vec<Parameter>,
    warnings: Vec<Warning>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    metric_kind: &'static str,
    scope: &'static str,
    dimensions: [u32; 2],
    frame: u32,
    parameter_scenario: &'static str,
    package_id: String,
    package_version: String,
    package_hash: String,
    source_revision: Option<String>,
    loop_warning_threshold: u64,
    scratch_warning_bytes: u64,
    strict: bool,
    effects: Vec<EffectCost>,
}

fn measure(
    package: &Arc<EffectPackage>,
    definition: &EffectDefinition,
) -> Result<EffectCost, Box<dyn std::error::Error>> {
    let mut project = Project::new(WIDTH, HEIGHT, 30, 60)?;
    let size = [WIDTH as f32, HEIGHT as f32];
    let mut layer = Layer::solid(
        1,
        "cost fixture",
        size,
        [size[0] / 2., size[1] / 2., 0.],
        [1.; 4],
    );
    layer.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        size,
    ));
    project.layers.push(layer);
    project.rebuild_plugin_dependencies();
    project.validate()?;
    let mut scene = Scene::new(&project);
    scene.sample(&project, 0., None)?;
    let effect = &scene.effects[0];
    let parameter_values: BTreeMap<_, _> = effect
        .param_ids
        .iter()
        .cloned()
        .zip(effect.values.iter().copied())
        .collect();
    let lookup = |id: &str, component: usize| parameter_values.get(id)?.get(component).copied();
    let input = scene.layers[0].source_rect;
    let padding_value = definition.padding.evaluate_with(lookup, input)?;
    let output_bounds_value = definition
        .output_bounds
        .as_ref()
        .map(|b| b.evaluate_with(lookup, input))
        .transpose()?;
    let mut registry = Registry::default();
    registry.insert(package.clone())?;
    let mut builder = PlanBuilder::new(registry)?;
    let scratch_budget_bytes = builder.scratch_budget();
    // Own the result before reading builder metadata; no device or GPU allocations.
    let outcome = builder
        .build(&scene, &[0], WIDTH, HEIGHT, true)
        .map(|p| (p.passes.len(), p.scratch_sizes, p.diagnostics.clone()));
    let request = builder.last_scratch_request;
    let (status, planned_passes, scratch_4k_sizes, diagnostics) = match outcome {
        Ok((passes, sizes, diagnostics)) => ("planned", Some(passes), Some(sizes), diagnostics),
        Err(error) => (
            if request.is_some_and(|r| r.rejection == Some(ScratchRejection::CapacityBudget)) {
                "budgetRejected"
            } else {
                "planningError"
            },
            None,
            None,
            vec![error],
        ),
    };
    let mut pass_costs = Vec::new();
    for (index, pass) in definition.passes.iter().enumerate() {
        let key = format!("{}:{}:{index}", package.hash, definition.id);
        let program = builder
            .programs
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| format!("cost report missing effect shader {key}"))?;
        pass_costs.push(PassCost {
            index,
            shader: pass.shader.clone(),
            entry: pass.entry.clone(),
            loop_work: program.shader.loop_work,
        });
    }
    let params_unimplemented: Vec<_> = definition
        .params
        .iter()
        .filter(|p| !p.implemented)
        .map(|p| Parameter {
            id: p.id.clone(),
            name: p.name.clone(),
        })
        .collect();
    let scratch_4k_bytes = scratch_4k_sizes.as_ref().map(scratch_capacity_bytes);
    let scratch_4k_required_sizes = request.map(|r| r.sizes);
    let scratch_4k_required_bytes = scratch_4k_required_sizes
        .as_ref()
        .map(scratch_capacity_bytes);
    let mut warnings = Vec::new();
    for pass in &pass_costs {
        if pass.loop_work > LOOP_WARNING {
            warnings.push(Warning {
                code: "loopWork",
                pass: Some(pass.index),
                detail: format!("static loop work {} exceeds {LOOP_WARNING}", pass.loop_work),
            });
        }
    }
    if scratch_4k_required_bytes
        .or(scratch_4k_bytes)
        .is_some_and(|n| n > scratch_budget_bytes)
    {
        warnings.push(Warning {
            code: "scratchBudget",
            pass: None,
            detail: format!(
                "4K scratch demand {} bytes exceeds {} bytes",
                scratch_4k_required_bytes.or(scratch_4k_bytes).unwrap(),
                scratch_budget_bytes
            ),
        });
    }
    if !params_unimplemented.is_empty() {
        warnings.push(Warning {
            code: "paramsUnimplemented",
            pass: None,
            detail: format!(
                "unimplemented parameters: {}",
                params_unimplemented
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if status == "planningError" {
        warnings.push(Warning {
            code: "planningError",
            pass: None,
            detail: diagnostics.join("; "),
        });
    }
    Ok(EffectCost {
        id: definition.id.clone(),
        name: definition.name.clone(),
        compatibility_profile: definition.compatibility_profile.clone(),
        passes: definition.passes.len(),
        planned_passes,
        loop_work_max: pass_costs.iter().map(|p| p.loop_work).max().unwrap_or(1),
        loop_work_sum: pass_costs.iter().map(|p| p.loop_work).sum(),
        pass_costs,
        scratch_4k_bytes,
        scratch_4k_sizes,
        scratch_4k_required_bytes,
        scratch_4k_required_sizes,
        scratch_budget_bytes,
        status,
        diagnostics,
        working_space: definition.working_space,
        alpha_mode: definition.alpha_mode,
        padding: definition.padding.clone(),
        padding_value,
        output_bounds: definition.output_bounds.clone(),
        output_bounds_value,
        parameter_values,
        params_unimplemented,
        warnings,
    })
}

fn markdown(report: &Report) -> String {
    let mut text = format!("# Effect costs · {} {}\n\nScope: {}; {} effects; 3840×2160; frame 0; default parameters; conservative default scratch policy ({} MiB), not a phone memory profile.\n\nStatic loop work is the portable validation product, not GPU time or texture samples. Scratch is exact planner capacity, not driver VRAM. Rejected demands do not allocate textures.\n\n", report.package_id, report.package_version, report.scope, report.effects.len(), report.scratch_warning_bytes / 1048576);
    if let Some(revision) = &report.source_revision {
        writeln!(text, "Revision: `{revision}`\n").unwrap();
    }
    text.push_str("| Effect | Loop max / sum | Declared / planned passes | 4K scratch MiB | Required MiB | Status | Warnings |\n|---|---:|---:|---:|---:|---|---|\n");
    let mut rows: Vec<_> = report.effects.iter().collect();
    rows.sort_by(|a, b| {
        b.loop_work_max
            .cmp(&a.loop_work_max)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mib = |bytes: Option<u64>| {
        bytes.map_or_else(|| "—".into(), |n| format!("{:.2}", n as f64 / 1048576.))
    };
    for row in rows {
        writeln!(
            text,
            "| {} | {} / {} | {} / {} | {} | {} | {} | {} |",
            row.id,
            row.loop_work_max,
            row.loop_work_sum,
            row.passes,
            row.planned_passes
                .map_or_else(|| "—".into(), |n| n.to_string()),
            mib(row.scratch_4k_bytes),
            mib(row.scratch_4k_required_bytes),
            row.status,
            row.warnings
                .iter()
                .map(|w| w.code)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    text.push_str("\n## Diagnostics\n\n");
    for row in &report.effects {
        for message in &row.diagnostics {
            writeln!(text, "- {}: {}", row.id, message.replace('\n', " ")).unwrap();
        }
        for param in &row.params_unimplemented {
            writeln!(
                text,
                "- {}: unimplemented `{}` ({})",
                row.id,
                param.id,
                param.name.replace('\n', " ")
            )
            .unwrap();
        }
    }
    text
}

fn write_report(directory: &Path, report: &Report, table: &str) -> TestResult {
    std::fs::create_dir_all(directory)?;
    std::fs::write(
        directory.join("effect-costs.json"),
        serde_json::to_string_pretty(report)? + "\n",
    )?;
    std::fs::write(directory.join("effect-costs.md"), table)?;
    Ok(())
}

fn enforce(strict: bool, warning_count: usize) -> TestResult {
    if strict && warning_count > 0 {
        return Err(format!("MOTION_EFFECT_STRICT=1: {warning_count} effect cost warnings; inspect the report before enabling CI strict mode").into());
    }
    Ok(())
}

#[test]
fn current_builtin_manifest_costs() -> TestResult {
    let package = motion_effects::builtin::package()?;
    let manifest = motion_effects::builtin::manifest();
    let mut effects: Vec<_> = manifest
        .effects
        .iter()
        .map(|d| measure(&package, d))
        .collect::<Result<_, _>>()?;
    effects.sort_by(|a, b| a.id.cmp(&b.id));
    let report = Report {
        schema_version: 1,
        metric_kind: "portable_validation_product_v1",
        scope: "current_builtin_manifest",
        dimensions: [WIDTH, HEIGHT],
        frame: 0,
        parameter_scenario: "defaults",
        package_id: package.manifest.id.clone(),
        package_version: package.manifest.version.clone(),
        package_hash: package.hash.clone(),
        source_revision: std::env::var("GITHUB_SHA").ok(),
        loop_warning_threshold: LOOP_WARNING,
        scratch_warning_bytes: motion_effects::SCRATCH_BUDGET,
        strict: std::env::var("MOTION_EFFECT_STRICT").as_deref() == Ok("1"),
        effects,
    };
    let table = markdown(&report);
    eprintln!("{table}");
    let count = report.effects.iter().map(|e| e.warnings.len()).sum();
    for effect in &report.effects {
        for w in &effect.warnings {
            eprintln!("WARN {} {}: {}", effect.id, w.code, w.detail);
        }
    }
    if let Some(directory) = std::env::var_os("MOTION_EFFECT_REPORT") {
        write_report(Path::new(&directory), &report, &table)?;
    }
    enforce(report.strict, count)
}

#[test]
fn warnings_only_block_when_strict_is_explicitly_enabled() {
    assert!(enforce(false, 100).is_ok());
    assert!(enforce(true, 0).is_ok());
    assert!(enforce(true, 1).is_err());
}

#[test]
fn high_work_shader_is_reported_and_snapshot_files_preserve_null_capacity() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut manifest = motion_effects::builtin::manifest();
    let mut definition = manifest
        .effects
        .iter()
        .find(|e| e.id == "tint")
        .unwrap()
        .clone();
    definition.passes[0].shader = "shaders/cost.wgsl".into();
    manifest.effects = vec![definition.clone()];
    std::fs::create_dir(directory.path().join("shaders"))?;
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&manifest)?,
    )?;
    std::fs::write(directory.path().join("shaders/cost.wgsl"),
        "fn main_fx(p:vec2<f32>)->vec4<f32>{var c=sample_input(p);for(var i:i32=0;i<32;i=i+1){for(var j:i32=0;j<80;j=j+1){c+=vec4(0.001);}}return c;}")?;
    let package = Arc::new(EffectPackage::from_bytes(motion_effects::package_directory(
        directory.path(),
    )?)?);
    let cost = measure(&package, &definition)?;
    assert_eq!(cost.loop_work_max, 2560);
    assert!(cost
        .warnings
        .iter()
        .any(|w| w.code == "loopWork" && w.pass == Some(0)));
    assert_eq!(cost.status, "budgetRejected");
    assert!(cost.scratch_4k_bytes.is_none());
    assert!(cost.scratch_4k_required_bytes.unwrap() > motion_effects::SCRATCH_BUDGET);
    let report = Report {
        schema_version: 1,
        metric_kind: "portable_validation_product_v1",
        scope: "test_package",
        dimensions: [WIDTH, HEIGHT],
        frame: 0,
        parameter_scenario: "defaults",
        package_id: manifest.id,
        package_version: manifest.version,
        package_hash: package.hash.clone(),
        source_revision: None,
        loop_warning_threshold: LOOP_WARNING,
        scratch_warning_bytes: motion_effects::SCRATCH_BUDGET,
        strict: false,
        effects: vec![cost],
    };
    let output = directory.path().join("report");
    let table = markdown(&report);
    write_report(&output, &report, &table)?;
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("effect-costs.json"))?)?;
    assert!(value["effects"][0]["scratch4kBytes"].is_null());
    assert!(
        value["effects"][0]["scratch4kRequiredBytes"]
            .as_u64()
            .unwrap()
            > motion_effects::SCRATCH_BUDGET
    );
    assert_eq!(
        std::fs::read_to_string(output.join("effect-costs.md"))?,
        table
    );
    // An unwritable target must fail even though cost warnings do not.
    let blocked = directory.path().join("blocked");
    std::fs::write(&blocked, "file")?;
    assert!(write_report(&blocked, &report, &table).is_err());
    Ok(())
}

#[test]
fn scratch_observation_keeps_rejections_passthrough_and_reset_behavior() -> TestResult {
    let package = motion_effects::builtin::package()?;
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "gaussian_blur")
        .unwrap();
    let mut project = Project::new(64, 64, 30, 60)?;
    let mut layer = Layer::solid(1, "4K source", [3840., 2160.], [32., 32., 0.], [1.; 4]);
    layer.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        layer.size,
    ));
    project.layers.push(layer);
    project.rebuild_plugin_dependencies();
    let sample = |p: &Project| {
        let mut scene = Scene::new(p);
        scene.sample(p, 0., None).unwrap();
        scene
    };
    let mut registry = Registry::default();
    registry.insert(package.clone())?;
    let mut builder = PlanBuilder::new(registry.clone())?;
    assert!(builder
        .build(&sample(&project), &[0], 64, 64, true)
        .is_err());
    let rejected = builder.last_scratch_request.unwrap();
    assert_eq!(rejected.budget_bytes, builder.scratch_budget());
    assert_eq!(rejected.rejection, Some(ScratchRejection::CapacityBudget));
    assert!(scratch_capacity_bytes(&rejected.sizes) > motion_effects::SCRATCH_BUDGET);
    assert_eq!(scratch_capacity_bytes(&builder.frame.scratch_sizes), 0);
    let passthrough = builder.build(&sample(&project), &[0], 64, 64, false)?;
    assert!(passthrough.passes.is_empty());
    assert_eq!(passthrough.diagnostics.len(), 1);
    assert_eq!(builder.last_scratch_request, Some(rejected));

    project.layers[0].size = [64.; 2];
    builder.build(&sample(&project), &[0], 64, 64, true)?;
    let accepted = builder.last_scratch_request.unwrap();
    assert_eq!(accepted.rejection, None);
    assert_eq!(
        scratch_capacity_bytes(&accepted.sizes),
        scratch_capacity_bytes(&builder.frame.scratch_sizes)
    );
    builder.device_dimension = 32;
    assert!(builder
        .build(&sample(&project), &[0], 64, 64, true)
        .is_err());
    assert_eq!(
        builder.last_scratch_request.unwrap().rejection,
        Some(ScratchRejection::DimensionLimit)
    );
    project.layers[0].effects.clear();
    project.rebuild_plugin_dependencies();
    builder.build(&sample(&project), &[0], 64, 64, true)?;
    assert!(builder.last_scratch_request.is_none());
    builder.last_scratch_request = Some(rejected);
    builder.set_registry(registry);
    assert!(builder.last_scratch_request.is_none());
    Ok(())
}

#[test]
fn observation_uses_device_budget_and_cached_preview_performs_no_new_check() -> TestResult {
    let package = motion_effects::builtin::package()?;
    let def = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "tint")
        .unwrap();
    let mut project = Project::new(WIDTH, HEIGHT, 30, 60)?;
    let size = [WIDTH as f32, HEIGHT as f32];
    let mut layer = Layer::solid(1, "4K tint", size, [1920., 1080., 0.], [1.; 4]);
    layer.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        size,
    ));
    project.layers.push(layer);
    let mut scene = Scene::new(&project);
    scene.sample(&project, 0., None)?;
    let mut registry = Registry::default();
    registry.insert(package)?;
    let mut builder = PlanBuilder::new(registry)?;
    assert!(builder.build(&scene, &[0], WIDTH, HEIGHT, true).is_err());
    let refused = builder.last_scratch_request.unwrap();
    assert_eq!(refused.budget_bytes, 64 << 20);
    assert_eq!(refused.rejection, Some(ScratchRejection::CapacityBudget));
    builder.set_scratch_budget(96 << 20)?;
    builder.build(&scene, &[0], WIDTH, HEIGHT, true)?;
    let accepted = builder.last_scratch_request.unwrap();
    assert_eq!(accepted.budget_bytes, 96 << 20);
    assert_eq!(accepted.rejection, None);
    assert_eq!(accepted.sizes, refused.sizes);
    builder.build_preview(&scene, &[0], 960, 540)?;
    assert!(builder.last_scratch_request.is_some());
    builder.build_preview(&scene, &[0], 960, 540)?;
    assert_eq!(builder.preview_cache_hits, 1);
    assert!(builder.last_scratch_request.is_none());
    Ok(())
}
