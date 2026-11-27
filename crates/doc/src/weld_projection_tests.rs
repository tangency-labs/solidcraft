//! Projection welds: round, elongated and ring projections, flat and with the dimple on either
//! face, against their closed-form volumes; the kernel's check on every body; several projections
//! in one weld; the refusals; the sizes ISO 8167 proposes; and the site of projections picked on a
//! stack of boxes, with an oval past the face's edge and a projection off the face.

use std::f64::consts::PI;

use solidcore_record::Vec3;
use solidcraft_kernel::{SmlibCheck, validity};

use super::*;

const T_U: f64 = 1.0;
const T_L: f64 = 1.5;

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

/// The record's check, and the kernel's own at its strictest level.
fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-6 * want.abs(), "{what}: volume {got} != {want}");
}

/// The zones' radii for a width `d`: 0, core, band, edge.
fn radii(d: f64) -> [f64; 4] {
    [0.0, 0.55 * d / 2.0, d / 2.0, 1.05 * d / 2.0]
}

/// The dish of width `d` as spot look F draws it: the sphere's cap `0.04 d` off the face at the
/// edge zone's radius `ro`, `0.004 d` on the centre line; its height at `s` across it.
struct Dish {
    zc: f64,
    rs: f64,
}

impl Dish {
    fn of(d: f64) -> Self {
        let (rim, middle, ro) = (0.04 * d, 0.004 * d, radii(d)[3]);
        let c = rim - middle;
        let rs = (ro * ro + c * c) / (2.0 * c);
        Self { zc: middle + rs, rs }
    }

    /// The area under the dish from `a` to `b` across (`0 <= a < b <= ro`).
    fn area(&self, a: f64, b: f64) -> f64 {
        let f = |s: f64| s / 2.0 * (self.rs * self.rs - s * s).sqrt() + self.rs * self.rs / 2.0 * (s / self.rs).asin();
        self.zc * (b - a) - (f(b) - f(a))
    }

    /// The volume under the dish turned a whole turn about its centre, between radii `a` and `b`.
    fn turned(&self, a: f64, b: f64) -> f64 {
        let (rs, zc) = (self.rs, self.zc);
        PI * zc * (b * b - a * a) + 2.0 * PI / 3.0 * ((rs * rs - b * b).powf(1.5) - (rs * rs - a * a).powf(1.5))
    }
}

/// Zone `k`'s volume on one face: `flat` with its marks `0.006 d` high, or under the dish, from
/// the zone's area across the centre line, `strip` long (an oval's straight part, both sides),
/// `ring` (a ring's centre radius, turned a whole turn), and the zone turned about the ends.
fn zone(d: f64, k: usize, flat: bool, strip: f64, ring: f64, ends: bool) -> f64 {
    let r = radii(d);
    let (a, b) = (r[k], r[k + 1]);
    let dish = Dish::of(d);
    let (across, turned) = if flat {
        let h = 0.006 * d;
        (h * (b - a), PI * h * (b * b - a * a))
    } else {
        (dish.area(a, b), dish.turned(a, b))
    };
    2.0 * across * strip + 4.0 * PI * ring * across + if ends { turned } else { 0.0 }
}

fn weld(projections: Vec<Projection>, dimple: Option<SpotSide>) -> ProjectionWeld {
    ProjectionWeld { projections, normal: [0.0, 0.0, 1.0], thicknesses: vec![T_L, T_U], dimple }
}

fn one(shape: ProjectionShape, dimple: Option<SpotSide>) -> Vec<SpotPiece> {
    let mut all = projection_weld(&weld(vec![Projection { at: [0.0; 3], shape }], dimple)).unwrap();
    assert_eq!(all.len(), 1);
    all.remove(0)
}

