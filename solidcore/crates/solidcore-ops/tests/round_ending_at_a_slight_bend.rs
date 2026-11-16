//! A round that ends where its edge runs on into another at a slight bend, the part widening
//! past the bend.
//!
//! The part is a disc with a handle, extruded: the handle's straight top edge runs
//! tangentially into a concave arc, which meets the disc's rim at the vertex V a few degrees
//! off tangent. The two circles (the disc's and the arc's) then cross twice, at V and a few
//! millimetres on, and between the crossings the disc stands across the arc continued. The
//! round of the handle's top and the arc, run on past V along the rim, would have to cut
//! through that sliver of the disc before it ran out through the rim: it has no end curve
//! on the disc's wall from V, and it was refused (status 1001). The bend is within the
//! corner's tangency tolerance, so V is a smooth vertex, and the round now ends there, cut
//! square to its edge.
//!
//! Graded on closed forms: the removed volume is the round's section taken along the two
//! edges (Pappus round the arc's centre, straight along the handle), the rails end at V's
//! section, and the disc's rim stays whole. Controls: with the arc tangent to the disc (no
//! bend) and with the bend the other way (the rim leaving V towards the round), the round
//! runs out through the disc's wall as before, ending where its rail on the top meets the
//! rim.

#![allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

mod common;

use std::f64::consts::{FRAC_PI_4, PI, TAU};

use common::{edge_at, kernel_copy};
use solidcore::query::api_queries::vertex_get_point;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{session, Body, CheckLevel};

/// The disc's radius, the arc's, the round's, the part's height, the handle's length and
/// width.
const DISC: f64 = 55.0;
const ARC: f64 = 49.0;
const ROUND: f64 = 2.0;
const HEIGHT: f64 = 16.0;
const HANDLE: f64 = 84.0;
const WIDTH: f64 = 16.0;
/// Where V lies on the disc's rim, and the angle the arc sweeps to it.
const AT: f64 = 50.0 * PI / 180.0;
const SWEEP: f64 = 48.0 * PI / 180.0;

/// V, on the rim (before mirroring).
fn vertex() -> [f64; 2] {
    [DISC * AT.cos(), DISC * AT.sin()]
}

/// What the round takes along exactly its two edges: its section, `(1 - pi/4) r^2`, straight
/// along the handle and, by Pappus, round the arc's centre at the section's centroid (outside
/// the arc: the arc is concave).
fn along_its_edges() -> f64 {
    let area = (1.0 - FRAC_PI_4) * ROUND * ROUND;
    let centroid = ROUND * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    area * (HANDLE + SWEEP * (ARC + centroid))
}

/// The part whose arc meets the disc's rim `bend` (radians) off tangent: positive, the rim
/// leaves V away from the round (the circles cross again past V); negative, towards it.
/// `mirrored` in the x axis.
struct Part {
    bend: f64,
    mirrored: bool,
}

impl Part {
    fn point(&self, p: [f64; 2], z: f64) -> [f64; 3] {
        [p[0], if self.mirrored { -p[1] } else { p[1] }, z]
    }

    /// The arc's centre: from V, the rim's outward normal turned by the bend.
    fn centre(&self) -> [f64; 2] {
        let v = vertex();
        let a = AT + self.bend;
        [v[0] + ARC * a.cos(), v[1] + ARC * a.sin()]
    }

    /// The point at `radius` from the arc's centre, at `angle` about it.
    fn about_centre(&self, radius: f64, angle: f64) -> [f64; 2] {
        let c = self.centre();
        [c[0] + radius * angle.cos(), c[1] + radius * angle.sin()]
    }

    /// The angles about the arc's centre of the arc's start (where the handle's top runs
    /// into it) and of V.
    fn arc_angles(&self) -> (f64, f64) {
        let at_v = AT + self.bend + PI;
        (at_v + SWEEP, at_v)
    }

    /// The handle's top edge, from its far end to the arc's start, and its unit direction.
    fn top(&self) -> ([f64; 2], [f64; 2], [f64; 2]) {
        let (start, _) = self.arc_angles();
        let end = self.about_centre(ARC, start);
        // the arc runs clockwise about its centre from its start
        let dir = [start.sin(), -start.cos()];
        (
            [end[0] - HANDLE * dir[0], end[1] - HANDLE * dir[1]],
            end,
            dir,
        )
    }

