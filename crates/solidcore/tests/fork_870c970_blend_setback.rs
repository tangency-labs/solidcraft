//! The SolidCraft fork's setback corner, `SmBlendSetback` (tangency-labs/smlib PR #79:
//! `870c970`, `beba428`, `3fdb76b`, `52da453`; tangency-labs/solidcore#2273): the capi cases
//! `SETBACK_CASES` of the fork's `capi/tests/ops_kernel.cpp`, with the fork's values and
//! tolerances. `870c970` committed the fill's cases as known failing; `beba428` and `3fdb76b`
//! make every case pass (`52da453` keeps them passing at the whole spoke's speed), which is
//! the end state here: all seven pass.
//!
//! The box corner is `x, y, z >= 0` at the origin: face f (0, 1, 2) the plane where coordinate
//! f is 0, its normal out of the material `-e_f`; the edge along axis a lies between faces
//! `(a + 1) % 3` and `(a + 2) % 3`.
//!
//! Each fill is graded as the fork grades it: the status and six patches; each setback
//! patch's end row on its round's section and its spring sides on their faces to
//! `1e-12 size`; every seam (the rounds' end sections, the spring curves' halves, the inner
//! curves' halves, the spokes) at nine stations by the graph grader (degree-7 fits through
//! seven points each side, step `0.0005 size`) reaching the fill's order at 1e-9, 1e-7 / mm,
//! 1e-5 / mm^2 and 1e-3 / mm^3, the normals within 1e-12 and for G2 the normal curvatures
//! across within `1e-9 / size`; and no patch past a face by more than `1e-12 size`
//! (`containment`, certified from the control points).
//!
//! What differs from the capi: the normal at a seam is the unit cross product of the
//! surface's first derivatives (the capi's `EvaluateNormal`), and the graph grader's Newton
//! walk evaluates with non-zero tangents (as `fork_ea59e9c_section_orders.rs` does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::needless_range_loop,
    clippy::neg_cmp_op_on_partial_ord
)] // test helpers

use solidcore::blend::blend_setback::{
    containment, distances, ease_surface, fill_hole, EaseRamp, Edge, Face, Round,
};
use solidcore::blend::fillet_section::section::{GnFaces, GnSectionFsg};
use solidcore::error::SolidError;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

fn unit(i: usize) -> Vec3 {
    let mut d = [0.0; 3];
    d[i] = 1.0;
    Vec3::new(d[0], d[1], d[2])
}

/// `box_corner_faces`.
fn box_corner_faces() -> Vec<Face> {
    (0..3)
        .map(|f| Face {
            point: Point3::new(0.0, 0.0, 0.0),
            normal: -unit(f),
        })
        .collect()
}

/// `box_corner_edges`.
fn box_corner_edges(radii: [f64; 3]) -> Vec<Edge> {
    (0..3)
        .map(|a| Edge {
            away: unit(a),
            radius: radii[a],
            faces: [(a + 1) % 3, (a + 2) % 3],
        })
        .collect()
}

/// `axis_point`.
fn axis_point(a: usize, along: f64, f: usize, at_f: f64, g: usize, at_g: f64) -> Point3 {
    let mut p = [0.0; 3];
    p[a] = along;
    p[f] = at_f;
    p[g] = at_g;
    Point3::new(p[0], p[1], p[2])
}

