[`crate::topology::edgeuse::p2::is_convex_radial_sector`]
(`SmEdgeuse::IsConvexRadialSector`) propagates a failing
[`crate::topology::edgeuse::p2::evaluate_binormal_step_off`] status on either this
edge use or its radial partner as `Err` instead of returning `Ok(true)` (pinned
v1.0.1's `SER(...)` inside an `SmBoolean` function returns the non-zero `SmStatus` as
`TRUE`, so a step-off that leaves a face's UV domain or a failed binormal evaluation
falsely classifies even a concave sector as convex; `WR-84`,
`plan/upstream-issues/WR-84--addendum-2026-11-01-propagate-step-off-failure.md`).
Off by default for pinned C++ parity.
