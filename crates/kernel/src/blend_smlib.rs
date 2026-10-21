//! Fillets and chamfers through SMLib, when [`crate::backend`] selects it.
//!
//! Each kernel entry point ([`crate::fillet`], [`crate::chamfer`], …) asks [`selected`] first and,
//! when SMLib is the kernel, hands its arguments here unchanged: the body goes into SMLib as its
//! schema 4 record (healed where SMLib's check refuses it as given), tagged as operand 0; each
//! point picks the body's nearest edge in its own edge order, as truck's blends do, and SMLib is
//! given that edge's midpoint, which no other edge is as near to (a point at a vertex between two
//! edges would leave SMLib to choose); SMLib blends those edges with their tangent chains, its check must pass, and the result comes back decoded with its
//! geometry checked. An option SMLib has no construction for is refused as "not supported yet",
//! never built by truck instead, so a comparison never compares truck with itself.
//!
//! The history is SMLib's tags, renumbered into the body's orders; a blend face, which SMLib
//! leaves untagged, comes from the edge it replaces ([`solidcraft_smlib::history::blends`]).

use solidcraft_geom::Vec3;

use crate::blend_variable::RadiusSpline;
use crate::body::Body;
use crate::{KernelError, Result};

/// Whether SMLib runs the blends ([`crate::backend`]). A setting naming SMLib in a build without
/// it, or naming no kernel, is the error it is for every operation.
pub(crate) fn selected() -> Result<bool> {
    Ok(crate::backend()? == crate::Backend::Smlib)
}

/// What SMLib cannot build yet: refused rather than built by truck.
pub(crate) fn unsupported(what: &str) -> KernelError {
    KernelError::Invalid(format!("not supported yet: {what} with the SMLib kernel"))
}

/// A constant-radius round of the edges nearest `edges` ([`crate::fillet`]).
pub(crate) fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    imp::blend(body, "fillet", Kind::Round, edges, &vec![radius; edges.len()])
}

/// Each edge rounded at its own radius ([`crate::fillet_each`]).
pub(crate) fn fillet_each(body: &Body, edges: &[Vec3], radii: &[f64]) -> Result<Body> {
    if radii.len() != edges.len() {
        return Err(KernelError::Invalid("one radius per edge".into()));
    }
    imp::blend(body, "fillet", Kind::Round, edges, radii)
}

/// Curvature-continuous rounds with the setback of a round of `radius` ([`crate::fillet_g2`]).
pub(crate) fn fillet_g2(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    imp::blend(body, "G2 fillet", Kind::Curvature, edges, &vec![radius; edges.len()])
}

/// Rounds `chord` wide from contact to contact ([`crate::fillet_chord`]).
pub(crate) fn fillet_chord(body: &Body, edges: &[Vec3], chord: f64) -> Result<Body> {
    imp::blend(body, "chord fillet", Kind::Chord, edges, &vec![chord; edges.len()])
}

/// An equal-distance chamfer ([`crate::chamfer`]).
pub(crate) fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    imp::blend(body, "chamfer", Kind::Chamfer, edges, &vec![distance; edges.len()])
}

/// A chamfer `d1` back along the face facing up most (down most with `flip`), the other side by
/// `side` ([`crate::chamfer_sides`]).
pub(crate) fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: crate::ChamferSide, flip: bool) -> Result<Body> {
    imp::sides(body, edges, d1, side, flip)
}

/// One edge rounded with the radius `spline` (checked by the caller) along it from the end
/// nearest `start` ([`crate::fillet_variable_spline`]).
pub(crate) fn fillet_variable(body: &Body, edge_at: Vec3, start: Vec3, spline: &RadiusSpline) -> Result<Body> {
    imp::variable(body, edge_at, start, spline)
}

#[derive(Clone, Copy)]
enum Kind {
    Round,
    Curvature,
    Chord,
    Chamfer,
}

#[cfg(feature = "smlib")]
mod imp {
    use std::collections::BTreeSet;

    use solidcraft_geom::Vec3;
    use solidcraft_smlib::{Blend, Brep, ChamferSecond, CheckLevel, Error as SmError};

    use super::Kind;
    use crate::blend_variable::RadiusSpline;
    use crate::body::Body;
    use crate::history::{BodyHistory, EdgeOrigin, History};
    use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
    use crate::{KernelError, Result, guard, record};

