//! The foot blend (#15129, jyviko's definition for weld beads): a blend set by its legs, not
//! by a rolling ball. Its leader is the joint edge `e(t)` itself, the root line. At each `t`
//! the foot on support `S_i` is the point of `S_i` in the plane square to `e'(t)` through
//! `e(t)`, at the leg's distance `z_i` from `e(t)` on that face's side: two equations per foot,
//! `(F - e(t)) . e'(t) = 0` and `|F - e(t)| = z_i`, in the foot's two surface parameters. The
//! section (the chord, or the bead's arc with its sagitta) lies between the two feet in that
//! same plane. A leg so measured is what a leg gauge measures: straight from the root to the
//! toe, at every station, on any supports.
//!
//! On a curved support the circle of radius `z_i` in the station's plane can meet the surface's
//! section there twice; the foot is the one on the branch that leaves `e(t)` along the face
//! (the face's direction into itself from the edge, its binormal), found by Newton from the
//! point `e(t) + z_i b_i` dropped onto the support.
//!
//! This module is the station solve ([`FootBlend::station`]). Between two planes along a
//! straight edge the feet are the planes' lines at the legs' distances from the edge, exactly.
//! The certificate (one root in a certified box, Krawczyk), the definition kind in
//! `ProceduralBlend` and its record follow.
//!
//! Not upstream (solidcore's). No recursion.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId};

const OP: &str = "blend::foot_blend";

/// The most Newton steps a foot takes.
const NEWTON_STEPS: usize = 40;

/// One support of a foot blend: the face, its surface, and the edge use on the void's side
/// along which the face leaves the edge.
#[derive(Debug, Clone)]
pub struct FootSide {
    /// The face.
    pub face: FaceId,
    /// The face's surface.
    pub surface: Surface,
    /// The edge use of the joint edge on this face, on the void's side.
    pub edgeuse: EdgeuseId,
}

/// A foot blend's leader and supports: the joint edge's curve over its interval and the
/// edge's two faces.
#[derive(Debug, Clone)]
pub struct FootBlend {
    /// The joint edge.
    pub edge: EdgeId,
    /// The edge's curve, the leader.
    pub curve: Curve,
    /// The edge's interval on its curve.
    pub interval: Extent1d,
    /// The two supports, face 1 first.
    pub sides: [FootSide; 2],
}

/// One station of a foot blend: the root and the two feet, with each foot's surface
/// parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FootStation {
    /// The leader's parameter.
    pub t: f64,
    /// The root `e(t)`.
    pub root: Point3,
    /// The leader's unit tangent there, the station plane's normal.
    pub tangent: Vec3,
    /// The feet on support 1 and support 2.
    pub feet: [Point3; 2],
    /// Each foot's parameters on its support.
    pub uv: [Point2; 2],
}

impl FootBlend {
    /// The foot blend of `edge` of `target`: its curve and its two faces, through the edge
    /// uses on the void's side (whose faces' normals point out of the material). With
    /// `first` a face of the edge, that face is support 1.
    ///
    /// # Errors
    ///
    /// Native invalid input for an edge with no curve, or not between two faces on the void's
    /// side, or a face with no surface.
    pub fn of_edge(target: &Brep, edge_id: EdgeId, first: Option<FaceId>) -> Result<Self> {
        let curve = target
            .edge(edge_id)?
            .curve
            .clone()
            .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
        let interval = edge::interval(target, edge_id)?;
        let void = target.infinite_region();
        let mut sides: Vec<FootSide> = Vec::with_capacity(2);
        for eu in edge::edgeuses(target, edge_id, None, None)? {
            let Some(face) = eu1::face(target, eu)? else {
                continue;
            };
            let region = match eu1::shell(target, eu)? {
                Some(shell) => target.shell(shell)?.region,
                None => None,
            };
            if void.is_some() && region != void {
                continue;
            }
            if sides.iter().any(|s| s.face == face) {
                continue;
            }
            let surface = target
                .face(face)?
                .surface
                .clone()
                .ok_or_else(|| SolidError::invalid_input(OP, "a face with no surface"))?;
            sides.push(FootSide {
                face,
                surface,
                edgeuse: eu,
            });
        }
        let [a, b] = <[FootSide; 2]>::try_from(sides)
            .map_err(|_| SolidError::invalid_input(OP, "an edge between two faces"))?;
        let sides = if first == Some(b.face) {
            [b, a]
        } else {
            [a, b]
        };
        Ok(Self {
            edge: edge_id,
            curve,
            interval,
            sides,
        })
    }

