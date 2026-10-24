# SELENE original appearance pack

Forty-eight original appearance recipes are prepared for the current opaque PBR command path. Every recipe has a directly copyable, model-independent baseline and a separate advanced target specification. The pack contains no measured material dataset, external artwork, engineering material properties or runtime implementation.

## Files and status

| File | Purpose |
| --- | --- |
| `appearance-pack.json` | Editable authority for 48 recipes, their opaque projections and future target requirements. This is authoring data, not a native portable library file. |
| `appearance-swatches.json` | Prepared command script with 48 labeled box swatches and one neutral support base. It uses the existing `file.new`, `solid.box` and `appearance.assign` command shapes. |
| `appearance-pack.md` | Contract, use, limitations and recipe index. |

The authoring step checked JSON structure, ID/name uniqueness, catalog and capability references, numeric admission bounds, exact runtime parameter keys, and every swatch assignment against the recipe authority. **No SolidCraft runtime was executed and no rendered image was visually reviewed during authoring.** The script is a prepared input whose execution and render admission must be verified on the selected build.

## Runtime projection contract

Copy `items[].baseline_projection.runtime_parameters`, then add an existing target selection before calling `appearance.assign`. The projection includes `appearance`, `color`, `opacity: 1` and exactly three PBR fields: `metallic`, `roughness` and `dielectric_ior`. Targets are deliberately absent because the improved SELENE source model must be pinned and inspected before binding.

```json
{
  "command": "appearance.assign",
  "params": {
    "bodies": [
      "Existing target body"
    ],
    "appearance": "SELENE-AP-001 Neutral matte enclosure",
    "color": "#717882",
    "opacity": 1.0,
    "pbr": {
      "metallic": 0.0,
      "roughness": 0.68,
      "dielectric_ior": 1.5
    }
  }
}
```

The body name in this example is a placeholder, not a ready-to-run model binding. The current command also accepts face points and components. An occurrence name supplied as a component target resolves to its component definition; do not assume an independent occurrence override. Face overrides take precedence over broader appearances. Assigning a look replaces it: omitting `pbr` later restores the scalar response instead of retaining these fields.

Current bounds are metallic 0 to 1, perceptual roughness 0.045 to 1, relative dielectric IOR 1 to 3, and opacity exactly 1 for PBR. Colors are sRGB8 hex values decoded by the existing renderer. All values here are original illustrative authoring choices. The common IOR control of 1.5 is not a measured value; it controls reflection of the nonmetal fraction and does not enable transmission. The metal recipes use metallic 1 and the dielectric recipes use metallic 0; no mechanical or electrical properties follow from either choice.

The pack JSON is **not** `solidcraft-library/1` and cannot be passed directly to `library.inspect` or `library.apply`. That scalar library format cannot preserve PBR. Use the prepared script, an explicit appearance overlay, or a native format-21 design. A future catalog importer needs its own versioned admission contract.

## Prepared swatch script

The 48 swatches are 12 x 12 x 8 mm boxes in 8 columns and 6 rows with an 18 mm pitch. Their lower corners run from x = -69 mm to 57 mm and y = -51 mm to 39 mm, starting at z = 1 mm. A 144 x 108 x 1 mm base sits below them. IDs ascend along x within each row, then along y. Body names carry the recipe ID and English name; there is no text geometry, font, decal or external asset.

The script contains 99 commands: one new design, 49 boxes, and 49 appearance assignments. Flat boxes intentionally bound tessellation demand; they are an inspection grid, not a complete optical validation target. A curved reference, an angular light sweep, and close-up geometry remain necessary for validating highlights, anisotropy and layered targets when those functions exist.

Prepared commands for a built native CLI, run from the repository root:

```sh
mkdir -p target/selene-render
solidcraft-cli run assets/render/selene/materials/appearance-swatches.json --save target/selene-render/appearance-swatches.solidcraft --results --json-out target/selene-render/appearance-swatches-results.json
solidcraft-cli render target/selene-render/appearance-swatches.solidcraft --out target/selene-render/appearance-swatches.png --width 768 --height 576
```

