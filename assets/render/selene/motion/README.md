# SELENE mechanism acceptance

This directory records the source-level acceptance contract for **`SELENE-showcase.solidcraft`**. The source is now pinned and its persisted assembly records have been inspected. The checklist binds exact source IDs and parameters; it makes no native runtime claim from that container inspection.

[The source lock](../source-lock.json) selects the unchanged file supplied by SychPL, matching the author's public commit byte for byte. [SOURCE.md](../SOURCE.md) records that evidence. The functional derivative has a separate mechanism contract in [MECHANISMS.md](../MECHANISMS.md) and an executable native validator in [validate_native.py](../tools/validate_native.py).

## Three evidence levels

Each item in [mechanism-acceptance.json](mechanism-acceptance.json) separates:

1. **Documentation evidence:** a published project description or author demonstration, linked through the source lock's references.
2. **Native evidence:** exact identities, geometry, parentage, limits and mechanism relationships inspected in the admitted file.
3. **Runtime evidence:** results produced by a recorded SolidCraft revision from explicit inputs and an approved starting pose.

Native evidence is `inspected_container_only`; source runtime evidence remains `not_executed` in this checklist. The bindings are tied to source SHA-256 `1b0df486f30da157db27a33e0b5ed67f13d527d4dfdc6c334717a1df4e5b8677`. Reading the native container confirms records, references and stored values. It does not evaluate geometry, solve a joint, drive a study or certify a derivative.

`required_for_native_admission` identifies the mechanisms that establish the selected combined showcase. The sampled collision demonstration and flexible-hose visibility treatment are additional review areas. Any differences in the delivered source need explicit explanation; an admission tool cannot infer identity from a count or automatically approve a substitute.

## Review areas

| ID | Review area | Native and runtime questions |
|---|---|---|
| MCA-001 | Grounded structure and static modules | Which occurrences anchor the turret, dish support and other fixed modules? Do intended subsystems move without shifting the chassis? |
| MCA-002 | Four split solar actuators | Which bodies belong to each barrel and rod, including their eyes and pins? Do the pieces move together correctly? |
| MCA-003 | Actuator loops and travel stops | Do root pin, stroke slider and wing pin remain connected? Does an infeasible tilt stop with a useful reason? |
| MCA-004 | Mirrored solar tilt | Does the actual link preserve the documented opposing increments, with valid axes, offsets and limits? |
| MCA-005 | Six-wheel link chain | Are all wheels correctly driven through the chain regardless of link serialization order? |
| MCA-006 | Rack-driven helper track | Is the 38 mm coupling correctly bound and explicitly treated as demonstration geometry? |
| MCA-007 | Turret/dish gear | What are the actual driver, follower, sign, ratio and offset of the documented 24:36 pair? |
| MCA-008 | Vial-cap screw | Does the independent cap or helper follow its reported screw motion? What remains necessary for a releasable, sealed vial? |
| MCA-009 | Arm reach and drag | Do reachable and unreachable targets produce bounded, correctly reported poses within limits? |
| MCA-010 | Coordinated tour and traces | Which study keys drive the subsystems? Which traces are analysis geometry, and what defines physical time? |
| MCA-011 | Motion analysis and interference | What was sampled, which pairs were checked, and were results truncated? |
| MCA-012 | Flexible-hose visibility | Which rigid hose approximations or sketches were hidden, and how will their treatment be disclosed? |

