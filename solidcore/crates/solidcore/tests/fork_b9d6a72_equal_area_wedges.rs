//! The SolidCraft fork's capi cases `sharp_wedge_g2_equal_area` (`ops_kernel.cpp`,
//! tangency-labs/smlib@b9d6a72) and `twisted_wedge_g2_equal_area` (04e49b9, regraded by
//! f2fb13e and 79db628; as at v1.1.0-alpha), smlib#79, tangency-labs/solidcore#2273: the
//! order-2 section's equal area thumbweight, resolved at every station from its own wedge
//! (b9d6a72, 7102bc5; brought over by #2274).
//!
//! The sharp wedge: an equilateral prism, side 40 and 10 tall, its edge at the origin (a 60
//! degree wedge) blended at size 3 by the order-2 section at the equal area thumbweight. The
//! section takes off the ball's circle's area, r^2 (cot(alpha/2) - (pi - alpha)/2) = 9 (sqrt 3
//! - pi/3) per mm, at a thumbweight below -1 (refused while w was kept to (-1, 1)).
//!
//! The twisted wedge: the block 60 x 30 x 50 below the fork's `twisted_top` (z = 25 + a(x) y, a
//! cubic spline in x: a = -tan 30 on x <= 12, 0 on 24..36, tan 30 on x >= 48, a twist between),
//! its front edge y = 0, z = 25 blended at size 3 by the order-2 section at the equal area
//! thumbweight. The wedge there opens from 60 to 120 degrees along the edge. Where a is
//! constant the top is a plane, the section at x lies in the plane x = const, and at its own
//! thumbweight it takes off the circle's area: 6.163679 at 60 degrees, 1.931417 at 90 and
//! 0.483764 at 120 (one thumbweight for the edge misses them by up to a fifth). It is graded
//! as the fork's `section_blend` grades it: the status, SMLib's validity check at level 2, the
//! blend face a definition whose net is within its certified bound of it on a 17 x 17 grid,
//! the three plane stations' areas and planes, the net's spans, and at nine stations along
//! each seam with the front or the top the order of contact, from the two surfaces' graphs in
//! the plane square to the seam (degree-7 fits through seven points each side, as in
//! `fork_ea59e9c_section_orders.rs`): G2 at Continuity Analysis's tolerances (a fitted blend)
//! at every station, and no station past G3. The span count is v1.1.0-alpha's 274 (79db628
//! had 275, 04e49b9 288 with the feet's own order-2 jets): solidcore's port gives 274. The
//! case is ignored by default as slow, like `fork_ea59e9c_section_orders.rs`'s 227-span
//! boss root: almost all of its time is the blend's certificate.
//!
//! Left out: the fork's counts of the definition's evaluations, station solves, certified
//! pieces and equal area sums and roots (work counts, which #2321 changes on purpose and which
//! differ from the fork's here, as `fork_7959c0e_round_on_a_round.rs` records), and
//! `twisted_wedge_g2_equal_area_net_on_definition` with the knot-line checks (#2356, #2384).
//!
//! How this test drives the kernel differs from the capi: the prism is a capped linear sweep
//! of its three sides; `smlib_split_body`'s back part is a slice by a one-face sheet on the
//! tool surface turned over (`SwapUV`), as the capi makes it; an edge is picked as the nearest
//! within 0.1 of a point; the blends are [`fillet_edges`] (the blend curve section of order 2
//! at [`GN_THUMBWEIGHT_EQUAL_AREA`], then `StitchAndOrient`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::needless_range_loop
)] // test helpers

use solidcore::blend::fillet_executive::api::{create_face_from_surface, fillet_edges};
use solidcore::blend::fillet_section::section::GN_THUMBWEIGHT_EQUAL_AREA;
use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::p1::MergeOptions;
use solidcore::curve::curve::Curve;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::tol;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3, PI};
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{EdgeId, FaceId};
use solidcore::{Brep, Kernel};

