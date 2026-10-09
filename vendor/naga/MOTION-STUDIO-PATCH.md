# Naga 24.0.0: portable GLES uint literals

Source: crates.io naga 24.0.0, gfx-rs/wgpu commit `dc1e50e08cd4a5c8d6c3e8a180e93535d40bec4a`. Upstream source and licenses are retained. This is a pinned dependency patch.

The sole compiler change is in `src/back/glsl/mod.rs`: uint values above INT_MAX are emitted as two 16-bit literals joined by shift/OR. Some GLES compilers clamp wide decimal or hex literals to INT_MAX; the expression preserves all 32 bits.

The Cargo patch covers both aem-effects GLSL export and wgpu-hal native GLES preview. Original WGSL, package bytes/hashes and Vulkan arithmetic are preserved. Remove this patch when the pinned dependency provides equivalent portability.
