# topology.face_p1: SwapUV surface notifications not delivered on two paths (2026-10-27)

Unit: `topology.face_p1` (`SmFace::SwapUV`), reached from `topology.brep_p3` (Orient),
`topology.brep_p1`/`p2`/`p9` and `heal.stitch_p1`. PR #1299 made
`face::p1::swap_uv_with_notifications` send each edited surface's `SM_NO_PRE_EDIT` /
`SM_NO_POST_EDIT` (`SmBSplineSurface::SetCanonical`, `SmSrfInVolume::SwapUV`) through the
caller's `NotifyEnv`, and the `topology.brep_p3` Orient cases compare them against the C++.
Two gaps remain. No current difftest fixture reaches either, so neither shows as a mismatch.

## 1. Callers without a `NotifyEnv`

- `topology::brep::p2::swap_uv` (`SmBrep::SwapUV`)
- `topology::brep::p1::swap_face_uv`, used by `topology::brep::p9::orient_sheet_surfaces`
  (`SmBrep::OrientSheetSurfaces`)
- `heal::stitch::p1` `unify_normals` (the stitcher's `UnifyNormals`, through
  `face::p1::swap_uv`)

These call `face::p1::swap_uv`, which has no environment. Upstream, the face's and the
surface's `Notify` calls reach the attributes and context of the objects involved. In Rust,
the face notifications already went to an empty environment before #1299, and the surface
notifications are now lost the same way. Only `SmSurface::Notify`'s trimmed-surface cache
deletion runs on these paths. A tracker or attribute callback attached during these
operations would miss the events the C++ sends.

Fix: thread a `NotifyEnv` (attributes, `Option<&mut Context>`, `key_of`) through these
signatures and their callers, and call `swap_uv_with_notifications`. Then compare the
tracker or attribute events in those units' runners, as `topology.brep_p3`'s `split_orient`
does.

## 2. Owner links of nested surfaces

`swap_uv_with_notifications` and the `topology.brep_p3` runner
(`brep::p3::face_surface_identity`) report the `owner_link` (surface -> face) of the face's
own surface only. In two cases the notifying surface is a nested one:

- an offset surface's extended or base surface (`SmOffsetSurface::SwapUV` sends no
  notification of its own)
- a surface in a volume's inner surface (it notifies inside its owner's PRE/POST)

The nested surface gets a key when it notifies but no owner link. The tracker's owner walk
(`TopologyChangeCallback`, `topology/topology.rs`) then returns `Unsupported` for its
POST_EDIT instead of reaching the face. Upstream walks the object owners directly.

Fix: report owner links for nested surfaces (inner surface -> outer surface) when their
identities are issued, and add an Orient fixture with an offset or volume face.

Noted in the Deviations of `crates/solidcore/src/topology/face/p1.rs`.