    /// The station at the leader's parameter `t`, with the legs `legs` (support 1's, support
    /// 2's): each foot on its support, in the plane square to the leader through the root, at
    /// its leg's distance from the root, on the branch that leaves the root along the face.
    /// `target` is the body the edge is on; it is only read: it is mutable for the binormal's
    /// evaluation, which is asked in 3D only and so builds no uv trim curve.
    ///
    /// # Errors
    ///
    /// Native invalid input for a leg that is not finite and positive; a failure where a foot's
    /// Newton does not converge or ends on the other branch (no foot on the face's side); those
    /// of the evaluations.
    pub fn station(
        &self,
        k: &Kernel,
        target: &mut Brep,
        t: f64,
        legs: [f64; 2],
    ) -> Result<FootStation> {
        if !legs.iter().all(|z| z.is_finite() && *z > 0.0) {
            return Err(SolidError::invalid_input(OP, "a leg that is not positive"));
        }
        let at = self.curve.evaluate(k, t, 1, false, false)?;
        let (Some(&root), Some(&d)) = (at.first(), at.get(1)) else {
            return Err(SolidError::blend_missing(OP, "the leader's point"));
        };
        let tangent = d.unitize()?;
        // to rounding: the feet are exact at their stations
        let tol =
            1e-11 * (1.0 + root.max_dimension() + legs.iter().fold(0.0_f64, |m, z| m.max(*z)));
        let mut feet = [Point3::ZERO; 2];
        let mut uvs = [Point2::new(0.0, 0.0); 2];
        for (i, side) in self.sides.iter().enumerate() {
            let z = legs.get(i).copied().unwrap_or(0.0);
            let into = evaluate_binormal(
                k,
                target,
                side.edgeuse,
                t,
                true,
                &BinormalOptions::default(),
            )?;
            let b = into.binormal - tangent * into.binormal.dot(tangent);
            let b = b.unitize()?;
            let (foot, uv) = foot_on(k, &side.surface, root, tangent, b, z, into.uv, tol)?;
            if let (Some(f), Some(w)) = (feet.get_mut(i), uvs.get_mut(i)) {
                *f = foot;
                *w = uv;
            }
        }
        Ok(FootStation {
            t,
            root,
            tangent,
            feet,
            uv: uvs,
        })
    }
}

/// The foot on `surface` in the plane through `root` square to the unit `tangent`, at
/// distance `z` from `root`, on the branch along the unit direction `b` (square to `tangent`,
/// into the face): Newton on `F1 = (P - root) . tangent`, `F2 = |P - root|^2 - z^2` in the
/// surface's parameters, from `root + z b` dropped onto the surface near `near`.
///
/// # Errors
///
/// A failure where the drop fails, the Jacobian is singular, Newton does not converge in
/// [`NEWTON_STEPS`], or the foot ends off the face's side (`(P - root) . b <= 0`); those of the
/// evaluations.
#[allow(clippy::too_many_arguments)]
fn foot_on(
    k: &Kernel,
    surface: &Surface,
    root: Point3,
    tangent: Vec3,
    b: Vec3,
    z: f64,
    near: Point2,
    tol: f64,
) -> Result<(Point3, Point2)> {
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let seed = root + b * z;
    let dropped = surface.drop_point::<crate::surface::surface::p4::NoObject>(
        k,
        seed,
        &domain,
        &DropPointOptions {
            guess_uv: Some(near),
            ..DropPointOptions::default()
        },
    )?;
    if !dropped.success {
        return Err(SolidError::blend_failed(
            OP,
            "the foot's seed does not drop onto its support",
        ));
    }
    let mut uv = dropped.result_uv;
    for _ in 0..NEWTON_STEPS {
        let d = surface.evaluate(k, uv, 1, 1, EvaluateFlags::default())?;
        let (Some(&p), Some(&du), Some(&dv)) = (
            d.first().and_then(|r| r.first()),
            d.get(1).and_then(|r| r.first()),
            d.first().and_then(|r| r.get(1)),
        ) else {
            return Err(SolidError::blend_missing(OP, "the support's derivatives"));
        };
        let w = p - root;
        let f1 = w.dot(tangent);
        let f2 = w.dot(w) - z * z;
        if f1.abs() <= tol && f2.abs() <= tol * z {
            if w.dot(b) <= 0.0 {
                return Err(SolidError::blend_failed(
                    OP,
                    "no foot on the face's side at its leg",
                ));
            }
            return Ok((p, uv));
        }
        let (a11, a12) = (du.dot(tangent), dv.dot(tangent));
        let (a21, a22) = (2.0 * w.dot(du), 2.0 * w.dot(dv));
        let det = a11 * a22 - a12 * a21;
        if det.abs() <= f64::EPSILON * (a11.abs() + a12.abs()) * (a21.abs() + a22.abs()) {
            return Err(SolidError::blend_failed(
                OP,
                "the foot's equations are singular",
            ));
        }
        let su = (f1 * a22 - f2 * a12) / det;
        let sv = (a11 * f2 - a21 * f1) / det;
        uv = Point2::new(uv.x - su, uv.y - sv);
    }
    Err(SolidError::blend_failed(
        OP,
        "the foot's Newton does not converge",
    ))
}

