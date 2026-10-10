# Position editing and spatial keyframes

Read `capabilities.spatial_paths` before offering path controls. Version 1 adds
the host's shared effect-position widget, viewport position point and editable spatial
handles. Position, center, origin and source-position vector parameters use the
same X/Y/Z selector, numeric wheel and touchpad overlay. Normal layer transforms retain their numeric fields and transform pad. Long-press an axis value
for precise input. The touchpad has a return button and an optional Z mode. Viewport path points and handles can be added or edited only while the host easing panel is open.

Native editor slots remain declarative. No plugin JavaScript owns these controls.
Position gestures keep the plugin page's undo transaction and cancel only their
own parameter track.

## Spatial data

Spatial data is optional on a keyframe and requires project format 9. Projects
without spatial data keep their previous linear interpolation. Tangents are
offsets from the key value, in that property's local pixel coordinates:

```json
{
  "frame": 30,
  "value": [120, 80, 0],
  "ease": "linear",
  "spatial": {
    "incoming": [-40, -20, 0],
    "outgoing": [40, 20, 0]
  }
}
```

The segment is a cubic Bezier. An unspecified control defaults to one third of
the straight segment. Temporal easing supplies the Bezier progress independently;
hold interpolation stays a hold. Spatial arc-length speed matching is not part
of version 1. Separated dimensions can display a trajectory; spatial handle
editing requires coupled dimensions. Reset the spatial handles before separating.
Numeric values and controls are checked against the property's effective range.

SDK effect parameters store four components, so append a zero to position
tangents for a vector effect parameter. Unused components must remain zero.

## Read-only geometry query

Use the composition request API on the engine owner thread:

```json
{
  "version": 1,
  "composition": "comp-main",
  "op": "position_path",
  "target": { "kind": "property", "object": 1, "property": "position" }
}
```

For a vector effect use a target with `kind: "effect"`, `object`, `effect` and
`param`. The response includes the current value, projection matrix, sampled
trace, keyframe positions, control points, numeric limits and editability flags.
Geometry uses the current parent and camera; it does not seek or create history.
Enabled expressions keep the base trajectory visible and make viewport handles
read-only. A particle emitter's position uses its selected source's pivot space.
Trace sampling is capped at 257 points; up to 512 nearby keyframes are returned,
with `keys_truncated` declared when applicable.

## Host and native-page edits

Layer command:

```json
{
  "op": "spatial", "object": 1, "property": "position", "frame": 30,
  "tangents": { "incoming": [-40, -20, 0], "outgoing": [40, 20, 0] }
}
```

An effect command uses `op: "effect"` with an action whose `kind` is `spatial`,
and includes `effect`, `param`, `frame` and four-component `tangents`.
`tangents: null` resets the key's spatial controls. Frame numbers in commands use
composition time; stored keyframes use the layer's local time.

Within an open native page, `parameter_begin` captures one vector parameter's
complete track. `set` edits at the current frame; `set_at` and `spatial` accept an
explicit composition frame. `parameter_finish` commits or restores that track.
Each mutating request includes the current `revision`. A stale request fails;
read `state` and retry instead of applying an outdated value. These requests
cannot access another effect instance or change an active color scope.
