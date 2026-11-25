//! What a body has on and past a round face: each reading against a body of revolution given as
//! plain numbers (pieces whose inner and outer radii run straight along the axis), whose probes
//! are answered by hand, and hostile numbers (not finite, no frame, probes that cannot be read)
//! refused or answered plainly, never a panic.

use std::cell::Cell;

use crate::eval::Frame;
use crate::math::Transform;
use crate::teeth::round_face::{
    beside_clear, extent, far_end, grow_block, into_seat, past_end, stands_over, whole_band,
    BandEdge, BandFace, End, Extent, Met, Probe, Round, RoundFaceRefusal, MOST_PROBES,
};
use crate::teeth::seat::SeatFrame;
use crate::Vec3;

/// Numbers no reading may panic on.
const HOSTILE: [f64; 10] = [
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    -1.0,
    0.0,
    1e-300,
    1e300,
];

/// A piece of a body of revolution about the seat's axis: from `z0` to `z1` along it, its inner
/// radius running straight from `in0` to `in1` and its outer from `out0` to `out1`.
#[derive(Clone, Copy, Debug)]
struct Turned {
    z0: f64,
    z1: f64,
    in0: f64,
    in1: f64,
    out0: f64,
    out1: f64,
}

/// A solid piece of radius `r` from `z0` to `z1`.
fn solid(z0: f64, z1: f64, r: f64) -> Turned {
    Turned {
        z0,
        z1,
        in0: 0.0,
        in1: 0.0,
        out0: r,
        out1: r,
    }
}

/// A ring from `z0` to `z1` with the hole `hole` and the outside `out`.
fn ring(z0: f64, z1: f64, hole: f64, out: f64) -> Turned {
    Turned {
        z0,
        z1,
        in0: hole,
        in1: hole,
        out0: out,
        out1: out,
    }
}

/// The part of `lo`..`hi` where a value running straight from `at_lo` to `at_hi` is above zero.
fn above_zero(lo: f64, hi: f64, at_lo: f64, at_hi: f64) -> Option<(f64, f64)> {
    match (at_lo > 0.0, at_hi > 0.0) {
        (true, true) => Some((lo, hi)),
        (false, false) => None,
        (first, _) => {
            let z = lo + at_lo / (at_lo - at_hi) * (hi - lo);
            if first {
                Some((lo, z))
            } else {
                Some((z, hi))
            }
        }
    }
}

impl Turned {
    /// Its inner and outer radius at `z`.
    fn at(&self, z: f64) -> (f64, f64) {
        let t = (z - self.z0) / (self.z1 - self.z0);
        (
            self.in0 + (self.in1 - self.in0) * t,
            self.out0 + (self.out1 - self.out0) * t,
        )
    }

    /// The box of what of it lies within the probe's ring over its stretch.
    fn met(&self, p: &Probe) -> Option<([f64; 3], [f64; 3])> {
        let pin = p.inner.unwrap_or(0.0);
        let (mut lo, mut hi) = (self.z0.max(p.from), self.z1.min(p.to));
        if !(lo < hi) {
            return None;
        }
        // The piece's hole inside the ring's outside.
        (lo, hi) = above_zero(lo, hi, p.outer - self.at(lo).0, p.outer - self.at(hi).0)?;
        if !(lo < hi) {
            return None;
        }
        // The ring's hole inside the piece's outside.
        (lo, hi) = above_zero(lo, hi, self.at(lo).1 - pin, self.at(hi).1 - pin)?;
        if !(lo < hi) {
            return None;
        }
        let across = self.at(lo).1.max(self.at(hi).1).min(p.outer);
        Some(([-across, -across, lo], [across, across, hi]))
    }
}

/// A body of revolution about the seat's axis.
struct Body(Vec<Turned>);

