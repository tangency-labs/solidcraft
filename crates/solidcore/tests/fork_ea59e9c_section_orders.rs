//! The SolidCraft fork's tangency-labs/smlib@ea59e9c (the first commit of smlib#78, #2250):
//! a blend's cross-section of a given order of contact with its faces: 0 the chord, 1 the
//! ball's circle, 2 the G^n section, between the rolling ball's contact lines.
//!
//! Each case that runs here is graded as the fork's `section_blend` grades it: the status,
//! the volume, SMLib's validity check at level 2, every point of a 17 x 17 grid of each blend
//! face's surface on the exact blend, and at nine stations along each seam with a rail face
//! the order of contact, from the two surfaces' graphs in the plane square to the seam
//! (degree-7 fits through seven points each side): the order at 1e-9, 1e-7 / mm, 1e-5 / mm^2
//! and 1e-3 / mm^3, and not the next at Continuity Analysis's tolerances.
//!
//! The six order-2 cases run on the port of the fork's 11ea2d9 (#2260,
//! `fillet_gn_section_generator`), under the same grading: the surface against the section
//! worked out apart from the kernel in `gn_section`, the volume, the check at level 2 and the
//! order of contact along each seam, which the G^n tests in `wr685_gn_section.rs` leave out.
//!
//! How this test drives the kernel differs from the capi: edges are picked as the nearest
//! within 0.1 of a point; the chord is `FilletSession` with the linear section and bevelled
//! corners, as `chamfer_offsets`; the round is `fillet_edges`; the variable round is
//! `variable_radius_fillet` with the law's ends in the edge's direction, which is the capi's
//! degree-1 law from the edge's end at x = 0.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::needless_range_loop
)] // test helpers

use solidcore::blend::fillet_executive::api::{fillet_edges, variable_radius_fillet};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_section::section::{GnSectionFsg, GN_THUMBWEIGHT_EQUAL_AREA};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{EdgeId, FaceId};
use solidcore::{Brep, Kernel, Result};

type P2 = [f64; 2];

/// The G^n section in a section's plane: `c1[k]`, `c2[k]` (k = 0..n) the faces' jets at the
/// contacts, `T_i(u) = sum_k c_ik (u - 1)^k`.
fn gn_section(n: usize, w: f64, c1: &[P2], c2: &[P2], t: f64) -> P2 {
    let (a, b) = ((1.0 - t).powi(n as i32 + 1), t.powi(n as i32 + 1));
    let f = a / (a + b);
    let u1 = -t / (1.0 - w + w * t);
    let u2 = (t - 1.0) / (1.0 - w * t);
    let (mut s1, mut s2) = ([0.0, 0.0], [0.0, 0.0]);
    let (mut k1, mut k2) = (1.0, 1.0);
    for k in 0..=n {
        for i in 0..2 {
            s1[i] += c1[k][i] * k1;
            s2[i] += c2[k][i] * k2;
        }
        k1 *= u1;
        k2 *= u2;
    }
    [f * s1[0] + (1.0 - f) * s2[0], f * s1[1] + (1.0 - f) * s2[1]]
}

/// The distance from `q` to the curve `c(t)`, t in [0, 1]: the nearest of 512 samples, then
/// a golden section search between its neighbours.
fn distance_to_curve(c: &dyn Fn(f64) -> P2, q: P2) -> f64 {
    let d2 = |t: f64| {
        let p = c(t);
        (p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1])
    };
    let m = 512;
    let mut best = 0;
    let mut bd = d2(0.0);
    for k in 1..=m {
        let d = d2(f64::from(k) / f64::from(m));
        if d < bd {
            bd = d;
            best = k;
        }
    }
    let mut lo = (f64::from(best) - 1.0).max(0.0) / f64::from(m);
    let mut hi = (f64::from(best) + 1.0).min(f64::from(m)) / f64::from(m);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..100 {
        let (x1, x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if d2(x1) < d2(x2) {
            hi = x2;
        } else {
            lo = x1;
        }
    }
    bd.min(d2(0.5 * (lo + hi))).sqrt()
}

/// A surface crossed by the plane through `p` square to `b`: the curve's height along `n`
/// over the line `p + sigma w`, at `sigma = j step` (j = 1..7), walked from `uv` by Newton.
/// Its first four derivatives at 0, from the polynomial of degree 7 through h(0) = 0 and
/// those seven.
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
fn seam_graph(
    k: &Kernel,
    s: &Surface,
    mut uv: Point2,
    p: Point3,
    b: Vec3,
    w: Vec3,
    n: Vec3,
    step: f64,
) -> Option<[f64; 4]> {
    let mut m = [[0.0f64; 8]; 7];
    let along = |q: Point3, d: Vec3| (q.x - p.x) * d.x + (q.y - p.y) * d.y + (q.z - p.z) * d.z;
    for j in 1..=7 {
        let sigma = f64::from(j) * step;
        let mut converged = false;
        for _ in 0..40 {
            if converged {
                break;
            }
            let (q, du, dv) = s.evaluate_1st_derivatives(k, uv, true, true).ok()?;
            let (f0, f1) = (along(q, b), along(q, w) - sigma);
            let (a00, a01, a10, a11) = (du.dot(b), dv.dot(b), du.dot(w), dv.dot(w));
            let det = a00 * a11 - a01 * a10;
            if det == 0.0 {
                return None;
            }
            let (ddu, ddv) = ((f0 * a11 - a01 * f1) / det, (a00 * f1 - a10 * f0) / det);
            uv = Point2::new(uv.x - ddu, uv.y - ddv);
            converged = ddu.abs() + ddv.abs() <= 1e-15 * (1.0 + uv.x.abs() + uv.y.abs());
        }
        let q = s.evaluate_point(k, uv).ok()?;
        let mut x = 1.0;
        let row = &mut m[usize::try_from(j - 1).unwrap()];
        for cell in row.iter_mut().take(7) {
            x *= sigma;
            *cell = x;
        }
        row[7] = along(q, n);
    }
    for c in 0..7 {
        let mut piv = c;
        for r in c + 1..7 {
            if m[r][c].abs() > m[piv][c].abs() {
                piv = r;
            }
        }
        m.swap(c, piv);
        for r in c + 1..7 {
            let f = m[r][c] / m[c][c];
            for i in c..8 {
                let v = m[c][i];
                m[r][i] -= f * v;
            }
        }
    }
    let mut a = [0.0; 7];
    for c in (0..7).rev() {
        let mut v = m[c][7];
        for i in c + 1..7 {
            v -= m[c][i] * a[i];
        }
        a[c] = v / m[c][c];
    }
    Some([a[0], 2.0 * a[1], 6.0 * a[2], 24.0 * a[3]])
}

