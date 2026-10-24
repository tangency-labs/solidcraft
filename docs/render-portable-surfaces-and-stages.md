# Portable built-in surfaces and passive render stages

Integration-branch implementation, not a release acceptance claim. Source, type/lint checks, regression execution and actual application acceptance are recorded separately in `render-materials/BATCH-2026-10-22.md`.

## Saved surfaces

`RenderSetup.surfaces` stores at most 128 built-in surface bindings: a `SURF-nnn` ID, resource version, four encoded SHA-256 receipts, an exact definition/producing-feature/component/occurrence-path target, a definition-local planar frame in millimetres, and normal strength. The roles are base color, OpenGL normal, roughness and metallic. Height remains authoring-only. Existing body IOR is retained. Engineering material cards, mass, topology and the CAD timeline are unchanged.

No captured body ordinal, GPU object or external path is saved. Every Check and Start resolves the semantic target, transforms its local frame into current world coordinates, then produces the ordinary bounded GPU texture request. Motion follows that transform; another instance does not inherit the assignment. Missing, hidden, ambiguous, renamed or replaced definitions/features refuse instead of selecting a similar body. An occurrence label may change while its ID path remains valid. Definition renaming and topology-producing feature changes currently require explicit rebinding.

Bindings are path-independent **for the pinned built-in pack**, not a general embedded external-texture format. Reopening preserves references, but execution refuses an unavailable resource or version/digest mismatch. A future installed pack must still supply those exact bytes; an old ID must never be silently reinterpreted.

Send the following to `render.library.surface.bind` after inspecting `render.scene`:

```json
{
  "name": "Product shot",
  "set": {"job": {"method": "gpu_path", "options": {"samples": 128}}},
  "surface": {
    "id": "SURF-001", "body_index": 0, "body_name": "Panel",
    "plane": "xy", "scale": 1.0
  },
  "replace": false
}
```

The temporary ordinal/name pair is checked once, its semantic identity is captured, and the opaque target and planar precision bounds are validated before one atomic setup change/undo. `replace: true` explicitly replaces a prior binding; an identical application is a no-op. The full setup is returned.

`render.library.surface.unbind` takes setup `name` and the exact saved `target`. It can remove an obsolete association without current geometry or a device. Switching a bound setup to CPU/raster without removing GPU-only maps refuses. Temporary optical overrides and unsaved external HDR capture cannot silently supersede saved maps; save the HDR first or explicitly remove the temporary inputs.

The existing Render library adds Save maps to this setup and saved-binding inspection/removal. The older temporary-map action stays separate. No alternate UI shell, new artwork or runtime Python dependency is introduced.

## Passive stages

`RenderSetup.stage` stores up to 32 finite passive rectangles, each with ID, world-mm center/half-edge vectors and exact linear RGB reflectance. They are two-sided diffuse surfaces, with zero emission and no transmission. They are appended only to the captured render scene, preserving product ordinals. No engineering bodies or mass are created.

`render.stage.ground` fits one finite panel below the visible product once. Inputs: setup `name`, optional setup `set`, `margin_fraction` (default 0.25), positive `gap_mm` (default 0.1), `reflectance_linear` (default [0.18,0.18,0.18]), and `replace_stage` (default false). The clearance gap is explicit geometric separation, not a simulated support contact. Moving the product does not automatically refit the stage. `render.stage.clear` removes only the stage and preserves lighting, maps and CAD data.

Every render repeats an original rectangle/AABB separating-axis clearance check against current product bounds. Touching or ambiguous placement refuses with the panel/body identity. This is conservative: hollow/nonconvex shapes may produce false-positive clearance failures. It is not an exact B-rep collision certificate. Each mesh's bounds is computed once per staging pass, not per card.

The GPU path integrator can transport diffuse reflection from these cards. The current CPU direct/specular integrator does not gain diffuse global illumination from the presence of geometry; preflight reports that limitation. Raster stage requests refuse rather than discarding panels.

## Complete authored rigs

For numeric rigs, `render.library.studio.prepare` now accepts `complete` with their original passive cards/flags. The reply includes lighting, stage and an explicit projection. `emitters_only` remains an acknowledged omission variant for card-dependent rigs. Calibration blockers and externally owned-Sun requirements remain unchanged.

`render.library.studio.apply` saves lighting and panels atomically. Replacing an existing stage additionally requires `replace_stage: true`. Explicit lighting/environment replacement still establishes one energy owner. The existing UI combines this as lighting/environment/stage consent. Product/card and emitter/product-or-card bounds are checked before mutation. Incompatible placement leaves the setup unchanged. Passive cards retain authored linear reflectance; they are not substituted with extra lights.

Original manifests/readiness flags remain unchanged. Scene/quality specifications requiring unsupported optics, role bindings, full room geometry or adaptive convergence remain specifications. This work does not promote the whole catalog to complete presets or admit the separate SELENE native source.

## Index-matched optical correction

The opaque Schlick approximation now preserves the exact IOR=1 dielectric limit at every angle. Its grazing coefficient is the metal fraction when dielectric IOR is exactly one, and one otherwise. Thus a pure matched dielectric has BRDF `reflectance / pi`, while a conductor retains its response. CPU direct, anisotropic and reflection helpers, the GPU path integrator and PBR viewport use the same rule. A matched-index coat leaves its substrate unchanged. Sampling proposal PDFs remain unchanged; no variance or speed claim accompanies this correction. Analytic and GPU parity checks are separate acceptance gates.


## Physical-reference inspector

The existing library's `physical_profiles` category exposes the 51 published
profiles without assigning them. `render.library.describe` validates each partial
native card through `ValidatedCard`, retains full extended properties, source
locators, conditions, qualifiers and uncertainty, and returns exact dataset
receipts. Surface/material descriptions include their authored candidate or
constituent relations. Source hash mismatches refuse; unknown is never zero.

`render.library.physical.prepare` takes a profile `id`, proposed material `name`
and `acknowledge_reference: true`. It returns a **plan only**, containing a native
`material.create` envelope plus the full extended profile and omitted fields.
It needs one native point density; ranges and missing values refuse. The caller
must review the grade/formulation/conditions and separately choose whether to
execute creation and later assignment. The UI can copy that complete plan, not
silently apply a constituent to the effective mass of an assembly. Forty-three
published profiles have a point-density candidate; that is not a certification.

## Focused native acceptance entry point

After a coordinated source batch, build/run this original public-command workflow
instead of repeatedly linking the full engine unit-test executable:

```sh
cargo run --locked --profile ci -p solidcraft-engine --features raytrace-cpu \
  --example render_resource_workflow -- target/render-workflow-new
```

The output directory must not exist. The probe checks saved surface identity,
reopen/removal/undo, incompatible method rejection, all physical-reference
inspections and density-plan refusals, complete rig card scaling, CAD authority
preservation, and actual CPU ground-stage completion. It writes a scoped receipt
and the completed PNG. It does not substitute for full unit tests, GUI acceptance,
GPU optical parity, or a whole-rover render. Record the source tree before/after.
