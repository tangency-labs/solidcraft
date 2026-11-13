# query.tess: BuildTree on a plane face needs the plane's NURB (review of PR #83)

- **Bug fixed.** `SmTessSrfCache::BuildTree` builds its top node from
  `pBSS->GetNaturalUVDomain()` and `SmBSplineSurface(*pBSS)`. Both read the NURB
  through the virtual `GetOrCreateGwNurbPointer`, which makes an `SmPlane`'s or
  `SmSurfOfRevolution`'s missing NURB first. The first port copied `p.base()` as it
  was; a Rust `Plane::new` has no NURB, so the top node got a zero uv domain and
  the tree was degenerate (`adv_plane_build`: node_geom[0] C++ -1.5, Rust 0; 16 of
  40 generated plane cases failed). The port now uses `Plane::base_with_nurb()`, and
  for a surface of revolution runs `make_nurb` on a copy when the NURB is missing.
  A failed `MakeNurb` copies the part as is. Upstream makes the NURB on the face
  surface itself; the port does it on the copy.
- **Coverage gap that hid it.** The shim built only `SmBSplineSurface`. It now also
  builds a bounded `SmPlane(o, x, y, scale, D)` (surface kind 1, same guards as
  surface.surface_cache_p3); 12% of generated cases and 8 adversarial fixtures use
  it. Still not compared: `SmSurfOfRevolution` faces, face-owned surfaces (the
  `GetTree` rebuild branch), CurveBounded/STEP faces (base `BuildTree`, unported
  until surface.surface_p6), and inputs the FFI rejects although upstream defines
  them (Implant without a tree, Test without a driver, Mark without a poly B-rep).
