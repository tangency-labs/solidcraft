//! The operations against analytic volumes and topology counts (built with SMLib only). `SMLIB_OPS_SHOTS=<dir>` also
//! writes every result there as a native `.smb`, to render.
#![cfg(feature = "native")]

use super::*;
use crate::CheckLevel;

/// Relative accuracy of SMLib's volume integration in these tests.
const ACC: f64 = 1e-7;

fn cube(a: f64) -> Brep {
    Brep::make_box([0.0; 3], [a; 3]).unwrap()
}

fn volume(b: &Brep) -> f64 {
    b.volume(ACC).unwrap()
}

fn assert_close(got: f64, want: f64, rel: f64, what: &str) {
    assert!((got - want).abs() <= rel * want.abs(), "{what}: {got} vs analytic {want} (rel {:.3e})", (got - want).abs() / want.abs());
}

/// SMLib's own check, every test, finds nothing wrong with `b`; then, when `SMLIB_OPS_SHOTS` is
/// set, `b` is written there as `<name>.smb`.
fn shot(b: &Brep, name: &str) {
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{name}: {} failed validity tests:\n{}", report.failures, report.text);
    if let Some(dir) = std::env::var_os("SMLIB_OPS_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        b.write(&dir.join(format!("{name}.smb")), false).unwrap();
    }
}

/// The midpoints of the 12 edges of the cube [0, a]³.
fn cube_edge_midpoints(a: f64) -> Vec<[f64; 3]> {
    let h = a / 2.0;
    let mut v = Vec::new();
    for (p, q) in [(0.0, 0.0), (a, 0.0), (0.0, a), (a, a)] {
        v.push([h, p, q]);
        v.push([p, h, q]);
        v.push([p, q, h]);
    }
    v
}

/// The inputs of an operation are left as they were.
fn assert_untouched(b: &Brep, vol: f64, counts: [usize; 3]) {
    assert_close(volume(b), vol, 1e-9, "input volume");
    assert_eq!(b.counts().unwrap(), counts, "input counts");
}

#[test]
fn union_of_overlapping_boxes() {
    let a = cube(20.0);
    let b = Brep::make_box([10.0, 5.0, 5.0], [20.0, 10.0, 10.0]).unwrap();
    let u = a.boolean(&b, BooleanOp::Union).unwrap().unwrap();
    shot(&u, "union");
    // 20³ + 20·10·10 − the 10·10·10 overlap.
    assert_close(volume(&u), 9000.0, 1e-9, "union volume");
    // A's 6 faces (+x now with a hole) and B's 5 outside it; A's 12 edges, B's 8 outside and the
    // 4 where B passes through +x; A's 8 vertices and B's 8 outside.
    assert_eq!(u.counts().unwrap(), [11, 24, 16]);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_untouched(&a, 8000.0, [6, 12, 8]);
    assert_untouched(&b, 2000.0, [6, 12, 8]);
}

#[test]
fn subtract_cuts_a_slot() {
    let a = cube(20.0);
    // A 10 mm wide, 10 mm deep slot across the top, from y = −1 to 21.
    let tool = Brep::make_box([5.0, -1.0, 10.0], [10.0, 22.0, 15.0]).unwrap();
    let s = a.boolean(&tool, BooleanOp::Subtract).unwrap().unwrap();
    shot(&s, "subtract_slot");
    assert_close(volume(&s), 8000.0 - 10.0 * 20.0 * 10.0, 1e-9, "slot volume");
    // A prism over a U of 8 corners: 8 sides + 2 ends, 24 edges, 16 vertices.
    assert_eq!(s.counts().unwrap(), [10, 24, 16]);
    assert_eq!(s.solid_count().unwrap(), 1);
    assert_untouched(&a, 8000.0, [6, 12, 8]);
}

#[test]
fn intersect_keeps_the_overlap() {
    let a = cube(20.0);
    let b = Brep::make_box([10.0, 5.0, 5.0], [20.0, 10.0, 10.0]).unwrap();
    let i = a.boolean(&b, BooleanOp::Intersect).unwrap().unwrap();
    shot(&i, "intersect");
    assert_close(volume(&i), 1000.0, 1e-9, "intersection volume");
    assert_eq!(i.counts().unwrap(), [6, 12, 8]);
}