impl Body {
    fn extent(&self) -> Extent {
        Extent {
            low: self.0.iter().map(|t| t.z0).fold(f64::INFINITY, f64::min),
            high: self
                .0
                .iter()
                .map(|t| t.z1)
                .fold(f64::NEG_INFINITY, f64::max),
            across: self
                .0
                .iter()
                .map(|t| t.out0.max(t.out1))
                .fold(0.0, f64::max),
        }
    }

    /// What the probe meets, as the box of the union of what it meets of each piece.
    fn met(&self, p: &Probe) -> Met {
        let boxes: Vec<_> = self.0.iter().filter_map(|t| t.met(p)).collect();
        if boxes.is_empty() {
            return Met::Nothing;
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for (lo, hi) in boxes {
            for (m, l) in min.iter_mut().zip(lo) {
                *m = m.min(l);
            }
            for (m, h) in max.iter_mut().zip(hi) {
                *m = m.max(h);
            }
        }
        Met::Piece { min, max }
    }
}

/// The plane z = 0 with x along +X.
fn xy() -> Frame {
    Frame {
        origin: [0.0, 0.0, 0.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    }
}

/// A round face of radius 10 whose free end is at z = 0, running on along +Z for 30.
fn seat() -> SeatFrame {
    SeatFrame {
        plane: xy(),
        inward: 1.0,
        room: 30.0,
        at: Vec3::new(10.0, 0.0, 15.0),
    }
}

/// A shaft of radius 10 from z = 0 to 30, with `past` past its free end.
fn shaft(past: &[Turned]) -> Body {
    let mut pieces = vec![solid(0.0, 30.0, 10.0)];
    pieces.extend_from_slice(past);
    Body(pieces)
}

/// A hub of radius 20 with a bore of radius 5 from z = 0 to 30, with `past` past the bore's end.
fn hub(past: &[Turned]) -> Body {
    let mut pieces = vec![ring(0.0, 30.0, 5.0, 20.0)];
    pieces.extend_from_slice(past);
    Body(pieces)
}

/// What lies past the free end of the seat's face of `radius` on `body`, asking no more than
/// [`MOST_PROBES`] probes.
fn end_of(body: &Body, radius: f64, bore: bool, reach: f64) -> End {
    let asked = Cell::new(0);
    let mut probe = |p: &Probe| {
        asked.set(asked.get() + 1);
        body.met(p)
    };
    let end = past_end(
        &seat(),
        radius,
        bore,
        reach,
        Some(body.extent()),
        &mut probe,
    );
    assert!(asked.get() <= MOST_PROBES, "{} probes", asked.get());
    end
}

#[test]
fn a_free_end_has_nothing_past_it() {
    let end = end_of(&shaft(&[]), 10.0, false, 9.0);
    assert!(
        end.free && end.open && end.out && !end.step && !end.cone,
        "{end:?}"
    );
    assert_eq!((end.end, end.through, end.run_out()), (0.0, 0.0, 0.0));
    assert_eq!(end.run_past(0.1), 0.1);
}

#[test]
fn a_shoulder_shuts_the_end() {
    let end = end_of(&shaft(&[solid(-5.0, 0.0, 15.0)]), 10.0, false, 9.0);
    assert_eq!(end, End::shut(10.0));
    assert!(!end.open && !end.out);
    assert_eq!((end.run_out(), end.run_past(0.1)), (0.0, 0.0));
}

#[test]
fn a_neck_is_a_step_with_its_radius() {
    let end = end_of(&shaft(&[solid(-20.0, 0.0, 6.0)]), 10.0, false, 9.0);
    assert!(end.open && end.out && end.step && !end.free, "{end:?}");
    assert!(
        (end.neck - 6.0).abs() < 1e-12 && end.through == 0.0,
        "{end:?}"
    );
    assert_eq!(end.run_past(0.1), 0.0);
}

/// A cone past the end: radius `rim` at z = -`long`, the face's radius 10 at z = 0.
fn chamfer(long: f64, rim: f64) -> Turned {
    Turned {
        z0: -long,
        z1: 0.0,
        in0: 0.0,
        in1: 0.0,
        out0: rim,
        out1: 10.0,
    }
}

#[test]
fn a_chamfer_is_read_as_a_cone_with_its_rim() {
    let end = end_of(&shaft(&[chamfer(1.0, 9.0)]), 10.0, false, 9.5);
    assert!(end.open && end.out && end.cone && !end.step, "{end:?}");
    assert!((end.end - 1.0).abs() < 1e-12, "{end:?}");
    assert!((end.rim - 9.0).abs() < 1e-9, "{end:?}");
    // The cuts reach radius 9.5: the chamfer is that wide half way along it.
    assert!((end.through - 0.5).abs() < 1e-4, "{end:?}");
    assert!((end.run_out() - 1.0).abs() < 1e-12);
    assert!((end.run_past(0.1) - 1.1).abs() < 1e-12);
}

/// A chamfer goes with a moved face along its own cone: how far further into the face that cone
/// meets the new surface is the chamfer's slope times the move.
#[test]
fn a_carried_chamfer_reaches_in_by_its_cone() {
    // A shaft of radius 10 with a 1 by 1 chamfer on its end: the rim at 9, 1 mm long.
    let end = End {
        open: true,
        out: true,
        cone: true,
        end: 1.0,
        rim: 9.0,
        ..End::shut(10.0)
    };
    // Grown to radius 12, the cone (1 along for each 1 across) meets the surface 2 mm further
    // in; left as it is, no further; turned down to 9.5, half a millimetre before the end's
    // plane. Every number here is exact in binary.
    assert_eq!(end.carried(10.0, 12.0), 2.0);
    assert_eq!(end.carried(10.0, 10.0), 0.0);
    assert_eq!(end.carried(10.0, 9.5), -0.5);
    // A bore of radius 10 with a 1 by 2 entry chamfer (its rim at 12, 1 mm long): made smaller
    // to radius 8, the cone (1 along for each 2 across) meets the bore 1 mm further in.
    let entry = End { rim: 12.0, ..end };
    assert_eq!(entry.carried(10.0, 8.0), 1.0);
    // No chamfer of its own: a round (no cone), no length, a shut end.
    assert_eq!(End { cone: false, ..end }.carried(10.0, 12.0), 0.0);
    assert_eq!(End { end: 0.0, ..end }.carried(10.0, 12.0), 0.0);
    assert_eq!(End::shut(10.0).carried(10.0, 12.0), 0.0);
    // A rim on the face's own radius gives no slope: no length, not an infinite one.
    assert_eq!(End { rim: 10.0, ..end }.carried(10.0, 12.0), 0.0);
    for bad in HOSTILE {
        assert!(end.carried(bad, 12.0).is_finite(), "{bad}");
        assert!(end.carried(10.0, bad).is_finite(), "{bad}");
        assert!(End { end: bad, ..end }.carried(10.0, 12.0).is_finite());
        assert!(End { rim: bad, ..end }.carried(10.0, 12.0).is_finite());
    }
}

#[test]
fn a_round_is_told_from_a_chamfer_by_its_slope() {
    // A round of radius 1 on the end: r(z) = 9 + sqrt(1 - z^2) from z = -1 to 0, in short
    // straight pieces.
    let n = 2000;
    let r = |z: f64| 9.0 + (1.0 - z * z).max(0.0).sqrt();
    let pieces: Vec<Turned> = (0..n)
        .map(|i| {
            let z0 = -1.0 + f64::from(i) / f64::from(n);
            let z1 = -1.0 + f64::from(i + 1) / f64::from(n);
            Turned {
                z0,
                z1,
                in0: 0.0,
                in1: 0.0,
                out0: r(z0),
                out1: r(z1),
            }
        })
        .collect();
    let end = end_of(&shaft(&pieces), 10.0, false, 9.5);
    assert!(end.open && end.out && !end.cone && !end.step, "{end:?}");
    assert!((end.end - 1.0).abs() < 1e-12, "{end:?}");
    // The rim is where the round is near the body's end, not where a chamfer would put it.
    assert!(end.rim > 9.1 && end.rim < 9.2, "{end:?}");
    assert!((end.through - 0.75_f64.sqrt()).abs() < 1e-3, "{end:?}");
}

#[test]
fn a_long_taper_is_open_with_no_end_of_its_own() {
    // A cone down to radius 2 over 40 past the end: longer than the face's radius.
    let end = end_of(&shaft(&[chamfer(40.0, 2.0)]), 10.0, false, 9.0);
    assert!(end.open && end.out && !end.cone && !end.step, "{end:?}");
    assert_eq!((end.end, end.rim), (0.0, 10.0));
    assert!((end.through - 5.0).abs() < 1e-4, "{end:?}");
}

#[test]
fn a_counterbore_is_a_step_and_an_entry_chamfer_a_cone() {
    let end = end_of(&hub(&[ring(-4.0, 0.0, 8.0, 20.0)]), 5.0, true, 5.5);
    assert!(end.open && end.out && end.step && !end.cone, "{end:?}");
    // The entry chamfered 1 by 1: the bore's mouth is 6 at z = -1.
    let entry = Turned {
        z0: -1.0,
        z1: 0.0,
        in0: 6.0,
        in1: 5.0,
        out0: 20.0,
        out1: 20.0,
    };
    let end = end_of(&hub(&[entry]), 5.0, true, 5.5);
    assert!(end.open && end.out && end.cone && !end.step, "{end:?}");
    assert!(
        (end.end - 1.0).abs() < 1e-12 && (end.rim - 6.0).abs() < 1e-6,
        "{end:?}"
    );
    assert!((end.through - 0.5).abs() < 1e-4, "{end:?}");
    // A bore that narrows past its end shuts it.
    let end = end_of(&hub(&[ring(-4.0, 0.0, 3.0, 20.0)]), 5.0, true, 5.5);
    assert_eq!(end, End::shut(5.0));
}

#[test]
fn what_stands_over_a_stretch_is_its_largest_radius() {
    let body = shaft(&[solid(-5.0, 0.0, 15.0)]);
    let mut probe = |p: &Probe| body.met(p);
    let over = stands_over(&seat(), 10.0, body.extent(), 10.0, -5.0, 0.0, &mut probe);
    assert!((over.unwrap() - 15.0).abs() < 1e-12);
    // Nothing past the far end, an empty stretch, a body no wider than `above`: `above`.
    assert_eq!(
        stands_over(&seat(), 10.0, body.extent(), 10.0, 30.0, 35.0, &mut probe),
        Ok(10.0)
    );
    assert_eq!(
        stands_over(&seat(), 10.0, body.extent(), 10.0, 0.0, 0.0, &mut probe),
        Ok(10.0)
    );
    assert_eq!(
        stands_over(&seat(), 10.0, body.extent(), 16.0, -5.0, 0.0, &mut probe),
        Ok(16.0)
    );
    // A reading that cannot be made is refused, never taken as nothing there.
    let mut unread = |_: &Probe| Met::Unread;
    assert_eq!(
        stands_over(&seat(), 10.0, body.extent(), 10.0, -5.0, 0.0, &mut unread),
        Err(RoundFaceRefusal::Probe)
    );
}

#[test]
fn the_stretch_beside_a_bore_s_step_is_clear_within_its_ledge() {
    let body = hub(&[ring(-4.0, 0.0, 8.0, 20.0)]);
    let mut probe = |p: &Probe| body.met(p);
    assert_eq!(beside_clear(&seat(), 5.0, 6.0, &mut probe), Ok(true));
    assert_eq!(beside_clear(&seat(), 5.0, 9.0, &mut probe), Ok(false));
    let mut unread = |_: &Probe| Met::Unread;
    assert_eq!(
        beside_clear(&seat(), 5.0, 6.0, &mut unread),
        Err(RoundFaceRefusal::Probe)
    );
}

#[test]
fn the_map_into_a_seat_takes_its_plane_to_the_origin() {
    // A plane through (1, 2, 3) with x along +Y and y along +Z: its normal is +X.
    let s = SeatFrame {
        plane: Frame {
            origin: [1.0, 2.0, 3.0],
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        },
        ..seat()
    };
    let m = into_seat(&s).unwrap();
    let p = Transform { m }.transform_point(Vec3::new(6.0, 9.0, 14.0));
    assert!((p - Vec3::new(7.0, 11.0, 5.0)).length() < 1e-12, "{p:?}");
    // The box of a body there reads as its extent: below zero past the end.
    let e = extent(&seat(), [-3.0, -4.0, -2.0], [5.0, 1.0, 30.0]).unwrap();
    assert_eq!((e.low, e.high, e.across), (-2.0, 30.0, 5.0));
    let back = SeatFrame {
        inward: -1.0,
        ..seat()
    };
    let e = extent(&back, [-3.0, -4.0, -2.0], [5.0, 1.0, 30.0]).unwrap();
    assert_eq!((e.low, e.high), (-30.0, 2.0));
}

#[test]
fn the_far_end_is_the_free_end_seen_from_the_other_side() {
    let far = far_end(&seat());
    assert_eq!(far.plane.origin, [0.0, 0.0, 30.0]);
    assert_eq!((far.inward, far.room), (-1.0, 30.0));
    let back = far_end(&far);
    assert_eq!((back.plane.origin, back.inward), ([0.0, 0.0, 0.0], 1.0));
}

#[test]
fn the_block_of_a_grown_stretch_is_wider_than_the_body_and_the_face() {
    let body = Extent {
        low: -5.0,
        high: 30.0,
        across: 15.0,
    };
    let block = grow_block(&seat(), 10.0, body, -2.0, 1.0, 0.5).unwrap();
    assert_eq!((block.radius, block.from, block.to), (28.0, -1.0, 30.5));
    let back = SeatFrame {
        inward: -1.0,
        ..seat()
    };
    let block = grow_block(&back, 10.0, body, 2.0, 1.0, 0.5).unwrap();
    assert_eq!((block.from, block.to), (-30.5, 1.0));
    // A stretch of no length.
    assert_eq!(
        grow_block(&seat(), 10.0, body, 2.0, -30.0, 0.0),
        Err(RoundFaceRefusal::Number)
    );
}

/// A frame at `origin` whose axis (x cross y) is `axis` (+Z or +X).
fn frame(origin: [f64; 3], along_z: bool) -> Frame {
    if along_z {
        Frame {
            origin,
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }
    } else {
        Frame {
            origin,
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        }
    }
}

/// An edge held as a circle of `radius` in `frame`.
fn circle(index: usize, frame: Frame, radius: f64) -> BandEdge {
    BandEdge {
        index,
        circle: Some(Round { frame, radius }),
        ..BandEdge::default()
    }
}

/// An edge held as a line from `a` to `b`.
fn line(index: usize, a: Vec3, b: Vec3) -> BandEdge {
    BandEdge {
        index,
        line: true,
        ends: Some([a, b]),
        ..BandEdge::default()
    }
}

/// The seat's cylinder as a face bounded by `edges`.
fn band(edges: Vec<BandEdge>) -> BandFace {
    BandFace {
        cylinder: Some(Round {
            frame: frame([0.0, 0.0, -7.0], true),
            radius: 10.0,
        }),
        edges,
    }
}

/// The edges of a whole band: its two rims and its seam.
fn rims_and_seam() -> Vec<BandEdge> {
    vec![
        circle(0, frame([0.0, 0.0, 0.0], true), 10.0),
        circle(1, frame([0.0, 0.0, 30.0], true), 10.0),
        line(2, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 30.0)),
    ]
}

#[test]
fn a_band_is_whole_only_with_rims_and_seams() {
    let end_disc = BandFace {
        cylinder: None,
        edges: vec![circle(0, frame([0.0; 3], true), 10.0)],
    };
    let whole = [band(rims_and_seam()), end_disc];
    assert_eq!(whole_band(&seat(), 10.0, &whole), Ok(true));
    // A hole across the shaft opens in the face: a circle about another axis.
    let mut edges = rims_and_seam();
    edges.push(circle(3, frame([10.0, 0.0, 15.0], false), 2.0));
    assert_eq!(whole_band(&seat(), 10.0, &[band(edges)]), Ok(false));
    // A rim and a seam held as other curves, read from points along them.
    let rim: Vec<Vec3> = (1..24)
        .map(|i| {
            let t = f64::from(i) / 24.0 * std::f64::consts::TAU;
            Vec3::new(10.0 * t.cos(), 10.0 * t.sin(), 0.0)
        })
        .collect();
    let (a, b) = (Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 30.0));
    let seam: Vec<Vec3> = (1..24)
        .map(|i| a + (b - a) * (f64::from(i) / 24.0))
        .collect();
    let sampled = vec![
        BandEdge {
            index: 0,
            ends: Some([a, a]),
            points: rim,
            ..BandEdge::default()
        },
        circle(1, frame([0.0, 0.0, 30.0], true), 10.0),
        BandEdge {
            index: 2,
            ends: Some([a, b]),
            points: seam,
            ..BandEdge::default()
        },
    ];
    assert_eq!(whole_band(&seat(), 10.0, &[band(sampled)]), Ok(true));
    // A curve along the face that is neither: a helix's points.
    let helix: Vec<Vec3> = (1..24)
        .map(|i| {
            let t = f64::from(i) / 24.0;
            Vec3::new(10.0 * (t * 3.0).cos(), 10.0 * (t * 3.0).sin(), 30.0 * t)
        })
        .collect();
    let mut edges = rims_and_seam();
    edges.push(BandEdge {
        index: 3,
        ends: Some([
            a,
            Vec3::new(10.0 * 3.0_f64.cos(), 10.0 * 3.0_f64.sin(), 30.0),
        ]),
        points: helix,
        ..BandEdge::default()
    });
    assert_eq!(whole_band(&seat(), 10.0, &[band(edges)]), Ok(false));
    // An edge known by nothing does not bound the band.
    let mut edges = rims_and_seam();
    edges.push(BandEdge {
        index: 9,
        ..BandEdge::default()
    });
    assert_eq!(whole_band(&seat(), 10.0, &[band(edges)]), Ok(false));
    // No face on the seat's cylinder: one of another radius, one about another axis.
    let other = BandFace {
        cylinder: Some(Round {
            frame: frame([0.0; 3], true),
            radius: 12.0,
        }),
        edges: rims_and_seam(),
    };
    let askew = BandFace {
        cylinder: Some(Round {
            frame: frame([0.0; 3], false),
            radius: 10.0,
        }),
        edges: rims_and_seam(),
    };
    assert_eq!(
        whole_band(&seat(), 10.0, &[other, askew]),
        Err(RoundFaceRefusal::NoBand)
    );
    assert_eq!(
        whole_band(&seat(), 10.0, &[]),
        Err(RoundFaceRefusal::NoBand)
    );
}