/// The fit tolerance a weld bead's section face is built to: 0.01 mm, far below what a weld
/// can be made or measured to. It is checked at samples (the midpoints between stations,
/// [`FootBlend::section_face`]), not proved: a sampled check, not a bound, until the face's
/// certificate follows. The feet themselves are solved to rounding at their stations
/// (Newton converges quadratically, so that costs a step or two); this tolerance sets how
/// many stations the face between them needs, which is where the time goes.
pub const WELD_FIT_TOLERANCE: f64 = 0.01;

/// The most times [`FootBlend::section_face`] doubles its stations.
const REFINEMENTS: usize = 9;

impl FootBlend {
    /// The section face of the foot blend with the legs `legs` and the bead's `sagitta`
    /// (`0`: the chord, `> 0` convex, `< 0` concave): at each station the chord between the
    /// two feet, or the circular arc between them at the sagitta in the station's plane
    /// (bulging away from the root for a positive sagitta), and through those sections a
    /// rational net, `u` the leader's parameter over the edge's interval, `v` from foot 1 (0)
    /// to foot 2 (1). The arcs are interpolated in homogeneous form (cubic in `u`) at stations
    /// on each of the leader's knots (at least four), doubled between them until, at the
    /// midpoint between each two stations, the net holds the feet, the section's middle and
    /// the root within `tol`. Exact at its stations; between them that is a **sampled** check
    /// at one point per interval, not a bound: nothing is certified yet.
    ///
    /// # Errors
    ///
    /// Those of [`Self::station`] at any station; native invalid input for an arc that would
    /// leave a foot into its face (a concave sagitta past what the station's toe angles
    /// allow); a failure where the net does not hold within `tol` after its refinements;
    /// those of the arcs and the interpolation.
    pub fn section_face(
        &self,
        k: &Kernel,
        target: &mut Brep,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
    ) -> Result<Surface> {
        let (lo, hi) = (self.interval.min(), self.interval.max());
        Ok(self.segment(k, target, legs, sagitta, tol, lo, hi)?.face)
    }

