//! Guide rails for a loft (#201): curves the loft follows between its sections. A rail runs from
//! the first section to the last and passes through every section's curve. Between each two
//! sections, stations are added: each is the plain loft there, moved so that the point of it a
//! rail guides lands on the rail. With one rail the station turns about the loft's direction
//! there and grows or shrinks about its middle (stretches across to the rail, with
//! [`RailFit::Stretch`]), shifted along the loft as far as the rail leads or lags; with two it
//! moves with the first rail, turns with the line between the rails and sizes with their distance
//! (stretches along that line), leaning to follow the second. At the sections the move is none,
//! so the sections and the loft's caps stay exactly as drawn; the loft's smooth surfaces then run
//! through the sections and the stations together. The stations are evenly spaced, twice as
//! many in a span where the surface would stray from a rail between them ([`spaced`]).
//!
//! Sections are exact NURBS curves ([`Section`], from the sketch segments' record curves); a
//! station is the plain loft's control points moved by an affine map, so each is exact too. Before
//! the stations are made, every section's k-th curve is brought to one form ([`unified`]: the same
//! degree and knots, the curve itself unchanged) so that the control points of a line pair with
//! those of an arc or a spline against it (#12512); a station's control points then go straight
//! between the sections' and its weights with them, so the weights need not agree either.

use solidcore_record::{Plane, Seg2, Vec3};
use solidcraft_geom::bspline::{CurveN, HPoint};
use solidcraft_geom::prelude::*;

use crate::sweep_path::RailFit;
use crate::{KernelError, Result};

/// Most curves a railed loft is skinned through per column, sections and even stations together;
/// [`spaced`] doubles a span's stations where the loft would stray from a rail between them.
pub(crate) const MAX_WIRES: usize = 33;
/// Most stations between two sections.
pub(crate) const MAX_PER_SPAN: usize = 15;
/// Steps between two even stations at which the rails are followed, to find where the loft would
/// stray from them between stations ([`spaced`]).
const TRACK_STEPS: usize = 8;

/// A loft section: its plane and its curves in order, each a NURBS curve with homogeneous control
/// points `[x w, y w, z w, w]`, and the profile segments each curve stands for.
#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub plane: Plane,
    pub curves: Vec<CurveN<4>>,
    pub numbers: Vec<Vec<usize>>,
}

impl Section {
    /// The sketch segments `segs` on `plane` as a section (their exact record curves), segment i
    /// standing for the profile segments `numbers[i]`.
    pub(crate) fn of(plane: &Plane, segs: &[Seg2], numbers: Vec<Vec<usize>>) -> Result<Section> {
        let curves = segs.iter().map(|s| nurbs(&crate::freeform::segment(plane, s)?.0)).collect::<Result<Vec<_>>>()?;
        Ok(Section { plane: *plane, curves, numbers })
    }
}

/// A record line or (rational) B-spline as a homogeneous NURBS curve.
fn nurbs(c: &solidcore_record::brep::Curve) -> Result<CurveN<4>> {
    use solidcore_record::brep::Curve;
    let degree = |knots: &[f64], n: usize| knots.len().checked_sub(n + 1).ok_or_else(|| KernelError::Invalid("spline degree".into()));
    Ok(match c {
        Curve::Line { start, end } => {
            CurveN { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[start[0], start[1], start[2], 1.0], [end[0], end[1], end[2], 1.0]] }
        }
        Curve::Spline { knots, points } => {
            CurveN { degree: degree(knots, points.len())?, knots: knots.clone(), ctrl: points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect() }
        }
        Curve::RationalSpline { knots, homogeneous } => {
            CurveN { degree: degree(knots, homogeneous.len())?, knots: knots.clone(), ctrl: homogeneous.clone() }
        }
        _ => return Err(KernelError::Invalid("a loft section curve that is not a line or a spline".into())),
    })
}

/// A curve's control points (x, y, z) and weights.
fn points(c: &CurveN<4>) -> (Vec<Vec3>, Vec<f64>) {
    c.ctrl.iter().map(|h| (Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]), h[3])).unzip()
}

/// `c` with the control points `pts` (its own weights and knots).
fn with(c: &CurveN<4>, pts: &[Vec3]) -> CurveN<4> {
    let ctrl = pts.iter().zip(&c.ctrl).map(|(p, h)| [p.x * h[3], p.y * h[3], p.z * h[3], h[3]]).collect();
    CurveN { degree: c.degree, knots: c.knots.clone(), ctrl }
}

/// The point `f` (0…1) of the way along a curve's parameter range.
fn point_at(c: &CurveN<4>, f: f64) -> Option<Vec3> {
    let (t0, t1) = (*c.knots.get(c.degree)?, *c.knots.get(c.ctrl.len())?);
    let ([x, y, z, w], _) = c.eval(t0 + (t1 - t0) * f.clamp(0.0, 1.0))?;
    (w.abs() > 1e-300).then(|| Vec3::new(x / w, y / w, z / w))
}

