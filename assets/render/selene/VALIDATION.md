# SELENE native validation

This report distinguishes evaluation of the complete saved rover, independent checks of its motion, reduced geometry fixtures and actual image generation. A clean feature timeline alone does not establish mechanical clearance, and a reduced fixture does not establish a collision-free whole rover.

## Native artifact and executors

The saved derivative is [models/SELENE-functional.solidcraft](models/SELENE-functional.solidcraft), **439,883 bytes**, native format **`solidcraft/28`**. Its SHA-256 is:

```text
3bd2bfa1c6336394c4a4208d30b52c80d06658a3bd7ea57cc93b774411d1bd68
```

The unchanged source is `source/SELENE-showcase.solidcraft`, 314,452 bytes, SHA-256 `1b0df486f30da157db27a33e0b5ed67f13d527d4dfdc6c334717a1df4e5b8677`. It is byte-identical to SychPL's published improved showcase file; [SOURCE.md](SOURCE.md) pins its author commit and license evidence. Native evaluation of that source found 1,418 bodies and no feature errors or warnings. Its container has 3,378 history features, 47 components excluding root, 47 occurrences, 49 joints, nine motion links and one motion study.

The authoring and final acceptance executor is the official SolidCraft v0.18.2 Linux CLI, SHA-256 `e50fe78292d569c36052b24f46f1732bc4e43d1ecdac6a64092aa1cee3b97086`. It writes `solidcraft/28`. The personal render branch's format-29 reader accepts that format. The native container is only read for inspection and selector construction; all CAD changes are made through SolidCraft commands.

The personal fork at `10ada1b45a4eb285ce752ac72f9aaa968ff8b5c4` was also compiled successfully and used for bounded product-render checks. Its CLI SHA-256 is `2a5485f7c7832d29b6340988837316e247b94eeeb9d4f7333ee616cea38c6826`. This separately built executable is not substituted into the authoring checkpoint chain.

## Complete-model build and motion acceptance

The full native build passed **17 saved stages and 2,318 native commands**, including the explicit inventory and graph inspections. Its final timeline has **zero errors and zero warnings**; all live body names are unique. The final native joint graph reports no errors, conflicts or floating occurrences.

| Native inventory | Supplied source | Functional derivative |
|---|---:|---:|
| Evaluated bodies | 1,418 | 2,116 |
| Components excluding root | 47 | 172 |
| Occurrences | 47 | 172 |
| History features | 3,378 | 5,484 |
| Joints | 49 | 180 |
| Motion links | 9 | 99 |
| Motion studies | 1 | 8 |

The final joint inventory contains 135 revolutes, 11 sliders, 33 rigid joints and one cylindrical joint. The saved document also contains **121 browser groups, ten named views and ten render setup records**. The build history and final native inventory are in `target/selene/native-build/`.

Independent final-master acceptance **passed all 33 cases and 880 numerical comparisons** in a fresh official v0.18.2 process. The run completed on 2026-10-22, from 08:40:24 to 08:53:15 UTC, with exit code 0. It exercised 26 kinematic/neutral cases and seven selected neutral-pose interference pairs. Both the master and original source remained byte-for-byte unchanged. All 19 independent drivers were neutral on open and after the final reset; the tested graph inspections reported no errors, conflicts or floating occurrences.

| Kinematic case family | Cases | Accepted exercise |
|---|---:|---|
| Drivetrain | 1 | Six wheels follow the driver; a 90° turn gives −62.831853 mm demonstration-track travel at the 40 mm nominal radius. Independent wheel and track world-position checks pass. |
| Solar deployment | 1 | The selected 20° deployment closes all four actuator loops and agrees with the independently calculated strokes. |
| Independent pointing | 2 | Sensor and dish pointing are driven independently, with native follower and world-position checks. |
| Gripper | 2 | Driver endpoints −15° and +18°, opposite follower values and jaw world positions. |
| Sample cap | 2 | 360° and 1440° rotation produce 1.5 mm and 6 mm axial travel respectively. |
| Suspension | 12 | Each of six carriers is driven to both −4° and +1.5°. Closed-loop strokes and carrier world centers of mass match independent calculations. |
| Rolling-element bearings | 5 | Shoulder, elbow, wrist, tool tilt and sensor elevation drivers exercise all ten bearing cages and 80 ball-spin relations, with one orbital witness per bearing. |
| Neutral reset | 1 | All 19 independent driver coordinates return to zero. |
| **Kinematic total** | **26** | **All passed.** |

