# Opaque product surfaces

This is the first integrated consumer slice of #1004, under #813/#620. It uses the
existing Render workspace, product geometry, appearance assignment and PNG job host.
There is no second renderer or external material dataset.

## Apply and preserve a response

```json
{"command":"appearance.assign","params":{"bodies":["Specimen"],"color":"#b47846","pbr":{"metallic":1,"roughness":0.3,"dielectric_ior":1.5}}}
```

The Appearance editor has an **Opaque PBR** checkbox and separate Metallic,
Roughness and Dielectric IOR fields. Enabling it explicitly sets opacity to one.
View the response in Render or export a product PNG; Design retains its simple
color preview. Apply/OK, swatch reuse and component/face targeting use the existing
assignment command. A later scalar face override replaces a body's PBR response
on that face; it does not inherit the optical response implicitly.

Parameters are finite and bounded: metallic [0,1], perceptual roughness [0.045,1],
relative dielectric IOR [1,3]. For PBR, unknown optical fields and
nonnumeric/out-of-range opacity refuse before changing the design. PBR requires
opacity exactly one.
IOR controls the nonmetal fraction's reflection; it does not enable refraction.
A library appearance selected by name keeps its complete preset response when
`pbr` is omitted. An explicit `color`, including a custom look named like a
library entry, starts with scalar shading unless a `pbr` block is supplied.
Supplying `pbr: null` explicitly selects scalar shading. Assignment replaces a
look; it is not a partial parameter patch.

Use `name` with an explicit `color` to copy an inert look label, including labels
that happen to equal a `starter.*` ID. `name` and `appearance` are mutually
exclusive. The Appearance editor uses `name` for owned looks and `appearance`
only for an explicitly selected recipe, so reopening or reusing a copied look
does not change its optical values. Explicit unavailable glass recipe requests
still refuse; an inert label never enables refraction.

`material.create/edit` accept the same optional `pbr` block alongside a default
appearance/color. A named PBR default also requires opacity exactly one, even
when the command omits `pbr`; invalid combinations refuse before creating or
editing the material. A density-only edit retains the entire existing look. Visual
response changes do not modify density, geometry, placement or engineering axes.
No grade-specific optical or mechanical values are inferred from a name or color.

Since format **21**, `pbr` is saved inside existing body/face/component and
material-default looks. Missing fields retain scalar shading. Format-20 render setups and old looks
upgrade without reinterpretation; older readers refuse newly saved files instead
of dropping their optical response. Undo/redo, copying and cold reopening use the
existing design-owned values, with no external file dependency.

The portable catalog v2 preserves PBR looks and physical-material defaults, alongside
validated engineering cards. Explicit scalar v1 export refuses any loss of those
records before writing. V1 imports and v2 looks without PBR explicitly select scalar
shading and clear a prior PBR response. See [local material libraries](book/src/dev/local-libraries.md).
In-design swatches retain and reuse the response.

## Inspect, replace and inherit a look

`appearance.chain` is a read-only command. Supply exactly one target: a body,
a face index from `model.faces` with its body, or a component name/ID:

```json
{"command":"appearance.chain","params":{"target":{"body":"Specimen","face":0}}}
{"command":"appearance.chain","params":{"target":{"component":"Housing"}}}
```

The result contains four `steps`, ordered `face`, `body`, `component`, `material`.
Each step contains `step` and `look`: either null or the full saved look
(`name`, hex `color`, `opacity`, and optional `pbr`). `active` names the first
nonempty step, or is null. Body reads have no face step. Component reads have
no face/body steps. Physical materials belong to bodies; a component's material
step shows the common material look of its directly owned bodies, or null for
mixed/empty components. Root is not an assignable component. Imported paint
remains a display fallback outside these four steps, so an all-null chain does
not imply a particular viewport color.

`appearance.replace` rewrites every equal body, face and component override in
one undo step. `from` accepts a library name or a complete look copied from
`appearance.list`'s `in_design` entries. Equality includes the label, color,
opacity and all PBR values, so looks with identical names but different responses
remain distinct. `to` accepts a library name or an object with the same look
fields as `appearance.assign` (including inert `name` with `color`).

```json
{"command":"appearance.replace","params":{"from":"ABS - Red","to":"Copper - Clean PBR"}}
{"command":"appearance.replace","params":{"from":{"name":"Custom","color":"#abcdef","opacity":0.5},"to":{"color":"#123456","opacity":0.4},"scope":{"bodies":["Specimen"]}}}
```

Omit `scope` to replace all stored overrides, including hidden or suppressed
ones. Optional `scope.bodies` selects body overrides and face overrides on those
bodies; `scope.components` selects only those component overrides. These lists
combine as a union. A component scope does not expand to its bodies or descendants.
Names and numeric component IDs are accepted; duplicate targets count once.
Scope targets must exist in the evaluated model/design. An empty target list
selects nothing. At most 10,000 scope entries and 100,000 stored overrides are
processed; larger requests/designs refuse. The chain read uses the same override
budget and at most 100,000 bodies for a component read.

The replacement result is `{"count": N}`, counting stored overrides rewritten.
Invalid values or targets refuse before mutation. A missing source look, empty
scope or identical replacement returns count zero without adding undo history or
clearing redo. Geometry, placements, density, mass, physical material defaults
and engineering records are unchanged. No command opens a dialog. Existing look
records support saving/reopening these edits without a format change.

