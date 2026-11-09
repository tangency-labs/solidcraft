# Definition-local visual mapping

`solidcraft_render::mapping` provides numerical admission and coordinates for #1214,
the first mapping foundation under #786. It is original Rust code with no new
dependency or asset. It does not install a sampler, triplanar shader, texture pack,
document record, command or UI. No renderer capability changes. Textured wood is
not yet available through this utility.

`VisualMapping::new(origin_mm, axes, repeat_mm)` admits an immutable frame. Origin
and axes belong to the body's **definition-local** space. Axes are ordered visual
grain/across/depth, not engineering L/R/T. They must be finite, unit length and
mutually perpendicular, with positive handedness. Squared lengths, dot products
and determinant use absolute tolerance `1e-10` for floating-point rotation error.
Values within that tolerance are retained exactly; no normalization, skew repair
or substitute frame is performed. Private fields and the absence of serde require
future loaders to go through admission.

Repeat sizes are positive physical distances along those axes, not fractions of a
body's current bounds. Coordinates are the signed, unwrapped values
`dot(local_point_mm - origin_mm, axis_i) / repeat_mm_i`. Negative coordinates are
valid. Wrapping, filtering, map-role interpretation and normal-map tangent bases
belong to the eventual consumer. These coordinates alone do not define seamless
triplanar blending, end-grain appearance or an anisotropic optical model.

Admission limits:

- Each local origin/input coordinate: finite, absolute value at most `1e9` mm.
- Each repeat size: finite, `[0.001, 1e6]` mm.
- Each resulting repeat coordinate: finite, absolute value at most `1e6`.

These are explicit numerical refusal bounds, not measured engineering ranges or
GPU quality guarantees. Calculations use f64; a future f32 shader needs its own
precision budget and supported scene limits. Values are never silently clamped.

`suggest_grain_axis` considers only a finite, ordered definition-local AABB with
at least one positive extent. It selects the largest extent; exact ties use X,
then Y, then Z. Flat or line bounds are admissible, a point/empty box is not. Its
`GrainAxis::axes()` supplies a deterministic right-handed initial frame. A caller
must store an accepted frame once, and allow explicit edits; it must not rerun the
suggestion automatically when parameters change. `VisualMapping` holds no bounds
and therefore cannot independently flip on resize.

For an instance, the host first inverts the actual rigid placement to recover
definition-local coordinates. This utility neither guesses placement from world
bounds nor implements another transform system. Mirroring, deformed/nonrigid
instances and face-specific mapping need explicit consumer policies. Tests use an
independent analytic quarter-turn/translation to check this coordinate convention;
they do not certify a document/assembly adapter that has not been installed.

The remaining phases are persistence/copy/undo/reopen through existing appearance
commands, actual CPU/GPU texture consumption and honest unsupported-map behavior,
then manual controls and native/browser visual evidence. No document version is
reserved here. Source/license receipts and admitted resources must remain portable
when that consumer is added. Visual grain never changes mass, CAD bounds, mechanical
axes or manufacturing stock/nesting. #786/#813/#922 remain open.
