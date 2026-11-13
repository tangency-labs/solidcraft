# Prepared SELENE demonstration scenes

This directory contains **12 shot specifications, 10 stages and 33 semantic roles** for the improved SELENE model. They are editable authoring inputs with status `specified_unbound`. The native source is now pinned in [source-lock.json](../source-lock.json); the complete catalog's semantic roles and renderer resources still require explicit binding. None of these specifications substitutes old rover component or feature IDs for a reviewed selection.

The files prepare composition, lighting ownership, material intent and review gates. They do not install a renderer, create native geometry, simulate a catch or demonstrate a finished image.

## Files and authorities

| File | Authority |
|---|---|
| [shots.json](shots.json) | Shot composition, camera/framing recipes, output intent, comparisons and presentation/planning sequences. |
| [stages.json](stages.json) | Explicit stage overrides, Sun/ambient/background ownership, generated geometry roles and clearance rules. |
| [roles.json](roles.json) | Semantic selection targets, candidate material concepts, scope limits and native opaque-proxy eligibility. |
| [../source-lock.json](../source-lock.json) | Accepted source identity and admission review, owned by the model-preparation workflow. |

The existing [render/materials catalog](../../../../docs/render-materials/README.md) remains authoritative for `MAT`, `ENVSET`, `STU`, `Q` and `SCENE` concepts. Stage geometry resolves the referenced `environment-presets.json` record and then applies only its declared overrides. This directory does not maintain a second copy of every terrain or room parameter. Metadata records hashes of the catalog files used to author the recipes; update those locks deliberately after a reviewed catalog revision.

`reference_scene_id` records design lineage. It is **not** an instruction to apply an entire catalog scene and then apply this shot again. Effective composition comes from the shot's explicit quality, studio, stage, roles and source policies. Existing catalog and scene IDs remain unchanged.

## Source and native-body preparation

Run from the repository root, using the pinned improved native file:

```sh
python3 assets/render/selene/tools/prepare.py inspect --model assets/render/selene/source/SELENE-showcase.solidcraft --out target/render-selene/inspection.json
python3 assets/render/selene/tools/prepare.py template --model assets/render/selene/source/SELENE-showcase.solidcraft --out target/render-selene/bindings.json
```

Inspection and template generation do not approve a model. Review the source identity, license/admission record and exact body selections. Preserve the unreviewed template and save the reviewed bindings separately. The source lock contains `active_source.status = "pinned_native"` and records the accepted file's SHA-256, byte size, format, units and Z-up frame. It must match the selected file exactly. Reviewed body bindings remain necessary before compilation.

After those checks, the prepared opaque-body subset can be compiled:

```sh
python3 assets/render/selene/tools/prepare.py compile --model assets/render/selene/source/SELENE-showcase.solidcraft --bindings target/render-selene/bindings-reviewed.json --lock assets/render/selene/source-lock.json --out target/render-selene/opaque-overlay.json
```

The tool does not overwrite outputs. Use a new output path for a new inspection, review or compilation. The compiled overlay is a reviewed native **BODY-only opaque diagnostic subset**. It does not apply these complete shot specifications, create terrain, reproduce layered materials or enable a future renderer.

Eligible roles satisfy all three conditions: `kind == "appearance"`, `owner == "subject"`, and `opaque_proxy_allowed == true`. Optics, emitters, dust layers, framing regions, interaction regions and generated stage geometry remain outside that compiler. A reviewed body must be appropriate for a uniform opaque proxy; face-level or multilayer targets cannot be silently converted to whole-body assignments. Preserve the source's engineering material and physical properties.

The current native BODY appearance command styles a shared body definition. An appearance selected through one occurrence can affect every copy, so the review must cover all affected occurrences. This subset does not provide independent per-occurrence styling. The compiler accepts feature-declared body names; generated viewport instance names are not automatic binding targets. Resolve conflicting role appearances explicitly before compiling a shared definition.

The prepared-package check is:

```sh
python3 assets/render/selene/tools/validate_prepared.py --json
```

The existing catalog check remains separate:

```sh
python3 docs/render-materials/validate_catalog.py --json
```