/// Whether two knots are the same (curves run on [0, 1]).
fn near(x: f64, y: f64) -> bool {
    (x - y).abs() <= 1e-9
}

fn hpoint(h: [f64; 4]) -> HPoint {
    HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }
}

fn array(h: HPoint) -> [f64; 4] {
    [h.x, h.y, h.z, h.w]
}

/// `c` on t in [0, 1] (its knots mapped affinely; the curve is unchanged).
fn normalized(c: &CurveN<4>) -> Result<CurveN<4>> {
    let (p, n) = (c.degree, c.ctrl.len());
    let bad = || KernelError::Invalid("a loft section curve".into());
    let (Some(&t0), Some(&t1)) = (c.knots.get(p), c.knots.get(n)) else { return Err(bad()) };
    if n <= p || c.knots.len() != n + p + 1 || !(t1 > t0) || !t0.is_finite() || !t1.is_finite() {
        return Err(bad());
    }
    let knots = c.knots.iter().map(|u| ((u - t0) / (t1 - t0)).clamp(0.0, 1.0)).collect();
    Ok(CurveN { degree: p, knots, ctrl: c.ctrl.clone() })
}

/// A Bézier piece's homogeneous control points one degree higher, the same curve (NURBS Book
/// eq. 5.36).
fn elevated(pts: &[HPoint]) -> Vec<HPoint> {
    let p = pts.len().saturating_sub(1);
    let at = |i: usize| pts.get(i).copied().unwrap_or(HPoint::ZERO);
    (0..=p + 1)
        .map(|i| {
            let a = i as f64 / (p + 1) as f64;
            let prev = if i == 0 { HPoint::ZERO } else { at(i - 1) * a };
            prev + at(i) * (1.0 - a)
        })
        .collect()
}

/// The distinct interior knots of `c` (on [0, 1]) with their multiplicities, in order.
fn interior(c: &CurveN<4>) -> Vec<(f64, usize)> {
    let mut out: Vec<(f64, usize)> = Vec::new();
    for &u in c.knots.iter().filter(|u| **u > 0.0 && **u < 1.0) {
        match out.last_mut() {
            Some(last) if near(last.0, u) => last.1 += 1,
            _ => out.push((u, 1)),
        }
    }
    out
}

/// `c` (on [0, 1]) as a curve of `degree`, the same curve: split into its Bézier pieces, each
/// raised (exactly) and joined again, at each break a knot of multiplicity `degree`.
fn raised(c: &CurveN<4>, degree: usize) -> Result<CurveN<4>> {
    if c.degree == degree {
        return Ok(c.clone());
    }
    let fail = |m: &str| KernelError::Failed(format!("loft rails: {m}"));
    if c.degree > degree {
        return Err(fail("degree"));
    }
    let h: Vec<HPoint> = c.ctrl.iter().map(|x| hpoint(*x)).collect();
    let mut breaks = vec![0.0];
    breaks.extend(interior(c).iter().map(|(u, _)| *u));
    breaks.push(1.0);
    let mut ctrl: Vec<[f64; 4]> = Vec::new();
    let mut knots = vec![0.0; degree + 1];
    for (i, w) in breaks.windows(2).enumerate() {
        let (_, mut piece) = solidcraft_geom::bspline::segment(c.degree, &c.knots, &h, w[0], w[1]).map_err(|e| fail(&e.to_string()))?;
        while piece.len() < degree + 1 {
            piece = elevated(&piece);
        }
        ctrl.extend(piece.iter().skip(usize::from(i > 0)).map(|q| array(*q)));
        if i > 0 {
            knots.extend(std::iter::repeat_n(w[0], degree));
        }
    }
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    if knots.len() != ctrl.len() + degree + 1 {
        return Err(fail("degree"));
    }
    Ok(CurveN { degree, knots, ctrl })
}

