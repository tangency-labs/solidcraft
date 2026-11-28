//! A bead on one flat panel, folded where it lies: its faces and what each is, its footprint in
//! the flat, its outline and note, its refusals, its clearance test and where a sketch line on the
//! panel's faces lies in the chart, each against the section's closed form.

use crate::math::det;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::super::plane::Planar;
use super::super::seg::{ccw, polygon};
use super::super::{develop, fold, Panel, Rigid2, Rigid3, Sheet, Surface};
use super::{
    bead_against, bead_flank_angle, bead_footprint, bead_form, bead_note, bead_outline,
    line_in_chart, segment_in_region, Bead, BeadAgainst, BeadPart, BeadRefusal, BeadSection, Fold,
};
use crate::eval::vec::VecExt;
use crate::math::{Vec2, Vec3};
use crate::prism::profile::{Loop2, Region2, Seg2};

const T: f64 = 1.0;

/// A 100 × 400 plate `T` thick, its chart the flat and folded where it lies.
fn plate() -> Fold {
    let pts = [
        Vec2::new(0.0, 0.0),
        Vec2::new(100.0, 0.0),
        Vec2::new(100.0, 400.0),
        Vec2::new(0.0, 400.0),
    ];
    let sheet = Sheet {
        thickness: T,
        k_factor: 0.44,
        panels: vec![Panel {
            name: "P".into(),
            surface: Surface::Plane,
            region: Region2 {
                outer: ccw(polygon(&pts)),
                holes: Vec::new(),
            },
        }],
        joints: Vec::new(),
        corners: Vec::new(),
        cuts: Vec::new(),
    };
    let dev = develop(&sheet, &[(0, Rigid2::IDENTITY)]).unwrap();
    fold(&sheet, &dev, &[Rigid3::IDENTITY]).unwrap()
}

/// A bead 2 deep and 10 wide (45°, R 1 at the base and the crown, ramps 6 long) from `a` to `b`.
fn bead(a: Vec2, b: Vec2, far: bool) -> Bead {
    Bead {
        panel: 0,
        a,
        b,
        far,
        section: BeadSection {
            depth: 2.0,
            width: 10.0,
            angle: FRAC_PI_4,
            base_radius: 1.0,
            crown_radius: 1.0,
            end_length: 6.0,
        },
    }
}

/// Along +y at x = 50, from y 40 to 360.
fn along_y(far: bool) -> Bead {
    bead(Vec2::new(50.0, 40.0), Vec2::new(50.0, 360.0), far)
}

/// The footprint is the width at the virtual sharps plus a base bend's setback either side, and
/// runs a base bend's setback along the ramps past each end.
#[test]
fn the_footprint_is_the_width_and_the_centreline_and_the_base_bends_setbacks() {
    let fp = bead_footprint(&along_y(false), T).unwrap();
    let xf = 5.0 + det::tan(FRAC_PI_4 / 2.0);
    let sq = det::atan(2.0f64 / 6.0) / 2.0;
    let sq = det::tan(sq);
    let xs: Vec<f64> = fp.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = fp.iter().map(|p| p.y).collect();
    let (x0, x1) = (
        xs.iter().copied().fold(f64::MAX, f64::min),
        xs.iter().copied().fold(f64::MIN, f64::max),
    );
    let (y0, y1) = (
        ys.iter().copied().fold(f64::MAX, f64::min),
        ys.iter().copied().fold(f64::MIN, f64::max),
    );
    assert!(
        (x1 - x0 - 2.0 * xf).abs() < 1e-12 && ((x0 + x1) / 2.0 - 50.0).abs() < 1e-12,
        "{fp:?}"
    );
    assert!(
        (y0 - (40.0 - sq)).abs() < 1e-12 && (y1 - (360.0 + sq)).abs() < 1e-12,
        "{fp:?}"
    );
}