/// The order a seam reaches: 1 where the faces' graphs have slopes within `tol[0]`, 2 and
/// their second derivatives within `tol[1]`, and so on.
fn seam_order(diff: &[f64; 4], tol: &[f64; 4]) -> usize {
    let mut k = 0;
    while k < 4 && diff[k] <= tol[k] {
        k += 1;
    }
    k
}

/// The faces of the body before the blend, as exact surfaces.
#[derive(Clone, Copy)]
enum Quadric {
    /// A point and the unit normal.
    Plane([f64; 3], [f64; 3]),
    /// An axis point, the unit axis and the radius.
    Cylinder([f64; 3], [f64; 3], f64),
}

impl Quadric {
    fn distance(self, c: Point3) -> f64 {
        let parts = |p: [f64; 3], d: [f64; 3]| {
            let w = [c.x - p[0], c.y - p[1], c.z - p[2]];
            let along = w[0] * d[0] + w[1] * d[1] + w[2] * d[2];
            let squared = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
            (along, (squared - along * along).max(0.0).sqrt())
        };
        match self {
            Self::Plane(p, n) => parts(p, n).0.abs(),
            Self::Cylinder(p, d, r) => (parts(p, d).1 - r).abs(),
        }
    }
}

const X_AXIS: [f64; 3] = [1.0, 0.0, 0.0];
const Y_AXIS: [f64; 3] = [0.0, 1.0, 0.0];
const Z_AXIS: [f64; 3] = [0.0, 0.0, 1.0];

/// How a case's blend is made.
#[derive(Clone, Copy)]
enum Blend {
    /// The chord of the given setback.
    Chord(f64),
    /// The ball's circle of the given radius.
    Round(f64),
    /// The radius from the first figure at x = 0 to the second at x = 20.
    LinearRound(f64, f64),
    /// The cubic Greville law `r` along y in [0, 20] on `box_10_20_30`.
    SplineRound([f64; 5]),
    /// The G^n section: order, thumbweight, radius.
    Gn(u32, f64, f64),
}

/// A case: its body, edge, blend and what the blend must be.
struct Case {
    name: &'static str,
    build: fn(&mut Kernel) -> Result<Brep>,
    pick: [f64; 3],
    blend: Blend,
    volume: f64,
    volume_rel: f64,
    off: Box<dyn Fn(Point3) -> f64>,
    /// `None`: the fillet's tolerance, 100 times the body's.
    off_tol: Option<f64>,
    faces: Vec<Quadric>,
    rails: [usize; 2],
    order: usize,
    /// The highest order the seams may reach, where it is above `order`: a fitted surface's
    /// seam may pass the next order's tolerance without its section having that order.
    order_max: Option<usize>,
    fitted: bool,
    size: f64,
}

fn box_40_30_20(k: &mut Kernel) -> Result<Brep> {
    create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)
}

fn box_20_30_20(k: &mut Kernel) -> Result<Brep> {
    create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 30.0, 20.0)
}

fn box_10_20_30(k: &mut Kernel) -> Result<Brep> {
    create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0)
}

fn boss_on_plate(k: &mut Kernel) -> Result<Brep> {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0)?;
    let boss = create_cylinder(k, Point3::new(30.0, 20.0, 10.0), 8.0, 10.0)?;
    boolean_union(k, plate, boss)
}

fn pipe_on_plate(k: &mut Kernel) -> Result<Brep> {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0)?;
    let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0)?;
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(k, &mut pipe, &pl, None, false)?;
    boolean_union(k, plate, pipe)
}

fn box_planes(l: f64, w: f64, h: f64) -> Vec<Quadric> {
    vec![
        Quadric::Plane([0.0, 0.0, 0.0], X_AXIS),
        Quadric::Plane([l, 0.0, 0.0], X_AXIS),
        Quadric::Plane([0.0, 0.0, 0.0], Y_AXIS),
        Quadric::Plane([0.0, w, 0.0], Y_AXIS),
        Quadric::Plane([0.0, 0.0, 0.0], Z_AXIS),
        Quadric::Plane([0.0, 0.0, h], Z_AXIS),
    ]
}

const BOX_TOP_JETS: [P2; 3] = [[27.0, 20.0], [-3.0, 0.0], [0.0, 0.0]];
const BOX_BACK_JETS: [P2; 3] = [[30.0, 17.0], [0.0, -3.0], [0.0, 0.0]];
const PLATE_JETS: [P2; 3] = [[10.0, 10.0], [2.0, 0.0], [0.0, 0.0]];
const WALL_JETS: [P2; 3] = [[8.0, 12.0], [0.0, 2.0], [0.0, 0.0]];