The 880 numerical comparisons include repeated neutral-coordinate readbacks between cases; they are not 880 distinct poses. Numerical agreement is a solver check, not a manufacturing tolerance claim.

| Numerical quantity | Comparisons | Maximum absolute error | Acceptance tolerance |
|---|---:|---:|---:|
| Joint angles | 756 | 7.106 × 10⁻¹⁵° | 10⁻⁶° |
| Linear joint coordinates | 19 | 3.777 × 10⁻¹¹ mm | 10⁻⁵ mm |
| Suspension strokes, included above | 12 | 3.927 × 10⁻¹² mm | 10⁻⁵ mm |
| World center-of-mass coordinates | 105 | 4.443 × 10⁻⁵ mm | 0.002 mm |
| Bearing orbit coordinates, included above | 30 | 1.318 × 10⁻⁵ mm | 0.002 mm |

The largest linear error was a solar actuator stroke; the largest world-coordinate error was the dish yaw mount's center-of-mass height.

| Selected neutral-pose pair | Completed method | Result |
|---|---|---|
| `Shoulder link -9` / `Elbow link -9` | Exact solid intersection | Clear |
| `Elbow link -9` / `Distal wrist link -9` | Exact solid intersection | Clear |
| `Tool wrist machined tongue` / `Tool tilt joint shaft` | Exact solid intersection | Clear |
| `Gripper jaw -1` / `Gripper jaw 1` | Conservative broad-phase separation | Clear |
| `Raspberry Pi 5 PCB` / `Pi5 USB2 mounting tab 1` | Exact solid intersection | Clear |
| `Raspberry Pi 5 PCB` / `Pi5 USB3 mounting tab 2` | Exact solid intersection | Clear |
| `Raspberry Pi 5 PCB` / `Pi5 GPIO pin 40` | Exact solid intersection | Clear |

These are **six exact intersection checks and one broad-phase clearance**, all with zero reported overlaps, unchecked or skipped pairs, no truncation and no exhausted budget. They are selected checks at neutral, not an exhaustive swept collision analysis.

The complete local machine-readable report is `target/selene/validation-final/validation.json`, schema `1.0.0`, 228,492 bytes, SHA-256 `0ccbf279f27885f6a4646a691a03915ca9ae22ae2f6bfef9574b856392474130`. It records every case, comparison, drive and interference result. The validator's SHA-256 is `b032962210e86a516193bd549d9137dcff93dbff4a3e038ca9a08de3493e1aac`. The repository's resource rules keep raw execution reports in ignored `target/`; the shipped validator and committed case selection reproduce them.

The reproducible builder saves every completed stage, validates complete native command and timing traces, rejects timeline errors or warnings, requires unique live body names and checks the native joint graph. Resume verifies source, executable, recipe, result and checkpoint hashes. The independent validator opens the final saved file in a new process and checks requested values, follower values and world-coordinate measurements against separate calculations. It does not save its test poses over the master.

The selected interference pairs in [motion/interference-pairs.json](motion/interference-pairs.json) are deliberately bounded. Any timeout, unchecked pair or failed command is incomplete evidence, not a clearance pass. They do not exhaust all body pairs or all simultaneous configurations.

## Reduced-fixture evidence

These checks were completed before integrating the corresponding production commands. Their scope remains local even when the same geometry is present in the full rover.

