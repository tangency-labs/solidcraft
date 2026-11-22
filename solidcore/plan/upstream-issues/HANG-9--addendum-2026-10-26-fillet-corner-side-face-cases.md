Updates: HANG-9

- **Two more confirmed triggers:** cases `corner_adjust_concave_face_12` and `corner_adjust_concave_face_25` of `blend.fillet_corner_p2` (the side-face variants, whose side edge use lies on a target face so the extension finds a side face) reach the zero-gap path of the earlier addendum (`HANG-9--addendum-2026-10-26-fillet-corner-gap-zero.md`): coincident rail points with `m_dThisApproxTol3d == 0`.
- **Evidence:** with the shim's pre-check bypassed and the C++ run natively, one process per case, both ran until a 60 s timeout; interrupting them under gdb gave `N_CrvArcLength` (or `N_EndNurbs` <- `N_CrvArcLength`) <- `ST_curxcdr` <- `N_CrvExtendByDist` <- `ST_surxsdr` <- `N_SrfExtendByDist` <- `SmBSplineSurface::CreateExtendedSurface` <- `SmFilletNx2Corner::AdjustConcaveCornerTopology`.
- **Shim guard:** unchanged (`blend_fillet_corner_p3_static.inc`, `dGapDist == 0` after the `< tol` early return); both cases stay rejected.
