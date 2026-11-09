# Original surface-map starter pack

This pack contains **48 original surface recipes and 240 PNG maps**, generated at
**512 × 512 pixels** per map. Each recipe supplies base color, tangent-space normal,
roughness, metallic and 16-bit height data. The resources cover metals, coatings,
polymers, electronics, additive manufacturing, wood, composites and space-inspired
surfaces.

The maps have been authored as reproducible inputs. Their manifest status is
`authoring_status: authored`, `runtime_status: unbound` and
`appearance_fidelity: illustrative_approximation`. No material import, application
render, CPU/GPU scene comparison, measured-material match or photorealistic result
is claimed by this package. The broader [material catalog](../../../../docs/render-materials/materials.json)
remains a separate inventory of target concepts; these resources realize selected
concepts without changing their stable `MAT-*` identities.

## Files and authority

| File or directory | Purpose |
|---|---|
| [source/recipes.json](source/recipes.json) | Editable authority for the 48 numerical recipes, physical tile dimensions, seeds, concept links and limitations. |
| [manifest.json](manifest.json) | Generated resource contract, source/generator receipts and encoded/decoded map hashes. |
| `exports/SURF-*/` | Five required PNG authoring exports per resource. |
| [../tools/materials.py](../tools/materials.py) | Version 1.0.0 offline generator, validator and review-sheet command. |
| [../tools/test_materials.py](../tools/test_materials.py) | Analytic numerical checks and hostile-input admission tests. |

Edit the recipes and regenerate the exports together. Do not repaint a generated
PNG and retain its old receipt. A change to the generator, source values, mapping
or fidelity requires new validation and updated receipts. The tools use Python
3.11 or later, NumPy and Pillow for local authoring; they add no Python dependency
to the Rust application.

## Map semantics and renderer binding

The four 8-bit roles are bounded texture inputs intended for a future explicit
renderer adapter. Their manifest consumer is
`bounded_texture_input_compatible_not_runtime_bound`. Their file shape and color
space do not establish that a particular SolidCraft build imports or applies them.
The 16-bit height role is **authoring-only** until an adapter explicitly supports
its physical displacement contract; its consumer is
`authoring_only_requires_explicit_height_adapter`.

| PNG filename | Storage | Interpretation |
|---|---|---|
| `base_color.png` | RGB, 8 bits per channel | sRGB-encoded color. Decode the sRGB transfer exactly once before linear shading. Values are authored reflectance/F0 approximations for the chosen metallic workflow. |
| `normal_open_gl.png` | RGB, 8 bits per channel | Linear numerical data. Decode each channel as `2 × sample − 1`; use the tangent convention below and renormalize after filtering. |
| `roughness.png` | Grayscale, 8 bits | Linear numerical roughness, not glossiness. Values are already the authored roughness control; no sRGB transfer or pre-squared microfacet parameter is stored. |
| `metallic.png` | Grayscale, 8 bits | Linear numerical metallic fraction. The current recipes use opaque metal or dielectric approximations; this channel does not identify an alloy or composition. |
| `height.png` | Grayscale, unsigned 16 bits | Linear numerical height. Convert to millimetres with the recorded zero and full normalized span. No automatic displacement is enabled. |

There is no packed ORM texture, opacity map or inferred ambient-occlusion map.
**Ambient occlusion is the constant 1.0**: no invented cavity darkening or baked
illumination is added. Emission, transmission, coating, anisotropy and volume
parameters are not supplied by these five roles. A consumer must report missing
support explicitly rather than treating these maps as a complete implementation
of the linked material concept.

Normal and height data describe the same local surface relief. An adapter must
choose how that relief enters its surface model; applying both the complete
height displacement and the complete derived normal perturbation can count the
same detail twice. Height-based displacement also requires tessellation,
silhouette, bounds and performance validation that this authoring package does
not perform.

### Color and numerical normalization

The source specifies low and high **linear** RGB values. The generator interpolates
those values using its original periodic tone field, applies the sRGB transfer,
then quantizes to 8 bits for base color. It never applies that transfer to normal,
roughness, metallic or height channels. The test suite checks the known 18%
linear-gray reference: approximately 46.14% sRGB, giving byte value 118. The same
0.18 numerical roughness or metallic value gives byte value 46.

Height is quantized to unsigned 16 bits before the normal field is calculated.
For stored height sample `q` and the resource's `height_span_mm`:

```text
normalized_height = q / 65535
height_mm = (normalized_height - 0.5) * height_span_mm
```

The nominal representable interval is therefore
`[-height_span_mm / 2, +height_span_mm / 2]`. A recipe need not reach either end.
Do not stretch each image's observed minimum and maximum to fill the interval;
that would change the authored physical amplitude. The normal is the normalized
vector `(-dh/dU_mm, -dh/dV_mm, 1)`, using wrapped central differences of that
quantized height. Consequently, the exported height and normal share the same
scale and relief authority.

### Coordinates, scale and repetition

Each whole UV repeat spans `tile_mm = [width, height]`. These are local physical
surface dimensions in millimetres; maintain them when a CAD body is resized.
SolidCraft's model frame remains Z-up. UV directions must be bound to a reviewed
surface frame rather than inferred from the world axes or camera.