/// The pipe's and the plate's jets at the contact of the ball of radius `d`.
fn pipe_jets(d: f64) -> ([P2; 3], [P2; 3]) {
    let g = (64.0 + 32.0 * d).sqrt();
    let (g1, g2, e) = (16.0 / g, -256.0 / (g * g * g), 10.0 + d);
    let plate = [[20.0 + g, 10.0], [d * g1, 0.0], [d * d / 2.0 * g2, 0.0]];
    let pipe = [
        [20.0 + 10.0 * g / e, 26.0 - 160.0 / e],
        [d * 10.0 * (g1 * e - g) / (e * e), d * 160.0 / (e * e)],
        [
            d * d / 2.0 * 10.0 * (g2 / e - 2.0 * g1 / (e * e) + 2.0 * g / (e * e * e)),
            d * d / 2.0 * (-320.0 / (e * e * e)),
        ],
    ];
    (plate, pipe)
}

fn boss_faces() -> Vec<Quadric> {
    let mut f = box_planes(60.0, 40.0, 10.0);
    f.push(Quadric::Cylinder([30.0, 20.0, 0.0], Z_AXIS, 8.0));
    f.push(Quadric::Plane([0.0, 0.0, 20.0], Z_AXIS));
    f
}

fn pipe_faces() -> Vec<Quadric> {
    let mut f = box_planes(60.0, 40.0, 10.0);
    f.push(Quadric::Cylinder([0.0, 20.0, 16.0], X_AXIS, 10.0));
    f
}

fn rho_z(x: Point3) -> P2 {
    [(x.x - 30.0).hypot(x.y - 20.0), x.z]
}

