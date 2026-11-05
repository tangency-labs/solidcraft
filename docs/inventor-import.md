# Inventor import

Inventor parts (`.ipt`) and assemblies (`.iam`) read through the Apache-2.0 cadmpeg codec,
like FreeCAD and Rhino (see [step import](step-import.md)). The translation into SolidCraft
document features is original project code; the attribution row is recorded in
`attribution/pending/inventor-cadmpeg-history.md` for folding on merge.

## Editable part history

Opening a part first tries to rebuild its fully decoded neutral history as native
SolidCraft features inside a component named after the file. It supports:

- Named scalar length/angle/real parameters with evaluable dependency expressions,
  checked against their decoded canonical values (mm/radians).
- Resolved planar sketches with point, line and circle geometry.
- Active horizontal, vertical, parallel, perpendicular, equal and explicit-locus
  coincidence constraints; parameter-bound line length, radius and diameter dimensions,
  driving and reference.
- Solid extrusions: normal/reversed-normal directions, explicit normal-aligned
  vectors, one-sided/two-sided blind distances, symmetric total distances,
  through-all extents, fixed plane offsets, and outer loops with their holes.
  Multi-body Boolean targeting is refused when the source targets cannot be mapped
  unambiguously.
- Feature order, names, component ownership, and known suppression state.

The history is installed only when it regenerates without errors; otherwise the
geometry import below follows, with a warning saying the history was not kept.
Explicit `names` or `up` options select the geometry path directly.

Extrusion extents are editable decoded values. The import does not guess which source
parameter drives an extent when the neutral feature does not carry that binding; the
import reply warns about this. Sketch dimensions whose neutral records explicitly
reference parameters keep those bindings.

## Geometry and assemblies

Insert and the geometry fallback convert the selected codec's admitted solids to STEP,
then use the existing Import feature. Assemblies remain limited to the
occurrence/geometry branches cadmpeg supports; this is not a claim of complete
Inventor version or assembly support. Browser builds use the same pure-Rust
decoding/translation path.

## Limits

The source decoder uses the existing service resource budgets and the step import's
file size limit; the document/sketch builders keep their own size and identity checks.
Tests in the existing native-CAD test module construct original neutral records,
check analytic volumes, change imported dimensions, and round-trip saved designs.
Hostile byte inputs must fail without an escaping panic.

Those tests verify the translation and its admission rules. A positive `.ipt`
byte-stream fixture exercising cadmpeg's parser end to end is still pending, and
broader validation and more history operations are needed before claiming
general-purpose compatibility.
