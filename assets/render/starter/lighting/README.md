# Authored lighting resources

This slice resolves the original 24 studio recipes into numerical authoring
resources. It includes **38 calibrated finite area sources**, one authored
finite-angular solar source, eight passive cards, and explicit blocked or
external-source states. It does not add an application command or install a rig.

The existing [studio preset file](../../../../docs/render-materials/studio-presets.json)
remains the editable authority for names, geometry, intent, requirements and
source relationships. [recipe.json](recipe.json) pins that file by SHA-256 and
defines the numerical calibration settings. Edit those authorities and rebuild;
do not edit [exports/rigs.json](exports/rigs.json) by hand.
The export records SHA-256 fingerprints for the recipe, upstream source and
generator, so authoring-code changes require a regenerated provenance record.

All authored content and tools are original project work under `MIT OR Apache-2.0`.
No external light probe, IES profile, commercial template or rendering
implementation is bundled. References in the recipe identify public radiometric
definitions and equations only. These sources are relative RGB references, not
measured luminaires, spectral distributions or engineering photometry.

## What is available

| Original preset | Numeric resource state |
|---|---|
| STU-001–017 | Complete finite-area source gains and passive-card transforms; runtime and subject geometry unbound |
| STU-018 | Inspection area calibrated; internal LED source blocked by incompatible calibration fixture |
| STU-019 | Entrance source blocked by incompatible calibration fixture |
| STU-020 | Uniform distant solar disk with explicit relative normal irradiance and angular radiance |
| STU-021 | Adds no light; requires one resolved existing environment sun |
| STU-022 | Adds no light; requires verification that the whole scene has zero emitted energy |
| STU-023 | Adds no light; requires one resolved environment sun for the externally supplied SELENE poses |
| STU-024 | Fill gain blocked until the existing environment sun's reference signal is measured |

There are 19 `authored_numeric_unbound` records, three `blocked_calibration`
records and two `requires_existing_environment_sun` records. Every record has
`runtime_ready: false`. The 19 numeric records include the zero-source diagnostic;
they are not 19 independently rendered or installed application presets.

The LED strip and light-guide source lie inside or intersect the prescribed
reference sphere. A closed opaque reference sphere cannot serve as their exterior
calibration receiver. The tool rejects those gains instead of moving the source,
shrinking the sphere, applying a point approximation or inventing optical output.
A separate aperture or diffuser fixture needs an explicit reviewed definition.
The editorial lunar fill likewise retains `emission: null` until its actual
environment-sun response is available.

## Rebuild and validate

From the repository root, with Python 3.11+ and NumPy installed:

```sh
python3 -B assets/render/starter/tools/lighting.py build
python3 -B assets/render/starter/tools/lighting.py validate
python3 -B assets/render/starter/tools/test_lighting.py
```

With Pillow installed, `python3 -B assets/render/starter/tools/lighting.py review`
writes `target/render-starter-review/lighting-reference.png`. This ignored sheet
shows the numerical Lambertian fixture at one fixed view, with source radiance
only and a declared Reinhard-to-sRGB display transform. It deliberately excludes
passive cards and actual CAD geometry. It is not a SolidCraft screenshot, final
rig render, material preview or photometric measurement.

`build` writes only the known lighting export. `validate` recomputes the export
and compares its bytes; it does not update files or change a source pin. Inputs
reject duplicate keys, non-finite or Boolean numeric fields, oversized arrays,
degenerate frames, unsupported source types, active reflectors and hidden physical
lunar fill. The committed source pin makes a changed upstream recipe an explicit
review event. The integration and memory bounds are enforced before allocation.

Generation is deterministic in the documented Python/NumPy environment. The
export rounds numbers to 12 significant digits. The convergence diagnostic
`higher_order_relative_difference` first uses 12 decimal places, a reporting
resolution of `1e-12`, to suppress insignificant NumPy/libm differences observed
between Linux and Windows. Convergence acceptance still compares the unrounded
full-precision result with the configured threshold; reporting precision never
relaxes that gate. Source gains, transforms and other substantive numbers retain
their existing precision. Cross-platform bit identity is not promised for every
mathematical-library version; a remaining difference must be reviewed, not
approved by replacing a stored source hash automatically.

