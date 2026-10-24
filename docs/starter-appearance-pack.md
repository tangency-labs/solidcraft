# Starter appearance pack

The pack contains 40 original illustrative visual recipes: eight woods, ten metals,
ten plastics, six glasses, two solar surfaces and four regolith surfaces. The 34
opaque presets use the existing opaque PBR appearance model. The six glass recipes
are unavailable through ordinary appearance assignment. The Render workspace
provides an explicit GPU runtime consumer with geometry certification; their
stable IDs still cannot be applied through a color or alpha override.

These choices describe appearance, not measured material grades. Applying one does
not set density, mass, engineering properties or certification. The original 20
scalar library entries retain their existing values and behavior.
The six opaque finishes from #1366 also retain their original names, values and
name-based lookup without starter IDs. The combined provider has 66 entries:
20 legacy scalar appearances, six existing opaque finishes and 40 starter recipes.
Of these, 60 are available through ordinary appearance assignment: 58 opaque
appearances and the two legacy scalar-alpha glass appearances (Clear 0.25,
Smoked 0.45). The six starter glass recipes remain disabled in that provider.
Their explicit GPU runtime use does not change this count. Legacy alpha is not
physical glass or refraction.
These counts describe the authored source contract; combined runtime acceptance
and independent review remain pending.

## Commands and saved designs

`appearance.library` lists stable IDs, display names, categories, revision,
provenance, requirements and availability. Use the stable `starter.*` ID as the
`appearance` of an appearance assignment through the existing appearance commands.
Opaque assignment creates an ordinary owned appearance containing color, metal
fraction, roughness and dielectric IOR. Native saves retain those values; they do
not save a live link to the pack ID, its revision or its provenance metadata.

A display name without an explicit color also resolves its recipe. A display
name together with an explicit color keeps the existing custom-look behavior:
the supplied values are authoritative and do not claim that a recipe was
applied. Stable IDs explicitly request the recipe, so the six unsupported glass
IDs refuse before color, opacity or PBR overrides. For opaque recipes, omitting
`pbr` preserves their response, an object replaces it, and explicit `pbr: null`
selects scalar shading. The central final-opacity validation is unchanged.

The existing Appearance editor searches by name or ID, groups swatches by
category, and can hide unavailable recipes. Glass recipe swatches are disabled
and explain their missing requirements. Choosing an opaque swatch copies its
values; subsequent manual edits detach the transient recipe ID. Apply and OK
share the existing undo transaction behavior.

The [portable catalog v2](book/src/dev/local-libraries.md) preserves owned opaque
PBR looks and engineering cards, including supplied sources and conditions. V1
remains readable; an explicit v1 export refuses data it cannot represent. Catalog
copies do not create a live binding to a recipe ID, and neither catalog version
transports runtime glass, coat or image bindings. Applying a visual recipe never
changes an assigned physical material, its engineering card, provenance, density
or mass.

## Uniform previews and future textures

Wood, solar and regolith presets are explicitly named uniform previews. They do
not render wood grain, solar cells or regolith detail. Their optional future
texture recipes describe intended physical repeat in millimetres and wood grain
along V; they are not loaded resources or saved texture bindings. The intended
map conventions are sRGB albedo, linear roughness and linear positive-Y tangent
normals. No generated maps or texture acceptance is implied by a recipe.

## Glass recipes

Each glass recipe specifies dielectric IOR, linear RGB transmittance at
an explicit positive reference path in millimetres, and either a thin boundary
with thickness or a closed boundary. The displayed swatch encodes transmission
for the picker only; it is not rendered glass.

The Render workspace's GPU material editor converts transmittance to absorption
per millimetre using `sigma = -ln(T) / reference_path_mm` and creates an explicit
whole-body runtime binding. Preparation certifies the selected thin sheet or
closed solid; a closed recipe alone does not certify an arbitrary mesh. These
bindings are not saved by the appearance commands or catalog v2. The CPU
dielectric API has its own thin-sheet/axis-aligned-box contract and does not
automatically consume this recipe selection. See [Render workspace](render-workspace.md)
and [transmission](render-transmission.md) for supported combinations. An opaque
PBR dielectric IOR only controls opaque reflectance; it does not turn an opaque
look into refractive glass. The recipe data does not establish rough-glass or
measured optical accuracy.

## Licensing and validation

All recipe values are original authoring by SolidCraft contributors under
MIT OR Apache-2.0, recorded in `ATTRIBUTION.md`. They contain no external material
library or measured optical dataset.

Source tests cover the combined provider counts, exact legacy and published finish values,
unique starter IDs, opaque save/reopen and undo behavior,
unchanged physical properties, and refusal of unavailable recipes. Compilation,
test execution, full CI and actual application screenshots remain required before
the implementation is ready for review; source checks alone do not qualify it.
