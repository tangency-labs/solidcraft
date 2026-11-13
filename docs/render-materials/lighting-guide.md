# Lighting catalog and authoring presets — v0.2.0

## Scope and status

This package defines **280 concept lighting rigs** and **24 numeric authoring presets** for the personal render/materials development fork. It preserves the stable identifiers `LUZ-001` through `LUZ-200` from the original catalog and adds `LUZ-201` through `LUZ-280`.

The lighting catalog has status `concept_catalog`. Its entries describe intended arrangements, controls, requirements and future validation. The numeric studio package has status `specified_unbound`: positions, dimensions and source relationships are specified, but a SolidCraft adapter has not been implemented and these presets have not been rendered or calibrated. A capability reference describes a requirement, not proof that an executor supports it.

All recipes are original authoring specifications under provenance identifier `ORIGINAL-2026-10-CATALOG`. No commercial preset, HDRI, photometric profile, material asset or renderer implementation is embedded. Where a recipe needs IES/LDT data, measured emission or a specialized optical model, that resource must be supplied, licensed and validated separately.

## Catalog coverage

The 28 families each contain ten distinct entries:

| IDs | Family |
|---|---|
| LUZ-001–010 | Essential studio |
| LUZ-011–020 | Catalog and commerce |
| LUZ-021–030 | Metals and reflective surfaces |
| LUZ-031–040 | Glass and optical products |
| LUZ-041–050 | Translucent and soft materials |
| LUZ-051–060 | Plastics and matte finishes |
| LUZ-061–070 | Jewelry and watches |
| LUZ-071–080 | Electronics and small mechanisms |
| LUZ-081–090 | Industrial machinery |
| LUZ-091–100 | Furniture and domestic products |
| LUZ-101–110 | Vehicles and transport |
| LUZ-111–120 | Architectural daylight |
| LUZ-121–130 | Artificial architectural lighting |
| LUZ-131–140 | Solar and sky exteriors |
| LUZ-141–150 | Functional photometry |
| LUZ-151–160 | Visual inspection and validation |
| LUZ-161–170 | Macro and miniatures |
| LUZ-171–180 | Dramatic presentation |
| LUZ-181–190 | Assemblies and exhibitions |
| LUZ-191–200 | Specialized physical scenes |
| LUZ-201–210 | Automotive and mobility details |
| LUZ-211–220 | Industrial robotics and manufacturing |
| LUZ-221–230 | Electronics and additive fabrication |
| LUZ-231–240 | Woodworking and DIY |
| LUZ-241–250 | LED optics and luminaires |
| LUZ-251–260 | Aerospace and spacecraft integration |
| LUZ-261–270 | Physical lunar surface lighting |
| LUZ-271–280 | SELENE arm and sample-handling studies |

The costs `Low`, `Medium` and `High` are qualitative estimates of authoring and transport difficulty. They are not performance measurements. Priorities `P0`, `P1` and `P2` express product-development order and do not imply current availability.

## Ownership boundaries

A lighting rig owns its sources, source groups, source placement, light-target references, and explicitly created flags, reflectors or fixture diffusers. It does not silently edit the product material, CAD dimensions, arm pose, sample-vial contents, camera, tone mapping, output format or environment.

Controls such as “grazing angle to reveal texture” change the source. They do not change bump amplitude. “Backlight to reveal thickness” changes illumination; it does not change absorption or wall thickness. References to another inspector are contextual links. Applying a rig does not apply that inspector's settings.

An environment can own a sun, sky, Earth contribution, room, ground, dust or other scene context. A preset that requires one of these checks the existing state. It must not install a second equivalent source or silently replace environmental data. Choosing a different ownership policy is an explicit scene transaction with undo support.

A screen, LED, luminous panel or solar disk has **one energy owner**. If an analytical light has a visible mesh representation, the two representations must share a single emission contribution. The visible mesh must not add the same energy again. Passive cards only reflect or block incident light. No negative-energy light is used to implement a flag.

## Coordinates, units and fitting

The subject frame is right-handed, with **+Z up**, **−Y front** and **+X right**. Camera motion does not alter this frame. Internal lengths resolve to millimeters and numeric angle fields use radians.