Data validation establishes reference and specification consistency. The source-inspection report, native evaluation, actual images and future simulation evidence are separate results. Keep generated reports, overlays, previews and caches under `target/render-selene/`; do not commit them as resource masters.

## Shot list

| Shot | Stage | Purpose | Quality / studio |
|---|---|---|---|
| `SELENE-SHOT-001` | `SELENE-STAGE-001` | Low-Sun hero with native scale and terrain contact. | `Q-006` / `STU-021` |
| `SELENE-SHOT-002` | `SELENE-STAGE-002` | High-Sun comparison, sharing the first shot's terrain, camera, materials and exposure. | `Q-006` / `STU-023` |
| `SELENE-SHOT-003` | `SELENE-STAGE-003` | Sampling tool and terrain contact macro. | `Q-007` / `STU-023` |
| `SELENE-SHOT-004` | `SELENE-STAGE-004` | LED-off/LED-on diagnosis of a genuinely solar-shadowed work patch. | `Q-005` / `STU-023` |
| `SELENE-SHOT-005` | `SELENE-STAGE-005` | Rover in a generic polar-ridge setting with grazing sunlight. | `Q-006` / `STU-021` |
| `SELENE-SHOT-006` | `SELENE-STAGE-006` | Arm, container, storage and transfer-station coverage. | `Q-007` / `STU-023` |
| `SELENE-SHOT-007` | `SELENE-STAGE-006` | Static release/capture planning poses, including a possible miss. | `Q-007` / `STU-023` |
| `SELENE-SHOT-008` | `SELENE-STAGE-007` | Shadow-interval audit with hardware lights off. | `Q-006` / `STU-023` |
| `SELENE-SHOT-009` | `SELENE-STAGE-008` | Neutral studio diagnosis of the reviewed rover finishes. | `Q-010` / `STU-002` |
| `SELENE-SHOT-010` | `SELENE-STAGE-009` | Eight presentation angles on a fixed studio stage. | `Q-003` / `STU-002` |
| `SELENE-SHOT-011` | `SELENE-STAGE-010` | Explicitly labeled editorial hero with one declared fill rig. | `Q-006` / `STU-024` |
| `SELENE-SHOT-012` | `SELENE-STAGE-006` | Vial walls, lid, contents and closure detail. | `Q-007` / `STU-023` |

The quality IDs are future authoring targets. They are not settings objects accepted by the current native command interface. In particular, do not submit the large lunar-master resolutions as current baseline setup commands without an independently verified executor contract.

## Coordinates, framing and scale

Lengths use millimeters and angles use radians. +Z is up. The -Y front / +X right labels are an **authored presentation-stage convention**, not an observation of the improved rover's heading. Review and record its source-to-stage instance alignment before resolving a shot. Camera and Sun azimuths refer to that stage frame; they are not geographic east/north or a lunar site ephemeris.

Cameras specify a target role, a zero target offset, a viewing azimuth/elevation, vertical field of view and frame-fill fraction. A future adapter derives target coordinates, distance and clip planes from reviewed role bounds, output aspect and the complete scene. It fits once on explicit request, respects camera locks and never rescales the model. The files contain no claimed native camera transform.

The sampling macro fits the actual contact region, while retaining the rest of the rover for shadows and reflections. It must not squeeze the whole rover into a small macro patch. The low/high-Sun pair shares the low-Sun geometry reference and seed so that the comparison changes illumination rather than terrain. The turntable has explicitly authored stage dimensions; the entire rotating subject envelope must fit with clearance before use.

The eight-view turntable rotates render instances of the rover and platform about the reviewed platform axis. Camera, room and rig remain fixed; internal joint poses remain frozen. These are presentation stills, not a mechanical turntable simulation or a lunar vehicle maneuver. The recipe requests no motion blur and makes no dynamics claim.

## Sun, vacuum and source ownership

Lunar stages use one environment-owned Sun. It is a **finite-angular disk represented directionally at infinity for the local stage**: direction and angular diameter are explicit, while a local XYZ light position is inapplicable. The local directional approximation has no stage-distance attenuation. The disk shown to the camera, if visible, must represent that same source. Do not add a second solar disk in an environment map or a second directional light in the studio rig.