/// Folded, a bead has 26 faces, each a part of its own: the crown, three pieces on either side
/// and three on either ramp, on each skin. The crown stands its depth out of the face it stands
/// out of, on the side away from the material, the crown of the skin under it a thickness less;
/// each face's left or right, start or end is where its name says. The flat leaves the footprint
/// open there.
#[test]
fn a_folded_bead_has_its_26_faces_where_their_parts_say() {
    let fold = plate();
    for far in [false, true] {
        let b = along_y(far);
        let form = bead_form(&fold, &b, T).unwrap();
        assert_eq!(form.faces.len(), 26, "far {far}");
        for (i, f) in form.faces.iter().enumerate() {
            assert!(
                form.faces.iter().skip(i + 1).all(|g| g.face != f.face),
                "far {far}: {:?} twice",
                f.face
            );
        }
        // The face it stands out of, and the way out of the material from it.
        let face = |depth: f64| {
            fold.surface_point(0, Vec2::new(50.0, 200.0), depth)
                .unwrap()
        };
        let (on, out) = if far {
            (face(T), face(T) - face(0.0))
        } else {
            (face(0.0), face(0.0) - face(T))
        };
        let out = out.normalized().unwrap();
        let at = |part: BeadPart, top: bool| {
            form.faces
                .iter()
                .find(|f| f.face.part == part && f.face.top == top)
                .unwrap()
                .point
        };
        // The raised skin continues the face it stands out of: the surface (top) unless far.
        let (raised, under) = (!far, far);
        assert!(
            ((at(BeadPart::Crown, raised) - on).dot(out) - 2.0).abs() < 1e-9,
            "far {far}"
        );
        assert!(
            ((at(BeadPart::Crown, under) - on).dot(out) - 1.0).abs() < 1e-9,
            "far {far}"
        );
        // Seen from the sheet's surface (the top, its way out `up`), along +y: left is −x here.
        let up = (face(0.0) - face(T)).normalized().unwrap();
        let left = up.cross(Vec3::Y);
        for top in [true, false] {
            for piece in 0..3 {
                let l = at(BeadPart::Across { left: true, piece }, top);
                let r = at(BeadPart::Across { left: false, piece }, top);
                assert!(
                    (l - face(0.0)).dot(left) > 0.0 && (r - face(0.0)).dot(left) < 0.0,
                    "far {far}, piece {piece}"
                );
                let s = at(BeadPart::Along { start: true, piece }, top);
                let e = at(
                    BeadPart::Along {
                        start: false,
                        piece,
                    },
                    top,
                );
                assert!(s.y < 200.0 && e.y > 200.0, "far {far}, piece {piece}");
            }
        }
        // The footprint, open in the flat (the chart, here), clockwise.
        let fp = bead_footprint(&b, T).unwrap();
        let [o0, o1, o2, o3] = form.open;
        assert!(
            [o0, o3, o2, o1]
                .iter()
                .zip(&fp)
                .all(|(o, f)| o.dist(*f) < 1e-9),
            "{:?} vs {fp:?}",
            form.open
        );
    }
}

/// In the flat, the outline is the footprint and the centreline; the crown is the width less the
/// flanks' and the crown bends' run either side, and the centreline less the ramps'; a note reads
/// left to right or upward, beside the centreline on its left. Mirrored, as a flat pattern is
/// read, every point's y turns over, the footprint still runs counter-clockwise, and the note
/// still reads upward.
#[test]
fn the_outline_gives_the_crown_and_a_note_that_reads_upright() {
    let fold = plate();
    let o = bead_outline(&fold, &along_y(false), T, false).unwrap();
    let ro = 1.0 + T;
    let half = |a: f64| det::tan(a / 2.0);
    let crown_width = 10.0 - 2.0 * (2.0 / det::tan(FRAC_PI_4) + ro * half(FRAC_PI_4));
    let beta = det::atan(2.0f64 / 6.0);
    let crown_length = 320.0 - 2.0 * (6.0 + ro * half(beta));
    assert!(
        (o.crown_width - crown_width).abs() < 1e-9 && (o.crown_length - crown_length).abs() < 1e-9,
        "{o:?}"
    );
    assert!(
        o.start.dist(Vec2::new(50.0, 40.0)) < 1e-12 && o.end.dist(Vec2::new(50.0, 360.0)) < 1e-12
    );
    assert!(
        (o.note_angle - FRAC_PI_2).abs() < 1e-12 && o.note_side.dist(Vec2::new(-1.0, 0.0)) < 1e-12,
        "{o:?}"
    );
    // Drawn right to left, the note still reads left to right, above the line.
    let back = bead_outline(
        &fold,
        &bead(Vec2::new(90.0, 200.0), Vec2::new(10.0, 200.0), false),
        T,
        false,
    )
    .unwrap();
    assert!(
        back.note_angle.abs() < 1e-12 && back.note_side.dist(Vec2::new(0.0, 1.0)) < 1e-12,
        "{back:?}"
    );
    // Mirrored: (x, −y), the corners still counter-clockwise (positive area).
    let m = bead_outline(&fold, &along_y(false), T, true).unwrap();
    assert!(
        m.start.dist(Vec2::new(50.0, -40.0)) < 1e-12 && m.end.dist(Vec2::new(50.0, -360.0)) < 1e-12,
        "{m:?}"
    );
    let flipped = |p: Vec2| Vec2::new(p.x, -p.y);
    assert!(
        m.footprint
            .iter()
            .all(|p| o.footprint.iter().any(|q| p.dist(flipped(*q)) < 1e-12)),
        "{m:?}"
    );
    let area: f64 = m
        .footprint
        .iter()
        .zip(m.footprint.iter().cycle().skip(1))
        .map(|(p, q)| p.x * q.y - q.x * p.y)
        .sum();
    assert!(area > 0.0, "{m:?}");
    assert!(
        (m.note_angle - FRAC_PI_2).abs() < 1e-12 && m.note_side.dist(Vec2::new(-1.0, 0.0)) < 1e-12,
        "{m:?}"
    );
}