#[test]
fn empty_and_split_boolean_results() {
    let a = cube(10.0);
    let far = Brep::make_box([50.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
    assert!(a.boolean(&far, BooleanOp::Intersect).unwrap().is_none(), "disjoint intersection");
    let big = Brep::make_box([-5.0; 3], [20.0; 3]).unwrap();
    assert!(a.boolean(&big, BooleanOp::Subtract).unwrap().is_none(), "subtracting an enclosing body");
    let two = a.boolean(&far, BooleanOp::Union).unwrap().unwrap();
    shot(&two, "union_disjoint");
    assert_eq!(two.solid_count().unwrap(), 2, "a union of disjoint boxes is two solids in one body");
    assert_close(volume(&two), 2000.0, 1e-9, "disjoint union volume");
    // A body through a slot splits in two.
    let bar = cube(20.0);
    let cut = Brep::make_box([8.0, -1.0, -1.0], [4.0, 22.0, 22.0]).unwrap();
    let halves = bar.boolean(&cut, BooleanOp::Subtract).unwrap().unwrap();
    shot(&halves, "subtract_split");
    assert_eq!(halves.solid_count().unwrap(), 2);
    assert_close(volume(&halves), 8000.0 - 4.0 * 400.0, 1e-9, "split volume");
}

#[test]
fn fillet_every_edge_of_a_cube() {
    let (a, r) = (20.0, 2.0);
    let c = cube(a);
    let f = c.fillet(&cube_edge_midpoints(a), 0.1, r).unwrap();
    shot(&f, "fillet_all_edges");
    // The rounded cube is the Minkowski sum of the inner cube b = a − 2r with a ball of radius r:
    // b³ (core) + 6·b²·r (face slabs) + 12·(π r²/4)·b (quarter cylinders on the edges)
    // + 8·(π r³/6) (sphere octants at the corners) = 7804.6961… for a = 20, r = 2.
    let b = a - 2.0 * r;
    let pi = std::f64::consts::PI;
    let want = b.powi(3) + 6.0 * b * b * r + 3.0 * pi * r * r * b + 4.0 / 3.0 * pi * r.powi(3);
    let got = volume(&f);
    eprintln!("filleted cube: volume {got} (analytic {want}), counts {:?}", f.counts().unwrap());
    assert_close(got, want, 1e-6, "rounded cube volume");
    // 6 planar faces, 12 edge rounds, 8 corner patches.
    assert_eq!(f.counts().unwrap()[0], 26);
    assert_untouched(&c, 8000.0, [6, 12, 8]);
}

#[test]
fn chamfer_one_edge() {
    let c = cube(20.0);
    // The edge y = 20, z = 20, set back 3 mm along both faces: a prism of 3·3/2 × 20 goes.
    let ch = c.chamfer(&[[10.0, 20.0, 20.0]], 0.1, 3.0).unwrap();
    shot(&ch, "chamfer_one_edge");
    assert_close(volume(&ch), 8000.0 - 4.5 * 20.0, 1e-9, "chamfered volume");
    // One face more; the edge becomes two edges joined by two new ones; 2 vertices more.
    assert_eq!(ch.counts().unwrap(), [7, 15, 10]);
    assert_untouched(&c, 8000.0, [6, 12, 8]);
}

#[test]
fn shell_a_box_open_at_the_top() {
    let c = cube(20.0);
    let top = [[10.0, 10.0, 20.0]];
    let inward = c.shell(&top, 0.1, -2.0).unwrap();
    shot(&inward, "shell_inward");
    // The cavity is 16 × 16 × 18 (the floor is 2 thick, the top open).
    assert_close(volume(&inward), 8000.0 - 16.0 * 16.0 * 18.0, 1e-9, "inward shell volume");
    // Outside 5, inside 5, the rim 1.
    assert_eq!(inward.counts().unwrap()[0], 11);
    let outward = c.shell(&top, 0.1, 2.0).unwrap();
    shot(&outward, "shell_outward");
    // The outside grows to [−2, 22]² × [−2, 20] around the original box, which becomes the cavity.
    assert_close(volume(&outward), 24.0 * 24.0 * 22.0 - 8000.0, 1e-9, "outward shell volume");
    let closed = c.shell(&[], 0.1, -2.0).unwrap();
    shot(&closed, "shell_closed");
    assert_close(volume(&closed), 8000.0 - 16.0f64.powi(3), 1e-9, "closed hollow volume");
    assert_untouched(&c, 8000.0, [6, 12, 8]);
}

#[test]
fn fillet_takes_exactly_the_picked_edges() {
    // Round the vertical edge x = y = 20 first (r = 5): the top rim is then line, arc, line,
    // meeting tangentially.
    let c = cube(20.0);
    let round = c.fillet(&[[20.0, 20.0, 10.0]], 0.1, 5.0).unwrap();
    shot(&round, "fillet_one_edge");
    let pi = std::f64::consts::PI;
    assert_close(volume(&round), 8000.0 - (1.0 - pi / 4.0) * 25.0 * 20.0, 1e-6, "one rounded edge");
    // One point on the straight part of the rim rounds that edge only; the chain is rounded when
    // all of its edges are picked.
    let one = round.fillet(&[[7.5, 20.0, 20.0]], 0.1, 1.0).unwrap();
    shot(&one, "fillet_one_of_a_chain");
    let arc = [15.0 + 5.0 * (pi / 4.0).cos(), 15.0 + 5.0 * (pi / 4.0).sin(), 20.0];
    let all = round.fillet(&[[7.5, 20.0, 20.0], arc, [20.0, 7.5, 20.0]], 0.1, 1.0).unwrap();
    shot(&all, "fillet_chain_picked");
    assert_eq!(one.counts().unwrap()[0], round.counts().unwrap()[0] + 1, "one round on the picked edge");
    assert_eq!(all.counts().unwrap()[0], round.counts().unwrap()[0] + 3, "three rounds on the chain");
    assert!(volume(&all) < volume(&one), "the chain takes more than its one edge");
}

#[test]
fn hostile_inputs_are_errors() {
    let c = cube(20.0);
    let mid = [[10.0, 20.0, 20.0]];
    for r in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e7] {
        assert_eq!(c.fillet(&mid, 0.1, r).unwrap_err(), Error::Argument, "radius {r}");
        assert_eq!(c.chamfer(&mid, 0.1, r).unwrap_err(), Error::Argument, "distance {r}");
    }
    for t in [f64::NAN, 0.0, -1.0, f64::INFINITY] {
        assert_eq!(c.fillet(&mid, t, 1.0).unwrap_err(), Error::Argument, "tolerance {t}");
    }
    assert_eq!(c.fillet(&[[f64::NAN, 0.0, 0.0]], 0.1, 1.0).unwrap_err(), Error::Argument);
    assert_eq!(c.fillet(&[], 0.1, 1.0).unwrap_err(), Error::Argument);
    assert_eq!(c.fillet(&vec![[10.0, 20.0, 20.0]; MAX_POINTS + 1], 0.1, 1.0).unwrap_err(), Error::Argument);
    // A second point far from every edge.
    assert_eq!(c.fillet(&[mid[0], [100.0, 100.0, 100.0]], 0.1, 1.0).unwrap_err(), Error::NoMatch { point: 1 });
    assert_eq!(c.shell(&[[10.0, 10.0, 25.0]], 0.1, -1.0).unwrap_err(), Error::NoMatch { point: 0 });
    // A corner is equally near three edges, an edge equally near two faces.
    assert_eq!(c.fillet(&[[20.0, 20.0, 20.0]], 0.1, 1.0).unwrap_err(), Error::Ambiguous { point: 0 });
    assert_eq!(c.shell(&mid, 0.1, -1.0).unwrap_err(), Error::Ambiguous { point: 0 });
    for t in [0.0, f64::NAN, -1e7] {
        assert_eq!(c.shell(&[[10.0, 10.0, 20.0]], 0.1, t).unwrap_err(), Error::Argument, "thickness {t}");
    }
    // Sizes SMLib cannot build fail as errors, whatever SMLib says.
    match c.fillet(&mid, 0.1, 50.0) {
        Ok(b) => panic!("radius larger than the cube: SMLib built a body of volume {:?}, counts {:?}", b.volume(ACC), b.counts()),
        Err(e) => eprintln!("radius larger than the cube: {e:?}"),
    }
    // A chamfer past the faces runs over their far edges, cliffs: it is cut as if the faces went
    // on (the plane y + z = 10 through the setbacks 30 back from the edge y = z = 20), leaving the
    // triangle y + z <= 10 of the cube's section along x: 50 x 20.
    let past = c.chamfer(&mid, 0.1, 30.0).unwrap();
    assert!((volume(&past) - 1000.0).abs() < 1e-6 * 1000.0, "{}", volume(&past));
    // A wall with no room inside it never reaches SMLib's offset.
    assert_eq!(c.shell(&[[10.0, 10.0, 20.0]], 0.1, -15.0).unwrap_err(), Error::NoRoom);
    assert_eq!(c.offset_faces(&[[10.0, 10.0, 20.0]], 0.1, -20.0).unwrap_err(), Error::NoRoom);
    assert_eq!(c.thicken(1.0, false).unwrap_err(), Error::Argument, "a solid is no sheet");
    assert_untouched(&c, 8000.0, [6, 12, 8]);
}

#[test]
fn boolean_history_through_tagged_operands() {
    // A slot across the top of the cube: the cube's 6 faces give 7 result faces (the top is cut
    // in two), the tool gives the slot's 2 walls and floor; no face is new.
    let a = cube(20.0).tagged(0).unwrap();
    let tool = Brep::make_box([5.0, -1.0, 10.0], [10.0, 22.0, 15.0]).unwrap().tagged(1).unwrap();
    let s = a.boolean(&tool, BooleanOp::Subtract).unwrap().unwrap();
    shot(&s, "subtract_slot_tagged");
    let h = s.history().unwrap();
    assert_eq!(h.faces.len(), 10);
    let mut from = [0usize; 2];
    for f in &h.faces {
        assert!(!f.origins.is_empty(), "{f:?}");
        for &(operand, _) in &f.origins {
            from[operand.min(1)] += 1;
        }
    }
    assert_eq!(from, [7, 3], "{h:?}");
    // The 8 edges where the slot's walls and floor meet the top, front and back are new.
    assert_eq!(h.edges.iter().filter(|e| e.origins.is_empty()).count(), 8, "{h:?}");
}

/// `blend` gives each picked edge its own size: two parallel top edges of a 20 mm cube rounded at
/// 2 and 3 lose (1 - pi/4) r^2 L each, chamfered r^2 L / 2 each; a chord of 2 sqrt 2 on a right
/// angle is the round of radius 2.
#[test]
fn blends_take_a_size_per_edge() {
    let c = cube(20.0);
    let tops = [[10.0, 20.0, 20.0], [10.0, 0.0, 20.0]];
    let q = 1.0 - std::f64::consts::FRAC_PI_4;
    let r = c.blend(Blend::Round, &tops, &[2.0, 3.0], 0.1).unwrap();
    shot(&r, "blend-round-2-3");
    assert_close(volume(&r), 8000.0 - q * 13.0 * 20.0, 1e-6, "rounds 2 and 3");
    let ch = c.blend(Blend::Chamfer, &tops, &[2.0, 3.0], 0.1).unwrap();
    shot(&ch, "blend-chamfer-2-3");
    assert_close(volume(&ch), 8000.0 - 0.5 * 13.0 * 20.0, 1e-6, "chamfers 2 and 3");
    let chord = c.blend(Blend::Chord, &tops[..1], &[2.0 * 2f64.sqrt()], 0.1).unwrap();
    assert_close(volume(&chord), 8000.0 - q * 4.0 * 20.0, 2e-6, "chord");
    assert!(c.blend(Blend::Round, &tops, &[2.0], 0.1).is_err());
}

/// A radius running linearly from 3 to 1 along a cube edge: (1 - pi/4) L (r0² + r0 r1 + r1²) / 3
/// taken away, whichever end is the start.
#[test]
fn a_variable_round_follows_its_law() {
    let c = cube(20.0);
    let q = 1.0 - std::f64::consts::FRAC_PI_4;
    let knots = [0.0, 0.0, 1.0, 1.0];
    let r = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 1, &knots, &[3.0, 1.0]).unwrap();
    shot(&r, "variable-3-1");
    assert_close(volume(&r), 8000.0 - q * 20.0 * 13.0 / 3.0, 1e-5, "linear law");
    let back = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [20.0, 20.0, 20.0], 1, &knots, &[1.0, 3.0]).unwrap();
    assert_close(volume(&back), volume(&r), 1e-7, "reversed");
    assert!(c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 1, &[0.0, 0.5, 1.0, 1.0], &[3.0, 1.0]).is_err());
    assert!(matches!(c.fillet_variable([90.0, 90.0, 90.0], 0.1, [0.0; 3], 1, &knots, &[3.0, 1.0]), Err(Error::NoMatch { .. })));
    // A quadratic law with control radii 4, 1, 4: r(t) = 4(1 − t)² + 2t(1 − t) + 4t².
    let law = |t: f64| 4.0 * (1.0 - t).powi(2) + 2.0 * t * (1.0 - t) + 4.0 * t * t;
    let n = 4000;
    let r2 = (0..n).map(|i| law((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64;
    let quad = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[4.0, 1.0, 4.0]).unwrap();
    shot(&quad, "variable-quadratic");
    assert_close(volume(&quad), 8000.0 - q * 20.0 * r2, 1e-5, "quadratic law");
    // Down to zero at the far end: the round closes to the edge's end point.
    let zero = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 1, &knots, &[4.0, 0.0]).unwrap();
    shot(&zero, "variable-to-zero");
    assert_close(volume(&zero), 8000.0 - q * 20.0 * 16.0 / 3.0, 1e-5, "down to zero");
    // A law with a corner (4 → 2 → 4, straight on both halves): the round turns at the corner,
    // each half taking away (1 - pi/4) (L / 2) (16 + 8 + 4) / 3; the same points joined smoothly
    // (C1 cubic pieces, as RadiusSpline::through makes them) are built too.
    let corner = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[4.0, 2.0, 4.0]).unwrap();
    shot(&corner, "variable-corner-4-2-4");
    assert_close(volume(&corner), 8000.0 - q * 20.0 * 28.0 / 3.0, 1e-6, "corner law");
    // A law down to zero inside the edge would pinch the round to a point: refused.
    let pinch = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[4.0, 0.0, 4.0]);
    assert!(pinch.is_err(), "a law pinching to zero inside the edge");
    let smooth_knots = [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
    let smooth = c.fillet_variable([10.0, 20.0, 20.0], 0.1, [0.0, 20.0, 20.0], 3, &smooth_knots, &[4.0, 10.0 / 3.0, 2.0, 2.0, 2.0, 10.0 / 3.0, 4.0]);
    shot(&smooth.unwrap(), "variable-smooth-4-2-4");
}

