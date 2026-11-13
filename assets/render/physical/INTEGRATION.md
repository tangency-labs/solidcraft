# Physical material integration contract

This resource slice is based on `6d79d44fa403581e4508d7c9df5e163ff85085bf`.
It does not edit the parallel renderer implementation or the native document
format. The native candidate-card schema is in
[`crates/doc/src/physical_material.rs`](../../../crates/doc/src/physical_material.rs).
The personal integration branch separately implements design-owned cards and
library v2; capability checks must use the actual executing build.

## Three independent representations

1. **Extended reference profile:** the authoritative physical data with quantities,
   individual source values, SI conversions, conditions, ranges, uncertainty,
   directions and unresolved applicability. It is not a solver material model.
2. **Native schema-1 card:** only the supported scalar point properties and complete
   admitted elastic blocks. Unsupported data remains in the extended profile and
   is listed in `not_projected`; it is never silently discarded.
3. **Assigned document material:** the application's material object, its explicit
   mass density, optional physical card and optional appearance. A profile link in
   the render catalog is not evidence that this object has been created or assigned.

`exports/native-cards.json` is a bundle of individually valid card objects plus
projection metadata. It is not `solidcraft-library/1`, `solidcraft-library/2`, or
an application command script. Do not pass its whole envelope as a single card.

## Supported current commands

The baseline read-only API is:

```json
{"command":"material.properties.validate","params":{"card":"<individual card object>"}}
```

Replace the placeholder string with the actual JSON object. It validates
numerical/structural consistency and reports missing conditions and source review;
it does not assign, persist or authenticate the data.

The parallel personal runtime's persistence contract, inspected in its engine
source, is `material.create` / `material.edit` with an explicit `physical_card`:

```text
material.create {name, density_g_cm3, physical_card}
material.edit   {material, physical_card}
```

A host must first verify that its registered command specification supports
`physical_card`; older command implementations may ignore fields they do not know.
The generated `prepare` plan names this prerequisite and is not executed here.
It does not issue `material.assign`, guess body names or create geometry.

On the persistence implementation, **editing the physical card does not silently
replace the document's mass density**. A discrepancy between card density in kg/m3
and assigned density in g/cm3 must be shown and explicitly resolved. The creation
plan uses the same selected point density in both fields. A range, missing density
or unselected formulation cannot create a falsely precise material.

For an existing body, retain its current physical material until a user explicitly
selects a compatible reference profile. Changing a visual finish must retain
physical fields, mass, joint geometry, body dimensions and material axes. Conversely,
changing an engineering card must not recolor the body or replace its PBR maps.
All document mutations must use the engine command/undo transaction, not direct
JSON mutation or a Python runtime dependency.

## Projection policy

The current native schema permits density, isotropic/orthotropic elasticity,
scalar tensile/compressive/yield/shear strengths, thermal conductivity/expansion
and specific heat. This exporter deliberately does not manufacture missing
constants to fill those fields.

- An isotropic block requires a same-source compatible E and Poisson ratio. Where
  Poisson ratio is explicitly derived from the same source's E and G, the formula
  and derived status are retained and identified in the native condition text.
- A single directional Young modulus is not a complete orthotropic tensor. Wood
  flexural values and printed XY/Z coupon values stay directional in the extended
  data; no isotropic substitute is introduced.
- Lower/upper bounds and intervals do not become native nominal point values.
  They remain listed omissions. No midpoint or minimum-as-typical substitution.
- Directional thermal data is not copied into three axes. A repeated thermal
  diagonal is allowed only for a profile explicitly marked as an isotropic
  reference approximation, which the native state text discloses.
- Each native property retains its own temperature, moisture and strain-rate
  fields. Other source conditions, including an interval or test method, are
  retained in its state text. Unknown conditions are null, not assumed room values.
- Electrical data, hardness scales, radiative properties, porosity and transition
  temperatures remain in the richer profile. Typed runtime support for these
  quantities is separate work; preserve the extended data when designing it.

A structurally valid empty card is not a usable numerical material. The bundle
labels it `extended_data_only`; the coverage and native report separate those
eleven records from the 48 nonempty partial cards.

## Layers, compounds and analogues

`bindings.json` states whether a concept is a bulk body, layer, deposit, treated
surface, composite, textile, panel, granular material, device or assembly. Read
both `physical_role` and every reference's `relation` before offering assignment.

A constituent reference cannot be directly used as the effective mass density or
stiffness of a foam, laminate, populated PCB, honeycomb or multi-layer blanket.
Layer properties need thickness/coverage; homogenized properties need an explicit
construction and admissible model. Do not turn an unqualified comparison into a
default because it is the first reference in a list.

Lunar-like concepts use named terrestrial simulants only where stated. Bulk
packing density differs from grain density. Angle of repose is not substituted
for an internal friction angle, and terrestrial packing data does not certify
vacuum, gravity or thermal behavior on the Moon.

## Future library v2 export

The integrated runtime's `solidcraft_io::catalog` owns library serialization.
It computes entry revisions from its compact serde representation of provenance
and definition. Do not calculate an assumed equivalent Python hash or hand-write
a catalog revision: cross-language float spelling and omitted fields can differ.

The next runtime adapter should construct `ValidatedCard` through native admission,
use the runtime's catalog types, retain provenance and the extended-profile ID,
and generate the canonical revision through `Entry::new` / `content_revision`.
Tests must cover import, undo/redo, saved reopen, unchanged appearance, density
comparison and retention of unsupported extended fields. This slice performs
none of those document mutations and does not claim them tested.

## Validation boundaries

The supplied runner can invoke a trusted installed CLI only for read-only card
validation. The report identifies its binary hash and explicitly says that it
was not rebuilt by this gate. Numeric admission is not a source-license decision,
a physical experiment, a constitutive validation, a safety factor, a whole-app
build, or evidence of physics-driven rendering.


## Safe native library exporter

`tools/export_library.py --acknowledge-reference` prepares a non-executing plan
for the 46 point-density references and identifies 13 omitted profiles. Omission
is explicit: the application requires a scalar mass density, but these profiles
have no defensible scalar value. Full source profiles remain authoritative.

With `--cli` the helper checks the executable's registered command contract before
creating a new review folder or running a material mutation. It requires explicit
physical-card support, native inspection and library-v2 export. It uses a fresh
CLI process, no geometry and no existing user design. The actual application
serializes the v2 catalog and its revisions; Python does not approximate serde
canonicalization. Every card, source, condition and density is compared before
and after a separate-process cold reopen/re-export. The extended unsupported
properties remain in this package, not falsely marked as embedded in schema 1.

The helper is an offline authoring/integration tool, not an application dependency.
It never runs `material.assign` or changes a PBR appearance. Passing plan and
refusal tests does not imply that modern-runtime persistence was executed; only
a successful helper receipt establishes that separate result. Baseline read-only
native admission and complete library export are deliberately distinct gates.
