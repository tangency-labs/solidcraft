//! CPU render jobs can save optional `options.output` presentation: tone map, finishing,
//! backdrop and raw linear EXR retention. Older designs omit it and retain the exact previous
//! f64 PNG output; exposure remains owned by RenderSetup. Readers missing these changes open
//! the newer design with a warning and preserve saved values they cannot read.
//! CPU jobs also save optional `options.lens` using the shared physical thin-lens
//! aperture radius and axial focus distance. Positive aperture requires perspective
//! plus explicit progressive camera sampling; absent/zero aperture keeps old pixels.
//! CPU raytrace jobs also retain default-false `transparent_background` beside options.
//! It exports straight-alpha primary geometry coverage; absent fields keep opaque PNGs.
//! CPU jobs also save opt-in `options.denoise`; omission/false preserves the prior
//! raw PNG calculation. Every complete published pass is filtered without changing
//! raw film or the final unfiltered EXR. Older files need no rewrite for this flag.
//! CPU jobs also preserve optional `options.path: {max_depth: 1..16}` for explicit
//! progressive diffuse/specular transport and certified Glass boundaries. The depth
//! counts shaded surfaces, optical boundaries and active scattering events. Traced shadows are required;
//! selected reflection/anisotropy overrides and unsupported closed-Glass
//! shadow connections refuse with authored intent retained. Omission keeps the legacy producer.
//!
//! CPU raytrace jobs save optional `options.aging` using the existing ordered
//! SavedAgingStack schema and stable definition/feature/component/occurrence targets.
//! Empty/absent stacks preserve prior saved JSON, CPU pixel arithmetic and work.
//! Check and Start resolve stable targets against the captured scene; stale/hidden
//! targets are refused rather than made global. Captured indices/names remain
//! runtime-only. Aging changes the opaque BRDF before lighting and denoising guides;
//! it does not change geometry, emission, physical materials or mass. Certified
//! glass targets and passive stages remain excluded; independently authored anisotropic axes
//! cannot be replaced by scalar aging roughness and are refused explicitly.
//!
//! Saves up to eight hard GPU directional sources alongside the existing optional
//! finite Sun (eight combined). Older designs omit the new list and retain their
//! exact implicit studio or finite-Sun lighting. Readers missing the new list warn
//! about the newer design and preserve these unread sources.
//! Also saves up to eight GPU point/spot sources with exact world-mm positions,
//! reference/minimum distances and optional world-axis/radian half cones. Missing
//! punctual lists stay empty. CPU cone intent retains its wider f64 domain; GPU
//! cone feathers below the documented precision bound refuse without rewriting.
//!
//! CPU jobs may also consume the existing portable saved `surfaces` bindings for
//! opaque built-in base color, roughness, metallic and normal maps. No new map
//! schema is added; raster and unsupported CPU optical combinations refuse the
//! authored bindings instead of dropping them. Old files without maps keep the
//! old CPU allocation/work/pixel path.
//! CPU jobs also save optional `options.adaptive` using the existing independent-pilot
//! policy. Explicit progressive production sampling is required; the unchanged CPU
//! profile budgets pilots plus the production cap. Exact per-pixel counts and pilot
//! statistics stay runtime-only; final SamplingReport and PreviewInfo.adaptive reuse
//! the existing metadata contract without changing linear-master schema/channels.

//! CPU `options.path` may consume the existing saved ClosedMesh optical boundary
//! after exact f64 edge, vertex-link, embedding/contact, outward and conservative
//! nested-parent certification. Runtime-only ClosedMesh and thin-shadow lowering
//! preserve stable authored optics. No new saved field or geometry repair is added.
//! Absent-path sheet/box transport and default serialized bytes remain unchanged.
//! Closed-shadow connections, Glass combined with external volume and selected
//! reflection/anisotropy combinations refuse explicitly with authored values retained.
//! Perspective CPU Path captures classify each physical eye/pupil cooperatively
//! against the certified boundaries and initialize the nested Beer medium stack.
//! Exact origin/near-clip boundary contacts refuse without changing saved cameras
//! or optics. This runtime-only extension adds no serialized field.
//! Existing saved OpticalMaterial.subsurface may also lower to optional captured
//! CPU dielectric subsurface parameters with explicit progressive Path. Selected
//! certified closed media use scalar free flight, HG directions and separate Beer
//! absorption, including physical eye/pupil starts and nested current media. No
//! refracting-boundary next-event approximation is added. Active scattering and
//! surface denoising refuse together; unsupported thin/legacy routes retain intent.
//! Some(rate0) remains serialized but adds no collision work/RNG or pixel change.
//! CPU jobs may also preserve optional options.volume using the same bounded
//! HomogeneousVolume world-box/extinction/RGB albedo/HG contract as GPU Atmosphere.
//! Positive extinction requires explicit progressive Path and disables no existing
//! option implicitly: Glass/SSS and surface denoising refuse with intent retained.
//! Absent/zero volume keeps the original transport work/RNG/heap owner count/pixels.
//! Inline intent's actual fixed retained-request size is prepaid even when absent;
//! regrade owns its separate header reservation until its last status handle drops.
//! Physical eye/pupil prefixes and the orthographic signed line/box union do not
//! alter saved camera/framing/model bounds or its model near interval. Existing
//! ray-distance/profile/task/frame/codec/host caps remain unchanged.

//! CPU jobs also retain optional `sun` and `sky` intent. `sun` reuses the shared
//! finite-cap direction/angular-radius/radiance domain; `sky` reuses the portable
//! gradient/rotation/intensity schema. Both require explicit CPU Path transport.
//! Omitted fields preserve old designs and the absent producer allocation/work/RNG
//! path. Constant, Sky and HDR sources are exclusive; finite Sun is additive.
//! No generated sky image or runtime resource is serialized.
//!
//! CPU and GPU jobs also save `coats`, up to the existing 64 runtime coat target
//! bound, with stable definition/feature/component/occurrence identities and exact
//! weight, roughness and dielectric IOR. Empty/absent lists keep prior JSON and
//! capture behavior. Coats require opaque targets; even a zero-weight authored coat
//! cannot share a target with Glass. Capture resolves targets without changing
//! appearances, engineering materials or geometry. Unsupported CPU selected
//! reflection/anisotropy overrides refuse instead of dropping the coat.
