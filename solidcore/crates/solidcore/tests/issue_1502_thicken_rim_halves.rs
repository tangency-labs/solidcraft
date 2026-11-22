//! tangency-labs/solidcore#1502: thickening a sphere cap whose rim is two half circles.
//! Upstream, `TrimExtendedEdge` can take the first trim point's twin across the lifted
//! offset circle's seam as an open rim half's second trim point. Each half then gets
//! the whole circle and the shell fails (1001).
//!
//! Test-only: the issue does not reproduce in the Rust port. On these bodies each rim
//! half's second trim point already lands on its own other vertex: the fork's guard,
//! tried as an opt-in, changed no result.
//!
//! Each body is a ball (r 10) cut by a half space, shelled open at the flat face by +1
//! or -1, with and without an offset solid. One cut is at the equator and one through
//! the poles, so the rim crosses the sphere's seam differently. The ninth body is the
//! two stitched quarter-sphere sheets thickened by +1. All runs rely on
//! `TrimExtendedEdge` taking the arc where its base edge runs (WR-463), so each half of a closed rim keeps its own
//! arc. Every result is a manifold solid of the spherical shell's volume
//! 2/3 π (`r_out³` − `r_in³`), to the accuracy the volume integration is asked for (1e-6;
//! the results come within 1.4e-6 relative).
//!
//! The last cases (tangency-labs/solidcraft#15282) thicken a sheet of two loft faces that are
//! tangent at the ends of their shared edge and creased between them; see the section's
//! comment.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use offset_fixture::{volume, Notifications};
use solidcore::blend::fillet_executive::api::create_face_from_surface;
use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol;
use solidcore::math::Vec3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::{Brep, FaceId, Kernel};

/// Relative tolerance on the volume: the integration is asked for 1e-6, and the shelled
/// caps come within 1.4e-6.
const REL: f64 = 1e-5;

fn kernel() -> Kernel {
    Kernel::default()
}

fn shell_options(distance: f64, create_offset_solid: bool) -> ShellOptions {
    ShellOptions {
        offset: OffsetOptions {
            distance,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid,
        merge_sheet_side_walls: false,
    }
}

/// The ball r 10 at the origin cut by the box at `origin` of `size` (a half space), shelled
/// open at its flat face by `distance`.
fn shelled_cap(origin: Vec3, size: Vec3, distance: f64, solid: bool) -> (Kernel, Brep) {
    let mut k = kernel();
    let ball = create_sphere(&mut k, Vec3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let half = create_box(&mut k, origin, size.x, size.y, size.z).unwrap();
    let mut cap = boolean_intersection(&mut k, ball, half).unwrap();
    let flat: Vec<FaceId> = cap
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            cap.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| s.is_kind_of(Plane::TYPE).unwrap())
        })
        .collect();
    assert_eq!(flat.len(), 1);
    let mut notifications = Notifications::new(&mut cap);
    let out = OffsetExecutive::shell_brep(
        &mut k,
        cap,
        shell_options(distance, solid),
        &flat,
        &mut notifications.context(),
    )
    .result
    .unwrap();
    (k, out)
}

/// The cut at the equator (z >= 0).
fn equator(distance: f64, solid: bool) -> (Kernel, Brep) {
    shelled_cap(
        Vec3::new(-20.0, -20.0, 0.0),
        Vec3::new(40.0, 40.0, 20.0),
        distance,
        solid,
    )
}

/// The cut through the poles (x >= 0).
fn poles(distance: f64, solid: bool) -> (Kernel, Brep) {
    shelled_cap(
        Vec3::new(0.0, -20.0, -20.0),
        Vec3::new(20.0, 40.0, 40.0),
        distance,
        solid,
    )
}

/// The upper hemisphere r 10 as two quarter-sphere sheets (longitudes 0-180 and
/// 180-360) stitched along their meridians, thickened by +1: its rim is two half circles.
fn stitched_quarters_thickened() -> (Kernel, Brep) {
    let mut k = kernel();
    let mut sheet =
        solidcore::topology::brep::p3::new_brep(&mut k, tol::USE_DEFAULT, tol::USE_DEFAULT)
            .unwrap();
    for (lon0, lon1) in [(0.0, 180.0), (180.0, 360.0)] {
        let mut quarter = Sphere::new(
            &k,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent2d::from_bounds(lon0, 0.0, lon1, 90.0),
            10.0,
            SphereOptions::default(),
        )
        .unwrap();
        quarter.make_nurb(&k).unwrap();
        create_face_from_surface(&mut k, &mut sheet, &Surface::from(quarter)).unwrap();
    }
    stitch_faces(&mut k, &mut sheet, 1.0e-5, None, false).unwrap();
    assert_eq!(sheet.face_list().len(), 2);
    assert!(!brep_is_manifold_solid(&sheet).unwrap());
    let mut notifications = Notifications::new(&mut sheet);
    let out = OffsetExecutive::shell_brep(
        &mut k,
        sheet,
        shell_options(1.0, true),
        &[],
        &mut notifications.context(),
    )
    .result
    .unwrap();
    (k, out)
}

