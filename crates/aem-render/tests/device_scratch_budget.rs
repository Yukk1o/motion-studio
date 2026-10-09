use aem_core::{Content, EffectInstance, Layer, Project, Scene};
use aem_render::{
    effect_plan::{PlanBuilder, PLAN_VERSION},
    resource_policy::scratch_budget,
};
const GIB: u64 = 1024 * 1024 * 1024;
fn project(names: &[&str]) -> Project {
    let package = aem_effects::builtin::package().unwrap();
    let mut p = Project::new(3840, 2160, 30, 60).unwrap();
    let mut l = Layer::solid(
        1,
        "4K adjustment",
        [3840., 2160.],
        [1920., 1080., 0.],
        [1.; 4],
    );
    l.content = Content::Adjustment;
    for (i, name) in names.iter().enumerate() {
        let d = package
            .manifest
            .effects
            .iter()
            .find(|d| &d.id == name)
            .unwrap();
        l.effects.push(EffectInstance::new(
            i as u64 + 1,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            d,
            l.size,
        ));
    }
    p.layers.push(l);
    p
}
#[test]
fn memory_policy_is_bounded_and_guarded_or_unknown_devices_keep_the_floor() {
    assert_eq!(scratch_budget(0, false), 64 << 20);
    assert_eq!(scratch_budget(3 * GIB, false), 96 << 20);
    assert_eq!(scratch_budget(6 * GIB, false), 192 << 20);
    assert_eq!(scratch_budget(12 * GIB, false), 384 << 20);
    assert_eq!(scratch_budget(u64::MAX, false), 384 << 20);
    for mem in [0, 3 * GIB, 6 * GIB, 12 * GIB] {
        assert_eq!(scratch_budget(mem, true), 64 << 20);
    }
}
#[test]
fn four_k_plans_obey_each_tier_and_serialize_the_same_allowance_for_gles() {
    let p = project(&["glow_edges"]);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let mut b = PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    for (mem, fits) in [(3 * GIB, false), (6 * GIB, true), (12 * GIB, true)] {
        let budget = scratch_budget(mem, false);
        b.set_scratch_budget(budget).unwrap();
        match b.build(&scene, &[0], 3840, 2160, true) {
            Ok(plan) => {
                assert!(fits);
                assert!(
                    aem_render::effect_plan::scratch_capacity_bytes(&plan.scratch_sizes) <= budget
                );
                let mut out = vec![0; plan.buffer_bytes(&scene)];
                plan.write(&scene, &mut out).unwrap();
                assert_eq!(
                    u32::from_ne_bytes(out[4..8].try_into().unwrap()),
                    PLAN_VERSION
                );
                assert_eq!(
                    u32::from_ne_bytes(out[76..80].try_into().unwrap()) as u64,
                    budget
                );
            }
            Err(error) => {
                assert!(!fits, "{error}");
                assert!(error.contains("effect scratch textures"), "{error}");
                assert!(error.contains("96 MiB"), "{error}");
                assert!(!error.contains("accumulators"), "{error}");
            }
        }
    }
    // Mixing linear glow and sRGB blur reserves both working-slot sets;
    // the chain needs 225.90 MiB here, so the 192 MiB tier must reject it.
    let p = project(&["glow_edges", "gaussian_blur"]);
    scene.sample(&p, 0., None).unwrap();
    b.set_scratch_budget(scratch_budget(6 * GIB, false))
        .unwrap();
    let error = b.build(&scene, &[0], 3840, 2160, true).err().unwrap();
    assert!(error.contains("192 MiB"), "{error}");
    b.set_scratch_budget(scratch_budget(12 * GIB, false))
        .unwrap();
    assert!(b.build(&scene, &[0], 3840, 2160, true).is_ok());
    let p = project(&["tint"]);
    scene.sample(&p, 0., None).unwrap();
    b.set_scratch_budget(scratch_budget(3 * GIB, false))
        .unwrap();
    assert!(b.build(&scene, &[0], 3840, 2160, true).is_ok());
    b.set_scratch_budget(scratch_budget(12 * GIB, true))
        .unwrap();
    let error = b.build(&scene, &[0], 3840, 2160, true).err().unwrap();
    assert!(error.contains("64 MiB"), "{error}");
}
#[test]
fn changing_budget_invalidates_preview_and_still_enforces_device_dimensions() {
    let p = project(&["glow_edges"]);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let mut b = PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    b.set_scratch_budget(192 << 20).unwrap();
    assert!(b
        .build_preview(&scene, &[0], 3840, 2160)
        .unwrap()
        .diagnostics
        .is_empty());
    b.build_preview(&scene, &[0], 3840, 2160).unwrap();
    assert_eq!(b.preview_cache_hits, 1);
    b.set_scratch_budget(64 << 20).unwrap();
    assert!(!b
        .build_preview(&scene, &[0], 3840, 2160)
        .unwrap()
        .diagnostics
        .is_empty());
    assert_eq!(b.preview_cache_hits, 1);
    assert!(b.set_scratch_budget(512 << 20).is_err());
    assert!(b.set_scratch_budget(1).is_err());
    b.set_scratch_budget(384 << 20).unwrap();
    b.device_dimension = 2048;
    let error = b.build(&scene, &[0], 3840, 2160, true).err().unwrap();
    assert!(error.contains("device dimension"), "{error}");
}
