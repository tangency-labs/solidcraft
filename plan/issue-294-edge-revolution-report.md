# Issue #294: exact private edge revolution transport

The private edge adapter now preserves actual `SmSurfOfRevolution` face surfaces
through the shared lossless exporter and reader already on main. This restores
72 genuine comparisons covering both seam forms, topology, tangency, blend and
radial operations, including rebuilt UV trims. The unit remains **partial**.

## Implementation and scope

The shim emits the shared exact database-record head only for native type
`15020`, followed by its regular B-spline block. The importer uses
`read_exact_surface` and `check_exact_base` and requires the independently
exported base to match bit for bit. Native non-mutation checks, record round trip,
analytic domain, generator, polar curve and out-of-bounds flags are preserved.
Face ownership/tolerance metadata and fixture inputs retain their existing layout.
A malformed, truncated, trailing, nested or mismatched record is a harness error.
The reader does not recurse into another head in place of the regular block.

The private shim deliberately limits the shared helper to actual revolution
surfaces. Sphere, torus, cone and extrusion transport remain outside this change;
the `torus.smb` cases below contain native revolution surfaces, not `SmTorus`.
No shared helper, Surface dispatch or kernel numerical implementation changes.
Phase 1 commit `6da1a55b` and its dependency authorship are retained through merges.

## Fresh native results

| Measurement | Current-main baseline `7c74a05f6` | Private adapter |
| --- | ---: | ---: |
| Cases | 1787 | 1787 |
| Passes with compared values | 1595 | 1667 |
| Deferrals | 192 | 120 |
| Failures / empty passes | 0 / 0 | 0 / 0 |
| Maximum relative error | 0 | 0 |


All 72 restored cases compare 120 values with zero relative error. Their outputs
are flags, traversal indices and statuses. Topological distance remains the
native `1e20` sentinel; this does not claim new nonzero distance coverage. Blend
returns the null edge-use index `-1`. Relative tolerance remains `1e-9`, with no
edge absolute floors.

The per-case vectors below were measured on Phase 1 source `6da1a55b` and
retained in the separately validated integration `d427cb2b5`. For every row,
before is a C++ `inexact_geometry` refusal and a Rust
`topology.edge` import deferral, with zero compared values. After is a genuine
pass with identical C++ and Rust state arrays, no operation error or rejection,
and no deferral. `_seam` and `_seam_f` exercise both seam forms; `_rb_` rebuilds
UV trim curves. Native edge curves are B-splines (`14033`), and incident faces are
exact `SmSurfOfRevolution` (`15020`).