**From Material** uses the existing clear form of `appearance.assign`:

```json
{"command":"appearance.assign","params":{"bodies":["Specimen"],"color":null}}
{"command":"appearance.assign","params":{"components":["Housing"]}}
{"command":"appearance.assign","params":{"body":"Specimen","faces":[[5,10,5]],"color":null}}
```

Clear removes only the selected override. A face then follows its body, a body
its component, and finally the physical material's default look. Clear remaining
higher-priority overrides when the material should be active. This changes the
look without changing density or mass.

## Original opaque finish presets

The existing Appearance library includes six additional opaque finishes. Its
20 scalar entries keep their names, colors, opacity and shading behavior. The
new names are distinct, and the Appearance editor uses its existing categories,
search, swatches and optical fields to select and edit them.

| Appearance | sRGB color | Metallic | Roughness |
|---|---|---:|---:|
| Steel - Satin PBR | `#9ea4ac` | 1 | 0.40 |
| Aluminum - Satin PBR | `#ccd0d6` | 1 | 0.30 |
| Copper - Clean PBR | `#cc7a50` | 1 | 0.25 |
| ABS - Molded PBR | `#deded8` | 0 | 0.40 |
| PLA - Smooth PBR | `#6ea0de` | 0 | 0.35 |
| Wood - Oak Uniform PBR Preview | `#be925c` | 0 | 0.65 |

Each preset has opacity 1 and an illustrative dielectric IOR of 1.5. The values
are original visual studies, not measured optical constants or engineering-grade
material properties. Oak is a uniform preview without grain; PLA has no printed
layers; satin metals use an isotropic response. A physical material's density and
engineering axes remain independent of these visual choices.

The source values live in `assets/appearances/opaque_finishes.rs`. Assigning a
preset saves its values inside the existing look, so a later library update does
not change a saved assignment. This addition does not change the design format.

`examples/material-finishes.json` creates six original radius-10 mm spheres,
assigns generic physical materials and applies these finishes through the same
commands used by the editor:

```sh
solidcraft-cli run examples/material-finishes.json --save target/material-finishes.sc
solidcraft-cli render target/material-finishes.sc --out target/material-finishes.png --width 1600 --height 1000
```

## Actual consumers and color

The installed integrated `gpu-viewport` and `cpu-product` image hosts advertise the
`pbr` feature. Requests require it when the captured product scene contains PBR.
The static legacy `cpu-raster` descriptor retains its previous capabilities.
No host gains HDRI, refraction, progressive/final rendering or EXR support.

Look colors are decoded from sRGB8 to linear sRGB once in the shared document/scene
adapter. On the GPU, a PBR triangle is a built triangle whose palette entry names its
material: the palette's material table holds the linear reflectance and optical
parameters, so a material edit rewrites table rows and no vertex. The CPU
rasterizer interpolates world position and normals with perspective correction;
the GPU uses the corresponding fragment interpolation. Both use the original
[opaque BRDF](render-surface.md) and three fixed neutral directional lights in Z-up
world coordinates. They sum BRDF × relative incident radiance × light cosine,
apply per-channel Reinhard preview tone mapping, then encode exactly once.
Linear and sRGB GPU attachments have distinct output transfer paths.

This fixed studio has no cast shadows, environment illumination, indirect bounces,
emission, anisotropy, coating, absorption or transmission. Lighting magnitudes are
relative preview values, not calibrated lux or spectral data. The single-scattering
model can darken rough metals. Transparent legacy looks retain approximate sorted
alpha blending, including when seen in front of an opaque PBR surface; that is not
glass optics. Nondefault exposure/environment controls remain explicitly unsupported.

Scene uploads keep the product preview's vertex bound (512 MiB on 64-bit desktop, 128 MiB
on the web and 32-bit targets); PBR vertices are the size of other built vertices.
CPU face-map admission accounts for retained optical descriptors. Existing immutable
capture, ownership, cancellation, stale-result rejection and PNG export policies
remain in effect. Invalid/nonfinite geometry or zero input normals refuse.

## Validation and original example

`examples/render-opaque-studio.json` builds four original spheres and a base,
varying metallic fraction and roughness independently while keeping base color.
Its values are an illustrative optical study, not engineering data for real grades.

The product offscreen test uses the actual installed callback/pipelines on Vulkan,
including both linear and sRGB attachments. For four metal/roughness combinations,
each comparison checks 12,454 interior pixels against CPU PNG output; observed mean
channel error is below 0.2/255, with no pixels beyond the 12/255 outlier threshold.
Silhouettes are excluded because CPU images use 2x supersampling and that test's GPU
target uses 1x. This is a scoped parity result, not browser or universal GPU certification.

Command tests check unchanged geometry/mass, atomic refusal, undo/redo, cold reopen,
physical default/face precedence, retained scalar format-20 looks upgraded to the current
format and explicit v1 catalog refusal. Preset regressions also cover named PBR
inheritance, `pbr: null`, custom looks with matching names, material create/edit
refusal without changing history, density-only edits and all six scene responses.
A clicked editor regression checks one transaction and reopening
the same selected body. Native media and exact current-head gate results belong
in the PR. wasm compilation and actual browser interaction are separate evidence.