    fn body(&self) -> Body {
        let (start, at_v) = self.arc_angles();
        let (far, end, dir) = self.top();
        // the handle's end, square to its top, and its bottom back to the rim
        let corner = [far[0] - WIDTH * dir[1], far[1] + WIDTH * dir[0]];
        // corner + t dir on the rim: t^2 + 2 t (corner . dir) + |corner|^2 - DISC^2 = 0
        let along = corner[0] * dir[0] + corner[1] * dir[1];
        let t = -along - (along * along - corner[0].hypot(corner[1]).powi(2) + DISC * DISC).sqrt();
        let foot = [corner[0] + t * dir[0], corner[1] + t * dir[1]];
        let foot_angle = foot[1].atan2(foot[0]);
        let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
            start: self.point(a, 0.0),
            end: self.point(b, 0.0),
        };
        // the arc about `centre` from the angle `from` to `to` (as before mirroring), either
        // way round: counter-clockwise about +z where the angle grows, else about -z, whose
        // angles are those about +z negated
        let arc = |centre: [f64; 2], radius: f64, from: f64, to: f64| {
            let (from, to) = if self.mirrored {
                (-from, -to)
            } else {
                (from, to)
            };
            let (axis, start, end) = if to > from {
                (1.0, from, to)
            } else {
                (-1.0, -from, -to)
            };
            Curve::Arc {
                center: self.point(centre, 0.0),
                axis: [0.0, 0.0, axis],
                reference: [1.0, 0.0, 0.0],
                radius,
                start,
                end,
            }
        };
        let centre = self.centre();
        // counter-clockwise: the rim from V round to the handle's foot, the handle, the arc
        // back to V; the mirror image the other way round
        let profile = if self.mirrored {
            vec![
                arc(centre, ARC, at_v, start),
                line(end, far),
                line(far, corner),
                line(corner, foot),
                arc([0.0, 0.0], DISC, foot_angle + TAU, AT),
            ]
        } else {
            vec![
                arc([0.0, 0.0], DISC, AT, foot_angle + TAU),
                line(foot, corner),
                line(corner, far),
                line(far, end),
                arc(centre, ARC, start, at_v),
            ]
        };
        extrude(
            &profile,
            [0.0, 0.0, 1.0],
            HEIGHT,
            0.0,
            DraftCorner::default(),
            true,
        )
        .unwrap()
    }

    /// The part with the handle's top and the arc rounded, how many faces the blend made and
    /// the volume it took.
    fn rounded(&self) -> (Body, usize, f64) {
        let b = self.body();
        let before = b.volume(1e-9).unwrap();
        let (start, at_v) = self.arc_angles();
        let (far, end, _) = self.top();
        let on_arc = self.about_centre(ARC, 0.5 * (start + at_v));
        let on_top = [0.5 * (far[0] + end[0]), 0.5 * (far[1] + end[1])];
        let edges = [
            edge_at(&b, self.point(on_arc, HEIGHT)),
            edge_at(&b, self.point(on_top, HEIGHT)),
        ];
        let made = blend(&b, Blend::Round, &edges, &[ROUND], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("bend {}: {e:?}", self.bend.to_degrees()));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(
            report.is_valid(),
            "bend {}: {report}",
            self.bend.to_degrees()
        );
        let after = b.volume(1e-9).unwrap();
        (b, made.made.len(), before - after)
    }

    /// Where the round's rail on the top (the circle of radius `ARC + ROUND` about the arc's
    /// centre) meets the rim past V (before mirroring): the two circles' crossing
    /// counter-clockwise from the line through their centres, from the triangle of the centres
    /// and the crossing.
    fn rail_meets_rim(&self) -> [f64; 2] {
        let c = self.centre();
        let apart = c[0].hypot(c[1]);
        let rail = ARC + ROUND;
        let along = (apart * apart + DISC * DISC - rail * rail) / (2.0 * apart);
        let across = (DISC * DISC - along * along).sqrt();
        let (ux, uy) = (c[0] / apart, c[1] / apart);
        // past V: counter-clockwise from the centres' line
        [along * ux - across * uy, along * uy + across * ux]
    }
}

