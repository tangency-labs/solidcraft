//! #16735: checks that a volume moved by `brep_volume`'s closure of tolerant edges is the
//! old integrator's leak, not a change of shape.
//!
//! The old integrator took each face over its own trims. Those trims' images lie a gap g
//! from the edge curve. The field `(0, 0, z - o_z)` then leaked through the gaps by about
//! sum over edge uses of `g L max|z - o_z|` ([`Leak::old_bound`], g sampled: MEASURED).
//! The new integrator closes every face onto the edge curves with ruled strips and corner
//! fans, so its volume does not depend on the origin ([`Leak::origin_spread`]).
//!
//! The gate is [`moved_by_the_old_leak`]: the value reproduces, the move is within the old
//! leak bound, and the volume is origin-free. The tube figures ([`Leak::new_bound`],
//! [`Leak::new_bound_measured`]) are informational: the solid is defined only up to the
//! tubes of width w about its edges and the balls about its vertices, sum pi w^2 L + sum
//! 4/3 pi r^3 (ARGUED and diagnostic only: w and r are sampled, not certified, so it is no
//! bound), with w each strip's width measured along the closure's own correspondence and r
//! each corner fan's reach from its vertex. (#15946's outward-rounded cap bound, `L^2 / 4`
//! times `max |f|` over the openings, is a proved one; the strips here leave no opening to
//! cap, so none is computed.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    dead_code
)] // test helpers; each caller uses a subset

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{
    brep_properties, edge_length, MassPropertyBehavior, PropertyAttributes,
};
use solidcore::math::{Point2, Point3, PI};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, loopuse, vertex};
use solidcore::{Brep, Kernel};

/// A B-rep read from SMB bytes (`ascii` for the text form).
pub fn read_smb(k: &mut Kernel, bytes: &[u8], ascii: bool) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), ascii, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The B-rep's volume about `origin` to `accuracy`.
pub fn volume_about(k: &Kernel, b: &mut Brep, origin: Point3, accuracy: f64) -> f64 {
    properties_about(k, b, origin, accuracy).volume
}

/// The B-rep's properties about `origin` to `accuracy`.
pub fn properties_about(
    k: &Kernel,
    b: &mut Brep,
    origin: Point3,
    accuracy: f64,
) -> solidcore::mass::integrator::properties::PreciseProperties {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_properties(
        k,
        b,
        accuracy,
        origin,
        MassPropertyBehavior::default(),
        &attrs,
    )
    .unwrap()
}

/// The face, edge and vertex counts.
pub fn counts(b: &Brep) -> [usize; 3] {
    [
        p1::faces(b, None).unwrap().len(),
        p1::edges(b, None).unwrap().len(),
        p1::vertices(b, None).unwrap().len(),
    ]
}

/// What [`leak`] measures.
#[derive(Debug, Clone)]
pub struct Leak {
    /// The volume about the bounding box's middle (`brep_volume`'s origin).
    pub volume: f64,
    /// The largest difference of the volume about two other origins, 100 above and
    /// (-37, 20, -55) off, from it: the integrand is `z n_z`, so each shift moves z.
    pub origin_spread: f64,
    /// What the quadrature alone may leave of that spread: 1e-9 of the volume plus the
    /// area times the origins' distance (each face's integrand grows with the distance;
    /// MEASURED: closed bodies stay 10x and more below it, leaking ones 10x and more above).
    pub origin_allowance: f64,
    /// sum over edge uses of `g L max|z - o_z|`, g the largest gap of the trim's image from
    /// its edge at the closure's 65 grid points (MEASURED). A gap peaking between the grid
    /// points, and the holes at the corners where trims do not meet, are not counted: both
    /// would only raise the true leak, so the bound is low and the gate it makes strict.
    pub old_bound: f64,
    /// sum pi tol^2 L over edges + sum 4/3 pi tol^3 over vertices: the tube bound when every
    /// strip and fan lies within its stored tolerance (ARGUED; see
    /// [`Leak::worst_gap_over_tolerance`] and [`Leak::worst_fan_over_tolerance`]).
    pub new_bound: f64,
    /// [`Leak::new_bound`] with each edge's strip width w and each vertex's fan reach r
    /// where those are larger than the tolerance: sum pi max(w, tol)^2 L + sum 4/3 pi
    /// max(r, tol)^3 (MEASURED inputs, ARGUED bound).
    pub new_bound_measured: f64,
    /// The widest strip over its edge's stored tolerance. The width is
    /// `|P(tau) - E(t(tau))|` along the closure's correspondence t (its 64-point grid of
    /// nearest points, linear between), sampled at `samples` points per grid segment and
    /// raised by [`WIDTH_MARGIN`] for a peak between samples.
    pub worst_gap_over_tolerance: f64,
    /// The farthest corner-fan point (trim end image or edge end) from its vertex, over the
    /// vertex's stored tolerance.
    pub worst_fan_over_tolerance: f64,
    /// The edges with a strip wider than their tolerance, with that ratio.
    pub strayed: Vec<(solidcore::topology::EdgeId, f64)>,
}