/// What does not fit is refused, with what would.
#[test]
fn sections_that_do_not_fit_are_refused() {
    let with = |f: &dyn Fn(&mut BeadSection)| {
        let mut b = along_y(false);
        f(&mut b.section);
        bead_footprint(&b, T)
    };
    assert!(
        matches!(with(&|s| s.depth = 0.5), Err(BeadRefusal::ShallowFlanks { min_depth, .. }) if min_depth > 0.5)
    );
    assert!(
        matches!(with(&|s| s.width = 3.0), Err(BeadRefusal::NoCrown { min_width, .. }) if min_width > 3.0)
    );
    assert!(matches!(
        with(&|s| s.angle = 0.0),
        Err(BeadRefusal::Angle { .. })
    ));
    assert!(matches!(
        with(&|s| s.base_radius = 0.0),
        Err(BeadRefusal::Radius)
    ));
    // A bend so gentle its two ends would be one height where the faces are built: radii at the
    // least, flanks at 0.01 rad.
    assert!(matches!(
        with(&|s| {
            s.base_radius = 1e-3;
            s.crown_radius = 1e-3;
            s.angle = 0.01;
        }),
        Err(BeadRefusal::GentleBend { rise }) if rise < 1e-6
    ));
    // A bend so tight its faces would collapse onto its edges.
    assert!(matches!(
        with(&|s| s.crown_radius = 1e-10),
        Err(BeadRefusal::Radius)
    ));
    assert!(matches!(
        with(&|s| s.depth = f64::NAN),
        Err(BeadRefusal::NotFinite)
    ));
    let short = bead(Vec2::new(50.0, 40.0), Vec2::new(50.0, 50.0), false);
    assert!(
        matches!(bead_footprint(&short, T), Err(BeadRefusal::TooShort { length, min_length }) if (length - 10.0).abs() < 1e-12 && min_length > 10.0)
    );
    let none = bead(Vec2::new(50.0, 40.0), Vec2::new(50.0, 40.0), false);
    assert_eq!(bead_footprint(&none, T), Err(BeadRefusal::NoLength));
}

/// A square `s` on a side from (x0, y0), counter-clockwise.
fn square(x0: f64, y0: f64, s: f64) -> Loop2 {
    polygon(&[
        Vec2::new(x0, y0),
        Vec2::new(x0 + s, y0),
        Vec2::new(x0 + s, y0 + s),
        Vec2::new(x0, y0 + s),
    ])
}

/// A whole circle about `c`, `r` in radius.
fn circle(c: Vec2, r: f64) -> Loop2 {
    Loop2 {
        segs: vec![Seg2::Arc {
            center: c,
            radius: r,
            start: 0.0,
            sweep: 2.0 * std::f64::consts::PI,
        }],
    }
}

/// An outline is placed against the footprint exactly: clear of it, across it, inside it or
/// round it, a line or an arc a hair clear (5e-4 mm) still clear, and one within `near` meeting it.
#[test]
fn an_outline_is_placed_against_the_footprint_exactly() {
    let b = along_y(false);
    let xf = 5.0 + det::tan(FRAC_PI_4 / 2.0);
    let at = |l: &Loop2, near: f64| bead_against(&b, T, l, near).unwrap();
    assert_eq!(at(&square(70.0, 100.0, 10.0), 0.0), BeadAgainst::Clear);
    assert_eq!(at(&square(52.0, 100.0, 10.0), 0.0), BeadAgainst::Meets);
    assert_eq!(at(&square(48.0, 100.0, 2.0), 0.0), BeadAgainst::Around);
    assert_eq!(at(&square(30.0, 20.0, 360.0), 0.0), BeadAgainst::Inside);
    // A hair clear of the footprint's side: a straight edge, and a circle's arc.
    let hair = 5e-4;
    assert_eq!(
        at(&square(50.0 + xf + hair, 100.0, 10.0), 0.0),
        BeadAgainst::Clear
    );
    assert_eq!(
        at(&circle(Vec2::new(50.0 + xf + hair + 3.0, 200.0), 3.0), 0.0),
        BeadAgainst::Clear
    );
    assert_eq!(
        at(&circle(Vec2::new(50.0 + xf - hair + 3.0, 200.0), 3.0), 0.0),
        BeadAgainst::Meets
    );
    // 70 − 50 − xf from the footprint's side: within 15 of it, not within 10.
    assert_eq!(at(&square(70.0, 100.0, 10.0), 10.0), BeadAgainst::Clear);
    assert_eq!(at(&square(70.0, 100.0, 10.0), 15.0), BeadAgainst::Meets);
}

