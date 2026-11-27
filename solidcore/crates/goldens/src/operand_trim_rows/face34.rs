//! Certificates that a `SmFace::Test #34` ("Face has partially Coincident Edges") line of a
//! moved row's body is a false positive of the check (#18212), not a defect the row brings:
//!
//! * **Same geometry as main.** Every edge of a pair the check flags has, bit for bit, the
//!   curve and interval of an edge of main's same body: its fingerprint ([`fingerprint`]: the
//!   curve's type and NURBS data and the interval's ends, every `f64` by its bits) is one
//!   main's run gave for that body (`FALSE_POSITIVES`' `main`, from a probe of main's code).
//! * **No looser.** Each such edge's tolerance is at most main's for that edge: the check
//!   compares the two curves within the larger tolerance, and main's body, with the same
//!   curves and tolerances no smaller, passes it (main's level 2 has no #34 line).
//! * **Only where the edges meet.** The two edges share exactly one vertex `V`, meet there
//!   transversally, and every range the check reports lies in the end contact that
//!   transversal meeting forces. Per edge, the curve end at `V` is the interval end nearer
//!   it, and its *near stretch* runs from a base through the farthest end of its reported
//!   ranges: the base is the end or one of the first knots past it ([`MAX_BASES`], nearest
//!   first, the first pair that certifies taken), and `g` bounds above how far the curve
//!   gets from `V` between its end and its base: its *hook* (e24's first 1e-5, which points
//!   back, is one). Over each near stretch the
//!   curve's derivative is enclosed in a box ([`curve_derivs_over`], rounded interval
//!   arithmetic, pointed away from the end), and [`sine_lo`] bounds below by `σ > 0` the
//!   sine of the angle between any vector of one box and any of the other (1 if no pair is
//!   acute). By the mean value theorem a point of a near stretch is its base plus a length
//!   `r` times a vector of that box (the box holds the derivative's convex hull), and for
//!   `x`, `y` at an angle of at least `θ`, `|x - y| >= |x| sin θ` (`>= |x|` past a right
//!   angle). The bases lie within `g_a`, `g_b` of `V`, so two points of the two near
//!   stretches at distance at most the pair's tolerance `t` have `r <= (t + g_a + g_b) / σ`
//!   on either curve: their being within `t` is forced by the meeting alone, inside the
//!   radius `R = (t + g_a + g_b) / σ` about each base, and no pair of points farther out is
//!   that close. A hook is no exception: its points lie within `g` of `V`, inside its own
//!   end contact `R + g`, and a point of the other curve within `t` of one lies within
//!   `t + g` of `V`, which is checked to be inside the other curve's end contact (it is
//!   whenever `σ <= 1`, as `R >= t + g_a + g_b`). So the end contact holds every pair of
//!   points of the two curves, from end to farthest range, within `t` of each other. Every
//!   reported range is enclosed ([`farthest_from`]) within `R + g` of `V`, the end contact
//!   (for curves meeting at a right angle with ends on `V`, the vertex ball of radius `t`).
//!   No stretch of either edge away from `V` is shared; a tangent meeting (`σ` not
//!   certified positive) fails.
//!
//! A flagged pair failing any of these fails the row. The pairs are found by running the
//! check's own comparison ([`flagged_pairs`]: `GlobalCoincidenceChecker` of each pair of the
//! face's edges within the larger tolerance, as `assert_test_face34`, on every pair, not only
//! those its box search keeps).

use super::{norm_hi, round_up, Ivl, V3};
use crate::trims_on_edges::curve_nurbs;
use solidcore::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use solidcore::curve::curve::Curve;
use solidcore::geom::solution_array::SolutionType;
use solidcore::kernel::Kernel;
use solidcore::topology::{Brep, EdgeId, ObjectHandle};

/// The test #34 line `check_brep` level 2 prints.
pub(crate) const FACE34: &str = "SmFace::Test #34 Face has partially Coincident Edges: Face has \
     partially Coincident Edges. [owner SmFace]";

/// A level 2 line certified a false positive: (case, body prefix, the line, its issue, main's
/// fingerprints and tolerances of the flagged edges, from main's (7d4445c0c) run).
pub(crate) type FalsePositive = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [(u64, f64)],
);

/// The level 2 lines certified false positives. None since the merge of main (e35bc3930):
/// `single_face_fillet1019_6`'s test #34 line (#18212, edges e21/e24 and e5/e8) is gone from
/// both of its bodies, as on main.
pub(crate) const FALSE_POSITIVES: &[FalsePositive] = &[];

