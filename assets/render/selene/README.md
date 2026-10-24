# SELENE functional showcase

This package develops the **author-supplied improved SELENE** into an editable native model for close-up rendering and subsequent mechanism animation. The input is SychPL's `SELENE-showcase.solidcraft`, matched byte for byte to his published file. The original is retained unchanged; the enhanced derivative is [models/SELENE-functional.solidcraft](models/SELENE-functional.solidcraft).

Open the derivative in the personal SolidCraft render branch. The model contains the geometry, component ownership, joints, motion links, physical-material proxies, appearances, browser groups, saved views and inspection studies. Python is only a development tool for reproducing the changes; opening and using the design does not require Python.

![SELENE functional showcase in native Design shading](exports/SELENE-overview.png)

*Actual native Design preview of the neutral master. Close-ups and the product-render executor's current limits are documented in [RENDER.md](RENDER.md).*

## What changed

| Area | Native implementation |
|---|---|
| Rolling-element bearings | Ten open radial bearings at the arm and sensor head. Each has grooved steel races, eight individually represented balls and a brass cage. The inner race follows the shaft, the outer race follows the housing, and linked revolutes drive cage orbit and relative ball spin. |
| Arm joints | Revised plate spacing, bored seats, stepped clamp hubs, shafts, spacers, locking screws and a machined wrist tongue. Rigid decorative hydraulic cylinders that would detach during movement are removed. |
| Gripper | Two separate moving jaw components, concentric brass plain sleeves, opposite revolute motion and elastomer contact pads. Its commanded range is −15° to +18° at the driver. |
| Suspension | Six hinged trailing carriers with reinforcing gussets. Separate hollow damper barrels and rods form six closed revolute–slider–revolute loops. Local support reliefs clear the moving eyes and arms over the bounded travel. |
| Drivetrain | Wheel shafts, hubs, flanges and retaining screws belong to the rotating wheels. Wheel joints attach to the moving carriers. The optional demonstration track uses a 40 mm nominal rolling radius. |
| Yaw supports and pointing | Three solid-disk placeholders become journal supports with flanged brass bushings, hollow rotating journals and recessed axial retaining pins. Sensor and dish pointing are independent; the illustrative gear coupling is removed. |
| Raspberry Pi 5 | A board with rounded corners, mounting holes, plated pads and vias, plus 491 new bodies for packages, shields, passives, headers, connector housings, contacts, solder details and mounting tabs. Hollow ports replace solid envelopes. |
| Materials and appearance | Retained orange/charcoal identity with distinct metals, polymer, elastomer, FR4, silicon and optical surfaces. Opaque bodies carry PBR values; physical density proxies are explicitly separate from optical properties. |
| Sample laboratory | Each glass vial is one hollow native solid with an integral bottom, retaining its original external dimensions. The existing screw-driven cap retains 1.5 mm travel per revolution. The source rock is grounded as demonstration scenery. |
| Inspection | Body groups by function, ten named views with render setup records, seven focused motion studies, and an adapted copy of the author's original tour. |

The native build and independent acceptance run have separate evidence. See [VALIDATION.md](VALIDATION.md) for the final hash, actual counts, executor, completed checks and remaining limitations. [MECHANISMS.md](MECHANISMS.md) gives the joint conventions and independent equations; [ELECTRONICS.md](ELECTRONICS.md) explains the board reconstruction and its fidelity boundaries.

## Using the model

The file opens in the neutral pose: all independent drivers are at zero, including sensor elevation. Saved views are named `SELENE - Whole rover`, `Electronics`, `Shoulder bearing`, `Elbow bearing`, `Gripper`, `Sensor head`, `Sample laboratory`, `Drivetrain`, `Suspension` and `Solar cells`, each with the same `SELENE - ` prefix.

Use the seven inspection studies for drivetrain, solar deployment, arm articulation, gripper, independent sensor pointing, sample cap and suspension. They return their independent drivers to neutral. The inherited tour is named **`SELENE - Legacy author tour (adapted limits)`**; it preserves the author's keyed motion while resolving recreated wheel joints and clamping the selected limits. A keyed study is an authoring resource, not evidence that every frame of every combined pose has been cleared.

For suspension motion, hide the six rigid spring solids and twelve motor leads. They remain useful neutral-pose render details but do not deform. The `motion` visibility profile in [tools/presentation.py](tools/presentation.py) performs this selection and also hides the optional track and source flexible-hose display geometry. Export or save inspection poses to a separate file; keep the master neutral.

