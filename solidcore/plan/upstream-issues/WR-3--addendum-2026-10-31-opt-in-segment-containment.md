Updates: WR-3

The pinned v1.0.1 source still ORs the endpoint tests in
`source/SMLib/src/SmExtent3d.cpp:1235`, despite the convexity comment requiring
both endpoints. The Rust port preserves that result by default.

`Kernel::fixes.extent3d_contains_both_segment_endpoints` is a default-off
opt-in. It makes `Volume::is_line_in_param_domain_simple` require both endpoints
inside the natural parameter box for bounded rectilinear domains (B-spline
volumes). The other volume kinds keep their existing domain checks.
The kernel is passed through `Volume::is_line_in_param_domain`, so in-space
line checks and parameter-box edge checks also honor the flag.
The kernel-free `Extent3d::contains_line_seg` remains upstream-compatible;
no additional public extent method is needed.

Caller trace: the only Rust production call to `contains_line_seg` is the
bounded branch of `Volume::is_line_in_param_domain_simple`. The upstream
`ContainsLineSeg3d` call is inline in `source/SMLib/inc/SmVolume.h:2241`,
reached by `SmVolume::IsLineInParamDomain` at `source/SMLib/src/SmVolume.cpp:1058`.
There are no other calls to `ContainsLineSeg3d` in `source/SMLib/src`.

The integration regression `wr3_segment_box_containment` uses a B-spline volume
with unit-box natural domain. It checks both orders of a segment with one
endpoint inside, both-inside (including boundary), both-outside and crossing
controls through simple, parameter-space and in-space line APIs, plus a flat
parameter box that extends outside the domain. Default mode accepts the partial
segment, while opt-in rejects it; both modes reject the partial box because its
outside face has edges with both endpoints outside. The existing `geom.extent3d` difftest
case `edge_line_seg_one_end_inside` retains the legacy C++ comparison.

Validation: the integration regression passes, and default-mode `--status-of`
runs are green for `geom.extent3d`, `surface.volume_p1`, `surface.volume_p2`,
`surface.b_spline_volume`, `surface.bend_volume`, `surface.twist_volume` and
`surface.unbend_volume`. Formatting, workspace clippy and the wasm32 check pass.
