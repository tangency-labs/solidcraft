//! Sketch Fillet on two drawn 3D lines (#21319): the corner where they meet is rounded with a
//! drawn 3D arc, for the bends of a tube or pipe path.
//!
//! Two lines that meet lie in one plane, so the round is the arc in that plane tangent to both.
//! Each line is shortened to its tangent point and held there by a Tangent with the arc, so the
//! bend follows the lines when the 3D constraints are solved. What held the sharp corner (a Fix,
//! a dimension to it, a third curve joined there) goes to a 3D point kept at the corner on both
//! lines, the way the planar Fillet keeps a virtual corner.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::model::{Result, Sketch, SketchError};
use crate::wire::drawn_polyline;
use crate::wire_solve::{ACCEPT, MEET, WireConstraintKind, WirePoint};

/// Lines closer than this (radians) to one straight line, or to lying on each other, have no
/// corner to round.
const NO_CORNER: f64 = 1e-6;

/// What [`Sketch::fillet_drawn_lines`] made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawnFillet {
    /// The arc that rounds the corner.
    pub arc: String,
    /// The Tangent constraints that hold each line's end on an end of the arc.
    pub tangents: Vec<String>,
    /// The 3D point kept at the sharp corner, where a constraint held the corner.
    pub corner: Option<String>,
    /// The constraints that now hold that point instead of a line's end.
    pub moved: Vec<String>,
}

/// The round of a corner: where the arc leaves each line, the point of the arc nearest the
/// corner, and the length it takes off each line.
struct Round {
    on_a: Vec3,
    middle: Vec3,
    on_b: Vec3,
    setback: f64,
}

/// The arc of radius `r` that rounds the corner at `corner` between the lines running to
/// `far_a` and `far_b`. With θ the angle between the lines at the corner, it leaves each line
/// r / tan(θ/2) from the corner, and its centre is r / sin(θ/2) from the corner along the
/// bisector. Refused, with the reason, where the lines make no corner.
fn round(corner: Vec3, far_a: Vec3, far_b: Vec3, r: f64) -> std::result::Result<Round, &'static str> {
    let (Some(u), Some(v)) = ((far_a - corner).normalized(), (far_b - corner).normalized()) else {
        return Err("a 3D line of no length has no corner to round");
    };
    let angle = u.cross(v).len().atan2(u.dot(v));
    if angle > std::f64::consts::PI - NO_CORNER {
        return Err("the lines continue each other in one straight line: there is no corner to round");
    }
    if angle < NO_CORNER {
        return Err("the lines lie on each other: there is no corner to round");
    }
    let half = 0.5 * angle;
    let bisector = (u + v).normalized().ok_or("the lines continue each other in one straight line: there is no corner to round")?;
    let setback = r / half.tan();
    let center = corner + bisector * (r / half.sin());
    let round = Round { on_a: corner + u * setback, middle: center - bisector * r, on_b: corner + v * setback, setback };
    if [round.on_a, round.middle, round.on_b].iter().all(|p| p.is_finite()) && setback.is_finite() {
        Ok(round)
    } else {
        Err("that radius is out of range for the corner")
    }
}

/// The fit points a 3D constraint names itself (its lines and arcs are named by curve, and
/// follow their curves).
fn own_points(kind: &mut WireConstraintKind) -> Vec<&mut WirePoint> {
    match kind {
        WireConstraintKind::Coincident { a, b } | WireConstraintKind::Distance { a, b, .. } | WireConstraintKind::AxisDistance { a, b, .. } => {
            vec![a, b]
        }
        WireConstraintKind::PointOnLine { p, .. }
        | WireConstraintKind::Fix { p, .. }
        | WireConstraintKind::OnPlane { p, .. }
        | WireConstraintKind::PlaneDistance { p, .. }
        | WireConstraintKind::PointOnCurve { p, .. } => vec![p],
        WireConstraintKind::AlongAxis { .. }
        | WireConstraintKind::Angle { .. }
        | WireConstraintKind::Radius { .. }
        | WireConstraintKind::Parallel { .. }
        | WireConstraintKind::Perpendicular { .. }
        | WireConstraintKind::Collinear { .. }
        | WireConstraintKind::Equal { .. }
        | WireConstraintKind::Tangent { .. }
        | WireConstraintKind::ParallelToPlane { .. }
        | WireConstraintKind::PlaneAngle { .. } => Vec::new(),
    }
}

