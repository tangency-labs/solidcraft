# Inventor import without Corliss

Inventor parts (`.ipt`) and assemblies (`.iam`) use the Apache-2.0 cadmpeg codec.
The integration contains no Corliss dependency or vendored Corliss implementation.
The translation into SolidCraft document features is original MIT OR Apache-2.0
project code. Third-party dependencies retain their licenses and notices; the
cadmpeg packages declare Apache-2.0, while their resolved dependencies include
Zlib, BSD data and Unicode licenses also present in the baseline application.
This is a copyleft-free integration, not a claim that every transitive license
is only MIT or Apache-2.0. The attribution row is recorded in
`attribution/pending/inventor-cadmpeg-history.md` for folding on merge.

## Editable part history

Opening a part attempts to rebuild the fully decoded neutral history as native
SolidCraft features inside a component named after the file. It currently supports:

- Named scalar length/angle/real parameters with evaluable dependency expressions.
  Expressions are checked against their decoded canonical values (mm/radians).
- Resolved planar sketches with point, line and circle geometry.
- Active horizontal, vertical, parallel, perpendicular, equal and explicit-locus
  coincidence constraints; parameter-bound line length, radius and diameter dimensions.
- Solid extrusions with normal/reversed-normal directions, explicit normal-aligned
  vectors, one-sided/two-sided blind distances, symmetric total distances,
  through-all extents, and fixed plane offsets. Multi-body Boolean targeting is
  refused when the source targets cannot be mapped unambiguously.
- Feature order, names, component ownership, and known suppression state.

Extrusion extents are editable decoded values. The adapter does **not** guess
which source parameter drives an extent when the neutral feature does not carry
that binding; the import reply warns about this limitation. Sketch dimensions
whose neutral records explicitly reference parameters retain those bindings.

Unsupported operations (including revolves, sheet-metal features and blends),
unresolved relations, incomplete design graphs and expressions with different
evaluated meanings are refused as a whole. The normal geometry path is then
attempted with a warning, rather than presenting an incomplete timeline as the
original part. Regeneration must succeed before `doc.open` installs an editable
design. Explicit STEP placement/naming options select the geometry path.

## Geometry and assemblies

Insert and the geometry fallback convert the selected codec's admitted solids
to STEP, then use SolidCraft's existing Import feature. Assemblies remain limited
to the occurrence/geometry branches supported by cadmpeg; this is not a claim
of complete Inventor version or assembly support. Browser builds use the same
pure-Rust decoding/translation path.

## Validation and limits

The source decoder uses the existing service resource budgets and panic guard;
the document/sketch builders retain their own size and identity checks. Tests
in the existing native-CAD test module construct original neutral records, check
analytic volumes, change imported dimensions, and round-trip saved designs.
Hostile byte inputs must fail without an escaping panic.

These tests verify the translation and its admission rules. They do not yet
provide a positive synthetic `.ipt` byte-stream fixture exercising every stage
of cadmpeg's parser. No commercial CAD application or Corliss test corpus is used
to manufacture that missing evidence. Broader format validation and additional
history operations are required before claiming general-purpose compatibility.
