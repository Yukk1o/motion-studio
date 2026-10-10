# SDK parity matrix

What the desktop host supports today, compared with the Android build. "Shared"
means the same code in `aem-host` runs on both platforms.

Legend: **yes** shipped · **partial** works with a documented limit · **no** not
yet implemented on desktop.

## Editing

| Capability | Desktop | Notes |
| --- | --- | --- |
| Keyframes, tracks, curve editing | yes | Shared (`aem_core::animation`, `curve`) |
| Undo, redo, gestures, coalescing | yes | Shared (`aem_core::editor`) |
| Bezier and elastic curves | yes | Shared (`aem_core::curve`) |
| Axis separation (XYZ) | yes | Shared (`Command::SeparateDimensions`) |
| Camera: position and orbit paths | yes | Shared (`aem_core::camera`) |
| Observer, top and side views | yes | Shared |
| Layer clips: move, trim, split | yes | Shared |
| Masks | yes | Shared (`aem_core::masks`) |
| Vector shapes and paths | yes | Shared (`aem_core::vector`) |
| Adjustment layers | yes | Shared |
| Nested compositions, precompose | yes | Shared (`aem_core::composition`) |
| Expression workspace | yes | Shared QuickJS runtime |
| Multi-select and batch edit | yes | Shared commands; desktop batch UI pending |
| Copy/paste layers with parents | yes | Shared |

## Effects and plugins

| Capability | Desktop | Notes |
| --- | --- | --- |
| 59 core effect packs | yes | Byte-identical `.msfx`; identity is the SHA-256 of the package |
| 6 scene effect packs, 1 emitter pack | yes | Same registry, same byte pinning |
| Effect chains, ordering, enable | yes | Shared |
| Parameter animation | yes | Shared |
| Five-channel colour curves | yes | Shared (`aem_core::color_curves`) |
| Per-instance parameter ranges | yes | Shared (`PlanBuilder::preflight_project`) |
| Effect pack install/enable/uninstall | yes | Shared (`Registry`); native project file dialog available; plugin pack picker pending |
| SDK 5 native plugin editors | partial | Engine side shared; slot widgets not drawn yet |
| SDK 2 HTML plugin editors | no | Needs WebView2 or WKWebView |

## Media

| Capability | Desktop | Notes |
| --- | --- | --- |
| Audio import (MP3, FLAC, AAC, ALAC, Vorbis, Opus, WAV, AIFF) | yes | Shared Symphonia path |
| Waveform generation | yes | Shared |
| Deterministic mixing, 48 kHz stereo | yes | Shared (`aem_media::mixer`) |
| Video import (H.264, H.265, VP8, VP9 in MP4/MOV/MKV/WebM) | partial | libav behind `--ffmpeg` |
| Video preview and PTS prefetch | partial | Shared scheduling, libav decode |
| Project package import and export | yes | Shared (`aem_core::storage`) |
| PNG still export | yes | Shared |
| MP4 export with AAC audio | no | Needs the desktop encoder; see below |
| Device decoder inventory query | yes | Fixed list rather than `MediaCodecList` |

## Known gaps and why

**MP4 export.** Android encodes through MediaCodec into an input Surface, driven
by `VideoExporter.kt`. The desktop equivalent is specified — `aem-render` already
emits a versioned frame plan and a GLSL ES 300 variant for exactly this purpose,
and `aem-desktop-media` declares the encoder settings and bitrate policy — but
the encoder itself is not written yet. It needs a libav encode loop that consumes
`PlanBuilder::build` output and muxes AAC from `media::read_frozen_pcm_into`.

**Text layers.** Android rasterises text to a PNG asset with
`android.graphics.Paint`. Desktop needs a platform text stack.

**Native plugin editor widgets.** `PluginEditorSession` is shared and the
protocol is declarative, so the engine half is done. `aem-ui` still needs to
render the slot kinds: parameters, layer source, image sprite, seed, transform,
preview, timeline and note.

**Multi-select UI.** The engine command surface is shared. The desktop batch
panel is not built. MCP motion_edit accepts shared-core atomic command batches.

**Plugin pack management UI.** Install, enable and uninstall all work; picking a
`.msfx` needs a native file dialog.

## How to keep this honest

`crates/aem-host/src/ops/snapshot.rs` builds the `capabilities` block that both
UIs read. When a host cannot satisfy a capability it must report it there rather
than omitting a field, so this table can be regenerated from a running build
instead of being maintained by hand.
## Desktop workspace and automation

Basic desktop panels now support typed numeric transform editing, animation toggles, timeline scrubbing, clip movement, undo/redo, project open/save and package export. Panels can resize, dock, combine tabs or float in native windows, including the composition preview. Chinese and English UI preferences persist separately from projects.

The stdio MCP adapter exposes state, atomic revision-checked editing, seek, history and save. --mcp-ui shares the displayed project and undo history with the client; --mcp is headless. Node authoring and msfx export from nodes remain a design proposal.
