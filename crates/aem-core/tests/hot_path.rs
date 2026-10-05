use aem_core::{Curve, CurveShape, CurveSpace, Ease, Easing, Layer, Project, Scene};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
struct CountedAllocator;
unsafe impl GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        MEASURING.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        MEASURING.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountedAllocator = CountedAllocator;

#[test]
fn twenty_layer_animation_sampling_reuses_memory_at_sixty_hz() {
    let mut project = Project::new(1080, 1920, 30, 180).unwrap();
    for i in 0..20 {
        let mut layer = Layer::solid(
            i + 1,
            "reference",
            [160.0, 240.0],
            [100.0 + i as f32 * 40.0, 960.0, i as f32 * 20.0],
            [0.3, 0.7, 0.8, 0.7],
        );
        layer.three_d = true;
        layer.transform.rotation.value[1] = if i % 2 == 0 { 12.0 } else { -12.0 };
        let start = layer.transform.position.value;
        layer
            .transform
            .position
            .upsert(0, start, Ease::InOut)
            .unwrap();
        layer
            .transform
            .position
            .upsert(150, [start[0] + 100.0, 860.0, start[2]], Ease::Linear)
            .unwrap();
        layer
            .transform
            .position
            .set_curve(
                0,
                Easing {
                    ease: Ease::Linear,
                    curve: Some(Curve {
                        space: if i % 2 == 0 {
                            CurveSpace::Progress
                        } else {
                            CurveSpace::Velocity
                        },
                        shape: CurveShape::Cubic {
                            control1: [0.2, 0.3],
                            control2: [0.8, 1.5],
                            start: 0.0,
                            end: 1.0,
                        },
                    }),
                },
            )
            .unwrap();
        project.layers.push(layer);
    }
    for (i, layer) in project.layers.iter_mut().enumerate() {
        layer.transform.position.separate().unwrap();
        layer.transform.rotation.separate().unwrap();
        layer.transform.scale.separate().unwrap();
        layer.timeline = Some(aem_core::LayerTimeline {
            in_frame: 0,
            out_frame: 180,
            offset_frame: i as i32 - 10,
        });
    }
    project.validate().unwrap();
    let mut scene = Scene::new(&project);
    let mut compositor = aem_core::PlaneCompositor::new();
    // Warm all animated geometry sizes, then require scratch reuse on playback.
    for tick in 0..360 {
        scene.sample(&project, tick as f64 * 0.5, None).unwrap();
        compositor.prepare(&scene).unwrap();
    }
    scene.sample(&project, 0.0, None).unwrap();
    ALLOCATIONS.with(|count| count.set(0));
    MEASURING.with(|flag| flag.set(true));
    for tick in 0..360 {
        scene.sample(&project, tick as f64 * 0.5, None).unwrap();
        compositor.prepare(&scene).unwrap();
    }
    MEASURING.with(|flag| flag.set(false));
    let count = ALLOCATIONS.with(Cell::get);
    assert_eq!(count, 0, "render sampling allocated on the CPU hot path");
    assert_eq!(scene.layers.len(), 20);
}

#[test]
fn continuous_preview_accepts_the_last_half_frame_but_not_the_end_boundary() {
    let p = Project::demo();
    let mut s = Scene::new(&p);
    s.sample(&p, 179.5, None).unwrap();
    assert!(s.sample(&p, 180.0, None).is_err());
}
