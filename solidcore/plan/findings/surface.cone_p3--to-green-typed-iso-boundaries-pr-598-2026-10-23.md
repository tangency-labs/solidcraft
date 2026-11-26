# surface.cone_p3 to green: typed iso boundaries (PR #598, 2026-10-23)

- `Surface::create_iso_boundaries` (unit `surface.surface_p9`) now returns the typed iso
  curves (`Curve`: `SmLine` for plane lines and cone rulings, `SmEllipse` circles for
  revolution sweeps) instead of their B-spline bases. This fixes the `surface.surface_p11`
  known disagreement above ("boundaries of an `SmPlane`"). It was found through the 30
  coincident-cylinder cases of `surface.cone_p3`, where C++ returned `SmLine`/`SmEllipse`
  (types 14010/14021) and Rust returned B-splines (14033) with otherwise identical data.
  The `surface.surface_p11` generator still uses patches for the other surface; adding
  plane cases is left as a follow-up.