| Subject | Actual evidence | Result and scope |
|---|---|---|
| Rolling-element kinematics | An arm bearing and a sensor-head bearing each driven by 30°; all eight relative ball spins and one orbital position checked in each case. | Arm cage 12.679558011° and relative ball spin −94.643843725°; head cage 13.360248447° and relative ball spin −135.576157538°. Maximum world orbital error 1.474 × 10⁻¹⁰ mm. Ideal no-slip relations, without contact-force simulation. |
| Bearing clearances | Eleven core arm/bearing pairs and four head-retention pairs: 14 completed exact Boolean checks and one bounding-box exclusion. Core fixture: 288 bodies, 736 features, 117 components, 115 joints and 90 links. | No overlap reported and no pair unresolved after retries. Both fixture inventories have zero feature errors/warnings. These official v0.19.0 fixtures precede the production arm set-screw tips' final 0.05 mm extension; the named local pairs do not establish all-body or all-pose clearance. |
| Raspberry Pi 5 | Official v0.19.0 fixture: 748 commands, 492 bodies and 1,202 features. Two analytic volumes, all 40 GPIO centers, six exact Boolean pairs and a separate broad-phase check of 26 major package envelopes. | PCB volume error 0.000514094% against a 0.01% tolerance; maximum GPIO center error 1.422 × 10⁻¹⁴ mm. No reported overlaps, unchecked pairs or timeline errors/warnings. Component layout remains a visual reconstruction. |
| Gripper | Native checks at −15°, 0° and +18°, plus a 661-point analytic sweep at 0.05° and a continuous vertex/stationary-point separation bound. | Minimum jaw-to-jaw gap 5.735342 mm; minimum pad-to-pad gap 6.360460 mm. The local jaw envelopes remain in opposite half-planes over the continuous driver interval. Pin/sleeve radial clearance is 0.05 mm. This is a local gripper proof. |
| Suspension | Retained-source-label fixture: 71 commands, 15 bodies, 43 features; both −4° and +1.5° endpoints; 12 pair-pose checks over eight unique pairs, including six exact Boolean intersections. | No timeline errors/warnings or reported overlaps; no unchecked pairs. Maximum slider-equation error 3.927 × 10⁻¹² mm and world center-of-mass error 1.973 × 10⁻⁹ mm. |
| Yaw journals | Official v0.18.2 fixture: 101 commands, 20 bodies, 67 features; six support-only rotation cases, 44 pair-pose checks over 26 unique pairs, including 29 exact Boolean intersections; nine volume checks and 12 seating/distance measurements. | All cases passed; no timeline errors/warnings, unchecked pairs or reported overlaps. Maximum analytic volume error 0.0155823%, within the 0.02% tolerance. |
| Integral glass vials | Official v0.18.2 fixture: 55 commands, six cups, one component and 49 features. The 22-command production subrecipe matches the validated recipe exactly. | Maximum analytic volume error 0.0080213%, COM error 0.000108088 mm and bounding-box error 0.000155825 mm. Each cup has one kernel shell; names, ownership, Glass material and opacity 0.30 are preserved. |

The yaw fixture exposed an existing overlap of approximately 21.33 mm³ between each of four dish mounting screw heads and their stationary support. Four local reliefs remove those overlaps. The support-only fixture exercises arm/turret ±160° and dish ±150° to inspect the axisymmetric interfaces; the complete rover retains its narrower documented joint limits. Those local rotations do not expand the rover's accepted motion envelope.

The journal supports have nominal 0.1 mm radial journal clearance, 0.15 mm radial pin clearance and 0.1 mm axial retention play. Display-mesh distance measurements of the tiny retainer gaps were 0.0644–0.0826 mm. These are mesh approximations; the nominal analytic dimensions and completed solid-interference results are recorded separately.