/// `box_corner_round`: the round of radius r along axis a over `[s, s + len]`, u along it,
/// v across from its rail on face `f = (a + 1) % 3` to its rail on face `g = (a + 2) % 3`.
/// Order 1: the ball's circle, the rational quadratic arc. Order 2: the order-2 section at
/// thumbweight 0 between the two planes, in Bernstein form of degree 5 the points `R1`,
/// `(R1 + E) / 2`, `E`, `E`, `(E + R2) / 2`, `R2` with weights 1, 2/5, 1/10, 1/10, 2/5, 1.
fn box_corner_round(a: usize, r: f64, s: f64, len: f64, order: usize) -> BSplineSurface {
    let (f, g) = ((a + 1) % 3, (a + 2) % 3);
    let mut pts = Vec::new();
    let mut w = Vec::new();
    let nv = if order == 1 { 3 } else { 6 };
    for i in 0..2 {
        let t = s + f64::from(i) * len;
        let r1 = axis_point(a, t, f, 0.0, g, r);
        let r2 = axis_point(a, t, f, r, g, 0.0);
        let e = axis_point(a, t, f, 0.0, g, 0.0);
        if order == 1 {
            pts.extend([r1, e, r2]);
            w.extend([1.0, 0.5f64.sqrt(), 1.0]);
        } else {
            pts.extend([r1, (r1 + e) * 0.5, e, e, (e + r2) * 0.5, r2]);
            w.extend([1.0, 0.4, 0.1, 0.1, 0.4, 1.0]);
        }
    }
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 1,
        v_degree: nv - 1,
        control_points: &pts,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &[2, 2],
        v_multiplicities: &[nv, nv],
        u_knots: &[s, s + len],
        v_knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&w),
        uv_domain: None,
    })
    .unwrap()
}

fn line(dim: u64, a: Point3, b: Point3) -> BSplineCurve {
    BSplineCurve::create_line_segment(dim, a, b).unwrap()
}

/// `box_corner_gn_round`: the same order-2 round (thumbweight 0) built by the G^n section
/// generator (`SmGnSectionFSG`), from its centre line and rails (lines) on the faces, each
/// face a bilinear B-spline patch 20 square from the corner (uv = `(p.x, p.y) / 20` in its
/// axes). Its end section is checked against the closed form to 1e-12.
fn box_corner_gn_round(k: &Kernel, a: usize, r: f64, s: f64, len: f64) -> BSplineSurface {
    let (f, g) = ((a + 1) % 3, (a + 2) % 3);
    let l = 20.0;
    let plane = |face: usize| {
        let x = unit((face + 1) % 3) * (1.0 / l);
        let y = unit((face + 2) % 3) * (1.0 / l);
        let o = Point3::new(0.0, 0.0, 0.0);
        let srf =
            BSplineSurface::create_bilinear_surface(o, x * (l * l), y * (l * l), (x + y) * (l * l))
                .unwrap();
        (Surface::from(srf), x, y)
    };
    let (pf, xf, yf) = plane(f);
    let (pg, xg, yg) = plane(g);
    let c0 = axis_point(a, s, f, r, g, r);
    let c1 = axis_point(a, s + len, f, r, g, r);
    let r10 = axis_point(a, s, f, 0.0, g, r);
    let r11 = axis_point(a, s + len, f, 0.0, g, r);
    let r20 = axis_point(a, s, f, r, g, 0.0);
    let r21 = axis_point(a, s + len, f, r, g, 0.0);
    let uv = |p: Point3, x: Vec3, y: Vec3| Point3::new(p.dot(x), p.dot(y), 0.0);
    let centre = line(3, c0, c1);
    let mut rail1 = line(3, r10, r11);
    let mut rail2 = line(3, r20, r21);
    let uv1 = line(2, uv(r10, xf, yf), uv(r11, xf, yf));
    let uv2 = line(2, uv(r20, xg, yg), uv(r21, xg, yg));
    let at = uv(r10, xf, yf);
    let on = pf.evaluate_point(k, Point2::new(at.x, at.y)).unwrap();
    assert!(on.distance(r10) <= 1e-14);
    let out = GnSectionFsg::new(2, 0.0)
        .create_surface_from_curves(
            k,
            &centre,
            [&mut rail1, &mut rail2],
            1e-6,
            GnFaces {
                surfaces: [&pf, &pg],
                uv: [&uv1, &uv2],
            },
        )
        .unwrap();
    // (a plain B-spline without `fillet_round_as_definition`)
    let solidcore::surface::surface::SurfaceKind::BSpline(out) = out.into_kind() else {
        panic!("the G^n round is a B-spline without the definition flag");
    };
    // its end section against the closed form
    let closed = box_corner_round(a, r, s, len, 2);
    let u0 = out.natural_uv_domain().u_interval().min();
    let mut off = 0.0f64;
    for j in 0..=8 {
        let v = f64::from(j) / 8.0;
        let p = out.evaluate_point(Point2::new(u0, v)).unwrap();
        let q = closed.evaluate_point(Point2::new(s, v)).unwrap();
        off = off.max(p.distance(q));
    }
    println!(
        "  SmGnSectionFSG round {a}: degree {} across, end section off the closed form by {off:.3e}",
        out.degree(SurfParamType::V)
    );
    assert!(off <= 1e-12, "round {a}: end section off by {off}");
    out
}

