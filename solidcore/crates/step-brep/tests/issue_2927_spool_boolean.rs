//! Analytical and invariant verification of issue #2927:
//! `boolean_difference` and `boolean_intersection` on the imported 92-face
//! `flanged_spool.step` across half-space box cuts along X, Y and Z.
//!
//! # Geometry taken directly from `flanged_spool.step` vs modelled here
//! - **Exact STEP surface and curve parameters (`fixtures/flanged_spool.step`, 92 faces):**
//!   - Axisymmetric lower/middle body on `z in [0, 128]`: `PLANE`, `CYLINDRICAL_SURFACE`,
//!     `CONICAL_SURFACE`, and four concave `TOROIDAL_SURFACE` fillets (`R0 = 45, rho = 10`
//!     on `[5, 15]`, `[98, 108]`, `[113, 123]` and `R0 = 40, rho = 5` on `[123, 128]`),
//!     stepped inner bore (`r = 31` on `[0, 130]`, `PLANE` annulus at `z = 130`, `r = 35`
//!     on `[130, 140]`), and 24 `r = 4` bolt-hole cylinders at `R = 55` on `[0, 5]` and
//!     `[108, 113]`.
//!   - Scalloped 6-lobe top flange on `z in [128, 140]`: six countersunk bolt holes at
//!     `R = 45` (`r = 5` cylinder on `[128, 139]`, `45 deg` countersink cone `r = 5 + (z - 139)`
//!     on `[139, 140]` tangent to the outer lobes at `z = 140`), six convex lobe
//!     `CYLINDRICAL_SURFACE`s (`R1 = 10` at `(45 cos(m pi/3), 45 sin(m pi/3))`) alternating
//!     with six concave scallop `CYLINDRICAL_SURFACE`s (`R2 = 40` tangent to adjacent lobes
//!     at `d_scallop = (45 sqrt(3) + 5 sqrt(319)) / 2 = 83.622570919029`, matching STEP
//!     `AXIS2_PLACEMENT_3D` centers `#1630..#2107` to 11+ digits), and `45 deg` lower/upper
//!     chamfer `CONICAL_SURFACE`s on `[128, 129]` and `[139, 140]`.
//!   - **Pre-existing STEP B-spline chamfer approximation (measured on the uncut STEP faces):**
//!     82 of the 92 STEP faces carry exact analytic edges (`<= k.tolerances.zone_tol_3d = 1e-5 mm`),
//!     while 10 faces on the top scalloped flange (`z = 128`, `z = 140`, and eight chamfer cones on
//!     `[128, 129]` and `[139, 140]`) carry cubic `B_SPLINE_CURVE` approximations
//!     (`max_etol = 6.66265e-3 mm`). [`measure_step_chamfer_residuals`] integrates only the **uncut**
//!     imported `spool`'s faces (partitioned at `z = 135.0`, where only `n_z == 0` vertical cylinders
//!     span) before any boolean runs, bounding the input chamfer volume offset and equal-and-opposite
//!     vertical flux residuals `+-z_flux` a priori from the STEP edge tolerances and deriving the exact
//!     `bbox.mid().z` shift of `brep_volume` (`iint (z - z_0) n_z dA`, `OdeIsoCurveLength`).
//! - **Test-side reference evaluator:** half-space cross-sections `Area(S(z) intersect {x_i <= c})`
//!   are evaluated in closed form via circular-segment areas (`clipped_disk_area`) on `z < 128`
//!   and Green's-theorem line integrals `oint min(x, c) dy` (`arc_clipped_green`) around the
//!   12 tangent circular arcs on `z in [128, 140]`, then integrated in `z` by 16-node
//!   Gauss-Legendre quadrature split at every profile break and chord-tangency kink with
//!   Hermite cubic endpoint smoothing (verified to `< 1e-8 mm^3` against `closed_form_full_volume`).
//! - **Cut positions:**
//!   - **26 cuts from issue #2927:** `13 y` (`-60, -45, -30, -20, -10, -5, 0, 5, 10, 20, 30, 45, 60`),
//!     `8 x` (`-45, -26, -20, -10, 0, 10, 20, 45`), and `5 z` (`10, 40, 70, 100, 130`).
//!   - **Added by this test:** `6` exterior boundary cuts (`y = -70, 70`, `x = -70, 70`,
//!     `z = -5, 145`) for `32` coarse cuts total, plus `6` fine `z` continuity/collar cuts
//!     (`129.5, 129.99, 130.0, 130.01, 130.5, 135.0`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use core::f64::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_6, PI};
use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{
    brep_properties, brep_volume, face_properties, MassPropertyBehavior, PropertyAttributes,
};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::brep::{calculate_bounding_box as brep_bbox, BoundingBoxOptions};
use solidcore::query::edge::calculate_tight_bounding_box as edge_tight_bbox;
use solidcore::query::face::p1::calculate_bounding_box as face_bbox;
use solidcore::topology::brep::p1::{edges as brep_edges, faces};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1::faceuse_region;
use solidcore::topology::face::p2::PPF_ALL;
use solidcore::{Brep, Kernel, Point3};

/// 16-node Gauss-Legendre nodes and weights on `[-1, 1]`.
const GL16: [(f64, f64); 16] = [
    (-0.989_400_934_991_649_9, 0.027_152_459_411_754_095),
    (-0.944_575_023_073_232_6, 0.062_253_523_938_647_894),
    (-0.865_631_202_387_831_8, 0.095_158_511_682_492_78),
    (-0.755_404_408_355_003, 0.124_628_971_255_533_87),
    (-0.617_876_244_402_643_8, 0.149_595_988_816_576_73),
    (-0.458_016_777_657_227_37, 0.169_156_519_395_002_54),
    (-0.281_603_550_779_258_9, 0.182_603_415_044_923_58),
    (-0.095_012_509_837_637_44, 0.189_450_610_455_068_5),
    (0.095_012_509_837_637_44, 0.189_450_610_455_068_5),
    (0.281_603_550_779_258_9, 0.182_603_415_044_923_58),
    (0.458_016_777_657_227_37, 0.169_156_519_395_002_54),
    (0.617_876_244_402_643_8, 0.149_595_988_816_576_73),
    (0.755_404_408_355_003, 0.124_628_971_255_533_87),
    (0.865_631_202_387_831_8, 0.095_158_511_682_492_78),
    (0.944_575_023_073_232_6, 0.062_253_523_938_647_894),
    (0.989_400_934_991_649_9, 0.027_152_459_411_754_095),
];

