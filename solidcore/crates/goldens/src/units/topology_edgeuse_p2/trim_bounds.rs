//! Certified bounds on rebuilt UV trims: the 9 rows that report a trim rebuilt on a found
//! sphere's NURB (`topology.edgeuse_p2` `surf_sphere150_*` ×8,
//! `blend.fillet_standard_solver_p1` `gen0183`), and every trim on a found face of the 9
//! files with a found sphere or torus.
//!
//! In each row the trim the edge use reports is a line of constant `u` of the sphere's NURB
//! (the B-rep's import or the op rebuilt it on that NURB), or the case's own input made from
//! it. For each row the test bounds the distance between the face's surface along the edge
//! use's trim and the edge's 3D curve over its interval by
//! [`span_bound`]: the convex hull of the Bernstein coefficients of their difference, every
//! rounding included (`nurbs::certificate` module docs). No point is sampled. The bound holds
//! for any correspondence of the two parameters, so it bounds both directed Hausdorff
//! distances; the test asserts it is within the edge's tolerance:
//! 1. for the trim the op starts from (the B-rep as the case makes it);
//! 2. for the trim the row reports, after the op, tied to the row: its encoding is a run of
//!    the row's values, bit for bit. One row (`rb_cs2_2_0`, a check without adjustment)
//!    reports the check's input instead, the trim of 1. moved by the case's `(du, dv)` of
//!    the face's domain (`topology_edgeuse_p2::uv_curve`): off the surface by construction,
//!    so no tolerance applies. For it the test asserts the reported trim is exactly that
//!    move of the certified trim, so its moved values are the certified trim's move carried
//!    through the case's offset.
//!
//! The trims are of degree 1: their vertices are their control points, read, not evaluated,
//! and the surface along each segment is the line between them in (u, v), the form
//! [`span_bound`] takes.
//!
//! A second test bounds every trim on a found face of the 9 files, so that the rows reading
//! those trims rest on a certificate too: [`trims_on_found_faces_are_bounded`].

use std::collections::BTreeMap;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::certificate::{span_bound, Node, RailSpans};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, face, Brep, EdgeuseId};

use crate::case::{repo_root, CaseContext};
use crate::output::Output;
use crate::util::trim_curve::put_trim_curve;
use crate::{Case, UnitDiff};

/// The unit `id` and its cases, by name, as `goldens` runs them.
fn unit_cases(id: &str) -> (Box<dyn UnitDiff>, BTreeMap<String, Case>) {
    let u = crate::registry()
        .into_iter()
        .find(|u| u.id() == id)
        .unwrap_or_else(|| panic!("no unit {id}"));
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = u
        .cases(&ctx)
        .expect("the cases")
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect();
    (u, cases)
}

/// The rows: unit and case.
const ROWS: [(&str, &str); 9] = [
    ("topology.edgeuse_p2", "surf_sphere150_cs0_4_1"),
    ("topology.edgeuse_p2", "surf_sphere150_rb0"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_0_0"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_0_1"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_2_0"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_2_1"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_nt2_10"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_nt2_15"),
    ("blend.fillet_standard_solver_p1", "gen0183"),
];

/// The row that reports the check's input, the edge use's trim moved by the case's offset.
const MOVED_INPUT: [&str; 1] = ["surf_sphere150_rb_cs2_2_0"];

/// The case's B-rep and edge use, before the op or after it.
fn brep(unit_id: &str, case: &Case, after: bool) -> (Brep, EdgeuseId) {
    let r = match (unit_id, after) {
        ("topology.edgeuse_p2", false) => crate::units::topology_edgeuse_p2::scene(case),
        ("topology.edgeuse_p2", true) => crate::units::topology_edgeuse_p2::after_op(case),
        (_, after) => crate::units::blend_fillet_standard_solver_p1::eu_case(case, after),
    };
    r.unwrap_or_else(|e| panic!("{}: {e:?}", case.name))
        .unwrap_or_else(|| panic!("{}: no B-rep", case.name))
}

/// The edge use's trim.
fn trim(b: &Brep, eu: EdgeuseId) -> &Curve {
    eu1::uv_trim_curve(b, eu).unwrap().0.unwrap()
}

