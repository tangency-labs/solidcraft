//! Regression test for WR-407 (tangency-labs/solidcore#434, the fork's
//! tangency-labs/smlib@1b6f5fa, SolidCraft kernel case sweeps/844): two walls 12 thick
//! and 60 tall swept along half circles (wall A r 44..56 over 0..180 degrees, wall B
//! r 52..64 over 90..270) overlap over a quarter turn and share the planes z = 0 and
//! z = 60, and are joined.
//!
//! With the sweep's frame as upstream built it, wall A's flat top was a B-spline 2.8e-5
//! off its plane, so it was not planar and its search domain reached past the fold of its
//! continuation; upstream's `CoverCoincidentSurface` merged wall B's top onto that folded
//! continuation and the join lost 5573 mm3. WR-407's frame on the path plane's normal is
//! the default now, and without `cover_refuses_folded_continuation` the join is still
//! wrong: the merged cover is a face of wall A's top B-spline whose trims run about 1.7 in
//! v past the spline's knot domain. With `cover_refuses_folded_continuation` the cover across the
//! fold is refused and the join has the closed-form volume
//! `V_A + V_B - overlap = 60 (pi/2)(56^2 - 44^2) + 60 (pi/2)(64^2 - 52^2) - 60 (pi/4)(56^2 - 52^2)`.
//! The walls' radial faces are the sweep's approximation of the arcs, so the volume is
//! held to 1 mm3 (the SolidCraft case's tolerance).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep::create_curve_sweep;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

/// The profile (u across the wall, v up), turned into the plane `turn` (axis, degrees)
/// maps the XY plane to.
fn profile(k: &mut Kernel, turn: (Vec3, f64)) -> Vec<Curve> {
    let pts = [
        [0.0, 0.0],
        [6.0, 0.0],
        [6.0, 60.0],
        [-6.0, 60.0],
        [-6.0, 0.0],
    ];
    let at = |[u, v]: [f64; 2]| Point3::new(u, v, 0.0);
    let mut out: Vec<Curve> = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| Curve::from(create_line_segment(at(*a), at(*b)).unwrap()))
        .collect();
    for c in &mut out {
        api_rotate(
            k,
            Some(TransformObject::Curve(c)),
            Point3::ZERO,
            turn.0,
            turn.1,
        )
        .unwrap();
    }
    out
}

/// The profile in `turn`'s plane swept along the XY arc of `radius` from `start` to
/// `end` degrees, the profile turning with the path's frame (`SMLIB_SWEEP_FOLLOW`).
fn wall(k: &mut Kernel, turn: (Vec3, f64), radius: f64, start: f64, end: f64) -> Brep {
    let curves = profile(k, turn);
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let path = Curve::from(create_arc(Point3::ZERO, radius, start, end).unwrap());
    let options = SweepOptions {
        cap_ends: true,
        translational_sweep: false,
        move_profile_center_to_path: false,
        orient_profile_perpendicular_to_path: false,
        move_to_profile: false,
        profile_at_path_start: false,
        up: None,
    };
    create_curve_sweep(k, &refs, Some(&path), None, None, Some(&options))
        .unwrap()
        .brep
}