The normal convention is **OpenGL +Y, +U right, +V image-up**. Image rows increase
downward, so the generator samples row `i`, column `j` of an `N × N` image at:

```text
u = (j + 0.5) / N
v = 1 - (i + 0.5) / N
```

Both axes use repeat sampling. Pixel centers exclude duplicated boundary samples:
the first and last pixel columns need not be equal, because they are distinct
adjacent samples across a periodic seam. The validator instead compares the
underlying fields at the exact repeat boundaries. It requires a maximum field
residual no greater than `1e-10` and checks the recorded residual against the
recomputed value. Wrapped differentiation preserves relief at those boundaries.
Mip generation, texture filtering and CAD surface-frame continuity still need
validation in the consuming renderer.

## Admission bounds

The generator rejects unknown recipe fields, duplicate IDs or JSON keys, invalid
concept links, nonobject recipes, nonfinite values and booleans in numeric fields.
Names, domains and fidelity limitations must contain nonempty strings.

| Input | Admitted range or rule |
|---|---|
| Source JSON size | At most 4,000,000 bytes. |
| Resource count | 1–64; this source contains 48. |
| Resource ID | Unique `SURF-` followed by exactly three digits; no paths. |
| Square map resolution | 256, 512, 1024 or 2048; this source uses 512. |
| Seed | Integer from 0 through `2^32 − 1`. |
| Each tile dimension | 0.001–10,000 mm. |
| Height span | 0 through one eighth of the smaller tile dimension. |
| Each authored linear color channel | 0–1. |
| Roughness, roughness variation, metallic | 0–1; the resulting roughness field is clamped to 0.02–1 before 8-bit quantization. |
| Each repeat-cycle count | Integer from 1 through `resolution / 8`. Plain-weave counts must be even; 2/2-twill counts must be multiples of four. |
| Encoded PNG size during admission | At most 16 MiB per map, with exact resolution, channel type and receipt checks. |

The manifest validator checks exact map paths, dimensions, channel counts,
bit depths, encodings, pixel extrema and consumer declarations. It rejects
unexpected ICC, EXIF or gamma metadata; verifies encoded-file and decoded-pixel
hashes; checks positive-Z, near-unit normals; and recomputes normal/height
agreement. Changing the availability, AO, repeat, height-zero or normal-convention
claims cannot silently pass admission.

## Fidelity and engineering independence

All colors, relief, roughness and pattern choices are original illustrative
authoring values. They are not optical measurements, manufacturing tolerances,
certified color matches or material-selection advice. Applying an appearance must
leave density, stiffness, friction, thermal properties, electrical properties,
CAD dimensions, joints and simulation state unchanged.

Brushed and machined resources encode directional relief, while true anisotropic
reflection remains a renderer task. Anodizing, lacquer, resin-facing composites
and thermal coatings are explicitly simplified opaque responses; they do not
reconstruct independent optical layers. The 2/2 twill has an original four-cell
diagonal crossing pattern, whereas plain weave alternates at each crossing. Both
are closed relief patches without individual fibers, openings or fiber scattering.

Wood, cork and regolith are deliberately **procedural approximations**. The wood
patterns are generic grain fields, cork uses original periodic cells, and regolith
uses generic relief and color variation. These resources contain no photograph,
facility scan, geological sample data or measured lunar scattering. They do not
establish species identification, composition, location accuracy or photorealism.
The lunar-inspired surfaces remain ground in vacuum, with no airborne dust or
atmospheric haze.

FDM resources encode local bead relief, not the original slicer path, support
scars, wall outline or printing defects. The aluminium-blanket resource is a local
wrinkle field, not a multilayer blanket assembly. None of the maps creates real
openings, object silhouettes, deep damage or complete manufacturing geometry.
Every resource retains its specific limitations in both the source and manifest.

## Resource index

