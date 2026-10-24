# Persistent render settings (R2)

Named setups save common render intent in the document. They do not start a render,
apply the navigation camera or make a currently unavailable backend available.
The integrated workspace and jobs consume these settings through their own capability
checks. [Opaque PBR](render-opaque-pbr.md) is an opt-in appearance response; texture
and environment assets remain follow-ups of [#620](https://github.com/bherbruck/solidcraft/issues/620);
see [the combined integration boundary](render-integration.md).

## Commands

```json
{"command":"render.settings.set","params":{"name":"Studio","set":{"width":1600,"height":900,"exposure_ev":1.5}}}
{"command":"render.settings.get","params":{"name":"Studio"}}
{"command":"render.settings.get","params":{}}
```

`set` creates a named setup from defaults or replaces supplied top-level fields in an
existing setup. A supplied `camera` replaces the whole camera; it is not a recursive
patch. Omitted fields retain their values. Unknown fields (including camera fields),
invalid types and invalid values fail before document mutation. A new empty `set`
saves the defaults; setting an existing setup to identical values is a no-op,
preserving redo history and revision.
Each effective edit is one undo step and marks the document dirty. There is no delete
or rename command in this slice.

Named `get` returns `{name, setup}` or an error for a missing name. Without a name,
it returns `{setups, defaults}`. Reads do not evaluate the model or change history.

| Field | Default | Saved contract |
|---|---|---|
| `backend` | `builtin` | Valid R1 backend ID, at most 64 ASCII bytes; unavailable IDs survive reopen |
| `mode` | `preview` | R1 `preview` / `final` mode; availability checked by a future render request |
| `camera` | Existing `Camera::default()` | Snapshot of target, yaw, pitch, roll, distance, fov and up; mm/radians, existing Z-up default |
| `width`, `height` | 1024, 768 | Positive, each at most 4096; product at most 4,194,304 pixels |
| `format` | `png` | R1 output format (`png` / `exr`); saving EXR does not add an encoder |
| `exposure_ev` | 0 | Finite, -32 through 32 |
| `environment_rotation` | 0 | Finite radians, magnitude at most 1,000,000 |
| `environment_intensity` | 1 | Finite, 0 through 100 |
| `background` | [236, 240, 245] | Three sRGB byte values, independent of future illumination |

Camera target coordinates and positive distance are bounded to magnitude 1e9 mm;
angles to 1e6 radians. FOV is finite, `0 <= fov < pi`; zero means orthographic in
the existing camera model. Both existing camera up axes are retained. Setup names
are trimmed, nonempty, at most 128 UTF-8 bytes, without control characters. A document
has at most 32 setups. Validation also runs during document deserialization.

The CPU product-image executor (`render.preflight` / `render.start`) doesn't apply exposure or
the environment controls yet. A setup that sets them still renders there; the reply's
`ignored_controls` lists the ones that render leaves out (for example `["exposure_ev"]`).

## Persistence and evaluation

Document format 21 adds `render: {setups: {name: setup}}`. Format-20 and older designs migrate with
an empty map; empty render state is omitted when saved. Existing geometry, appearances,
opacity and physical materials are untouched. Plain JSON, design containers and crash
journal replay use the same document/command representation. Older builds (0.11.0 reads up to 20) reject format
21 instead of silently losing setups. This slice adds only render intent. No runtime capability, job, texture or device is saved.

The engine avoids geometry evaluation only after proving that the document differs
solely in validated render settings, with unchanged active sketch and pending moves.
The proof compares the complete document after normalizing the render field, so mixed
geometry/appearance/placement edits take the usual conservative refresh path. It uses
a document clone/comparison, not a constant-time or whole-application speedup claim.
Unchanged render pointers short-circuit the check for normal modelling commands.

Render-only edits, undo/redo and checkpoint restore preserve model state, its stale
marker, cached tessellation and the prepared world state. A stale live-drag result never
becomes authoritative through a settings edit; the next model-reading command still
settles it normally. Revision advances to let future consumers invalidate shading work.
Actual render accumulation and resource invalidation are not implemented in R2.

No new third-party dependency or asset is introduced. `doc` depends on the existing
pure `render` crate for its shared camera and R1 identifiers, within the L2 → L1 rule.