The vial fixture also exported 57,264 native STL triangles. After coordinate welding at 10⁻⁶ mm, the mesh has six connected components and zero boundary, nonmanifold or inconsistent-winding edges, and zero degenerate triangles. Each vessel retains its 11.6 mm outside diameter and 27 mm height, with a 0.8 mm wall and 1 mm integral floor. Its fixture SHA-256 is `76f0cb4e25e7beb2f04a1ccd84619144441f9d580129f0325c29ef8d774ef2a6`; detailed evidence is in `target/selene/vial-repair/validation-report.json`. Mesh closure does not establish physical dielectric renderer support.

Two initial bearing checks exhausted their eight-second budgets. With 30-second budgets, the shoulder-clevis/outer-race check completed in 12.122 seconds and the ball/inner-race check in 10.382 seconds, both clear. The original timeout reports remain alongside the successful retries. Their initial zero-overlap counts were not accepted as passes.

The electronics and core bearing fixtures use official v0.19.0, format 30; they are development fixtures, not the distributed master. Their production commands were subsequently replayed in the complete rover with v0.18.2. The retained-label suspension fixture's SHA-256 is `1da6c92ba9ff2c421dc5b475f9971b458d3d43bbb048147df0cdeb70fefb9a11`; the yaw fixture's is `65ba6b033e301bd6cc4c91caa31d2ea67c69064d970355ff0597609adc62efcd`. The unchanged electronics generator is `67a69a7e613ad59dcf474dc6ecd9d9fb8439aa193e2e2ee458c282b65186fd9d`; its fixture is `578d9ba7b00738552ef9201c57489520fd86275c7fcfa23818adcdaed07aae04`.

Local report locations in a development checkout are `target/selene/bearings/compact-phase-report.json`, `target/selene/bearings/review-gripper-presentation.json`, `target/selene/electronics/validation-report.json`, `target/selene/suspension/retained-labels/validation-report.json` and `target/selene/yaw-support-audit/validation-report.json`. Detailed generated logs, fixture files and diagnostic images stay outside the production asset package.

## Image evidence

The compiled personal fork reopened the complete final master and generated three actual **1400 × 1000 native Design PNGs**. All three were visually reviewed and are shipped under `exports/`. The overview shows the neutral rover, the electronics view exposes the connector mouths and populated board, and the bearing view isolates its races, balls and cage. The capture opened all 2,116 bodies with a clean timeline and left the master hash unchanged.

| Shipped preview | Bytes | SHA-256 |
|---|---:|---|
| [SELENE-overview.png](exports/SELENE-overview.png) | 423,686 | `44d317ca9b0f77c3ac5d43409f0b6b281b9d6724d58a3c8edd8ca2db19a0bc63` |
| [SELENE-electronics.png](exports/SELENE-electronics.png) | 225,933 | `cd8af0700ce6a4cc0aa00bdfa5d246e4b3ed491847795636c65d9a8ead45991e` |
| [SELENE-bearing.png](exports/SELENE-bearing.png) | 284,094 | `ed3a3da01946019c9c380bf8515013eca3a05c7bf5161ff4bb98ee36eb5ef92b` |

The native capture metadata identifies the CPU rasterizer and its expected warning: surface-analysis colors, highlights and legends are not drawn. These previews do not claim physical PBR or transmission. Cameras, executor hash and capture metadata are recorded in `target/selene/final-previews/export-report.json`; [tools/export_views.py](tools/export_views.py) reproduces the three selections without saving transient visibility or camera changes over the master.

Two actual `cpu-product` renders from the compiled personal fork passed preflight and image generation with no warnings: a single bearing fixture (20 visible bodies, 93,654 triangles) and the Pi isolated from the integrated rover checkpoint (504 visible bodies, 103,004 triangles). Both outputs were 768 × 576 PNGs and were visually inspected. The bearing is a reduced fixture; the electronics image uses actual integrated geometry. See [RENDER.md](RENDER.md) for exact resource estimates, rendering scope and limitations.