/// The two walls joined, with `cover_refuses_folded_continuation` (WR-407's cover half) `fixed`.
fn joined(fixed: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.cover_refuses_folded_continuation = fixed;
    // XZ is XY turned 90 degrees about X; YZ is XY turned 120 degrees about (1, 1, 1).
    let a = wall(&mut k, (Vec3::X, 90.0), 50.0, 0.0, 180.0);
    let b = wall(&mut k, (Vec3::new(1.0, 1.0, 1.0), 120.0), 58.0, 90.0, 270.0);
    let joined = boolean_union(&mut k, a, b).unwrap();
    (k, joined)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// `V_A + V_B - overlap`.
fn closed_form() -> f64 {
    let ring = |r0: f64, r1: f64| r1 * r1 - r0 * r0;
    60.0 * (PI / 2.0) * ring(44.0, 56.0) + 60.0 * (PI / 2.0) * ring(52.0, 64.0)
        - 60.0 * (PI / 4.0) * ring(52.0, 56.0)
}

#[test]
fn wr_407_walls_join_to_the_closed_form_with_the_fix() {
    let (k, mut b) = joined(true);
    assert!(brep_is_manifold_solid(&b).unwrap());
    let v = volume(&k, &mut b);
    assert!(
        (v - closed_form()).abs() < 1.0,
        "{v} against {}",
        closed_form()
    );
}

/// How far, in uv, the farthest sampled point of a face's UV trim lies outside its surface's
/// natural (knot) domain, over all faces: a sampled point past the domain proves the trim
/// leaves it by at least that much (a lower bound, PROVEN by the sample itself).
fn farthest_trim_past_domain(k: &Kernel, b: &Brep) -> f64 {
    use solidcore::topology::brep::p1::faces;
    use solidcore::topology::edgeuse::p1::{faceuse_loopuses, loopuse_edgeuses, uv_trim_curve};
    let mut worst = 0.0_f64;
    for f in faces(b, None).unwrap() {
        let face = b.face(f).unwrap();
        let dom = face
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_with_nurb(k)
            .unwrap();
        let up = face.faceuse.unwrap();
        for lu in faceuse_loopuses(b, up, false, None).unwrap() {
            for eu in loopuse_edgeuses(b, lu).unwrap() {
                let Some(t) = uv_trim_curve(b, eu).unwrap().0 else {
                    continue;
                };
                let iv = t.natural_interval();
                for i in 0..=256 {
                    let tau = iv.min() + (iv.max() - iv.min()) * f64::from(i) / 256.0;
                    let p = t.evaluate_point(k, tau).unwrap();
                    let out = (dom.u_min() - p.x)
                        .max(p.x - dom.u_max())
                        .max(dom.v_min() - p.y)
                        .max(p.y - dom.v_max());
                    worst = worst.max(out);
                }
            }
        }
    }
    worst
}

/// Every face's UV trims lie in its surface's natural (knot) domain widened by `margin`,
/// checked on the trims' control points (PROVEN for B-spline trims: each control point is
/// read in homogeneous form `(x w, y w, z w, w)`, its weight must be positive, and its
/// point `(x w / w, y w / w)` inside the widened domain; with positive weights a B-spline
/// lies in the convex hull of its control points, and the widened domain is convex). A
/// trim with no B-spline form, a weight that is not positive or a NaN fails. `margin`
/// covers the one division's rounding. `Err` names the first trim that fails.
fn trims_inside_domain(k: &Kernel, b: &Brep, margin: f64) -> Result<(), String> {
    use solidcore::blend::fillet_section::certificate::NurbsCurve;
    use solidcore::topology::brep::p1::faces;
    use solidcore::topology::edgeuse::p1::{faceuse_loopuses, loopuse_edgeuses, uv_trim_curve};
    for f in faces(b, None).unwrap() {
        let face = b.face(f).unwrap();
        let dom = face
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_with_nurb(k)
            .unwrap();
        let (u0, u1) = (dom.u_min() - margin, dom.u_max() + margin);
        let (v0, v1) = (dom.v_min() - margin, dom.v_max() + margin);
        let up = face.faceuse.unwrap();
        for lu in faceuse_loopuses(b, up, false, None).unwrap() {
            for eu in loopuse_edgeuses(b, lu).unwrap() {
                let t = uv_trim_curve(b, eu)
                    .unwrap()
                    .0
                    .unwrap_or_else(|| panic!("edge use {eu:?} of face {f:?} has no UV trim"));
                let Some(bs) = t.b_spline_base() else {
                    return Err(format!("face {f:?}: a trim with no B-spline form"));
                };
                let cn = NurbsCurve::of(bs).unwrap();
                for p in cn.pw.chunks(4) {
                    let &[xw, yw, _, w] = p else {
                        return Err(format!("face {f:?}: a control point not homogeneous"));
                    };
                    // written so a NaN fails
                    if w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                        return Err(format!("face {f:?}: a control point's weight {w}"));
                    }
                    let (x, y) = (xw / w, yw / w);
                    if !(x >= u0 && x <= u1 && y >= v0 && y <= v1) {
                        return Err(format!(
                            "face {f:?}: control point ({x}, {y}) outside [{u0}, {u1}] x \
                             [{v0}, {v1}]"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The body's tolerance volume: `sum A_f tol_f` over faces (area times tolerance) `+ sum pi
/// tol_e^2 L_e` over edges (the tubes) `+ sum 4/3 pi tol_v^3` over vertices (the balls), from
/// the body's own stored tolerances. The region a body bounds is defined only up to it: a
/// face within its tolerance of the solid's boundary moves the volume by at most its area
/// times that tolerance (to first order), and near an edge or vertex the closure lies in the
/// edge's tube or the vertex's ball (ARGUED: the strips' widths are measured against the
/// tolerances in `support/volume_leak.rs`, not certified).
fn tolerance_volume(k: &Kernel, b: &mut Brep) -> f64 {
    use solidcore::mass::integrator::properties::{edge_length, face_area};
    use solidcore::topology::brep::p1::{edges, faces, vertices};
    use solidcore::topology::{edge, face, vertex};
    let mut slack = 0.0;
    for f in faces(b, None).unwrap() {
        let tol = face::p2::tolerance(b, f).unwrap();
        slack += face_area(k, b, f, 1e-8).unwrap() * tol;
    }
    for e in edges(b, None).unwrap() {
        let tol = edge::tolerance(b, e).unwrap();
        slack += PI * tol * tol * edge_length(k, b, e, 1e-9).unwrap();
    }
    for v in vertices(b, None).unwrap() {
        let tol = vertex::tolerance(b, v).unwrap();
        slack += 4.0 / 3.0 * PI * tol * tol * tol;
    }
    slack
}

/// Without the fold rule the cover merges onto wall A's folded continuation: wall B's top
/// becomes a face of wall A's top B-spline whose trims run about 1.7 in v past the spline's
/// knot domain (1.69 here), where every consumer that clamps to the domain (upstream's
/// `EvaluateSimple`) sees another surface than the face's edges lie on, and one of its
/// edges carries a tolerance of 2.3. With the fix every trim lies inside its domain (to
/// 1e-9, on its control points).
///
/// This used to be measured by the volume (7744 mm3 too large; 5573 mm3 short with
/// upstream's sweep frame, 14596 mm3 with a drop piece stretched onto each trim's edge).
/// Those figures were the clamped integration of that face. `brep_volume` now integrates
/// it on the B-spline continued past its domain (#16735), where it lies on its edges, so
/// the two joins bound the same solid up to their tolerances: their volumes agree within
/// the two bodies' tolerance volumes ([`tolerance_volume`]).
#[test]
fn wr_407_default_join_loses_the_folded_cover() {
    // (the volume makes the faces' missing UV trims first)
    let (k, mut b) = joined(false);
    let v = volume(&k, &mut b);
    let slack = tolerance_volume(&k, &mut b);
    // a sampled point past the domain is a proven lower bound on how far the trim leaves it
    let past = farthest_trim_past_domain(&k, &b);
    assert!(
        past > 1.5,
        "the cover's trims leave the domain by only {past}"
    );
    let (kf, mut bf) = joined(true);
    let vf = volume(&kf, &mut bf);
    let slack_f = tolerance_volume(&kf, &mut bf);
    eprintln!(
        "wr_407: without the fix {v} (tolerance volume {slack:.3e}), with it {vf} \
         ({slack_f:.3e}), closed form {}",
        closed_form()
    );
    if let Err(e) = trims_inside_domain(&kf, &bf, 1e-9) {
        panic!("with the fix a trim leaves its domain: {e}");
    }
    assert!(
        (v - vf).abs() <= slack + slack_f,
        "{v} without the fix against {vf} with it: {:.3e} past the tolerance volumes \
         {slack:.3e} + {slack_f:.3e}",
        (v - vf).abs()
    );
}
