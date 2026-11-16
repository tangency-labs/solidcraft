# General: addendum (2026-10-31): the five fork-main commits the audit had not probed

Updates: plan/findings/general--smlib-fork-port-audit-2026-10-31.md

The audit record lists six fork-main commits as not yet probed. f653ac9 (per-edge chamfer second offsets) is covered by #2095. For each of the other five, main's log, `plan/`, `crates/` and the open PRs were searched by sha and by topic, and the fork's SMLib diff was read against the Rust. No open PR touches any of them.

| Commit | Fork change | Verdict |
| --- | --- | --- |
| 3545008 | A split ProtoEdge's upper EdgeDefinition starts with no owner. | In, on the default path. |
| 8c4bda5 | `ValidateAndUpdateTolerances` repairs gaps over half the tolerance. | Moot on the SMLib/SM_API path. |
| 8ea6a20 | Refining a range solution accepts an outside parameter again. | Moot under SolidCraft's preset; one follow-up. |
| 9a160ba | A revolved arc's sphere is built on its own domain. | In, by a later fork commit. |
| 1279539 | A cone's iso circle through a point is at the point's height. | Ported as opt-in `cone_iso_circle_at_axial_height` (WR-704, PR #2289). |

## 3545008

`ProtoTopologyManager::edge_definition_split` (`crates/solidcore/src/topology/proto_topology/p3.rs`) clears the new EdgeDefinition's proto edge after `copy_edge_definition` (#518), so the upper copy never names the lower proto edge. With arena ids, a stale owner could only cause the same bogus `RemoveEdgeDefinition`, and it is cleared before `AddEdgeDefinition`. Nothing was run.

## 8c4bda5

The fork changes the repair branch's test from `> tol` to `> tol / 2`, and with it the `rbTolerancesViolatedOrUpdated` result. Every SMLib caller asks for validation only:
- `SmMerge.cpp` 2616 and 2628, in the debug drawing block;
- `SmBrep.cpp` 36228, in `Dump`.
Their result is read only in that debug block. The port has the validate-only report (`tolerances` in `crates/solidcore/src/topology/brep/validation/assert_valid/brep.rs`), which already checks half the tolerance. The repair path has no reachable caller in SMLib or SM_API. Nothing was run.

## 8ea6a20

8ea6a20 undoes the fork's own `SER(Inversion)` from 8311f8e. In the port that status pass-on exists only behind `query_statuses_pass_on` (`inversion_passed_on` in `refine_cc_solutions`, `crates/solidcore/src/curve/curve/p1.rs`), which SolidCraft's preset leaves off. With it off, the port drops the status, as upstream and the fork after 8ea6a20 do. A zero-length range gives `UNDEF_DOUBLE`, whose magnitude fails the 0.002 shift limit, so the end stays: the same as 8ea6a20's rule. Nothing was run.

Follow-up: if `query_statuses_pass_on` ships, this site should be exempted from it, so that an outside fraction is judged by the shift limit instead of aborting.

## 9a160ba

`crates/solidcore/src/sweep/analytic_revolution.rs` builds a revolved arc's sphere with `Sphere::new` on the arc's own latitude domain and then makes its NURB, not with `CreateCanonical` followed by `AdjustSTEPUVDomain`. That came from the fork's 9853e54 (#1490), which supersedes 9a160ba's version. Nothing was run.

## 1279539

The fork's `CreateIsoCircleFromPoint` takes the circle's STEP height as the point's axial height `dParam`, clamped to the cone's height interval, where upstream inverts the point moved onto the cone. The port already took the clamped height when that inversion fails (UB-87, #1067), the case the fork's commit describes.

Probe batch 54g7 ran SolidCraft's preset and the default kernel on main 2be079d409.
- **The fork's own capi test** (`sphere_cone_top_circle`, as the fork's capi builds it): it passes under both. The sphere through the point 2 tol past a 70 degree cone's top gives 2 crossing curves, one of them the top circle at z = 1, with every sample within 2.2e-4 of both surfaces (the test allows 2 tol).
- **Our circle height against the fork's rule** (the rule re-implemented in the probe, not a run of the fork). Five cones were tested: bottom radius 0 and 0.5, the latter in all four `swap_uv`/`inside_out` combinations. Each took 22 heights, from 2 tol below the bottom to 2 tol above the top, at 5 radial positions.
  - The two agree to 2.2e-16 inside the cone, past the top end, and past the bottom of the cones with no apex there.
  - They differ past the apex of the cone whose apex is its bottom end. There the point moved onto the cone is on the far nappe, the inversion mirrors it through the apex and succeeds with a height outside the cone, and the circle fails with `SM_ERR`. The fork gives the apex curve. `SmSphere::IntersectWithCone` passes such a point for a sphere crossing the cone's line within tolerance past the apex, so that intersection fails on the default path.

The default path stays as the pinned C++ (by reading; the new `surface.cone_p1` fixture cases at the apex compare both sides' status). The fork's rule is the opt-in `Kernel::fixes.cone_iso_circle_at_axial_height`; the defect is WR-704 (#2289).