/// Every section's k-th curve brought to one form without changing any curve: each on t in
/// [0, 1], raised to the highest degree in its column, then given every knot the others have
/// (Boehm insertion), so that their control points correspond one to one. The weights stay each
/// curve's own (a line's are all 1, an arc's are not); a station between takes its control
/// points straight between the sections' and its weights between theirs.
fn unified(sections: &[Section]) -> Result<Vec<Section>> {
    let n = sections.first().map_or(0, |s| s.curves.len());
    let fail = |m: &str| KernelError::Failed(format!("loft rails: {m}"));
    let mut columns: Vec<Vec<CurveN<4>>> = Vec::with_capacity(n);
    for k in 0..n {
        let column: Vec<&CurveN<4>> = sections.iter().map(|s| s.curves.get(k).ok_or_else(|| fail("sections"))).collect::<Result<_>>()?;
        let degree = column.iter().map(|c| c.degree).max().unwrap_or(1);
        let mut curves: Vec<CurveN<4>> = column.iter().map(|c| raised(&normalized(c)?, degree)).collect::<Result<_>>()?;
        // The union of the interior knots, each with its greatest multiplicity.
        let mut union: Vec<(f64, usize)> = Vec::new();
        for (u, m) in curves.iter().flat_map(interior) {
            match union.iter_mut().find(|(x, _)| near(*x, u)) {
                Some(e) => e.1 = e.1.max(m),
                None => union.push((u, m)),
            }
        }
        union.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut knots = vec![0.0; degree + 1];
        knots.extend(union.iter().flat_map(|(u, m)| std::iter::repeat_n(*u, *m)));
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        for c in &mut curves {
            let mut h: Vec<HPoint> = c.ctrl.iter().map(|x| hpoint(*x)).collect();
            let mut k = c.knots.clone();
            for (u, m) in &union {
                let have = k.iter().filter(|x| near(**x, *u)).count();
                for _ in have..*m {
                    (k, h) = solidcraft_geom::bspline::insert_knot(degree, &k, &h, *u).map_err(|e| fail(&e.to_string()))?;
                }
            }
            if k.len() != knots.len() {
                return Err(fail("knots"));
            }
            *c = CurveN { degree, knots: knots.clone(), ctrl: h.iter().map(|q| array(*q)).collect() };
        }
        columns.push(curves);
    }
    Ok(sections
        .iter()
        .enumerate()
        .map(|(i, s)| Section { plane: s.plane, curves: columns.iter().filter_map(|col| col.get(i).cloned()).collect(), numbers: s.numbers.clone() })
        .collect())
}

/// Where a rail meets a section: the point of the section it guides (as a wire parameter: the
/// curve's index plus how far along its parameter range), that point, the nudge onto it from where
/// the rail meets the section's plane, and how far along the rail that is (segment and fraction).
struct Anchor {
    g: f64,
    on: Vec3,
    nudge: Vec3,
    at: (usize, f64),
}

/// A rail as a polyline from the first section, and where it meets each section.
struct Rail {
    pts: Vec<Vec3>,
    anchors: Vec<Anchor>,
}

/// An affine move: `x` goes to `to + m·(x − from)` (`m` by columns).
pub(crate) struct Affine {
    from: Vec3,
    to: Vec3,
    m: [Vec3; 3],
}

impl Affine {
    pub(crate) fn apply(&self, x: Vec3) -> Vec3 {
        let d = x - self.from;
        self.to + self.m[0] * d.x + self.m[1] * d.y + self.m[2] * d.z
    }
}

