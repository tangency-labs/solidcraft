# Native Render studio UI

Integration-branch source, continuing the frozen personal renderer fork. Historical layout acceptance is retained in `render-materials/UI-BATCH-2026-10-22.md`. Current transport, aging/effects, Backgrounds-page and final showcase evidence is linked from `render-materials/AGING-EFFECTS-SHOWCASE-2026-10-23.md` and the shipped showcase manifest; this guide alone is not proof of acceptance.

## Workspace and navigation

RENDER remains one ribbon tab. The existing Browser card has a local Model / Library switch in both Docked and Floating layouts. The native model tree, visibility and selection are not replaced by a competing scene tree. The central canvas retains the original product-navigation and completed-image tools.

The right inspector has eleven local sections: Scene, Appearance, Aging, Lighting, Environment, Backgrounds, Camera, Effects, Output, Diagnostics and All controls. These are sections of one saved render draft, not rendering modes. All controls retains the dense combined native editor; specialized ordered Aging/Effects and Backgrounds workflows also have their dedicated pages. They share execution/validation rather than inventing a simplified rendering path. Each open document retains its own page and library state.

Check, Render, Cancel and Export remain outside the scrolling inspector body. They use the existing immutable job protocol. A later draft edit does not substitute a different image for a retained completed export. PNG export copies the completed native frame, not the reduced UI display texture. Check and Start use the same request builder; inspection does not silently start a renderer or open a GPU.

## Resource library

The native library includes original surface maps, SELENE appearances, HDR environments, authored rigs, physical-reference profiles, advanced scene/material/light/environment concepts, quality specifications, capability requirements and the twelve authored SELENE shot specifications. Twenty-five separate `NATIVE-STUDIO-*` entries cover all twenty-four original authored HDR resources as executable compositions; they do not relabel the unbound `SCENE-*` concepts as finished.

Category-specific search, page and selection survive switching categories. Favorites are bounded at 256 entries and recent inspection at 32; these are per-open-document session state, not an on-disk shared library. Search, availability filters and collection inspection are read-only. Physical profiles remain engineering references, never optical substitutes. Resource details retain original values, omissions, source locators and provenance.

An optional source preview decodes the bounded, hash-verified original base-color PNG. It is labelled **source map**, not a measured BRDF swatch or a path-traced thumbnail. Every surfaced projection remains explicit: uniform optical appearance, planar authored maps, embedded HDR, complete rig or acknowledged emitter-only variant. Physical texture scale and XY / YZ / ZX mapping feed the native validator. No height displacement is implied by a normal map.

## Native scene composition

`render.preset.resolve` accepts a native studio identity, saved setup name, optional current typed GPU setup patch and optional expected revision. It resolves original lighting/HDR/passive ground and validates conservative clearance without changing the document. Its response identifies replacements (lighting, environment, stage and exposure), preserved fields and limitations.

`render.preset.apply` requires the reviewed revision, explicit lighting/environment/exposure consent, and stage-replacement consent when needed. It reconstructs and validates the whole candidate before publishing one undoable render-settings change. Stale reviews, empty subjects, unsupported methods, existing-stage consent bypasses and unbound SCENE identities refuse atomically. Camera, dimensions, sampling, optical bindings, CAD geometry, engineering materials, pose and visibility are preserved. The three-light composition's 1/16 radiance multiplier is a creative variant, not the original measured/calibrated reference.

The scene inspector also fits a finite render-only ground with explicit margin, gap and linear reflectance. Passive panels do not create engineering mass. Conservative AABB separation is not exact B-rep collision or physical contact certification.

## Shot lifecycle and unsaved edits

Saved shots can be duplicated, renamed and removed through `render.shot.duplicate`, `render.shot.rename` and `render.shot.remove`. Each uses an expected revision, validates names and rejects destination conflicts. Resources and captured jobs are retained; no file deletion or camera movement is implied. Mutations use one document undo.

Changing or reloading a shot with an edited draft opens Save and continue / Discard draft and continue / Keep editing. Removal has a separate confirmation. A review cannot be reused after the document revision or identity changes. A failed save preserves the draft rather than continuing with lost data.

## Optical, light and camera controls

The Appearance section provides an explicitly applied opaque body editor and the existing exact-target glass, clear-coat and four-channel map controls. Body color is sRGB; roughness, metallic fraction and optical IOR are not engineering constants. The reviewed target is revision/document checked. Existing face-specific appearances and physical material assignments remain separate. Temporary optical overrides retain their existing portability limitations and cannot masquerade as saved intent.

GPU illumination exposes analytic area/sun lighting and the shared environment controls. CPU controls expose directional and point/spot lights plus bounded rectangular emitters. CPU rectangle vectors are **full sides**, while GPU vectors are **half edges**; labels and native request schemas preserve the distinction. Sky zenith/horizon/ground are linear RGB. Display background is not an extra light source.

The Camera section edits the shared native camera via public commands, with millimeter position/distance, degree-displayed angles and explicit perspective FOV. Thin-lens controls remain GPU-specific. Navigation, camera fit and restore are not reimplemented as private mouse gestures. Unedited numeric values do not change merely because an editor is shown.

## Output and capability honesty

Output dimensions follow the selected CPU/GPU/raster envelope. Named size buttons are explicit edits; invalid combinations stay visible for correction rather than being silently reduced. GPU sample budgets, path depth, seed, tone mapping and denoising preserve the validated numerical contract. Optional independent-pilot adaptive allocation discards pilot radiance before production; its error targets are not a certified noise bound. Conditional Fresnel strata are explicit extra path work. CPU resource and sampling profiles remain explicit. Completed PNG supports RGBA8/sRGB; optional native uncompressed FLOAT32 RGBA linear-sRGB/D65 EXR is retained before denoising/exposure/display filters and exported from the exact completed job.