/// Integrates `f(z)` on `[a, b]` using 16-node Gauss-Legendre after the smooth Hermite
/// substitution `z(u) = a + (b - a) * (3 u^2 - 2 u^3)` (`u in [0, 1]`), which regularizes
/// endpoint `sqrt(z - a)` and `sqrt(b - z)` chord-entry singularities to analytic terms.
fn integrate_smooth_span(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let span = b - a;
    if span <= 1e-14 {
        return 0.0;
    }
    let mut sum = 0.0;
    for &(xi, wi) in &GL16 {
        let u = 0.5 * (xi + 1.0);
        let phi = u * u * (3.0 - 2.0 * u);
        let dphi = 6.0 * u * (1.0 - u);
        let z = a + span * phi;
        sum += 0.5 * wi * f(z) * (span * dphi);
    }
    sum
}

/// Area of a circular disk of radius `r` centered at the origin inside `{coord <= t}`.
fn clipped_disk_area(r: f64, t: f64) -> f64 {
    if r <= 0.0 || t <= -r {
        0.0
    } else if t >= r {
        PI * r * r
    } else {
        let ratio = (-t / r).clamp(-1.0, 1.0);
        r * r * ratio.acos() + t * (r * r - t * t).max(0.0).sqrt()
    }
}

/// Line integral `int_{t_start}^{t_end} min(x(t), d) dy(t)` along the circular arc
/// `(cx + r cos t, cy + r sin t)`. Summing around a closed CCW Jordan curve yields
/// `Area(P intersect {x <= d})` by Green's theorem.
fn arc_clipped_green_x(cx: f64, _cy: f64, r: f64, t_start: f64, t_end: f64, d: f64) -> f64 {
    let sign = if t_end >= t_start { 1.0 } else { -1.0 };
    let t_min = t_start.min(t_end);
    let t_max = t_start.max(t_end);
    if t_max - t_min <= 1e-15 {
        return 0.0;
    }

    let mut breaks = vec![t_min, t_max];
    let u = (d - cx) / r;
    if u > -1.0 && u < 1.0 {
        let alpha = u.acos();
        for k in -2..=3 {
            let shift = 2.0 * PI * f64::from(k);
            for cand in [alpha + shift, -alpha + shift] {
                if cand > t_min + 1e-13 && cand < t_max - 1e-13 {
                    breaks.push(cand);
                }
            }
        }
        breaks.sort_by(f64::total_cmp);
    }

    let mut total = 0.0;
    for pair in breaks.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if b - a <= 1e-15 {
            continue;
        }
        let mid = 0.5 * (a + b);
        if cx + r * mid.cos() <= d {
            total += cx * r * (b.sin() - a.sin())
                + 0.5 * r * r * ((b - a) + 0.5 * ((2.0 * b).sin() - (2.0 * a).sin()));
        } else {
            total += d * r * (b.sin() - a.sin());
        }
    }
    sign * total
}

/// Green's-theorem contribution of a circular arc to `Area(P intersect {coord[axis] <= d})`.
/// For `axis == 1` (`y <= d`), applies the orientation-preserving `-90 deg` rotation
/// `(X, Y) = (y, -x)` (`dX ^ dY = dx ^ dy`, `{y <= d} == {X <= d}`).
fn arc_clipped_green(
    axis: usize,
    cx: f64,
    cy: f64,
    r: f64,
    t_start: f64,
    t_end: f64,
    d: f64,
) -> f64 {
    if axis == 0 {
        arc_clipped_green_x(cx, cy, r, t_start, t_end, d)
    } else {
        arc_clipped_green_x(cy, -cx, r, t_start - FRAC_PI_2, t_end - FRAC_PI_2, d)
    }
}

/// Outer revolved profile radius `r_out(z)` on `z in [0, 128]` from `flanged_spool.step`:
/// - `[0, 1]`: bottom chamfer cone `64 + z` (STEP Face 72; bottom face `z = 0` is Face 71)
/// - `[1, 4]`: bottom flange cylinder `65` (STEP Face 87)
/// - `[4, 5]`: bottom flange upper chamfer cone `69 - z` (STEP Face 90; top face `z = 5` is Face 88)
/// - `[5, 15]`: lower fillet torus `45 - sqrt(100 - (15 - z)^2)` (STEP Face 91)
/// - `[15, 98]`: barrel cylinder `35` (STEP Face 89)
/// - `[98, 108]`: middle lower fillet torus `45 - sqrt(100 - (z - 98)^2)` (STEP Face 86)
/// - `[108, 109]`: middle flange lower chamfer cone `64 + (z - 108)` (STEP Face 85; Face 69 at `z = 108`)
/// - `[109, 112]`: middle flange cylinder `65` (STEP Face 70)
/// - `[112, 113]`: middle flange upper chamfer cone `65 - (z - 112)` (STEP Face 56; Face 54 at `z = 113`)
/// - `[113, 123]`: upper fillet torus `45 - sqrt(100 - (123 - z)^2)` (STEP Face 52)
/// - `[123, 128]`: neck fillet torus `40 - sqrt(25 - (z - 123)^2)` (STEP Face 19)
fn revolved_outer_radius(z: f64) -> f64 {
    if z <= 1.0 {
        64.0 + z
    } else if z <= 4.0 {
        65.0
    } else if z <= 5.0 {
        69.0 - z
    } else if z <= 15.0 {
        let dz = 15.0 - z;
        45.0 - (100.0 - dz * dz).max(0.0).sqrt()
    } else if z <= 98.0 {
        35.0
    } else if z <= 108.0 {
        let dz = z - 98.0;
        45.0 - (100.0 - dz * dz).max(0.0).sqrt()
    } else if z <= 109.0 {
        64.0 + (z - 108.0)
    } else if z <= 112.0 {
        65.0
    } else if z <= 113.0 {
        65.0 - (z - 112.0)
    } else if z <= 123.0 {
        let dz = 123.0 - z;
        45.0 - (100.0 - dz * dz).max(0.0).sqrt()
    } else {
        let dz = z - 123.0;
        40.0 - (25.0 - dz * dz).max(0.0).sqrt()
    }
}