/// The loft's sections with stations added between them so that the loft follows `rails`
/// (polylines; see the module notes), in loft order: each entry one section's (or station's)
/// curves. `closed` says whether the sections close. Without rails the sections' own curves come
/// back. With rails, the sections' k-th curves are first brought to one form ([`unified`]) so
/// that their control points correspond.
pub(crate) fn railed(sections: &[Section], closed: bool, rails: &[Vec<Vec3>], fit: RailFit) -> Result<Vec<Vec<CurveN<4>>>> {
    if rails.is_empty() {
        return Ok(sections.iter().map(|s| s.curves.clone()).collect());
    }
    if rails.len() > 2 {
        return Err(KernelError::Invalid("a loft takes one or two guide rails".into()));
    }
    let fail = |m: &str| KernelError::Failed(format!("loft rails: {m}"));
    let count = sections.len();
    let lead = sections.first().map(|s| &s.curves).ok_or_else(|| fail("sections"))?;
    let n = lead.len();
    if count < 2 || n == 0 || sections.iter().any(|s| s.curves.len() != n) {
        return Err(KernelError::Invalid("the loft sections have different numbers of curves".into()));
    }
    let sections = &unified(sections)?;
    let lead = sections.first().map(|s| &s.curves).ok_or_else(|| fail("sections"))?;
    // Each section's control points in space with their weights (`pctrl`: x, y, z, w) and alone
    // (`ctrl`).
    let pctrl: Vec<Vec<Vec<[f64; 4]>>> = sections
        .iter()
        .map(|s| {
            s.curves
                .iter()
                .map(|c| {
                    let (pts, ws) = points(c);
                    pts.iter().zip(ws).map(|(p, w)| [p.x, p.y, p.z, w]).collect()
                })
                .collect()
        })
        .collect();
    let ctrl: Vec<Vec<Vec<Vec3>>> = sections.iter().map(|s| s.curves.iter().map(|c| points(c).0).collect()).collect();
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for p in ctrl.iter().flatten().flatten() {
        if !p.is_finite() {
            return Err(fail("sections"));
        }
        bounds.add(*p);
    }
    let size = bounds.diagonal();
    let tol = (size * 1e-4).max(1e-6);
    // The sections' parameters along the loft: in proportion to the mean distance between their
    // curves' starts.
    let mut along = vec![0.0];
    for w in ctrl.windows(2) {
        let step = w[0].iter().zip(&w[1]).filter_map(|(a, b)| Some(a.first()?.dist(*b.first()?))).sum::<f64>() / n as f64;
        along.push(along.last().copied().unwrap_or(0.0) + step);
    }
    let total = along.last().copied().unwrap_or(0.0);
    if !(total > 1e-9) || along.windows(2).any(|w| !(w[1] - w[0] > 1e-9 * total)) {
        return Err(KernelError::Invalid("two loft sections sit on top of each other".into()));
    }
    along.iter_mut().for_each(|u| *u /= total);
    // The plain loft between the sections: each control point (and its weight) straight between
    // two sections, the curve (quadratic, or cubic from four) through them across more.
    let degree = (count - 1).min(3);
    let across: Vec<Vec<CurveN<4>>> = if count == 2 {
        Vec::new()
    } else {
        (0..n)
            .map(|k| {
                (0..lead.get(k).map_or(0, |c| c.ctrl.len()))
                    .map(|j| {
                        let pts: Vec<[f64; 4]> = pctrl.iter().filter_map(|s| s.get(k)?.get(j).copied()).collect();
                        CurveN::interpolate(&along, &pts, degree).map_err(|e| fail(&e.to_string()))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?
    };
    let plain = |v: f64| -> Result<Vec<Vec<[f64; 4]>>> {
        let (Some(a), Some(b)) = (pctrl.first(), pctrl.last()) else { return Err(fail("sections")) };
        if count == 2 {
            let lerp = |p: &[f64; 4], q: &[f64; 4]| [0, 1, 2, 3].map(|i| p[i] + (q[i] - p[i]) * v);
            return Ok(a.iter().zip(b).map(|(ea, eb)| ea.iter().zip(eb).map(|(p, q)| lerp(p, q)).collect()).collect());
        }
        across.iter().map(|cs| cs.iter().map(|c| c.eval(v).map(|(h, _)| h).ok_or_else(|| fail("the plain loft"))).collect()).collect()
    };
    // The station curves: the lead's degree and knots, the plain loft's points and weights.
    let curves_of = |h: &[Vec<[f64; 4]>]| -> Result<Vec<CurveN<4>>> {
        lead.iter()
            .zip(h)
            .map(|(c, pts)| {
                if pts.iter().any(|q| !(q[3] > 0.0) || !q.iter().all(|x| x.is_finite())) {
                    return Err(fail("the plain loft"));
                }
                let ctrl = pts.iter().map(|q| [q[0] * q[3], q[1] * q[3], q[2] * q[3], q[3]]).collect();
                Ok(CurveN { degree: c.degree, knots: c.knots.clone(), ctrl })
            })
            .collect()
    };
    // The plain loft's middle and its direction there.
    let middle_at = |v: f64| -> Result<Vec3> { centroid(&curves_of(&plain(v)?)?).ok_or_else(|| fail("the plain loft")) };
    let middle = |v: f64| -> Result<(Vec3, Vec3)> {
        let h = 1e-4;
        let d = middle_at((v + h).min(1.0))? - middle_at((v - h).max(0.0))?;
        let d = d.normalized().ok_or_else(|| KernelError::Invalid("two loft sections sit on top of each other".into()))?;
        Ok((middle_at(v)?, d))
    };
    let start = centroid(lead).ok_or_else(|| fail("sections"))?;
    let rails: Vec<Rail> = rails.iter().map(|r| rail(r, start, sections, tol)).collect::<Result<_>>()?;
    if let [a, b] = &rails[..]
        && a.anchors.iter().zip(&b.anchors).any(|(p, q)| p.on.dist(q.on) <= tol)
    {
        return Err(KernelError::Invalid("the two guide rails meet a section at the same point".into()));
    }
    let normals = section_normals(sections, |v| middle(v).ok().map(|(_, d)| d), &along);
    let spans = count - 1;
    let per = (MAX_WIRES.saturating_sub(count) / spans).clamp(1, MAX_PER_SPAN);
    let mut out = Vec::with_capacity(count + spans * per);
    for i in 0..spans {
        out.push(sections.get(i).map(|s| s.curves.clone()).ok_or_else(|| fail("sections"))?);
        let (Some(&v0), Some(&v1), Some(&n0), Some(&n1)) = (along.get(i), along.get(i + 1), normals.get(i), normals.get(i + 1)) else {
            return Err(fail("sections"));
        };
        let ends: Vec<(&Anchor, &Anchor)> =
            rails.iter().map(|r| r.anchors.get(i).zip(r.anchors.get(i + 1)).ok_or_else(|| fail("rail"))).collect::<Result<_>>()?;
        let start: Vec<(usize, f64)> = ends.iter().map(|(a0, _)| a0.at).collect();
        // The plain loft's station `f` of the way through the span, and each guided point of it
        // with its rail's point, walking each rail on from `cursors`.
        let guided = |f: f64, cursors: &mut [(usize, f64)]| -> Result<(Vec<CurveN<4>>, Vec<(Vec3, Vec3)>)> {
            let station = curves_of(&plain(v0 + f * (v1 - v0))?)?;
            // The station's plane turns from one section's to the next's.
            let normal = n0.lerp(n1, f).normalized().ok_or_else(|| fail("section planes"))?;
            let mut pairs = Vec::with_capacity(rails.len());
            for ((r, (a0, a1)), cursor) in rails.iter().zip(&ends).zip(cursors.iter_mut()) {
                let anchor = wire_point(&station, interpolate_g(a0.g, a1.g, f, n, closed))?;
                // Where the rail crosses the station's plane through the point it guides, nudged so
                // that at each section it is the section's own point.
                let Some((hit, at)) = crate::sweep_path::rail_crossing(&r.pts, anchor, normal, *cursor, tol) else {
                    return Err(KernelError::Invalid(format!(
                        "a guide rail turns back between sections {} and {}: it has to run on from one section to the next",
                        i + 1,
                        i + 2
                    )));
                };
                *cursor = at;
                pairs.push((anchor, hit + a0.nudge * (1.0 - f) + a1.nudge * f));
            }
            Ok((station, pairs))
        };
        // Each rail's track: where it meets the stations at fine, even steps through the span,
        // from section to section. The steps' grid holds the even stations.
        let steps = TRACK_STEPS * (per + 1);
        let at = |s: usize| if s.is_multiple_of(TRACK_STEPS) { (s / TRACK_STEPS) as f64 / (per + 1) as f64 } else { s as f64 / steps as f64 };
        let mut cursors = start.clone();
        let mut tracks: Vec<Vec<Vec3>> = ends.iter().map(|(a0, _)| vec![a0.on]).collect();
        for s in 1..steps {
            let (_, pairs) = guided(at(s), &mut cursors)?;
            for (track, (_, q)) in tracks.iter_mut().zip(pairs) {
                track.push(q);
            }
        }
        for (track, (_, a1)) in tracks.iter_mut().zip(&ends) {
            track.push(a1.on);
        }
        let mut cursors = start;
        for s in spaced(&tracks, per, 0.5 * tol, at) {
            let f = at(s);
            let (station, pairs) = guided(f, &mut cursors)?;
            let (mid, dir) = middle(v0 + f * (v1 - v0))?;
            let map = station_map(&pairs, mid, dir, fit, size)?;
            out.push(station.iter().map(|c| with(c, &points(c).0.iter().map(|p| map.apply(*p)).collect::<Vec<_>>())).collect());
        }
    }
    out.push(sections.last().map(|s| s.curves.clone()).ok_or_else(|| fail("sections"))?);
    Ok(out)
}

/// A span's stations, as steps of the rails' `tracks`: the `per` even ones, or twice as many
/// (and one), still evenly spaced, where the loft would stray from a rail between the even ones
/// by more than `tol` (#13663); `at(s)` is step s as a fraction of the way through the span.
/// Between its stations the loft follows a cubic across them, so each rail's points at the even
/// stations and the sections are interpolated by the cubic at their fractions, and compared
/// with the rail's track at every step between.
///
/// The stations stay evenly spaced: closer stations in one gap than in the next make the skin
/// through them double back on itself, which the surface loft refuses. On the hull sides in the
/// tests, one extra station in the middle of a gap already does. The stations are doubled only
/// where every rail moves on by more than `tol` from each of them to the next.
fn spaced(tracks: &[Vec<Vec3>], per: usize, tol: f64, at: impl Fn(usize) -> f64) -> Vec<usize> {
    let even: Vec<usize> = (0..=per + 1).map(|q| q * TRACK_STEPS).collect();
    let params: Vec<f64> = even.iter().map(|&s| at(s)).collect();
    let strays = tracks.iter().any(|track| {
        let Some(pts) = even.iter().map(|&s| track.get(s).map(|p| [p.x, p.y, p.z, 1.0])).collect::<Option<Vec<[f64; 4]>>>() else { return false };
        // Interpolation through increasing fractions fails only on input the steps cannot hold;
        // the even stations then stay alone.
        let Ok(cubic) = CurveN::interpolate(&params, &pts, pts.len().saturating_sub(1).min(3)) else { return false };
        even.windows(2).any(|w| {
            (w[0] + 1..w[1])
                .any(|s| cubic.eval(at(s)).zip(track.get(s)).is_some_and(|(([x, y, z, h], _), q)| q.dist(Vec3::new(x / h, y / h, z / h)) > tol))
        })
    });
    // Each rail moves on from station to station: a rail that lies in a station's plane (a
    // stem drawn in the keel's plane, next to the keel) meets it where it met the station
    // before, and the stations would meet there.
    let half = TRACK_STEPS / 2;
    let moves = |step: usize| {
        tracks
            .iter()
            .all(|t| (0..TRACK_STEPS * (per + 1)).step_by(step).all(|s| t.get(s).zip(t.get(s + step)).is_some_and(|(a, b)| a.dist(*b) > tol)))
    };
    let step = if strays && moves(half) { half } else { TRACK_STEPS };
    (1..TRACK_STEPS * (per + 1) / step).map(|q| q * step).collect()
}

/// The sections' normals, turned so that the stations between two sections turn the right way
/// whichever way the sections' sketches face (#12031). `along` holds the sections' parameters
/// along the loft, and `direction(v)` gives the loft's direction there, if it has one.
///
/// Two sections whose planes are within 60° of each other turn the short way, from one normal
/// to the next. That holds where the loft doubles back, too. Between planes further apart, the
/// short way is a guess (at 90° there is none), so each normal faces along the loft. A normal
/// that the loft cannot decide, because its plane lies along the loft's direction or the loft
/// has no direction there, takes the short way from the section before it. The first section
/// takes it from the nearest section the loft decides.
pub(crate) fn section_normals(sections: &[Section], direction: impl Fn(f64) -> Option<Vec3>, along: &[f64]) -> Vec<Vec3> {
    let turn = |n: Vec3, to: Vec3| if n.dot(to) < 0.0 { n * -1.0 } else { n };
    let forward: Vec<Option<Vec3>> = sections
        .iter()
        .zip(along)
        .map(|(s, &v)| {
            let n = s.plane.normal();
            let facing = direction(v).map_or(0.0, |d| n.dot(d));
            (facing.abs() > 1e-3).then(|| if facing < 0.0 { n * -1.0 } else { n })
        })
        .collect();
    let mut normals: Vec<Vec3> = Vec::with_capacity(sections.len());
    for (s, decided) in sections.iter().zip(&forward) {
        let n = s.plane.normal();
        normals.push(match (normals.last(), decided) {
            (None, Some(f)) => *f,
            (None, None) => forward.iter().flatten().next().map_or(n, |f| turn(n, *f)),
            (Some(prev), Some(f)) if n.dot(*prev).abs() < 0.5 => *f,
            (Some(prev), _) => turn(n, *prev),
        });
    }
    normals
}

/// The rail `pts` from its end nearer `start` (the first section's middle), and where it meets
/// each section, on the section's curve within `tol`.
///
/// A rail runs from the first section to the last: it meets the last section at its end. In
/// between it meets each section where it first crosses the section's plane after the section
/// before. The end is not a crossing: a rail that lies in the last section's plane (a keel drawn
/// in the bow's sketch plane) crosses that plane all along, first at the far section, and meets the
/// section only where it ends.
fn rail(pts: &[Vec3], start: Vec3, sections: &[Section], tol: f64) -> Result<Rail> {
    let mut pts =
        crate::sweep_path::clean(pts).map_err(|_| KernelError::Invalid("a guide rail needs 2…100000 finite points along its length".into()))?;
    if let (Some(a), Some(b)) = (pts.first(), pts.last())
        && b.dist(start) < a.dist(start)
    {
        pts.reverse();
    }
    let not_reached = |i: usize| {
        KernelError::Invalid(format!(
            "a guide rail does not reach section {}: a rail has to run from the first section to the last, through every one",
            i + 1
        ))
    };
    let mut cursor = (0usize, 0.0f64);
    let mut anchors = Vec::with_capacity(sections.len());
    for (i, s) in sections.iter().enumerate() {
        let (plane, curves) = (&s.plane, &s.curves);
        let (hit, at) = if i + 1 == sections.len() {
            let end = *pts.last().ok_or_else(|| not_reached(i))?;
            if plane.height(end).abs() > tol {
                return Err(not_reached(i));
            }
            (end, (pts.len().saturating_sub(2), 1.0))
        } else {
            crate::sweep_path::rail_crossing(&pts, plane.origin, plane.normal(), cursor, tol).ok_or_else(|| not_reached(i))?
        };
        cursor = at;
        let (g, on, d) = nearest_on(curves, hit).ok_or_else(|| KernelError::Failed("loft rails: section".into()))?;
        if d > tol {
            return Err(KernelError::Invalid(format!(
                "a guide rail crosses section {}'s plane {d:.3} mm from its curves: a rail has to pass through every section",
                i + 1
            )));
        }
        anchors.push(Anchor { g, on, nudge: on - hit, at });
    }
    Ok(Rail { pts, anchors })
}

/// The point of a section (its curves) nearest `p`: as a wire parameter, the point, and how far
/// it is. Each curve is searched from its nearest of 256 even samples by golden-section search.
fn nearest_on(section: &[CurveN<4>], p: Vec3) -> Option<(f64, Vec3, f64)> {
    const SAMPLES: usize = 256;
    let mut best: Option<(f64, Vec3, f64)> = None;
    for (k, c) in section.iter().enumerate() {
        let d = |f: f64| point_at(c, f).map_or(f64::INFINITY, |q| q.dist(p));
        let Some(i) = (0..=SAMPLES).min_by(|a, b| d(*a as f64 / SAMPLES as f64).total_cmp(&d(*b as f64 / SAMPLES as f64))) else { continue };
        let (mut lo, mut hi) = ((i as f64 - 1.0).max(0.0) / SAMPLES as f64, (i as f64 + 1.0).min(SAMPLES as f64) / SAMPLES as f64);
        let g = 0.5 * (5f64.sqrt() - 1.0);
        for _ in 0..80 {
            let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
            if d(a) < d(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let f = 0.5 * (lo + hi);
        let q = point_at(c, f)?;
        if best.as_ref().is_none_or(|b| q.dist(p) < b.2) {
            best = Some((k as f64 + f, q, q.dist(p)));
        }
    }
    best
}

/// A wire parameter `f` of the way from `g0` to `g1`; round a closed wire of `n` edges the
/// shorter way.
pub(crate) fn interpolate_g(g0: f64, g1: f64, f: f64, n: usize, closed: bool) -> f64 {
    let n = n as f64;
    if !closed {
        return (g0 + f * (g1 - g0)).clamp(0.0, n);
    }
    let mut d = g1 - g0;
    if d > n / 2.0 {
        d -= n;
    } else if d < -n / 2.0 {
        d += n;
    }
    (g0 + f * d).rem_euclid(n)
}

/// The point at wire parameter `g` of a section's curves.
fn wire_point(section: &[CurveN<4>], g: f64) -> Result<Vec3> {
    let k = (g.floor().max(0.0) as usize).min(section.len().saturating_sub(1));
    section.get(k).and_then(|c| point_at(c, g - k as f64)).ok_or_else(|| KernelError::Failed("loft rails: section".into()))
}

/// The middle of a section: the centroid of its curves by length, ∫ x ds / ∫ ds, each knot
/// span by 8-point Gauss–Legendre. It is the section's shape alone, whatever control points
/// describe it: a circle's is its centre, however many pieces it is drawn in (a whole circle as
/// one closed rational curve repeats its first control point as its last, and the mean of the
/// control points sat a ninth of the radius towards it). `None` for a section with no length.
fn centroid(section: &[CurveN<4>]) -> Option<Vec3> {
    let (mut sum, mut length) = (Vec3::ZERO, 0.0);
    for c in section {
        let (lo, hi) = (*c.knots.get(c.degree)?, *c.knots.get(c.ctrl.len())?);
        for w in c.knots.windows(2) {
            let (a, b) = (w[0].max(lo), w[1].min(hi));
            if !(b > a) {
                continue;
            }
            let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
            for (x, wt) in solidcraft_geom::bspline::GAUSS_8 {
                let ([hx, hy, hz, hw], [dx, dy, dz, dw]) = c.eval(mid + half * x)?;
                if !(hw.abs() > 1e-300) {
                    return None;
                }
                let p = Vec3::new(hx / hw, hy / hw, hz / hw);
                // The rational point's derivative: (h' − p w') / w.
                let speed = ((Vec3::new(dx, dy, dz) - p * dw) * (1.0 / hw)).len();
                let ds = wt * half * speed;
                sum += p * ds;
                length += ds;
            }
        }
    }
    (length > 0.0 && sum.is_finite()).then(|| sum * (1.0 / length))
}

/// The move that puts a station's guided points onto the rails (`pairs`: each point with its
/// rail's point), the station's middle `middle`, the loft's direction there `dir` (unit); `size`
/// the loft's size. One rail: turned about `dir` and sized about the middle, shifted along `dir`;
/// two: carried with the first, turned and sized with the line between them, leaning along `dir`.
pub(crate) fn station_map(pairs: &[(Vec3, Vec3)], middle: Vec3, dir: Vec3, fit: RailFit, size: f64) -> Result<Affine> {
    let (from, to, a, r, two) = match pairs {
        [(p, q)] => (middle, middle, *p - middle, *q - middle, false),
        [(p1, q1), (p2, q2)] => (*p1, *q1, *p2 - *p1, *q2 - *q1, true),
        _ => return Err(KernelError::Invalid("a loft takes one or two guide rails".into())),
    };
    let flat = |v: Vec3| v - dir * v.dot(dir);
    let (ap, rp) = (flat(a), flat(r));
    let tiny = 1e-6 * size.max(1e-9);
    let (Some(u0), Some(u1)) = (ap.normalized().filter(|_| ap.len() > tiny), rp.normalized().filter(|_| rp.len() > tiny)) else {
        return Err(KernelError::Invalid(if two {
            "the two guide rails come together between the sections: the loft would shrink to nothing".into()
        } else {
            "a guide rail runs through the middle of the loft: it has to guide a point of the sections away from their middle".into()
        }));
    };
    // Turned about the loft's direction and sized as the sweep's rails size its profile.
    let mut m = crate::sweep_path::rail_frame(dir, u0, dir, u1, rp.len() / ap.len(), fit);
    // Whatever the rails ask along the loft: one rail shifts the station that far, two lean it,
    // in step with the way across.
    let lead = (r - a).dot(dir);
    if two {
        let lean = lead / ap.len();
        for (col, e) in m.iter_mut().zip([Vec3::X, Vec3::Y, Vec3::Z]) {
            *col += dir * (lean * e.dot(u0));
        }
    }
    let shift = if two { Vec3::ZERO } else { dir * lead };
    Ok(Affine { from, to: to + shift, m })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod unify_tests {
    use super::*;
    use solidcore_record::Vec2;

    fn plane(z: f64) -> Plane {
        Plane { origin: Vec3::new(0.0, 0.0, z), x: Vec3::X, y: Vec3::Y }
    }

    /// Each unified curve is the curve it was, point for point along its parameter.
    #[test]
    fn unified_curves_are_the_curves_they_were() {
        let half = [
            Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep: std::f64::consts::PI },
            Seg2::Line { a: Vec2::new(-10.0, 0.0), b: Vec2::new(10.0, 0.0) },
        ];
        let rect = [
            Seg2::Line { a: Vec2::new(-20.0, -10.0), b: Vec2::new(20.0, -10.0) },
            Seg2::Line { a: Vec2::new(20.0, -10.0), b: Vec2::new(20.0, 10.0) },
            Seg2::Line { a: Vec2::new(20.0, 10.0), b: Vec2::new(-20.0, 10.0) },
            Seg2::Line { a: Vec2::new(-20.0, 10.0), b: Vec2::new(-20.0, -10.0) },
        ];
        // Arc against line, line against line, quarter arc against third-of-a-turn arc.
        let pie = |sweep: f64| {
            let end = Vec2::new(10.0 * sweep.cos(), 10.0 * sweep.sin());
            [
                Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: 0.0, sweep },
                Seg2::Line { a: end, b: Vec2::ZERO },
                Seg2::Line { a: Vec2::ZERO, b: Vec2::new(10.0, 0.0) },
            ]
        };
        let cases: Vec<Vec<Section>> = vec![
            vec![
                Section::of(&plane(0.0), &half, vec![vec![0], vec![1]]).unwrap(),
                Section::of(&plane(40.0), &rect[..2], vec![vec![0], vec![1]]).unwrap(),
            ],
            vec![
                Section::of(&plane(0.0), &pie(std::f64::consts::FRAC_PI_2), vec![vec![0], vec![1], vec![2]]).unwrap(),
                Section::of(&plane(40.0), &pie(2.0 * std::f64::consts::PI / 3.0), vec![vec![0], vec![1], vec![2]]).unwrap(),
            ],
        ];
        for sections in &cases {
            let out = unified(sections).unwrap();
            for (s, u) in sections.iter().zip(&out) {
                for (c, d) in s.curves.iter().zip(&u.curves) {
                    assert_eq!(d.knots.len(), d.ctrl.len() + d.degree + 1, "{d:?}");
                    for k in 0..=100 {
                        let f = k as f64 / 100.0;
                        let (p, q) = (point_at(c, f).unwrap(), point_at(d, f).unwrap());
                        assert!(p.dist(q) < 1e-9, "at {f}: {p:?} vs {q:?}\n{c:?}\n{d:?}");
                    }
                }
                let lead = &out[0].curves;
                for (c, d) in lead.iter().zip(&u.curves) {
                    assert_eq!((c.degree, &c.knots, c.ctrl.len()), (d.degree, &d.knots, d.ctrl.len()));
                }
            }
        }
    }
}