/// The fork's cases that have a kernel to run on today, with the fork's figures.
fn running_cases() -> Vec<Case> {
    let boss_pick = [30.0 + 4.0 * 2f64.sqrt(), 20.0 + 4.0 * 2f64.sqrt(), 10.0];
    let (pipe_plate_1, pipe_pipe_1) = pipe_jets(1.0);
    let (pipe_g, pipe_d) = ((64.0f64 + 64.0).sqrt(), 2.0);
    let box_case = |name, blend, cut: f64, off: Box<dyn Fn(Point3) -> f64>, order| Case {
        name,
        build: box_40_30_20,
        pick: [20.0, 30.0, 20.0],
        blend,
        volume: 24000.0 - 40.0 * 9.0 * cut,
        volume_rel: 1e-7,
        off,
        off_tol: Some(1e-9),
        faces: box_planes(40.0, 30.0, 20.0),
        rails: [5, 3],
        order,
        order_max: None,
        fitted: false,
        size: 3.0,
    };
    let boss_case = |name, blend, volume, off: Box<dyn Fn(Point3) -> f64>, order| Case {
        name,
        build: boss_on_plate,
        pick: boss_pick,
        blend,
        volume,
        volume_rel: 1e-7,
        off,
        off_tol: Some(1e-9),
        faces: boss_faces(),
        rails: [5, 6],
        order,
        order_max: None,
        fitted: false,
        size: 2.0,
    };
    let pipe_case = |name, blend, volume, off: Box<dyn Fn(Point3) -> f64>, order| Case {
        name,
        build: pipe_on_plate,
        pick: [30.0, 28.0, 10.0],
        blend,
        volume,
        volume_rel: 1e-7,
        off,
        off_tol: Some(1e-9),
        faces: pipe_faces(),
        rails: [5, 6],
        order,
        order_max: None,
        fitted: false,
        size: if order == 0 { 1.0 } else { 2.0 },
    };
    let (apex, l) = ([-5.0, 30.0, 20.0], 1.08f64.sqrt());
    let ax = [1.0 / l, -0.2 / l, -0.2 / l];
    let (sphi, cphi) = (1.0 / 27f64.sqrt(), (26.0f64 / 27.0).sqrt());
    vec![
        box_case(
            "box_edge_chamfer",
            Blend::Chord(3.0),
            0.5,
            Box::new(|x| {
                distance_to_curve(
                    &|t| gn_section(0, 0.0, &BOX_TOP_JETS, &BOX_BACK_JETS, t),
                    [x.y, x.z],
                )
            }),
            0,
        ),
        box_case(
            "box_edge_round",
            Blend::Round(3.0),
            1.0 - PI / 4.0,
            Box::new(|x| ((x.y - 27.0).hypot(x.z - 17.0) - 3.0).abs()),
            1,
        ),
        boss_case(
            "boss_root_chamfer",
            Blend::Chord(2.0),
            26119.5278436219,
            Box::new(|x| {
                distance_to_curve(
                    &|t| gn_section(0, 0.0, &PLATE_JETS, &WALL_JETS, t),
                    rho_z(x),
                )
            }),
            0,
        ),
        boss_case(
            "boss_root_round",
            Blend::Round(2.0),
            26056.1770421533,
            Box::new(|x| {
                let q = rho_z(x);
                ((q[0] - 10.0).hypot(q[1] - 12.0) - 2.0).abs()
            }),
            1,
        ),
        pipe_case(
            "pipe_on_plate_chamfer",
            Blend::Chord(2.0),
            40241.7164766411,
            Box::new(move |x| {
                distance_to_curve(
                    &|t| gn_section(0, 0.0, &pipe_plate_1, &pipe_pipe_1, t),
                    [x.y, x.z],
                )
            }),
            0,
        ),
        pipe_case(
            "pipe_on_plate_round",
            Blend::Round(2.0),
            40300.8060669181,
            Box::new(move |x| ((x.y - 20.0 - pipe_g).hypot(x.z - 10.0 - pipe_d) - pipe_d).abs()),
            1,
        ),
        Case {
            name: "variable_round_on_its_cone",
            build: box_20_30_20,
            pick: [10.0, 30.0, 20.0],
            blend: Blend::LinearRound(1.0, 5.0),
            volume: 11956.2224267359,
            volume_rel: 1e-5,
            off: Box::new(move |x| {
                let w = [x.x - apex[0], x.y - apex[1], x.z - apex[2]];
                let along = w[0] * ax[0] + w[1] * ax[1] + w[2] * ax[2];
                let squared = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
                let across = (squared - along * along).max(0.0).sqrt();
                (across * cphi - along * sphi).abs()
            }),
            off_tol: None,
            faces: box_planes(20.0, 30.0, 20.0),
            rails: [5, 3],
            order: 1,
            order_max: None,
            fitted: true,
            size: 1.0,
        },
    ]
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

/// As the capi's `chamfer_radius`: the offset at which the chord sits `distance` back.
fn chamfer_radius(k: &Kernel, b: &mut Brep, e: EdgeId, distance: f64) -> f64 {
    let u0 = edge::blend_edgeuse(k, b, e).unwrap().unwrap();
    let u1 = eu1::radial(b, u0).unwrap().unwrap();
    let t = b.edge(e).unwrap().interval.evaluate(0.5);
    let o = BinormalOptions::default();
    let e0 = evaluate_binormal(k, b, u0, t, false, &o).unwrap();
    let e1 = evaluate_binormal(k, b, u1, t, false, &o).unwrap();
    let c = e0
        .binormal
        .unitize()
        .unwrap()
        .dot(e1.binormal.unitize().unwrap())
        .clamp(-1.0, 1.0);
    distance * ((1.0 - c) / (1.0 + c)).sqrt()
}

fn chord(k: &mut Kernel, b: &mut Brep, e: EdgeId, distance: f64) -> Result<()> {
    let r = chamfer_radius(k, b, e, distance);
    {
        let mut s = FilletSession::new(k, b)?;
        s.prepare_per_edge_constant_radius_with_second(
            k,
            &[e],
            &[r],
            &[r],
            &[FilletSurfaceGeneratorType::Linear],
            1,
            1.0,
        )?;
        s.create_corners(k, None)?;
        for id in s.fillet_corners()? {
            s.set_corner_bevel(id, true)?;
        }
        s.do_filleting(k, None)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(())
}

/// The radii of a linear round in the edge's direction.
fn linear_round(k: &mut Kernel, b: &mut Brep, e: EdgeId, at0: f64, at20: f64) -> Result<()> {
    let ivl = edge::interval(b, e)?;
    let c = edge::curve(b, e)?.unwrap();
    let start = c.evaluate(k, ivl.min(), 0, true, false)?[0];
    let (r0, r1) = if start.x < 10.0 {
        (at0, at20)
    } else {
        (at20, at0)
    };
    variable_radius_fillet(k, b, &[e], r0, r1, 1, 1)
}

/// Cubic Greville B-spline law over [0, 1] (`cf1c08b` / `0389af2`).
fn greville_cubic_curve(r: [f64; 5]) -> BSplineCurve {
    let g = [0.0, 1.0 / 6.0, 0.5, 5.0 / 6.0, 1.0];
    let control: Vec<Vec3> = (0..5).map(|i| Vec3::new(r[i], g[i], 0.0)).collect();
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[4, 1, 4],
        knots: &[0.0, 0.5, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap()
}

/// The cubic Greville law `r` along `e` from y = 0 to y = 20.
fn spline_round(k: &mut Kernel, b: &mut Brep, e: EdgeId, mut r: [f64; 5]) -> Result<()> {
    let ivl = edge::interval(b, e)?;
    let c = edge::curve(b, e)?.unwrap();
    let start = c.evaluate(k, ivl.min(), 0, true, false)?[0];
    if start.y > 10.0 {
        r.reverse();
    }
    let law = FilletLaw::bspline(greville_cubic_curve(r), &Extent1d::from_min_max(0.0, 1.0));
    {
        let mut s = FilletSession::new(k, b)?;
        s.prepare_variable_radius_edges(k, &[e], r[0], law, 1, 1)?;
        s.do_fillet(k)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(())
}

fn volume_of(k: &Kernel, b: &mut Brep) -> Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs)
}

/// What grading a case found.
#[derive(Debug)]
struct Graded {
    /// The status of the blend, when it failed.
    failed: Option<String>,
    /// What's wrong with the result.
    problems: Vec<String>,
    volume: f64,
    off: f64,
    seams: Vec<(usize, usize)>,
}

fn on_face(k: &Kernel, s: &Surface, q: Quadric) -> bool {
    let Ok(dom) = s.natural_uv_domain_with_nurb(k) else {
        return false;
    };
    let mid = Point2::new(
        0.5 * (dom.min().x + dom.max().x),
        0.5 * (dom.min().y + dom.max().y),
    );
    s.evaluate_point(k, mid)
        .is_ok_and(|m| q.distance(m) <= 1e-9)
}

fn grade(case: &Case, k: &mut Kernel) -> Graded {
    let mut g = Graded {
        failed: None,
        problems: Vec::new(),
        volume: f64::NAN,
        off: 0.0,
        seams: Vec::new(),
    };
    let mut b = (case.build)(k).unwrap();
    let off_tol = case.off_tol.unwrap_or(100.0 * b.tolerance());
    let e = pick(k, &b, case.pick);
    let made = match case.blend {
        Blend::Chord(d) => chord(k, &mut b, e, d),
        Blend::Round(r) => fillet_edges(k, &mut b, &[e], r, 1, 1, 1.0),
        Blend::LinearRound(a, c) => linear_round(k, &mut b, e, a, c),
        Blend::SplineRound(r) => spline_round(k, &mut b, e, r),
        Blend::Gn(n, w, r) => fillet_edges(k, &mut b, &[e], r, 2, n, w),
    };
    if let Err(err) = made {
        g.failed = Some(format!("{err:?}"));
        return g;
    }
    g.volume = volume_of(k, &mut b).unwrap();
    if (g.volume - case.volume).abs() > case.volume_rel * case.volume.abs() {
        g.problems
            .push(format!("volume {:.9}, want {:.9}", g.volume, case.volume));
    }
    let lines = check_brep(k, &mut b, CheckLevel::Level2).unwrap().lines();
    if !lines.is_empty() {
        g.problems
            .push(format!("{} failed validity tests", lines.len()));
    }
    let tight = [1e-9, 1e-7, 1e-5, 1e-3];
    let loose = [1f64.to_radians().tan(), 0.01, 0.001, 0.0001];
    let (mut blends, mut seams) = (0, 0);
    let ids: Vec<FaceId> = b.faces().ids().collect();
    let edges: Vec<EdgeId> = b.edge_list().to_vec();
    for f in ids {
        let Some(srf) = face::surface(&b, f).unwrap() else {
            continue;
        };
        if case.faces.iter().any(|&q| on_face(k, srf, q)) {
            continue;
        }
        blends += 1;
        let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
        for a in 0..=16 {
            for c in 0..=16 {
                let uv = Point2::new(
                    dom.min().x + (dom.max().x - dom.min().x) * f64::from(a) / 16.0,
                    dom.min().y + (dom.max().y - dom.min().y) * f64::from(c) / 16.0,
                );
                let x = srf.evaluate_point(k, uv).unwrap();
                g.off = g.off.max((case.off)(x));
            }
        }
        for &e in &edges {
            let uses = edge::edgeuses(&b, e, None, None).unwrap();
            let (mut mine, mut other) = (None, None);
            for &u in &uses {
                match eu1::face(&b, u).unwrap() {
                    Some(h) if h == f => mine = Some(u),
                    Some(_) => other = Some(u),
                    None => {}
                }
            }
            let (Some(mine), Some(other)) = (mine, other) else {
                continue;
            };
            let (Some(tm), Some(to)) = (
                eu1::uv_trim_curve(&b, mine).unwrap().0,
                eu1::uv_trim_curve(&b, other).unwrap().0,
            ) else {
                continue;
            };
            let of = eu1::face(&b, other).unwrap().unwrap();
            let osrf = face::surface(&b, of).unwrap().unwrap();
            if !on_face(k, osrf, case.faces[case.rails[0]])
                && !on_face(k, osrf, case.faces[case.rails[1]])
            {
                continue; // an end of the blend, not one of its contact lines
            }
            seams += 1;
            let ivl = edge::interval(&b, e).unwrap();
            let curve = edge::curve(&b, e).unwrap().unwrap();
            let (mut most, mut least) = ([0.0f64; 4], [f64::INFINITY; 4]);
            for st in 1..=9 {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(st) / 10.0;
                let pt = curve.evaluate(k, t, 1, true, false).unwrap();
                let (p, along) = (pt[0], pt[1].unitize().unwrap());
                let w1 = tm.evaluate(k, t, 0, true, false).unwrap()[0];
                let w2 = to.evaluate(k, t, 0, true, false).unwrap()[0];
                let uv2 = Point2::new(w2.x, w2.y);
                let (_, du, dv) = osrf.evaluate_1st_derivatives(k, uv2, true, true).unwrap();
                let n = du.cross(dv).unitize().unwrap();
                let mut w = n.cross(along).unitize().unwrap();
                let uv = Point2::new(w1.x, w1.y);
                let inward = Point2::new(
                    uv.x + 1e-3 * (0.5 * (dom.min().x + dom.max().x) - uv.x),
                    uv.y + 1e-3 * (0.5 * (dom.min().y + dom.max().y) - uv.y),
                );
                let q = srf.evaluate_point(k, inward).unwrap();
                if (q.x - p.x) * w.x + (q.y - p.y) * w.y + (q.z - p.z) * w.z < 0.0 {
                    w = -w;
                }
                let step = (if case.fitted { 0.02 } else { 0.004 }) * case.size;
                let hb = seam_graph(k, srf, uv, p, along, w, n, step);
                let ho = seam_graph(k, osrf, uv2, p, along, w, n, -step);
                for m in 0..4 {
                    let d = match (&hb, &ho) {
                        (Some(a), Some(c)) => (a[m] - c[m]).abs(),
                        _ => f64::INFINITY,
                    };
                    most[m] = most[m].max(d);
                    least[m] = least[m].min(d);
                }
            }
            let reached = seam_order(&most, if case.fitted { &loose } else { &tight });
            let at_most = seam_order(&least, &loose);
            g.seams.push((reached, at_most));
            if reached < case.order || at_most > case.order_max.unwrap_or(case.order) {
                g.problems.push(format!(
                    "seam G{reached}, at most G{at_most}, want G{}",
                    case.order
                ));
            }
        }
    }
    if g.off > off_tol {
        g.problems.push(format!(
            "{:.3e} off the exact blend (allowed {off_tol:.3e})",
            g.off
        ));
    }
    if blends == 0 || seams < 2 {
        g.problems
            .push(format!("{blends} blend faces, {seams} seams"));
    }
    g
}

fn solidcraft_kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

const VARIABLE: &str = "variable_round_on_its_cone";
const BOSS_CHAMFER: &str = "boss_root_chamfer";

fn named(name: &str) -> Case {
    running_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap()
}

fn assert_passes(case: &Case, k: &mut Kernel) {
    let g = grade(case, k);
    println!("{}: {g:?}", case.name);
    assert!(
        g.failed.is_none() && g.problems.is_empty(),
        "{}: {g:?}",
        case.name
    );
    assert!(
        g.seams
            .iter()
            .all(|&(reached, at_most)| reached >= case.order
                && at_most <= case.order_max.unwrap_or(case.order)),
        "{}: {g:?}",
        case.name
    );
}

/// The fork's mark: "the round's sections are the balls' great circles, not the circles the
/// balls' envelope touches them along: 0.0166 off the cone, volume 11955.6466 against
/// 11956.2224".
fn assert_variable_mark(k: &mut Kernel) {
    let g = grade(&named(VARIABLE), k);
    println!("{VARIABLE}: {g:?}");
    assert!(g.failed.is_none(), "{g:?}");
    assert!((g.off - 0.0166).abs() <= 0.0001, "{g:?}");
    assert!((g.volume - 11955.6466).abs() <= 1e-4, "{g:?}");
    assert_eq!(g.problems.len(), 2, "{g:?}");
    assert_eq!(g.seams, [(1, 1), (1, 1)], "{g:?}");
}

#[test]
fn with_solidcraft_s_fixes_the_chords_and_circles_have_the_fork_s_marks() {
    for case in running_cases().iter().filter(|c| c.name != VARIABLE) {
        assert_passes(case, &mut solidcraft_kernel());
    }
}

/// The mark is the fork's at ea59e9c, the great circles. `fillet_gn_section_generator` brings
/// the fork's later touch circles, tested with that port, so it stays off here.
#[test]
fn with_solidcraft_s_fixes_the_variable_round_has_the_fork_s_mark() {
    let mut k = solidcraft_kernel();
    k.fixes.fillet_gn_section_generator = false;
    assert_variable_mark(&mut k);
}

#[test]
fn on_the_default_kernel_the_chords_and_circles_pass_but_the_boss_root_s_chord() {
    for case in running_cases()
        .iter()
        .filter(|c| c.name != VARIABLE && c.name != BOSS_CHAMFER)
    {
        assert_passes(case, &mut Kernel::default());
    }
}

#[test]
fn on_the_default_kernel_the_variable_round_has_the_fork_s_mark() {
    assert_variable_mark(&mut Kernel::default());
}

/// Upstream traces a chord round a circle: 3.07e-5 off the cone it should lie on, where the
/// fork builds the cone (`fillet_chamfer_cone_on_arc`).
#[test]
fn the_boss_root_s_chord_is_exact_only_with_the_cone_on_the_arc() {
    let case = named(BOSS_CHAMFER);
    let g = grade(&case, &mut Kernel::default());
    println!("{BOSS_CHAMFER}: {g:?}");
    assert!((g.off - 3.07e-5).abs() <= 0.01 * 3.07e-5, "{g:?}");
    assert_eq!(g.problems.len(), 1, "{g:?}");
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = true;
    assert_passes(&case, &mut k);
}

/// The grading sees what it's for: a wrong volume, a wrong exact blend and a wrong order of
/// contact are each refused on a case that passes.
#[test]
fn the_grading_refuses_a_wrong_volume_blend_and_order() {
    let refused = |change: &dyn Fn(&mut Case)| {
        let mut case = named("box_edge_round");
        change(&mut case);
        grade(&case, &mut Kernel::default()).problems
    };
    assert!(refused(&|_| {}).is_empty());
    let volume = refused(&|c| c.volume *= 1.000_001);
    assert!(volume[0].contains("volume"), "{volume:?}");
    let blend = refused(&|c| {
        c.off = Box::new(|x| ((x.y - 27.0).hypot(x.z - 17.0) - 3.000_001).abs());
    });
    assert!(blend[0].contains("off the exact blend"), "{blend:?}");
    for order in [0, 2] {
        let wrong = refused(&|c| c.order = order);
        assert!(wrong.iter().all(|p| p.contains("seam G1")), "{wrong:?}");
        assert_eq!(wrong.len(), 2, "{wrong:?}");
    }
}

/// Area of a closed polygon by the shoelace formula, and its first moment about x = `x0`.
fn area_and_moment(pts: &[P2], x0: f64) -> (f64, f64) {
    let (mut a, mut m) = (0.0, 0.0);
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        let c = p[0] * q[1] - q[0] * p[1];
        a += c;
        m += c * (p[0] + q[0]);
    }
    let area = 0.5 * a;
    (area, m / 6.0 - x0 * area)
}

fn section(n: usize, w: f64, c1: &[P2], c2: &[P2]) -> Vec<P2> {
    let m = 200_000;
    (0..=m)
        .map(|i| gn_section(n, w, c1, c2, f64::from(i) / f64::from(m)))
        .collect()
}

fn near(got: f64, want: f64, tol: f64, what: &str) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: {got:.15}, want {want:.15}"
    );
}