`B` is the largest positive extent of the selected subject's bounding box. Lights, cards, floors, cameras and backgrounds are excluded. `position_b`, `target_b` and `size_b` contain dimensionless multiples of `B`; an adapter multiplies by the resolved millimeter value exactly once.

The ordinary origin is the selected subject's bounding-box center. Macro presets can use an explicit region. LED coupling presets instead require the named existing aperture or diffuser-plane center. A region or aperture that cannot be resolved is a blocking error, not an opportunity to guess its location.

Fitting happens once as an explicit authoring operation. The result records actual source dimensions and transforms. Moving the camera, changing a robot pose or switching a selected face does not trigger automatic refitting. Workcell examples use a fixed declared workspace envelope to avoid lighting changes between poses.

For a target-oriented area, its emitting front normal points toward `target_b`. Local up is the projection of world +Z into the light plane; when that projection is degenerate, use +Y. Apply `roll_rad` around the emitting normal. Width follows local right and height follows local up. Directional-sun vectors point along incoming photon travel and are unit length.

A numeric starting position is not a collision-free placement guarantee. Before authoring, check actual subject geometry, surrounding guards, optical openings and robot swept volumes. Sources must occupy free space appropriate to their role. A real internal LED is allowed inside a luminaire cavity, but never inside its opaque wall.

## Relative contribution and calibration

Studio lights use the quantity `reference_contribution_ev`. These values are **authoring ratios in a calibration scene**. They are not measured lumens, watts, candela, lux or a universal conversion between point lights and area emitters.

The proposed `CAL-LIGHT-001` scene contains a centered Lambertian sphere with radius `0.25B`, linear diffuse reflectance `0.18`, and `B = 1000 mm`. It has no floor, environment emission or additional source. The reference sphere is an idealized calibration object, not a claim about a measured commercial surface.

Measure each source separately using six fixed orthographic views along ±X, ±Y and ±Z. Compute the masked-sphere mean linear luminance for each view and average those six means. Keep camera geometry, sampling and scene version fixed. Use scene-referred linear values before display transforms. The reference fixture uses fixed 0 EV camera exposure; that is not an instruction to overwrite the user's camera.

Zero reference EV defines one arbitrary normalized response unit. A light at −1 EV targets half that reference signal; −2 EV targets one quarter. An adapter must solve and record the appropriate source gain. No such calibration has been performed for this package, and no gain values are invented here.

Resolve normalization at the applied physical scale, then preserve the resulting source state. Different source types have different native quantities; their numerical gains cannot be assumed interchangeable. Passive cards are excluded from per-source calibration and contribute through transport when the final rig is assembled.

For the explicitly editorial lunar fill, `CAL-ENV-SUN-001` first measures the existing environment sun in the same reference fixture. That signal becomes the zero-EV reference for the added fill. The sun remains unchanged. If the environment source or its reference signal cannot be resolved, block this preset.

The quantity `relative_solar_irradiance` is also dimensionless. A value of `1.0` is an illustrative reference, not a measured lunar irradiance. The included solar angular diameter `0.0093 rad` is marked approximate and illustrative. Solar direction is an authored input, not a mission ephemeris or a site-specific prediction. Neutral RGB solar color is not a measured spectrum.

## The 24 numeric presets

