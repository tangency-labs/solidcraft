//! Extend (Surface › Extend): boundary edges of a surface body pushed outward across their
//! face, by a distance, up to a plane or a face, or (natural only) back to the surface's own
//! bounds. Three ways to continue past an edge, each SMLib's extension of the surface
//! (`smlib_extend_sheet`, `SmBSplineSurface::CreateExtendedSurface`):
//!
//! - natural: the surface itself continued (a B-spline's end spans as their own polynomials,
//!   which keeps a plane, a cylinder or any conic section on itself);
//! - tangent: continued tangent to the surface across the edge (G1);
//! - curvature: continued keeping the surface's curvature across the edge too (G2).
//!
//! The face is extended where it is its surface's whole parameter box and the edge is a whole
//! side of it: the box grows past that side, the sides next to it continue along the surface's
//! parameter lines, and the face stays one face on the extended surface. Edges picked on two
//! sides grow the box on both, meeting at the box's corner. Up to a target, the face is extended
//! past it and trimmed back where the target meets it ([`crate::trim_keep`]: SMLib's exact
//! intersection). Back to the bounds is the face untrimmed to its surface's box
//! ([`crate::untrim`]). A plane's straight edges pushed out with their neighbours lengthened
//! keep the exact planar extension ([`crate::surfaces::extend`]).
//!
//! Refused rather than built: edges between two faces, edges of holes, and (not supported yet)
//! an edge that is not a whole side of its surface's box (a trimmed edge, whose extension is no
//! curve SMLib holds exactly), or a body of more than one face.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self, eval::CurveEval};
use solidcraft_smlib::trim::Continuation;

use crate::body::Body;
use crate::history::{Change, FaceOrigin};
use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
use crate::surface_trim::TrimKeep;
use crate::{KernelError, Result, SplitTool};

/// Most edge picks.
const MAX_PICKS: usize = 10_000;
/// How far past its edge, in body sizes, an extension up to a target runs before it is trimmed
/// back to the target: a target farther than that is not reached.
const REACH: f64 = 50.0;
/// Samples along an edge that find the edge nearest a pick.
const PICK_SAMPLES: usize = 32;

/// How a surface continues past its edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtendMode {
    /// The surface itself, continued.
    #[default]
    Natural,
    /// Continued tangent to the surface across the edge (G1).
    Tangent,
    /// Continued keeping the surface's curvature across the edge (G2).
    Curvature,
}

/// How far an extension goes.
#[derive(Clone, Copy, Debug)]
pub enum ExtendLimit<'a> {
    /// This far from the edge, measured across the surface.
    Distance(f64),
    /// To where it meets a plane, or the surface of a face (extended).
    UpTo(&'a SplitTool<'a>),
    /// Back to the surface's own bounds (natural only), undoing a trim: a free-form surface's
    /// parameter box, an analytic surface's the box of the face's own span.
    Bounds,
}

/// What [`extend_surface`] does.
#[derive(Clone, Copy, Debug)]
pub struct ExtendOptions<'a> {
    pub mode: ExtendMode,
    pub limit: ExtendLimit<'a>,
    /// Lengthen the edges next to an extended edge to meet the extension. On a face that is its
    /// surface's whole box those edges are parameter lines, which the extension continues.
    pub neighbours: bool,
}