| Resource | Name | Catalog concept |
|---|---|---|
| SURF-001 | Brushed stainless steel | MAT-002 |
| SURF-002 | Cross-satin stainless steel | MAT-003 |
| SURF-003 | Shot-blasted steel | MAT-004 |
| SURF-004 | Galvanized steel spangle | MAT-006 |
| SURF-005 | Face-milled aluminium | MAT-011 |
| SURF-006 | Turned aluminium tool marks | MAT-012 |
| SURF-007 | Bead-blasted aluminium | MAT-014 |
| SURF-008 | Brushed titanium appearance | MAT-019 |
| SURF-009 | Brushed copper appearance | MAT-022 |
| SURF-010 | Machined brass appearance | MAT-024 |
| SURF-011 | Satin nickel appearance | MAT-027 |
| SURF-012 | Solidified solder finish | MAT-165 |
| SURF-013 | Matte white industrial paint | MAT-031 |
| SURF-014 | Black textured powder coat | MAT-032 |
| SURF-015 | Satin red industrial paint | MAT-033 |
| SURF-016 | Grey hammer-finish enamel | MAT-037 |
| SURF-017 | Red orange-peel lacquer surrogate | MAT-204 |
| SURF-018 | Black anodized opaque surrogate | MAT-016 |
| SURF-019 | Fine black ABS housing | MAT-042 |
| SURF-020 | Ivory textured ABS housing | MAT-042 |
| SURF-021 | Machined white POM appearance | MAT-046 |
| SURF-022 | Machined tan PEEK appearance | MAT-047 |
| SURF-023 | Blue silicone jacket surface | MAT-333 |
| SURF-024 | Charcoal technical rubber | MAT-061 |
| SURF-025 | Diamond-pattern TPU grip | MAT-065 |
| SURF-026 | Agglomerated cork surface | MAT-146 |
| SURF-027 | Oiled oak grain approximation | MAT-111 |
| SURF-028 | Walnut grain coat surrogate | MAT-113 |
| SURF-029 | Sanded beech grain | MAT-114 |
| SURF-030 | Natural birch grain | MAT-117 |
| SURF-031 | Birch plywood exposed edge | MAT-121 |
| SURF-032 | Uncoated MDF cut surface | MAT-123 |
| SURF-033 | Carbon 2/2 twill opaque surrogate | MAT-151 |
| SURF-034 | Matte carbon plain weave | MAT-152 |
| SURF-035 | Woven aramid resin surrogate | MAT-157 |
| SURF-036 | Green PCB solder-mask finish | MAT-161 |
| SURF-037 | Blue PCB solder-mask finish | MAT-161 |
| SURF-038 | Epoxy IC package surface | MAT-166 |
| SURF-039 | Opaque black electronics potting | MAT-169 |
| SURF-040 | Orange PLA FDM sidewall | MAT-171 |
| SURF-041 | Grey monotonic FDM top skin | MAT-361 |
| SURF-042 | Black ABS FDM sidewall | MAT-173 |
| SURF-043 | Blue TPU FDM sidewall | MAT-174 |
| SURF-044 | White powder polyamide surface | MAT-175 |
| SURF-045 | White thermal coating surrogate | MAT-491 |
| SURF-046 | Wrinkled aluminium blanket face | MAT-501 |
| SURF-047 | Mare-style regolith surface | MAT-531 |
| SURF-048 | Highland-style regolith surface | MAT-546 |

## Rebuild, validate and inspect

Run from the repository root, in an environment with the authoring dependencies:

```sh
python3 -B assets/render/starter/tools/materials.py build
python3 -B assets/render/starter/tools/materials.py validate --reproduce
python3 -B -m unittest discover -s assets/render/starter/tools -p test_materials.py -v
python3 -B assets/render/starter/tools/materials.py preview
```

`build` regenerates all 240 maps and the manifest from the source authority.
`validate --reproduce` admits the stored exports, recomputes every map and compares
decoded pixels exactly. Its success result retains `runtime_ready: false`.

The manifest records SHA-256 receipts for the exact recipe file, generator source,
each encoded PNG and each decoded pixel array. Decoded arrays use little-endian
byte order for hashing. PNG compression can differ between toolchain versions;
an encoded receipt identifies the stored file, while the independent decoded
receipt identifies its pixel content. If regeneration differs, inspect the source
and toolchain change rather than replacing receipts to conceal a discrepancy.

The focused suite completed **21 passing tests** during this authoring iteration.
It checks analytic sinusoidal normal direction and magnitude, physical scale over
three resolutions, sRGB reference values, periodicity of all pattern kinds,
2/2-twill versus plain-weave crossings, 16-bit round-trip, analytic height/normal
coherence and hostile source/manifest/file admission. These are numerical and
resource checks, not application renders. They do not establish material-library
integration, CAD mapping quality, CPU/GPU agreement or runtime availability.

`preview` writes an **unlit base-color** contact sheet to
`target/render-starter-review/materials/base-color-contact-sheet.png`. It is a
review artifact, not a shaded material thumbnail or renderer result. Keep that
sheet, other inspection images and temporary reports under ignored `target/`;
do not add them to the production resource collection.

Before admitting these resources as application presets, bind their roles and
physical mapping to a pinned renderer contract, verify unchanged engineering
state, exercise unsupported-feature handling, and render flat, curved and edge
targets with appropriate sampling. Record the actual executor revision and
numeric/visual results. The authored source hashes cannot substitute for that
runtime evidence.

## Clean-room provenance and license

The recipes, periodic fields, tools and exports are original SolidCraft
contributor work under **MIT OR Apache-2.0**. No external texture, photograph,
material library, commercial CAD output or third-party artwork was used as an
input. The generator uses original numerical constructions and deterministic
seeds; it does not infer measured properties from generated images. No GPL, LGPL
or AGPL implementation was read, copied or used as an implementation reference.

Every non-Markdown resource and tool is accounted for by an exact entry in the
root [ATTRIBUTION.md](../../../../ATTRIBUTION.md). The wider resource policy and
future admission requirements are in the
[clean-room record](../../../../docs/render-materials/CLEAN_ROOM.md) and
[implementation handoff](../../../../docs/render-materials/IMPLEMENTATION_HANDOFF.md).