/// Area of the 12-arc scalloped top flange outer boundary `P(z)` (`z in [128, 140]`)
/// clipped to `{coord[axis] <= d}`:
/// - `[128, 129]`: lower chamfer cones, `delta = 129 - z` (STEP Faces 4..=15; Face 2 at `z = 128`)
/// - `[129, 139]`: 6 lobe cylinders `R1 = 10` + 6 concave scallop cylinders `R2 = 40` (STEP Faces 40..=51)
/// - `[139, 140]`: upper chamfer cones, `delta = z - 139` (STEP Faces 22..=33; Face 3 at `z = 140`)
fn scalloped_outer_clipped_area(z: f64, axis: usize, d: f64) -> f64 {
    let delta = if z < 129.0 {
        129.0 - z
    } else if z > 139.0 {
        z - 139.0
    } else {
        0.0
    };
    let r1 = 10.0 - delta;
    let r2 = 40.0 + delta;
    let d_scallop = 0.5 * (45.0 * 3.0_f64.sqrt() + 5.0 * 319.0_f64.sqrt());
    let s0x = d_scallop * FRAC_PI_6.cos();
    let s0y = d_scallop * FRAC_PI_6.sin();
    let alpha = s0y.atan2(s0x - 45.0);

    let mut area = 0.0;
    for m in 0..6 {
        let theta = f64::from(m) * FRAC_PI_3;
        let psi = theta + FRAC_PI_6;
        let cx = 45.0 * theta.cos();
        let cy = 45.0 * theta.sin();
        let sx = d_scallop * psi.cos();
        let sy = d_scallop * psi.sin();

        // Convex lobe arc around C_m (CCW from theta - alpha to theta + alpha).
        area += arc_clipped_green(axis, cx, cy, r1, theta - alpha, theta + alpha, d);
        // Concave scallop arc around S_m (CW from theta + pi + alpha down to theta + 4*pi/3 - alpha).
        area += arc_clipped_green(
            axis,
            sx,
            sy,
            r2,
            theta + PI + alpha,
            theta + 4.0 * FRAC_PI_3 - alpha,
            d,
        );
    }
    area
}

/// Exact cross-sectional area of `flanged_spool.step` at height `z in [0, 140]` inside
/// `{coord[axis] <= d}` (`d = +infinity` gives the full cross-section `A_full(z)`).
fn cross_section_area(z: f64, axis: usize, d: f64) -> f64 {
    // Inner bore: r = 31 on [0, 130] (STEP Face 55), step plane at z = 130 (Face 53),
    // r = 35 on [130, 140] (Face 34).
    let r_in = if z < 130.0 { 31.0 } else { 35.0 };

    if z < 128.0 {
        let r_out = revolved_outer_radius(z);
        let mut area = clipped_disk_area(r_out, d) - clipped_disk_area(r_in, d);
        // 12 bolt holes of radius 4 at R = 55 on bottom [0, 5] (Faces 73..=84) and
        // middle [108, 113] (Faces 57..=68) flanges.
        if z <= 5.0 || (108.0..=113.0).contains(&z) {
            for k in 0..12 {
                let ang = f64::from(k) * FRAC_PI_6;
                let c = if axis == 0 {
                    55.0 * ang.cos()
                } else {
                    55.0 * ang.sin()
                };
                area -= clipped_disk_area(4.0, d - c);
            }
        }
        area
    } else {
        let mut area = scalloped_outer_clipped_area(z, axis, d) - clipped_disk_area(r_in, d);
        // 6 countersunk bolt holes at R = 45: cylinder r = 5 on [128, 139]
        // (STEP Faces 0, 16..=18, 20..=21) and cone r = 5 + (z - 139) on [139, 140]
        // (STEP Faces 1, 35..=39).
        let r_hole = if z <= 139.0 { 5.0 } else { 5.0 + (z - 139.0) };
        for m in 0..6 {
            let ang = f64::from(m) * FRAC_PI_3;
            let c = if axis == 0 {
                45.0 * ang.cos()
            } else {
                45.0 * ang.sin()
            };
            area -= clipped_disk_area(r_hole, d - c);
        }
        area
    }
}

/// Exact analytical volume of `A \ {coord[axis] >= at}` (`A intersect {coord[axis] <= at}`)
/// computed from `flanged_spool.step`'s analytic definitions.
fn expected_cut_volume(axis: usize, at: f64) -> f64 {
    if axis == 2 {
        // Horizontal cut z <= at: integrate full cross-section A_full(z) from 0 to clamp(at, 0, 140).
        let z_hi = at.clamp(0.0, 140.0);
        let base_breaks = [
            0.0, 1.0, 4.0, 5.0, 15.0, 98.0, 108.0, 109.0, 112.0, 113.0, 123.0, 128.0, 129.0, 130.0,
            139.0, 140.0,
        ];
        let mut z_breaks = vec![0.0, z_hi];
        for &b in &base_breaks {
            if b > 0.0 && b < z_hi {
                z_breaks.push(b);
            }
        }
        z_breaks.sort_by(f64::total_cmp);
        let mut vol = 0.0;
        for pair in z_breaks.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if b - a > 1e-14 {
                vol += integrate_smooth_span(a, b, |z| cross_section_area(z, 0, 1e6));
            }
        }
        return vol;
    }

    // Vertical cut x <= at (axis == 0) or y <= at (axis == 1): split at every profile
    // break and every chord-tangency kink where r(z) == |at - c|.
    let mut z_breaks = vec![
        0.0, 1.0, 4.0, 5.0, 15.0, 98.0, 108.0, 109.0, 112.0, 113.0, 123.0, 128.0, 129.0, 130.0,
        139.0, 140.0,
    ];
    let ad = at.abs();
    if ad > 64.0 && ad < 65.0 {
        z_breaks.push(ad - 64.0);
        z_breaks.push(69.0 - ad);
        z_breaks.push(108.0 + (ad - 64.0));
        z_breaks.push(112.0 + (65.0 - ad));
    }
    if ad > 35.0 && ad < 45.0 {
        let dz = (100.0 - (45.0 - ad) * (45.0 - ad)).max(0.0).sqrt();
        z_breaks.push(15.0 - dz);
        z_breaks.push(98.0 + dz);
        z_breaks.push(123.0 - dz);
    }
    if ad > 35.0 && ad < 40.0 {
        let dz = (25.0 - (40.0 - ad) * (40.0 - ad)).max(0.0).sqrt();
        z_breaks.push(123.0 + dz);
    }
    // Top scalloped flange chamfer kinks on [128, 129] and [139, 140]:
    let d_scallop = 0.5 * (45.0 * 3.0_f64.sqrt() + 5.0 * 319.0_f64.sqrt());
    for m in 0..6 {
        let theta = f64::from(m) * FRAC_PI_3;
        let psi = theta + FRAC_PI_6;
        let c_lobe = if axis == 0 {
            45.0 * theta.cos()
        } else {
            45.0 * theta.sin()
        };
        let c_scal = if axis == 0 {
            d_scallop * psi.cos()
        } else {
            d_scallop * psi.sin()
        };
        // Countersink cone r_h(z) = 5 + (z - 139) == |at - c_lobe| on [139, 140]
        let dh = (at - c_lobe).abs();
        if dh > 5.0 && dh < 6.0 {
            z_breaks.push(139.0 + (dh - 5.0));
        }
        // Lobe chamfer cone R1(z) = 10 - delta(z) == |at - c_lobe| (R1 in (9, 10))
        if dh > 9.0 && dh < 10.0 {
            let delta = 10.0 - dh;
            z_breaks.push(129.0 - delta);
            z_breaks.push(139.0 + delta);
        }
        // Scallop chamfer cone R2(z) = 40 + delta(z) == |at - c_scal| (R2 in (40, 41))
        let ds = (at - c_scal).abs();
        if ds > 40.0 && ds < 41.0 {
            let delta = ds - 40.0;
            z_breaks.push(129.0 - delta);
            z_breaks.push(139.0 + delta);
        }
    }
    // Subdivide long cylinder barrel [15, 98] and [129, 139] for uniform high precision.
    for z_mid in [35.0, 56.0, 77.0, 134.0] {
        z_breaks.push(z_mid);
    }

    z_breaks.sort_by(f64::total_cmp);
    let mut vol = 0.0;
    for pair in z_breaks.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if b - a > 1e-12 {
            let mid = 0.5 * (a + b);
            vol += integrate_smooth_span(a, mid, |z| cross_section_area(z, axis, at));
            vol += integrate_smooth_span(mid, b, |z| cross_section_area(z, axis, at));
        }
    }
    vol
}