The inherited draft diameter is `0.009250245036` radians, approximately 0.53 degrees. It remains an authored approximation, not a calculated lunar ephemeris. For context, the [NASA Sun Fact Sheet](https://nssdc.gsfc.nasa.gov/planetary/factsheet/sunfact.html) gives an apparent diameter of 1,919 arcseconds at 1 AU and variation with observing distance. The table does not calibrate these particular lunar shots. The recipe's irradiance value `1.0` is a dimensionless reference; it is not watts, lux, lumens or a measured exposure.

Physical lunar stages have zero sky radiance and no atmospheric scattering, fog, clouds, rain or wind. This is the surface-rendering approximation described in the [SELENE plan](../../../../docs/render-materials/SELENE.md), not a claim that the Moon has literally no exosphere. Geometry and reflected light can still illuminate shadows. Earth and stars remain disabled until separately reviewed celestial placement, visibility, radiance and exposure are available. An arbitrary oversized Earth or dense star overlay cannot be added to a physical shot.

The LED work patch retains the Sun and uses terrain to block its direct illumination. It needs exactly one calibrated subject-owned `rover_task_led`. Its visible emitter, analytic sampling proxy and optic must share the intended source path without duplicated power. The shadow-interval audit also retains the Sun behind geometry, disables all hardware sources and accepts a nearly black image.

Editorial staging keeps the vacuum rules but deliberately permits `STU-024` fill and an editorial fidelity label. The stage explicitly overrides the base `ENVSET-024` ambient fill to zero so that this demonstration uses one declared fill rig. It must remain distinguishable from the physical lunar shots.

## Role decisions that need explicit review

- `lunar_rocks` is a generated geometry set; `lunar_highland_rock` assigns the existing `MAT-545` appearance to that set. Every generated rock has an owner and appearance even when a base environment omitted a material-role field.
- `rover_dust_overlay` uses `MAT-551` as a bounded surface layer. Its host appearance remains intact. It is outside the native opaque compiler and does not create atmospheric dust or assign contact parameters.
- `task_led_emitter` and `task_led_optic` are separate. The existing candidate `MAT-456` specifically describes a **scattering silicone light tube**. If the improved model uses a different optical construction, choose a matching concept through a recorded shot-level override; do not treat that ID as a generic lens.
- `sample_scoop` requires an actual sampling tool or an approved derivative. A gripper alone does not fulfill a scoop/contact shot.
- `sample_container` identifies a reviewed vial assembly. The retaining grommet remains with the rack. Free-flight work requires an independent container identity, explicit closure and suitable collision geometry.
- A scene can record an absent or not-visible optional role. A required camera, tool, source or interaction role cannot resolve to absent. Unresolved roles must remain visible in the preparation report.

## Staged animation work

1. **Source and appearance diagnosis.** Verify the pinned native source, review semantic selections, compile the eligible opaque body subset and inspect native scale. No dynamics is involved.
2. **Still-image reference.** Bind stage geometry, materials, the Sun and optional hardware source. Review the physical hero, macro, lighting comparison and shadow audit before using the scene as an animation reference.
3. **Planning poses.** Use `SELENE-SHOT-007` to inspect release, candidate flight, capture/miss and storage coverage. Joint limits, clearances, closure and reach remain explicit checks. The file assigns no physical event times or launch velocities.
4. **Independent physical validation.** Establish mass/inertia, contact, actuation, constraint transitions and the millimeter/SI boundary. Use [lunar-physics-fixtures.json](../../../../docs/render-materials/lunar-physics-fixtures.json) as analytic checks, not as a ready-made rover trajectory. Rendering roughness does not determine friction or restitution.
5. **Approved bake and image sequence.** After the [36 simulation requirements](../../../../docs/render-materials/simulation-requirements.json) are satisfied for the chosen approximation, record an immutable bake and source/resource hashes. The future `SCENE-020`, `Q-008` and `Q-009` references define preview/final intent. Preserve misses and solver failures, and keep physics timing independent of output frame rate.

These catalog specifications prepare those stages. Source admission is recorded separately in the source lock; native derivative results and concrete showcase renders are reported by their own build and validation records. The catalog itself does not establish contact simulation, complete renderer binding or final images for all 12 shots.