    /// [`Self::section_face`] over the leader's parameters `a..b` inside its interval, with
    /// its sides (the feet's curves) and the root's curve through the same stations.
    ///
    /// # Errors
    ///
    /// Those of [`Self::section_face`].
    #[allow(clippy::too_many_arguments)]
    fn segment(
        &self,
        k: &Kernel,
        target: &mut Brep,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
        a: f64,
        b: f64,
    ) -> Result<Segment> {
        let mut breaks = vec![a];
        breaks.extend(
            self.breaks()?
                .into_iter()
                .filter(|&x| x > a + 1e-9 * (b - a) && x < b - 1e-9 * (b - a)),
        );
        breaks.push(b);
        // at least four stations, the cubic's least
        let spans = breaks.len().saturating_sub(1).max(1);
        let mut count = 3_usize.div_ceil(spans).max(1);
        for _ in 0..REFINEMENTS {
            // each span between the leader's knots split evenly: a station on every knot, where
            // the leader may turn less smoothly
            let mut stations = Vec::new();
            for pair in breaks.windows(2) {
                let &[lo, hi] = pair else { continue };
                stations.extend((0..count).map(|i| lo + (hi - lo) * i as f64 / count as f64));
            }
            if let Some(&last) = breaks.last() {
                stations.push(last);
            }
            let mut rows = Vec::with_capacity(stations.len());
            let mut roots = Vec::with_capacity(stations.len());
            for &t in &stations {
                let (row, root) = self.section_rows(k, target, t, legs, sagitta)?;
                rows.push(row);
                roots.push(root);
            }
            let (face, feet) = net_through_rows(&stations, &rows, sagitta)?;
            let root = through_at(&roots, &stations)?;
            // sampled, not proved: at the midpoint between each two stations, the feet, the
            // section's middle and the root
            let mut worst = 0.0_f64;
            for pair in stations.windows(2) {
                let &[lo, hi] = pair else { continue };
                let t = 0.5 * (lo + hi);
                let ((row, _), want_root) = self.section_rows(k, target, t, legs, sagitta)?;
                let (Some(&first), Some(&last)) = (row.first(), row.last()) else {
                    continue;
                };
                let middle = section_middle(&row, sagitta);
                for (v, want) in [(0.0, first), (0.5, middle), (1.0, last)] {
                    let got = face.evaluate_point(k, Point2::new(t, v))?;
                    worst = worst.max(got.distance(want));
                }
                worst = worst.max(root.evaluate_point(t)?.distance(want_root));
            }
            if worst <= tol {
                return Ok(Segment { face, feet, root });
            }
            count *= 2;
        }
        Err(SolidError::blend_failed(
            OP,
            "the section face does not hold its feet and sections within the tolerance",
        ))
    }

    /// The leader's interval split at its curve's knots inside it (a B-spline leader's; any
    /// other has no knots), ends included, ascending.
    ///
    /// # Errors
    ///
    /// Those of the knots.
    fn breaks(&self) -> Result<Vec<f64>> {
        use crate::curve::curve::p2::CurveKind;
        let (lo, hi) = (self.interval.min(), self.interval.max());
        let mut breaks = vec![lo];
        if let CurveKind::BSpline(c) = self.curve.kind() {
            let (knots, _) = c.knots(None)?;
            let gap = 1e-9 * (hi - lo);
            breaks.extend(knots.into_iter().filter(|&x| x > lo + gap && x < hi - gap));
        }
        breaks.push(hi);
        Ok(breaks)
    }

    /// One station's section as control rows across, and its root: the chord's two feet
    /// (weights 1), or the arc's five rows of [`BeadArc::rows`] with their weights.
    ///
    /// [`BeadArc::rows`]: crate::surface::procedural_blend::bead::BeadArc::rows
    fn section_rows(
        &self,
        k: &Kernel,
        target: &mut Brep,
        t: f64,
        legs: [f64; 2],
        sagitta: f64,
    ) -> Result<(SectionRow, Point3)> {
        use crate::surface::procedural_blend::bead::BeadArc;
        let s = self.station(k, target, t, legs)?;
        let [a, b] = s.feet;
        if sagitta == 0.0 {
            return Ok(((vec![a, b], vec![1.0, 1.0]), s.root));
        }
        let convex = (a + (b - a) * 0.5) - s.root;
        let arc = BeadArc::new(a, b, s.tangent, convex, sagitta)?;
        // the arc leaves each foot into the open side of its face: along the face's normal
        // turned towards the other foot (a concave arc past the station's bound would dip
        // into the face there)
        let leaving = [
            (0, arc.tangent(0.0), b - a),
            (1, arc.tangent(arc.turn) * -1.0, a - b),
        ];
        for (i, out, towards) in leaving {
            let (Some(side), Some(&uv)) = (self.sides.get(i), s.uv.get(i)) else {
                continue;
            };
            let d = side
                .surface
                .evaluate(k, uv, 1, 1, EvaluateFlags::default())?;
            let (Some(&du), Some(&dv)) = (
                d.get(1).and_then(|r| r.first()),
                d.first().and_then(|r| r.get(1)),
            ) else {
                return Err(SolidError::blend_missing(OP, "the support's derivatives"));
            };
            let mut n = du.cross(dv);
            if n.dot(towards) < 0.0 {
                n *= -1.0;
            }
            if out.dot(n) < 0.0 {
                return Err(SolidError::invalid_input(
                    OP,
                    "a concave bead cuts into a face at a station (its sagitta is past the bound there)",
                ));
            }
        }
        let (row, w) = arc.rows();
        Ok(((row.to_vec(), w.to_vec()), s.root))
    }
}

