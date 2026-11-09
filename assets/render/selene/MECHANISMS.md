# SELENE mechanism design and verification contract

The improved native source already contains assembly structure, solar-actuator loops, wheel links and a tour. The functional derivative builds on those mechanisms through SolidCraft commands. This document distinguishes inspected source facts from reconstruction targets and the numeric checks needed to accept the result. It does not certify an unexecuted generator or replace native validation evidence.

## Source assembly

Read-only inspection of the pinned source in [SOURCE.md](SOURCE.md) confirms 49 joints, nine motion links and the 120-step `SELENE tour`. Joint IDs below belong to that source and must be resolved again if a derivative deletes and recreates joints.

| Source mechanism | Native source identity | Relation |
|---|---|---|
| Wheel train | `Wheel axis -1:-90` through `Wheel axis 1:90`, IDs 1–6 | Five links, ratio +1 between neighboring wheel joints. |
| Solar wings | Port/starboard tilt, IDs 9–10 | Mirrored rotation, ratio −1. |
| Solar actuators | Four sets of root pin, stroke and wing pin, IDs 18–29 | Each rod follows a closed loop; slider limits −6 to +0.4 mm. |
| Sensor and dish yaw | Turret ID 7, dish ID 16 | Source demonstrator gear, radii 24:36, ratio −2/3. |
| Moving demonstration track | `Track slide`, ID 48 | Source rack coupling to wheel ID 1, nominal radius 38 mm, reversed. |
| Vial cap | `Vial cap thread`, ID 49 | Cylindrical joint; 1.5 mm axial travel per full turn; slide limits 0–6 mm. |

The source turret elevation is saved at −12 degrees. New bearing geometry and motion links must be constructed in a deliberately restored common neutral pose; mixing that saved pose with zero-angle link offsets can rotate new parts unexpectedly.

## Reconstruction targets

The individual component scripts and the executed derivative are the authority for implementation. The following bounds are deliberately narrower than the source's broad demonstration ranges. They define an inspection envelope to be checked, not a proof of global collision freedom.

| Driver | Intended limits, degrees | Purpose |
|---|---:|---|
| Sensor azimuth | −135 to +135 | Independent pointing with a bounded cable envelope. |
| Sensor elevation | −25 to +45 | Controlled sensor-head tilt. |
| Dish azimuth | −120 to +120 | Independent dish pointing. |
| Dish elevation | −35 to +40 | Controlled dish tilt. |
| Arm azimuth | −45 to +45 | Keep a conservative showcase sweep. |
| Shoulder | −35 to +15 | Restrict travel around the modeled clevis. |
| Elbow | −30 to +55 | Articulate the rebuilt link stack. |
| Wrist and tool tilt | −45 to +45 each | Bound the local tool envelope. |
| Port solar wing | 0 to +30 | Remain within the inherited actuator stroke. |
| Starboard solar wing | −30 to 0 | Mirror the port driver. |
| Gripper jaw driver | −15 to +18 | Move two independently mounted jaws with opposite signs. |
| Suspension arm | −4 to +1.5 | Short-travel closed-loop damper demonstration. |

Remove the source's illustrative sensor-to-dish gear link so either assembly can point without moving the other. Transfer wheel shafts, flanges, hubs and retaining hardware to the parts they physically rotate with. The track remains an optional visualization helper. A 40 mm nominal coupling may match the grouser envelope, but it does not solve discrete grouser contact, slip or rover traction.

The gripper requires separate jaw components, concentric pivot sleeves, opposite revolute motion and contact pads. A revolute joint by itself is insufficient if the jaw is still part of the fixed gripper body. The geometric envelope needs a sweep check independently of the linked angle values.

## Rolling-element bearings

The bearing reconstruction separates the outer race, inner race, cage and each ball. The outer race follows its housing; the inner race follows the shaft. The cage revolves around the shaft axis, and each ball spins relative to the cage while its center orbits with it. Grooved races and spaced cage bridges provide a recognizable section and visible rolling elements.

Let **R** be pitch radius, **r** ball radius, and **θ** the shaft angle relative to the housing. For radial contact and an ideal no-slip bearing with a fixed outer ring:

```text
cage angle relative to housing = 0.5 × (1 − r/R) × θ
ball spin relative to cage     = −0.5 × (R/r − r/R) × θ
ball spin relative to housing  = −(R/r) × cage angle
```

The sign assumes all revolute axes use the same positive direction. The ball's CAD joint is relative to the cage; substituting an absolute spin rate there produces the wrong motion. These relations follow the cage and rolling-element frequencies in [Schaeffler, An Overview of Bearing Vibration Analysis, section 3](https://www.schaeffler.com/remotemedien/media/_shared_media/08_media_library/01_publications/schaeffler_2/technicalpaper_1/download_1/vibration_analysis_en_en.pdf), specialized to zero contact angle.

The design uses visual clearance between ideal raceway surfaces and balls. It does not solve Hertzian contact, lubrication, cage slip, preload, bearing life or load ratings. The equations drive a kinematic demonstration; no ball-to-race contact force is inferred.

For every bearing, validate a neutral pose, an interior nonzero shaft angle and its return to neutral. Compare both cage angle and relative ball spin with the equations. Also compare a ball's center of mass with its expected orbit; angle labels alone cannot prove that the component frame is correct. The spherical appearance of a ball makes spin visually ambiguous, so the joint readback is part of the evidence.

## Plain yaw supports

[yaw_supports.py](tools/yaw_supports.py) replaces the three source stacks of solid yaw disks with plain journal and thrust geometry. The fixed housing, rotating disk, existing component ownership and joint frames remain in place. Each housing receives a flanged brass bushing; its rotating disk gains an integral hollow steel journal. A stationary retaining pin captures the rotor inside a recess below the unchanged mast or clevis mounting plane.

| Support | Fixed housing / rotor diameter, mm | Steel journal diameter, mm | Brass bore diameter, mm | Retaining pin diameter, mm |
|---|---:|---:|---:|---:|
| Arm | 42 / 36 | 12 | 12.2 | 5 |
| Dish | 20 / 18 | 6 | 6.2 | 2.4 |
| Sensor turret | 28 / 26 | 8 | 8.2 | 3.6 |

The flanged thrust seats are 0.3 mm thick, with nominal seated contact against the rotating disks. Journal radial clearance is 0.1 mm; the retaining pins clear the hollow journals by 0.15 mm radially. Recessed heads provide 0.1 mm axial retention play. Four local reliefs also remove the dish housing's approximately 21.33 mm³ overlap with each original mounting head. This stage adds six bodies and no components or joints. Retainer threads use nominal cylindrical seats; fit classes, preload, friction, wear and structural capacity require separate engineering work.

The official 0.18.2 reduced native fixture reproduced the actual support and neighboring mast/clevis envelopes. Its 101 commands produced 20 bodies and 67 features without timeline errors or warnings. All 44 scoped interference checks, six source-range endpoint drives, nine analytic volume checks and twelve seating/separation measurements passed; each rebuilt journal, bushing and retainer is one connected shell. The largest volume discrepancy was 0.0156%, within the stated 0.02% numerical tolerance. Native section and exterior views were inspected. This fixture evidence does not replace final integration validation; the derivative's narrower pointing limits above remain unchanged.

## Closed-loop checks

### Solar actuators

For each actuator, rotate the tip point **T** about the wing hinge **H** and compute:

```text
stroke(θ) = length(T(θ) − R) − length(T(0) − R)
```

Here **R** is the fixed root pin. The source port hinge is at `(0, −140, 158)`, the root at `(x, −91, 134)`, and the tip at `(x, −180.40678269379407, 169.70686616300375)`, with `x = −62` or `+62`. Positive port tilt rotates about +X. At **20 degrees**, the expected stroke is approximately **−2.8662098186 mm**. The starboard wing has mirrored geometry and a negative drive angle. Verify all four sliders, not only one.