/// The vertices of a degree-1 trim with their parameters (a vertex of a clamped degree-1
/// B-spline is its control point, at the knot after it).
fn polyline(c: &Curve) -> Vec<(f64, Point2)> {
    let base = match c.kind() {
        CurveKind::BSpline(b) => std::borrow::Cow::Borrowed(b),
        CurveKind::Line(l) => l.base_with_nurb().unwrap(),
        _ => panic!("a trim neither a B-spline nor a line"),
    };
    let nurb = base.gw_nurb().unwrap();
    assert_eq!(nurb.degree(), 1, "a trim not of degree 1");
    assert!(!nurb.is_rational());
    let cpts = nurb.cpts().get(..=nurb.n()).unwrap();
    cpts.iter()
        .zip(nurb.knots().iter().skip(1))
        .map(|(q, &s)| (s, Point2::new(q.x, q.y)))
        .collect()
}

/// The certified bound of the distance between the face's surface along the edge use's trim
/// and the edge's 3D curve over its interval, and the edge's tolerance. The correspondence
/// is linear between the trim's vertices and the edge's interval, in either orientation;
/// each orientation is a correspondence, so each bound it gives holds, and the least is
/// taken (one that cannot be bounded gives none).
///
/// [`span_bound`] is given an infinite tolerance, so it never halves a piece: a halving's
/// new node is the rail's middle projected onto the surface, a point off the trim, and the
/// bound would then be of another path. Its only test, `bound > tol`, is false for every
/// bound against `f64::INFINITY`. Its remaining cuts, at the surface's patch lines and the
/// rail's knots, split a segment at a point of the segment (linear in `(u, v)` and `t`), so
/// the path bounded is the trim's own segments, each with a linear `t`.
fn trim_bound(k: &Kernel, b: &Brep, eu: EdgeuseId) -> (f64, f64) {
    let f = eu1::face(b, eu).unwrap().unwrap();
    let surf = face::p2::surface(b, f).unwrap().unwrap();
    let e = eu1::edge(b, eu).unwrap().unwrap();
    let tol = edge::tolerance(b, e).unwrap();
    let rail = edge::curve(b, e).unwrap().unwrap();
    let CurveKind::BSpline(rb) = rail.kind() else {
        panic!("an edge curve not a B-spline");
    };
    let i = edge::interval(b, e).unwrap();
    let p = rail.evaluate_point(k, i.min()).unwrap();
    let spans = RailSpans::about(rb.gw_nurb().unwrap(), p).unwrap();
    let vs = polyline(trim(b, eu));
    let (s0, s1) = (vs.first().unwrap().0, vs.last().unwrap().0);
    let mut best = f64::INFINITY;
    for reversed in [false, true] {
        // the trim's ends go to the interval's ends exactly (the formula's width can round)
        let t_at = |s: f64| {
            if s == s0 || s == s1 {
                return match (s == s0, reversed) {
                    (true, false) | (false, true) => i.min(),
                    _ => i.max(),
                };
            }
            let f = (s - s0) / (s1 - s0);
            let f = if reversed { 1.0 - f } else { f };
            i.min() + f * (i.max() - i.min())
        };
        let mut worst = 0.0_f64;
        for w in vs.windows(2) {
            let a = Node {
                t: t_at(w[0].0),
                uv: w[0].1,
            };
            let z = Node {
                t: t_at(w[1].0),
                uv: w[1].1,
            };
            let d =
                span_bound(k, surf, rail, &spans, p, a, z, f64::INFINITY).unwrap_or(f64::INFINITY);
            worst = worst.max(d);
        }
        best = best.min(worst);
    }
    (best, tol)
}

/// The encoding of the edge use's trim, asserted to be a run of the row's values, bit for bit.
fn tied(k: &Kernel, b: &Brep, eu: EdgeuseId, out: &Output) -> Vec<u64> {
    let mut enc = Vec::new();
    put_trim_curve(k, &mut enc, Some(trim(b, eu))).unwrap();
    let bits: Vec<u64> = enc.iter().map(|x| x.to_bits()).collect();
    let found = out.values.values().any(|v| {
        let v: Vec<u64> = v.iter().map(|x| x.to_bits()).collect();
        v.windows(bits.len()).any(|w| w == bits.as_slice())
    });
    assert!(found, "the trim {enc:?} is not in the row");
    bits
}