/// The closure's grid of nearest points per strip (`properties::CLOSURE_GRID`).
const GRID: u32 = 64;

/// The margin on a strip's sampled width for a peak between samples. ARGUED, not derived:
/// between two samples a ruled strip's width is smooth, so at 8 samples per grid segment a
/// peak between them is expected to stay well under 10% above the larger of them; no bound
/// is proven. It feeds only the informational tube figures, never the gate.
pub const WIDTH_MARGIN: f64 = 1.1;

/// The nearest point's parameter on `curve` over `[t0, t1]` to `p` by Gauss-Newton from
/// `guess`: `properties::nearest_on_edge`, step for step, so the grid is the closure's.
fn nearest_on_edge(
    k: &Kernel,
    curve: &solidcore::curve::curve::Curve,
    t0: f64,
    t1: f64,
    p: Point3,
    guess: f64,
) -> Option<f64> {
    let mut t = guess.max(t0).min(t1);
    for _ in 0..64 {
        let e = curve.evaluate(k, t, 1, true, false).unwrap();
        let (Some(&ep), Some(&d1)) = (e.first(), e.get(1)) else {
            panic!("the edge curve's evaluation returned no derivative");
        };
        let df = d1.dot(d1);
        let f = (ep - p).dot(d1);
        if !(df > 0.0 && df.is_finite() && f.is_finite()) {
            return None;
        }
        let next = (t - f / df).max(t0).min(t1);
        let step = (next - t).abs();
        t = next;
        if step <= 1e-15 * (1.0 + t.abs()) {
            break;
        }
    }
    Some(t)
}