The [SolidCraft mechanism reference](https://github.com/tangency-labs/solidcraft/blob/ac1768e08b83ed33212459951f5b50aa7a57cddc/skills/solidcraft-mechanisms/reference.md) documents this closed-loop pattern, `flip: true` for co-directed frames and comparison with independent length calculations. Do not key the root pins, sliders or wing pins in a study; they are followers.

### Suspension dampers

The short-travel reconstruction gives each drive carrier its own hinge and splits each damper into barrel and rod. With the chosen neutral frame, the root and tip separation is 22 mm. Its analytic stroke relation is:

```text
stroke(θ) = hypot(35 cos θ − 20 sin θ − 35,
                  −35 sin θ − 20 cos θ − 2) − 22
```

Angles in this formula are radians. Expected strokes are **−2.4462732115 mm at −4 degrees** and **+0.9155981856 mm at +1.5 degrees**. Validate every closed loop against its actual reconstructed world points before using these numbers. Solid coil springs and rigid motor wires do not deform; hide them in moving inspection profiles unless a later animation model replaces them.

The suspension generator retains the original `Damper piston …` and `Suspension bearing …` body labels to avoid costly late rebuilds of the source history. The piston label also names the new rod component; the pivot ring functions as a brass plain bushing and the carrier ring as a fixed spacer. Those retained labels do not alter the geometry or joint behavior.

### Screw cap

The expected axial displacement is `1.5 × angle_degrees / 360` mm. A 360-degree drive gives 1.5 mm; 1,440 degrees gives 6 mm. The angular and axial limits should describe the same four-turn travel. This is an ideal screw coupling; modeled thread surface contact and sealing compression require separate work.

## Native acceptance

Use the model's own saved studies and exact joint names. Save the final derivative with all independent drivers at the agreed neutral pose. A study should return its drivers to neutral so opening another study starts predictably.

The pinned authoring executor is the official v0.18.2 CLI described in [SOURCE.md](SOURCE.md), producing format 28 for compatibility with the personal render branch. Once the master is saved and its bytes are fixed, run the independent validator from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 assets/render/selene/tools/validate_native.py \
  --cli /path/to/solidcraft-cli \
  --model assets/render/selene/models/SELENE-functional.solidcraft \
  --out target/selene/validation-final \
  --interference-pairs assets/render/selene/motion/interference-pairs.json
```

Use a new output directory for every run. The default suite performs 25 sampled driven cases and a final neutral check. Before each case it reads all independent joint coordinates, resets those that are nonzero, and verifies every primary coordinate is neutral. It compares actual native joint readbacks and world-coordinate centers of mass, retaining requests, responses, hashes, tolerances and failures. It drives an in-memory copy and never saves test poses to the master. The input model and pinned original must have identical hashes before and after validation.

Run full-model native processes serially. The validator confines its native subprocess to two Rayon workers and sets one glibc allocation arena with 64 KiB mmap and trim thresholds. These resource settings are recorded in the report alongside the unchanged geometric tolerances.

Optional `--interference-pairs path/to/pairs.json` accepts a JSON array of exact body-name pairs. The committed [selection](motion/interference-pairs.json) requests seven neutral-pose checks for opposing arm links, the tool wrist, gripper jaws and PCB connector clearances. Each pair receives its own bounded native interference check. The result records whether conservative broad-phase exclusion or exact Boolean intersection resolved it. Missing bodies, skipped or unchecked pairs, truncation and budget exhaustion remain failures or incomplete checks. A request to run the suite is not a passing result; report the returned status and evidence path explicitly.

The minimum evidence for each accepted mechanism is:

- A native timeline with no unreviewed errors or geometry warnings.
- A grounded assembly chain and a clean joint graph; no unexplained floating mechanism, loop conflict or coupling error.
- Actual movement of the intended occurrence, with positions or centers of mass checked before and after.
- Numeric follower agreement at selected interior and endpoint values, with native degrees/mm distinguished from persisted radians/mm.
- Scoped interference checks between parts that must clear each other. Timeouts and unchecked pairs remain incomplete checks, never passes.
- A native view or render showing the actual geometry. A diagram or an inferred body count cannot substitute for this review.

Broad all-body pair checks are expensive and can confuse deliberate mating contact with penetration. Check the relevant opposing link plates, clevis/race seats, shaft/bore and gripper contacts explicitly. A few passing samples do not prove clearance across every possible combination of joint angles.

Lunar gravity, loads, motor torque, thermal behavior, flexible wiring, suspension spring dynamics, granular terrain and autonomous sample capture remain separate simulation and animation work.