/// Unequal chamfers of a cube's top edge y = 20: 5 back along the face facing `first` most and 2
/// along the other take away a triangle 5 × 2 / 2 along the 20 mm edge, either way round; 5 at
/// 30° to the first face is 5 and 5 tan 30°.
#[test]
fn chamfers_with_two_setbacks() {
    let c = cube(20.0);
    let top = [[10.0, 20.0, 20.0]];
    let up = c.chamfer_sides(&top, 0.1, 5.0, ChamferSecond::Distance(2.0), [0.0, 0.0, 1.0]).unwrap();
    shot(&up, "chamfer-5-2");
    assert_close(volume(&up), 8000.0 - 5.0 * 20.0, 1e-9, "5 and 2");
    let flipped = c.chamfer_sides(&top, 0.1, 5.0, ChamferSecond::Distance(2.0), [0.0, 0.0, -1.0]).unwrap();
    assert_close(volume(&flipped), volume(&up), 1e-9, "flipped");
    // Which face took the 5: the removed prism's centroid lies a third of each setback in from
    // the edge, so the result's centroid moves away from it.
    let centroid = |b: &Brep| b.mass_properties(ACC).unwrap().centroid;
    let left = |dy: f64, dz: f64| [10.0, (8000.0 * 10.0 - 100.0 * (20.0 - dy / 3.0)) / 7900.0, (8000.0 * 10.0 - 100.0 * (20.0 - dz / 3.0)) / 7900.0];
    for (b, want, what) in [(&up, left(5.0, 2.0), "5 along the top"), (&flipped, left(2.0, 5.0), "5 down the side")] {
        let got = centroid(b);
        assert!((0..3).all(|i| (got[i] - want[i]).abs() < 1e-6), "{what}: centroid {got:?}, want {want:?}");
    }
    let angle = c.chamfer_sides(&top, 0.1, 5.0, ChamferSecond::Angle(30f64.to_radians()), [0.0, 0.0, 1.0]).unwrap();
    shot(&angle, "chamfer-5-at-30");
    assert_close(volume(&angle), 8000.0 - 5.0 * 5.0 * 30f64.to_radians().tan() / 2.0 * 20.0, 1e-9, "5 at 30 degrees");
    assert_eq!(c.chamfer_sides(&top, 0.1, 5.0, ChamferSecond::Angle(4.0), [0.0, 0.0, 1.0]).unwrap_err(), Error::Argument);
    assert_eq!(c.chamfer_sides(&top, 0.1, 5.0, ChamferSecond::Distance(2.0), [0.0; 3]).unwrap_err(), Error::Argument);
    assert_eq!(c.chamfer_sides(&top, 0.1, 5.0, ChamferSecond::Distance(-2.0), [0.0, 0.0, 1.0]).unwrap_err(), Error::Argument);
}

