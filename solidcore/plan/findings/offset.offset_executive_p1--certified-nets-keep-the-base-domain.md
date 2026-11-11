# The certified nets of the p1 replace cases keep the base's domain

- **Unit:** `offset.offset_executive_p1`, its 11 `replace_*` cases:
  `OffsetExecutive::replace_implicit_offsets` on one implicit offset face each.
- **Before (`N_SrfOffset`'s sampled fit):** each net was reparametrised to `[0,0,1,1]`
  (`[0,0,1,1.5]` for `replace_extrusion_cubic_oblique`), so the unit's samples sat at other
  (u, v) than the base's.
- **After (`certified_offset`, the default):** the net keeps the base's parametrisation, so
  `natural_domain` is the base's domain and the samples move with it. Each net was checked by a
  scratch probe against the exact offset S + d n at the same (u, v) on a 41 × 41 grid; the
  worst gap is within the case's approximation tolerance:

| case | old domain | new domain | gap to exact | tol |
|---|---|---|---|---|
| `replace_bspline_bump` | [0,0,1,1] | [0,0,1,1] | 3.023e-6 | 1e-5 |
| `replace_extrusion_arc_noswap` | [0,0,1,1] | [0,-1,1,1] | 3.610e-5 | 1e-4 |
| `replace_extrusion_arc_swap` | [0,0,1,1] | [-1,0,1,1] | 2.166e-5 | 1e-4 |
| `replace_extrusion_cubic_noswap_in`, `replace_extrusion_cubic_noswap_out` | [0,0,1,1] | [0,0,1,2] | 3.708e-6 | 1e-5 |
| `replace_extrusion_cubic_oblique` | [0,0,1,1.5] | [0,0,1,1.5] | 3.133e-4 | 1e-3 |
| `replace_extrusion_cubic_swap_in`, `replace_extrusion_cubic_swap_out` | [0,0,1,1] | [0,0,2,1] | 3.708e-6 | 1e-5 |
| `replace_extrusion_cubic_tight_tol` | [0,0,1,1] | [0,0,1,2] | 4.269e-8 | 1e-7 |
| `replace_extrusion_cubic_tiny_scale` | [0,0,1,1] | [0,0,1,2] | 4.100e-10 | 1e-9 |
| `replace_plane` | [0,0,1,1] | [0,0,1,1] | 4.871e-16 | 1e-5 |