/// Pure closed-form antiderivative of the full unclipped spool volume `V(A)` as an
/// independent check on the quadrature routine:
/// - Cone frustum `[0, h]` with linear radius `r0 -> r1`: `pi * h / 3 * (r0^2 + r0 r1 + r1^2)`
/// - Concave torus span of height `H` (`H = rho` quarter-torus, `r(u) = R0 - sqrt(rho^2 - u^2)`):
///   `pi * int_0^rho (R0^2 - 2 R0 sqrt(rho^2 - u^2) + rho^2 - u^2) du`
///   `= pi * (R0^2 rho - pi/2 R0 rho^2 + 2/3 rho^3)`.
fn closed_form_full_volume() -> f64 {
    let frustum = |r0: f64, r1: f64, h: f64| PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    let quarter_concave_torus = |r0: f64, rho: f64| {
        PI * (r0 * r0 * rho - 0.5 * PI * r0 * rho * rho + 2.0 / 3.0 * rho.powi(3))
    };

    // Outer solid on [0, 128]:
    let mut v = 0.0;
    v += frustum(64.0, 65.0, 1.0); // [0, 1]
    v += PI * 65.0 * 65.0 * 3.0; // [1, 4]
    v += frustum(65.0, 64.0, 1.0); // [4, 5]
    v += quarter_concave_torus(45.0, 10.0); // [5, 15]
    v += PI * 35.0 * 35.0 * 83.0; // [15, 98]
    v += quarter_concave_torus(45.0, 10.0); // [98, 108]
    v += frustum(64.0, 65.0, 1.0); // [108, 109]
    v += PI * 65.0 * 65.0 * 3.0; // [109, 112]
    v += frustum(65.0, 64.0, 1.0); // [112, 113]
    v += quarter_concave_torus(45.0, 10.0); // [113, 123]
    v += quarter_concave_torus(40.0, 5.0); // [123, 128]
                                           // Subtract bore r = 31 on [0, 128] and 2 * 12 bolt holes (r = 4, h = 5) on [0, 5] & [108, 113]:
    v -= PI * 31.0 * 31.0 * 128.0;
    v -= 24.0 * PI * 4.0 * 4.0 * 5.0;

    // Top scalloped flange on [128, 140]:
    // Notice Area(P(delta)) is a quadratic polynomial in delta (`[0, 1]`), so Simpson's
    // rule `(A(0) + 4 A(0.5) + A(1)) / 6` is algebraically exact for `int_0^1 Area(P(delta)) d(delta)`!
    let a0 = scalloped_outer_clipped_area(134.0, 0, 1e6); // delta = 0
    let a_half = scalloped_outer_clipped_area(128.5, 0, 1e6); // delta = 0.5
    let a1 = scalloped_outer_clipped_area(128.0, 0, 1e6); // delta = 1.0
    let chamfer_vol = (a0 + 4.0 * a_half + a1) / 6.0;
    v += 10.0 * a0 + 2.0 * chamfer_vol;
    // Subtract upper bore (`r = 31` on `[128, 130]`, `r = 35` on `[130, 140]`) and 6 countersunk holes:
    v -= PI * 31.0 * 31.0 * 2.0 + PI * 35.0 * 35.0 * 10.0;
    v -= 6.0 * (PI * 5.0 * 5.0 * 11.0 + frustum(5.0, 6.0, 1.0));
    v
}

fn volume(k: &Kernel, b: &Brep) -> f64 {
    if faces(b, None).unwrap().is_empty() {
        return 0.0;
    }
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = b.clone_standing_for_self().unwrap();
    brep_volume(k, &mut copy, 1e-8, &attrs).unwrap()
}

fn volume_about_zero(k: &Kernel, b: &Brep) -> f64 {
    if faces(b, None).unwrap().is_empty() {
        return 0.0;
    }
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = b.clone_standing_for_self().unwrap();
    brep_properties(
        k,
        &mut copy,
        1e-8,
        Point3::ZERO,
        MassPropertyBehavior::default(),
        &attrs,
    )
    .unwrap()
    .volume
}

fn assert_valid_brep(k: &mut Kernel, b: &Brep, label: &str) {
    let mut copy = b.clone_standing_for_self().unwrap();
    let chk = check_brep(k, &mut copy, CheckLevel::Level2).unwrap();
    assert!(
        chk.pointer_reports.is_empty(),
        "{label}: pointer errors: {:?}",
        chk.pointer_reports
    );
    assert_eq!(
        chk.assert_valid,
        Some(true),
        "{label}: assert_valid failed: {:?}",
        chk.reports.reports()
    );
    assert!(
        chk.reports.reports().is_empty(),
        "{label}: validation reports: {:?}",
        chk.reports.reports()
    );
}

fn halfspace_box(k: &mut Kernel, axis: usize, at: f64) -> Brep {
    let mut o = [-1000.0; 3];
    let mut s = [2000.0; 3];
    o[axis] = at;
    s[axis] = 1000.0 - at;
    create_box(k, Point3::new(o[0], o[1], o[2]), s[0], s[1], s[2]).unwrap()
}

