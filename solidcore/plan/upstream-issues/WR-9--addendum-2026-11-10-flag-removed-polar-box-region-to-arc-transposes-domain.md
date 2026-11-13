Updates: WR-9

#### WR-9 (update, 2026-11-10). `polar_box_region_to_arc_transposes_domain` is removed

When `PolarBox::add_vector` (`SmPolarBox::AddVector3d`) relabels a collapsed region as an arc, it rotates the domain to `[lo.y, hi.y] x [-hi.x, -lo.x]`, so the arc's span lies on the x axis that arc operations read. Upstream leaves the span on y. The rotation is now the only path, and the flag is gone. Removed: the field, `add_vector_with_kernel` and `add_vector_opt_in` (the B-spline surface normal boxes call `add_vector`), SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `geom.polar_box` changes in 1 case, `silhouette_cross`. Worked analytically: the second cross box contains (1, 0, 0), (1, 0, -0.1) and (1, -0.01, -0.1) (up to normalisation), a genuine two-dimensional patch. Its longitude/latitude bounds are [0, atan 0.1] x [-atan 0.01, 0], area 9.95e-4, which is solidcore's domain. Upstream's arc of area 0 excludes (1, -0.01, -0.1), so it is not a bound.
- **Tests:** the WR-9 unit test in `geom::polar_box`.