/// A segment lies in a region when both its ends do and it crosses none of its edges: across a
/// hole between its ends, it does not.
#[test]
fn a_segment_lies_in_a_region_unless_it_crosses_a_hole() {
    let holed = Region2 {
        outer: square(0.0, 0.0, 100.0),
        holes: vec![circle(Vec2::new(50.0, 50.0), 5.0).reversed()],
    };
    assert_eq!(
        segment_in_region(&holed, Vec2::new(20.0, 20.0), Vec2::new(80.0, 20.0)),
        Some(true)
    );
    assert_eq!(
        segment_in_region(&holed, Vec2::new(20.0, 50.0), Vec2::new(80.0, 50.0)),
        Some(false)
    );
    assert_eq!(
        segment_in_region(&holed, Vec2::new(20.0, 20.0), Vec2::new(120.0, 20.0)),
        Some(false)
    );
    // A hole away from the segment's middle, which a test of its ends and middle alone misses.
    let aside = Region2 {
        outer: square(0.0, 0.0, 100.0),
        holes: vec![circle(Vec2::new(35.0, 50.0), 5.0).reversed()],
    };
    assert_eq!(
        segment_in_region(&aside, Vec2::new(20.0, 50.0), Vec2::new(80.0, 50.0)),
        Some(false)
    );
    assert_eq!(
        segment_in_region(&aside, Vec2::new(20.0, 70.0), Vec2::new(80.0, 70.0)),
        Some(true)
    );
}

/// The flanks' angle that leaves a crown is the one whose run across, (width − crown) / 2, the
/// depth rises over; a crown no narrower than the width is refused.
#[test]
fn the_flank_angle_leaves_the_crown_asked_for() {
    assert!((bead_flank_angle(2.0, 10.0, 6.0).unwrap() - FRAC_PI_4).abs() < 1e-12);
    assert!(matches!(
        bead_flank_angle(2.0, 10.0, 10.0),
        Err(BeadRefusal::CrownWidth { .. })
    ));
    assert!(matches!(
        bead_flank_angle(2.0, 10.0, -1.0),
        Err(BeadRefusal::CrownWidth { .. })
    ));
}

/// A note stands beside the centreline's middle, half the width and a text height off it on the
/// side it reads from, at the centreline's angle.
#[test]
fn a_note_stands_beside_the_middle_of_its_bead() {
    let (at, angle) = bead_note(&plate(), &along_y(false), T, false, 3.0).unwrap();
    assert!(
        at.dist(Vec2::new(50.0 - 5.0 - 3.0, 200.0)) < 1e-12 && (angle - FRAC_PI_2).abs() < 1e-12,
        "{at:?} {angle}"
    );
}

/// A line drawn on the panel's surface or its far face lies in its chart; one on another plane,
/// or a tilted one, does not.
#[test]
fn a_line_on_a_face_of_the_panel_lies_in_its_chart() {
    let place = Rigid3::IDENTITY;
    let (a, b) = (Vec3::new(50.0, 40.0, 0.0), Vec3::new(50.0, 360.0, 0.0));
    let (qa, qb, far) = line_in_chart(&place, T, Vec3::ZERO, Vec3::Z, a, b).unwrap();
    assert!(
        qa.dist(Vec2::new(50.0, 40.0)) < 1e-12 && qb.dist(Vec2::new(50.0, 360.0)) < 1e-12 && !far
    );
    let lift = Vec3::new(0.0, 0.0, T);
    assert!(matches!(
        line_in_chart(&place, T, lift, Vec3::Z, a + lift, b + lift),
        Some((_, _, true))
    ));
    assert!(line_in_chart(&place, T, Vec3::new(0.0, 0.0, 5.0), Vec3::Z, a, b).is_none());
    assert!(line_in_chart(&place, T, Vec3::ZERO, Vec3::X, a, b).is_none());
}
