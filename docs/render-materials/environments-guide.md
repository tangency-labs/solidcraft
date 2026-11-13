# Environment catalog and authoring drafts, version 0.2.0

## Scope and status

This is original concept-authoring work for the personal SolidCraft fork. It contains no competitor assets or source code, downloaded space imagery, proprietary facility scans, or copied shader implementations. The catalog has 280 entries in 28 families. `ENV-001` through `ENV-200` retain the meaning and order of the earlier proposals; `ENV-201` through `ENV-280` add automotive facilities, electronics and DIY benches, LED laboratories, aerospace integration contexts, orbital settings and lunar/SELENE work stages.

`environments.json` is a `concept_catalog`. Each `concept` item describes a desired environment; it is not an implemented renderer asset. `environment-presets.json` contains 24 numeric `specified_unbound` drafts. Their dimensions, relative light scales, angles, seeds and layout values are authored starting points. They are not measurements. Calibration, material-role resolution, geometry generation, renderer binding and visual validation remain pending.

The common provenance record is `ORIGINAL-2026-10-CATALOG`. It denotes original catalog authorship, not a grant of rights in any future imported texture, HDRI, photometry file or real spacecraft model. Those dependencies need their own provenance and permitted-use records when selected.

## Components and ownership

An environment is composed of ambient illumination, reflected surroundings, visible background, ground and nearby geometry. A lighting rig and the subject are separate owners. Replacing an environment must preserve the subject, its engineering material, its dimensions, the selected lighting rig and a locked camera unless an explicit whole-scene recipe says otherwise.

Each numeric draft has at most one sun, owned by `environment`. `ambient.sun_disk_in_environment` is false, so its solar contribution is not also baked into the ambient source. A compatible daylight rig links to that sun or explicitly replaces it in one reversible transaction. It must not add a second solar emitter silently. A product LED belongs to `subject`; an analytical substitute and an emissive mesh must represent one physical source rather than add their energy twice. Portals and passive reflectors never emit on their own.

An Earth image, skyline or other LDR background is not a calibrated light source. A future HDRI must preserve its color transform, orientation, range, provenance and any calibration. Saving LDR imagery as EXR does not recover missing radiance. Optional AI imagery can support concept composition or a declared backdrop, but it does not establish HDR, geometry, physical scale or astronomical accuracy.

## Units, frames and scale

All numeric geometry lengths are in millimeters. Angular fields use radians. The local frame has +Z up, -Y front and +X right. Sun azimuth runs from +X toward +Y about +Z; elevation is above the XY plane. The specified angular diameter is a single authored solar-disk draft value, not an ephemeris calculation. Relative irradiance and radiance scales do not claim watts, lumens, lux, candela or a measured exposure.

`geometry.parameters_mm` contains explicit generator dimensions. Seeds make an eventual procedural implementation repeatable; the generator implementation and version must be recorded when bound. Macro details must be resolved in physical units rather than stretched to fill the image. Topography changes should alter actual geometry when they affect silhouette, shadow or contact.

Stage fitting occurs only on an explicit request and uses an explicitly selected subject. The allowed scope is support placement and camera framing. It never scales the CAD subject or continuously readjusts after a part moves. Room dimensions remain independent. Maximum subject extents are proposed admission limits, not a guarantee that every elongated or articulated model fits. A degenerate selection, incompatible size, closed enclosure or insufficient clearance needs an actionable response. Tabletop presets have explicit support-plane elevation above the world floor.

## Capability dependencies

Catalog `capability_requirements` and preset `required_capabilities` reference the existing `CAP-001` through `CAP-104` planning registry. These references are requirements, not a statement that SolidCraft currently supports them. A preset stays unavailable until every required capability and symbolic binding is satisfied. Optional refinements, such as substituting a captured HDRI for a procedural sky, must not silently turn into mandatory requirements for the simpler path.

Typical foundations are light transport, Fresnel and surface roughness, physical texture scale, geometric stages, ground contact, color management and reproducible seeds. A procedural sky needs its declared sky capability. Terrain displacement, detailed vegetation and granular topography need their own geometry and memory paths. A volumetric terrestrial fog entry requires participating-media support. A vacuum scene must never activate atmospheric features merely because the engine offers them.

Rendering capabilities do not supply a physics solver. Authored wheel tracks do not prove traction, terrain slopes do not certify traversability, a thermal-vacuum chamber model does not simulate temperatures, and an optical-bench context does not produce certified photometry. Any later calculation or mechanics subsystem needs its own verified model, units and validation.

## Modes

### Terrestrial