/// Reference analytic full volume `V(A)` (`mm^3`) from [`closed_form_full_volume`].
const REF_FULL_VOL_MM3: f64 = 248_052.032_028;
/// Maximum `|z - bbox.mid().z|` (`mm`) on the `130 x 130 x 140 mm` spool (`140 / 2`).
const CAP_MAX_HALF_SPAN_MM: f64 = 70.0;
/// Upper bound on cut-cap perimeter (`mm`; maximum across the cuts is `~596 mm` at `y = 0`).
const CAP_MAX_PERIMETER_MM: f64 = 620.0;
/// `Plane::drop_curve` cubic B-spline UV trim approximation tolerance (`mm`).
const PLANE_DROP_CURVE_TOL_MM: f64 = 5e-6;
/// Relative accuracy passed to [`brep_volume`] and [`brep_properties`].
const INTEGRATOR_REL_TOL: f64 = 1e-8;
/// Unidirectional `z`-divergence trim + quadrature bound (`mm^3`):
/// `brep_volume` (`OdeIsoCurveLength`, `topology::face::p2:2139-2146`) integrates
/// `iint (z - bbox.mid().z) n_z dA` (`div((z - z_0) z_hat) = 1`, factor `1`, not `1/3`).
/// Shifting an intersection trim curve of perimeter `P <= 620 mm` by `<= 5e-6 mm`
/// (`Plane::drop_curve` cubic B-spline UV trim fit) perturbs projected horizontal area
/// `|delta(n_z dA)| <= P * 5e-6` at `|z - z_mid| <= 70 mm`, giving at most
/// `70 * 620 * 5e-6 = 0.217000 mm^3` (vs `140 * 600 * 5e-6 = 0.42 mm^3` before `bbox.mid()`
/// centering), plus `1e-8 * V(A) = 0.002481 mm^3`, totaling `0.219481 mm^3`.
const CAP_AND_QUAD_TOL_MM3: f64 =
    CAP_MAX_HALF_SPAN_MM * CAP_MAX_PERIMETER_MM * PLANE_DROP_CURVE_TOL_MM
        + INTEGRATOR_REL_TOL * REF_FULL_VOL_MM3;

/// Quadrature-level tolerance (`mm^3`, `2 * 1e-8 * V(A) = 0.004961 mm^3`) for exact-circle
/// horizontal cuts, common-origin (`Point3::ZERO`) additivity, and uncut-face-compensated
/// collar cuts (measured `< 1e-4 mm^3`).
const EXACT_CAP_QUAD_TOL_MM3: f64 = 2.0 * INTEGRATOR_REL_TOL * REF_FULL_VOL_MM3;

/// Upper bound on the 6-lobe scalloped outer perimeter (`mm`; exact perimeter at `z in [129, 139]`
/// is `6 * (10 * 2.132 + 40 * 1.085) = 388.3 mm`, plus 6 `r = 5` holes `188.5 mm` < `600 mm`).
const SCALLOP_CHAMFER_PERIMETER_BOUND_MM: f64 = 600.0;

/// Pre-existing input residuals of `flanged_spool.step`'s cubic B-spline chamfer faces on
/// `z in [128, 129]` and `z in [139, 140]`, measured **solely on the uncut imported `spool`**
/// before any boolean operation runs (zero calibration against boolean outputs):
/// - Because `OdeIsoCurveLength` integrates `v(z_0) = iint (z - z_0) n_z dA`, evaluating each
///   uncut face at `origin = (0, 0, 0)` (`v0`) and `origin = (0, 0, 1)` (`v1`) yields both
///   `v0(f) = iint_f z n_z dA` and `flux_z(f) = v0(f) - v1(f) = iint_f n_z dA`.
/// - Every face of `spool` spanning `z = 135.0` is a vertical cylinder (`n_z == 0`), so
///   partitioning the uncut faces into `fbox.max().z < 135.0` vs `fbox.min().z > 135.0`
///   isolates the lower `[128, 129]` chamfer residual (`lower_z_flux`, `lower_zero_offset`) and
///   upper `[139, 140]` chamfer residual (`upper_z_flux`, `upper_zero_offset`) against the exact
///   collar cross-section area `A_collar = cross_section_area(135.0, 0, 1e6)` and
///   `expected_cut_volume(2, 130.0)`.
/// - Moving `z_0` from `0` to `bbox.mid().z` (`c / 2` for `diff`, `(c + 140) / 2` for `inter`,
///   and `70` for `orig` on `c in [129, 139]`) shifts `diff` by `-(c / 2) * lower_z_flux`,
///   `inter` by `-((c + 140) / 2) * upper_z_flux`, and `V(diff) + V(inter) - V(orig)` by
///   `(70 - c / 2) * lower_z_flux - (c / 2) * upper_z_flux`, checking uncut-face integral
///   preservation across the boolean to `EXACT_CAP_QUAD_TOL_MM3` (`0.004961 mm^3`).
#[derive(Debug, Clone, Copy)]
struct StepChamferResiduals {
    step_vol_offset: f64,
    lower_zero_offset: f64,
    upper_zero_offset: f64,
    lower_z_flux: f64,
    upper_z_flux: f64,
}