/// One station's section across: its control points and their weights.
type SectionRow = (Vec<Point3>, Vec<f64>);

/// One segment of a foot blend over `a..b`: its section face, the face's two sides (the
/// feet's curves, foot 1's and foot 2's) and the root's curve, all through the same stations.
struct Segment {
    face: Surface,
    feet: [BSplineCurve; 2],
    root: BSplineCurve,
}

/// The cubic through `points` at the parameters `stations`.
///
/// # Errors
///
/// Those of the interpolation.
fn through_at(points: &[Point3], stations: &[f64]) -> Result<BSplineCurve> {
    use crate::nurbs::crv_fit::p1::ParamMethod;
    BSplineCurve::create_interpolating_curve_with_params(
        ParamMethod::ChordLength,
        3,
        3,
        points,
        &[],
        None,
        false,
        Some(stations),
    )
}

/// The middle across of a section row: the chord's midpoint, or the arc's apex (its middle
/// control point).
fn section_middle(row: &[Point3], sagitta: f64) -> Point3 {
    if sagitta == 0.0 {
        match (row.first(), row.last()) {
            (Some(&a), Some(&b)) => a + (b - a) * 0.5,
            _ => Point3::ZERO,
        }
    } else {
        row.get(2).copied().unwrap_or(Point3::ZERO)
    }
}

/// The rational net through the section rows at `stations`: each row's control points in
/// homogeneous form interpolated cubic along `u` at the stations, the weights too, then
/// divided back.
fn net_through_rows(
    stations: &[f64],
    rows: &[SectionRow],
    sagitta: f64,
) -> Result<(Surface, [BSplineCurve; 2])> {
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
    use crate::nurbs::b_spline_surface::BSplineSurface;
    let across = if sagitta == 0.0 { 2 } else { 5 };
    let mut columns: Vec<Vec<Point3>> = vec![Vec::with_capacity(stations.len()); across];
    let mut weights: Vec<Vec<Point3>> = vec![Vec::with_capacity(stations.len()); across];
    for (points, w) in rows {
        for (j, (column, wcol)) in columns.iter_mut().zip(weights.iter_mut()).enumerate() {
            let (Some(&p), Some(&w)) = (points.get(j), w.get(j)) else {
                return Err(SolidError::blend_missing(OP, "a section's row"));
            };
            column.push(Point3::new(p.x * w, p.y * w, p.z * w));
            wcol.push(Point3::new(w, 0.0, 0.0));
        }
    }
    let through = |points: &[Point3]| through_at(points, stations);
    let mut curves = Vec::with_capacity(across);
    let mut wcurves = Vec::with_capacity(across);
    for (column, wcol) in columns.iter().zip(&weights) {
        curves.push(through(column)?);
        wcurves.push(through(wcol)?);
    }
    let first = curves
        .first()
        .ok_or_else(|| SolidError::blend_missing(OP, "a section's column"))?;
    // the net's sides v = 0 and v = 1: the feet's columns, of weight 1
    let sides = [
        first.clone(),
        curves
            .last()
            .ok_or_else(|| SolidError::blend_missing(OP, "a section's column"))?
            .clone(),
    ];
    let (u_knots, u_mults) = first.knots(None)?;
    let count = first.control_points()?.len();
    let mut points = Vec::with_capacity(count * across);
    let mut net_weights = Vec::with_capacity(count * across);
    for i in 0..count {
        for (c, wc) in curves.iter().zip(&wcurves) {
            let p = c
                .control_points()?
                .get(i)
                .copied()
                .ok_or_else(|| SolidError::blend_missing(OP, "a column's control point"))?;
            let w = wc
                .control_points()?
                .get(i)
                .map(|c| c.x)
                .filter(|w| *w > 0.0)
                .ok_or_else(|| {
                    SolidError::blend_failed(OP, "a section's weight is not positive")
                })?;
            points.push(Point3::new(p.x / w, p.y / w, p.z / w));
            net_weights.push(w);
        }
    }
    let (v_degree, v_mults, v_knots): (usize, &[usize], &[f64]) = if sagitta == 0.0 {
        (1, &[2, 2], &[0.0, 1.0])
    } else {
        (2, &[3, 2, 3], &[0.0, 0.5, 1.0])
    };
    let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 3,
        v_degree,
        control_points: &points,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &u_mults,
        v_multiplicities: v_mults,
        u_knots: &u_knots,
        v_knots,
        knot_type: KnotType::Unspecified,
        weights: Some(&net_weights),
        uv_domain: None,
    })?;
    Ok((Surface::from(net), sides))
}