The detailed checks cite the [mechanism reference](https://github.com/tangency-labs/solidcraft/blob/463cea0615a22be2f7d6c30512211f9eda97ff28/skills/solidcraft-mechanisms/reference.md), the [author's combined showcase](https://github.com/tangency-labs/solidcraft/pull/1540#issuecomment-6205857321), [motion export](https://github.com/tangency-labs/solidcraft/pull/1680), and [motion analysis](https://github.com/tangency-labs/solidcraft/pull/1712).

## Source bindings and deliberate derivative changes

The unchanged source contains 47 occurrences, 49 joints, nine links and one `SELENE tour` study. Structure occurrence `1930` is grounded. Rigid joints `30..47` attach 18 modules to it; the joint graph reaches every source occurrence except the independent rock sample `3458`. The four actuator loops use joint triples `18/19/20`, `21/22/23`, `24/25/26` and `27/28/29`. These are persisted source facts, not evaluated degrees of freedom or a solver result.

The original helper track has a 38 mm rack radius; the functional derivative specifies 40 mm to match the modeled wheel radius. The source's `7 → 16` link couples sensor and dish azimuth at `-24/36`; the derivative deliberately gives those pointing systems independent controls. The source cap keeps its 1.5 mm lead and 0–6 mm axial travel. Source facts remain in the checklist so that an intentional change is visible rather than silently relabeled as the original design.

The source tour has 120 steps and nine keyed joint coordinates. Its three rigid hydraulic-hose sweeps and six associated sketches are hidden. Hiding them avoids presenting a fixed sweep as a deforming hose; flexible routing remains separate work.

## Numeric planning checks

The four `analytic_candidate` records are original calculations from published values. They can be recomputed without a model. Passing them says nothing about the contents of a candidate native file.

Native lengths use millimeters and native angles use radians. The documented joint and study commands accept degrees or explicit angle expressions. A tool must convert once at the boundary; it must not pass a radian-valued native angle to a degree-valued command unchanged.

### Actuator length and stop

The public mechanism reference supplies rounded port-side coordinates. For a planar planning example, use the common actuator X coordinate as zero:

- Hinge: `H = (0, -140, 158) mm`.
- Root pin: `R = (0, -91, 134) mm`.
- Wing pin at zero angle: `T0 = (0, -180.41, 169.71) mm`.
- Wing rotation: positive about X.

The candidate relation is:

```text
T(theta) = H + Rx(theta) * (T0 - H)
L(theta) = norm(T(theta) - R)
stroke(theta) = L(theta) - L(0)
```

These inputs give a zero-angle length of **96.277475 mm** and a stroke of **-2.865953 mm** at **0.3490658504 rad** (20 degrees). The first positive angle reaching **-6 mm** stroke is approximately **0.5625425647 rad** (32.231315 degrees).

The reference reports approximately -2.87 mm at 20 degrees and a stop near 32.23 degrees. The small rounding difference is expected from its published coordinate precision. These rounded points are not exact native geometry and do not define a native replay tolerance.

The source bindings now record the full-precision pin positions. They give a 20-degree stroke expectation of **-2.8662098186467375 mm**, used by the functional validator. Verify closure over the path, not just at the final pose. An endpoint that happens to close can hide an impossible intermediate motion.

### Rack-and-pinion displacement

For the author's reported **38 mm pitch radius**, the displacement magnitude is:

```text
abs(delta_x_mm) = 38 * abs(delta_theta_rad)
```

One full turn gives **238.761042 mm**. The direction and initial offset depend on the source's joint axes and coupling. The moving regolith track is a prescribed demonstration helper; this relation does not establish wheel traction, contact forces, slip or lunar-ground dynamics.

### Gear-pair magnitude

Under the candidate interpretation that the 24 side drives the 36 side:

```text
abs(delta_follower_rad) = (24 / 36) * abs(delta_driver_rad)
```

A driver increment of **1.5707963268 rad** gives a follower magnitude of **1.0471975512 rad**. Source inspection confirms turret azimuth drives dish azimuth with ratio `-2/3` and zero offset. This historic analytic example does not apply to the independent pointing controls of the functional derivative.

### Vial-cap screw

Interpreting the documented 1.5 mm screw as **1.5 mm axial lead per revolution** gives:

```text
abs(delta_axial_mm) = 1.5 * abs(delta_angle_rad) / (2*pi)
```

A full turn yields **1.5 mm** axial travel. Source joint `49` is cylindrical about +Z, with a screw link from angular coordinate `0` to axial coordinate `1` and travel limits of 0–6 mm. This is a kinematic screw relation; it does not prove thread contact, closure sealing or capture retention.

## Runtime evidence to record

Each executed acceptance case should retain the native-file SHA-256, SolidCraft code revision, exact command inputs, initial pose, selected joint IDs and axes, effective link values, solver tolerances, resolved poses and any stop or collision reports. The functional validator writes actual MCP request/response JSONL, hashes its inputs before and after, resets primary drivers before each case, and records numeric comparisons without saving its in-memory test poses to the master.

A published numerical residual is evidence of that demonstration only. The author's reported arm residual is not a guaranteed precision threshold for arbitrary poses. Select a useful tolerance for the actual model and solver, and report it with the result.

The cited inverse-kinematics work does not check contact sets during `joint.reach`; contact review is a separate operation. Likewise, a sampled interference run can miss motion between samples. Record spacing, work limits and any truncation; use stronger checking when the task requires continuous clearance.

The reported 121 exported tour frames are export metadata. They do not establish the native study's exact key count, duration or physically feasible actuator speeds. A time mapping must be explicit before deriving velocities or accelerations.

## Limits of this acceptance

These checks establish reviewed kinematic structure and behavior. They do not establish mass properties, actuator torque, controller bandwidth, thermal limits, regolith forces, cable deformation or a feasible lunar sample throw and catch.

A vial cap that unscrews is useful source evidence. A future sample-transfer scene still needs a reviewed independent vial body, contents and closure boundaries, grasp/release rules, collision proxies, physical properties and capture behavior. No flight or collection mechanism is inferred from the demonstration.

Retain the source's provenance and record deliberate changes as new derivatives. A new source digest invalidates bindings and motion evidence; rendering quality settings cannot repair or silently retime those changes.


## Prepared inspection and future dynamics inputs

[preflight-commands.json](preflight-commands.json) is a read-only five-command inventory to run on the actual opened design. It does not create, drive or alter a joint. Save its output under ignored `target/`, with the source digest and executor revision.

[sample-sequence.json](sample-sequence.json) specifies eleven future dynamics states from approach and collection through closure, release, flight, capture and storage, including a failed-catch branch. It reuses the existing kinematic authoring foundation and adds the contact, mass/inertia, actuation and event evidence needed for a physically validated animation. It is not an executable `motion.study` or an approved bake.