fn measure_step_chamfer_residuals(
    k: &Kernel,
    spool: &Brep,
    v_exact_full: f64,
) -> StepChamferResiduals {
    // 1. Check edge tolerances on the uncut STEP solid: all edges below `z = 127.5` sit at
    //    `k.tolerances.zone_tol_3d` (`1e-5 mm`), while the B-spline chamfer edges on `[128, 140]`
    //    carry `~6.663e-3 mm`.
    let mut max_chamfer_etol = 0.0_f64;
    for e in brep_edges(spool, None).unwrap() {
        let etol = edge::tolerance(spool, e).unwrap();
        let ebox = edge_tight_bbox(k, spool, e).unwrap().unwrap();
        if ebox.max().z < 127.5 {
            assert!(
                etol <= k.tolerances.zone_tol_3d,
                "expected exact edge below z = 127.5 (<= {}), got etol = {etol}",
                k.tolerances.zone_tol_3d
            );
        } else {
            max_chamfer_etol = max_chamfer_etol.max(etol);
        }
    }
    assert!(
        max_chamfer_etol > 1e-3 && max_chamfer_etol < 1e-2,
        "unexpected max chamfer edge tolerance {max_chamfer_etol}"
    );

    // 2. Integrate each face of the UNCUT spool at `z_0 = 0` and `z_0 = 1` using the exact
    //    material-bounding faceuse orientation of `brep_properties`.
    let mut copy = spool.clone_standing_for_self().unwrap();
    let bbox = brep_bbox(k, &mut copy, BoundingBoxOptions::default()).unwrap();
    let sz = bbox.size();
    let max_dim = sz.z.max(sz.x.max(sz.y));
    let estimated = max_dim * max_dim / 100.0;

    let mut lower_v0 = 0.0;
    let mut lower_flux = 0.0;
    let mut upper_v0 = 0.0;
    let mut upper_flux = 0.0;

    for f in faces(&copy, None).unwrap() {
        let up = copy.face(f).unwrap().faceuse.unwrap();
        let mate = copy.faceuse(up).unwrap().mate.unwrap();
        let fu = [up, mate]
            .into_iter()
            .find(|&cand| {
                let reg = faceuse_region(&copy, cand).unwrap().unwrap();
                copy.region(reg).unwrap().is_void() == 0
            })
            .expect("manifold solid face has one material faceuse");
        let orient = copy.faceuse(fu).unwrap().orientation;
        let p0 = face_properties(
            k,
            &mut copy,
            f,
            orient,
            INTEGRATOR_REL_TOL,
            Point3::ZERO,
            estimated,
            0.0,
            PPF_ALL,
        )
        .unwrap();
        let p1 = face_properties(
            k,
            &mut copy,
            f,
            orient,
            INTEGRATOR_REL_TOL,
            Point3::new(0.0, 0.0, 1.0),
            estimated,
            0.0,
            PPF_ALL,
        )
        .unwrap();
        let v0 = p0.volume;
        let flux = p0.volume - p1.volume;
        let fbox = face_bbox(k, &copy, f, false).unwrap();
        if fbox.max().z < 135.0 {
            lower_v0 += v0;
            lower_flux += flux;
        } else if fbox.min().z > 135.0 {
            upper_v0 += v0;
            upper_flux += flux;
        } else {
            // Every face spanning `z = 135.0` is a vertical cylinder (`n_z == 0`).
            assert!(
                v0.abs() < 1e-11 && flux.abs() < 1e-11,
                "spanning face at z = 135 had nonzero z-integral: v0={v0}, flux={flux}"
            );
        }
    }

    let a_collar = cross_section_area(135.0, 0, 1e6);
    let v_exp_130 = expected_cut_volume(2, 130.0);
    let lower_z_flux = lower_flux + a_collar;
    let upper_z_flux = upper_flux - a_collar;
    // Closed-surface divergence theorem on the uncut B-rep: net vertical flux vanishes up to
    // trim/quadrature tolerance (`~1.04e-6 mm^2`, `< INTEGRATOR_REL_TOL * REF_FULL_VOL_MM3`).
    assert!(
        (lower_z_flux + upper_z_flux).abs() <= INTEGRATOR_REL_TOL * REF_FULL_VOL_MM3,
        "lower ({lower_z_flux}) and upper ({upper_z_flux}) chamfer z-flux residuals must cancel on uncut body"
    );

    let lower_zero_offset = (lower_v0 + 130.0 * a_collar) - v_exp_130;
    let upper_zero_offset = (upper_v0 - 130.0 * a_collar) - (v_exact_full - v_exp_130);
    let step_vol_offset =
        (lower_zero_offset + upper_zero_offset) - 70.0 * (lower_z_flux + upper_z_flux);

    // A priori geometric bounds from the uncut STEP file's chamfer edge tolerance (`~6.663e-3 mm`):
    let apriori_flux_bound = SCALLOP_CHAMFER_PERIMETER_BOUND_MM * max_chamfer_etol;
    let apriori_vol_bound = 2.0 * SCALLOP_CHAMFER_PERIMETER_BOUND_MM * 1.5 * max_chamfer_etol;
    assert!(
        lower_z_flux.abs().max(upper_z_flux.abs()) <= apriori_flux_bound,
        "chamfer z-flux ({lower_z_flux}, {upper_z_flux}) exceeds a priori edge-tolerance bound {apriori_flux_bound}"
    );
    assert!(
        step_vol_offset.abs() <= apriori_vol_bound,
        "STEP chamfer volume offset {step_vol_offset} exceeds a priori bound {apriori_vol_bound}"
    );

    StepChamferResiduals {
        step_vol_offset,
        lower_zero_offset,
        upper_zero_offset,
        lower_z_flux,
        upper_z_flux,
    }
}

struct VerifiedSpool {
    spool: Brep,
    v_exact_full: f64,
    v_orig: f64,
    v_orig_zero: f64,
    chamfer: StepChamferResiduals,
}

fn import_verified_spool(k: &mut Kernel) -> VerifiedSpool {
    let v_exact_full = closed_form_full_volume();
    let v_quad_full = expected_cut_volume(2, 140.0);
    assert!(
        (v_exact_full - v_quad_full).abs() < 1e-8,
        "quadrature {v_quad_full} vs closed form {v_exact_full}"
    );
    assert!(
        (expected_cut_volume(0, 0.0) - 0.5 * v_exact_full).abs() < 1e-8,
        "x <= 0 half-volume mismatch"
    );
    assert!(
        (expected_cut_volume(1, 0.0) - 0.5 * v_exact_full).abs() < 1e-8,
        "y <= 0 half-volume mismatch"
    );

    let bytes = include_bytes!("fixtures/flanged_spool.step");
    let report = step_brep::import_with_report(bytes, &step_import::Limits::default()).unwrap();
    assert_eq!(report.bodies.len(), 1);
    assert!(
        report.face_failures.is_empty(),
        "unexpected STEP face failures: {:?}",
        report.face_failures
    );
    let spool = report.bodies.into_iter().next().unwrap();
    assert_eq!(faces(&spool, None).unwrap().len(), 92);

    assert_valid_brep(k, &spool, "imported spool");
    let v_orig = volume(k, &spool);
    let v_orig_zero = volume_about_zero(k, &spool);
    let chamfer = measure_step_chamfer_residuals(k, &spool, v_exact_full);
    assert!(
        (v_orig - (v_exact_full + chamfer.step_vol_offset)).abs() <= EXACT_CAP_QUAD_TOL_MM3,
        "imported spool volume {v_orig} vs exact + uncut STEP chamfer offset {}",
        v_exact_full + chamfer.step_vol_offset
    );
    VerifiedSpool {
        spool,
        v_exact_full,
        v_orig,
        v_orig_zero,
        chamfer,
    }
}