/// How many segments a closed joint's bead is built in: each face then runs less than a full
/// turn round it, so no face needs a seam.
const CLOSED_SEGMENTS: usize = 4;

/// The points along a rung, root excluded: the share of each leg.
const RUNG_SHARES: [f64; 4] = [0.25, 0.5, 0.75, 1.0];

impl FootBlend {
    /// The weld bead's body (#15129): the section face, a strip on each support between the
    /// root and its foot, and, where the joint edge is open, a planar end cap at each end in
    /// the station's plane, stitched into a solid of its own. A closed joint (a branch round a
    /// tube) is built in [`CLOSED_SEGMENTS`] segments, each face less than a turn. `target` is
    /// only read: no face, edge or vertex of it is added, changed or removed.
    ///
    /// # Errors
    ///
    /// Those of [`Self::section_face`] and of the curves and faces; a failure where the faces
    /// do not close into a solid.
    pub fn body(
        &self,
        k: &mut Kernel,
        target: &mut Brep,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
    ) -> Result<Brep> {
        let mut body = Brep::new_with_infinite_region()?;
        body.set_editing_enabled(true);
        self.add_faces(k, target, &mut body, legs, sagitta, tol)?;
        close_bead_body(k, &mut body, tol)?;
        Ok(body)
    }

    /// The faces of [`Self::body`], added to `body` unstitched (several beads go into one
    /// body before it is closed with [`close_bead_body`]).
    ///
    /// # Errors
    ///
    /// Those of [`Self::section_face`] and of the curves and faces.
    pub fn add_faces(
        &self,
        k: &mut Kernel,
        target: &mut Brep,
        body: &mut Brep,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
    ) -> Result<()> {
        use crate::blend::fillet_cliff_overflow::{face_of_loop, LoopCurve};
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::surface::plane::Plane;

        let (lo, hi) = (self.interval.min(), self.interval.max());
        let (start, end) = edge::start_end_vertices(target, self.edge)?;
        let closed = start.is_some() && start == end;
        let pieces = if closed { CLOSED_SEGMENTS } else { 1 };
        let cuts: Vec<f64> = (0..=pieces)
            .map(|i| lo + (hi - lo) * i as f64 / pieces as f64)
            .collect();
        let same = |curve: &BSplineCurve| LoopCurve {
            curve: Curve::from(curve.clone()),
            orient: OrientType::Same,
        };
        let opposite = |curve: &BSplineCurve| LoopCurve {
            curve: Curve::from(curve.clone()),
            orient: OrientType::Opposite,
        };
        // the cuts' rungs (one per support) and sections, shared by the faces either side
        let mut rungs: Vec<[BSplineCurve; 2]> = Vec::with_capacity(cuts.len());
        let mut sections: Vec<BSplineCurve> = Vec::with_capacity(cuts.len());
        for &t in &cuts {
            let (r1, r2, section) = self.cut_curves(k, target, t, legs, sagitta)?;
            rungs.push([r1, r2]);
            sections.push(section);
        }
        for (j, pair) in cuts.windows(2).enumerate() {
            let &[a, b] = pair else { continue };
            let seg = self.segment(k, target, legs, sagitta, tol, a, b)?;
            let (Some(ra), Some(rb), Some(sa), Some(sb)) = (
                rungs.get(j),
                rungs.get(j + 1),
                sections.get(j),
                sections.get(j + 1),
            ) else {
                return Err(SolidError::blend_missing(OP, "a cut's curves"));
            };
            let [f1, f2] = &seg.feet;
            // the section face: foot 1 along, the far section, foot 2 back, the near section
            face_of_loop(
                k,
                body,
                seg.face.clone(),
                vec![same(f1), same(sb), opposite(f2), opposite(sa)],
            )?;
            // the strips: the root along, the far rung out, the foot back, the near rung in
            for (i, (side, foot)) in self.sides.iter().zip([f1, f2]).enumerate() {
                let (Some(near), Some(far)) = (ra.get(i), rb.get(i)) else {
                    continue;
                };
                face_of_loop(
                    k,
                    body,
                    side.surface.clone(),
                    vec![same(&seg.root), same(far), opposite(foot), opposite(near)],
                )?;
            }
        }
        // the end caps of an open joint, in the stations' planes
        if !closed {
            for (end, t) in [(0_usize, lo), (cuts.len() - 1, hi)] {
                let (Some([r1, r2]), Some(section)) = (rungs.get(end), sections.get(end)) else {
                    continue;
                };
                let s = self.station(k, target, t, legs)?;
                let plane = Surface::from(Plane::from_normal(s.root, s.tangent));
                face_of_loop(k, body, plane, vec![same(r1), same(section), opposite(r2)])?;
            }
        }
        Ok(())
    }