/// The G^n section's flag and the one its plane rails need, nothing else.
fn gn_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_surface_plane_curve_on_plane_equation = true;
    k
}

/// The kernel SolidCraft runs: every fix on but `geometric_g2_continuity`.
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

/// The edge nearest `p`, within 0.1.
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

/// The fork's `volume_of`: the body's volume to 1e-7.
fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// SMLib's validity check at level 2 finds nothing wrong with `b`.
fn sound(k: &mut Kernel, b: &mut Brep) -> bool {
    check_brep(k, b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .is_empty()
}

// ---- the sharp wedge (b9d6a72) ----

/// The fork's equilateral prism: the triangle (0, 0), (40, 0), (20, 20 sqrt 3) at z = 0
/// extruded 10 up.
fn prism(k: &mut Kernel) -> Brep {
    let h = 20.0 * 3f64.sqrt();
    let p = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(40.0, 0.0, 0.0),
        Point3::new(20.0, h, 0.0),
    ];
    let sides: Vec<Curve> = (0..3)
        .map(|i| Curve::from(create_line_segment(p[i], p[(i + 1) % 3]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// What the sharp wedge's round takes off the prism, and whether the result checks clean.
fn sharp_wedge(k: &mut Kernel) -> (f64, bool) {
    let mut b = prism(k);
    let e = pick(k, &b, [0.0, 0.0, 5.0]);
    fillet_edges(k, &mut b, &[e], 3.0, 2, 2, GN_THUMBWEIGHT_EQUAL_AREA).unwrap();
    let removed = 10.0 * (0.25 * 3f64.sqrt() * 1600.0) - volume_of(k, &mut b);
    (removed, sound(k, &mut b))
}

/// The fork's `sharp_wedge_g2_equal_area`: the round takes off 10 times the circle's
/// area at a 60 degree wedge, 9 (sqrt 3 - pi/3) per mm, to 1e-6, and checks clean; on the
/// G^n section's own flags and with SolidCraft's fixes.
#[test]
fn the_sharp_wedge_takes_off_the_circle_s_area() {
    let want = 10.0 * 9.0 * (3f64.sqrt() - PI / 3.0);
    for (name, mut k) in [("gn", gn_kernel()), ("solidcraft", solidcraft_kernel())] {
        let (removed, sound) = sharp_wedge(&mut k);
        println!("{name}: sharp wedge: removed {removed:.9}, want {want:.9}");
        assert!(
            (removed - want).abs() <= 1e-6 * want,
            "{name}: removed {removed}, want {want}"
        );
        assert!(sound, "{name}: the result does not check clean");
    }
}

// ---- the twisted wedge (04e49b9, f2fb13e, 79db628) ----

/// `TWIST_TAN`: tan 30 degrees.
const TWIST_TAN: f64 = 0.577_350_269_189_625_76;

/// The fork's `twisted_top`: cubic in u over the knots -4 (4 times), 0, 4, .., 60, 64
/// (4 times), linear in v from y -1 to 31; the control points' x Greville's, their z
/// `25 + a y`, a = -tan 30 for the first seven columns, 0 for the next six, tan 30 after.
/// u is x, v is (y + 1) / 32.
fn twisted_top() -> Surface {
    let mut ku = vec![-4.0; 4];
    let mut uk = vec![-4.0];
    let mut um = vec![4];
    for x in (0..=60).step_by(4) {
        ku.push(f64::from(x));
        uk.push(f64::from(x));
        um.push(1);
    }
    ku.extend_from_slice(&[64.0; 4]);
    uk.push(64.0);
    um.push(4);
    let nu = ku.len() - 4;
    let mut pts = Vec::new();
    for i in 0..nu {
        let x = (ku[i + 1] + ku[i + 2] + ku[i + 3]) / 3.0;
        let a = if i < 7 {
            -TWIST_TAN
        } else if i < 13 {
            0.0
        } else {
            TWIST_TAN
        };
        for y in [-1.0, 31.0] {
            pts.push(Point3::new(x, y, 25.0 + a * y));
        }
    }
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &um,
            v_multiplicities: &[2, 2],
            u_knots: &uk,
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// `smlib_split_body`'s back part: `body` sliced (`SM_BO_SLICE`, every option off) by a
/// one-face sheet of the body's tolerance on `tool` turned over.
fn split_back(k: &mut Kernel, body: Brep, tool: &Surface) -> Brep {
    let mut s = tool.clone();
    s.swap_uv(k).unwrap();
    let mut sheet =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    let t = body.tolerance();
    solidcore::topology::brep::p2::set_tolerance(k, &mut sheet, t, false, true).unwrap();
    create_face_from_surface(k, &mut sheet, &s).unwrap();
    boolean_with_options(
        k,
        body,
        sheet,
        BooleanOperation::Slice,
        MergeOptions::new(false, false, false, false, false),
    )
    .unwrap()
    .result
}

/// The block's faces before the blend: the five planes (a point and the unit normal) and the
/// twisted top, as their distance functions.
fn on_block_face(k: &Kernel, top: &Surface, i: usize, x: Point3) -> f64 {
    let plane = |p: [f64; 3], n: [f64; 3]| {
        ((x.x - p[0]) * n[0] + (x.y - p[1]) * n[1] + (x.z - p[2]) * n[2]).abs()
    };
    match i {
        0 => plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        1 => plane([60.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        2 => plane([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        3 => plane([0.0, 30.0, 0.0], [0.0, 1.0, 0.0]),
        4 => plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        // the top: its height off the cutting surface's at (x, y)
        _ => top
            .evaluate_point(k, Point2::new(x.x, (x.y + 1.0) / 32.0))
            .map_or(f64::INFINITY, |q| (x.z - q.z).abs()),
    }
}

/// The front (y = 0) and the top: the faces the blend meets along its contact lines.
const RAILS: [usize; 2] = [2, 5];
const BLOCK_FACES: usize = 6;

/// Whether the middle of `s`'s domain lies on block face `i`.
fn on_face(k: &Kernel, top: &Surface, s: &Surface, i: usize) -> bool {
    let Ok(dom) = s.natural_uv_domain_with_nurb(k) else {
        return false;
    };
    let mid = Point2::new(
        0.5 * (dom.min().x + dom.max().x),
        0.5 * (dom.min().y + dom.max().y),
    );
    s.evaluate_point(k, mid)
        .is_ok_and(|m| on_block_face(k, top, i, m) <= 1e-9)
}

/// A surface crossed by the plane through `p` square to `b`: the curve's height along `n`
/// over the line `p + sigma w`, at `sigma = j step` (j = 1..7), walked from `uv` by Newton.
/// Its first four derivatives at 0, from the polynomial of degree 7 through h(0) = 0 and
/// those seven (the fork's `seam_graph`, as `fork_ea59e9c_section_orders.rs` has it).
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

/// One plane station of the twisted wedge's section: where a is constant, the wedge's angle,
/// how far the section is off the plane x = x0, the area it takes off the wedge and the
/// circle's.
#[derive(Debug)]
struct Station {
    x0: f64,
    degrees: f64,
    off_plane: f64,
    area: f64,
    want: f64,
}

/// The fork's `also` check on a blend face: at the middle of each plane where a is
/// constant, the section (u where the rail is at x0, found by bisection on the first rail),
/// its area off the wedge by Green's theorem about the edge (x0, 0, 25), by Simpson's rule
/// on 20000 intervals with central differences, and how far it is off x = x0.
fn plane_stations(k: &Kernel, srf: &Surface) -> Vec<Station> {
    let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
    let (u0, u1, v0, v1) = (dom.min().x, dom.max().x, dom.min().y, dom.max().y);
    let at = |u: f64, v: f64| srf.evaluate_point(k, Point2::new(u, v)).unwrap();
    let mut out = Vec::new();
    for a in [-TWIST_TAN, 0.0, TWIST_TAN] {
        let x0 = if a < 0.0 {
            6.0
        } else if a > 0.0 {
            54.0
        } else {
            30.0
        };
        let (mut lo, mut hi) = (u0, u1);
        let rising = at(u1, v0).x > at(u0, v0).x;
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if (at(mid, v0).x < x0) == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let u = 0.5 * (lo + hi);
        let n = 20000;
        let h = 1e-6 * (v1 - v0);
        let (mut sum, mut plane) = (0.0, 0.0f64);
        for i in 0..=n {
            let v = v0 + (v1 - v0) * f64::from(i) / f64::from(n);
            let (va, vb) = (v0.max(v - h), v1.min(v + h));
            let (p, pa, pb) = (at(u, v), at(u, va), at(u, vb));
            let dy = (pb.y - pa.y) / (vb - va);
            let dz = (pb.z - pa.z) / (vb - va);
            let weight = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            sum += weight * (p.y * dz - (p.z - 25.0) * dy);
            plane = plane.max((p.x - x0).abs());
        }
        let area = (0.5 * sum * (v1 - v0) / (3.0 * f64::from(n))).abs();
        let alpha = 0.5 * PI + a.atan();
        let want = 9.0 * (1.0 / (0.5 * alpha).tan() - 0.5 * (PI - alpha));
        out.push(Station {
            x0,
            degrees: alpha.to_degrees(),
            off_plane: plane,
            area,
            want,
        });
    }
    out
}

/// What grading the twisted wedge found.
#[derive(Debug)]
struct Graded {
    sound: bool,
    /// Per blend face: whether it is a definition, the net's distance off the definition on
    /// the 17 x 17 grid and its certified bound, its spans in all (u spans times v spans).
    faces: Vec<(bool, f64, f64, usize)>,
    stations: Vec<Station>,
    /// Per seam with a rail face: the order every station reaches (at Continuity
    /// Analysis's tolerances) and the most any station reaches.
    seams: Vec<(usize, usize)>,
}

/// The fork's `twisted_wedge_g2_equal_area`, graded as its `section_blend` grades it (the
/// grades it sets: no volume and no exact blend, which have no closed form in the twist).
fn twisted_wedge(k: &mut Kernel) -> Graded {
    let top = twisted_top();
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 30.0, 50.0).unwrap();
    let mut b = split_back(k, block, &top);
    let e = pick(k, &b, [30.0, 0.0, 25.0]);
    fillet_edges(k, &mut b, &[e], 3.0, 2, 2, GN_THUMBWEIGHT_EQUAL_AREA).unwrap();
    let mut g = Graded {
        sound: sound(k, &mut b),
        faces: Vec::new(),
        stations: Vec::new(),
        seams: Vec::new(),
    };
    // the fitted blend's tolerances: 1 degree, 0.01 / mm, 0.001 / mm^2, 0.0001 / mm^3
    let loose = [1f64.to_radians().tan(), 0.01, 0.001, 0.0001];
    let size = 3.0;
    let ids: Vec<FaceId> = b.faces().ids().collect();
    let edges: Vec<EdgeId> = b.edge_list().to_vec();
    for f in ids {
        let Some(srf) = face::surface(&b, f).unwrap() else {
            continue;
        };
        if (0..BLOCK_FACES).any(|i| on_face(k, &top, srf, i)) {
            continue;
        }
        let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
        // a G^n blend is its definition: its net within the certified bound of it
        let (definition, gap, bound, spans) = match srf.kind() {
            SurfaceKind::ProceduralBlend(pb) => {
                let mut gap = 0.0f64;
                for a in 0..=16 {
                    for c in 0..=16 {
                        let uv = Point2::new(
                            dom.min().x + (dom.max().x - dom.min().x) * f64::from(a) / 16.0,
                            dom.min().y + (dom.max().y - dom.min().y) * f64::from(c) / 16.0,
                        );
                        let x = srf.evaluate_point(k, uv).unwrap();
                        let y = pb.net().evaluate_point(uv).unwrap();
                        gap = gap.max(x.distance(y));
                    }
                }
                let nu = pb.net().knots(SurfParamType::U, None).unwrap().0.len();
                let nv = pb.net().knots(SurfParamType::V, None).unwrap().0.len();
                (true, gap, pb.bound(), (nu - 1) * (nv - 1))
            }
            SurfaceKind::BSpline(bs) => {
                let nu = bs.knots(SurfParamType::U, None).unwrap().0.len();
                let nv = bs.knots(SurfParamType::V, None).unwrap().0.len();
                (false, f64::NAN, f64::NAN, (nu - 1) * (nv - 1))
            }
            _ => (false, f64::NAN, f64::NAN, 0),
        };
        g.faces.push((definition, gap, bound, spans));
        g.stations.extend(plane_stations(k, srf));
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
            if !RAILS.iter().any(|&i| on_face(k, &top, osrf, i)) {
                continue; // an end of the blend, not one of its contact lines
            }
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
                // w points into the blend: towards a point of its surface a step inside
                let uv = Point2::new(w1.x, w1.y);
                let inward = Point2::new(
                    uv.x + 1e-3 * (0.5 * (dom.min().x + dom.max().x) - uv.x),
                    uv.y + 1e-3 * (0.5 * (dom.min().y + dom.max().y) - uv.y),
                );
                let q = srf.evaluate_point(k, inward).unwrap();
                if (q.x - p.x) * w.x + (q.y - p.y) * w.y + (q.z - p.z) * w.z < 0.0 {
                    w = -w;
                }
                // a fitted blend's seam is graded over steps wide enough that its gap (the
                // fit's) does not read as a difference of the derivatives
                let step = 0.02 * size;
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
            println!("  seam: derivatives 1 to 4 differ by up to {most:?} (at least {least:?})");
            g.seams
                .push((seam_order(&most, &loose), seam_order(&least, &loose)));
        }
    }
    g
}

/// The twisted wedge on `k`, graded and printed.
fn graded_twisted_wedge(name: &str, k: &mut Kernel) -> Graded {
    let g = twisted_wedge(k);
    println!("{name}: sound {}, blend faces (definition, net off it, bound, spans) {:?}, seams (G, at most G) {:?}", g.sound, g.faces, g.seams);
    for s in &g.stations {
        println!(
            "  wedge {:.1} degrees at x = {}: the section off x by {:.3e}, takes off {:.9}, the circle {:.9}",
            s.degrees, s.x0, s.off_plane, s.area, s.want
        );
    }
    g
}

/// The fork's `twisted_wedge_g2_equal_area` at v1.1.0-alpha, with SolidCraft's fixes: the
/// blend is one definition, its net within the certified bound, in 274 spans; at each plane
/// station its section lies in its plane to 1e-9 and takes off the circle's area to 1e-6;
/// its seams with the front and the top are G2 at every station and nowhere past G3; and the
/// result checks clean.
#[test]
#[ignore = "slow: about 3.5 min in the difftest profile, a 274-span certificate"]
fn the_twisted_wedge_takes_off_the_circle_s_area_at_every_plane_station() {
    let g = graded_twisted_wedge("solidcraft", &mut solidcraft_kernel());
    assert!(g.sound, "{g:?}");
    assert_eq!(g.faces.len(), 1, "{g:?}");
    let (definition, gap, bound, spans) = g.faces[0];
    assert!(definition, "{g:?}");
    assert!(gap <= bound, "{g:?}");
    assert_eq!(spans, 274, "{g:?}");
    assert_eq!(g.stations.len(), 3, "{g:?}");
    for s in &g.stations {
        assert!(s.off_plane <= 1e-9, "{s:?}");
        assert!((s.area - s.want).abs() <= 1e-6 * s.want, "{s:?}");
    }
    assert!(g.seams.len() >= 2, "{g:?}");
    for &(reached, at_most) in &g.seams {
        assert!(reached >= 2 && at_most <= 3, "{g:?}");
    }
}