The source cap has an ideal screw relation. The six complete vials remain in the sample laboratory assembly. Independent grasp, release, free flight and capture are future work; the current cap motion does not implement those events.

## Reproduce the native derivative

Use Python 3.11+ and the **official SolidCraft v0.18.2 CLI** identified in [SOURCE.md](SOURCE.md). That executor writes `solidcraft/28`, readable by the personal render branch's format-29 reader. The newer official v0.19.0 writes format 30 and is unsuitable as a drop-in authoring replacement for that branch. No format marker is edited to bypass compatibility.

From the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 assets/render/selene/tools/build.py \
  --cli /path/to/solidcraft-cli \
  --work target/selene/rebuild \
  --output target/selene/SELENE-functional.solidcraft
```

The builder checks the pinned original, evaluates it, then uses only native commands. Stages run in fresh processes so the large model's undo history remains bounded. Each stage retains its command script, full native results, timing trace and saved checkpoint under the selected work directory. Every checkpoint must have a clean feature timeline, unique live body names and a clean joint graph. The build history records hashes of the source, executor, recipes, results and checkpoints.

Resume an interrupted build by repeating the command with `--resume`. The builder rejects changed recipes, executables, results or checkpoints. `--until` stops at a named stage for inspection. Inspect failures in their logs; use a separate work directory when changing an already completed stage.

Run independent acceptance on the saved derivative:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 assets/render/selene/tools/validate_native.py \
  --cli /path/to/solidcraft-cli \
  --model target/selene/SELENE-functional.solidcraft \
  --interference-pairs assets/render/selene/motion/interference-pairs.json \
  --out target/selene/acceptance
```

This drives an in-memory copy and compares native joint readbacks and occurrence centers of mass with independent calculations. It never saves test poses over the master. Optional exact-name interference pairs are bounded and report unchecked pairs as incomplete. Detailed execution evidence stays in `target/`; production native files and English documentation are the shipped resources.

## Render preparation and the wider catalog

The full-detail [native 4K studio render](RENDER.md#full-detail-4k-path-traced-studio-image-2026-10-22) now provides a path-traced showcase of the unchanged functional master. It preserves all original visible geometry, uses explicit glass and authored materials, and ships with its exact [request/provenance manifest](scenes/studio-hero.json). The Design previews remain distinct geometry-inspection resources.

The ten saved camera/setup records and the model's body appearances are concrete native resources. [RENDER.md](RENDER.md) documents the actual image workflow and any executor limits. Transparent viewport appearances alone do not establish physical refraction, and density does not supply roughness, friction, electrical behavior or optical transmission.

The existing [48-appearance pack](materials/appearance-pack.json), [33 semantic roles](scenes/roles.json), [ten stage specifications](scenes/stages.json), [12 shots](scenes/shots.json), [UI flow](ui/demo-flow.json) and [future sample dynamics sequence](motion/sample-sequence.json) remain a broader authoring specification. The source is now pinned; those resources still need their individual geometry bindings and renderer admission. The catalog's `runtime_ready: false` status is intentional. No lunar terrain, dust dynamics, calibrated optical library or throw-and-catch simulation is implied by the native model's completion.

```sh
python3 docs/render-materials/validate_catalog.py
python3 assets/render/selene/tools/validate_prepared.py
cargo xtask ci
```

The Python checks validate prepared data and the source contract. They do not replace native geometry, mechanism or visual checks. Repository-wide gate status is recorded separately in the validation report.

## Provenance and scope

[SOURCE.md](SOURCE.md) and [source-lock.json](source-lock.json) pin the exact source bytes and author history. The original rover is credited to 0xlillith, the improved showcase assembly to SychPL, and the emblem to SolidCraft contributors. Ubuntu Light lettering retains its [Ubuntu Font Licence notice](licenses/UFL.txt). The source branch has no separate file-specific license declaration for the showcase additions; that provenance distinction is preserved.

New geometry, procedural scripts and documentation are original contributions under MIT OR Apache-2.0. Exact non-Markdown asset rows are in [ATTRIBUTION.md](../../../ATTRIBUTION.md). Manufacturer CAD, PCB artwork, logos and netlists are not embedded.

The model demonstrates geometric detail and ideal kinematics. It is not a manufacturing release, a complete electrical design, a load-rated bearing selection or a physics simulation. Joint limits are a documented inspection envelope; sampled checks do not certify every simultaneous arm, solar and suspension configuration.