#[test]
fn hostile_numbers_are_refused_or_answered_plainly() {
    let body = shaft(&[chamfer(1.0, 9.0)]);
    let flat = SeatFrame {
        plane: Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [1.0, 0.0, 0.0],
        },
        ..seat()
    };
    assert_eq!(into_seat(&flat), Err(RoundFaceRefusal::Seat));
    assert_eq!(
        whole_band(&flat, 10.0, &[band(rims_and_seam())]),
        Err(RoundFaceRefusal::Seat)
    );
    let mut unread = |_: &Probe| Met::Unread;
    let mut garbage = |_: &Probe| Met::Piece {
        min: [f64::NAN; 3],
        max: [f64::INFINITY; 3],
    };
    // An unread body, unread probes and probes that answer nonsense shut the end or leave it
    // open with nothing read past it.
    assert_eq!(
        past_end(&seat(), 10.0, false, 9.0, None, &mut unread),
        End::shut(10.0)
    );
    assert_eq!(
        past_end(&seat(), 10.0, false, 9.0, Some(body.extent()), &mut unread),
        End::shut(10.0)
    );
    let end = past_end(&seat(), 10.0, true, 9.0, Some(body.extent()), &mut garbage);
    assert!(!end.out && end.end == 0.0, "{end:?}");
    for bad in HOSTILE {
        let s = SeatFrame {
            plane: Frame {
                origin: [bad, 0.0, 0.0],
                ..xy()
            },
            inward: bad,
            room: bad,
            ..seat()
        };
        let asked = Cell::new(0);
        let mut probe = |p: &Probe| {
            asked.set(asked.get() + 1);
            body.met(p)
        };
        for bore in [false, true] {
            let _ = past_end(&seat(), bad, bore, 9.0, Some(body.extent()), &mut probe);
            let _ = past_end(&seat(), 10.0, bore, bad, Some(body.extent()), &mut probe);
            let low = Extent {
                low: bad,
                ..body.extent()
            };
            let _ = past_end(&seat(), 10.0, bore, 9.0, Some(low), &mut probe);
            let _ = past_end(&s, 10.0, bore, 9.0, Some(body.extent()), &mut probe);
        }
        assert!(
            asked.get() <= 8 * MOST_PROBES,
            "{bad}: {} probes",
            asked.get()
        );
        let _ = stands_over(&s, 10.0, body.extent(), 10.0, -1.0, 0.0, &mut probe);
        let _ = stands_over(&seat(), 10.0, body.extent(), 10.0, bad, 0.0, &mut probe);
        let _ = stands_over(&seat(), 10.0, body.extent(), 10.0, -1.0, bad, &mut probe);
        let _ = beside_clear(&s, 10.0, 5.0, &mut probe);
        let _ = into_seat(&s);
        let _ = far_end(&s);
        let _ = whole_band(&s, 10.0, &[band(rims_and_seam())]);
        if !bad.is_finite() {
            assert_eq!(
                stands_over(&seat(), 10.0, body.extent(), bad, -1.0, 0.0, &mut probe),
                Err(RoundFaceRefusal::Number)
            );
            assert!(extent(&seat(), [bad; 3], [1.0; 3]).is_err());
            assert!(grow_block(&seat(), 10.0, body.extent(), bad, 0.0, 0.0).is_err());
            assert!(grow_block(&seat(), 10.0, body.extent(), 1.0, bad, 0.0).is_err());
        }
        if !(bad.is_finite() && bad > 0.0) {
            let end = past_end(&seat(), bad, false, 9.0, Some(body.extent()), &mut probe);
            assert!(!end.open && !end.out, "{bad}: {end:?}");
            assert_eq!(
                stands_over(&seat(), bad, body.extent(), 10.0, -1.0, 0.0, &mut probe),
                Err(RoundFaceRefusal::Number)
            );
            assert_eq!(
                beside_clear(&seat(), 10.0, bad, &mut probe),
                Err(RoundFaceRefusal::Number)
            );
            assert_eq!(
                beside_clear(&seat(), bad, 5.0, &mut probe),
                Err(RoundFaceRefusal::Number)
            );
            assert_eq!(
                whole_band(&seat(), bad, &[band(rims_and_seam())]),
                Err(RoundFaceRefusal::Number)
            );
            assert!(grow_block(&seat(), bad, body.extent(), 1.0, 0.0, 0.0).is_err());
        }
    }
}