/// The areas the fork's volumes are worked out from, from `gn_section`.
#[test]
fn the_sections_give_the_fork_s_areas_on_the_box_edge() {
    let cut = |n, w| {
        let mut pts = section(n, w, &BOX_TOP_JETS, &BOX_BACK_JETS);
        pts.push([30.0, 20.0]);
        area_and_moment(&pts, 0.0).0.abs() / 9.0
    };
    near(cut(0, 0.0), 0.5, 1e-9, "chord");
    near(cut(2, 0.0), 0.154192655384275, 1e-9, "order 2, w = 0");
    near(
        cut(2, 0.034293402266607321),
        113.0 / 756.0,
        1e-9,
        "order 2, w = w*",
    );
    near(cut(2, 0.5), 0.0744372522086925, 1e-9, "order 2, w = 1/2");
}

#[test]
fn the_section_gives_the_fork_s_figures_on_the_boss_root() {
    let mut pts = section(2, 0.0, &PLATE_JETS, &WALL_JETS);
    pts.push([8.0, 10.0]);
    let (a, m) = area_and_moment(&pts, 8.0);
    let (a, m) = (a.abs(), m.abs());
    near(a, 0.616770621537099, 1e-9, "area");
    near(m, 0.233402636580288, 1e-9, "moment");
    let v0 = 24000.0 + 640.0 * PI;
    near(
        v0 + 2.0 * PI * (8.0 * a + m),
        26043.0880831714,
        1e-6,
        "volume",
    );
}