/// Every zone of one projection on both faces, its pieces added up (an oval's zone is its strips
/// and its two end halves): upper core, band, edge, then the lower; `want` gives a zone's volume
/// from its index and whether that face is flat. Every piece passes the check.
fn zones_are(got: &[SpotPiece], dimple: Option<SpotSide>, want: impl Fn(usize, bool) -> f64, what: &str) {
    for (i, side) in [SpotSide::Upper, SpotSide::Lower].into_iter().enumerate() {
        for (k, zone) in [SpotZone::Core, SpotZone::Band, SpotZone::Edge].into_iter().enumerate() {
            let of: Vec<&SpotPiece> = got.iter().filter(|p| (p.side, p.zone) == (side, zone)).collect();
            assert!(!of.is_empty(), "{what}: no {zone:?} {side:?} ({i})");
            close(of.iter().map(|p| volume(&p.body)).sum(), want(k, dimple != Some(side)), &format!("{what}: {zone:?} {side:?}"));
        }
    }
    assert!(got.iter().all(|p| p.side != SpotSide::Through), "{what}");
    for piece in got {
        check(&piece.body, what);
    }
}

/// The box round all of `pieces`.
fn bounds(pieces: &[SpotPiece]) -> ([f64; 3], [f64; 3]) {
    pieces
        .iter()
        .map(|p| p.body.exact_bounds().unwrap())
        .map(|b| ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]))
        .fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(lo, hi), (a, b)| {
            (std::array::from_fn(|i| lo[i].min(a[i])), std::array::from_fn(|i| hi[i].max(b[i])))
        })
}

/// A round projection is the spot of its diameter in look A; with the dimple, that face's marks
/// are dished as look F, the other face's flat.
#[test]
fn a_round_projection_is_the_spot_with_the_dimple_on_one_face() {
    let d1 = 4.0;
    for dimple in [None, Some(SpotSide::Upper), Some(SpotSide::Lower)] {
        let got = one(ProjectionShape::Round { d1 }, dimple);
        zones_are(&got, dimple, |k, flat| zone(d1, k, flat, 0.0, 0.0, true), &format!("round {dimple:?}"));
    }
}

/// An oval 4 wide and 10 long: each zone the strip along its 6 mm straight part and the half turns
/// round its ends, flat or dished; it reaches 3 + 2.1 either way along its direction.
#[test]
fn an_elongated_projection_is_an_oval_flat_or_dished() {
    let (b1, l1) = (4.0, 10.0);
    let c = (l1 - b1) / 2.0;
    for dimple in [None, Some(SpotSide::Upper), Some(SpotSide::Lower)] {
        let got = one(ProjectionShape::Elongated { b1, l1, along: [1.0, 0.0, 0.0] }, dimple);
        zones_are(&got, dimple, |k, flat| zone(b1, k, flat, 2.0 * c, 0.0, true), &format!("oval {dimple:?}"));
        let edge: Vec<SpotPiece> = got.into_iter().filter(|p| p.zone == SpotZone::Edge && p.side == SpotSide::Upper).collect();
        let (lo, hi) = bounds(&edge);
        let reach = c + radii(b1)[3];
        assert!((hi[0] - reach).abs() < 1e-6 && (lo[0] + reach).abs() < 1e-6, "{lo:?}..{hi:?}");
    }
}

/// A ring from 2 to 4 across (1 wide round a centre circle of radius 1.5): each zone its band
/// across the circle turned a whole turn, flat or dished.
#[test]
fn a_ring_projection_is_a_ring_flat_or_dished() {
    let (d1, di) = (4.0, 2.0);
    let (d, rc) = ((d1 - di) / 2.0, (d1 + di) / 4.0);
    for dimple in [None, Some(SpotSide::Upper), Some(SpotSide::Lower)] {
        let got = one(ProjectionShape::Ring { d1, di }, dimple);
        zones_are(&got, dimple, |k, flat| zone(d, k, flat, 0.0, rc, false), &format!("ring {dimple:?}"));
    }
}