| Case | After C++ state = Rust state | Values compared |
| --- | --- | ---: |
| `torus_e0_seam` | `[1.0]` | 1 |
| `torus_e0_seam_f` | `[1.0]` | 1 |
| `torus_e0_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `torus_e0_tangent` | `[1.0]` | 1 |
| `torus_e0_blend` | `[-1.0]` | 1 |
| `torus_e0_radial` | `[0.0, 2.0, -1.0]` | 3 |
| `torus_e2_seam` | `[1.0]` | 1 |
| `torus_e2_seam_f` | `[1.0]` | 1 |
| `torus_e2_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `torus_e2_tangent` | `[1.0]` | 1 |
| `torus_e2_blend` | `[-1.0]` | 1 |
| `torus_e2_radial` | `[0.0, 2.0, -1.0]` | 3 |
| `torus_e5_seam` | `[1.0]` | 1 |
| `torus_e5_seam_f` | `[1.0]` | 1 |
| `torus_e5_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `torus_e5_tangent` | `[1.0]` | 1 |
| `torus_e5_blend` | `[-1.0]` | 1 |
| `torus_e5_radial` | `[0.0, 1.0, -1.0]` | 3 |
| `torus_e9_seam` | `[1.0]` | 1 |
| `torus_e9_seam_f` | `[1.0]` | 1 |
| `torus_e9_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `torus_e9_tangent` | `[1.0]` | 1 |
| `torus_e9_blend` | `[-1.0]` | 1 |
| `torus_e9_radial` | `[0.0, 1.0, -1.0]` | 3 |
| `torus_e11_seam` | `[1.0]` | 1 |
| `torus_e11_seam_f` | `[1.0]` | 1 |
| `torus_e11_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `torus_e11_tangent` | `[1.0]` | 1 |
| `torus_e11_blend` | `[-1.0]` | 1 |
| `torus_e11_radial` | `[0.0, 1.0, -1.0]` | 3 |
| `torus_rb_e0_seam` | `[1.0]` | 1 |
| `torus_rb_e0_seam_f` | `[1.0]` | 1 |
| `torus_rb_e0_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `torus_rb_e0_tangent` | `[1.0]` | 1 |
| `torus_rb_e0_blend` | `[-1.0]` | 1 |
| `torus_rb_e0_radial` | `[0.0, 2.0, -1.0]` | 3 |
| `surf_rev_e0_seam` | `[0.0]` | 1 |
| `surf_rev_e0_seam_f` | `[0.0]` | 1 |
| `surf_rev_e0_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `surf_rev_e0_tangent` | `[0.0]` | 1 |
| `surf_rev_e0_blend` | `[-1.0]` | 1 |
| `surf_rev_e0_radial` | `[0.0, 0.0, -1.0]` | 3 |
| `surf_rev_e2_seam` | `[0.0]` | 1 |
| `surf_rev_e2_seam_f` | `[0.0]` | 1 |
| `surf_rev_e2_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `surf_rev_e2_tangent` | `[0.0]` | 1 |
| `surf_rev_e2_blend` | `[-1.0]` | 1 |
| `surf_rev_e2_radial` | `[0.0, 2.0, -1.0]` | 3 |
| `surf_rev_e5_seam` | `[0.0]` | 1 |
| `surf_rev_e5_seam_f` | `[0.0]` | 1 |
| `surf_rev_e5_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `surf_rev_e5_tangent` | `[0.0]` | 1 |
| `surf_rev_e5_blend` | `[-1.0]` | 1 |
| `surf_rev_e5_radial` | `[0.0, 1.0, -1.0]` | 3 |
| `surf_rev_e9_seam` | `[0.0]` | 1 |
| `surf_rev_e9_seam_f` | `[0.0]` | 1 |
| `surf_rev_e9_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `surf_rev_e9_tangent` | `[0.0]` | 1 |
| `surf_rev_e9_blend` | `[-1.0]` | 1 |
| `surf_rev_e9_radial` | `[0.0, 1.0, -1.0]` | 3 |
| `surf_rev_e11_seam` | `[0.0]` | 1 |
| `surf_rev_e11_seam_f` | `[0.0]` | 1 |
| `surf_rev_e11_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `surf_rev_e11_tangent` | `[0.0]` | 1 |
| `surf_rev_e11_blend` | `[-1.0]` | 1 |
| `surf_rev_e11_radial` | `[0.0, 3.0, -1.0]` | 3 |
| `surf_rev_rb_e0_seam` | `[0.0]` | 1 |
| `surf_rev_rb_e0_seam_f` | `[0.0]` | 1 |
| `surf_rev_rb_e0_topological` | `[0.0, -1.0, 1e+20]` | 3 |
| `surf_rev_rb_e0_tangent` | `[0.0]` | 1 |
| `surf_rev_rb_e0_blend` | `[-1.0]` | 1 |
| `surf_rev_rb_e0_radial` | `[0.0, 0.0, -1.0]` | 3 |

## Controls and remaining dependencies

Exact controls retain 4 values (`line_join_fw_tangent_start`), 18
(`line_join_fw_trimmed`), 18 (`net_trimmed_e0`), 1
(`plane_square_e0_tangent`) and 50 (`Mouse_e0_query`). The native regression also
checks imported concrete kinds and face owners, unsupported import deferrals,
and damaged records.

The remaining 120 deferrals comprise 42 cone, 36 sphere and 36 extrusion import
gaps plus six DropCurve calls. DropCurve is owned by the active continuation
[#463](https://github.com/tangency-labs/solidcore/pull/463), which supersedes closed
[#136](https://github.com/tangency-labs/solidcore/pull/136). Concrete-surface
CrvOnSurf remains with [#307](https://github.com/tangency-labs/solidcore/pull/307).
The owner-tolerance work from #301 and shared exact-export work from #304/#436
are already on the pinned main. This PR adds no competing dependency work.

## Separate six-case DropCurve integration evidence

An earlier bounded local integration combined main `215f690e`, the Phase 1 adapter
and #136 owner continuation `4190d481`, preserving merge authorship. Its tested
source is `d427cb2b5`. These measurements describe that separate source, not the
current PR or the current head of #463.

That integration changed exactly six of 1787 records: value passes increased from
1667 to 1673 and deferrals fell from 120 to 114, with zero failures, rejections,
both-error passes or empty passes and maximum relative error zero. The other
1781 records and both raw outputs were unchanged; all 72 revolution cases and
existing exact controls were retained.

| Case | Before C++ / Rust | After C++ = Rust | Compared values before / after |
| --- | --- | --- | ---: |
| `Endcap_e0_uv_of_surface` | `[1.0,0.0]` / `{}` | `[1.0,0.0]` | 0 / 2 |
| `Endcap_e2_uv_of_surface` | `[1.0,0.0]` / `{}` | `[1.0,0.0]` | 0 / 2 |
| `Endcap_e5_uv_of_surface` | `[1.0,0.0]` / `{}` | `[1.0,0.0]` | 0 / 2 |
| `Endcap_e9_uv_of_surface` | `[1.0,0.0]` / `{}` | `[1.0,0.0]` | 0 / 2 |
| `Endcap_e11_uv_of_surface` | `[1.0,0.0]` / `{}` | `[1.0,0.0]` | 0 / 2 |
| `Endcap_rb_e0_uv_of_surface` | `[1.0,0.0]` / `{}` | `[1.0,0.0]` | 0 / 2 |

Each before deferral names `intersect.surface_drop_lib_p3: SmSurface::DropCurve`.
Both operation error fields are absent before and after; all after comparisons
have relative error zero, no rejection and no deferral. The two standard values
record UV-curve presence and owning edge-use traversal index. All six names
resolve to one actual edge/face pair; the rebuilt case requests a separate export.
They are not six distinct geometric edges.

An additional observational audit exported the resulting cubic 2D UV B-spline
through the existing serializers: all 795 scalar values per case were
bit-identical, comprising six header fields, 157 weighted control-point records
and 161 knots. These are separate observations, not extra standard-runner
comparison counts or permanent regression assertions.

Concrete B-spline faces, raw knot vectors and owner
`ObjectRef(16002,ObjectKey(1))` were retained. Native export, Rust import and
ordinary-copy knot bits matched, including the interior U knot
`0.418782349525896` and V knot `0.5020448001044`. Face tolerance remained `1e-5`.
UV-trim creation increased edge tolerance from `0.0040511335586128185` to
`0.007557289128801149` identically in both engines; that was the only changed
Rust edge field. No default tolerance or approximate geometry was substituted.
The temporary surface copy intentionally clears its owner as upstream does;
the arena face owner and geometry remain intact. Neither #307 nor #301 was a
missing dependency on these six paths.

On that separate source, native validation passed both strict workspace Clippy
gates, formatting, 1625 kernel tests, wasm, the native edge regression, 239-unit
default and sensitivity gates (120 green, 119 partial, zero failed), edge and
DropCurve perturbations, and 2000-input fuzz runs for each. DropCurve itself had
1529 cases, 1310 value passes, 219 CrvOnSurf deferrals and zero failures/empty
passes. Its sensitivity detected 1310/1310 perturbations. The previously reported
`probe_map_plane` ownership error did not reproduce: both engines agreed on
14 integer values and returned no error. This evidence is available for #463's
integration review; its head still requires its own validation.

## Validation of this PR

The native default edge comparison on source `0471572f6` passed with the
counts above. Formatting and both strict workspace/all-target Clippy gates passed.
The current branch is checked by the [CI jobs on PR #492](https://github.com/tangency-labs/solidcore/pull/492/checks),
including workspace tests, native C++ runner regressions, wasm and the full
differential gate. Those checks validate the PR's merge with newer main separately
from the pinned baseline/adapter measurements above.


Both raw runner outputs and default reports were captured for the fresh pinned
baseline and adapter. The earlier audits retain complete case-name joins and
post-gate default reports.
Reports are CI artifacts under the current repository layout and are not
versioned.
