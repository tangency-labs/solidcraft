# Portable built-in surfaces and passive render stages

Integration-branch implementation, not a release acceptance claim. Source, type/lint checks, regression execution and actual application acceptance are recorded separately in `render-materials/BATCH-2026-10-22.md`.

## Saved surfaces

`RenderSetup.surfaces` stores at most 128 surface bindings. Built-in sources retain: a `SURF-nnn` ID, resource version, four encoded SHA-256 receipts, an exact definition/producing-feature/component/occurrence-path target, a definition-local planar frame in millimetres, and normal strength. The roles are base color, OpenGL normal, roughness and metallic. Height remains authoring-only. Existing body IOR is retained. Engineering material cards, mass, topology and the CAD timeline are unchanged.

No captured body ordinal, GPU object or external path is saved. Every Check and Start resolves the semantic target, transforms its local frame into current world coordinates, then produces the ordinary bounded texture request for the selected executor. Motion follows that transform; another instance does not inherit the assignment. Missing, hidden, ambiguous, renamed or replaced definitions/features refuse instead of selecting a similar body. An occurrence label may change while its ID path remains valid. Definition renaming and topology-producing feature changes currently require explicit rebinding.

Built-in bindings are path-independent **for the pinned built-in pack**. Reopening preserves references, but execution refuses an unavailable resource or version/digest mismatch. A future installed pack must still supply those exact bytes; an old ID must never be silently reinterpreted.

Send the following to `render.library.surface.bind` after inspecting `render.scene`:

```json
{
  "name": "Product shot",
  "set": {"job": {"method": "gpu_path", "options": {"samples": 128}}},
  "surface": {
    "id": "SURF-001", "body_index": 0, "body_name": "Panel",
    "plane": "xy", "scale": 1.0, "rotation_rad": 0.7853981633974483
  },
  "replace": false
}
```

The temporary ordinal/name pair is checked once, its semantic identity is captured, and the opaque target and planar precision bounds are validated before one atomic setup change/undo. `replace: true` explicitly replaces a prior binding; an identical application is a no-op. The full setup is returned.

`render.library.surface.unbind` takes setup `name` and the exact saved `target`. It can remove an obsolete association without current geometry or a device. Saved opaque maps work with native CPU raytracing and GPU path tracing. Switching to raster refuses instead of dropping the maps. Temporary optical overrides and unsaved external HDR capture cannot silently supersede saved maps; save the HDR first or explicitly remove the temporary inputs.

The existing Render library adds Save maps to this setup and saved-binding inspection/removal. The older temporary-map action stays separate. No alternate UI shell, new artwork or runtime Python dependency is introduced.

## Imported wood and other opaque surface images

The Surfaces panel can import a local color image, choose its physical U/V repeat
and grain angle, and save it with the selected shot. Import records the supplied
author, source and license as inert declared provenance. It does not fetch a URL
or approve licensing. The full frame is saved in definition-local millimetres;
rotating or moving an occurrence carries its grain with it. Physical materials,
mechanical axes, mass and geometry are unchanged.

`render.surface.import` captures exact static 8-bit PNG/JPEG bytes and binds the
image in one undo step. Example after inspecting `render.scene`:

```json
{
  "name": "Wood",
  "set": {"job": {"method": "gpu_path", "options": {"samples": 128}}},
  "body_index": 0, "body_name": "Panel",
  "frame": {"origin": [0,0,0], "u": [240,0,0], "v": [0,480,0]},
  "maps": {"base_color_srgb": {"path": "/textures/original-wood.png"}},
  "provenance": {"author": "Your name", "source": "Original photograph", "license": "Your declared terms"}
}
```

Map inputs also accept `normal_opengl`, `roughness_linear` and `metallic_linear`.
Each map takes `path` and optional `expected_sha256`; a changed selection refuses.
The role is explicit: color normalizes from sRGB before filtering, while normals
and scalar maps remain linear data. Portable base-color PNG declarations must
agree with normalized sRGB. Static metadata and encoding checks apply to portable
imports; older built-in and temporary GPU maps retain their prior decoder admission,
explicit-role interpretation, single-level bilinear sampling and face-map precedence.
Newly imported GPU resources opt into full mip chains and yield to explicit scalar
face appearances. Native CPU maps use full mip chains. `normal_strength` defaults to one and
`flip_v` defaults to false. `replace: true` is required for an existing binding.

The design deduplicates bytes by SHA-256, independently of role. Importing an
existing digest reuses its first recorded provenance; later imports do not replace
that author, source or license metadata. The design stores at most
32 unique images and 64 MiB total encoded storage; each image is at most 16 MiB
and 1024×1024. Complete decode and metadata validation happen before publishing
any resource. Native containers reuse verified `blobs/` entries; portable JSON
uses bounded canonical base64. Save, delete the source file, and reopen offline:
no filesystem path is retained or reopened. Failed import/open leaves the prior
design intact. Older applications refuse the newer format instead of discarding
its resources.

`render.texture.list` reports digests, dimensions, declared provenance and setup
users. `render.texture.remove` refuses referenced images and lists their users;
explicitly unbind them first. Import, unbind, removal and their undo operations
preserve the exact bytes. Completed render handles retain metadata rather than
keeping an uncharged encoded-image cache alive.

The native CPU consumer prepares rows and complete mip chains cooperatively,
then maps the opaque closure used for direct lights and HDR illumination. It
accepts only noninterlaced 8-bit PNG when importing into a CPU setup. Supported
JPEG8 and interlaced PNG remain GPU inputs; explicitly normalize them before CPU use. It
rejects incompatible selected reflections, glass, anisotropy and procedural
surface overrides. Browser execution and unsupported source encodings refuse
explicitly. Projection remains planar: use separately oriented bindings for
separate boards. This is an opaque image-based wood preview, not a volumetric
end-grain or measured wood-scattering model.

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
