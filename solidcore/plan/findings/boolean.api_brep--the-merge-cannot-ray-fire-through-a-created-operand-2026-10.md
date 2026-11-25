# boolean.api_brep: the merge cannot ray-fire through a created operand (2026-10-24)

**Resolved** on fix/boolean-api-e2e (PR #816): the merge queries use `BrepQuery::new(BrepIdentity::of_brep(b))`, picking each B-rep's recorded naming scheme (cherry-picked 09706e3d6, bb1945463, 56ae09d46). Both cases now pass and their `known_mismatches.toml` entries are removed; `with_options_difference` / `with_options_keep_other` (box minus the inscribed sphere, compared once Point3DClassify was wired) pass for the same reason.

- **What.** The `boolean.api_brep` runner now builds its operands with the ported
  `SmApiCreateBox` / `SmApiCreateSphere` (`primitive::api_primitives`), as the shim does,
  instead of empty stand-ins. Two cases disagree. In both, the second operand lies entirely
  inside the first, so no face intersects:

| case | inputs | record (faces edges vertices regions) |
|---|---|---|
| `census_difference_cavity` (TestSmBrep.cpp:3309) | box (0,0,0) 10x10x10 minus box (3,3,3) 4x4x4 | cpp 12 24 16 3, rust 6 12 8 2 |
| `gen0220` | sphere c=(0.00716,1.07511,0.84640) r=1.37304 minus box (-0.56173,1.02171,-0.07353) 1.00673x0.54777x1.47193 | cpp 7 13 10 3, rust 1 1 2 2 |

- **Cause (port instrumented, not committed).** `boolean::merge::p4::classify_remaining`
  classifies each unintersected face set with `BrepQuery::default()`. That query keys the
  cache by `brep_cache::arena_identity`, and `make_brep_cache` adds a face's surface only
  when `surface.owner() == keys(Face)`. The primitive constructors own their face surfaces
  under the `topology::loopuse::notify_ref` scheme instead (`type 16002`, key
  `1 + (4 << 32) + index`, set by `SetSurface` through the creation's `NotifyEnv`). So the
  cache holds no surfaces, every ray from a point inside the box finds no hit, and
  `RayFire` answers the infinite region. The cavity's faces are then deleted instead of
  kept. A fresh `BrepQuery::default().point_3d_classify` on a created box puts (5,5,5) in
  the infinite region too. Imported B-reps (every other merge runner) own their geometry
  under `arena_identity`, so they never show this.
- **Scope.** The same assumption is made at every `BrepQuery::default()` in
  `boolean::merge` (p1, p4, p5). Only the containment cases reach a ray fire here; the
  intersecting cases classify their faces through the intersection edges.
- **Fix.** The operands' geometry owners and the merge's query identity must use one
  scheme: either the primitive constructors own geometry under `arena_identity`, as
  `BrepIdentity::default` documents for B-reps this crate builds, or the merge keys its
  queries by the scheme the operands use. Both cases are listed in
  `crates/difftest/known_mismatches.toml` until then.