Indoor drafts use separately managed lights and a declared low ambient contribution. Outdoor drafts may use procedural sky illumination, with the solar disk represented once. `atmosphere.model = procedural_sky_only` describes the background/sky model; it does not automatically add a participating fog volume. Rain, moisture, snow and surface wetness remain explicit optional scene components.

### Lunar physical

The lunar drafts set ambient sky radiance to zero and atmosphere to none. There is no blue sky, terrestrial haze, rain, wind or atmospheric smoke. Ground and neighboring geometry can still reflect sunlight into nominally shaded regions; that contribution is computed by transport and is not a hidden fill slider. Unlit areas may be dark, particularly when all hardware lights are off. The renderer must not brighten them automatically to make the image look promotional.

The low-sun and high-sun mare drafts are separate contexts. The high-sun draft is not a south-pole claim. Polar low-sun scenes are authored lighting states, not proof of continuous illumination, permanent shadow or the duration of a particular interval at an actual location.

In the LED shadow-work draft the sun remains active. An explicit nearby occluder must block direct sunlight at the selected work patch. The task LED is a subject-owned unresolved binding; the environment does not invent a fill source. The full-shadow draft uses a below-horizon solar direction and closed ground/horizon geometry. Earth, stars and hardware lights default off, so a nearly black image is expected rather than a failure requiring automatic exposure compensation.

All lunar topography here is procedural and conceptual. No entry is an exact landing site, measured elevation model or mineralogical sample. A real terrain dataset requires its own source, coordinate frame, scale, resolution, registration and license. SELENE names describe project-level operation stages and do not import any third-party mission geometry or assert actual operations.

Dust is either a surface appearance or explicitly modeled particles. The ballistic-dust catalog entry requires a documented initial state and particle trajectory model. It must not be implemented as a wind-driven fog volume. The 24 numeric drafts do not currently include an automatic dust emitter or a validated particle-dynamics solver.

### Space physical

The free-flight spacecraft draft has no ground. Its reference extents are non-rendered framing guides, not a large enclosure. It uses a single sun and zero sky radiance. Astronomical distances, body size and real trajectories are absent unless supplied through a separately reviewed geometry and ephemeris binding.

Earth is off by default in every space and lunar draft. Visibility requires location or trajectory, line of sight, ephemeris and coherent exposure. It cannot be made into an arbitrary oversized disk. Likewise, a sunlit spacecraft or lunar surface must not acquire a dense star overlay automatically. The initial drafts disable stars; a later exposure-aware astronomical background is a separate feature.

### Editorial

Transparent compositing backgrounds and the lunar promotional-fill stage are explicitly editorial. The lunar editorial draft has a nonzero declared uniform fill to improve product readability while retaining a black background. That is an artistic adjustment and must not be relabeled `lunar_physical`. The inspector and saved scene must preserve the mode and show the modified contribution. Disabling the fill is a reversible operation, not a hidden preset conversion.

## Symbolic bindings

Ground `appearance_role` values are symbolic and must be resolved to versioned material assets by the integration layer. They are not stable engine material IDs. The current roles are:

- `neutral_gray_matte`
- `white_diffuse`
- `black_matte`
- `light_wood`
- `esd_mat_visual`
- `workbench_laminate`
- `neutral_satin_floor`
- `optical_black_matte`
- `cleanroom_floor_visual`
- `lunar_mare_regolith`
- `lunar_highland_regolith`
- `lunar_highland_rock` for optional terrain rocks

The LED draft additionally needs the `rover_task_led` subject-emitter binding. The transfer draft needs `selene_container_transfer_station` bound to actual project CAD. Unresolved roles prevent a ready/validated status. A disabled ground in the free-flight draft does not require a visible floor to be manufactured simply because a fallback appearance role is recorded.

## Review gates before a draft becomes usable

1. Validate schema, finite values, bounds, IDs, capability references and binding resolution.
2. Preserve selected-subject scale, room dimensions, saved cameras and unrelated light groups across apply, undo, save and reopen.
3. Verify ground contact and generator dimensions on small, medium and large subjects, including a long thin part and an articulated assembly.
4. Check solar ownership and visibility rays, particularly rim, boulder, ridge and full-shadow scenes.
5. Compare diffuse gray, conductor and thick-glass references where the scene supports them. Keep a common exposure and declared color transform.
6. Test zero-atmosphere and zero-sky invariants for physical vacuum scenes. Confirm that editorial fill stays identified.
7. Test performance, detail levels and memory limits on the intended devices. No cost label here is a benchmark.
8. Record a validated generator version, seed, appearance versions, source inventory and reproducible reference images before assigning a shipped status.

These drafts intentionally stop before publication as renderer-ready assets. The next integration step is to bind their finite authored values to real commands and generators, resolve dependencies and capture evidence of the resulting images.