#[test]
fn imprint_cuts_faces_where_a_tool_meets_them() {
    // A box half sunk into the cube cuts the four side faces it crosses and the top it lies in.
    let cube = cube(20.0);
    let tool = Brep::make_box([5.0, -1.0, 10.0], [10.0, 22.0, 15.0]).unwrap();
    let cut = cube.imprint(&tool).unwrap().unwrap();
    assert!(cut.counts().unwrap()[0] > 6);
    assert!((cut.volume(1e-7).unwrap() - 8000.0).abs() < 1e-6);
    assert_eq!(cube.counts().unwrap()[0], 6);
    // A tool that misses cuts nothing.
    let far = Brep::make_box([50.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
    assert!(cube.imprint(&far).unwrap().is_none());
}

#[test]
fn draft_tilts_a_side_about_its_line_on_the_neutral_plane() {
    let c = cube(20.0);
    let deg = std::f64::consts::PI / 180.0;
    let wedge = 0.5 * 20.0 * 20.0 * (5.0 * deg).tan() * 20.0;
    let side = [[20.0, 10.0, 10.0]];
    let up = [0.0, 0.0, 1.0];
    let leaned = c.draft(&side, 0.1, [0.0; 3], up, up, 5.0 * deg).unwrap();
    assert!((leaned.volume(ACC).unwrap() - (8000.0 - wedge)).abs() < 1e-6);
    let out = c.draft(&side, 0.1, [0.0; 3], up, up, -5.0 * deg).unwrap();
    assert!((out.volume(ACC).unwrap() - (8000.0 + wedge)).abs() < 1e-6);
    // The top isn't parallel to the pull.
    assert_eq!(c.draft(&[[10.0, 10.0, 20.0]], 0.1, [0.0; 3], up, up, 5.0 * deg).unwrap_err(), Error::Unsupported);
    assert_eq!(c.draft(&side, 0.1, [0.0; 3], up, up, 0.0).unwrap_err(), Error::Argument);
    assert_eq!(c.draft(&[], 0.1, [0.0; 3], up, up, 5.0 * deg).unwrap_err(), Error::Argument);
    assert_untouched(&c, 8000.0, [6, 12, 8]);
}