These commands have been prepared, not executed. The render CLI automatically fits a camera when none is supplied. Existing output is protected unless `--overwrite` is explicitly provided. Keep local render evidence under ignored `target/`; this pack does not bundle unreviewed preview exports.

To style a pinned SELENE design, prepare a separate overlay containing only `appearance.assign` commands with verified target names, then run:

```sh
solidcraft-cli run target/selene-render/verified-appearance-overlay.json --in path/to/pinned-selene.solidcraft --save target/selene-render/selene-styled.solidcraft --results
```

Both input paths above are placeholders until the model and overlay are prepared. The overlay must contain no `file.new`, geometry edits, physical-material edits, joint edits or pose changes. Native save is the preservation path. `timeline.export_script` currently omits appearance overrides and refuses component/joint assemblies; it is not an appearance or articulated-model round-trip mechanism. A multi-command script stops at the first error and is not a whole-script transaction, so verify targets before applying it to the review copy.

## What the baseline can show

The current Render consumer supports opaque direct-lit responses under three fixed neutral directional lights, with the existing preview tone mapping and PNG export. It does not implement the textured, layered, transmissive or emissive targets described here. Current consumers have no cast shadows, environment illumination, indirect bounces, surface coating, anisotropy, absorption or transmission. Saved nondefault environment/exposure intent is not executable by this baseline. These limits are documented in [`docs/render-opaque-pbr.md`](../../../../docs/render-opaque-pbr.md) and [`docs/render-jobs.md`](../../../../docs/render-jobs.md).

Advanced targets remain separate authoring data. Their numeric controls are illustrative texture or geometry settings in millimetres, radians or explicitly named dimensionless units. They are not manufacturing recommendations or measurements. Optical spectra, refractive data, layer thicknesses and scattering coefficients remain unspecified where the recipe lists missing inputs. CAP references link to requirements in [`capabilities.json`](../../../../docs/render-materials/capabilities.json), not to a claim of implementation.

## SELENE-specific fidelity boundaries

- The sample vial, optical window and diffuser recipes are explicitly opaque inspection surrogates (AP-029, AP-030 and AP-039). They do not show glass, refraction or light transmission. Transparent containers need closed wall geometry and correct interior/exterior medium handling before the advanced target can be represented.
- The unpowered LED face (AP-038) emits no light. Active LEDs require a separate emission model; spectrally excited phosphor conversion additionally requires excitation, conversion and emission data. Ordinary RGB emission must not be labelled fluorescence.
- The black and silver anodized recipes (AP-015 and AP-016) are simplified outer-response surrogates. Actual anodizing combines a dielectric or absorbing oxide with a metal substrate; a single metalness value is not that layer model.
- Blanket folds (AP-018 and AP-019), honeycomb walls (AP-040), mesh-wheel openings (AP-044), serrations and threads require geometry. Color or roughness alone cannot provide their silhouettes, empty space or self-shadowing.
- Lunar terrain and deposits (AP-041 to AP-043) are granular surfaces in vacuum. The dust-patch baseline belongs on separate deposit geometry or a deliberately bounded face. Applying it to an entire rover body does not reproduce sparse dust coverage. No terrestrial fog, wetness or aerial perspective is implied.
- Appearance assignments do not define density, stiffness, friction, thermal emissivity, electrical performance, leak tightness, cleanliness, ESD behavior or flight readiness. They must leave the improved model geometry, articulation, joint limits and motion data intact.
- The automotive rubber sidewall (AP-048) is a representative terrestrial material target. Do not substitute it for the SELENE open metal wheel (AP-044) merely because both are wheel-related.

## Recipe index

All entries use opacity 1 and the illustrative IOR control 1.5. M is metallic fraction; R is perceptual roughness. The catalog link is semantic traceability to the 600-recipe planning catalog, not an imported texture or measured asset.