    /// At the leader's parameter `t`: each support's rung (its section by the station's plane
    /// from the root to its foot, through points at shares of the leg, exact in that plane)
    /// and the section between the feet (the chord, or the arc as a rational quadratic with
    /// [`BeadArc::rows`]' points and weights).
    ///
    /// [`BeadArc::rows`]: crate::surface::procedural_blend::bead::BeadArc::rows
    ///
    /// # Errors
    ///
    /// Those of the stations, the rows and the curves.
    fn cut_curves(
        &self,
        k: &Kernel,
        target: &mut Brep,
        t: f64,
        legs: [f64; 2],
        sagitta: f64,
    ) -> Result<(BSplineCurve, BSplineCurve, BSplineCurve)> {
        use crate::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};
        let root = self.station(k, target, t, legs)?.root;
        let mut rung_points: [Vec<Point3>; 2] = [vec![root], vec![root]];
        for share in RUNG_SHARES {
            let scaled = legs.map(|z| z * share);
            let s = self.station(k, target, t, scaled)?;
            for (points, foot) in rung_points.iter_mut().zip(s.feet) {
                points.push(foot);
            }
        }
        let rung = |points: &[Point3]| {
            BSplineCurve::create_interpolating_curve_with_params(
                crate::nurbs::crv_fit::p1::ParamMethod::ChordLength,
                3,
                3,
                points,
                &[],
                None,
                false,
                None,
            )
        };
        let [p1, p2] = &rung_points;
        let (r1, r2) = (rung(p1)?, rung(p2)?);
        let ((row, weights), _) = self.section_rows(k, target, t, legs, sagitta)?;
        let (degree, mults, knots): (usize, &[usize], &[f64]) = if sagitta == 0.0 {
            (1, &[2, 2], &[0.0, 1.0])
        } else {
            (2, &[3, 2, 3], &[0.0, 0.5, 1.0])
        };
        let section = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree,
            control_points: &row,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: mults,
            knots,
            knot_type: KnotType::Unspecified,
            weights: if sagitta == 0.0 { None } else { Some(&weights) },
            trim: None,
        })?;
        Ok((r1, r2, section))
    }
}

/// Closes the weld bead faces added to `body` ([`FootBlend::add_faces`]): stitched within
/// `tol` (or the body's own tolerance, if larger) into closed shells, oriented as a solid, and
/// its geometry named by its own topology, a body of its own.
///
/// # Errors
///
/// A failure where the faces do not close into a manifold solid; those of the stitching and
/// the orientation.
pub fn close_bead_body(k: &mut Kernel, body: &mut Brep, tol: f64) -> Result<()> {
    use crate::topology::brep::p3::{orient_trimmed_surfaces, with_kernel_callbacks};
    use crate::topology::topology_traverser::Marks;
    let tolerance = body.tolerance().max(tol);
    let stitched = crate::topology::brep::p4::stitch_faces(k, body, tolerance, None, false)?;
    let mut open = stitched.lamina_edges != 0;
    if !open {
        let oriented = with_kernel_callbacks(k, body, |k, b, env| {
            let mut marks = Marks::default();
            orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)
        })?;
        open = oriented.maybe_not_closed_solid;
    }
    if open || !crate::topology::brep::p2::is_manifold_solid(body)? {
        return Err(SolidError::blend_failed(
            OP,
            "the weld bead's faces do not close into a solid",
        ));
    }
    body.use_arena_naming()
}