#[test]
fn the_sections_give_the_fork_s_volumes_on_the_pipe() {
    let v0 = 40165.7846135291;
    let added = |n, w, d: f64| {
        let (plate, pipe) = pipe_jets(d);
        let mut pts = section(n, w, &plate, &pipe);
        // the pipe's arc back from its contact to the edge (28, 10), the plate's line after it
        let (a0, a1) = (
            (pts[pts.len() - 1][1] - 16.0).atan2(pts[pts.len() - 1][0] - 20.0),
            (10.0f64 - 16.0).atan2(8.0),
        );
        for i in 1..=20_000 {
            let a = a0 + (a1 - a0) * f64::from(i) / 20_000.0;
            pts.push([20.0 + 10.0 * a.cos(), 16.0 + 10.0 * a.sin()]);
        }
        area_and_moment(&pts, 0.0).0.abs()
    };
    near(
        v0 + 60.0 * added(0, 0.0, 1.0),
        40241.7164766411,
        1e-5,
        "chord",
    );
    near(
        v0 + 60.0 * added(2, 0.0, 2.0),
        40258.5435962825,
        1e-5,
        "order 2, w = 0",
    );
    near(
        v0 + 60.0 * added(2, 0.5, 2.0),
        40216.5022652017,
        1e-5,
        "order 2, w = 1/2",
    );
}

