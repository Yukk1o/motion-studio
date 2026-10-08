# Android runtime boundaries

`../runtime.rs` owns `Session`, its worker-thread invariant, graphics resources,
Surface attachment, rendering and scene sampling. These child modules implement
the Java-facing adapters; moving an endpoint here does not change its exported
`Java_com_motionstudio_editor_*` symbol or JNI calling convention.

| Module | Responsibility |
| --- | --- |
| `bridge.rs` | JNI strings, Base64 and the existing JSON success/error envelope; panic fallbacks for integer and byte-array results |
| `buffers.rs` | Direct-buffer metadata, preserved validation order and validated byte writes |
| `project.rs` | Session creation/destruction, creation errors, templates, project storage and resource registry queries |
| `editing.rs` | Command dispatch, drag edits, history and temporal curve graphs |
| `preview.rs` | Seeking/observation/navigation, Surface binding, render calls, preview policy and profiling/diagnostics |
| `effects.rs` | Effect registry and plugin/editor request dispatch, editor preview and package image reads |
| `export.rs` | Render-plan descriptions and packed outputs, capture, project packages and source image reads |
| `geometry.rs` | Hit queries and geometry parameter/vertex outputs |
| `images.rs` | Image inspection, bounded preview proxy preparation and full-resolution direct-buffer transfer |
| `snapshot.rs` | State, sampled values, capabilities and preview metrics projected into the existing JSON protocol |
| `composition.rs` | Composition context, request dispatch, frame bundles and composition rendering |
| `media_audio.rs` | Media import/probe dispatch, URI access, audio preparation and frozen/live PCM readers |
| `media_video.rs` | Live/frozen video frame transfer and `JNI_OnLoad` |

The media and composition modules retain their Rust module names
`audio_runtime`, `video_runtime` and `composition_runtime` in the parent so that
the platform decoders and Session context types keep their existing paths.
Decoder, codec-capability and video-cache/frame implementations remain outside
this directory.

Adapters acquire sessions through `with_session`, which keeps the original
registry locking and owning-worker-thread check. Command and plugin request
dispatch operate on `Session`; they do not depend on a Java environment.
Snapshots and resource resets still run at the same points in those operations.

Wire contracts are unchanged: JSON uses `ok`/`data` or `ok`/`error`, composition
errors retain `error_detail`, packed integer APIs retain their failure sentinel,
frame bundles retain the negative required-capacity response, and pixel APIs
retain a null-array failure response. Buffer access deliberately preserves each
endpoint's existing read-only/capacity/address validation order. Callers retain
ownership and validation of the memory passed to the unsafe byte-write helpers.

The `diagnostics` feature continues to guard GPU fault injection inside the
preview adapter, and `diagnosticsEnabled` remains feature-derived in snapshots.
This refactor does not change project/render-plan versions or introduce features
from other worktrees.
