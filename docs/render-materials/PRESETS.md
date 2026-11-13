# Presets: quality, studio, environment and complete scenes

## Four independent recipe types

| Recipe | Owns | Must preserve unless explicitly included |
|---|---|---|
| Quality (`Q-*`) | Resolution, integrator intent, convergence budget, image output | Appearance, rig, environment, camera pose and CAD geometry |
| Studio (`STU-*`) | Sources, passive cards, flags and source normalization | Engineering properties, model scale, output quality and camera |
| Environment (`ENVSET-*`) | Stage geometry, ground, ambient field, sky/Sun policy | Subject geometry, engineering material and locked shot fields |
| Scene (`SCENE-*`) | A reviewed composition of the above plus material roles | User locks and unrelated document state |

All four are **future specifications** with status `specified_unbound`. Numeric values are original starting points for calibration. They are not accepted runtime JSON commands, measured photometry or a guarantee that the current executor can render them.

`baseline-setup-commands.json` is deliberately a different file and shape: it contains six examples of the currently documented `render.settings.set` command. Those examples store builtin preview dimensions, background and documented exposure/environment fields. They omit the camera to preserve it. They were checked against the published contract, not executed in an application in this contribution. A saved field must not be assumed to affect shading unless the selected backend implements it.

## Quality targets

The sample columns are minimum and maximum samples per pixel for a future path-tracing adapter. Error targets refer to a versioned relative standard-error estimator on scene-linear luminance with an explicitly defined dark-signal floor. The adapter must bind the estimator before use; these numbers are not interchangeable with another renderer's “noise” slider.

| ID | Name | Resolution | Samples min–max | Relative error target | Total / transmission depth | Denoising |
|---|---|---|---|---|---|---|
| Q-001 | Interactive Draft | 960 × 540 | 8–64 | 0.12 | 4 / 2 | Off |
| Q-002 | Balanced Product | 1600 × 900 | 32–512 | 0.035 | 8 / 6 | On, retain raw |
| Q-003 | Product Portfolio | 2560 × 1440 | 64–2048 | 0.015 | 12 / 10 | On, retain raw |
| Q-004 | Glass and Layers | 2560 × 1440 | 128–4096 | 0.010 | 20 / 16 | On, retain raw |
| Q-005 | LED Optical Study | 2560 × 1440 | 128–4096 | 0.010 | 20 / 16 | Off |
| Q-006 | Lunar Hero Master | 3840 × 2160 | 256–8192 | 0.005 | 24 / 20 | Off |
| Q-007 | Lunar Sample Macro | 3072 × 3072 | 256–8192 | 0.005 | 24 / 20 | Off |
| Q-008 | Animation Preview | 1280 × 720 | 16–256 | 0.060 | 8 / 6 | Off |
| Q-009 | Animation Final | 3840 × 2160 | 128–4096 | 0.012 | 16 / 12 | Off |
| Q-010 | Reference Convergence | 2048 × 2048 | 1024–16384 | 0.002 | 32 / 24 | Off |

Q-006 and Q-007 are initial maximum-quality demonstration targets with bounded work. Hitting the maximum sample count before convergence must produce an **unconverged** result and diagnostics, not a “maximum quality complete” badge. Finite path depth is still a truncation; compensated Russian roulette does not remove that separate approximation. A higher-resolution output needs a resource admission check and potentially tiled rendering.

Q-008 and Q-009 request motion blur from an approved bake, using a half-frame shutter centered on the frame time and initial temporal stratification counts of 8 and 32 respectively. Each camera path samples one time within the shutter; the strata count does not multiply the per-pixel path budget. Bind a positive finite shot frame rate before execution: shutter duration is `0.5 / frame_rate_hz` seconds. Physics solver substeps are independent of both quantities. Temporal denoising is not required by these profiles; any later enablement requires a motion-stability comparison against raw frames.

No target makes a frame-rate or render-time promise. Report executor, version, device, memory, scene complexity, preparation time, elapsed render time and remaining error. Quality profiles requiring EXR depend on an actual linear encoder; the current contract's ability to store the string `exr` is insufficient.

## Coordinates and scale