/// The result is a manifold solid of `faces` faces whose volume is the hemispherical
/// shell between radii 10 and `10 + distance`.
fn assert_sound((k, mut b): (Kernel, Brep), distance: f64, faces: usize) {
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), faces);
    let (outer, inner) = if distance > 0.0 {
        (10.0 + distance, 10.0)
    } else {
        (10.0, 10.0 + distance)
    };
    let expected = 2.0 / 3.0 * PI * (outer * outer * outer - inner * inner * inner);
    let v = volume(&k, &mut b).unwrap();
    assert!(
        (v - expected).abs() <= REL * expected,
        "volume {v}, expected {expected}"
    );
}

#[test]
fn equator_cap_shelled_out_as_offset_solid() {
    assert_sound(equator(1.0, true), 1.0, 3);
}

#[test]
fn equator_cap_shelled_in_as_offset_solid() {
    assert_sound(equator(-1.0, true), -1.0, 3);
}

#[test]
fn equator_cap_shelled_out() {
    assert_sound(equator(1.0, false), 1.0, 3);
}

#[test]
fn equator_cap_shelled_in() {
    assert_sound(equator(-1.0, false), -1.0, 3);
}

#[test]
fn poles_cap_shelled_out_as_offset_solid() {
    assert_sound(poles(1.0, true), 1.0, 5);
}

#[test]
fn poles_cap_shelled_in_as_offset_solid() {
    assert_sound(poles(-1.0, true), -1.0, 5);
}

#[test]
fn poles_cap_shelled_out() {
    assert_sound(poles(1.0, false), 1.0, 5);
}

#[test]
fn poles_cap_shelled_in() {
    assert_sound(poles(-1.0, false), -1.0, 5);
}

#[test]
fn stitched_hemisphere_quarters_thickened_out() {
    assert_sound(stitched_quarters_thickened(), 1.0, 6);
}

// ---- #15282: two loft faces tangent at the ends of their shared edge ------------------------
//
// The HoodCorner sheet of tangency-labs/solidcraft#15282 (its STEP's two rational patches,
// degree 1 by 2): face A is ruled between a conic at y = 30 and a line at y = 60, face B
// between a conic at y = 30 and a quarter circle at y = 60, and they share the ruling from
// (31.68, 30, 23.59) to (94, 60, 70). Their normals agree at both ends of that edge and
// differ by up to 0.91 degrees between them: a crease of varying angle, concave on the
// normal side. Each face's own offset edge then ends where the other's does, but leaves it
// in between (by up to 0.032 at a thickness of 2), and the mated edge is the intersection of
// the two offset surfaces. The shell kept each face's own edge because the intersection's
// ends matched it, and SolidCraft's thicken by 2 or -2 refused the result as not closed.
//
// The volume is graded against a reference built without the kernel: each face's offset
// solid is the union of its normal segments, of volume
// `V_f = ∫∫ ∫_0^d ((S_u + t n_u) × (S_v + t n_v)) · n dt du dv` (Steiner's formula, by
// Gauss-Legendre quadrature of the exact rational patches), and at the shared edge the two
// segments' unions overlap (concave side, d > 0) or leave the wedge the extended faces close
// (convex side, d < 0), a kite of area `d² tan(θ/2)` in each plane normal to the straight
// edge, `θ` the angle between the two normals there. So the solid lies in the two-sided
// enclosure `[S − 2C, S]` for d > 0 and `[S, S + 2C]` for d < 0 (`S = V_A + V_B`,
// `C = ∫ d² tan(θ/2) ds`; the overlap and the wedge are not negative and stay below twice
// the flat kite for these curvatures), and its point estimate is `S ∓ C`. The point
// estimate agrees with an independent divergence-theorem integration over the exact
// boundary (offset surfaces cut at their Newton-solved intersection, ruled side walls) to
// 3e-10 relative at both thicknesses.