    fn failed(what: &str, step: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib {what}: {step}: {e}"))
    }

    /// How far a pick may lie from its edge: as far as truck's blends accept one.
    fn pick_tolerance(body: &Body) -> f64 {
        body.size() * 0.05 + 1e-3
    }

    /// The edges `picks` select: each one's index in the body's edge order and the point halfway
    /// along it, for SMLib to pick. A pick as near to a smooth join (a seam where a face meets
    /// itself, or faces meeting tangentially: never a blend's edge) as to a sharp edge, such as
    /// one at the vertex where a cylinder's seam meets its rim, takes the sharp edge.
    pub(super) fn resolve(body: &Body, picks: &[Vec3]) -> Result<Picked> {
        if picks.iter().any(|p| !p.is_finite()) {
            return Err(KernelError::Invalid("edge picks must be finite points".into()));
        }
        let mesh = body.tessellate((body.size() * 2e-3).max(1e-3))?;
        let seams = crate::topo::seam_flags(body, &mesh);
        let (smooth, _) = crate::smooth_edges::flags(body, &mesh);
        let reach = pick_tolerance(body);
        let tie = (body.size() * 1e-6).max(1e-6);
        let mut out = Picked { edges: Vec::with_capacity(picks.len()), points: Vec::with_capacity(picks.len()) };
        for p in picks {
            // The nearest edge, and the nearest that is no seam.
            let (mut any, mut edge): (Option<(f64, usize)>, Option<(f64, usize)>) = (None, None);
            for (i, line) in mesh.edges.iter().enumerate() {
                let d =
                    line.windows(2).filter_map(|w| if let [a, b] = w { Some(p.dist_to_segment(*a, *b)) } else { None }).fold(f64::INFINITY, f64::min);
                let seam = seams.get(i).copied().unwrap_or(false) || smooth.get(i).copied().unwrap_or(false);
                for (slot, take) in [(&mut any, true), (&mut edge, !seam)] {
                    if take && slot.is_none_or(|(bd, _)| d < bd) {
                        *slot = Some((d, i));
                    }
                }
            }
            let chosen = match (any, edge) {
                (Some((d, _)), Some(e)) if e.0 <= d + tie => Some(e),
                (a, _) => a,
            };
            let Some((_, i)) = chosen.filter(|(d, _)| *d <= reach) else {
                return Err(KernelError::Invalid(format!("no edge near the pick at ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)));
            };
            let (mid, _) = crate::body::polyline_mid(mesh.edges.get(i).map(Vec::as_slice).unwrap_or(&[]));
            out.edges.push(i);
            out.points.push([mid.x, mid.y, mid.z]);
        }
        Ok(out)
    }

    pub(super) struct Picked {
        /// Each pick's edge in the body's edge order.
        pub(super) edges: Vec<usize>,
        /// A point halfway along each.
        pub(super) points: Vec<[f64; 3]>,
    }

    fn sizes_ok(what: &str, sizes: &[f64]) -> Result<()> {
        if sizes.is_empty() || sizes.len() > solidcraft_smlib::MAX_POINTS {
            return Err(KernelError::Invalid(format!("{what}: select 1…{} edges", solidcraft_smlib::MAX_POINTS)));
        }
        if sizes.iter().any(|s| !(s.is_finite() && *s > 1e-6 && *s < 1e6)) {
            return Err(KernelError::Invalid(format!("{what}: the size must be positive and finite")));
        }
        Ok(())
    }

    pub(super) fn blend(body: &Body, what: &str, kind: Kind, edges: &[Vec3], sizes: &[f64]) -> Result<Body> {
        body.require_brep(what)?;
        sizes_ok(what, sizes)?;
        let picked = resolve(body, edges)?;
        let kind = match kind {
            Kind::Round => Blend::Round,
            Kind::Curvature => Blend::Curvature,
            Kind::Chord => Blend::Chord,
            Kind::Chamfer => Blend::Chamfer,
        };
        let tol = pick_tolerance(body);
        run(body, what, &picked.edges, |b| b.blend(kind, &picked.points, sizes, tol))
    }

    pub(super) fn sides(body: &Body, edges: &[Vec3], d1: f64, side: crate::ChamferSide, flip: bool) -> Result<Body> {
        body.require_brep("chamfer")?;
        sizes_ok("chamfer", &vec![d1; edges.len()])?;
        let picked = resolve(body, edges)?;
        let second = match side {
            crate::ChamferSide::Distance(d) => ChamferSecond::Distance(d),
            crate::ChamferSide::Angle(a) => ChamferSecond::Angle(a),
        };
        let up = if flip { [0.0, 0.0, -1.0] } else { [0.0, 0.0, 1.0] };
        let tol = pick_tolerance(body);
        run(body, "chamfer", &picked.edges, |b| b.chamfer_sides(&picked.points, tol, d1, second, up))
    }

    pub(super) fn variable(body: &Body, edge_at: Vec3, start: Vec3, spline: &RadiusSpline) -> Result<Body> {
        body.require_brep("fillet")?;
        if !start.is_finite() {
            return Err(KernelError::Invalid("the start point must be finite".into()));
        }
        let picked = resolve(body, &[edge_at])?;
        let [at] = picked.points[..] else { return Err(KernelError::Invalid("one edge".into())) };
        let tol = pick_tolerance(body);
        run(body, "variable fillet", &picked.edges, |b| {
            b.fillet_variable(at, tol, [start.x, start.y, start.z], spline.degree, &spline.knots, &spline.radii)
        })
    }

    /// `op` on `body` in SMLib, tagged as operand 0, checked, decoded, with its history.
    fn run(body: &Body, what: &str, edges: &[usize], op: impl FnOnce(&Brep) -> solidcraft_smlib::Result<Brep>) -> Result<Body> {
        guard(what, || {
            let (sb, order) = smlib_body_ordered(body)?;
            let tagged = sb.tagged(0).map_err(|e| failed(what, "tag", e))?;
            let r = op(&tagged).map_err(|e| match e {
                SmError::Unsupported => super::unsupported(&format!("this {what}, which SMLib cannot build exactly")),
                e => failed(what, "operation", e),
            })?;
            let check = r.check(CheckLevel::All).map_err(|e| failed(what, "check", e))?;
            if !check.is_valid() {
                let input_fails = sb.check(CheckLevel::All).is_ok_and(|c| !c.is_valid());
                let step =
                    if input_fails { "SMLib's check refuses the result (the input already fails it)" } else { "SMLib's check refuses the result" };
                return Err(failed(what, step, check));
            }
            let (out, result_order) = body_from_smlib_ordered(&r)?;
            let history = match (order, result_order) {
                (Some(o), Some(ro)) => match r.history().map_err(|e| failed(what, "history", e))?.history(&[o], &ro) {
                    Ok(mut h) => {
                        let operand = History::identity(body).edges;
                        let replaced = blended(body, edges, &h);
                        solidcraft_smlib::history::blends(&mut h, &operand, &replaced);
                        Some(h)
                    }
                    Err(e) => {
                        log::debug!(target: "smlib", "{what} history: {e}");
                        None
                    }
                },
                _ => None,
            };
            Ok(out.with_checked_history(history, &record::counts(&[body])).inherit_regularization_protection(body))
        })
    }

    /// The operand's edges the blend replaced: each picked edge, and every edge no result edge
    /// comes from (the rest of the tangent chains), in the operand's edge order.
    fn blended(body: &Body, picked: &[usize], h: &History) -> Vec<usize> {
        let mut out: BTreeSet<usize> = picked.iter().copied().collect();
        let kept: BTreeSet<usize> = h
            .edges
            .iter()
            .flat_map(|e| e.origins.iter())
            .filter_map(|o| match o {
                EdgeOrigin::Edge { operand: 0, edge } => Some(*edge),
                _ => None,
            })
            .collect();
        out.extend((0..body.edge_count()).filter(|e| !kept.contains(e)));
        out.into_iter().collect()
    }
}

#[cfg(not(feature = "smlib"))]
mod imp {
    use solidcraft_geom::Vec3;

    use super::Kind;
    use crate::blend_variable::RadiusSpline;
    use crate::body::Body;
    use crate::{KernelError, Result};

    fn not_built() -> KernelError {
        KernelError::Invalid("not supported yet: this build has no SMLib".into())
    }

    pub(super) fn blend(_: &Body, _: &str, _: Kind, _: &[Vec3], _: &[f64]) -> Result<Body> {
        Err(not_built())
    }

    pub(super) fn variable(_: &Body, _: Vec3, _: Vec3, _: &RadiusSpline) -> Result<Body> {
        Err(not_built())
    }

    pub(super) fn sides(_: &Body, _: &[Vec3], _: f64, _: crate::ChamferSide, _: bool) -> Result<Body> {
        Err(not_built())
    }
}

#[cfg(all(test, feature = "smlib"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use solidcraft_geom::Vec3;

    use super::*;
    use crate::history::{Change, FaceOrigin};
    use crate::{box_solid, measure};

    const QUARTER: f64 = 1.0 - std::f64::consts::FRAC_PI_4;

    fn cube() -> Body {
        box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0)).unwrap()
    }