/// The checks' quantities for `b` at `accuracy`, each strip's width sampled at `samples`
/// points per grid segment.
pub fn leak(k: &Kernel, b: &mut Brep, accuracy: f64, samples: u32) -> Leak {
    let o = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())
        .unwrap()
        .mid();
    let here = properties_about(k, b, o, accuracy);
    let volume = here.volume;
    let origin_allowance = 1e-9 * (volume.abs() + here.area * 100.0);
    let mut origin_spread = 0.0_f64;
    for d in [
        Point3::new(0.0, 0.0, 100.0),
        Point3::new(-37.0, 20.0, -55.0),
    ] {
        origin_spread = origin_spread.max((volume_about(k, b, o + d, accuracy) - volume).abs());
    }
    let mut new_bound = 0.0;
    for e in p1::edges(b, None).unwrap() {
        let t = edge::tolerance(b, e).unwrap();
        new_bound += PI * t * t * edge_length(k, b, e, 1e-9).unwrap();
    }
    for v in p1::vertices(b, None).unwrap() {
        let t = vertex::tolerance(b, v).unwrap();
        new_bound += 4.0 / 3.0 * PI * t * t * t;
    }
    let mut old_bound = 0.0;
    let mut widths: std::collections::BTreeMap<solidcore::topology::EdgeId, f64> =
        std::collections::BTreeMap::new();
    let mut reach: std::collections::BTreeMap<solidcore::topology::VertexId, f64> =
        std::collections::BTreeMap::new();
    for f in p1::faces(b, None).unwrap() {
        let Some(s) = b.face(f).unwrap().surface.clone() else {
            continue;
        };
        let Some(fu) = solidcore::topology::face::p2::upward_faceuse(b, f).unwrap() else {
            continue;
        };
        let img = |trim: &solidcore::curve::curve::Curve, tau: f64| -> Point3 {
            let uv = trim.evaluate_point(k, tau).unwrap();
            s.evaluate(k, Point2::new(uv.x, uv.y), 0, 0, EvaluateFlags::default())
                .unwrap()[0][0]
        };
        for lu in eu1::faceuse_loopuses(b, fu, false, None).unwrap() {
            for eu in eu1::loopuse_edgeuses(b, lu).unwrap() {
                let e = eu1::edge(b, eu).unwrap().unwrap();
                let Some(trim) = eu1::uv_trim_curve(b, eu).unwrap().0 else {
                    continue;
                };
                let ti = trim.natural_interval();
                let (c0, c1) = (ti.min(), ti.max());
                // the corner fans: the trim's and the edge's ends against their vertices
                // (the strips run along the edge: c0 and e0 at its start)
                let ends = [
                    (edge::start_vertex(b, e).unwrap(), c0, true),
                    (edge::end_vertex(b, e).unwrap(), c1, false),
                ];
                let Some(c) = edge::curve(b, e).unwrap() else {
                    for (v, tau, _) in ends {
                        let Some(v) = v else { continue };
                        let at = b.vertex(v).unwrap().point;
                        let r = reach.entry(v).or_insert(0.0);
                        *r = r.max((img(trim, tau) - at).length());
                    }
                    continue;
                };
                let ivl = b.edge(e).unwrap().interval;
                let (e0, e1) = (ivl.min(), ivl.max());
                for (v, tau, start) in ends {
                    let Some(v) = v else { continue };
                    let at = b.vertex(v).unwrap().point;
                    let q = c.evaluate_point(k, if start { e0 } else { e1 }).unwrap();
                    let r = reach.entry(v).or_insert(0.0);
                    *r = r.max((img(trim, tau) - at).length()).max((q - at).length());
                }
                // the closure's correspondence: the ends to the ends, nearest points on
                // its grid marched from one to the next, linear between
                let mut table = vec![(c0, e0)];
                let mut last = e0;
                for i in 1..GRID {
                    let tau = c0 + (c1 - c0) * f64::from(i) / f64::from(GRID);
                    let p = img(trim, tau);
                    let share = e0 + (e1 - e0) * f64::from(i) / f64::from(GRID);
                    last = nearest_on_edge(k, c, e0, e1, p, last)
                        .or_else(|| nearest_on_edge(k, c, e0, e1, p, share))
                        .expect("the closure refuses a strip singular from both seeds");
                    table.push((tau, last));
                }
                table.push((c1, e1));
                let (mut g, mut w, mut mz) = (0.0_f64, 0.0_f64, 0.0_f64);
                for &(tau, t) in &table {
                    let q = c.evaluate_point(k, t).unwrap();
                    g = g.max((img(trim, tau) - q).length());
                    mz = mz.max((q.z - o.z).abs());
                }
                for seg in table.windows(2) {
                    let [(ta, sa), (tb, sb)] = [seg[0], seg[1]];
                    for j in 0..=samples {
                        let l = f64::from(j) / f64::from(samples);
                        let q = c.evaluate_point(k, sa + (sb - sa) * l).unwrap();
                        w = w.max((img(trim, ta + (tb - ta) * l) - q).length());
                    }
                }
                old_bound += g * edge_length(k, b, e, 1e-9).unwrap() * mz;
                let width = widths.entry(e).or_insert(0.0);
                *width = width.max(WIDTH_MARGIN * w);
            }
        }
    }
    let (mut worst, mut worst_fan) = (0.0_f64, 0.0_f64);
    let mut strayed: Vec<(solidcore::topology::EdgeId, f64)> = Vec::new();
    let mut new_bound_measured = 0.0;
    for e in p1::edges(b, None).unwrap() {
        let tol = edge::tolerance(b, e).unwrap();
        let w = widths.get(&e).copied().unwrap_or(0.0);
        worst = worst.max(w / tol);
        if w > tol {
            strayed.push((e, w / tol));
        }
        let t = tol.max(w);
        new_bound_measured += PI * t * t * edge_length(k, b, e, 1e-9).unwrap();
    }
    for v in p1::vertices(b, None).unwrap() {
        let tol = vertex::tolerance(b, v).unwrap();
        let r = reach.get(&v).copied().unwrap_or(0.0);
        worst_fan = worst_fan.max(r / tol);
        let t = tol.max(r);
        new_bound_measured += 4.0 / 3.0 * PI * t * t * t;
    }
    Leak {
        volume,
        origin_spread,
        origin_allowance,
        new_bound_measured,
        old_bound,
        new_bound,
        worst_gap_over_tolerance: worst,
        worst_fan_over_tolerance: worst_fan,
        strayed,
    }
}