type P3 = [f64; 3];

/// The `HoodCorner` patch: rows (u = 0, 1) of three control points (v quadratic), their
/// weights and the v range (`[0, v_max]`).
struct Patch {
    rows: [[P3; 3]; 2],
    weights: [[f64; 3]; 2],
    v_max: f64,
}

const P20: P3 = [31.680702858445635, 30.0, 23.592012766927596];
const P23: P3 = [94.0, 60.0, 70.0];

fn hood_a() -> Patch {
    Patch {
        rows: [
            [
                P20,
                [19.834329387008736, 30.0, 39.5],
                [2.4186774283160227E-15, 30.0, 39.5],
            ],
            [P23, [47.0, 60.0, 70.0], [0.0, 60.0, 70.0]],
        ],
        weights: [[1.0, 0.8936627288855027, 1.0], [1.0, 1.0, 1.0]],
        v_max: 0.5,
    }
}

fn hood_b() -> Patch {
    Patch {
        rows: [
            [
                [33.260610741372545, 30.0, 21.306848033223808],
                [32.510862160493126, 30.0, 22.47722741846382],
                P20,
            ],
            [
                [100.0, 59.151471862576145, 64.06030303803301],
                [100.0, 60.0, 70.0],
                P23,
            ],
        ],
        weights: [[1.0, 1.0, 1.0], [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]],
        v_max: 1.0,
    }
}