/// FNV-1a over the curve's type code, its NURBS degree, count, knots and homogeneous control
/// points and the interval's ends, each as its bits.
pub(crate) fn fingerprint(c: &Curve, lo: f64, hi: f64) -> Result<u64, String> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |x: u64| {
        for byte in x.to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let n = curve_nurbs(c)?;
    eat(c.type_code() as u64);
    eat(n.p as u64);
    eat(n.n as u64);
    for &x in n.u.iter().chain(n.pw.iter()) {
        eat(x.to_bits());
    }
    eat(lo.to_bits());
    eat(hi.to_bits());
    Ok(h)
}

/// One flagged pair: its edges and, per coincident range, each curve's parameter range.
pub(crate) struct Flagged {
    pub(crate) edges: [EdgeId; 2],
    pub(crate) ranges: Vec<[(f64, f64); 2]>,
}

/// The pairs of edges of a face of `b` that `GlobalCoincidenceChecker` (within the larger
/// tolerance, as test #34) finds coincident over a range of values.
///
/// # Errors
///
/// A missing edge or curve; the checker's error.
pub(crate) fn flagged_pairs(k: &Kernel, b: &Brep) -> Result<Vec<Flagged>, String> {
    let e = |x: solidcore::SolidError| x.to_string();
    let mut out = Vec::new();
    for f in solidcore::topology::brep::p1::faces(b, None).map_err(e)? {
        let edges = solidcore::topology::face::p4::edges(b, f, None, false, None).map_err(e)?;
        for (ii, &e1) in edges.iter().enumerate() {
            for &e2 in edges.iter().skip(ii + 1) {
                let (r1, r2) = (b.edge(e1).map_err(e)?, b.edge(e2).map_err(e)?);
                let t = if r1.tolerance > r2.tolerance {
                    r1.tolerance
                } else {
                    r2.tolerance
                };
                let c1 = r1.curve.as_ref().ok_or("no edge curve")?;
                let c2 = r2.curve.as_ref().ok_or("no edge curve")?;
                let found = c1
                    .global_coincidence_checker::<ObjectHandle>(
                        k,
                        &r1.interval,
                        c2,
                        &r2.interval,
                        t,
                        true,
                        [None, None],
                    )
                    .map_err(e)?;
                let ranges: Vec<[(f64, f64); 2]> = found
                    .solutions
                    .iter()
                    .filter(|s| s.solution_type == SolutionType::RangeOfValues)
                    .map(|s| {
                        [
                            (s.start.parameters[0], s.end.parameters[0]),
                            (s.start.parameters[1], s.end.parameters[1]),
                        ]
                    })
                    .collect();
                if !ranges.is_empty() {
                    out.push(Flagged {
                        edges: [e1, e2],
                        ranges,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// `a <= b`; a NaN on either side is not within.
fn within(a: f64, b: f64) -> bool {
    a <= b
}

/// The deepest a range is halved in [`farthest_from`].
const MAX_DEPTH: u32 = 24;

/// The most enclosures [`derivative_box`] or [`farthest_from`] takes.
const MAX_PARTS: usize = 4096;

/// The most bases (the end, then knots) tried per edge in [`certify`].
const MAX_BASES: usize = 4;

/// An upper bound on the distance from `c`'s points over `[lo, hi]` (either order) to `p`:
/// the most over parts of the range, each enclosed whole, a part whose bound is above `tol`
/// halved (to [`MAX_DEPTH`] and [`MAX_PARTS`] enclosures); the parts cover the range, so no
/// point is skipped.
fn farthest_from(c: &Curve, lo: f64, hi: f64, p: [f64; 3], tol: f64) -> Result<f64, String> {
    let n = curve_nurbs(c)?;
    let mut parts = vec![(lo.min(hi), lo.max(hi), 0_u32)];
    let mut worst = 0.0_f64;
    let mut count = 0_usize;
    while let Some((a, b, depth)) = parts.pop() {
        count += 1;
        let d = curve_derivs_over(&n, Ivl::new(a, b), 0, None).map_err(|x| x.to_string())?;
        let at = d.first().ok_or("no enclosure")?;
        let d = norm_hi(V3 {
            x: at.x - Ivl::from(p[0]),
            y: at.y - Ivl::from(p[1]),
            z: at.z - Ivl::from(p[2]),
        });
        // (a NaN is not within)
        if !within(d, tol) && depth < MAX_DEPTH && count + parts.len() < MAX_PARTS && a < b {
            let m = 0.5 * (a + b);
            parts.push((m, b, depth + 1));
            parts.push((a, m, depth + 1));
            continue;
        }
        worst = if d.is_nan() {
            f64::INFINITY
        } else {
            worst.max(d)
        };
    }
    Ok(worst)
}

/// Certifies `fp`'s line for `b` (see the module docs): every flagged pair is main's
/// geometry, no looser than main's, and coincident only inside its shared vertex's end
/// contact. Returns the flagged pairs' count and the largest ratio, over the reported ranges
/// and the hooks (and the points within tolerance of them), of an enclosed distance from the
/// vertex to the end contact's reach (at most 1).
///
/// # Errors
///
/// The first condition that fails, named.
pub(crate) fn certify(k: &Kernel, b: &Brep, fp: &FalsePositive) -> Result<(usize, f64), String> {
    let e = |x: solidcore::SolidError| x.to_string();
    let main = fp.4;
    let pairs = flagged_pairs(k, b)?;
    if pairs.is_empty() {
        return Err("no pair flagged".to_string());
    }
    let mut worst = 0.0_f64;
    for pair in &pairs {
        let mut ends = Vec::new();
        for &edge in &pair.edges {
            let r = b.edge(edge).map_err(e)?;
            let c = r.curve.as_ref().ok_or("no edge curve")?;
            let print = fingerprint(c, r.interval.min(), r.interval.max())?;
            let Some(&(_, main_tol)) = main.iter().find(|m| m.0 == print) else {
                return Err(format!(
                    "edge {}: geometry {print:#018x} is not main's",
                    edge.index()
                ));
            };
            if !within(r.tolerance, main_tol) {
                return Err(format!(
                    "edge {}: tolerance {:e} above main's {main_tol:e}",
                    edge.index(),
                    r.tolerance
                ));
            }
            let (v0, v1) = solidcore::topology::edge::start_end_vertices(b, edge).map_err(e)?;
            ends.push([v0, v1]);
        }
        let shared: Vec<_> = ends[0]
            .iter()
            .flatten()
            .filter(|v| ends[1].iter().flatten().any(|w| w == *v))
            .copied()
            .collect();
        let [v] = shared[..] else {
            return Err(format!(
                "edges {} and {}: share {} vertices, not one",
                pair.edges[0].index(),
                pair.edges[1].index(),
                shared.len()
            ));
        };
        let vr = b.vertex(v).map_err(e)?;
        let p = [vr.point.x, vr.point.y, vr.point.z];
        // per edge, its curve and its candidate bases: (base, gap, the derivative box over
        // the near stretch from the base, pointed away from the end)
        let mut near = Vec::new();
        for (i, &edge) in pair.edges.iter().enumerate() {
            let r = b.edge(edge).map_err(e)?;
            let c = r.curve.as_ref().ok_or("no edge curve")?;
            let (lo, hi) = (r.interval.min(), r.interval.max());
            let (g_lo, g_hi) = (
                farthest_from(c, lo, lo, p, 0.0)?,
                farthest_from(c, hi, hi, p, 0.0)?,
            );
            let end = if g_lo <= g_hi { lo } else { hi };
            let far = pair
                .ranges
                .iter()
                .flat_map(|x| [x[i].0, x[i].1])
                .fold(end, |f, s| {
                    if (s - end).abs() > (f - end).abs() {
                        s
                    } else {
                        f
                    }
                });
            let n = curve_nurbs(c)?;
            // the end, then the knots strictly inside the near stretch, nearest the end first
            let mut bases = vec![end];
            let mut inside: Vec<f64> =
                n.u.iter()
                    .copied()
                    .filter(|&s| s > end.min(far) && s < end.max(far))
                    .collect();
            inside.sort_by(|x, y| (x - end).abs().total_cmp(&(y - end).abs()));
            inside.dedup();
            bases.extend(inside.into_iter().take(MAX_BASES - 1));
            let mut candidates = Vec::new();
            for base in bases {
                let gap = farthest_from(c, end, base, p, vr.tolerance)?;
                let d = derivative_box(&n, base.min(far), base.max(far))?;
                let d = if end == lo {
                    d
                } else {
                    V3 {
                        x: Ivl::from(0.0) - d.x,
                        y: Ivl::from(0.0) - d.y,
                        z: Ivl::from(0.0) - d.z,
                    }
                };
                candidates.push((base, gap, d));
            }
            near.push((edge, c, candidates));
        }
        let [(_, _, ref ca), (_, _, ref cb)] = near[..] else {
            return Err("not a pair".to_string());
        };
        // the first pair of bases, nearest the ends first, whose boxes are transversal
        let Some((sine, gap_a, gap_b, base_a, base_b)) = ca
            .iter()
            .flat_map(|a| cb.iter().map(move |b| (a, b)))
            .map(|(a, b)| (sine_lo(a.2, b.2), a.1, b.1, a.0, b.0))
            .find(|x| x.0 > 0.0)
        else {
            return Err(format!(
                "edges {} and {}: not certified transversal at vertex {} (their directions \
                 near it may be parallel): candidate bases {:?} {:?}",
                pair.edges[0].index(),
                pair.edges[1].index(),
                v.index(),
                ca,
                cb
            ));
        };
        let tol = {
            let (ta, tb) = (
                b.edge(pair.edges[0]).map_err(e)?.tolerance,
                b.edge(pair.edges[1]).map_err(e)?.tolerance,
            );
            if ta > tb {
                ta
            } else {
                tb
            }
        };
        // the end contact's radius about either curve's base, then about the vertex
        let radius = round_up(round_up(round_up(tol + gap_a) + gap_b) / sine);
        let reaches = [round_up(radius + gap_a), round_up(radius + gap_b)];
        eprintln!(
            "  face34 edges {}/{} at vertex {}: tolerance {tol:e}, bases {base_a:e}/{base_b:e}, \
             hooks within {gap_a:e}/{gap_b:e}, sine >= {sine:e}, reaches {:e}/{:e}",
            pair.edges[0].index(),
            pair.edges[1].index(),
            v.index(),
            reaches[0],
            reaches[1]
        );
        // the hooks: each curve's stretch from its end to its base (empty when the base is
        // the end) lies within its gap of the vertex, so a point of the other curve within the
        // tolerance of it lies within tolerance + gap of the vertex: inside the other curve's
        // end contact, checked here, and the hook itself inside its own
        for (i, (gap, base)) in [(gap_a, base_a), (gap_b, base_b)].into_iter().enumerate() {
            let own = reaches.get(i).copied().unwrap_or(f64::NAN);
            let other = reaches.get(1 - i).copied().unwrap_or(f64::NAN);
            let touch = round_up(tol + gap);
            if !(within(gap, own) && within(touch, other)) {
                return Err(format!(
                    "edge {}: hook to base {base:e} reaches {gap:e} from vertex {}; within \
                     tolerance of it {touch:e}, past the end contacts {own:e} / {other:e}",
                    pair.edges.get(i).map_or(u32::MAX, |x| x.index()),
                    v.index(),
                ));
            }
            worst = worst.max(gap / own).max(touch / other);
        }
        for range in &pair.ranges {
            for (i, &(edge, c, _)) in near.iter().enumerate() {
                let reach = reaches.get(i).copied().unwrap_or(f64::NAN);
                let d = farthest_from(c, range[i].0, range[i].1, p, reach)?;
                if !within(d, reach) {
                    return Err(format!(
                        "edge {}: range {:?} reaches {d:e} from vertex {}, past the end \
                         contact's {reach:e} (tolerance {tol:e}, sine {sine:e})",
                        edge.index(),
                        range[i],
                        v.index(),
                    ));
                }
                worst = worst.max(d / reach);
            }
        }
    }
    Ok((pairs.len(), worst))
}

/// A box holding the curve's first derivative over `[lo, hi]`: the hull of enclosures over
/// parts covering the range, a part whose box is wide against its least norm (a side over
/// 1/16 of it) halved, to [`MAX_DEPTH`] and [`MAX_PARTS`] enclosures. Holding every value,
/// it holds their convex hull.
fn derivative_box(n: &NurbsCurve, lo: f64, hi: f64) -> Result<V3<Ivl>, String> {
    let mut parts = vec![(lo, hi, 0_u32)];
    let mut out: Option<V3<Ivl>> = None;
    let mut count = 0_usize;
    while let Some((a, b, depth)) = parts.pop() {
        count += 1;
        let d = curve_derivs_over(n, Ivl::new(a, b), 1, None).map_err(|x| x.to_string())?;
        let d = *d.get(1).ok_or("no derivative enclosure")?;
        let mig = |x: Ivl| x.mig();
        let least = (mig(d.x) * mig(d.x) + mig(d.y) * mig(d.y) + mig(d.z) * mig(d.z)).sqrt();
        let wide = [d.x, d.y, d.z]
            .iter()
            .any(|x| !within(x.hi - x.lo, least / 16.0));
        if wide && depth < MAX_DEPTH && count + parts.len() < MAX_PARTS && a < b {
            let m = 0.5 * (a + b);
            parts.push((m, b, depth + 1));
            parts.push((a, m, depth + 1));
            continue;
        }
        out = Some(match out {
            None => d,
            Some(o) => V3 {
                x: o.x.hull(d.x),
                y: o.y.hull(d.y),
                z: o.z.hull(d.z),
            },
        });
    }
    out.ok_or_else(|| "no derivative enclosure".to_string())
}

/// A lower bound on the sine of the angle between any vector of box `a` and any of box `b`
/// (1 when no pair is acute); 0 (nothing certified) when a box may hold the zero vector or
/// the bound is not positive.
fn sine_lo(a: V3<Ivl>, b: V3<Ivl>) -> f64 {
    let norm_lo = |v: V3<Ivl>| {
        (Ivl::from(v.x.mig()) * Ivl::from(v.x.mig())
            + Ivl::from(v.y.mig()) * Ivl::from(v.y.mig())
            + Ivl::from(v.z.mig()) * Ivl::from(v.z.mig()))
        .sqrt()
        .lo
    };
    let (na, nb) = (norm_lo(a), norm_lo(b));
    let dot = (a.x * b.x + a.y * b.y + a.z * b.z).hi;
    if !(na > 0.0 && nb > 0.0) || dot.is_nan() {
        return 0.0;
    }
    if dot <= 0.0 {
        return 1.0;
    }
    // cos <= dot / (|a| |b|) over the boxes
    let cos = (Ivl::from(dot) / (Ivl::from(na) * Ivl::from(nb))).hi;
    if cos.is_nan() || cos >= 1.0 {
        return 0.0;
    }
    let s = (Ivl::from(1.0) - Ivl::from(cos) * Ivl::from(cos)).sqrt().lo;
    if s > 0.0 {
        s
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real partial coincidence is still refused: a box face whose edge's curve is made its
    /// neighbour's (the two now overlap along the whole neighbour) is flagged, and the
    /// certificate fails, the two near stretches not transversal, even with the geometry and
    /// tolerances accepted as main's.
    #[test]
    fn a_true_coincidence_is_not_certified() {
        let mut k = Kernel::default();
        let mut b = solidcore::primitive::api_primitives::create_box(
            &mut k,
            solidcore::Point3::new(0.0, 0.0, 0.0),
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        let f = solidcore::topology::brep::p1::faces(&b, None).unwrap()[0];
        let edges = solidcore::topology::face::p4::edges(&b, f, None, false, None).unwrap();
        // two edges of the face that share a vertex
        let ends = |e| solidcore::topology::edge::start_end_vertices(&b, e).unwrap();
        let (a, n) = edges
            .iter()
            .flat_map(|&a| edges.iter().map(move |&n| (a, n)))
            .find(|&(a, n)| {
                let (x, y) = (ends(a), ends(n));
                a != n && (x.0 == y.0 || x.0 == y.1 || x.1 == y.0 || x.1 == y.1)
            })
            .unwrap();
        let (curve, interval) = {
            let r = b.edge(n).unwrap();
            (r.curve.clone(), r.interval)
        };
        {
            let r = b.edge_mut(a).unwrap();
            r.curve = curve;
            r.interval = interval;
        }
        let pairs = flagged_pairs(&k, &b).unwrap();
        assert!(!pairs.is_empty());
        // main's prints and tolerances: the doctored edges' own
        let mut main = Vec::new();
        for &e in &[a, n] {
            let r = b.edge(e).unwrap();
            let print = fingerprint(
                r.curve.as_ref().unwrap(),
                r.interval.min(),
                r.interval.max(),
            )
            .unwrap();
            main.push((print, r.tolerance));
        }
        let main: &'static [(u64, f64)] = Box::leak(main.into_boxed_slice());
        let fp: FalsePositive = ("x", "x", FACE34, "#0", main);
        let err = certify(&k, &b, &fp).unwrap_err();
        assert!(err.contains("not certified transversal"), "{err}");
    }
}