/// Whether `b` has a vertex within 1e-6 of `p`.
fn has_vertex(b: &Body, p: [f64; 3]) -> bool {
    let (_, brep) = kernel_copy(b);
    brep.vertex_list().iter().any(|&v| {
        let q = vertex_get_point(&brep, v).unwrap();
        (q.x - p[0]).hypot(q.y - p[1]).hypot(q.z - p[2]) < 1e-6
    })
}

/// The kernel's flags for this test's thread: none (the default), or every fix.
fn flags(all: bool) {
    if all {
        common::all_fixes();
    } else {
        session::set_thread_fixes([]).unwrap();
    }
}

/// The rim leaves V away from the round, by a bend within the tangency tolerance (10 degrees)
/// that takes it deeper under the arc continued than the top face's zone tolerance (1e-5;
/// from about 0.05 degrees here): the round ends at V on a cap square to the arc. It takes
/// exactly its section along its two edges, its rails end in V's section (on the wall under
/// V, and on the top where the arc's radius through V meets the rail), and the rim is whole:
/// V is still its vertex and nothing ends where the rail on the top would meet it.
#[test]
fn a_round_ends_at_a_slight_bend_where_the_part_widens_past_it() {
    for all in [false, true] {
        flags(all);
        for mirrored in [false, true] {
            for deg in [0.1, 1.0, 6.0, 9.0] {
                let part = Part {
                    bend: f64::to_radians(deg),
                    mirrored,
                };
                let what = format!("every fix {all}, mirrored {mirrored}, bend {deg}");
                let (b, made, removed) = part.rounded();
                let want = along_its_edges();
                assert!(
                    (removed - want).abs() < 1e-6,
                    "{what}: removed {removed}, the section along the two edges {want}"
                );
                assert_eq!(made, 3, "{what}: two rounds and the cap");
                let v = vertex();
                let (_, at_v) = part.arc_angles();
                let on_top = part.about_centre(ARC + ROUND, at_v);
                assert!(
                    has_vertex(&b, part.point(v, HEIGHT - ROUND)),
                    "{what}: the wall rail's end"
                );
                assert!(
                    has_vertex(&b, part.point(on_top, HEIGHT)),
                    "{what}: the top rail's end"
                );
                assert!(has_vertex(&b, part.point(v, HEIGHT)), "{what}: V");
                assert!(
                    !has_vertex(&b, part.point(part.rail_meets_rim(), HEIGHT)),
                    "{what}: the round ran on to the rim"
                );
            }
        }
    }
}

/// Controls, which the rule for the bend must leave as they were: the arc tangent to the rim,
/// the rim leaving V away from the round by a bend that takes it no deeper than the tolerance
/// (1e-4 degrees: 4e-11), and the rim leaving V towards the round (6 degrees the other way).
/// The rim turns towards the round, so the round runs on past V and out through the disc's
/// wall: it ends on the rim where its rail on the top meets it, takes more than its section
/// along its two edges, and has no cap. (With every fix: without them the run-on's end rib is
/// refused, with or without a bend.)
#[test]
fn a_round_runs_out_through_the_wall_where_the_rim_turns_towards_it() {
    flags(true);
    for mirrored in [false, true] {
        for deg in [0.0, 1e-4, -6.0] {
            let part = Part {
                bend: f64::to_radians(deg),
                mirrored,
            };
            let what = format!("mirrored {mirrored}, bend {deg}");
            let (b, made, removed) = part.rounded();
            assert_eq!(made, 2, "{what}: two rounds, no cap");
            assert!(
                has_vertex(&b, part.point(part.rail_meets_rim(), HEIGHT)),
                "{what}: the round ends where its rail meets the rim"
            );
            assert!(
                removed > along_its_edges() + 1.0,
                "{what}: removed {removed}, more than the section along the two edges"
            );
        }
    }
}