fn v3(p: P3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

impl Patch {
    /// The patch as a solidcore B-spline surface (weighted control points, as in the STEP).
    fn surface(&self) -> Surface {
        use solidcore::nurbs::b_spline_surface::p1::BSplineSurface;
        use solidcore::nurbs::math::CPoint;
        let pw: Vec<Vec<CPoint>> = (0..2)
            .map(|i| {
                (0..3)
                    .map(|j| {
                        let [x, y, z] = self.rows[i][j];
                        let w = self.weights[i][j];
                        CPoint::new(x * w, y * w, z * w, w)
                    })
                    .collect()
            })
            .collect();
        let vm = self.v_max;
        Surface::from(
            BSplineSurface::from_data(
                1,
                2,
                1,
                2,
                3,
                5,
                &pw,
                &[0., 0., 1., 1.],
                &[0., 0., 0., vm, vm, vm],
            )
            .unwrap(),
        )
    }

    /// The point and its first and second derivatives (S, Su, Sv, Suu, Suv, Svv), by the
    /// quotient rule on the homogeneous patch.
    fn derivs(&self, u: f64, v: f64) -> [Vec3; 6] {
        let t = v / self.v_max;
        let s = 1.0 / self.v_max;
        let bu = [[1.0 - u, u], [-1.0, 1.0], [0.0, 0.0]];
        let bv = [
            [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t],
            [-2.0 * (1.0 - t) * s, (2.0 - 4.0 * t) * s, 2.0 * t * s],
            [2.0 * s * s, -4.0 * s * s, 2.0 * s * s],
        ];
        let h = |a: usize, b: usize| -> (Vec3, f64) {
            let mut p = Vec3::new(0.0, 0.0, 0.0);
            let mut w = 0.0;
            for ((bui, row), wrow) in bu[a].iter().zip(&self.rows).zip(&self.weights) {
                for ((bvj, pt), wt) in bv[b].iter().zip(row).zip(wrow) {
                    let c = bui * bvj * wt;
                    p += v3(*pt) * c;
                    w += c;
                }
            }
            (p, w)
        };
        let (p, w) = h(0, 0);
        let (pu, wu) = h(1, 0);
        let (pv, wv) = h(0, 1);
        let (puu, wuu) = h(2, 0);
        let (puv, wuv) = h(1, 1);
        let (pvv, wvv) = h(0, 2);
        let s0 = p * (1.0 / w);
        let su = (pu - s0 * wu) * (1.0 / w);
        let sv = (pv - s0 * wv) * (1.0 / w);
        let suu = (puu - su * (2.0 * wu) - s0 * wuu) * (1.0 / w);
        let svv = (pvv - sv * (2.0 * wv) - s0 * wvv) * (1.0 / w);
        let suv = (puv - su * wv - sv * wu - s0 * wuv) * (1.0 / w);
        [s0, su, sv, suu, suv, svv]
    }

    /// The unit normal (`S_u × S_v`) and its derivatives.
    fn normal(&self, u: f64, v: f64) -> (Vec3, Vec3, Vec3, Vec3, Vec3) {
        let [_, su, sv, suu, suv, svv] = self.derivs(u, v);
        let big_n = su.cross(sv);
        let l = big_n.length();
        let n = big_n * (1.0 / l);
        let nu_raw = suu.cross(sv) + su.cross(suv);
        let nv_raw = suv.cross(sv) + su.cross(svv);
        let nu = (nu_raw - n * n.dot(nu_raw)) * (1.0 / l);
        let nv = (nv_raw - n * n.dot(nv_raw)) * (1.0 / l);
        (n, nu, nv, su, sv)
    }

    /// Steiner's volume of the union of the normal segments of length `|d|` (side `d`).
    fn steiner(&self, d: f64, order: usize) -> f64 {
        let (xs, ws) = gauss_legendre(order);
        let mut total = 0.0;
        for (xu, wu) in xs.iter().zip(&ws) {
            let u = (xu + 1.0) / 2.0;
            for (xv, wv) in xs.iter().zip(&ws) {
                let v = (xv + 1.0) / 2.0 * self.v_max;
                let (n, nu, nv, su, sv) = self.normal(u, v);
                let a0 = su.cross(sv).dot(n);
                let a1 = (su.cross(nv) + nu.cross(sv)).dot(n);
                let a2 = nu.cross(nv).dot(n);
                let vol = a0 * d + a1 * d * d / 2.0 + a2 * d * d * d / 3.0;
                total += vol.abs() * wu * wv * 0.25 * self.v_max;
            }
        }
        total
    }
}

/// Gauss-Legendre nodes and weights on [-1, 1].
fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut xs = Vec::new();
    let mut ws = Vec::new();
    let nf = n as f64;
    for i in 1..=n {
        let mut x = (PI * (i as f64 - 0.25) / (nf + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for k in 2..=n {
                let kf = k as f64;
                let p2 = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
                p0 = p1;
                p1 = p2;
            }
            dp = nf * (x * p1 - p0) / (x * x - 1.0);
            let dx = p1 / dp;
            x -= dx;
            if dx.abs() < 1e-16 {
                break;
            }
        }
        xs.push(x);
        ws.push(2.0 / ((1.0 - x * x) * dp * dp));
    }
    (xs, ws)
}

/// `∫ d² tan(θ/2) ds` along the shared straight edge (A's v = 0, B's v = `v_max`, both
/// running with u from P20 to P23).
fn crease_kites(a: &Patch, b: &Patch, d: f64, order: usize) -> f64 {
    let (xs, ws) = gauss_legendre(order);
    let len = (v3(P23) - v3(P20)).length();
    let mut total = 0.0;
    for (x, w) in xs.iter().zip(&ws) {
        let u = (x + 1.0) / 2.0;
        let (na, ..) = a.normal(u, 0.0);
        let (nb, ..) = b.normal(u, b.v_max);
        let theta = na.cross(nb).length().atan2(na.dot(nb));
        total += d * d * (theta / 2.0).tan() * w * 0.5 * len;
    }
    total
}

/// The `HoodCorner` sheet: the two patches, stitched along their shared edge.
fn hood_corner(k: &mut Kernel) -> Brep {
    let mut sheet =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    for patch in [hood_a(), hood_b()] {
        create_face_from_surface(k, &mut sheet, &patch.surface()).unwrap();
    }
    stitch_faces(k, &mut sheet, 1.0e-5, None, false).unwrap();
    assert_eq!(sheet.face_list().len(), 2);
    assert_eq!(sheet.edge_list().len(), 7);
    sheet.refresh_cached_topology();
    sheet
}

/// SolidCraft's thicken (`smlib_thicken`, not symmetric): `SmApiShellBrepFull` with an
/// offset solid, extended convex edges, merged results and merged sheet side walls.
fn thicken_hood_corner(d: f64) -> (Kernel, Brep) {
    let mut k = kernel();
    let mut sheet = hood_corner(&mut k);
    let mut notifications = Notifications::new(&mut sheet);
    let options = ShellOptions {
        merge_sheet_side_walls: true,
        ..shell_options(d, true)
    };
    let mut out =
        OffsetExecutive::shell_brep(&mut k, sheet, options, &[], &mut notifications.context())
            .result
            .unwrap_or_else(|e| panic!("d {d}: {e}"));
    out.refresh_cached_topology();
    (k, out)
}

/// The thickened sheet's volume lies in the enclosure and matches the point estimate to
/// 1e-6 relative, the volume integrated to 1e-10.
fn assert_hood_corner_volume(k: &Kernel, b: &mut Brep, d: f64) {
    let (a, bb) = (hood_a(), hood_b());
    let s = a.steiner(d, 40) + bb.steiner(d, 40);
    let c = crease_kites(&a, &bb, d, 40);
    let (lo, hi, estimate) = if d > 0.0 {
        (s - 2.0 * c, s, s - c)
    } else {
        (s, s + 2.0 * c, s + c)
    };
    // integrated to 1e-10, far inside the 1e-6 the estimate is checked to
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    let v = solidcore::mass::integrator::properties::brep_volume(k, b, 1e-10, &attrs).unwrap();
    assert!(lo <= v && v <= hi, "d {d}: volume {v} outside [{lo}, {hi}]");
    assert!(
        (v - estimate).abs() <= 1e-6 * estimate,
        "d {d}: volume {v}, estimate {estimate} (rel {:.2e})",
        (v - estimate).abs() / estimate
    );
}

/// The thickened sheet is a manifold solid of ten faces (the two faces, their offsets and
/// the six side walls), Level 2 check-clean, no edge tolerance past twice the drops' 2e-5,
/// whose volume lies in the enclosure.
fn assert_checked_hood_corner(d: f64) {
    let (mut k, mut b) = thicken_hood_corner(d);
    assert!(brep_is_manifold_solid(&b).unwrap(), "d {d}");
    assert_eq!(b.face_list().len(), 10, "d {d}");
    let lines = solidcore::topology::brep::validation::check_brep(
        &mut k,
        &mut b,
        solidcore::topology::brep::validation::CheckLevel::Level2,
    )
    .unwrap()
    .lines();
    assert!(lines.is_empty(), "d {d}: {lines:?}");
    // the trims flattened onto the boundary raised it to 1.03e-3; a drop's reported
    // deviation (its measured one plus the bound of its boundary moves) can raise it to
    // twice that deviation, within the drop's 2e-5 tolerance
    let tol = max_edge_tolerance(&b);
    assert!(tol <= 4e-5, "d {d}: edge tolerance {tol}");
    assert_hood_corner_volume(&k, &mut b, d);
}

/// The largest edge tolerance of the body.
fn max_edge_tolerance(b: &Brep) -> f64 {
    b.edge_list()
        .iter()
        .map(|&e| b.edge(e).unwrap().tolerance)
        .fold(0.0, f64::max)
}

/// Outwards the offset faces overlap at the crease; their mated edge runs into the
/// domain corner of each face's shrunk surface at a shallow angle, so its UV trims
/// must not be flattened onto the boundary (`intersect::surface_drop_lib::boundary_moves`).
#[test]
fn hood_corner_thickened_out_is_a_checked_solid_of_its_volume() {
    assert_checked_hood_corner(2.0);
}

#[test]
fn hood_corner_thickened_in_is_a_checked_solid_of_its_volume() {
    assert_checked_hood_corner(-2.0);
}

/// Thickened in by 0.1, the sheet is a manifold solid of ten faces, check-clean at both
/// geometry levels, whose volume is within its area times its largest edge tolerance of
/// the estimate (Steiner's volume plus the crease kites): a tolerance-scale difference.
#[test]
fn hood_corner_thickened_in_a_little_is_a_checked_solid_within_tolerance_of_its_volume() {
    let d = -0.1;
    let (mut k, mut b) = thicken_hood_corner(d);
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 10);
    for level in [
        solidcore::topology::brep::validation::CheckLevel::Level1,
        solidcore::topology::brep::validation::CheckLevel::Level2,
    ] {
        let lines = solidcore::topology::brep::validation::check_brep(&mut k, &mut b, level)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{level:?}: {lines:?}");
    }
    let (a, bb) = (hood_a(), hood_b());
    let estimate = a.steiner(d, 40) + bb.steiner(d, 40) + crease_kites(&a, &bb, d, 40);
    let area: f64 = b
        .face_list()
        .to_vec()
        .into_iter()
        .map(|f| solidcore::mass::integrator::properties::face_area(&k, &mut b, f, 1e-9).unwrap())
        .sum();
    let tol = max_edge_tolerance(&b);
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    let v =
        solidcore::mass::integrator::properties::brep_volume(&k, &mut b, 1e-10, &attrs).unwrap();
    assert!(
        (v - estimate).abs() <= area * tol,
        "volume {v}, estimate {estimate}, area {area} x tolerance {tol}"
    );
}