/// The trim after a check of the case's moved input is the trim before it moved by the
/// case's `(du, dv)` of the face's domain, as `topology_edgeuse_p2::uv_curve` makes it.
fn assert_moved_input(case: &Case, before: (&Brep, EdgeuseId), after: (&Brep, EdgeuseId)) {
    let uv = &case.inputs["uv"];
    assert_eq!(case.inputs["op"], "check_set");
    assert_eq!(uv["src"], "current");
    let (du, dv) = (uv["du"].as_f64().unwrap(), uv["dv"].as_f64().unwrap());
    assert!(du != 0.0 || dv != 0.0);
    let f = eu1::face(before.0, before.1).unwrap().unwrap();
    let d = before.0.face(f).unwrap().uv_domain;
    let (sx, sy) = (du * (d.max().x - d.min().x), dv * (d.max().y - d.min().y));
    // (parameter, point) pairs: the knots are the same, only the points move
    let moved: Vec<(f64, f64, f64)> = polyline(trim(before.0, before.1))
        .iter()
        .map(|&(s, q)| (s, q.x + sx, q.y + sy))
        .collect();
    let reported: Vec<(f64, f64, f64)> = polyline(trim(after.0, after.1))
        .iter()
        .map(|&(s, q)| (s, q.x, q.y))
        .collect();
    assert_eq!(
        moved, reported,
        "{}: not the trim moved by the case's offset",
        case.name
    );
}

/// The trim of each row, before the op and as the row reports it, is within the edge's
/// tolerance of its curve, by a certified bound; the trim the row reports is a run of the
/// row's values, or the certified trim moved by the case's own offset.
#[test]
fn rebuilt_trims_are_within_the_edge_tolerance() {
    let k = crate::runner_kernel();
    let mut report = Vec::new();
    for (unit_id, name) in ROWS {
        let (u, all) = unit_cases(unit_id);
        let case = &all[name];
        let out = u.run_rust(case).unwrap();
        let before = brep(unit_id, case, false);
        let after = brep(unit_id, case, true);
        tied(&k, &after.0, after.1, &out);
        let (d, tol) = trim_bound(&k, &before.0, before.1);
        report.push(format!("{name} before the op: {d:e} (tolerance {tol:e})"));
        assert!(d <= tol, "{name}: bound {d:e} above {tol:e}");
        if MOVED_INPUT.contains(&name) {
            assert_moved_input(case, (&before.0, before.1), (&after.0, after.1));
            continue;
        }
        let (d, tol) = trim_bound(&k, &after.0, after.1);
        report.push(format!("{name} reported: {d:e} (tolerance {tol:e})"));
        assert!(d <= tol, "{name} reported: bound {d:e} above {tol:e}");
    }
    eprintln!("{}", report.join("\n"));
}

// ---- every trim on a found face ---------------------------------------------------------------

/// The files with a found sphere or torus
/// (`solidcore/tests/wr493_class_proofs.rs`, `found_spheres_and_tori_keep_the_file_nurb_and_trims`).
const FILES: [&str; 9] = [
    "Fillet/box_w_slot.smb",
    "Fillet/sphere_diff_3cylinders.smb",
    "Fillet/sphere_plane.smb",
    "Solids/nurb_fillet.smb",
    "Solids/revolution_360.smb",
    "Solids/revolution_mold.smb",
    "Solids/sphere.smb",
    "sphere.smb",
    "surf_sphere150.smb",
];

/// Uniform samples for a lower bound of a trim's largest gap.
const SAMPLES: usize = 20_000;

/// One edge use's trim on a found face: its certified upper bound and its encoding.
struct Gap {
    /// The edge's tolerance.
    tol: f64,
    /// The certified upper bound of `sup |C(t) - S(W(t))|` ([`curve_uv_gap_bound`]).
    hi: f64,
    /// Whether the trim is one of the file's UV curves, bit for bit.
    file: bool,
    /// Computes a lower bound of the same supremum (below).
    lo: Box<dyn Fn() -> f64>,
}