The complete rover exceeds this revision's 262,144-triangle CPU product admission limit. The 64 arm-bearing balls alone contain 654,336 reference-mesh triangles. That code path has not produced a whole-rover PBR image. Native Design previews and bounded product renders are different evidence.

## Repository checks

The catalog and prepared-resource validators passed with the source status `pinned_native`. The wider scene catalog intentionally remains `runtime_ready: false`: the native model's completion does not bind every proposed stage, material role or future dynamics event.

The required repository-wide `cargo xtask ci` command was attempted with Rust 1.95.0 and the required socket permissions. Formatting, asset attribution, icon checks (17 tests), license checks, generated lists and dependency-layer checks (17 crates) passed. CI then stopped at a pre-existing `clippy::nonminimal_bool` diagnostic in `crates/geom/src/fill_general.rs:166`, under `-D warnings`. That application source is unchanged by this work. Later workspace tests and WebAssembly checks were not reached. The whole repository gate is therefore **not green**; the successful native CLI build and asset checks do not replace it.

The final `cargo xtask ci` attempt, after adding the shipped master and all three previews, reproduced that same baseline Clippy failure. Its exact output is retained locally in `target/selene/ci-final.log`. Its asset and icon gates passed. The catalog and prepared-resource validators also passed with zero errors, and the final whitespace check was clean. No application Rust source is changed by this package.

## Resource failures and recovered attempts

The working environment has an 8 GiB memory limit. Three earlier native attempts were killed for exceeding it: one overlapped a second cold full-model open; two involved late renames of original suspension bodies. Late original-body renames forced a long history rebuild while prior evaluated state remained resident. The production recipe retains those original labels and splits suspension geometry into smaller saved stages. Geometry, ownership and joint intent remain explicit.

Full-model native processes now run sequentially. The provided tools set two native worker threads and an allocator policy of `MALLOC_ARENA_MAX=1`, `MALLOC_MMAP_THRESHOLD_=65536` and `MALLOC_TRIM_THRESHOLD_=65536`. These are process-memory settings, not geometry simplifications. Failed attempts and their logs are retained under `target/selene/failed-attempts/`; they are not counted as successful validation.

Strict container inspection also rejected one intermediate suspension checkpoint because the inherited tour still referenced the original wheel joint ID after wheel reparenting. The builder now refreshes that study by exact joint names before saving the suspension checkpoint. The corrected stage completed with 172 components and 180 joints, and the rejected file remains diagnostic evidence only.

The first presentation attempt failed at the original touching vial-wall/bottom union and saved no presentation checkpoint. The validated replacement removes each split pair and creates an envelope with a blind bore, retaining the original wall name, external dimensions, component and appearance. Its base coordinates account for the source's already baked `Move9` translation. The failed union log remains under `target/selene/failed-attempts/vial-touching-union/`.

## Remaining physical and animation scope

The rover is an editable geometric and kinematic showcase. The rolling bearings use ideal linked revolutes; the suspension uses rigid links and a closed pin–slider–pin loop. Neither provides contact forces, spring dynamics, damping, backlash, stress, fatigue or a rated load capacity. Fasteners have nominal seats rather than fully modeled thread engagement.

The six rigid spring solids and twelve motor leads remain neutral-pose render details and must be hidden for suspension movement. The motion visibility profile handles this. Flexible hose display geometry and rigid decorative arm hydraulic cylinders are excluded from the relevant moving presentation.

The Pi is an original visual reconstruction based on cited public mechanical dimensions. It is not manufacturer CAD, a PCB netlist, exact component artwork or an electrical design. Opaque PBR parameters are authored appearances; nominal material density does not establish measured mass, thermal behavior or friction. Glass viewport opacity is not a physical transmission model.

The adapted legacy tour and seven focused studies are animation authoring resources. Their presence does not certify every frame, every combined pose, obstacle clearance or a future throw-and-catch sequence. Those checks belong to the specific animation shot and any physical simulator chosen later.