| ID | Original recipe | Catalog source | sRGB color | M | R | Baseline role |
| --- | --- | --- | --- | ---: | ---: | --- |
| SELENE-AP-001 | Neutral matte enclosure | MAT-031 | `#717882` | 0 | 0.68 | opaque surface |
| SELENE-AP-002 | White thermal-coating preview | MAT-491 | `#d9dde0` | 0 | 0.58 | opaque surface |
| SELENE-AP-003 | Matte optical baffle | MAT-468 | `#141619` | 0 | 0.94 | opaque surface |
| SELENE-AP-004 | Ceramic gripper contact pad | MAT-574 | `#d9d7cf` | 0 | 0.32 | opaque surface |
| SELENE-AP-005 | Machined aluminium wheel rim | MAT-561 | `#b9c0c7` | 1 | 0.29 | opaque surface |
| SELENE-AP-006 | Serrated metal gripper jaw | MAT-573 | `#a6adb5` | 1 | 0.40 | opaque surface |
| SELENE-AP-007 | Linear-brushed stainless housing | MAT-002 | `#b8bdc2` | 1 | 0.34 | opaque surface |
| SELENE-AP-008 | Polished stainless sample cup | MAT-593 | `#c9ced3` | 1 | 0.16 | opaque surface |
| SELENE-AP-009 | Titanium wrist finish | MAT-572 | `#8b9098` | 1 | 0.43 | opaque surface |
| SELENE-AP-010 | Exposed copper conductor | MAT-164 | `#b97850` | 1 | 0.27 | opaque surface |
| SELENE-AP-011 | Machined brass printer nozzle | MAT-391 | `#b4a06b` | 1 | 0.30 | opaque surface |
| SELENE-AP-012 | Solidified solder joint | MAT-165 | `#bec3c6` | 1 | 0.25 | opaque surface |
| SELENE-AP-013 | Metal sampling scoop | MAT-575 | `#9ba4ad` | 1 | 0.44 | opaque surface |
| SELENE-AP-014 | Gold-finished contact pad | MAT-163 | `#bf9c49` | 1 | 0.21 | opaque surface |
| SELENE-AP-015 | Black anodized chassis surrogate | MAT-567 | `#252a30` | 0 | 0.56 | opaque surface |
| SELENE-AP-016 | Silver anodized cover surrogate | MAT-015 | `#abb3ba` | 1 | 0.39 | opaque surface |
| SELENE-AP-017 | Composite arm tube surrogate | MAT-571 | `#252a2e` | 0 | 0.42 | opaque surface |
| SELENE-AP-018 | Aluminized blanket metal face | MAT-501 | `#c3c8cd` | 1 | 0.18 | opaque surface |
| SELENE-AP-019 | Polyimide-facing blanket surrogate | MAT-502 | `#80602e` | 0 | 0.30 | opaque surface |
| SELENE-AP-020 | Solar-cell opaque preview | MAT-511 | `#172b43` | 0 | 0.24 | opaque surface |
| SELENE-AP-021 | Silicone cable jacket | MAT-333 | `#484d56` | 0 | 0.77 | opaque surface |
| SELENE-AP-022 | Polyimide harness wrap | MAT-332 | `#9d713d` | 0 | 0.46 | opaque surface |
| SELENE-AP-023 | Fluoropolymer sealing ring | MAT-598 | `#dbddd7` | 0 | 0.49 | opaque surface |
| SELENE-AP-024 | Elastomer seal candidate | MAT-599 | `#26292b` | 0 | 0.86 | opaque surface |
| SELENE-AP-025 | Opaque sample cassette body | MAT-595 | `#424a54` | 0 | 0.54 | opaque surface |
| SELENE-AP-026 | Textured ABS instrument shell | MAT-042 | `#59616b` | 0 | 0.64 | opaque surface |
| SELENE-AP-027 | Powder-printed polyamide preview | MAT-175 | `#abb0b1` | 0 | 0.87 | opaque surface |
| SELENE-AP-028 | FDM top-skin preview | MAT-171, MAT-361 | `#aab5bd` | 0 | 0.46 | opaque surface |
| SELENE-AP-029 | Sample vial opaque inspection surrogate | MAT-582 | `#91a2ae` | 0 | 0.35 | opaque inspection surrogate |
| SELENE-AP-030 | Window opaque inspection surrogate | MAT-583 | `#537286` | 0 | 0.24 | opaque inspection surrogate |
| SELENE-AP-031 | Matte ceramic sample insert | MAT-594 | `#c9c5bb` | 0 | 0.59 | opaque surface |
| SELENE-AP-032 | Threaded metal closure cap | MAT-596 | `#b6bcc3` | 1 | 0.31 | opaque surface |
| SELENE-AP-033 | Graphite thermal-sheet preview | MAT-349 | `#35383d` | 0 | 0.91 | opaque surface |
| SELENE-AP-034 | Solder-mask board face | MAT-161 | `#32624b` | 0 | 0.42 | opaque surface |
| SELENE-AP-035 | Exposed glass-epoxy board edge | MAT-162 | `#9b9270` | 0 | 0.77 | opaque surface |
| SELENE-AP-036 | Molded epoxy component body | MAT-166 | `#252727` | 0 | 0.80 | opaque surface |
| SELENE-AP-037 | Battery-can metal preview | MAT-341 | `#b3bbc3` | 1 | 0.35 | opaque surface |
| SELENE-AP-038 | Unpowered LED phosphor face | MAT-441 | `#d6bd64` | 0 | 0.65 | opaque surface |
| SELENE-AP-039 | Diffuser opaque inspection surrogate | MAT-451 | `#dadcdb` | 0 | 0.72 | opaque inspection surrogate |
| SELENE-AP-040 | Aluminium honeycomb cut edge | MAT-481 | `#a5adb6` | 1 | 0.40 | opaque surface |
| SELENE-AP-041 | Mare-style regolith surface | MAT-531 | `#5e5c58` | 0 | 0.96 | opaque surface |
| SELENE-AP-042 | Highland-style regolith surface | MAT-546 | `#8b8881` | 0 | 0.96 | opaque surface |
| SELENE-AP-043 | Tool-transferred dust patch | MAT-559 | `#7b766d` | 0 | 0.98 | opaque deposit geometry response |
| SELENE-AP-044 | Woven-metal wheel mesh | MAT-565 | `#999fa7` | 1 | 0.38 | opaque surface |
| SELENE-AP-045 | Oiled oak workshop surface | MAT-111 | `#a17a51` | 0 | 0.56 | opaque surface |
| SELENE-AP-046 | Varnished walnut joinery | MAT-113 | `#6d4d37` | 0 | 0.30 | opaque surface |
| SELENE-AP-047 | Automotive smooth clearcoat surrogate | MAT-034, MAT-204 | `#4e697e` | 0 | 0.21 | opaque surface |
| SELENE-AP-048 | Embossed tyre-sidewall preview | MAT-237 | `#282a2c` | 0 | 0.83 | opaque surface |