/// Several projections in one weld: one group of bodies each, in order, each where its centre is.
#[test]
fn several_projections_give_one_group_each() {
    let shapes = [
        ProjectionShape::Round { d1: 4.0 },
        ProjectionShape::Elongated { b1: 4.0, l1: 10.0, along: [0.0, 1.0, 0.0] },
        ProjectionShape::Ring { d1: 4.0, di: 2.0 },
    ];
    let centres = [[-20.0, 0.0, 0.0], [0.0, 0.0, 0.0], [20.0, 0.0, 0.0]];
    let projections = shapes.iter().zip(centres).map(|(&shape, at)| Projection { at, shape }).collect();
    let got = projection_weld(&weld(projections, Some(SpotSide::Lower))).unwrap();
    assert_eq!(got.len(), 3);
    for (group, at) in got.iter().zip(centres) {
        assert!(group.len() >= 6);
        let (lo, hi) = bounds(group);
        assert!(lo[0] < at[0] && at[0] < hi[0], "{at:?} in {lo:?}..{hi:?}");
    }
}

/// No projections, a dimple through the sheets, an oval not longer than wide, a ring whose inner
/// diameter is not under its outer or leaves no room inside, no direction, a bad normal or stack
/// are bad input.
#[test]
fn bad_projections_are_refused_as_invalid_input() {
    let bad = |shape: ProjectionShape, dimple: Option<SpotSide>| {
        matches!(projection_weld(&weld(vec![Projection { at: [0.0; 3], shape }], dimple)), Err(ProjectionRefusal::Invalid(_)))
    };
    assert!(matches!(projection_weld(&weld(vec![], None)), Err(ProjectionRefusal::Invalid(_))));
    assert!(bad(ProjectionShape::Ring { d1: 4.0, di: 2.0 }, Some(SpotSide::Through)));
    assert!(bad(ProjectionShape::Elongated { b1: 4.0, l1: 4.0, along: [1.0, 0.0, 0.0] }, None));
    assert!(bad(ProjectionShape::Elongated { b1: 4.0, l1: 10.0, along: [0.0; 3] }, None));
    assert!(bad(ProjectionShape::Elongated { b1: f64::NAN, l1: 10.0, along: [1.0, 0.0, 0.0] }, None));
    assert!(bad(ProjectionShape::Ring { d1: 2.0, di: 2.0 }, None));
    assert!(bad(ProjectionShape::Ring { d1: 4.0, di: 0.05 }, None));
    let round = vec![Projection { at: [0.0; 3], shape: ProjectionShape::Round { d1: 4.0 } }];
    assert!(matches!(projection_weld(&ProjectionWeld { normal: [0.0; 3], ..weld(round.clone(), None) }), Err(ProjectionRefusal::Invalid(_))));
    assert!(matches!(projection_weld(&ProjectionWeld { thicknesses: vec![1.0], ..weld(round, None) }), Err(ProjectionRefusal::Invalid(_))));
}

/// ISO 8167:2021 Annex A group B by the thinner sheet, 6.3 past 3 mm, Table 3's lengths for an
/// elongated projection, Table A.2 and Table 2 for a ring, none for a ring outside its sheets.
#[test]
fn iso_8167_proposes_group_b_by_the_thinner_sheet() {
    let size = |t: [f64; 2], kind| proposed_projection(&t, kind).unwrap();
    for (t, want) in [(0.3, 1.6), (0.5, 2.0), (0.6, 2.5), (1.0, 3.2), (1.2, 4.0), (2.0, 5.0), (3.0, 6.3), (4.0, 6.3)] {
        assert_eq!(size([t, 5.0], ProjectionKind::Spherical).map(|p| p.size), Some(want), "t {t}");
    }
    let oval = size([1.2, 2.0], ProjectionKind::Elongated).unwrap();
    assert_eq!((oval.size, oval.length), (4.0, Some([8.0, 12.8])));
    for (t, want) in [(0.7, Some((3.0, 1.6))), (0.9, Some((4.0, 2.0))), (1.0, Some((4.5, 2.0))), (0.5, None), (1.2, None)] {
        let got = size([t, 2.0], ProjectionKind::Ring).map(|p| (p.size, p.inner.unwrap()));
        assert_eq!(got, want, "t {t}");
    }
    assert!(matches!(proposed_projection(&[1.0], ProjectionKind::Spherical), Err(ProjectionRefusal::Invalid(_))));
}