// ---- the order-2 cases (the port of 11ea2d9, #2260) ----

const W_STAR: f64 = 0.034_293_402_266_607_321;

fn box_g2(name: &'static str, w: f64, cut: f64) -> Case {
    Case {
        name,
        build: box_40_30_20,
        pick: [20.0, 30.0, 20.0],
        blend: Blend::Gn(2, w, 3.0),
        volume: 24000.0 - 40.0 * 9.0 * cut,
        volume_rel: 1e-7,
        off: Box::new(move |x| {
            distance_to_curve(
                &|t| gn_section(2, w, &BOX_TOP_JETS, &BOX_BACK_JETS, t),
                [x.y, x.z],
            )
        }),
        off_tol: Some(1e-9),
        faces: box_planes(40.0, 30.0, 20.0),
        rails: [5, 3],
        order: 3,
        fitted: false,
        order_max: None,
        size: 3.0,
    }
}

fn pipe_g2(name: &'static str, w: f64, volume: f64) -> Case {
    let (plate, pipe) = pipe_jets(2.0);
    Case {
        name,
        build: pipe_on_plate,
        pick: [30.0, 28.0, 10.0],
        blend: Blend::Gn(2, w, 2.0),
        volume,
        volume_rel: 1e-7,
        off: Box::new(move |x| {
            distance_to_curve(&|t| gn_section(2, w, &plate, &pipe, t), [x.y, x.z])
        }),
        off_tol: Some(1e-9),
        faces: pipe_faces(),
        rails: [5, 6],
        order: 2,
        fitted: false,
        order_max: None,
        size: 2.0,
    }
}

fn boss_g2() -> Case {
    Case {
        name: "boss_root_g2",
        build: boss_on_plate,
        pick: [30.0 + 4.0 * 2f64.sqrt(), 20.0 + 4.0 * 2f64.sqrt(), 10.0],
        blend: Blend::Gn(2, 0.0, 2.0),
        volume: 26043.0880831714,
        volume_rel: 1e-7,
        off: Box::new(|x| {
            distance_to_curve(
                &|t| gn_section(2, 0.0, &PLATE_JETS, &WALL_JETS, t),
                rho_z(x),
            )
        }),
        off_tol: Some(4e-6),
        faces: boss_faces(),
        rails: [5, 6],
        order: 2,
        fitted: true,
        order_max: Some(3),
        size: 2.0,
    }
}

/// The G^n section's flag and the one its plane rails need, nothing else.
fn gn_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_surface_plane_curve_on_plane_equation = true;
    k
}

/// Each order-2 case on the G^n section's own flags and with SolidCraft's fixes.
fn assert_order_2(case: &Case) {
    assert_passes(case, &mut gn_kernel());
    assert_passes(case, &mut solidcraft_kernel());
}

#[test]
fn the_box_edge_at_order_2_is_g3_with_its_planes() {
    assert_order_2(&box_g2("box_edge_g2", 0.0, 0.154_192_655_384_275));
}

#[test]
fn the_box_edge_at_the_equal_area_thumbweight_is_g3_with_its_planes() {
    assert_order_2(&box_g2(
        "box_edge_g2_thumbweight_w_star",
        W_STAR,
        113.0 / 756.0,
    ));
}

#[test]
fn the_box_edge_at_thumbweight_one_half_is_g3_with_its_planes() {
    assert_order_2(&box_g2(
        "box_edge_g2_thumbweight_half",
        0.5,
        0.074_437_252_208_692_5,
    ));
}

#[test]
fn the_pipe_root_at_order_2_is_g2_with_its_faces() {
    assert_order_2(&pipe_g2("pipe_on_plate_g2", 0.0, 40258.5435962825));
}

#[test]
fn the_pipe_root_at_thumbweight_one_half_is_g2_with_its_faces() {
    assert_order_2(&pipe_g2(
        "pipe_on_plate_g2_thumbweight_half",
        0.5,
        40216.5022652017,
    ));
}

#[test]
#[ignore = "slow: 4 min per kernel in the difftest profile, a 227-span certificate"]
fn the_boss_root_at_order_2_is_at_least_g2_with_its_faces() {
    assert_order_2(&boss_g2());
}

// ---- equal-area G2 (`b30ad9f`, `e447863`) and variable union-of-balls (`4af4349`, `cf1c08b`, `0389af2`) ----

