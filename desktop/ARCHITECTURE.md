# Desktop architecture

The desktop application is a native winit + wgpu program that runs the same
editing engine as the Android build. This document explains how the two stay in
step, what the desktop port adds, and which capabilities are deliberately not
shared.

## Layering

```
                    ┌───────────────────────────────────────────┐
 Android (Kotlin)  │  Compose UI  ·  MediaCodec  ·  SurfaceView  │
                    └───────────────────┬───────────────────────┘
                                        │ JNI (JSON envelope)
 Desktop (winit)    │  aem-ui panels  ·  libav  ·  winit window   │
                    └───────────────────┬───────────────────────┘
                                        │ direct Rust calls
                    ┌───────────────────▼───────────────────────┐
                    │  aem-host: session, ops, state snapshot   │
                    ├───────────────────────────────────────────┤
                    │  aem-core · aem-render · aem-effects      │
                    │  aem-media                                 │
                    └───────────────────────────────────────────┘
```

`aem-host` is the whole editor. It owns the session, the sampled scene, the
effect plan builder, the GPU preview, media scheduling, and the JSON protocol the
UI speaks. It knows nothing about Android or desktop.

Everything below `aem-host` is a platform backend behind
`aem_host::platform::Platform`:

| Capability | Android | Desktop |
| --- | --- | --- |
| Video probe | `AMediaExtractor` | libav `avformat` |
| Video decode | `AMediaCodec` + `ImageReader` | libav `avcodec` |
| Audio fallback | `AMediaCodec` | none — Symphonia covers it |
| Decoder inventory | `MediaCodecList` | fixed build-time list |
| Presentation | `SurfaceView` + `ANativeWindow` | winit window |
| File selection | `content://` via `ContentResolver` | filesystem path |

## Why the host is shared rather than duplicated

The desktop UI cannot drift behind the Android SDK surface, because there is no
second implementation to drift. A capability is added once, in `aem-host`, and
both applications see it. Concretely:

* The `state` snapshot, including the `capabilities` block, is produced by one
  function. Both UIs read the same keys, so a panel that works on the phone
  works on the desktop unless the host explicitly reports it absent.
* Command, gesture, undo, timeline, composition, effect, plugin and media
  operations are ordinary Rust functions in `aem-host::ops`. The JNI layer and
  the desktop shell are both thin adapters over them.
* The versioned render plan (`aem_render::effect_plan`) and the composition
  bundle (`composition_plan`) already had a backend-neutral design: "the same
  frame plan drives wgpu and the MediaCodec/GLES adapter". Desktop uses the wgpu
  path directly.

`crates/aem-android/src/jni_api.rs` is the reference for how little a platform
adapter should contain. If a change makes that file grow, the change probably
belongs in `aem-host`.

## Threads

One engine thread owns the session, on both platforms.

* Android: a `HandlerThread("motion-render")`; every JNI call arrives there.
* Desktop: a `motion-engine` thread started by `Engine::start`, with a typed
  `Command` channel. Requests are a closed enum rather than strings, so a UI typo
  becomes a compile error instead of a runtime failure.

The session asserts its owning `ThreadId`. The UI thread only reads an immutable
snapshot, which is what lets the desktop shell redraw at any time without
touching engine state.

## The UI layer

`desktop/ui` (`aem-ui`) is a small immediate-mode toolkit rather than a general
purpose one:

* `paint` records flat shape and glyph commands and draws them in two passes.
* `dock` resolves an After Effects style tab/dock tree into rectangles. It has no
  GPU dependency, so the layout rules carry their own tests.
* `ui` draws widgets — buttons, scrub fields, tab strips, checkboxes, the layer
  list, the timeline — and reports interaction through `input`.
* `text` rasterises glyphs into a single atlas, with system CJK fonts and the selected font collection face index.
* The desktop shell owns a mutable dock tree and native floating windows. All windows use one session, adapter and GPU device; each window has one swapchain.

The panels in `desktop/app/src/panels.rs` follow the After Effects arrangement:
Project on the left, Composition in the centre, Effect Controls on the right, Timeline
across the bottom, with a menu bar and toolbar above.

## Performance

The preview is presented by aem-host into one swapchain per window. The panel
painter loads the same target after the composition pass. Preview has no pixel
readback; diagnostic screenshots explicitly use a separate capture target.

Winit window handles and surfaces are created on the GUI thread, including on
Windows, and then transferred to the session worker. Attach, resize and present
requests return asynchronously so the GUI can continue pumping OS messages.
Floating composition windows temporarily select their target during the render;
other floating panels share the device and atlas without creating a second
editing session.

MCP stdio may run headless or share the GUI's Arc<Engine>. Successful tools wake
the GUI through a winit user event. ToolRouter is the embedded-agent boundary:
read-only/edit permissions and atomic revision-checked batches use the existing
command queue, and an active pointer gesture rejects agent edits.

* **Present mode.** Mailbox where the platform supports it, Fifo otherwise, with
  `desired_maximum_frame_latency: 2`. This matches the Android path's Fifo choice
  while allowing lower latency on desktop.
* **Backend order.** Vulkan, then Metal, DX12 and GL. Only one backend's surface
  stays alive for a window, because creating a surface can connect the window's
  buffer producer even when the adapter turns out to be unusable.
* **Preview policy.** `PreviewPolicy` is the existing engine code: clear, smooth,
  economy and automatic tiers, chosen from measured CPU and GPU cost.
* **Scratch budget.** `resource_policy::scratch_budget` derives the GPU scratch
  budget from device memory. Desktop passes machine memory, because wgpu does not
  expose device memory; the policy itself is untouched.
* **Video.** Decoded frames stay in YUV and the existing GPU conversion path
  runs, so importing 4K video costs no more on the desktop than on the phone.

## Deliberately not shared

| Concern | Why |
| --- | --- |
| Interaction model | The phone UI is touch-first: long press to move a keyframe, inertial scrolling, explicit layout modes. Desktop uses pointer precision and keyboard shortcuts. The gesture *decisions* live in `aem-host::video_decode_policy` and `aem_core`; only the gesture recognition differs. |
| Widgets | Compose versus `aem-ui`. |
| Colour management | Desktop displays are usually sRGB and managed; Android panels are not assumed to be. Colour conversion stays in the engine. |
| Text rasterisation | Android rasterises text layers with `android.graphics`. Desktop needs a platform text stack and is not implemented yet. |
| Legacy plugin editors | SDK 2 editors are HTML pages mounted in a WebView. Desktop needs WebView2 or WKWebView before legacy editors open. SDK 5 native editors have no such requirement. |

## Building

```powershell
# Shell that edits, previews and exports audio; no libav needed.
py tools/build_desktop.py --task build

# Shell with video import and export through libav.
py tools/build_desktop.py --task build --ffmpeg
```

On Windows the desktop build needs the MSVC C++ build tools, the same
requirement the Android native core checks already have. `FFMPEG_INCLUDE_DIR` and
`FFMPEG_LIB_DIR` override pkg-config discovery.

`MOTION_PROJECT` selects the project directory to open at start.

## Verification

`aem-ui::dock` and `aem-host` carry unit tests that need no GPU. The CI `desktop`
job checks the workspace, runs those tests, and produces a release binary on
Linux, Windows and macOS. The `desktop-media` job installs libav and builds with
`--ffmpeg` so the optional backend cannot silently rot.