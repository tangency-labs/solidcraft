# Render resource starter pack

Version **1.0.0**, authored 2026-10-21. This is a reproducible resource handoff
for the personal SolidCraft render/materials fork. Its parent is the catalog
checkpoint `10ada1b45a4eb285ce752ac72f9aaa968ff8b5c4`, on the fixed
`lab/render-materials` base. It does not update upstream or the application.

## What is delivered

| Resource | Delivered data | Availability |
| --- | --- | --- |
| [48 surfaces](materials/README.md) | 240 PNGs at 512 × 512: base color, OpenGL normal, roughness, metallic and 16-bit height | Authored, numerically checked; renderer binding and reference renders pending |
| [24 illumination environments](environments/README.md) | 24 original 1024 × 512 Radiance RGBE HDR inputs | Authored, independently decoded; complete scene geometry and runtime binding pending |
| [24 lighting recipes](lighting/README.md) | Finite-area source transforms and radiance, finite-angular Sun, passive cards and explicit blockers | 19 numerical recipes; 3 calibration blockers; 2 require an existing environment-owned Sun |

The stable `MAT-*`, `ENV-*`, `LUZ-*` and `STU-*` concept identities remain in the
[catalog](../../../docs/render-materials/README.md). This pack supplies authored
realizations; it does not change all 1,160 concepts to "available". Neither
`pack.json` nor the child manifests are a native `solidcraft-library/1` or
`solidcraft-library/2` import file. Python is an offline authoring dependency only.

## Entry points and ownership

- [pack.json](pack.json) identifies the three collections and their file contracts.
- `materials/source/recipes.json` is the editable surface authority.
- `environments/source/recipes.json` is the editable environment authority.
- `lighting/recipe.json` pins the existing `studio-presets.json` authority and
  defines its numerical calibration; the studio geometry is not duplicated as
  a second editable authority.
- `exports/` contains required portable authoring inputs, with one resolution
  per map. Manifests capture source/generator hashes and payload receipts.
- [VALIDATION.md](VALIDATION.md) records what was actually checked and what remains.

The data covers industrial metals and coatings, automotive finishes, electronic
housings and PCB surfaces, FDM/powder printing, wood/DIY, passive LED-associated
surfaces, composites, thermal coverings and generic lunar surface studies.
Material names express an appearance target. They do not certify composition,
strength, thermal/electrical behavior, vacuum compatibility or flight readiness.

## Rebuild and check

Use Python 3.11 or newer, NumPy and Pillow in a development environment. The
environment decoder comparison additionally uses an installed FFmpeg executable
as a separate process; no FFmpeg source, library or binary is bundled or linked.
The exact development versions used for the release are in the validation record.

```sh
python3 -B assets/render/starter/tools/materials.py build
python3 -B assets/render/starter/tools/environments.py build
python3 -B assets/render/starter/tools/lighting.py build

python3 -B assets/render/starter/tools/validate_pack.py
python3 -B assets/render/starter/tools/materials.py validate --reproduce
python3 -B assets/render/starter/tools/environments.py validate --independent-decode --reproducible
python3 -B assets/render/starter/tools/lighting.py validate
python3 -B -m unittest discover -s assets/render/starter/tools -p 'test_*.py'
```

For a read-only combined authoring and native Rust check, use:

```sh
python -m pip install -r assets/render/starter/tools/requirements.txt
python -B assets/render/starter/tools/check_all.py
```

See [native admission and continuous validation](NATIVE_ADMISSION.md) for the
192 texture inputs, 48 separately checked height sources, 24 HDR inputs, coordinate
conventions, test coverage and evidence receipts. The dedicated Render resource
gate runs these checks for relevant pull requests, including drafts, on Windows
and Linux. It does not replace complete `cargo xtask ci`, application rendering
acceptance or persistent resource binding; all runtime availability remains false.

The surface reproduction compares exact decoded pixel arrays; PNG compressed
bytes may depend on the encoder build and platform even with the same Pillow
version. All 240 decoded images were verified equal on Linux and Windows.
Scoped Git attributes preserve LF in hashed text sources and binary map bytes.
Environment and lighting reproduction checks compare encoded bytes. Every submitted export also has an
exact SHA-256 receipt. Altering a receipt is not a substitute for reviewing and
rebuilding a changed recipe.

Optional inspection outputs stay under ignored `target/render-starter-review/`:

```sh
python3 -B assets/render/starter/tools/materials.py preview
python3 -B assets/render/starter/tools/environments.py preview
python3 -B assets/render/starter/tools/lighting.py review
```

These are numerical data/fixture diagnostics, not screenshots of SolidCraft or
proof of photorealistic output. They are not shipped as production resources.

## Integration contract

1. Admit sources with exact hashes and byte/dimension budgets before allocating
   decoded textures. The current source limit remains 2048 pixels per texture
   axis and 2048 × 1024 for HDR; the submitted resources are below those limits.
2. Decode base color as sRGB and scalar/normal channels as linear data. Preserve
   the OpenGL normal convention and millimeter tile sizes. Renormalize normals
   after filtering. The 16-bit height exports are authoring data and require an
   explicit height/displacement adapter; do not send them through the strict
   eight-bit texture-input path or silently quantize them.
3. Preserve physical material cards, part dimensions, geometry, poses, joints
   and motion. An appearance realization does not assign engineering properties.
4. For HDR, honor explicit linear-sRGB primaries and the documented longitude
   mapping, source exposure and environment ownership. These probes provide
   illumination/reflection, not room parallax, support geometry or terrain.
5. Use a probe or its equivalent analytic emitters as the energy source. Do not
   duplicate its bright patches with extra lights. Black vacuum supplies exactly
   zero ambient radiance; a separately owned Sun must be supplied deliberately.
6. Apply lighting transforms once using the selected subject extent. Keep
   passive cards non-emissive, distinguish radiance from irradiance and retain
   blocked sources. Reference EV numbers are not lumens, watts or device values.
7. Bind each admitted resource to the actual renderer version, then capture
   common-exposure reference scenes on flat and curved coupons. CPU/GPU agreement,
   aliasing, filtering, orientation, save/reopen and resource lifetime remain
   integration acceptance work. Follow the existing Render tab/icon/UI contract.

Opaque coat, anodizing and fibre appearances are marked approximations. There
is no glass transmission, layered clearcoat, spectral response, subsurface
scattering, sampled emissive material or measured photometry in this surface
slice. Such features require their own supported binding and reference evidence.

## Clean-room and publication boundary

All new recipes, tools and map fields were authored from original mathematical
construction and the project's own contracts. There are no external textures,
commercial CAD outputs, sampled proprietary materials, copied implementations,
AI-generated beauty images or imported HDR photography. Public format/equation
references are cited in the collection documentation. Each non-Markdown file has
an exact root [ATTRIBUTION.md](../../../ATTRIBUTION.md) row under the repository's
`MIT OR Apache-2.0` choice.

The improved SELENE native source remains `awaiting_native_source`; this resource
pack does not change that lock, substitute the historical v7 model or assert that
the improved rover has been rendered. New resources are isolated from the frozen
PR #3 review in the stacked `assets/render-starter-pack` branch. Integrate their
attribution additions alongside, and without deleting, the other renderer
thread's notices. Full repository gates remain required before application merge.