`render.quality.prepare` resolves an acknowledged `fixed_samples_png` projection of a Q specification. It returns only compatible dimensions, fixed sample count, supported maximum depth, seed and denoise intent. That legacy `fixed_samples_png` projection lists adaptive rules, independently authored transmission-depth/Russian-roulette rules, raw-linear/EXR intent and convergence guarantees as omitted by the projection. This is not a statement that the separately validated native EXR or independent-pilot controls are unavailable. It refuses excessive profiles rather than clamping them. The UI changes only those budget fields, preserving executor, camera, lighting, optics, exposure and stage.

Diagnostics exposes the exact current request, last preflight, logical-memory estimate, captured-job metadata, scene counts and structured errors. Stale checks are identified. The native producer has bounded independent-pilot allocation, conditional transmission strata, homogeneous participating media and immutable-master regrading; advanced-optics capabilities have their own explicit admission limits. Certified convergence, arbitrary density grids, unrestricted nested optical media, a full spectral pipeline, every unbound material concept and production queues are not implied. Seeing a concept in the library is not runtime admission.

## Backgrounds and visibility

The Backgrounds ribbon opens one dedicated page with camera-only colour/gradient controls, physical stage actions, an exact retained-master regrade action and a separate HDR-illumination link. Merely opening it is read-only. Camera backgrounds show through uncovered pixels; they neither illuminate nor replace captured geometry. A large physical ground or wall can hide them completely. A completed result is immutable, and Live Preview is the modelling view: render or regrade explicitly to see final camera presentation.

The physical floor/wall has two panels. The native cyclorama has a floor, twenty-four polygonal cove strips and a wall; it receives/casts supported native lighting interactions but is not an analytic smooth or infinite studio. Replacement consent remains explicit. Clearing a stage forwards the entire current numerical draft so unrelated unsaved background, exposure or output edits are preserved, then saves one ordinary command result. HDR resources illuminate and reflect but are not automatically displayed as a panorama behind the subject. Transparency is never silently flattened.

The selected final showcase includes neutral/gradient comparisons sharing exactly the same EXR, floor/wall versus cyclorama native renders, and a capture of this actual Backgrounds page. The thirteen saved production/background shots reopen identically; the video is edited native imagery, not real-time performance evidence.

## Layered aging and nondestructive finishing

Aging offers up to eight ordered authored optical layers with bounded exact targets, world-millimetre scale, seed, masks and optical response. Rust, patina, deposited dust, grime, scratches, chipped paint and fading do not change CAD geometry, physical material cards, mass or pose. They are not corrosion-time, curvature/cavity or geometric-displacement simulations. Passive stages and glass are excluded. Airborne dust is a separate finite homogeneous transport medium with its own compatibility checks, not a surface layer or a fog bitmap.

Effects offers up to eight ordered original colour looks plus bounded image-space photographic controls. Highlight-driven flare/ghosts/halo/streaks, chromatic offsets, radial distortion, soft focus, sharpening and deterministic grain are not calibrated optical lenses or imported commercial film LUTs. They preserve raw EXR bytes. `render.result.regrade` produces a distinct cancellable PNG job from an exact retained native master without new rays or document edits; it does not reconstruct a master from PNG or reapply absent denoiser guides.

## Verification

Focused regression sources cover atomic native composition, saved-shot conflicts and undo, quality projection refusals, original source-preview identity, per-category navigation, bounded recent inspection, context-page nonmutation, sticky job actions in both layouts/themes, draft confirmation, stale reviews and real UI composition application. Existing dense-inspector interaction tests explicitly select the public All controls section and retain their numerical assertions. They exercise the same production request/actions as the contextual inspector.

Actual application screenshots must be captured from an isolated native application session using a public-safe model fixture, in both themes/layouts, after the coordinated batch build. Review images/logs belong outside production assets. The existing native SELENE master and selected 4K image remain unchanged by this UI work.


### Isolated native review profile

The desktop application accepts `SOLIDCRAFT_SETTINGS_DIR` as an explicit absolute directory for its settings, graphics preference and startup marker. Invalid/relative/file paths refuse before startup; an explicit profile never adopts the ordinary or legacy settings. Without the variable, normal behavior is unchanged. This is profile isolation, not a filesystem sandbox. Set the existing recovery/crash/control directory overrides separately when running a disposable native review. It allows the actual hardware application to be exercised without rewriting personal preferences or substituting safe-mode/software behavior.


## Full-detail live preview and bounded refusal

The live preview preserves all captured visible product triangles. Expanded PBR/legacy/glass vertex bytes are counted before allocating output buffers, which are reserved once and uploaded through the existing device-sized chunk path. Native 64-bit builds admit at most 512 MiB of vertex payload; web/restricted targets retain 128 MiB. These are payload bounds, not promises about total GPU memory, retained CAD history or runtime speed. Output image jobs retain their separate CPU/GPU envelopes.

The Diagnostics page shows the actual admitted live-preview payload. A refused capture is cached, not rebuilt on every repaint. An explicit Retry live preview action or a changed captured scene rechecks it. The shared-kit wrapped error card remains legible independently of the user-selected scene background, and leaves shot editing and image-job controls available. This display remains the fixed-light product preview, not evidence that an authored HDR/material effect has been path traced.

Closing a complex native document must not consume a timeline-sized call stack. Naming ancestry and placed-copy ownership are reclaimed iteratively only when uniquely owned; live shared snapshots retain their geometry and naming identity. The full-master close/quit check is a separate native acceptance step, not inferred from layout screenshots.