## Calibration and quantities

The numerical fixture preserves `CAL-LIGHT-001`: a centered Lambertian sphere
with radius `0.25B`, diffuse reflectance 0.18 and `B = 1000 mm`, no other source,
no floor, no environment and no passive cards. Its response is the equally
weighted average of the masked disk mean in six orthographic views along the
positive and negative Cartesian axes. Values are scene-referred and linear;
no display transform, clipping, automatic exposure or camera change is applied.

For each area source, the tool integrates incident radiance over the finite
rectangle using both source and receiver cosines and squared distance. The
Lambertian outgoing radiance is reflectance times irradiance divided by pi.
The sum of the six projected view weights at a sphere normal is
`(|nx| + |ny| + |nz|) / (6*pi)`. Surface integration of this weight reproduces
the specified six-view measurement without raster-mask aliasing.

The primary integration uses 48 Gauss-Legendre latitude nodes, 96 azimuth samples
and a 12-by-12 rectangle quadrature. Every calibrated source is also evaluated
with 72 latitude nodes, 144 azimuth samples and a 20-by-20 rectangle quadrature.
The relative difference must be below 0.0015. The initial export's largest
observed difference is approximately **0.00017489 (0.01749%)**. This is a
convergence observation, not a rigorous error bound or runtime renderer result.

Zero reference EV targets one arbitrary response unit. The per-source radiance
gain is `2**EV / unit_radiance_response`; a change of one EV doubles its reference
signal. Sources retain their distinct areas, orientations and distances. The
numbers are **relative radiance**, not point radiant intensity, lumens, candela,
watts or lux. Passive-card interreflection is deliberately outside per-source
calibration and must be evaluated in the completed scene.

The solar recipe has one owned uniform disk. Its authored normal irradiance
`E` and angular radius `a` give radiance `E / (pi*sin(a)**2)`. The export records
both quantities distinctly. The neutral RGB and approximate disk size are
authoring inputs, not a measured solar spectrum or site ephemeris. The runtime
adapter must provide finite-angular sampling and visibility and must not add a
second environmental solar contribution.

The mathematical tests check an independently derived closed-form directional
sphere response, six separately integrated projected disks, finite-area
far-field inverse-square behavior, zero back emission, solar-disk integration,
EV ratios, uniform-scale behavior, valid source frames and the source-admission
failures. They are numeric authoring checks; they do not exercise SolidCraft's
CPU/GPU backends or imply measured optical accuracy.

## Integration requirements

Positions and sizes in the export are resolved in millimeters once for the
reference `B = 1000 mm`; width, height and emitting-normal axes are explicit and
orthonormal. The subject frame remains `+Z` up, `-Y` front and `+X` right. Each
record preserves the source's original origin, aperture, region and fit policy.
Do not apply the reference transforms directly to an unrelated subject without
resolving that policy and checking geometry clearance.

For similarly scaled finite-area sources and sphere fixtures, both source area
and squared distance scale together, so the reference radiance gain is unchanged;
the source's total emitted power changes with area. This does not apply to point
intensity. After the explicit fit, freeze the result: camera moves and robot poses
must not refit lights or alter normalization.

A future adapter must check all capability and scene preconditions, preserve
undo, retain the existing camera/exposure and material state, resolve actual
subject geometry, implement passive-card transport, and keep one energy owner
per physical emitter. Source meshes may make an analytic source visible but
must not emit a duplicate contribution. The rig is a single transaction; do not
apply the calibrated subset of a blocked rig silently.

No CAD source model, articulated SELENE state, optical-guide solution, full-scene
lighting render, photometric compliance or runtime installation was verified by
this authoring slice.