Studio rigs use a selected subject's maximum positive bounding-box extent **B**. Their `position_b`, `target_b` and `size_b` values are dimensionless multiples of B. Resolve these once to millimeters after an explicit fit action. The source origin is the subject center unless a preset explicitly uses an optical aperture. +Z is up, -Y is front and +X is right. Camera motion never rotates a fixed rig. Animated arm poses must not refit lights every frame.

Environment geometry has explicit millimeter dimensions. An environment may reposition a support or reframe a camera on request; it cannot silently rescale the subject or stretch a room. If the subject exceeds the preset's declared clearance, preflight offers a different stage or an explicit stage resize. Texture scale also stays physical: a 0.2 mm print layer must not turn into a 20 mm ridge when a part grows.

All stored angles use radians. UI labels may display degrees using a reversible conversion. Sun direction is a direction at effectively infinite distance, separate from a nearby area-light position.

## Source normalization and color

Studio source intensities use `reference_contribution_ev` against `CAL-LIGHT-001`, whose definition is in [lighting-guide.md](lighting-guide.md). This is an authoring calibration convention, not a claim of measured lumens, candela or watts. The runtime adapter must resolve it for source type, area, distance, spectral/color assumptions and applied scale, then store the resolved physical units. Do not equate identical numeric values across point, area and emissive-mesh sources.

Source RGB values and passive-card reflectances declare a linear working space. A neutral RGB emitter is not automatically a specified blackbody spectrum. Spectral LED studies need an independently sourced spectral distribution; RGB appearance alone cannot establish optical efficiency, phosphor behavior or spectral dispersion.

The scene-linear output, exposure, view transform and output encoding remain distinct. Bloom is a camera treatment and does not illuminate the scene. A visible emissive surface and an analytic sampling representation share one energy owner. Any proxy must be an implementation of that contribution, not an additional lamp.

## Composition and ownership

The resolver proceeds in this order:

1. Resolve resource versions and hashes, subject binding and explicit material roles.
2. Validate units, geometry clearance, stage size, required capabilities and memory admission.
3. Combine owners for Sun, ambient field, subject emitters and source proxies.
4. Resolve explicit composition overrides, inherited values and user locks; show conflicts before mutation.
5. Produce an immutable request with source and resolved values, then apply atomically through one engine command.

The supplied studio scene combinations explicitly set `environment_overrides.ambient_scale` to zero because their studio ratios assume no unaccounted ambient source. They also use scene geometry for the visible stage background. These are visible composition choices, not hidden alterations performed by the lighting rig. An environment used on its own retains its declared ambient field.

Physical lunar combinations use the environment-owned Sun with a pass-through rig. `STU-020` is an alternative single-Sun owner for separately authored scenes; it cannot be merged with another enabled Sun. `STU-022` intentionally requires a zero-source night audit. The supplied full-shadow environment instead retains a Sun behind occluding terrain; do not confuse absence of direct illumination at the subject with removal of the Sun.

Physical lunar recipes have no atmospheric scattering or artificial sky dome. Reflection from terrain, rover surfaces, a properly resolved Earth and actual hardware lights may illuminate shadows when included. An editorial fill, star overlay or decoupled background changes the scene's mode and saved fidelity label. A nearly black physical result is a valid diagnostic outcome, not an invitation to secretly add fill.

## Material role binding

A scene names roles such as `rover_structure`, `vial_glass`, `lunar_mare_regolith` or `pcb_solder_mask`, then maps them to catalog IDs. A material ID resolves to a concept at this stage; before application it must resolve further to a versioned authored resource. Never guess face assignments solely from a fuzzy body-name match.

SELENE's inspected component and body-name references are stored in `selene-bindings.json`. Validate the native-file hash and the expected occurrence graph before offering automatic assignment. Unmatched or ambiguous geometry produces a selection task for the user; it does not receive a guessed appearance.

## Current-contract setup examples

The six examples are Draft 720p, Presentation 1080p, Square Detail, Portrait Product, Technical Review and Dark Detail. They remain within the published 4096-per-axis and 4,194,304-total-pixel limits and use `backend: builtin`, `mode: preview`, `format: png`. They introduce six named setups and may replace an existing setup with the same name, following the command's behavior. The caller must inspect the existing setup count and reject a combined total above the documented maximum of 32.

Do not send the larger Q-006, Q-007 or Q-009 resolutions through the current settings examples. Do not pass quality, rig or environment recipe objects to `render.settings.set`. Future integration needs typed contracts and capability-aware adapters first.