fn verify_axis_sweep(
    k: &mut Kernel,
    s: &VerifiedSpool,
    axis: usize,
    positions: &[i64],
    symmetric_ds: &[i64],
    max_area: f64,
) {
    let spool = &s.spool;
    let v_exact_full = s.v_exact_full;
    let v_orig = s.v_orig;
    let v_orig_zero = s.v_orig_zero;
    let chamfer = s.chamfer;
    let vertical_cut_tol = CAP_AND_QUAD_TOL_MM3 + chamfer.step_vol_offset.abs();

    let ax = ["x", "y", "z"][axis];
    let mut measured: std::collections::BTreeMap<i64, (f64, f64)> =
        std::collections::BTreeMap::new();

    for &d_int in positions {
        let at = d_int as f64;
        let label_diff = format!("diff {ax} > {at}");
        let label_inter = format!("inter {ax} > {at}");

        let tool_diff = halfspace_box(k, axis, at);
        let mut diff = boolean_difference(k, spool.clone_standing_for_self().unwrap(), tool_diff)
            .unwrap_or_else(|e| panic!("{label_diff} returned Err: {e:?}"));
        diff.refresh_cached_topology();

        let tool_inter = halfspace_box(k, axis, at);
        let mut inter =
            boolean_intersection(k, spool.clone_standing_for_self().unwrap(), tool_inter)
                .unwrap_or_else(|e| panic!("{label_inter} returned Err: {e:?}"));
        inter.refresh_cached_topology();

        let diff_empty = faces(&diff, None).unwrap().is_empty();
        let inter_empty = faces(&inter, None).unwrap().is_empty();
        if !diff_empty {
            assert_valid_brep(k, &diff, &label_diff);
        }
        if !inter_empty {
            assert_valid_brep(k, &inter, &label_inter);
        }

        let v_diff = volume(k, &diff);
        let v_inter = volume(k, &inter);
        let v_exp_diff = expected_cut_volume(axis, at);
        let v_exp_inter = v_exact_full - v_exp_diff;

        // Range invariant: 0 <= V(A \ H) <= V(A), empty iff the half-space clears the body.
        if v_exp_diff <= 1e-12 {
            assert!(
                diff_empty,
                "{label_diff}: expected empty B-rep, got {v_diff}"
            );
        } else if (v_exp_diff - v_exact_full).abs() <= 1e-10 {
            assert!(
                inter_empty,
                "{label_inter}: expected empty B-rep, got {v_inter}"
            );
        } else {
            assert!(
                v_diff > 0.0 && v_diff < v_orig,
                "{label_diff}: {v_diff} out of (0, {v_orig})"
            );
            assert!(
                v_inter > 0.0 && v_inter < v_orig,
                "{label_inter}: {v_inter} out of (0, {v_orig})"
            );
        }

        // Two-sided agreement with closed-form / quadrature reference and default-origin additivity.
        if axis == 2 && (129.0..=139.0).contains(&at) {
            let exp_diff_shifted =
                v_exp_diff + (chamfer.lower_zero_offset - 0.5 * at * chamfer.lower_z_flux);
            let exp_inter_shifted = v_exp_inter
                + (chamfer.upper_zero_offset - 0.5 * (at + 140.0) * chamfer.upper_z_flux);
            let exp_additivity_shift =
                (70.0 - 0.5 * at) * chamfer.lower_z_flux - 0.5 * at * chamfer.upper_z_flux;
            assert!(
                (v_diff - exp_diff_shifted).abs() <= EXACT_CAP_QUAD_TOL_MM3,
                "{label_diff}: got {v_diff:.6}, expected origin-shifted {exp_diff_shifted:.6}"
            );
            assert!(
                (v_inter - exp_inter_shifted).abs() <= EXACT_CAP_QUAD_TOL_MM3,
                "{label_inter}: got {v_inter:.6}, expected origin-shifted {exp_inter_shifted:.6}"
            );
            assert!(
                ((v_diff + v_inter - v_orig) - exp_additivity_shift).abs()
                    <= EXACT_CAP_QUAD_TOL_MM3,
                "additivity {ax} > {at}: got {:+.6}, expected {exp_additivity_shift:+.6}",
                (v_diff + v_inter) - v_orig
            );
        } else if axis == 2 {
            let exp_diff_b = if at >= 140.0 {
                v_exp_diff + chamfer.step_vol_offset
            } else {
                v_exp_diff
            };
            let exp_inter_b = if at <= 128.0 {
                v_exp_inter + chamfer.step_vol_offset
            } else {
                v_exp_inter
            };
            assert!(
                (v_diff - exp_diff_b).abs() <= EXACT_CAP_QUAD_TOL_MM3,
                "{label_diff}: got {v_diff:.6}, expected {exp_diff_b:.6}"
            );
            assert!(
                (v_inter - exp_inter_b).abs() <= EXACT_CAP_QUAD_TOL_MM3,
                "{label_inter}: got {v_inter:.6}, expected {exp_inter_b:.6}"
            );
            assert!(
                (v_diff + v_inter - v_orig).abs() <= EXACT_CAP_QUAD_TOL_MM3,
                "additivity {ax} > {at}: diff {v_diff:.6} + inter {v_inter:.6} - orig {v_orig:.6}"
            );
        } else {
            assert!(
                (v_diff - v_exp_diff).abs() <= vertical_cut_tol,
                "{label_diff}: got {v_diff:.6}, expected {v_exp_diff:.6} (err {:+.6})",
                v_diff - v_exp_diff
            );
            assert!(
                (v_inter - v_exp_inter).abs() <= vertical_cut_tol,
                "{label_inter}: got {v_inter:.6}, expected {v_exp_inter:.6} (err {:+.6})",
                v_inter - v_exp_inter
            );
            assert!(
                (v_diff + v_inter - v_orig).abs() <= CAP_AND_QUAD_TOL_MM3,
                "additivity {ax} > {at}: diff {v_diff:.6} + inter {v_inter:.6} - orig {v_orig:.6} = {:+.6}",
                (v_diff + v_inter) - v_orig
            );
        }

        // Common-origin (`Point3::ZERO`) additivity cancels both shared cap trims and `bbox.mid()` shifts.
        let sum_zero = volume_about_zero(k, &diff) + volume_about_zero(k, &inter);
        assert!(
            (sum_zero - v_orig_zero).abs() <= EXACT_CAP_QUAD_TOL_MM3,
            "common-origin additivity {ax} > {at}: sum {sum_zero:.6} vs orig {v_orig_zero:.6}"
        );

        measured.insert(d_int, (v_diff, v_inter));
    }

    // Mirror symmetry invariants (`x -> -x` and `y -> -y`).
    for &d in symmetric_ds {
        let (v_diff_pos, v_inter_pos) = measured[&d];
        let (v_diff_neg, v_inter_neg) = measured[&-d];
        assert!(
            (v_diff_pos - v_inter_neg).abs() <= CAP_AND_QUAD_TOL_MM3,
            "{ax} mirror symmetry at d={d}: diff(+d)={v_diff_pos:.6} vs inter(-d)={v_inter_neg:.6}"
        );
        assert!(
            (v_diff_neg - v_inter_pos).abs() <= CAP_AND_QUAD_TOL_MM3,
            "{ax} mirror symmetry at d=-{d}: diff(-d)={v_diff_neg:.6} vs inter(+d)={v_inter_pos:.6}"
        );
    }

    // Monotonicity and Lipschitz continuity across the sorted cut positions:
    // by the triangle inequality, `v(d2) - v(d1) <= (v_exp(d2) - v_exp(d1)) + |err(d2)| + |err(d1)|
    // <= max_area * step + 2 * endpoint_tol`.
    for pair in positions.windows(2) {
        let d1 = pair[0];
        let d2 = pair[1];
        let v1 = measured[&d1].0;
        let v2 = measured[&d2].0;
        let step = (d2 - d1) as f64;
        let endpoint_tol = if axis == 2 {
            EXACT_CAP_QUAD_TOL_MM3
                + (chamfer.lower_zero_offset.abs() + 70.0 * chamfer.lower_z_flux.abs())
        } else {
            vertical_cut_tol
        };
        assert!(
            v2 >= v1 - 2.0 * EXACT_CAP_QUAD_TOL_MM3,
            "axis {ax}: non-monotone volume from {d1} ({v1}) to {d2} ({v2})"
        );
        assert!(
            v2 - v1 <= max_area * step + 2.0 * endpoint_tol,
            "axis {ax}: jump {} exceeds max_area * step + 2 * endpoint_tol between {d1} and {d2}",
            v2 - v1
        );
    }
}