// ---- where a projection weld sits ----

/// A 40 × 40 sheet from `z` up `t`, centred on the z axis.
fn sheet(z: f64, t: f64) -> Body {
    solidcraft_kernel::box_solid(Vec3::new(-20.0, -20.0, z), Vec3::new(20.0, 20.0, z + t)).unwrap()
}

/// The record face of a box whose plane holds `p`.
fn face_at(b: &Body, p: [f64; 3]) -> usize {
    (0..b.face_count()).find(|&f| b.face_square_level(f, Vec3::new(0.0, 0.0, 1.0), p.into()).unwrap().is_some_and(|l| l.level.abs() < 1e-9)).unwrap()
}

fn site(projections: &[Projection]) -> Result<ProjectionSite> {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    projection_site(&[&lower, &upper], 1, face_at(&upper, [0.0, 0.0, 2.5]), projections)
}

/// A round, an elongated and a ring projection picked on the upper sheet's top: the line runs
/// down, the upper sheet first, every centre moved onto the faying face; the site builds the weld.
#[test]
fn projections_picked_on_top_give_the_frame_the_sheets_and_the_moved_centres() {
    let picked = [
        Projection { at: [-10.0, 0.0, 2.5], shape: ProjectionShape::Round { d1: 4.0 } },
        Projection { at: [10.0, 0.0, 2.5], shape: ProjectionShape::Elongated { b1: 4.0, l1: 10.0, along: [1.0, 0.0, 0.0] } },
        Projection { at: [0.0, 10.0, 2.5], shape: ProjectionShape::Ring { d1: 4.0, di: 2.0 } },
    ];
    let s = site(&picked).unwrap();
    assert!(s.normal.iter().zip([0.0, 0.0, -1.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{s:?}");
    assert_eq!(s.sheets.iter().map(|x| x.sheet).collect::<Vec<_>>(), [1, 0]);
    for (got, was) in s.projections.iter().zip(&picked) {
        assert!((got.at[2] - 1.5).abs() < 1e-9 && (got.at[0] - was.at[0]).abs() < 1e-9, "{got:?}");
        assert_eq!(got.shape, was.shape);
    }
    let weld = ProjectionWeld {
        projections: s.projections,
        normal: s.normal,
        thicknesses: s.sheets.iter().map(|x| x.thickness).collect(),
        dimple: Some(SpotSide::Lower),
    };
    assert_eq!(projection_weld(&weld).unwrap().len(), 3);
}

/// An oval whose far end comes within 1 of the sheet's edge (its width is 4) is refused at that
/// end with the seam's reason; a projection off the face is refused with the spot's.
#[test]
fn an_oval_past_the_edge_or_a_projection_off_the_face_is_refused() {
    let oval = Projection { at: [15.0, 0.0, 2.5], shape: ProjectionShape::Elongated { b1: 4.0, l1: 12.0, along: [1.0, 0.0, 0.0] } };
    let got = site(&[oval]);
    assert!(matches!(got, Err(ProjectionRefusal::AlongProjection { projection: 0, why: SeamRefusal::AtJoint { joint: 1, .. } })), "{got:?}");
    let off = Projection { at: [30.0, 0.0, 2.5], shape: ProjectionShape::Round { d1: 4.0 } };
    assert_eq!(site(&[off]), Err(ProjectionRefusal::AtProjection { projection: 0, why: SpotRefusal::PointOffFace }));
}