/// A trim's encoding, bit for bit.
fn encoding(k: &Kernel, c: &Curve) -> Vec<u64> {
    let mut enc = Vec::new();
    put_trim_curve(k, &mut enc, Some(c)).unwrap();
    enc.iter().map(|x| x.to_bits()).collect()
}

/// The file read as the runners read it (`own_reader::read_file`), with every trim on a
/// found face (a sphere or torus) fetched or made (`get_or_create_uv_trim_curve`), and each
/// such edge use's [`Gap`].
fn gaps(name: &str) -> BTreeMap<u32, Gap> {
    use solidcore::query::curve_surface_gap::{curve_uv_gap_bound, curve_uv_gap_lower_bound};
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep_data::BrepData;
    use solidcore::topology::database_io::FileType;
    use solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(crate::fixtures::UPSTREAM_TEST_FILES)
        .join(name);
    let mut k = crate::runner_kernel();
    let data =
        BrepData::read_from_file(&k, std::fs::read(&path).unwrap(), FileType::Ascii).unwrap();
    let file: Vec<Vec<u64>> = data
        .uv_curves
        .iter()
        .map(|c| encoding(&k, &c.object))
        .collect();
    let mut b = crate::own_reader::read_file(&mut k, path.to_str().unwrap())
        .unwrap()
        .unwrap();
    let found = |b: &Brep, eu: EdgeuseId| {
        eu1::face(b, eu).unwrap().is_some_and(|f| {
            face::p2::surface(b, f)
                .unwrap()
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)))
        })
    };
    let ids: Vec<EdgeuseId> = b
        .edgeuses()
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| found(&b, id))
        .collect();
    let mut out = BTreeMap::new();
    for id in ids {
        get_or_create_uv_trim_curve(&k, &mut b, id, false, false, true).unwrap();
        let (Some(t), owner) = eu1::uv_trim_curve(&b, id).unwrap() else {
            panic!("{name} {id:?}: no trim");
        };
        let owner = owner.unwrap_or(id);
        let f = eu1::face(&b, owner).unwrap().unwrap();
        let s = face::p2::surface(&b, f).unwrap().unwrap().clone();
        let w = match t.kind() {
            CurveKind::BSpline(x) => x.clone_exact(),
            CurveKind::Line(l) => l.base_with_nurb().unwrap().into_owned(),
            _ => panic!("{name} {id:?}: a trim neither a B-spline nor a line"),
        };
        let e = eu1::edge(&b, id).unwrap().unwrap();
        let tol = edge::tolerance(&b, e).unwrap();
        let i = edge::interval(&b, e).unwrap();
        let rail = edge::curve(&b, e).unwrap().unwrap().clone();
        let CurveKind::BSpline(c) = rail.kind() else {
            panic!("{name} {id:?}: an edge curve not a B-spline");
        };
        let range = [i.min(), i.max()];
        let hi = curve_uv_gap_bound(&k, c, &w, &s, range, tol / 100.0)
            .unwrap()
            .unwrap_or_else(|| panic!("{name} {id:?}: the gap is not enclosed"));
        let bits = encoding(&k, t);
        let lo_k = crate::runner_kernel();
        // A lower bound: the largest of the certified lower bounds of the gap at uniform
        // parameters ([`curve_uv_gap_lower_bound`]: interval evaluation, every rounding
        // included): the supremum is at least the gap at any one parameter. A parameter whose
        // gap is not enclosed (its UV box not inside the net) is skipped.
        let lo = Box::new(move || {
            let CurveKind::BSpline(c) = rail.kind() else {
                unreachable!("checked above");
            };
            (0..=SAMPLES)
                .filter_map(|j| {
                    let t = range[0] + (range[1] - range[0]) * j as f64 / SAMPLES as f64;
                    curve_uv_gap_lower_bound(&lo_k, c, &w, &s, t).unwrap()
                })
                .fold(0.0_f64, f64::max)
        });
        out.insert(
            id.index(),
            Gap {
                tol,
                hi,
                file: file.contains(&bits),
                lo,
            },
        );
    }
    out
}