/// The checks on one volume that moved from `old` to `new` (`brep_volume` at `accuracy`).
/// The gate: it reproduces (`brep_volume` gives `new` to 1e-9); the move is within
/// [`Leak::old_bound`] plus twice the quadrature's own share at `accuracy` (its difference
/// from the volume at 1e-10; MEASURED); and the volume does not depend on the origin beyond
/// [`Leak::origin_allowance`]. Informational, in the figures only: the tube bounds, the
/// widest strip and farthest fan over their tolerances, and whether the measured tube bound
/// is below the old leak bound or the move lies inside the tube. `Ok` with the figures, or
/// `Err` with them and the failed checks.
pub fn moved_by_the_old_leak(
    k: &Kernel,
    b: &mut Brep,
    old: f64,
    new: f64,
    accuracy: f64,
) -> Result<String, String> {
    let store = AttributeStore::new();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let at_accuracy = solidcore::mass::integrator::properties::brep_volume(k, b, accuracy, &attrs)
        .map_err(|e| format!("volume {new} (was {old}): brep_volume at {accuracy:e}: {e}"))?;
    let l = leak(k, b, 1e-10, 8);
    let quadrature = 2.0 * (at_accuracy - l.volume).abs();
    let moved = (new - old).abs();
    // informational: the measured tube is the tighter bound, or the move lies inside it
    let tube =
        l.new_bound_measured <= l.old_bound || moved <= l.new_bound_measured || moved <= quadrature;
    let line = format!(
        "volume {new} (was {old}, moved {moved:.3e}): old leak bound {:.3e} + quadrature \
         {quadrature:.3e}; origin spread {:.3e} (allowance {:.3e}); tube {:.3e} (measured \
         {:.3e}, tighter or holding the move {tube}); widest strip / tol {:.3}, farthest fan \
         / tol {:.3}",
        l.old_bound,
        l.origin_spread,
        l.origin_allowance,
        l.new_bound,
        l.new_bound_measured,
        l.worst_gap_over_tolerance,
        l.worst_fan_over_tolerance
    );
    let reproduces = (at_accuracy - new).abs() <= 1e-9 * new.abs();
    let within_old_leak = moved <= l.old_bound + quadrature;
    let origin_free = l.origin_spread <= l.origin_allowance;
    if reproduces && within_old_leak && origin_free {
        Ok(line)
    } else {
        Err(format!(
            "{line} [reproduces {reproduces} (got {at_accuracy}), within old leak \
             {within_old_leak}, origin-free {origin_free}]"
        ))
    }
}