    fn volume(b: &Body) -> f64 {
        measure(b).unwrap().volume
    }

    /// Within the kernel measure's accuracy on a curved face (a few 1e-6 of the volume; SMLib's
    /// own exact volumes match to 1e-3 mm³ in the C API's tests).
    fn near(got: f64, want: f64) -> bool {
        (got - want).abs() <= 5e-6 * want.abs()
    }

    /// The edge of `b` nearest `p`, in its edge order.
    fn edge_at(b: &Body, p: Vec3) -> usize {
        b.nearest_edge(p, 1e-3).unwrap().unwrap().0
    }

    #[test]
    fn a_round_loses_its_corner_and_its_face_comes_from_the_edge() {
        let b = cube();
        let at = Vec3::new(10.0, 20.0, 20.0);
        let r = imp::blend(&b, "fillet", Kind::Round, &[at], &[2.0]).unwrap();
        assert!(near(volume(&r), 8000.0 - QUARTER * 4.0 * 20.0), "{}", volume(&r));
        assert_eq!(r.face_count(), 7);
        let h = r.history().expect("a history");
        let blends: Vec<_> = h.faces.iter().filter(|f| f.change == Change::Generated).collect();
        assert_eq!(blends.len(), 1);
        assert_eq!(blends[0].origins, vec![FaceOrigin::Edge { operand: 0, edge: edge_at(&b, at) }]);
    }