/// A surface's point and unit normal at `uv` (the capi's `EvaluatePoint` and
/// `EvaluateNormal`).
fn point_normal(k: &Kernel, s: &Surface, uv: Point2) -> Option<(Point3, Vec3)> {
    let (p, du, dv) = s.evaluate_1st_derivatives(k, uv, true, true).ok()?;
    Some((p, du.cross(dv).unitize().ok()?))
}

/// `seam_graph`: a surface crossed by the plane through `p` square to `b`: the curve's height
/// along `n` over the line `p + sigma w`, at `sigma = j step` (j = 1..7), walked from `uv` by
/// Newton. Its first four derivatives at 0, from the polynomial of degree 7 through
/// `h(0) = 0` and those seven.
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
    for j in 1..=7usize {
        let sigma = j as f64 * step;
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
        let row = &mut m[j - 1];
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

/// `seam_order`.
fn seam_order(diff: &[f64; 4], tol: &[f64; 4]) -> usize {
    let mut k = 0;
    while k < 4 && diff[k] <= tol[k] {
        k += 1;
    }
    k
}

/// `cross_curvature`: the normal curvature of `s` at `uv` in the direction of its tangent
/// `w` (`w`'s part in the tangent plane), from its first and second derivatives.
fn cross_curvature(k: &Kernel, s: &Surface, uv: Point2, w: Vec3) -> Option<f64> {
    let flags = EvaluateFlags {
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    let d = s.evaluate(k, uv, 2, 2, flags).ok()?;
    let (su, sv, suu, suv, svv) = (d[1][0], d[0][1], d[2][0], d[1][1], d[0][2]);
    let nrm = su.cross(sv).unitize().ok()?;
    let (g11, g12, g22) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (b1, b2) = (w.dot(su), w.dot(sv));
    let det = g11 * g22 - g12 * g12;
    if !(det > 0.0) {
        return None;
    }
    let du = (b1 * g22 - b2 * g12) / det;
    let dv = (g11 * b2 - g12 * b1) / det;
    let d1 = su * du + sv * dv;
    let d2 = suu * (du * du) + suv * (2.0 * du * dv) + svv * (dv * dv);
    Some(nrm.dot(d2) / d1.dot(d1))
}

/// One side of a seam: the surface, its uv along the seam (t in [0, 1]) and a uv step into it.
struct SeamSide<'a> {
    s: &'a Surface,
    uv: Box<dyn Fn(f64) -> Point2 + 'a>,
    inside: Vec2,
}

/// `grade_seam`: the order the surfaces a and b meet to across their common boundary at nine
/// stations, the normals within 1e-12, and for order 2 the normal curvatures across within
/// `1e-9 / size`.
fn grade_seam(k: &Kernel, name: &str, a: &SeamSide<'_>, b: &SeamSide<'_>, size: f64, order: usize) {
    let tight = [1e-9, 1e-7, 1e-5, 1e-3];
    let (mut most, mut normals, mut curvatures) = ([0.0f64; 4], 0.0f64, 0.0f64);
    let mut ok = true;
    for st in 1..=9 {
        let t = f64::from(st) / 10.0;
        let h = 1e-4;
        let uv = (a.uv)(t);
        let station = (|| {
            let (p, n) = point_normal(k, a.s, uv)?;
            let p0 = a.s.evaluate_point(k, (a.uv)(t - h)).ok()?;
            let p1 = a.s.evaluate_point(k, (a.uv)(t + h)).ok()?;
            let along = (p1 - p0).unitize().ok()?;
            let mut w = n.cross(along).unitize().ok()?;
            let q =
                a.s.evaluate_point(
                    k,
                    Point2::new(uv.x + 1e-3 * a.inside.x, uv.y + 1e-3 * a.inside.y),
                )
                .ok()?;
            if (q - p).dot(w) < 0.0 {
                w = -w;
            }
            // the setback patches are small and bend hard: at 0.0005 size the degree-7 fit's
            // truncation is under the tight tolerances
            let step = 0.0005 * size;
            let ha = seam_graph(k, a.s, uv, p, along, w, n, step)?;
            let hb = seam_graph(k, b.s, (b.uv)(t), p, along, w, n, -step)?;
            let (_, nb) = point_normal(k, b.s, (b.uv)(t))?;
            let ka = cross_curvature(k, a.s, uv, w)?;
            let kb = cross_curvature(k, b.s, (b.uv)(t), w)?;
            Some((ha, hb, n.cross(nb).length(), (ka - kb).abs()))
        })();
        let Some((ha, hb, nn, kk)) = station else {
            ok = false;
            break;
        };
        normals = normals.max(nn);
        curvatures = curvatures.max(kk);
        for m in 0..4 {
            most[m] = most[m].max((ha[m] - hb[m]).abs());
        }
    }
    let reached = if ok {
        seam_order(&most, &tight) as i64
    } else {
        -1
    };
    println!(
        "  seam {name}: derivatives 1 to 4 differ by up to {:.3e}, {:.3e}, {:.3e}, {:.3e}: G{reached}; normals {normals:.3e} apart, curvatures across {curvatures:.3e}",
        most[0], most[1], most[2], most[3]
    );
    assert!(
        reached >= order as i64,
        "seam {name}: G{reached} < G{order}"
    );
    assert!(
        ok && normals <= 1e-12 && (order < 2 || curvatures <= 1e-9 / size),
        "seam {name}: normals {normals}, curvatures {curvatures}"
    );
}

/// `setback_box_corner`: the box corner's rounds of radii `radii`, set back by `distances`,
/// made to `round_order` (3: order 2 by the G^n section generator) and eased into the
/// order-2 section over `ease` before their ends when that is positive, filled to `order`.
fn setback_box_corner(radii: [f64; 3], round_order: usize, ease: f64, order: usize) {
    let k = Kernel::default();
    let faces = box_corner_faces();
    let setbacks = distances(&faces, &box_corner_edges(radii))
        .unwrap()
        .setbacks;
    assert_eq!(setbacks.len(), 3);
    let len = 10.0;
    let mut rounds = Vec::new();
    for a in 0..3 {
        let (r, s) = (radii[a], setbacks[a]);
        let mut own = if round_order == 3 {
            box_corner_gn_round(&k, a, r, s, len)
        } else {
            box_corner_round(a, r, s, len, round_order)
        };
        if ease > 0.0 {
            let g2 = box_corner_round(a, r, s, len, 2);
            let ramp = [EaseRamp {
                from: s + ease,
                to: s,
            }];
            own = ease_surface(&own, &g2, SurfParamType::U, &ramp).unwrap();
        }
        rounds.push(own);
    }
    let input: Vec<Round<'_>> = rounds
        .iter()
        .map(|own| Round {
            surface: own,
            along: SurfParamType::U,
            end: own.natural_uv_domain().u_interval().min(),
            corner_above: false,
        })
        .collect();
    let fill = fill_hole(&faces, &input, order, 1e-9);
    match &fill {
        Ok(f) => println!(
            "  setbacks {:.9}, {:.9}, {:.9}; fill status 0 (no refusal), interior degree {}",
            setbacks[0], setbacks[1], setbacks[2], f.degree
        ),
        Err(e) => println!(
            "  setbacks {:.9}, {:.9}, {:.9}; fill status {} ({})",
            setbacks[0],
            setbacks[1],
            setbacks[2],
            e.error.sm_status().unwrap_or(0),
            e.reason.unwrap_or("no refusal")
        ),
    }
    let fill = fill.unwrap();
    assert_eq!(fill.patches.len(), 6);
    let size = setbacks.iter().copied().fold(f64::MIN, f64::max);
    let planes: Vec<Surface> = (0..3)
        .map(|f| {
            Surface::from(
                Plane::create_canonical(&Axis2Placement::new(
                    Point3::new(0.0, 0.0, 0.0),
                    unit((f + 1) % 3),
                    unit((f + 2) % 3),
                ))
                .unwrap(),
            )
        })
        .collect();
    let patches: Vec<Surface> = fill.patches.iter().cloned().map(Surface::from).collect();
    let round_surfaces: Vec<Surface> = rounds.iter().cloned().map(Surface::from).collect();
    let at = |x: f64, y: f64| Point2::new(x, y);
    let origin = |_: f64| Point2::new(0.0, 0.0);
    for m in 0..3 {
        let sp = &fill.patches[m];
        let rnd = fill.round[m];
        let round = &rounds[rnd];
        let rev = fill.reversed[m];
        let end = input[rnd].end;
        let (fa, fb) = (&faces[fill.face[m]], &faces[fill.face[(m + 1) % 3]]);
        // the end row is the round's section, and the spring sides lie on their faces
        let (mut row, mut spring) = (0.0f64, 0.0f64);
        for j in 0..=8 {
            let t = f64::from(j) / 8.0;
            let p = sp.evaluate_point(at(0.0, t)).unwrap();
            let q = round
                .evaluate_point(at(end, if rev { 1.0 - t } else { t }))
                .unwrap();
            let pa = sp.evaluate_point(at(t, 0.0)).unwrap();
            let pb = sp.evaluate_point(at(t, 1.0)).unwrap();
            row = row.max(p.distance(q));
            spring = spring
                .max((pa - fa.point).dot(fa.normal).abs())
                .max((pb - fb.point).dot(fb.normal).abs());
        }
        println!(
            "  setback patch {m}: end row off its round's section by {row:.3e}, spring sides off their faces by {spring:.3e}"
        );
        assert!(row <= 1e-12 * size && spring <= 1e-12 * size);
        let sps = &patches[m];
        let side = |s, uv: Box<dyn Fn(f64) -> Point2>, inside| SeamSide { s, uv, inside };
        // the round's end section
        grade_seam(
            &k,
            &format!("round {rnd} / setback {m}"),
            &side(sps, Box::new(move |t| at(0.0, t)), Vec2::new(1.0, 0.0)),
            &side(
                &round_surfaces[rnd],
                Box::new(move |t| at(end, if rev { 1.0 - t } else { t })),
                Vec2::new(1.0, 0.0),
            ),
            size,
            order,
        );
        // the spring curves' halves
        grade_seam(
            &k,
            &format!("setback {m} / face {}", fill.face[m]),
            &side(sps, Box::new(move |t| at(t, 0.0)), Vec2::new(0.0, 1.0)),
            &side(&planes[fill.face[m]], Box::new(origin), Vec2::new(0.0, 0.0)),
            size,
            order,
        );
        grade_seam(
            &k,
            &format!("setback {m} / face {}", fill.face[(m + 1) % 3]),
            &side(sps, Box::new(move |t| at(t, 1.0)), Vec2::new(0.0, -1.0)),
            &side(
                &planes[fill.face[(m + 1) % 3]],
                Box::new(origin),
                Vec2::new(0.0, 0.0),
            ),
            size,
            order,
        );
        // the inner curve's halves, against interior parts m and m + 1
        let (im, in1) = (&patches[3 + m], &patches[3 + (m + 1) % 3]);
        grade_seam(
            &k,
            &format!("setback {m} / interior {m}"),
            &side(
                sps,
                Box::new(move |t| at(1.0, 0.5 * t)),
                Vec2::new(-1.0, 0.0),
            ),
            &side(
                im,
                Box::new(move |t| at(1.0, 1.0 - t)),
                Vec2::new(-1.0, 0.0),
            ),
            size,
            order,
        );
        grade_seam(
            &k,
            &format!("setback {m} / interior {}", (m + 1) % 3),
            &side(
                sps,
                Box::new(move |t| at(1.0, 0.5 + 0.5 * t)),
                Vec2::new(-1.0, 0.0),
            ),
            &side(in1, Box::new(move |t| at(t, 1.0)), Vec2::new(0.0, -1.0)),
            size,
            order,
        );
        // the spoke between interior parts m and m + 1
        grade_seam(
            &k,
            &format!("interior {m} / interior {}", (m + 1) % 3),
            &side(im, Box::new(move |t| at(t, 0.0)), Vec2::new(0.0, 1.0)),
            &side(in1, Box::new(move |t| at(0.0, t)), Vec2::new(1.0, 0.0)),
            size,
            order,
        );
    }
    let worst = containment(&fill, &faces, true).unwrap();
    println!("  stands past a face by at most {worst:.3e} (certified)");
    assert!(worst <= 1e-12 * size);
}

/// `setback_distances_box_corner`: the setbacks by the owner's rule, `max(1.5 r, least +
/// 0.25 r)`. Equal radii 2: least 2, setback 3. Radii 2, 3, 4 along x, y, z: least 4, 4, 3,
/// setbacks 4.5, 4.75 and 6. A face between three edges is refused, with its reason.
#[test]
fn distances_box_corner() {
    let faces = box_corner_faces();
    let d = distances(&faces, &box_corner_edges([2.0, 2.0, 2.0])).unwrap();
    assert_eq!(d.setbacks.len(), 3);
    for i in 0..3 {
        assert!((d.setbacks[i] - 3.0).abs() <= 1e-15 && (d.least[i] - 2.0).abs() <= 1e-15);
    }
    let d = distances(&faces, &box_corner_edges([2.0, 3.0, 4.0])).unwrap();
    let (want, want_least) = ([4.5, 4.75, 6.0], [4.0, 4.0, 3.0]);
    for i in 0..3 {
        println!(
            "  edge {i}: least {:.17}, setback {:.17}",
            d.least[i], d.setbacks[i]
        );
        assert!((d.setbacks[i] - want[i]).abs() <= 1e-15);
        assert!((d.least[i] - want_least[i]).abs() <= 1e-14);
    }
    // a face between three edges is no corner of the rule
    let mut bad = box_corner_edges([2.0, 2.0, 2.0]);
    bad[0].faces[0] = 0;
    let refusal = distances(&faces, &bad).unwrap_err();
    assert!(matches!(refusal.error, SolidError::InvalidInput { .. }));
    let why = refusal.reason.expect("a reason");
    println!("  refused: {why}");
}

/// Three rounds r 2 meeting at a box corner, filled to G1.
#[test]
fn box_corner_g1() {
    setback_box_corner([2.0, 2.0, 2.0], 1, 0.0, 1);
}

/// The same rounds eased into the order-2 section over 2 before their setbacks, filled to G2.
#[test]
fn box_corner_g2_eased() {
    setback_box_corner([2.0, 2.0, 2.0], 1, 2.0, 2);
}

/// Order-2 rounds r 2 (no ease), filled to G2.
#[test]
fn box_corner_g2_rounds() {
    setback_box_corner([2.0, 2.0, 2.0], 2, 0.0, 2);
}

/// Order-2 rounds r 2 as the G^n section generator builds them (degree 7 across, cubic
/// Hermite along), filled to G2.
#[test]
fn box_corner_g2_gn_section_rounds() {
    setback_box_corner([2.0, 2.0, 2.0], 3, 0.0, 2);
}

/// Rounds r 2, 3, 4 along x, y, z: no ball touches all three faces and rounds, so the corner
/// is set back and filled to G1.
#[test]
fn box_corner_unequal_g1() {
    setback_box_corner([2.0, 3.0, 4.0], 1, 0.0, 1);
}

/// A G2 fill of circular rounds not eased first is refused: they meet their faces only to G1.
#[test]
fn g2_fill_of_uneased_rounds_refused() {
    let faces = box_corner_faces();
    let rounds: Vec<BSplineSurface> = (0..3)
        .map(|a| box_corner_round(a, 2.0, 3.0, 10.0, 1))
        .collect();
    let input: Vec<Round<'_>> = rounds
        .iter()
        .map(|s| Round {
            surface: s,
            along: SurfParamType::U,
            end: 3.0,
            corner_above: false,
        })
        .collect();
    let refusal = fill_hole(&faces, &input, 2, 1e-9).unwrap_err();
    assert!(matches!(refusal.error, SolidError::InvalidInput { .. }));
    let why = refusal.reason.expect("a reason");
    println!("  refused: {why}");
}