impl Sketch {
    /// Round the corner where the drawn 3D lines `a` and `b` share an end point with an arc of
    /// radius `r` (mm) in the plane of the two lines: both lines are shortened to where the arc
    /// leaves them, and a Tangent holds each on its end of the arc. The radius is the caller's
    /// to dimension. A constraint that held the corner now holds a 3D point kept there, on both
    /// lines. Refused, with the reason and with the sketch as it was, where there is no such
    /// corner or the round does not fit.
    pub fn fillet_drawn_lines(&mut self, a: &str, b: &str, r: f64) -> Result<DrawnFillet> {
        let bad = |m: String| SketchError::Invalid(m);
        if a == b {
            return Err(bad("a corner is between two different 3D lines".into()));
        }
        let line =
            |id: &str| self.wires.iter().find(|w| w.id == id && w.link.is_none() && w.fit.len() == 2 && w.arc.is_none() && w.control.is_none());
        let (Some(wa), Some(wb)) = (line(a), line(b)) else {
            return Err(bad("a 3D corner is rounded between two drawn 3D lines".into()));
        };
        if wa.space != wb.space {
            return Err(bad("these 3D lines are in different coordinate spaces (one fixed in the world, one in its component)".into()));
        }
        if !(r.is_finite() && r > 0.0) {
            return Err(bad("the radius must be positive".into()));
        }
        let space = wa.space;
        let end = |wire: &str, index: usize| WirePoint { wire: wire.to_string(), index };
        let joined = |p: &WirePoint, q: &WirePoint| {
            self.wire_constraints
                .iter()
                .any(|c| matches!(&c.kind, WireConstraintKind::Coincident { a, b } if (a == p && b == q) || (a == q && b == p)))
        };
        // The end they share: at one place, or joined.
        let shared = [(0usize, 0usize), (0, 1), (1, 0), (1, 1)].into_iter().find(|(ia, ib)| {
            matches!((wa.fit.get(*ia), wb.fit.get(*ib)), (Some(p), Some(q)) if p.dist(*q) < MEET) || joined(&end(a, *ia), &end(b, *ib))
        });
        let Some((ia, ib)) = shared else {
            return Err(bad(format!("`{a}` and `{b}` do not share an end point: a fillet rounds the corner where two 3D lines meet")));
        };
        let (Some(corner), Some(corner_b), Some(far_a), Some(far_b)) =
            (wa.fit.get(ia).copied(), wb.fit.get(ib).copied(), wa.fit.get(1 - ia).copied(), wb.fit.get(1 - ib).copied())
        else {
            return Err(bad("a 3D corner is rounded between two drawn 3D lines".into()));
        };
        if corner.dist(corner_b) > ACCEPT {
            return Err(bad(format!("`{a}` and `{b}` are joined at ends that are apart: the 3D sketch is not solved there")));
        }
        let (corner_a, corner_b) = (end(a, ia), end(b, ib));
        // What shortening a line would break, and cannot follow the corner.
        for c in &self.wire_constraints {
            match &c.kind {
                WireConstraintKind::Tangent { line, arc, line_end, .. } if (line == a && *line_end == ia) || (line == b && *line_end == ib) => {
                    return Err(bad(format!("`{line}` is already tangent to the arc `{arc}` at that end: there is no corner to round")));
                }
                WireConstraintKind::Equal { a: x, b: y, arcs: false } if [a, b].contains(&x.as_str()) != [a, b].contains(&y.as_str()) => {
                    let (line, other) = if [a, b].contains(&x.as_str()) { (x, y) } else { (y, x) };
                    return Err(bad(format!(
                        "`{line}` is held equal in length to `{other}` by `{}`, and a fillet shortens it: remove that constraint first",
                        c.id
                    )));
                }
                _ => {}
            }
        }
        let round = round(corner, far_a, far_b, r).map_err(|why| bad(why.to_string()))?;
        for (id, far) in [(a, far_a), (b, far_b)] {
            let length = corner.dist(far);
            if round.setback >= length - 1e-7 * (1.0 + length) {
                return Err(bad(format!(
                    "the radius is too large for that corner (it needs {:.3} mm of each line, and must leave some of `{id}`, which is {length:.3} mm long)",
                    round.setback
                )));
            }
        }
        // Built on a copy, so a step that fails leaves the sketch as it was.
        let mut sk = self.clone();
        let arc = sk.add_drawn_arc_in(&[round.on_a, round.middle, round.on_b], space)?;
        for (id, index, to) in [(a, ia, round.on_a), (b, ib, round.on_b)] {
            let Some(w) = sk.wires.iter_mut().find(|w| w.id == id) else { continue };
            if let Some(at) = w.fit.get_mut(index) {
                *at = to;
            }
            w.pts = drawn_polyline(&w.fit)?;
        }
        // The lines no longer meet: their joint goes.
        sk.wire_constraints.retain(|c| !matches!(&c.kind, WireConstraintKind::Coincident { a: p, b: q } if (*p == corner_a && *q == corner_b) || (*p == corner_b && *q == corner_a)));
        // What else held the corner holds a point kept there, on both lines.
        let holds = |kind: &mut WireConstraintKind| own_points(kind).iter().any(|p| **p == corner_a || **p == corner_b);
        let mut moved: Vec<String> = Vec::new();
        for c in &mut sk.wire_constraints {
            if holds(&mut c.kind) {
                moved.push(c.id.clone());
            }
        }
        let mut kept = None;
        if !moved.is_empty() {
            let point = sk.add_drawn_wire_in(&[corner], space)?;
            let at = end(&point, 0);
            for c in &mut sk.wire_constraints {
                for p in own_points(&mut c.kind) {
                    if *p == corner_a || *p == corner_b {
                        *p = at.clone();
                    }
                }
            }
            // Both lines' ends held alike now say one thing twice: once is enough.
            let mut twice: Vec<String> = Vec::new();
            for (i, c) in sk.wire_constraints.iter().enumerate() {
                if moved.contains(&c.id) && sk.wire_constraints.iter().take(i).any(|earlier| earlier.kind.same_as(&c.kind)) {
                    twice.push(c.id.clone());
                }
            }
            sk.wire_constraints.retain(|c| !twice.contains(&c.id));
            // And what a move left naming one point twice (a distance between the two ends).
            sk.prune_wire_constraints();
            moved.retain(|id| sk.wire_constraints.iter().any(|c| c.id == *id));
            for line in [a, b] {
                sk.add_wire_constraint(WireConstraintKind::PointOnLine { p: at.clone(), line: line.to_string() })?;
            }
            kept = Some(point);
        }
        let tangents = vec![
            sk.add_wire_constraint(WireConstraintKind::Tangent { line: a.to_string(), arc: arc.clone(), line_end: ia, arc_end: 0 })?,
            sk.add_wire_constraint(WireConstraintKind::Tangent { line: b.to_string(), arc: arc.clone(), line_end: ib, arc_end: 2 })?,
        ];
        *self = sk;
        Ok(DrawnFillet { arc, tangents, corner: kept, moved })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// Two lines drawn end to end through `points`, joined as the 3D Line tool joins them.
    fn path(points: &[Vec3]) -> (Sketch, Vec<String>) {
        let mut sk = Sketch::new();
        let ids: Vec<String> = points.windows(2).map(|w| sk.add_drawn_wire(w).unwrap()).collect();
        sk.join_drawn_ends(&ids).unwrap();
        (sk, ids)
    }

    fn fit(sk: &Sketch, id: &str) -> Vec<Vec3> {
        sk.wires.iter().find(|w| w.id == id).unwrap().fit.clone()
    }

    /// A square corner of radius 20: each line loses 20 / tan(45°) = 20 mm, the arc is a
    /// quarter circle about (80, 0, 20), and the constraints the fillet leaves hold as built.
    #[test]
    fn a_square_corner_is_rounded_by_a_quarter_circle_that_holds_as_built() {
        let (mut sk, ids) = path(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0), v(100.0, 0.0, 80.0)]);
        let done = sk.fillet_drawn_lines(&ids[0], &ids[1], 20.0).unwrap();
        assert!(fit(&sk, &ids[0])[1].dist(v(80.0, 0.0, 0.0)) < 1e-9 && fit(&sk, &ids[1])[0].dist(v(100.0, 0.0, 20.0)) < 1e-9);
        let arc = sk.wires.iter().find(|w| w.id == done.arc).unwrap().arc.unwrap();
        assert!(arc.center.dist(v(80.0, 0.0, 20.0)) < 1e-9 && (arc.radius - 20.0).abs() < 1e-9, "{arc:?}");
        assert!((crate::arc_sweep(&arc) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert_eq!((done.tangents.len(), done.corner.clone(), done.moved.len()), (2, None, 0));
        assert!(sk.wire_constraints_hold(), "{:?}", sk.wire_constraints);
        sk.validate_wires().unwrap();
        sk.validate_wire_constraints().unwrap();
        let before = sk.clone();
        assert!(sk.solve_wires(None).ok);
        assert_eq!(sk.wires, before.wires, "a solve moves nothing");
    }

    /// Either order of the lines and either direction each was drawn in gives the same round.
    #[test]
    fn the_round_does_not_depend_on_how_the_lines_were_drawn() {
        let (corner, p, q) = (v(60.0, 0.0, 0.0), v(0.0, 0.0, 0.0), v(90.0, 40.0, 30.0));
        let mut centres = Vec::new();
        for (first, second) in [([p, corner], [corner, q]), ([corner, p], [corner, q]), ([p, corner], [q, corner]), ([corner, p], [q, corner])] {
            for swap in [false, true] {
                let mut sk = Sketch::new();
                let (a, b) = (sk.add_drawn_wire(&first).unwrap(), sk.add_drawn_wire(&second).unwrap());
                let done = if swap { sk.fillet_drawn_lines(&b, &a, 10.0) } else { sk.fillet_drawn_lines(&a, &b, 10.0) }.unwrap();
                assert!(sk.wire_constraints_hold(), "{:?}", sk.wire_constraints);
                centres.push(sk.wires.iter().find(|w| w.id == done.arc).unwrap().arc.unwrap().center);
            }
        }
        assert!(centres.iter().all(|c| c.dist(centres[0]) < 1e-9), "{centres:?}");
    }

    /// What held the corner holds a point kept there on both lines: a Fix, a dimension from
    /// the far end (the line's length, 100 mm, now measured to the sharp corner) and a third
    /// line joined there. The joint of the two lines themselves goes.
    #[test]
    fn what_held_the_corner_holds_a_point_kept_there() {
        let (mut sk, ids) = path(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0), v(100.0, 0.0, 80.0)]);
        let third = sk.add_drawn_wire(&[v(100.0, 0.0, 0.0), v(100.0, 50.0, 0.0)]).unwrap();
        sk.join_drawn_ends(std::slice::from_ref(&third)).unwrap();
        let end = |wire: &str, index: usize| WirePoint { wire: wire.to_string(), index };
        let fix = sk.add_wire_constraint(WireConstraintKind::Fix { p: end(&ids[1], 0), at: v(100.0, 0.0, 0.0), exprs: None }).unwrap();
        let length =
            sk.add_wire_dimension(WireConstraintKind::Distance { a: end(&ids[0], 0), b: end(&ids[0], 1), value: 100.0 }, None, false).unwrap();
        let done = sk.fillet_drawn_lines(&ids[0], &ids[1], 20.0).unwrap();
        let corner = done.corner.clone().expect("the corner is kept");
        assert_eq!(fit(&sk, &corner), vec![v(100.0, 0.0, 0.0)]);
        assert!(done.moved.contains(&fix) && done.moved.contains(&length), "{done:?}");
        let kind = |id: &str| sk.wire_constraints.iter().find(|c| c.id == id).map(|c| c.kind.clone()).unwrap();
        assert!(matches!(kind(&fix), WireConstraintKind::Fix { p, .. } if p == end(&corner, 0)));
        assert!(
            matches!(kind(&length), WireConstraintKind::Distance { a, b, value } if a == end(&ids[0], 0) && b == end(&corner, 0) && value == 100.0)
        );
        let joins = |p: &WirePoint, q: &WirePoint| {
            sk.wire_constraints.iter().any(|c| matches!(&c.kind, WireConstraintKind::Coincident { a, b } if (a == p && b == q) || (a == q && b == p)))
        };
        assert!(joins(&end(&corner, 0), &end(&third, 0)), "the third line ends on the kept corner: {:?}", sk.wire_constraints);
        assert!(!joins(&end(&ids[0], 1), &end(&ids[1], 0)));
        for line in &ids {
            assert!(
                sk.wire_constraints
                    .iter()
                    .any(|c| matches!(&c.kind, WireConstraintKind::PointOnLine { p, line: l } if *p == end(&corner, 0) && l == line))
            );
        }
        assert!(sk.wire_constraints_hold(), "{:?}", sk.wire_constraints);
        sk.validate_wire_constraints().unwrap();
    }

    /// No corner, no fit, or a constraint a shorter line would break: refused with the reason,
    /// and the sketch is as it was.
    #[test]
    fn what_cannot_be_rounded_is_refused_and_the_sketch_stays() {
        let refused = |sk: &mut Sketch, a: &str, b: &str, r: f64, why: &str| {
            let before = sk.clone();
            let e = sk.fillet_drawn_lines(a, b, r).unwrap_err().to_string();
            assert!(e.contains(why), "{e}");
            assert_eq!(*sk, before);
        };
        let (mut sk, ids) = path(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0), v(100.0, 0.0, 80.0)]);
        refused(&mut sk, &ids[0], &ids[0], 5.0, "two different 3D lines");
        refused(&mut sk, &ids[0], "nothing", 5.0, "two drawn 3D lines");
        // 80 mm of the second line is all of it; 80.001 is more.
        refused(&mut sk, &ids[0], &ids[1], 80.0, "too large for that corner (it needs 80.000 mm of each line");
        refused(&mut sk, &ids[0], &ids[1], 1e12, "too large for that corner");
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            refused(&mut sk, &ids[0], &ids[1], r, "radius must be positive");
        }
        let apart = sk.add_drawn_wire(&[v(0.0, 50.0, 0.0), v(0.0, 50.0, 40.0)]).unwrap();
        refused(&mut sk, &ids[0], &apart, 5.0, "do not share an end point");
        let arc = sk.add_drawn_arc_in(&[v(100.0, 0.0, 80.0), v(110.0, 0.0, 90.0), v(100.0, 0.0, 100.0)], crate::WireSpace::World).unwrap();
        refused(&mut sk, &ids[1], &arc, 5.0, "two drawn 3D lines");
        let other_space = sk.add_drawn_wire_in(&[v(0.0, 0.0, 0.0), v(0.0, -30.0, 0.0)], crate::WireSpace::Component).unwrap();
        refused(&mut sk, &ids[0], &other_space, 5.0, "different coordinate spaces");
        let equal = sk.add_wire_constraint(WireConstraintKind::Equal { a: ids[1].clone(), b: apart.clone(), arcs: false }).unwrap();
        refused(&mut sk, &ids[0], &ids[1], 5.0, &format!("held equal in length to `{apart}` by `{equal}`"));
        // In one straight line, and back along each other.
        let (mut sk, ids) = path(&[v(0.0, 0.0, 0.0), v(10.0, 0.0, 5.0), v(20.0, 0.0, 10.0)]);
        refused(&mut sk, &ids[0], &ids[1], 2.0, "one straight line");
        let (mut sk, ids) = path(&[v(0.0, 0.0, 0.0), v(10.0, 0.0, 5.0), v(4.0, 0.0, 2.0)]);
        refused(&mut sk, &ids[0], &ids[1], 2.0, "lie on each other");
        // A line already tangent to an arc at the corner end.
        let (mut sk, ids) = path(&[v(0.0, 0.0, 0.0), v(100.0, 0.0, 0.0), v(100.0, 0.0, 80.0)]);
        let done = sk.fillet_drawn_lines(&ids[0], &ids[1], 20.0).unwrap();
        let from_tangent_point = sk.add_drawn_wire(&[v(80.0, 0.0, 0.0), v(80.0, 30.0, 0.0)]).unwrap();
        refused(&mut sk, &ids[0], &from_tangent_point, 5.0, &format!("already tangent to the arc `{}`", done.arc));
    }
}