#[test]
fn spool_boolean_y_cuts_closed_forms_and_invariants() {
    // 13 `y` cuts from #2927 (`-60..=60`) + 2 exterior boundary cuts (`-70, 70`).
    let mut k = Kernel::default();
    let verified = import_verified_spool(&mut k);
    verify_axis_sweep(
        &mut k,
        &verified,
        1,
        &[
            -70, -60, -45, -30, -20, -10, -5, 0, 5, 10, 20, 30, 45, 60, 70,
        ],
        &[0, 5, 10, 20, 30, 45, 60, 70],
        18_200.0,
    );
}

#[test]
fn spool_boolean_x_cuts_closed_forms_and_invariants() {
    // 8 `x` cuts from #2927 (`-45..=45`) + 2 exterior boundary cuts (`-70, 70`).
    let mut k = Kernel::default();
    let verified = import_verified_spool(&mut k);
    verify_axis_sweep(
        &mut k,
        &verified,
        0,
        &[-70, -45, -26, -20, -10, 0, 10, 20, 45, 70],
        &[0, 10, 20, 45, 70],
        18_200.0,
    );
}

#[test]
fn spool_boolean_z_cuts_closed_forms_and_invariants() {
    // 4 non-bore-step `z` cuts from #2927 (`10, 40, 70, 100`; `z = 130` added with WR-759)
    // + 2 exterior boundary cuts (`-5, 145`).
    let mut k = Kernel::default();
    let verified = import_verified_spool(&mut k);
    verify_axis_sweep(
        &mut k,
        &verified,
        2,
        &[-5, 10, 40, 70, 100, 145],
        &[],
        10_300.0,
    );

    // Fine continuity and collar sweep across the `z = 130` inner-bore step plane and `z = 135`.
    let chamfer = verified.chamfer;
    let mut prev_fine: Option<(f64, f64)> = None;
    for z_cut in [129.5, 129.99, 130.01, 130.5, 135.0] {
        let tool = halfspace_box(&mut k, 2, z_cut);
        let mut diff = boolean_difference(
            &mut k,
            verified.spool.clone_standing_for_self().unwrap(),
            tool,
        )
        .unwrap();
        diff.refresh_cached_topology();
        assert_valid_brep(&mut k, &diff, &format!("fine z > {z_cut}"));
        let v = volume(&k, &diff);
        let v_exp = expected_cut_volume(2, z_cut)
            + (chamfer.lower_zero_offset - 0.5 * z_cut * chamfer.lower_z_flux);
        assert!(
            (v - v_exp).abs() <= EXACT_CAP_QUAD_TOL_MM3,
            "fine z > {z_cut}: {v:.6} vs {v_exp:.6}"
        );
        if let Some((prev_z, prev_v)) = prev_fine {
            let dz = z_cut - prev_z;
            let dv: f64 = v - prev_v;
            let fine_lip_tol = 2.0 * EXACT_CAP_QUAD_TOL_MM3 + 0.5 * dz * chamfer.lower_z_flux.abs();
            assert!(
                dv > 0.0 && dv <= 10_300.0 * dz + fine_lip_tol,
                "discontinuous jump {dv} across dz={dz} near z = 130..135 collar"
            );
        }
        prev_fine = Some((z_cut, v));
    }
}

#[test]
fn primitive_cylinder_control_cuts_from_issue_2927() {
    // Control from #2927: solid cylinder of matching outer envelope (r = 65, h = 140)
    // cut by the same half-space boxes matches closed-form circular-segment volumes.
    let mut k = Kernel::default();
    let cyl = create_cylinder(&mut k, Point3::ZERO, 65.0, 140.0).unwrap();
    for (axis, at, expected) in [
        (1usize, 0.0, 0.5 * PI * 65.0 * 65.0 * 140.0),
        (1usize, -30.0, clipped_disk_area(65.0, -30.0) * 140.0),
        (2usize, 70.0, PI * 65.0 * 65.0 * 70.0),
    ] {
        let tool = halfspace_box(&mut k, axis, at);
        let mut diff =
            boolean_difference(&mut k, cyl.clone_standing_for_self().unwrap(), tool).unwrap();
        diff.refresh_cached_topology();
        assert_valid_brep(&mut k, &diff, &format!("cylinder axis={axis} at={at}"));
        let v = volume(&k, &diff);
        assert!(
            (v - expected).abs() < 1e-2,
            "cylinder axis={axis} at={at}: got {v}, expected {expected}"
        );
    }
}