/// The boundary edges of `body` picked by points on them, extended as `opts` says. The result is
/// a surface body whose history is over `[body]`: an extended face is the face it was (bounded
/// differently) and its edges the edges they continue.
pub fn extend_surface(body: &Body, at: &[Vec3], opts: &ExtendOptions) -> Result<Body> {
    body.require_brep("extend")?;
    if at.is_empty() {
        return Err(KernelError::Invalid("pick the edges to extend".into()));
    }
    if at.len() > MAX_PICKS {
        return Err(KernelError::Invalid("too many edges picked".into()));
    }
    if at.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("an edge point is not finite".into()));
    }
    match opts.limit {
        ExtendLimit::Distance(d) if !(d.is_finite() && d > 0.0 && d <= 1e6) => {
            return Err(KernelError::Invalid("the extension distance must be positive".into()));
        }
        ExtendLimit::Bounds if opts.mode != ExtendMode::Natural => {
            return Err(KernelError::Invalid("only a natural extension goes back to the surface's bounds".into()));
        }
        _ => {}
    }
    let record = crate::native::brep_record(body)?;
    // A plane's straight edges pushed out with their neighbours lengthened: the exact planar
    // extension (what designs saved before the other modes build).
    if let (ExtendMode::Natural, ExtendLimit::Distance(d), true) = (opts.mode, opts.limit, opts.neighbours)
        && plain_planar(&record)
    {
        return crate::surfaces::extend(body, at, d);
    }
    let faces = picked_faces(&record, body.size(), at)?;
    match opts.limit {
        ExtendLimit::Bounds => back_to_bounds(body, &faces),
        ExtendLimit::Distance(d) => continued(body, at, d, opts.mode),
        ExtendLimit::UpTo(tool) => up_to(body, at, tool, opts.mode),
    }
}

/// One planar face, one loop of straight edges: what [`crate::surfaces::extend`] takes.
fn plain_planar(record: &brep::Brep) -> bool {
    let [face] = record.faces.as_slice() else { return false };
    let [brep::Loop::Edges { edges }] = face.loops.as_slice() else { return false };
    matches!(face.surface, brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. })
        && edges.iter().all(|u| record.edges.get(u.edge as usize).is_some_and(|e| matches!(e.curve, brep::Curve::Line { .. })))
}

/// The faces (in face order, each once) of the boundary edges the points pick: for each point,
/// the record edge nearest it (by samples along each edge), within a twentieth of the body's
/// size. An edge two faces share, or one of a hole, is refused.
fn picked_faces(record: &brep::Brep, size: f64, at: &[Vec3]) -> Result<Vec<usize>> {
    let curves: Vec<CurveEval> = record
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| CurveEval::new(e.parameters()).map_err(|err| KernelError::Failed(format!("extend: edge {i}'s curve: {err}"))))
        .collect::<Result<_>>()?;
    let reach = size * 0.05 + 1e-6;
    let mut faces = Vec::new();
    for p in at {
        let mut best: Option<(f64, usize)> = None;
        for (i, (e, c)) in record.edges.iter().zip(&curves).enumerate() {
            let [t0, t1] = e.interval;
            for k in 0..=PICK_SAMPLES {
                let Some(q) = c.point(t0 + (t1 - t0) * k as f64 / PICK_SAMPLES as f64) else { continue };
                let d = q.dist(*p);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, i));
                }
            }
        }
        let edge = match best {
            Some((d, i)) if d <= reach => record.edges.get(i),
            _ => None,
        }
        .ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        let [only] = edge.uses.as_slice() else {
            return Err(KernelError::Invalid("an edge between two faces can't be extended: pick edges on the surface's boundary".into()));
        };
        // A face's first loop is its outer one (as SMLib keeps them).
        if only.r#loop != 0 {
            return Err(KernelError::Invalid("not supported yet: extending an edge of a hole (Untrim takes holes away)".into()));
        }
        let face = only.face as usize;
        if !faces.contains(&face) {
            faces.push(face);
        }
    }
    faces.sort_unstable();
    Ok(faces)
}