/// Distance from `x` to the boundary of the union of rolling balls along `(10, 20 s, 30)`
/// (`s in [0, 1]`) of radius `R(s) = law(s).x`, touching `x = 10` and `z = 30`.
fn distance_to_balls_union(law: &BSplineCurve, x: Point3) -> f64 {
    let signed = |s: f64| {
        let r = law.evaluate_point(s).unwrap().x;
        let c = Point3::new(10.0 - r, 20.0 * s, 30.0 - r);
        x.distance(c) - r
    };
    let m = 512;
    let mut best = 0;
    let mut bd = signed(0.0);
    for k in 1..=m {
        let d = signed(f64::from(k) / f64::from(m));
        if d < bd {
            bd = d;
            best = k;
        }
    }
    let mut lo = (f64::from(best) - 1.0).max(0.0) / f64::from(m);
    let mut hi = (f64::from(best) + 1.0).min(f64::from(m)) / f64::from(m);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..80 {
        let (s1, s2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if signed(s1) < signed(s2) {
            hi = s2;
        } else {
            lo = s1;
        }
    }
    signed(0.5 * (lo + hi)).abs()
}

/// `b30ad9f` + `e447863`: at `GN_THUMBWEIGHT_EQUAL_AREA` (`w = -0.5252...`), the box edge's
/// order-2 section removes the circle's area `(1 - pi/4) r^2`, is G2 at `tight` tolerances,
/// and at most G3 at Continuity Analysis's `loose` tolerances.
#[test]
fn the_box_edge_at_gn_equal_area_default_is_g2_and_at_most_g3_with_its_planes() {
    let w_eq = GnSectionFsg::equal_area_thumbweight(2, PI / 2.0).unwrap();
    let case = Case {
        name: "box_edge_g2_equal_area_default",
        build: box_40_30_20,
        pick: [20.0, 30.0, 20.0],
        blend: Blend::Gn(2, GN_THUMBWEIGHT_EQUAL_AREA, 3.0),
        volume: 24000.0 - 40.0 * 9.0 * (1.0 - PI / 4.0),
        volume_rel: 1e-7,
        off: Box::new(move |x| {
            distance_to_curve(
                &|t| gn_section(2, w_eq, &BOX_TOP_JETS, &BOX_BACK_JETS, t),
                [x.y, x.z],
            )
        }),
        off_tol: Some(1e-9),
        faces: box_planes(40.0, 30.0, 20.0),
        rails: [5, 3],
        order: 2,
        order_max: Some(3),
        fitted: false,
        size: 3.0,
    };
    assert_order_2(&case);
}

/// `4af4349`: with `fillet_gn_section_generator` on, `variable_round_on_its_cone` lies on the
/// exact cone of its balls' envelope, passes `Level2`, and is G1 along both seams.
#[test]
fn the_linear_variable_round_lies_on_its_cone_and_is_g1() {
    let case = named(VARIABLE);
    assert_passes(&case, &mut gn_kernel());
    assert_passes(&case, &mut solidcraft_kernel());
}

/// `cf1c08b` + `0389af2`: the progressive cubic law `[2, 2, 6, 3, 0]` lies on the union of its
/// balls, passes `Level2`, matches the fork's volume `5957.7178399654949`, and is G1 along both
/// seams.
#[test]
fn the_progressive_cubic_variable_round_lies_on_its_balls_union_and_is_g1() {
    let holds_law = [2.0, 2.0, 6.0, 3.0, 0.0];
    let holds_curve = greville_cubic_curve(holds_law);
    let case = Case {
        name: "variable_round_spline_holds",
        build: box_10_20_30,
        pick: [10.0, 10.0, 30.0],
        blend: Blend::SplineRound(holds_law),
        volume: 5957.7178399654949,
        volume_rel: 1e-7,
        off: Box::new(move |x| distance_to_balls_union(&holds_curve, x)),
        off_tol: None,
        faces: box_planes(10.0, 20.0, 30.0),
        rails: [1, 5],
        order: 1,
        order_max: None,
        fitted: true,
        size: 1.0,
    };
    assert_passes(&case, &mut gn_kernel());
    assert_passes(&case, &mut solidcraft_kernel());
}

/// `cf1c08b` + `0389af2`: the folded cubic law `[2, 2, 40, 3, 2]` builds folded (far off the
/// union of its balls) when `fillet_gn_section_generator` is off, and is refused when on.
#[test]
fn the_folded_cubic_variable_round_leaves_its_balls_union_without_fix_and_is_refused_with_fix() {
    let folds_law = [2.0, 2.0, 40.0, 3.0, 2.0];
    let folds_curve = greville_cubic_curve(folds_law);
    let case = Case {
        name: "variable_round_spline_folds",
        build: box_10_20_30,
        pick: [10.0, 10.0, 30.0],
        blend: Blend::SplineRound(folds_law),
        volume: 6000.0 - 332.2,
        volume_rel: 1e-3,
        off: Box::new(move |x| distance_to_balls_union(&folds_curve, x)),
        off_tol: None,
        faces: box_planes(10.0, 20.0, 30.0),
        rails: [1, 5],
        order: 1,
        order_max: None,
        fitted: true,
        size: 1.0,
    };
    let mut k_off = solidcraft_kernel();
    k_off.fixes.fillet_gn_section_generator = false;
    let g_off = grade(&case, &mut k_off);
    assert!(g_off.failed.is_none(), "{g_off:?}");
    assert!(
        g_off.off > 1.0,
        "expected folded surface far off balls union: {g_off:?}"
    );
    assert!(!g_off.problems.is_empty(), "{g_off:?}");

    let g_on = grade(&case, &mut solidcraft_kernel());
    assert!(
        g_on.failed.is_some(),
        "expected folded law to be refused: {g_on:?}"
    );
}