## Source and contract references

- [`appearance.assign` implementation](../../../../crates/engine/src/cmd/inspect.rs): named custom colors, body/face/component targeting and exact PBR block admission.
- [Persistable PBR parameters](../../../../crates/doc/src/pbr_appearance.rs): appearance-owned optical values and sRGB decoding.
- [Opaque product surface contract](../../../../docs/render-opaque-pbr.md): current consumers, limits, native preservation and unsupported functions.
- [Original opaque studio example](../../../../examples/render-opaque-studio.json): existing `solid.box` and appearance command shapes used for this prepared script.
- [CLI render workflow](../../../../apps/solidcraft-cli/src/render.rs) and [product-image jobs](../../../../docs/render-jobs.md): bounded PNG admission, preflight/start/wait/export and overwrite behavior.
- [Script executor](../../../../crates/engine/src/lib.rs) and [timeline export](../../../../crates/engine/src/cmd/script.rs): replay semantics and export omissions.
- [Scalar catalog contract](../../../../crates/io/src/catalog.rs): version-1 scalar-only fields and PBR export refusal.

## Original provenance

Author: SolidCraft contributors. License: MIT OR Apache-2.0. The two JSON files are original clean-room resource data and analytic command inputs. No commercial material library, copied shader, third-party artwork, restricted CAD extraction or generated image texture is bundled. Their exact attribution rows belong in the repository root `ATTRIBUTION.md`; Markdown documentation is exempt from the non-Markdown asset gate.