    #[test]
    fn a_pick_at_a_vertex_takes_the_nearest_edge_by_its_midpoint() {
        // (20, 20, 10) is on the vertical edge only; (20, 20, 20) is a corner of three edges, which
        // the body's own order resolves as truck's blends do, never left to SMLib.
        let b = cube();
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(20.0, 20.0, 20.0)], &[1.0]).is_ok());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(60.0, 60.0, 60.0)], &[1.0]).is_err());
    }

    #[test]
    fn a_pick_where_a_seam_meets_a_rim_takes_the_rim() {
        // Every point of the top rim's circle, the seam's top end among them, is on the rim.
        let c = crate::cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 8.0, 10.0).unwrap();
        assert!(c.edge_count() > 2, "the cylinder has a seam");
        for k in 0..8 {
            let a = k as f64 * std::f64::consts::FRAC_PI_4;
            let picked = imp::resolve(&c, &[Vec3::new(8.0 * a.cos(), 8.0 * a.sin(), 10.0)]).unwrap();
            let [_, _, z] = picked.points[0];
            assert!((z - 10.0).abs() < 1e-9, "the pick at {a} took an edge through z = {z}");
        }
    }

    #[test]
    fn each_edge_takes_its_own_size() {
        let b = cube();
        let picks = [Vec3::new(10.0, 20.0, 20.0), Vec3::new(10.0, 0.0, 20.0)];
        let r = imp::blend(&b, "fillet", Kind::Round, &picks, &[2.0, 3.0]).unwrap();
        assert!(near(volume(&r), 8000.0 - QUARTER * 13.0 * 20.0), "{}", volume(&r));
        let c = imp::blend(&b, "chamfer", Kind::Chamfer, &picks, &[2.0, 3.0]).unwrap();
        assert!(near(volume(&c), 8000.0 - 0.5 * 13.0 * 20.0), "{}", volume(&c));
        assert_eq!(c.history().map(|h| h.faces.iter().filter(|f| matches!(f.origins[..], [FaceOrigin::Edge { .. }])).count()), Some(2));
    }

    #[test]
    fn a_linear_radius_runs_from_the_start_end() {
        // 3 at x = 0 to 1 at x = 20: (1 - pi/4) L (r0^2 + r0 r1 + r1^2) / 3 taken away, from
        // either end given as the start with the law reversed.
        let b = cube();
        let at = Vec3::new(10.0, 20.0, 20.0);
        let law = RadiusSpline { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], radii: vec![3.0, 1.0] };
        let r = imp::variable(&b, at, Vec3::new(0.0, 20.0, 20.0), &law).unwrap();
        let want = 8000.0 - QUARTER * 20.0 * 13.0 / 3.0;
        assert!((volume(&r) - want).abs() < 0.05, "{}", volume(&r));
        let back = imp::variable(&b, at, Vec3::new(20.0, 20.0, 20.0), &law.reversed()).unwrap();
        assert!((volume(&back) - volume(&r)).abs() < 1e-6 * 8000.0);
    }

    /// A chamfer 5 back along the top and 2 down the side of the cube's edge y = 20, z = 20 (or
    /// 2 along the top and 5 down with `flip`): a 5 × 2 / 2 triangle along the 20 mm edge, its
    /// face coming from the edge.
    #[test]
    fn a_chamfer_takes_its_two_setbacks() {
        let b = cube();
        let at = Vec3::new(10.0, 20.0, 20.0);
        for flip in [false, true] {
            let r = imp::sides(&b, &[at], 5.0, crate::ChamferSide::Distance(2.0), flip).unwrap();
            assert!(near(volume(&r), 8000.0 - 100.0), "{}", volume(&r));
            let top = if flip { 18.0 } else { 15.0 };
            assert!(
                r.edges(0.01).unwrap().iter().any(|e| e.points.iter().all(|p| (p.y - top).abs() < 1e-6 && (p.z - 20.0).abs() < 1e-6)),
                "flip {flip}"
            );
            let h = r.history().unwrap();
            assert!(h.faces.iter().any(|f| f.origins == [crate::history::FaceOrigin::Edge { operand: 0, edge: edge_at(&b, at) }]));
        }
        let r = imp::sides(&b, &[at], 5.0, crate::ChamferSide::Angle(45f64.to_radians()), false).unwrap();
        assert!(near(volume(&r), 8000.0 - 12.5 * 20.0), "{}", volume(&r));
    }

    /// The ladder's 08/006: a 2 mm chamfer of a cylinder's top rim. SMLib's cone and rail lie some
    /// 2e-5 mm off the faces they bound, more than the kernel's working tolerance; the body keeps
    /// the tolerances SMLib's record gave them, so its own record (and the saved body's) is valid
    /// and SMLib's check takes it back.
    #[test]
    fn an_smlib_blend_keeps_its_record_valid() {
        let c = crate::cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 10.0, 20.0).unwrap();
        let r = imp::blend(&c, "chamfer", Kind::Chamfer, &[Vec3::new(10.0, 0.0, 20.0)], &[2.0]).unwrap();
        let v = crate::validity(&r);
        assert!(v.record.is_ok(), "{:?}", v.record);
        assert!(matches!(v.smlib, Some(crate::SmlibCheck::Valid)), "{:?}", v.smlib);
        let saved = crate::decode_native_body(&crate::encode_native_body(&r).unwrap()).unwrap();
        assert!(crate::validity(&saved).record.is_ok());
        assert!(
            crate::validity(&saved.placed(&truck_modeling::Matrix4::from_translation(truck_modeling::Vector3::new(5.0, 0.0, 0.0))).unwrap())
                .record
                .is_ok()
        );
    }

    #[test]
    fn options_smlib_cannot_build_are_refused() {
        assert!(unsupported("x").to_string().contains("not supported yet"));
        let b = cube();
        // A radius law with a corner (4 → 2 → 4, straight on both halves).
        let corner = RadiusSpline { degree: 1, knots: vec![0.0, 0.0, 0.5, 1.0, 1.0], radii: vec![4.0, 2.0, 4.0] };
        let e = imp::variable(&b, Vec3::new(10.0, 20.0, 20.0), Vec3::new(0.0, 20.0, 20.0), &corner).unwrap_err();
        assert!(e.to_string().contains("not supported yet"), "{e}");
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(10.0, 20.0, 20.0)], &[f64::NAN]).is_err());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(f64::NAN, 20.0, 20.0)], &[1.0]).is_err());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[], &[]).is_err());
    }
}
