# surface.surface_p5: the generic iso body answers an iso line outside the given domain, the B-spline override refuses it (2026-11-21)

- **Unit:** `surface.surface_p5` (`Surface::create_iso_parametric_curve`).
- **Upstream:** `SmSurface::CreateIsoParametricCurve` (`source/SMLib/src/SmSurface.cpp:14943`) never compares the iso parameter with the optional domain's interval in the iso direction. On the singular branch (`SmSurface.cpp:14977-14991`) it intersects the natural domain with `pOptDomain` and reads only the other direction's interval. `SmBSplineSurface::CreateIsoParametricCurve` (`source/SMLib/src/SmBSplineSurface.cpp:1480-1502`) refuses (`SM_ERR`) an iso parameter outside that interval. `SmSurface.h:551` documents `pOptDomain` as an "optional trim bound for IsoParameterCurve".
- **Port:** the same split. The generic body is at `crates/solidcore/src/surface/surface/p5.rs` (singular branch at line 687). The B-spline check is at `crates/solidcore/src/nurbs/b_spline_surface/p2.rs:495-512`.
- **Who takes which:**
  - The B-spline kinds, planes, surfaces of revolution and extrusion, and (since curve-bounded surfaces answer from their basis) curve-bounded surfaces over them refuse.
  - STEP, offset and volume surfaces run the generic body and answer.
- **Inputs:** the iso line v = 0 of a net singular along v = 0, with the domain u in [0.2, 0.9], v in [0.1, 0.7] (fixture `iso_bounded_singular_v_iso_domain`; generated `gen1582`, v in [0.34185, 0.74665]).
  - On a STEP surface over the net: a point curve at the apex over u in [0.2, 0.9].
  - On the net itself, or on a curve-bounded surface over it: `SM_ERR`, "parameter 0 is outside the domain [0.1, 0.7]".
- **Consequence:** the answer depends on the surface kind, not on the geometry. The unit test `an_iso_line_through_a_singularity_of_a_step_surface_is_a_point_curve` uses a domain whose v range holds v = 0, so it tests the singular branch without relying on either answer.
- **Suspected cause:** the generic body was written for the singular case and its non-singular fallback, and it uses the domain only to trim the curve.
- **PR:** the curve-bounded geometry queries (issue 20163).
