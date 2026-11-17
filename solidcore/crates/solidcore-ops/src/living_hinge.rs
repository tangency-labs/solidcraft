//! Planning a living hinge (design 15): the slots of a kerf-bending pattern over a sketch area in a
//! flat plate.
//!
//! Recipes of kernel calls (`solidcore::living_hinge`, which holds the lattice, the edge rules and
//! the checks of its inputs): the record's spec and slots are converted by
//! [`solidcore_convert::living_hinge`]. It computes no geometric quantity.

use solidcore::living_hinge as kernel;
use solidcore::living_hinge::NearEdge;
use solidcore::prism::{read, sheet};
use solidcore_convert as convert;
use solidcore_record::living_hinge::{HingeSlot, LivingHingePlan, LivingHingeSpec};
use solidcore_record::{Loop2, Plane, Region2, Vec2};

use crate::boolean::BooleanOp;
use crate::prism::planar_regions_boolean;
use crate::session::{read_body, Answer};
use crate::{Body, Error};

pub use solidcore::living_hinge::{LivingHingeError, MAX_SLOTS, MIN_SIZE};

/// The pattern a new living hinge starts from (the kernel's defaults).
#[must_use]
pub fn default_spec() -> LivingHingeSpec {
    convert::living_hinge::record_spec(&kernel::LivingHingeSpec::default())
}

/// The loops of the area (`field: true`) or the plate in the hinge frame of `angle`, refused as
/// that input when a curve cannot be followed by chords.
fn frame_loops(
    loops: &[Loop2],
    angle: f64,
    field: bool,
) -> Result<Vec<Vec<NearEdge>>, LivingHingeError> {
    loops
        .iter()
        .map(|lp| {
            kernel::frame_loop(&convert::loop2(lp), angle).ok_or_else(|| {
                let why = "a curve of it cannot be followed by chords".to_owned();
                if field {
                    LivingHingeError::Field(why)
                } else {
                    LivingHingeError::Plate(why)
                }
            })
        })
        .collect()
}

/// The slots of a living hinge over the area `field` in the plate's outline `plate`, both closed
/// loops of the sketch plane, with the slots along `angle` (radians, from the sketch's X axis).
/// The lattice is centred on the area, or anchored at the sketch point `anchor`. The plan is in
/// the hinge frame (`u` along the slots, `v` across the rows).
///
/// # Errors
///
/// [`LivingHingeError`]: a parameter, the area or the plate is refused, or the area reaches more
/// than [`MAX_SLOTS`] lattice slots.
pub fn plan_living_hinge(
    field: &[Loop2],
    plate: &[Loop2],
    spec: &LivingHingeSpec,
    angle: f64,
    anchor: Option<Vec2>,
) -> Result<LivingHingePlan, LivingHingeError> {
    let f = frame_loops(field, angle, true)?;
    let p = frame_loops(plate, angle, false)?;
    let anchor = anchor.map(|a| kernel::frame_point((a.x, a.y), angle));
    let plan = kernel::plan(&f, &p, None, &convert::living_hinge::spec(spec), anchor)?;
    Ok(convert::living_hinge::record_plan(&plan, spec.slot_width))
}

/// Whether the area `field` and the plate's outline `plate` overlap (design 3.5): touching along
/// an edge or at a point is not meeting. `false` when either cannot be followed by chords.
#[must_use]
pub fn meets(field: &[Loop2], plate: &[Loop2]) -> bool {
    match (
        frame_loops(field, 0.0, true),
        frame_loops(plate, 0.0, false),
    ) {
        (Ok(f), Ok(p)) => kernel::meets(&f, &p),
        _ => false,
    }
}

/// The region of `slot` in the hinge frame: a stadium `slot_width` wide around its centre line.
#[must_use]
pub fn slot_region(slot: &HingeSlot, slot_width: f64) -> Region2 {
    convert::record_region2(&kernel::slot_region(
        &convert::living_hinge::slot(slot),
        slot_width,
    ))
}

/// One flat face of a plate parallel to the sketch plane (design 6.1).
#[derive(Clone, Debug, PartialEq)]
pub struct PlateFace {
    /// The face's regions in the plane's coordinates.
    pub regions: Vec<Region2>,
    /// Its level along the plane's normal (0 in the plane).
    pub level: f64,
    /// Whether its outward normal is the plane's normal.
    pub up: bool,
    /// Whether its level is within [`PlateOutline::tolerance`] of 0: the face lies in the plane.
    pub in_plane: bool,
}

/// A plate's outline seen along a plane's normal (design 6.1): the union of its flat faces facing
/// along the normal, with every flat face parallel to the plane and the faces not read.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateOutline {
    /// The union of the regions of the faces facing along the normal: empty when there is none.
    pub regions: Vec<Region2>,
    /// The flat faces parallel to the plane, in the body's face order.
    pub faces: Vec<PlateFace>,
    /// Faces along the normal (walls).
    pub walls: usize,
    /// Faces neither parallel nor along the normal (a chamfer, a rounded edge): not read.
    pub other: usize,
    /// The tolerance the faces were read and united to (mm).
    pub tolerance: f64,
}