/// The one face of `body` continued `distance` past each picked edge, as `mode` says.
fn continued(body: &Body, at: &[Vec3], distance: f64, mode: ExtendMode) -> Result<Body> {
    if body.face_count() != 1 {
        return Err(KernelError::Invalid(format!(
            "not supported yet: extending a surface of more than one face (this one has {})",
            body.face_count()
        )));
    }
    let how = match mode {
        ExtendMode::Natural => Continuation::Natural,
        ExtendMode::Tangent => Continuation::Tangent,
        ExtendMode::Curvature => Continuation::Curvature,
    };
    let reach = body.size() * 0.05 + 1e-6;
    let (sb, ob) = smlib_body_ordered(body)?;
    let mut sheet = sb.tagged(0).map_err(|e| failed("tag", e))?;
    for p in at {
        sheet = sheet.extend_sheet([p.x, p.y, p.z], reach, distance, how).map_err(|e| match e {
            solidcraft_smlib::Error::Unsupported => KernelError::Invalid(
                "not supported yet: extending an edge that is not a whole side of its surface's parameter box (a trimmed edge)".into(),
            ),
            solidcraft_smlib::Error::NotExact => KernelError::Invalid("the extension would run past a pole of its surface or wrap round it".into()),
            solidcraft_smlib::Error::NoMatch { .. } => KernelError::Invalid(format!("no edge at {p:?}")),
            e => failed("extend", e),
        })?;
    }
    let (out, order) = body_from_smlib_ordered(&sheet)?;
    let history = match (ob, order) {
        (Some(ob), Some(or)) => sheet.history().map_err(|e| failed("history", e))?.history(&[ob], &or).ok().map(|mut h| {
            // The extended face is the face it was, bounded differently.
            for f in &mut h.faces {
                if f.origins.contains(&FaceOrigin::Face { operand: 0, face: 0 }) {
                    f.change = Change::Modified;
                }
            }
            h
        }),
        _ => None,
    };
    Ok(out.with_checked_history(history, &crate::record::counts(&[body])))
}

/// The face extended past the target and trimmed back where the target meets it, keeping the
/// piece the face itself lies in.
fn up_to(body: &Body, at: &[Vec3], tool: &SplitTool, mode: ExtendMode) -> Result<Body> {
    let inside = crate::surface_untrim::face_point(body, 0)?;
    let long = continued(body, at, body.size().max(1e-6) * REACH, mode)?;
    let cut = crate::trim_keep(&long, tool, TrimKeep::Regions(&[inside]))?;
    Ok(crate::record::after(cut, &long, &crate::record::counts(&[body])))
}

/// The faces of the picked edges back on their surfaces' bounds: each untrimmed in place, its
/// holes kept. One already there is refused.
fn back_to_bounds(body: &Body, faces: &[usize]) -> Result<Body> {
    let [face] = faces else {
        return Err(KernelError::Invalid("not supported yet: going back to the bounds of more than one face at a time".into()));
    };
    let before = crate::measure(body)?.area;
    let back = crate::untrim(body, *face, crate::UntrimHoles::At(&[]), true, true)?;
    let after = crate::measure(&back)?.area;
    if (after - before).abs() <= 1e-9 * before.abs().max(1e-12) {
        return Err(KernelError::Invalid("the edge already lies on its surface's bounds".into()));
    }
    Ok(back)
}

fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib extend: {what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{measure, patch_region};
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }
    fn area(b: &Body) -> f64 {
        measure(b).unwrap().area
    }
    fn with(mode: ExtendMode, limit: ExtendLimit, neighbours: bool) -> ExtendOptions {
        ExtendOptions { mode, limit, neighbours }
    }
    fn natural(d: f64) -> ExtendOptions<'static> {
        with(ExtendMode::Natural, ExtendLimit::Distance(d), true)
    }
    fn edges(b: &Body) -> usize {
        b.edge_count()
    }
    fn square() -> Body {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        patch_region(&Plane::XY, &r).unwrap()
    }
    /// A strip of the cylinder of radius 10 about Z, heights 0…20, angles 0…`sweep`: the arc
    /// extruded, one face, its edges two generators and two arcs.
    fn cylinder_strip(sweep: f64) -> Body {
        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 10.0, start: 0.0, sweep }];
        crate::extrude_sheet(&Plane::XY, &[arc], 0.0, 20.0).unwrap()
    }

    /// Arc length of z = x²/20 from 0 to x.
    fn parabola_arc(x: f64) -> f64 {
        0.5 * x * (1.0 + (x / 10.0).powi(2)).sqrt() + 5.0 * (x / 10.0).asinh()
    }
    /// The parabolic sheet z = x²/20 over 0 ≤ x, y ≤ 10: the parabola (a quadratic Bézier,
    /// sketched on the plane y = 0 whose sketch y is −z) extruded 10 along y.
    fn parabola() -> Body {
        let p = vec![Seg2::Conic { a: Vec2::new(0.0, 0.0), apex: Vec2::new(5.0, 0.0), b: Vec2::new(10.0, -5.0), w: 1.0 }];
        crate::extrude_sheet(&Plane::XZ, &[p], 0.0, 10.0).unwrap()
    }

    /// The default (natural, by a distance, neighbours lengthened) on a plane's straight edges
    /// is today's planar extension, exactly; the general path builds the same square.
    #[test]
    fn a_planar_extension_is_unchanged() {
        let s = square();
        let old = crate::extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], 5.0).unwrap();
        let new = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &natural(5.0)).unwrap();
        assert_eq!(measure(&old).unwrap().area, measure(&new).unwrap().area);
        assert_eq!(edges(&old), edges(&new));
        // Up to the plane x = 15 takes the general path: the same 15 × 10, its lengthened sides
        // one edge each.
        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(15.0, 0.0, 0.0), Vec3::X).unwrap());
        let up = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &with(ExtendMode::Natural, ExtendLimit::UpTo(&tool), true)).unwrap();
        assert!(rel(area(&up), 150.0) < 1e-9, "{}", area(&up));
        assert_eq!((up.face_count(), edges(&up)), (1, 4));
        // Up to x = 17: 170.
        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(17.0, 0.0, 0.0), Vec3::X).unwrap());
        let up = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &with(ExtendMode::Natural, ExtendLimit::UpTo(&tool), true)).unwrap();
        assert!(rel(area(&up), 170.0) < 1e-9, "{}", area(&up));
        let bb = measure(&up).unwrap().bbox;
        assert!((bb.max.x - 17.0).abs() < 1e-9, "{bb:?}");
    }

    /// A cylinder strip (radius 10, a quarter turn, heights 0…20): extended 5 along its axis
    /// (its top arc picked), 10 · π/2 · 25; 5 around its circumference (a straight edge
    /// picked), 10 · (π/2 + ½) · 20; both, meeting at the box's corner, 10 · (π/2 + ½) · 25; up
    /// to the plane z = 30, 10 · π/2 · 30. Each stays one cylindrical face; the sides along the
    /// extension are the old edges lengthened.
    #[test]
    fn a_cylinder_strip_extended_along_its_axis_and_around() {
        let c = cylinder_strip(FRAC_PI_2);
        assert!(rel(area(&c), 100.0 * PI) < 1e-3);
        let top = Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0);
        let side = Vec3::new(0.0, 10.0, 10.0);
        let cylinder =
            |b: &Body| matches!(crate::face_surface(b, 0), Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6);

        let up = extend_surface(&c, &[top], &natural(5.0)).unwrap();
        let want = 10.0 * FRAC_PI_2 * 25.0;
        assert!(rel(area(&up), want) < 1e-3, "{} vs {want}", area(&up)); // (measured on meshes)
        assert!((measure(&up).unwrap().bbox.max.z - 25.0).abs() < 1e-6);
        assert!(up.face_count() == 1 && cylinder(&up) && edges(&up) == 4, "{}", edges(&up));

        let round = extend_surface(&c, &[side], &natural(5.0)).unwrap();
        let want = 10.0 * (FRAC_PI_2 + 0.5) * 20.0;
        assert!(rel(area(&round), want) < 1e-3, "{} vs {want}", area(&round)); // (measured on meshes)
        assert!(round.face_count() == 1 && cylinder(&round) && edges(&round) == 4);
        // The moved edge is at angle π/2 + ½ (5 mm of arc): x = −10 sin ½.
        assert!((measure(&round).unwrap().bbox.min.x + 10.0 * 0.5f64.sin()).abs() < 1e-6, "{:?}", measure(&round).unwrap().bbox);

        let both = extend_surface(&c, &[top, side], &natural(5.0)).unwrap();
        let want = 10.0 * (FRAC_PI_2 + 0.5) * 25.0;
        assert!(rel(area(&both), want) < 1e-3, "{} vs {want}", area(&both)); // (measured on meshes)

        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 30.0), Vec3::Z).unwrap());
        let to = extend_surface(&c, &[top], &with(ExtendMode::Natural, ExtendLimit::UpTo(&tool), true)).unwrap();
        let want = 10.0 * FRAC_PI_2 * 30.0;
        assert!(rel(area(&to), want) < 1e-3, "{} vs {want}", area(&to)); // (measured on meshes)
        assert!((measure(&to).unwrap().bbox.max.z - 30.0).abs() < 1e-6);

        // The history: the face is the one it was, trimmed differently.
        let h = up.history().unwrap();
        assert_eq!(h.faces.first().map(|f| f.change), Some(crate::history::Change::Modified));
    }

    /// The same strip swept as a B-spline sheet (a rational arc extruded): its end spans
    /// continue as themselves, so the extension stays on the cylinder.
    #[test]
    fn an_extruded_cylinder_strip_continues_on_its_cylinder() {
        let sheet = cylinder_strip(FRAC_PI_2);
        let top = Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0);
        let up = extend_surface(&sheet, &[top], &natural(5.0)).unwrap();
        let want = 10.0 * FRAC_PI_2 * 25.0;
        assert!(rel(area(&up), want) < 1e-3, "{} vs {want}", area(&up)); // (measured on meshes)
        let round = extend_surface(&sheet, &[Vec3::new(0.0, 10.0, 10.0)], &natural(5.0)).unwrap();
        let want = 10.0 * (FRAC_PI_2 + 0.5) * 20.0;
        assert!(rel(area(&round), want) < 1e-3, "{} vs {want}", area(&round)); // (measured on meshes)
        assert!(matches!(crate::face_surface(&round, 0), Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6));
    }

    /// The parabolic B-spline sheet z = x²/20 (0 ≤ x, y ≤ 10) extended 5 past its edge x = 10,
    /// naturally: the parabola itself continues, 5 mm of arc further (to x* where its arc
    /// length grows by 5): area 10 · (s(10) + 5), reaching z = x*²/20.
    #[test]
    fn a_bspline_extended_naturally() {
        let p = parabola();
        let base = 10.0 * parabola_arc(10.0);
        assert!(rel(area(&p), base) < 1e-4, "{} vs {base}", area(&p)); // (measured on meshes)
        let edge = [Vec3::new(10.0, 5.0, 5.0)];
        let nat = extend_surface(&p, &edge, &natural(5.0)).unwrap();
        let (mut lo, mut hi) = (10.0, 20.0);
        for _ in 0..100 {
            let m = 0.5 * (lo + hi);
            if parabola_arc(m) - parabola_arc(10.0) < 5.0 { lo = m } else { hi = m }
        }
        let x = 0.5 * (lo + hi);
        let bb = measure(&nat).unwrap().bbox;
        assert!((bb.max.x - x).abs() < 1e-6 && (bb.max.z - x * x / 20.0).abs() < 1e-6, "{bb:?} vs x* = {x}");
        assert!(rel(area(&nat), base + 50.0) < 1e-4, "{} vs {}", area(&nat), base + 50.0); // (measured on meshes)
        assert_eq!(nat.face_count(), 1);
    }

    /// Back to the surface's own bounds: the half saddle (a B-spline over 0…10 × 0…10, x from 0
    /// to 5) goes back to its whole patch; the half cylinder trimmed by a slanted plane (top
    /// from z = 15 down to 10 and back) goes back to its span, heights 0…15: 150π.
    #[test]
    fn extended_back_to_the_surfaces_bounds() {
        let front = (Plane::XZ, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }]);
        let back = (Plane { origin: Vec3::new(0.0, 10.0, 0.0), ..Plane::XZ }, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, -10.0) }]);
        let saddle = crate::loft_sheet(&[front, back]).unwrap();
        let cut = SplitTool::Plane(Plane::from_normal(Vec3::new(5.0, 0.0, 0.0), Vec3::X).unwrap());
        let half = crate::trim(&saddle, &cut, Vec3::new(1.0, 5.0, 0.5)).unwrap();
        let bounds = with(ExtendMode::Natural, ExtendLimit::Bounds, true);
        let whole = extend_surface(&half, &[Vec3::new(5.0, 5.0, 2.5)], &bounds).unwrap();
        let bb = measure(&whole).unwrap().bbox;
        assert!((bb.max.x - 10.0).abs() < 1e-9 && (bb.max.z - 10.0).abs() < 1e-9, "{bb:?}");
        // Already there: refused.
        assert!(extend_surface(&whole, &[Vec3::new(10.0, 5.0, 5.0)], &bounds).is_err());

        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 10.0, start: 0.0, sweep: PI }];
        let sheet = crate::extrude_sheet(&Plane::XY, &[arc], 0.0, 20.0).unwrap();
        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 15.0), Vec3::new(0.0, 0.5, 1.0)).unwrap());
        let cut = crate::trim(&sheet, &tool, Vec3::new(0.0, 10.0, 1.0)).unwrap();
        let back = extend_surface(&cut, &[Vec3::new(0.0, 10.0, 10.0)], &bounds).unwrap();
        let a = area(&back);
        assert!(rel(a, 150.0 * PI) < 1e-3, "{a} vs {}", 150.0 * PI); // (measured on meshes)
        assert!((measure(&back).unwrap().bbox.max.z - 15.0).abs() < 1e-6);
    }

    /// Hostile and unsupported requests are refused, never panic.
    #[test]
    fn hostile_requests_are_refused() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(extend_surface(&s, &at, &natural(d)).is_err(), "{d}");
        }
        assert!(extend_surface(&s, &[], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &[Vec3::new(f64::NAN, 0.0, 0.0)], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &vec![Vec3::ZERO; MAX_PICKS + 1], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &[Vec3::new(50.0, 50.0, 0.0)], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &at, &with(ExtendMode::Tangent, ExtendLimit::Bounds, true)).is_err());
        // A solid's edges lie between two faces.
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        assert!(extend_surface(&b, &[Vec3::new(10.0, 5.0, 10.0)], &natural(1.0)).is_err());
        // A hole's edge.
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(20.0, 20.0), Vec2::new(0.0, 20.0)]),
            holes: vec![Loop2::circle(Vec2::new(10.0, 10.0), 3.0).reversed()],
        };
        let holed = patch_region(&Plane::XY, &r).unwrap();
        let e = extend_surface(&holed, &[Vec3::new(13.0, 10.0, 0.0)], &with(ExtendMode::Tangent, ExtendLimit::Distance(1.0), true)).unwrap_err();
        assert!(e.to_string().contains("hole"), "{e}");
        // A target the extension never reaches (behind the edge).
        let behind = SplitTool::Plane(Plane::from_normal(Vec3::new(-5.0, 0.0, 0.0), Vec3::X).unwrap());
        assert!(extend_surface(&s, &at, &with(ExtendMode::Natural, ExtendLimit::UpTo(&behind), true)).is_err());
    }
}