| Preset | Catalog | Mode | Purpose |
|---|---|---|---|
| STU-001 | LUZ-001 | Studio | One side softbox and a passive reflector |
| STU-002 | LUZ-003 | Studio | Key, fill and rim with independent contributions |
| STU-003 | LUZ-013 | Studio | White product separated by passive dark edge cards |
| STU-004 | LUZ-012 | Studio | Black housing defined by broad side reflections |
| STU-005 | LUZ-021 | Studio | Narrow reflection bands on polished metal |
| STU-006 | LUZ-022 | Studio | Grazing source for existing brushed-metal orientation |
| STU-007 | LUZ-031 | Studio | Bright-field glass with a rear panel and side flags |
| STU-008 | LUZ-032 | Studio | Dark-field glass with rear-side strips |
| STU-009 | LUZ-074 | Studio | External light around an already emissive display |
| STU-010 | LUZ-071 | Studio | Populated PCB overview and low relief light |
| STU-011 | LUZ-058 | Studio | Existing FDM layers revealed by a grazing strip |
| STU-012 | LUZ-232 | Studio | Selected woodworking-joint region |
| STU-013 | LUZ-233 | Studio | Consistent face-grain and end-grain comparison |
| STU-014 | LUZ-101 | Studio | Car roof and side reflection bands |
| STU-015 | LUZ-202 | Studio | Brake hardware lit through an actual wheel opening |
| STU-016 | LUZ-215 | Studio | Fixed robot workcell and target-tray lighting |
| STU-017 | LUZ-251 | Studio | Spacecraft integration stage, explicitly not orbital light |
| STU-018 | LUZ-241 | Studio | Finite LED strip behind an existing diffuser |
| STU-019 | LUZ-245 | Studio | Finite entrance source for an existing light guide |
| STU-020 | LUZ-261 | Lunar physical | One rig-owned illustrative solar source |
| STU-021 | LUZ-262 | Lunar physical | No added lights; existing environment sun |
| STU-022 | LUZ-268 | Lunar physical | Idealized zero-source night diagnostic |
| STU-023 | LUZ-271 | Lunar physical | No added lights; fixed-sun SELENE arm comparison |
| STU-024 | LUZ-277 | Lunar editorial | Declared fill for a SELENE sample-vial communication view |

Every preset keeps manual camera exposure fixed during comparisons. That requirement is checked separately from applying the lighting rig. The preset does not set exposure, white balance, tone mapping or the output image configuration.

## Physical lunar and editorial modes

`lunar_physical` adds no atmospheric sky illumination, atmospheric haze or unexplained ambient fill. Terrain, spacecraft surfaces and other real objects may still reflect light. Darkness or occlusion must not be “repaired” with an undisclosed softbox.

STU-020 owns its solar source and therefore conflicts with an already emissive environmental sun unless ownership is explicitly transferred. STU-021 and STU-023 intentionally have empty `lights` arrays: their sole solar source is already owned by the environment. They require a resolved source before proceeding.

STU-022 is a zero-source diagnostic idealization. It is not a model of every lunar night and does not supply Earthshine or starlight. A black background alone does not establish zero environment illumination. If a physically brighter night study is wanted, its source, visibility, radiance and ownership must be supplied explicitly.

`lunar_editorial` labels deliberate additional light. STU-024 introduces a finite fill area so that a sample vial and gripper can be read in a presentation. It preserves the environment sun and requires an A/B with fill disabled at identical exposure. Its result must never be described as an unassisted natural-light lunar view.

SELENE poses, arm geometry, mounts, sample contents and actions are external scene inputs. The catalog does not invent robot dimensions or motion planning. A sequence study compares supplied snapshots or animation states under one recorded lighting configuration. An occluded grip or vial interface is allowed to remain dark in physical mode.

## Capability checks and authoring workflow

1. Resolve the selected subject, region or aperture and its coordinate frame.
2. Read all `required_capabilities` for the numeric preset, together with the actual scene's material and environment requirements.
3. Confirm one energy owner for each emitter, especially displays, LED representations and the sun.
4. Verify external dependencies and source clearance. Confirm manual exposure is locked for comparison.
5. Resolve normalized coordinates, source normalization and modifier geometry into a reviewable scene transaction.
6. Apply that transaction only when all checks pass. Preserve undo and the prior scene variant.
7. Render a small region first, retain an undenoised linear reference and record all resolved inputs.

If a required capability is unavailable, keep the scene unchanged and report its exact `CAP-...` identifier. Do not silently replace a finite emitter with a point, remove glass thickness, erase caustics, approximate a light guide with an emissive exit or add atmospheric fill. An explicitly chosen lower-fidelity alternative can be a separate, clearly labeled preset.

The validation arrays are future acceptance checks. They do not assert that calibration, renderer parity, image quality, optical engineering, fixture compliance, solder quality, robot safety or material performance has been established.