/// [`moved_by_the_old_leak`] for a difference of two bodies' volumes, `before - after` (what
/// an operation removed), that moved from `old` to `new`: each body's volume may have moved
/// by its own old leak, so the gate is the move within the two old leak bounds and the two
/// quadratures' shares, with both volumes reproduced (the difference to 1e-9 of itself plus
/// a 1e-14 rounding floor on the larger volume) and origin-free.
pub fn difference_moved_by_the_old_leak(
    k: &Kernel,
    before: &mut Brep,
    after: &mut Brep,
    old: f64,
    new: f64,
    accuracy: f64,
) -> Result<String, String> {
    let store = AttributeStore::new();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut bound = 0.0;
    let mut origin_free = true;
    let mut volumes = [0.0; 2];
    let mut figures = Vec::new();
    for (i, b) in [before, after].into_iter().enumerate() {
        let at_accuracy =
            solidcore::mass::integrator::properties::brep_volume(k, b, accuracy, &attrs)
                .map_err(|e| format!("body {i}: brep_volume at {accuracy:e}: {e}"))?;
        let l = leak(k, b, 1e-10, 8);
        let quadrature = 2.0 * (at_accuracy - l.volume).abs();
        bound += l.old_bound + quadrature;
        origin_free &= l.origin_spread <= l.origin_allowance;
        volumes[i] = at_accuracy;
        figures.push(format!(
            "{at_accuracy}: old leak bound {:.3e} + quadrature {quadrature:.3e}, origin spread \
             {:.3e} (allowance {:.3e}), widest strip / tol {:.3}",
            l.old_bound, l.origin_spread, l.origin_allowance, l.worst_gap_over_tolerance
        ));
    }
    let moved = (new - old).abs();
    // relative to the removed volume itself (the bodies' volumes are recomputed at the
    // caller's accuracy, the same evaluation), with only a rounding floor on the larger one
    let reproduces = (volumes[0] - volumes[1] - new).abs()
        <= 1e-9 * new.abs() + 1e-14 * volumes[0].abs().max(volumes[1].abs());
    let within_old_leak = moved <= bound;
    let line = format!(
        "difference {new} (was {old}, moved {moved:.3e}) within {bound:.3e}; before {}; after {}",
        figures[0], figures[1]
    );
    if reproduces && within_old_leak && origin_free {
        Ok(line)
    } else {
        Err(format!(
            "{line} [reproduces {reproduces}, within old leaks {within_old_leak}, origin-free \
             {origin_free}]"
        ))
    }
}

/// [`difference_moved_by_the_old_leak`], panicking with its figures when a check fails.
pub fn assert_difference_moved_by_the_old_leak(
    k: &Kernel,
    before: &mut Brep,
    after: &mut Brep,
    old: f64,
    new: f64,
    accuracy: f64,
) {
    match difference_moved_by_the_old_leak(k, before, after, old, new, accuracy) {
        Ok(line) => eprintln!("#16735 move: {line}"),
        Err(line) => panic!("#16735 move: {line}"),
    }
}

/// [`moved_by_the_old_leak`], panicking with its figures when a check fails.
pub fn assert_moved_by_the_old_leak(k: &Kernel, b: &mut Brep, old: f64, new: f64, accuracy: f64) {
    match moved_by_the_old_leak(k, b, old, new, accuracy) {
        Ok(line) => eprintln!("#16735 move: {line}"),
        Err(line) => panic!("#16735 move: {line}"),
    }
}

/// The body's tolerance volume: `sum A_f tol_f` over faces (area times tolerance) `+ sum pi
/// tol_e^2 L_e` over edges (the tubes) `+ sum 4/3 pi tol_v^3` over vertices (the balls), from
/// the body's own stored tolerances. The region a body bounds is defined only up to it: a
/// face within its tolerance of the solid's boundary moves the volume by at most its area
/// times that tolerance (to first order), and near an edge or vertex the closure lies in the
/// edge's tube or the vertex's ball (ARGUED: the strips' widths are measured against the
/// tolerances by [`leak`], not certified).
pub fn tolerance_volume(k: &Kernel, b: &mut Brep) -> f64 {
    use solidcore::mass::integrator::properties::face_area;
    use solidcore::topology::face;
    let mut slack = 0.0;
    for f in p1::faces(b, None).unwrap() {
        let tol = face::p2::tolerance(b, f).unwrap();
        slack += face_area(k, b, f, 1e-8).unwrap() * tol;
    }
    for e in p1::edges(b, None).unwrap() {
        let tol = edge::tolerance(b, e).unwrap();
        slack += PI * tol * tol * edge_length(k, b, e, 1e-9).unwrap();
    }
    for v in p1::vertices(b, None).unwrap() {
        let tol = vertex::tolerance(b, v).unwrap();
        slack += 4.0 / 3.0 * PI * tol * tol * tol;
    }
    slack
}