/// Every edge use's trim on a found face of the 9 files, by a certified upper bound of how
/// far the face's surface along the trim strays from the edge's curve at the same parameter
/// ([`curve_uv_gap_bound`]: interval Taylor models with bisection, every rounding
/// included). A trim within its edge's tolerance is PROVEN. The others are pinned below,
/// each proved out of tolerance by a certified lower bound above the edge's tolerance (the
/// largest of the lower bounds of the gap at [`SAMPLES`] parameters,
/// [`curve_uv_gap_lower_bound`]: a supremum is at least its value at any parameter), and
/// each is one of the file's UV curves, bit for bit: the file's own data, kept as read.
///
/// The metric is the same-parameter gap: the surface at the trim's point for `t` against
/// the edge's curve at the same `t`. It is stricter than what an edge tolerance bounds,
/// the distance of the trim's image from the edge's curve, which the read's kept-trim cover
/// (tangency-labs/solidcraft#19295, `topology::brep::p2::cover_kept_uv_trim_curves`)
/// encloses for every kept trim, raising the edge's tolerance to it where it is farther.
/// "Out of tolerance" here is by the same-parameter metric only: a trim pinned out below
/// can lie within its edge's tolerance as a curve (`LShape_I.smb`'s trims of edge uses 242
/// and 246 lie on their edges, 1.1e-5 apart at equal parameters); its geometric distance is
/// the cover's, not re-derived here.
///
/// Counts are of edge uses: mated uses share one trim (`uv_trim_curve` returns the owner's),
/// so each pair below (0/1, 8/9, ...) is one trim seen from both of its uses.
#[test]
fn trims_on_found_faces_are_bounded() {
    // (file, edge use): the file trims out of tolerance
    let want: &[(&str, u32)] = &[
        ("Fillet/sphere_diff_3cylinders.smb", 0),
        ("Fillet/sphere_diff_3cylinders.smb", 1),
        ("Fillet/sphere_diff_3cylinders.smb", 8),
        ("Fillet/sphere_diff_3cylinders.smb", 9),
        ("Fillet/sphere_diff_3cylinders.smb", 16),
        ("Fillet/sphere_diff_3cylinders.smb", 17),
        ("Fillet/sphere_diff_3cylinders.smb", 18),
        ("Fillet/sphere_diff_3cylinders.smb", 19),
        ("Fillet/sphere_diff_3cylinders.smb", 20),
        ("Fillet/sphere_diff_3cylinders.smb", 21),
        ("Fillet/sphere_diff_3cylinders.smb", 28),
        ("Fillet/sphere_diff_3cylinders.smb", 29),
        ("Fillet/sphere_diff_3cylinders.smb", 30),
        ("Fillet/sphere_diff_3cylinders.smb", 31),
        ("Fillet/sphere_diff_3cylinders.smb", 38),
        ("Fillet/sphere_diff_3cylinders.smb", 39),
        ("Fillet/sphere_plane.smb", 8),
        ("Fillet/sphere_plane.smb", 9),
        ("Solids/nurb_fillet.smb", 60),
        ("Solids/nurb_fillet.smb", 61),
        ("Solids/nurb_fillet.smb", 66),
        ("Solids/nurb_fillet.smb", 67),
        ("Solids/nurb_fillet.smb", 70),
        ("Solids/nurb_fillet.smb", 71),
    ];
    let mut got = Vec::new();
    let mut report = Vec::new();
    let mut proven = 0;
    for name in FILES {
        for (eu, g) in &gaps(name) {
            if g.hi <= g.tol {
                proven += 1;
                continue;
            }
            let lo = (g.lo)();
            assert!(lo > g.tol, "{name} {eu}: out only by its bound");
            assert!(g.file, "{name} {eu}: out of tolerance and not a file trim");
            report.push(format!(
                "{name} eu {eu}: [{lo:e}, {:e}] tol {:e}",
                g.hi, g.tol
            ));
            got.push((name, *eu));
        }
    }
    eprintln!(
        "{proven} edge uses' trims within tolerance\n{}",
        report.join("\n")
    );
    assert_eq!(got, want);
}