struct Read(sheet::ParallelFaces, f64);

impl Answer for Read {}

/// The outline of the plate `body` seen along `plane`'s normal (design 6.1): its flat faces
/// parallel to the plane read exactly ([`sheet::parallel_faces`]), and the union of those facing
/// along the normal ([`planar_regions_boolean`], no boolean for one face). A face is `in_plane`
/// when its level is within the read tolerance of 0.
///
/// # Errors
/// [`Error::Empty`] for a body without size; [`Error::Unsupported`] for a face not read exactly;
/// those of [`planar_regions_boolean`] and the session.
pub fn plate_outline(body: &Body, plane: &Plane) -> crate::Result<PlateOutline> {
    let frame = convert::plane(plane);
    let Read(read, tolerance) = read_body("plate_outline", body, |k, b| {
        let Some(tol) = read::planar_edges_tolerance(b) else {
            return Ok(Read(sheet::ParallelFaces::default(), 0.0));
        };
        Ok(Read(sheet::parallel_faces(k, b, &frame, tol)?, tol))
    })?;
    if tolerance <= 0.0 {
        return Err(Error::Empty);
    }
    let faces: Vec<PlateFace> = read
        .faces
        .iter()
        .map(|f| PlateFace {
            regions: f.regions.iter().map(convert::record_region2).collect(),
            level: f.level,
            up: f.up,
            in_plane: f.level.abs() <= tolerance,
        })
        .collect();
    let mut regions: Option<Vec<Region2>> = None;
    for f in faces.iter().filter(|f| f.up) {
        regions = Some(match regions {
            None => f.regions.clone(),
            Some(acc) => planar_regions_boolean(&acc, &f.regions, BooleanOp::Union, tolerance)?,
        });
    }
    Ok(PlateOutline {
        regions: regions.unwrap_or_default(),
        faces,
        walls: read.walls,
        other: read.other,
        tolerance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::living_hinge::{HingeClip, SlotKind};
    use solidcore_record::Seg2;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
        let p = [
            Vec2 { x: x0, y: y0 },
            Vec2 { x: x1, y: y0 },
            Vec2 { x: x1, y: y1 },
            Vec2 { x: x0, y: y1 },
        ];
        Loop2 {
            segs: p
                .iter()
                .zip(p.iter().cycle().skip(1))
                .map(|(&a, &b)| Seg2::Line { a, b })
                .collect(),
        }
    }

    fn counts(p: &LivingHingePlan) -> (usize, usize, usize, usize, usize, usize) {
        let n = |k| p.slots.iter().filter(|s| s.kind == k).count();
        (
            p.rows,
            p.slots.len(),
            n(SlotKind::Whole),
            n(SlotKind::Shortened),
            n(SlotKind::RunOut),
            p.left_out,
        )
    }

    #[test]
    fn the_default_spec_is_the_designs() {
        let s = default_spec();
        assert_eq!(
            (
                s.slot_length,
                s.bridge,
                s.row_spacing,
                s.slot_width,
                s.margin
            ),
            (20.0, 3.0, 1.5, 0.2, 0.0)
        );
        assert_eq!(s.clip, HingeClip::RunOut);
    }

    #[test]
    fn case_i_angle_0_plans_the_designs_slots() {
        // Plate 60 x 100, area (0, 30)-(60, 70), slots along X: 27 rows, 95 slots.
        let p = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &default_spec(),
            0.0,
            None,
        )
        .unwrap();
        assert_eq!(counts(&p), (27, 95, 41, 26, 28, 0));
    }

    #[test]
    fn case_i_reports_its_lengths_area_and_band() {
        // Design 8.1: the band is 40 mm across the slots; per slot `len·w + πr²` (closed) or
        // `len·w + πr²/2` (run-out), outlines `2·len + 2πr` or `2·len + πr`.
        let spec = default_spec();
        let p = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &spec,
            0.0,
            None,
        )
        .unwrap();
        assert!((p.band_width - 40.0).abs() < 1e-9, "{}", p.band_width);
        let r = spec.slot_width / 2.0;
        let pi = std::f64::consts::PI;
        let (mut cut, mut outline, mut area) = (0.0, 0.0, 0.0);
        for s in &p.slots {
            let len = match s.crossing {
                Some(x) if (x - s.a0).abs() < (x - s.a1).abs() => s.a1 - x,
                Some(x) => x - s.a0,
                None => s.a1 - s.a0,
            };
            let caps = if s.kind == SlotKind::RunOut { 0.5 } else { 1.0 };
            cut += len;
            outline += 2.0 * len + 2.0 * pi * r * caps;
            area += len * spec.slot_width + pi * r * r * caps;
        }
        assert!(
            cut > 0.0 && (p.cut_length - cut).abs() < 1e-6,
            "{} {cut}",
            p.cut_length
        );
        assert!((p.outline_length - outline).abs() < 1e-6);
        assert!((p.open_area - area).abs() < 1e-6);
    }

    #[test]
    fn case_a_angle_90_plans_the_same_slots() {
        // Plate 100 x 60, area (30, 0)-(70, 60), slots along Y.
        let p = plan_living_hinge(
            &[rect(30.0, 0.0, 70.0, 60.0)],
            &[rect(0.0, 0.0, 100.0, 60.0)],
            &default_spec(),
            std::f64::consts::FRAC_PI_2,
            None,
        )
        .unwrap();
        assert_eq!(counts(&p), (27, 95, 41, 26, 28, 0));
    }

    #[test]
    fn case_h_anchor_moves_the_lattice() {
        // Case I turned: anchor (0, 0.5) in the sketch is row 0's slot centre.
        let p = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &default_spec(),
            0.0,
            Some(Vec2 { x: 0.0, y: 30.5 }),
        )
        .unwrap();
        assert!(p.slots.iter().any(|s| s.row == 0 && s.v == 30.5));
    }

    #[test]
    fn an_area_meets_a_plate_only_by_overlapping_it() {
        let plate = [rect(0.0, 0.0, 60.0, 100.0)];
        assert!(meets(&[rect(0.0, 30.0, 60.0, 70.0)], &plate));
        assert!(meets(&[rect(-10.0, -10.0, 5.0, 5.0)], &plate));
        assert!(!meets(&[rect(60.0, 30.0, 80.0, 70.0)], &plate));
        assert!(!meets(&[rect(70.0, 30.0, 80.0, 70.0)], &plate));
    }

    #[test]
    fn a_bad_spec_is_refused() {
        let mut s = default_spec();
        s.bridge = f64::NAN;
        let e = plan_living_hinge(
            &[rect(0.0, 30.0, 60.0, 70.0)],
            &[rect(0.0, 0.0, 60.0, 100.0)],
            &s,
            0.0,
            None,
        );
        assert!(matches!(e, Err(LivingHingeError::Parameter(_))));
    }

    /// Twice the signed area of a loop of lines (shoelace), for the plate tests.
    fn line_area2(l: &Loop2) -> f64 {
        l.segs
            .iter()
            .map(|s| match s {
                Seg2::Line { a, b } => a.x * b.y - b.x * a.y,
                _ => panic!("a plate loop of lines only"),
            })
            .sum()
    }

    #[test]
    fn plate_outline_of_a_plain_plate_is_its_top() {
        let plate = crate::make_box([0.0, 0.0, -2.0], [40.0, 20.0, 2.0]).unwrap();
        let o = plate_outline(&plate, &Plane::XY).unwrap();
        assert_eq!(o.faces.len(), 2);
        assert_eq!((o.walls, o.other), (4, 0));
        let top: Vec<_> = o.faces.iter().filter(|f| f.up).collect();
        assert_eq!(top.len(), 1);
        assert!(top[0].in_plane);
        assert!(o.faces.iter().any(|f| !f.up && !f.in_plane));
        assert_eq!(o.regions.len(), 1);
        assert!(o.regions[0].holes.is_empty());
        assert!((line_area2(&o.regions[0].outer) - 1600.0).abs() < 1e-6);
    }

    #[test]
    fn plate_outline_of_a_recessed_plate_fills_the_recess() {
        let plate = crate::make_box([0.0, 0.0, -2.0], [40.0, 20.0, 2.0]).unwrap();
        let pocket = crate::make_box([10.0, 5.0, -1.0], [20.0, 10.0, 2.0]).unwrap();
        let body = crate::boolean::boolean(plate, pocket, BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let o = plate_outline(&body, &Plane::XY).unwrap();
        let up: Vec<_> = o.faces.iter().filter(|f| f.up).collect();
        assert_eq!(up.len(), 2, "the top and the recess floor");
        assert_eq!(up.iter().filter(|f| f.in_plane).count(), 1);
        assert_eq!(o.walls, 8);
        assert_eq!(o.other, 0);
        // the floor fills the top face's hole through their coincident edges
        assert_eq!(o.regions.len(), 1);
        assert!(o.regions[0].holes.is_empty());
        assert!((line_area2(&o.regions[0].outer) - 1600.0).abs() < 1e-6);
    }

    #[test]
    fn plate_outline_of_a_body_seen_edge_on_has_no_up_face() {
        let plate = crate::make_box([0.0, 0.0, -2.0], [40.0, 20.0, 2.0]).unwrap();
        let o = plate_outline(&plate, &Plane::XZ).unwrap();
        assert_eq!(o.faces.iter().filter(|f| f.up).count(), 1);
        assert_eq!(o.regions.len(), 1);
    }
}
