// SPDX-License-Identifier: Apache-2.0
//! Sketch-arrangement and profile-containment computational geometry.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_i64, truncate_f64_to_usize};
use cadmpeg_core::decode::index_from_u32;

use crate::design::profile_select::historical_face_points;
use crate::loss::F3dLossCode;
use crate::records::{
    sketch_relations::SketchRelationOperand,
    topology::extrude_selection::DesignExtrudeSelectionMember,
};
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::evaluator::PcurveEvaluatorLanes;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

const EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12: f64 = 1.0e-12;
const EPS_GEOMETRY_TANGENT_NESTED_LINE_PROFILE_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_ARC_ANGULAR_TOLERANCE_E9: f64 = 1.0e-9;
const EPS_GEOMETRY_TANGENT_RAY_ANGLE_E9: f64 = 1.0e-9;

macro_rules! geometric {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

/// Radians between the departure tangents of two arrangement edges at one
/// node that leave in the same direction.
const ARRANGEMENT_TANGENT_TOLERANCE: f64 = 1.0e-9;
/// Inverse millimetres between the signed curvatures of two arrangement
/// edges that leave a node with the same tangent and bend alike.
const ARRANGEMENT_CURVATURE_TOLERANCE: f64 = 1.0e-12;

/// Local work cap for arrangement edge retention walks, shared by every walk
/// of one decode. An exhausted cap skips the arrangement.
pub(crate) const MAX_ARRANGEMENT_WALK_WORK: usize = 1_000_000;

/// Local cap on the certified tubes of one NURBS span. A span that needs
/// more skips the certification that uses it.
const MAX_SUBDIVISIONS: f64 = 100_000.0;

/// Local work caps of the sketch-profile geometry that binds extrude profile
/// selections, and the selections that skipped a search at each cap.
pub(crate) struct ProfileGeometryWork {
    arrangement: WorkBudget,
    arrangement_skipped: Cell<bool>,
    densification_skipped: Cell<bool>,
    arrangement_skipped_selections: Cell<usize>,
    densification_skipped_selections: Cell<usize>,
    crossing_loop_skipped: Cell<bool>,
    crossing_loop_skipped_selections: Cell<usize>,
    crossing_loop_assumed: Cell<bool>,
    crossing_loop_assumed_selections: Cell<usize>,
    loop_flags_assumed: Cell<bool>,
    loop_flags_assumed_selections: Cell<usize>,
    coincident_direction_assumed: Cell<bool>,
    coincident_direction_assumed_selections: Cell<usize>,
    cut_regions_assumed: Cell<bool>,
    cut_regions_assumed_selections: Cell<usize>,
    cut_regions_beyond_profile: Cell<bool>,
    cut_regions_beyond_profile_selections: Cell<usize>,
    island_absence_assumed: Cell<bool>,
    island_absence_assumed_selections: Cell<usize>,
    polyline_side_assumed: Cell<bool>,
    polyline_side_assumed_selections: Cell<usize>,
    cap_outline_assumed: Cell<bool>,
    cap_outline_assumed_selections: Cell<usize>,
    cap_carrier_assumed: Cell<bool>,
    cap_carrier_assumed_selections: Cell<usize>,
    hole_boundary_inferred_selections: Cell<usize>,
    region_hole_inferred_selections: Cell<usize>,
}

impl ProfileGeometryWork {
    pub(crate) fn new() -> Self {
        Self::with_arrangement_work(MAX_ARRANGEMENT_WALK_WORK)
    }

    pub(crate) fn with_arrangement_work(limit: usize) -> Self {
        Self {
            arrangement: WorkBudget::new(limit),
            arrangement_skipped: Cell::new(false),
            densification_skipped: Cell::new(false),
            arrangement_skipped_selections: Cell::new(0),
            densification_skipped_selections: Cell::new(0),
            crossing_loop_skipped: Cell::new(false),
            crossing_loop_skipped_selections: Cell::new(0),
            crossing_loop_assumed: Cell::new(false),
            crossing_loop_assumed_selections: Cell::new(0),
            loop_flags_assumed: Cell::new(false),
            loop_flags_assumed_selections: Cell::new(0),
            coincident_direction_assumed: Cell::new(false),
            coincident_direction_assumed_selections: Cell::new(0),
            cut_regions_assumed: Cell::new(false),
            cut_regions_assumed_selections: Cell::new(0),
            cut_regions_beyond_profile: Cell::new(false),
            cut_regions_beyond_profile_selections: Cell::new(0),
            island_absence_assumed: Cell::new(false),
            island_absence_assumed_selections: Cell::new(0),
            polyline_side_assumed: Cell::new(false),
            polyline_side_assumed_selections: Cell::new(0),
            cap_outline_assumed: Cell::new(false),
            cap_outline_assumed_selections: Cell::new(0),
            cap_carrier_assumed: Cell::new(false),
            cap_carrier_assumed_selections: Cell::new(0),
            hole_boundary_inferred_selections: Cell::new(0),
            region_hole_inferred_selections: Cell::new(0),
        }
    }

    pub(crate) fn record_hole_boundary_profile_inferred(&self) {
        self.hole_boundary_inferred_selections
            .set(self.hole_boundary_inferred_selections.get() + 1);
    }

    pub(crate) fn record_region_hole_profile_inferred(&self) {
        self.region_hole_inferred_selections
            .set(self.region_hole_inferred_selections.get() + 1);
    }

    pub(crate) fn inferred_counts(&self) -> (usize, usize) {
        (
            self.hole_boundary_inferred_selections.get(),
            self.region_hole_inferred_selections.get(),
        )
    }

    pub(crate) fn set_inferred_counts(&self, counts: (usize, usize)) {
        self.hole_boundary_inferred_selections.set(counts.0);
        self.region_hole_inferred_selections.set(counts.1);
    }

    pub(crate) fn assumed_flags(&self) -> [bool; 9] {
        [
            self.crossing_loop_assumed.get(),
            self.loop_flags_assumed.get(),
            self.coincident_direction_assumed.get(),
            self.cut_regions_assumed.get(),
            self.cut_regions_beyond_profile.get(),
            self.island_absence_assumed.get(),
            self.polyline_side_assumed.get(),
            self.cap_outline_assumed.get(),
            self.cap_carrier_assumed.get(),
        ]
    }

    pub(crate) fn set_assumed_flags(&self, flags: [bool; 9]) {
        self.crossing_loop_assumed.set(flags[0]);
        self.loop_flags_assumed.set(flags[1]);
        self.coincident_direction_assumed.set(flags[2]);
        self.cut_regions_assumed.set(flags[3]);
        self.cut_regions_beyond_profile.set(flags[4]);
        self.island_absence_assumed.set(flags[5]);
        self.polyline_side_assumed.set(flags[6]);
        self.cap_outline_assumed.set(flags[7]);
        self.cap_carrier_assumed.set(flags[8]);
    }

    /// Close one profile selection. Count it against each cap that skipped a
    /// search since the previous selection closed.
    pub(crate) fn finish_selection(&self) {
        for (skipped, selections) in [
            (
                &self.arrangement_skipped,
                &self.arrangement_skipped_selections,
            ),
            (
                &self.densification_skipped,
                &self.densification_skipped_selections,
            ),
            (
                &self.crossing_loop_skipped,
                &self.crossing_loop_skipped_selections,
            ),
            (
                &self.crossing_loop_assumed,
                &self.crossing_loop_assumed_selections,
            ),
            (
                &self.coincident_direction_assumed,
                &self.coincident_direction_assumed_selections,
            ),
            (
                &self.loop_flags_assumed,
                &self.loop_flags_assumed_selections,
            ),
            (
                &self.cut_regions_assumed,
                &self.cut_regions_assumed_selections,
            ),
            (
                &self.cut_regions_beyond_profile,
                &self.cut_regions_beyond_profile_selections,
            ),
            (
                &self.island_absence_assumed,
                &self.island_absence_assumed_selections,
            ),
            (
                &self.polyline_side_assumed,
                &self.polyline_side_assumed_selections,
            ),
            (
                &self.cap_outline_assumed,
                &self.cap_outline_assumed_selections,
            ),
            (
                &self.cap_carrier_assumed,
                &self.cap_carrier_assumed_selections,
            ),
        ] {
            if skipped.take() {
                selections.set(selections.get() + 1);
            }
        }
    }

    /// Record that the open selection took regions from a self-crossing
    /// stored loop split by curve-use counts.
    pub(crate) fn note_crossing_loop_assumed(&self) {
        self.crossing_loop_assumed.set(true);
    }

    /// Record that the member flags of an edge of coincident curves decided
    /// a counted member ring of the open selection.
    /// Record that the open selection joined stored loops to faces through
    /// the assumed reading of each entry's first operand.
    pub(crate) fn note_loop_flags_assumed(&self) {
        self.loop_flags_assumed.set(true);
    }

    pub(crate) fn note_coincident_direction_assumed(&self) {
        self.coincident_direction_assumed.set(true);
    }

    /// Record that the open selection took regions from the faces a cut
    /// changed.
    pub(crate) fn note_cut_regions_assumed(&self) {
        self.cut_regions_assumed.set(true);
    }

    /// Record that the open selection took the regions a cut removed from the
    /// planes it changed in place of the stored profile reading, with region
    /// boundaries that include curves outside the stored profile.
    pub(crate) fn note_cut_regions_beyond_profile(&self) {
        self.cut_regions_beyond_profile.set(true);
    }

    /// Record that the open selection took a face that a curve outside the
    /// arrangement can lie inside, as a face without that curve's island.
    pub(crate) fn note_island_absence_assumed(&self) {
        self.island_absence_assumed.set(true);
    }

    /// Record that the polyline side of a projected point decided the face
    /// of the open selection.
    pub(crate) fn note_polyline_side_assumed(&self) {
        self.polyline_side_assumed.set(true);
    }

    /// Drop the island assumptions of the open selection, whose profile took
    /// no sketch regions.
    pub(crate) fn discard_island_assumptions(&self) {
        self.island_absence_assumed.set(false);
        self.polyline_side_assumed.set(false);
    }

    /// Record that the open selection took regions from an end cap whose
    /// outline was compared partly by samples.
    pub(crate) fn note_cap_outline_assumed(&self) {
        self.cap_outline_assumed.set(true);
    }

    /// Record that the open selection took regions from an end cap with an
    /// edge read from its samples.
    pub(crate) fn note_cap_carrier_assumed(&self) {
        self.cap_carrier_assumed.set(true);
    }

    /// One loss for each cap that made a closed selection skip a search.
    pub(crate) fn losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        let arrangement = self.arrangement_skipped_selections.get();
        if arrangement != 0 {
            losses.push(F3dLossCode::FeatureProfileArrangementCapped.note(format!(
                "{arrangement} extrude profile selection(s) skipped the sketch arrangement search because the arrangement walks exceeded MAX_ARRANGEMENT_WALK_WORK ({MAX_ARRANGEMENT_WALK_WORK})."
            )));
        }
        let densification = self.densification_skipped_selections.get();
        if densification != 0 {
            losses.push(F3dLossCode::FeatureProfileDensificationCapped.note(format!(
                "{densification} extrude profile selection(s) skipped a certified profile test because a curve span needed more than MAX_SUBDIVISIONS ({MAX_SUBDIVISIONS}) tubes."
            )));
        }
        let crossing_skipped = self.crossing_loop_skipped_selections.get();
        if crossing_skipped != 0 {
            losses.push(F3dLossCode::FeatureProfileCrossingLoopCapped.note(format!(
                "{crossing_skipped} extrude profile selection(s) skipped the self-crossing loop split because more than MAX_CROSSING_LOOP_CANDIDATES ({MAX_CROSSING_LOOP_CANDIDATES}) atomic regions lie in one stored loop."
            )));
        }
        let crossing_assumed = self.crossing_loop_assumed_selections.get();
        if crossing_assumed != 0 {
            losses.push(F3dLossCode::FeatureProfileCrossingLoopAssumed.note(format!(
                "{crossing_assumed} extrude profile selection(s) split a self-crossing stored loop into atomic regions by curve-use counts; the stored curve order of the loop was not checked, and atomic faces that enclose an island were assumed not to be selected."
            )));
        }
        let flags = self.loop_flags_assumed_selections.get();
        if flags != 0 {
            losses.push(F3dLossCode::FeatureProfileLoopFlagsAssumed.note(format!(
                "{flags} extrude profile selection(s) joined stored loops to atomic faces by reading each entry's first operand as a traversal flag; that reading is assumed (DR-124), and the listed entry order was not checked."
            )));
        }
        let coincident = self.coincident_direction_assumed_selections.get();
        if coincident != 0 {
            losses.push(F3dLossCode::FeatureProfileCoincidentDirectionAssumed.note(format!(
                "{coincident} extrude profile selection(s) depended on the member flags of an edge of coincident curves: a selected ring has such an edge, or a ring with such an edge has the curves of a member list. The reading accepts such an edge when one distinct member that names one of its curves has that curve's traversal direction, and it checks no flag of the edge's other members; which curve's direction sets the flag is open (DR-124)."
            )));
        }
        let cut_assumed = self.cut_regions_assumed_selections.get();
        if cut_assumed != 0 {
            losses.push(F3dLossCode::FeatureProfileCutRegionsAssumed.note(format!(
                "{cut_assumed} extrude profile selection(s) took atomic regions bounded by the stored profile's curves from the faces the cut changed on planes parallel to the Sketch: a region is selected when one interior point lay on such a face before the cut and on none after it. The region boundaries were not compared with the face boundaries."
            )));
        }
        let beyond = self.cut_regions_beyond_profile_selections.get();
        if beyond != 0 {
            losses.push(F3dLossCode::FeatureProfileCutRegionsAssumed.note(format!(
                "{beyond} extrude profile selection(s) took the atomic regions a cut removed from the planes it changed, in place of a stored profile reading that covers other regions: a region is removed when one interior point lay on such a face before the cut and on none after it. The extrude is a Cut, and at least one taken region is bounded by curves outside the stored profile. A region over no face of those planes before or after the cut is left unselected, and the region boundaries were not compared with the face boundaries."
            )));
        }
        let island_absence = self.island_absence_assumed_selections.get();
        if island_absence != 0 {
            losses.push(F3dLossCode::FeatureProfileIslandAbsenceAssumed.note(format!(
                "{island_absence} extrude profile selection(s) took a face that a curve outside the arrangement can lie in: a full ellipse or a periodic NURBS whose box meets or holds a piece of the face boundary or lies in a face with islands, or another curve without a box. The region was taken without an island of that curve (DR-124)."
            )));
        }
        let polyline_side = self.polyline_side_assumed_selections.get();
        if polyline_side != 0 {
            losses.push(F3dLossCode::FeatureProfilePolylineSideAssumed.note(format!(
                "{polyline_side} extrude profile selection(s) took the side of a face polyline for a projected point within the linear tolerance of a certified NURBS tube of the face boundary. The boundary curve can lie on the other side of the point (DR-124)."
            )));
        }
        let cap_outline = self.cap_outline_assumed_selections.get();
        if cap_outline != 0 {
            losses.push(F3dLossCode::FeatureProfileCapOutlineAssumed.note(format!(
                "{cap_outline} extrude profile selection(s) were read from an end cap whose outline has a sketch curve that is not a line or circular arc. The boundary comparison is incomplete: only that curve's ends were checked on the cap boundary, and the cap edges it should cover were checked against its samples at their end and interior points."
            )));
        }
        let cap_carrier = self.cap_carrier_assumed_selections.get();
        if cap_carrier != 0 {
            losses.push(F3dLossCode::FeatureProfileCapCarrierAssumed.note(format!(
                "{cap_carrier} extrude profile selection(s) were read from an end cap with an edge whose carrier is not proven. The reading assumes, from the edge's end points and interior sample, that the edge is the straight line between its ends, or the circular arc through its ends and the sample."
            )));
        }
        let hole_inferred = self.hole_boundary_inferred_selections.get();
        if hole_inferred != 0 {
            losses.push(F3dLossCode::FeatureProfileHoleBoundaryInferred.note(format!(
                "{hole_inferred} extrude profile selection(s) assume that the supported hole boundary of an inserted region identifies the selected profile."
            )));
        }
        let region_hole_inferred = self.region_hole_inferred_selections.get();
        if region_hole_inferred != 0 {
            losses.push(F3dLossCode::FeatureProfileRegionHoleInferred.note(format!(
                "{region_hole_inferred} extrude profile selection(s) assume that an inserted region's holes belong to the selected profile."
            )));
        }
        losses
    }
}

#[derive(Clone)]
struct SketchArrangementEdge {
    nodes: [usize; 2],
    boundary: cadmpeg_ir::features::SketchProfileBoundaryUse,
    polyline: Vec<Point2>,
    /// Curves of coincident edges merged into this edge, each with its
    /// direction along the edge from its first node to its second.
    coincident: Vec<cadmpeg_ir::sketches::SketchEntityUse>,
}

/// One stored fragment of a ring: consecutive edges of one curve that meet
/// at a synthetic circle split node are one fragment.
#[derive(Clone)]
struct ArrangementFragment {
    /// The fragment's curve and traversal direction.
    curve: cadmpeg_ir::sketches::SketchEntityUse,
    /// Curves of coincident edges merged into the fragment's edges, each
    /// with its traversal direction.
    coincident: Vec<cadmpeg_ir::sketches::SketchEntityUse>,
}

impl ArrangementFragment {
    /// Whether `member` names one of the fragment's curves with that curve's
    /// traversal direction.
    fn names(&self, member: &cadmpeg_ir::sketches::SketchEntityUse) -> bool {
        std::iter::once(&self.curve)
            .chain(&self.coincident)
            .any(|curve| curve == member)
    }
}

/// One closed boundary walk of an arrangement.
#[derive(Clone)]
struct ArrangementRing {
    boundary: Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>,
    /// Boundary fragments in walk order.
    fragments: Vec<ArrangementFragment>,
}

impl ArrangementRing {
    /// The fragment and coincident curves of the ring.
    fn curves(&self) -> impl Iterator<Item = &cadmpeg_ir::sketches::SketchEntityId> {
        self.fragments.iter().flat_map(|fragment| {
            std::iter::once(&fragment.curve.entity)
                .chain(fragment.coincident.iter().map(|use_| &use_.entity))
        })
    }

    /// Whether `members` name the ring's fragment and coincident curves with
    /// their multiplicities.
    fn has_curves(&self, members: &[cadmpeg_ir::sketches::SketchEntityUse]) -> bool {
        let mut curves = self.curves().collect::<Vec<_>>();
        if curves.len() != members.len() {
            return false;
        }
        curves.sort_unstable();
        let mut named = members.iter().map(|use_| &use_.entity).collect::<Vec<_>>();
        named.sort_unstable();
        curves == named
    }

    /// Whether `members` have the ring's curves, and each fragment has a
    /// distinct member that names one of its curves with that curve's
    /// traversal direction.
    fn matches_members(&self, members: &[cadmpeg_ir::sketches::SketchEntityUse]) -> bool {
        if !self.has_curves(members) {
            return false;
        }
        let mut free = members.iter().collect::<Vec<_>>();
        let mut shared = Vec::new();
        for fragment in &self.fragments {
            if !fragment.coincident.is_empty() {
                shared.push(fragment);
                continue;
            }
            let Some(index) = free.iter().position(|member| **member == fragment.curve) else {
                return false;
            };
            free.swap_remove(index);
        }
        let mut owners = vec![None; free.len()];
        (0..shared.len()).all(|fragment| {
            let mut seen = vec![false; free.len()];
            assign_fragment(fragment, &shared, &free, &mut owners, &mut seen)
        })
    }

    /// The number of fragments with a coincident curve.
    fn shared_fragments(&self) -> usize {
        self.fragments
            .iter()
            .filter(|fragment| !fragment.coincident.is_empty())
            .count()
    }
}

/// Whether `shared[fragment]` takes a member of `free` that no other fragment
/// of `shared` holds, moving earlier assignments in `owners` along an
/// augmenting path.
fn assign_fragment(
    fragment: usize,
    shared: &[&ArrangementFragment],
    free: &[&cadmpeg_ir::sketches::SketchEntityUse],
    owners: &mut [Option<usize>],
    seen: &mut [bool],
) -> bool {
    for (index, member) in free.iter().enumerate() {
        if seen[index] || !shared[fragment].names(member) {
            continue;
        }
        seen[index] = true;
        let owner = owners[index];
        if owner.is_none_or(|owner| assign_fragment(owner, shared, free, owners, seen)) {
            owners[index] = Some(fragment);
            return true;
        }
    }
    false
}

impl SketchArrangementFace {
    /// Whether the face holds `point`, decided by the exact lines and arcs
    /// and the certified NURBS tubes of its boundary. `None` when a piece
    /// lies within `tolerance` of the point.
    fn holds(&self, point: Point2, tolerance: f64) -> Option<bool> {
        self.pieces
            .iter()
            .all(|piece| piece.clears_point_by(point, tolerance))
            .then(|| point_in_segment_loop(point, self.loop_segments()))
    }

    /// Whether the face holds the component of `members`, decided by the
    /// first member node that lies farther than `tolerance` from every
    /// boundary piece. `None` when no member node does.
    fn holds_component(&self, members: &[Point2], tolerance: f64) -> Option<bool> {
        members.iter().find_map(|node| self.holds(*node, tolerance))
    }

    /// The segments of the boundary pieces, each followed by a line to the
    /// start of the next piece, so that the loop closes where adjacent
    /// pieces evaluate a shared node differently.
    fn loop_segments(&self) -> impl Iterator<Item = ProfileBoundarySegment> + '_ {
        self.pieces
            .iter()
            .zip(self.pieces.iter().cycle().skip(1))
            .flat_map(|(piece, next)| {
                [
                    piece.segment(),
                    ProfileBoundarySegment::Line {
                        start: piece.end(),
                        end: next.start(),
                    },
                ]
            })
    }
}

/// How the non-construction curves outside the arrangement relate to a face.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ExcludedIsland {
    /// Each such curve lies outside the face.
    Absent,
    /// Such a curve can lie inside the face.
    Possible,
    /// Such a curve lies inside the face, which holds no island.
    Present,
}

struct SketchArrangementFace {
    ring: ArrangementRing,
    polyline: Vec<Point2>,
    /// The outer walk of each arrangement component that lies inside the
    /// face and inside no smaller face. `None` when such a component has no
    /// unique outer walk, when the face belongs to a component that has no
    /// unique outer walk and lies inside another face, when no node of
    /// another component clears the face's boundary pieces, or when no face
    /// around a component inside the face is the innermost.
    holes: Option<Vec<ArrangementRing>>,
    excluded_island: ExcludedIsland,
    /// The certified pieces of the boundary curves in walk order.
    pieces: Vec<CertifiedPiece>,
    /// The lowest and highest corners of a box that holds the face's
    /// boundary curves: exact for lines and circular arcs, and the certified
    /// tube bounds for other curves.
    bounds: [Point2; 2],
}

pub(super) fn arrangement_region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point2],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ArrangementRegion>, CodecError> {
    let Some(faces) = sketch_arrangement_faces(sketch, entities, tolerance, work, ctx)? else {
        return Ok(None);
    };
    let mut boundary = None;
    let mut boundary_count = 0;
    for face in &faces {
        let mut matches = true;
        for point in points {
            let mut on_boundary = false;
            for use_ in &face.ring.boundary {
                if point_on_profile_boundary_use(*point, use_, entities, tolerance) {
                    on_boundary = true;
                    break;
                }
            }
            if !on_boundary {
                matches = false;
                break;
            }
        }
        if matches {
            boundary_count += 1;
            boundary = Some(face);
            if boundary_count == 2 {
                break;
            }
        }
    }
    if boundary_count == 1 {
        return Ok(arrangement_face_region(geometric!(boundary)));
    }
    let mut interior = None;
    let mut interior_count = 0;
    let mut assumed = false;
    for face in &faces {
        let mut excluded = false;
        let mut assumed_out = false;
        let mut assumed_in = false;
        for point in points {
            let mut on_boundary = false;
            for use_ in &face.ring.boundary {
                if point_on_profile_boundary_use(*point, use_, entities, tolerance) {
                    on_boundary = true;
                    break;
                }
            }
            if on_boundary {
                excluded = true;
                break;
            }
            match face.holds(*point, tolerance) {
                Some(false) => {
                    excluded = true;
                    break;
                }
                Some(true) => {}
                None if point_in_polygon(*point, &face.polyline) => assumed_in = true,
                None => assumed_out = true,
            }
        }
        if excluded {
            continue;
        }
        if assumed_out {
            assumed = true;
            continue;
        }
        assumed |= assumed_in;
        interior_count += 1;
        interior = Some(face);
        if interior_count == 2 {
            break;
        }
    }
    if interior_count != 1 {
        return Ok(None);
    }
    Ok(
        arrangement_face_region(geometric!(interior)).map(|mut found| {
            found.polyline_side_assumed |= assumed;
            found
        }),
    )
}

pub(super) fn arrangement_region_matching_curve_program(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    program: &[cadmpeg_ir::sketches::SketchEntityId],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ArrangementRegion>, CodecError> {
    if program.is_empty() {
        return Ok(None);
    }
    let faces = geometric!(curve_arrangement_faces(
        sketch, entities, tolerance, work, ctx, false
    )?);
    let mut selected = None;
    for face in &faces {
        let mut boundary = Vec::new();
        for use_ in &face.ring.boundary {
            if boundary.last().copied() != Some(&use_.entity) {
                boundary.push(&use_.entity);
            }
        }
        if boundary.len() > 1 && boundary.first() == boundary.last() {
            boundary.pop();
        }
        if boundary.len() != program.len() {
            continue;
        }
        let count = program.len();
        let charged = count
            .checked_mul(count)
            .and_then(|count| count.checked_mul(2))
            .is_some_and(|count| work.arrangement.charge_by(count));
        if !charged {
            work.arrangement_skipped.set(true);
            return Ok(None);
        }
        let matches = (0..count).any(|start| {
            (0..count).all(|index| *boundary[(start + index) % count] == program[index])
                || (0..count)
                    .all(|index| *boundary[(start + count - index) % count] == program[index])
        });
        if !matches {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some(face);
    }
    Ok(selected.and_then(arrangement_face_region))
}

/// The regions that counted member rings select in the arrangement of the
/// non-construction lines, circles, circular arcs, bounded ellipses, and
/// nonperiodic NURBS of `sketch`. Each ring names its curves and whether the
/// boundary traverses each against the curve's direction. A ring that
/// matches the outer ring of a face selects that face, and a ring that
/// matches the outer walk of an island of a selected face is a hole ring of
/// that face's region. `None` when the rings are empty, the arrangement is
/// undefined, a ring matches no walk or several walks, two rings match one
/// face or one hole ring, a hole ring belongs to a face that no ring
/// selects, a selected face has an island that no ring names or `None`
/// holes, a curve outside the arrangement lies inside a selected face, a
/// region does not form, or the work budget runs out, which also marks
/// `arrangement_skipped`.
pub(super) fn arrangement_regions_matching_member_rings(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    rings: &[Vec<cadmpeg_ir::sketches::SketchEntityUse>],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<MemberRingRegions>, CodecError> {
    if rings.is_empty() {
        return Ok(None);
    }
    let faces = geometric!(curve_arrangement_faces(
        sketch, entities, tolerance, work, ctx, true
    )?);
    let mut candidates = Vec::new();
    for (index, face) in faces.iter().enumerate() {
        candidates.push((index, None, &face.ring, face.ring.curves().count()));
        for (hole, ring) in face.holes.iter().flatten().enumerate() {
            candidates.push((index, Some(hole), ring, ring.curves().count()));
        }
    }
    let mut outers = Vec::new();
    let mut holes = Vec::new();
    let mut coincident = false;
    for members in rings {
        let mut found = None;
        for (index, hole, ring, curves) in &candidates {
            if *curves != members.len() {
                continue;
            }
            let charged = members
                .len()
                .checked_mul(members.len())
                .and_then(|count| count.checked_mul(ring.shared_fragments().checked_add(2)?))
                .is_some_and(|count| work.arrangement.charge_by(count));
            if !charged {
                work.arrangement_skipped.set(true);
                return Ok(None);
            }
            coincident |= ring.shared_fragments() != 0 && ring.has_curves(members);
            if !ring.matches_members(members) {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            found = Some((*index, *hole));
        }
        let (face, hole) = geometric!(found);
        match hole {
            None if !outers.contains(&face) => outers.push(face),
            Some(hole) if !holes.contains(&(face, hole)) => holes.push((face, hole)),
            _ => return Ok(None),
        }
    }
    if holes.iter().any(|(face, _)| !outers.contains(face)) {
        return Ok(None);
    }
    let mut regions = Vec::new();
    let mut island_absence = false;
    for face_index in outers {
        let face = &faces[face_index];
        match face.excluded_island {
            ExcludedIsland::Present => return Ok(None),
            ExcludedIsland::Possible => island_absence = true,
            ExcludedIsland::Absent => {}
        }
        let islands = geometric!(face.holes.as_ref());
        let named = holes
            .iter()
            .filter(|(face, _)| *face == face_index)
            .map(|(_, hole)| islands[*hole].boundary.clone())
            .collect::<Vec<_>>();
        if named.len() != islands.len() {
            return Ok(None);
        }
        regions.push(geometric!(
            cadmpeg_ir::features::SketchProfileRegion::trimmed(face.ring.boundary.clone(), named)
                .ok()
        ));
    }
    Ok(Some(MemberRingRegions {
        regions,
        coincident,
        island_absence,
    }))
}

/// The regions that counted member rings select.
pub(super) struct MemberRingRegions {
    pub(super) regions: Vec<cadmpeg_ir::features::SketchProfileRegion>,
    /// Whether a ring with an edge of coincident curves has the curves of a
    /// member list, so that the member flags of that edge decided the
    /// selection.
    pub(super) coincident: bool,
    /// Whether a curve outside the arrangement can lie inside a selected
    /// face, so that its region assumes that the curve makes no island in it.
    pub(super) island_absence: bool,
}

/// Whether the arrangement of every curve holds `entity`: a line, circle,
/// circular arc, bounded ellipse, or nonperiodic NURBS, or a point or text,
/// which bound no face.
fn arrangement_family(entity: &cadmpeg_ir::sketches::SketchEntity) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Point { .. }
            | SketchGeometryDefinition::Text { .. }
            | SketchGeometryDefinition::Circle { .. }
    ) || sketch_geometry_parameter_range(&entity.geometry).is_some()
}

/// The lowest and highest corners of a box that holds a curve outside the
/// arrangement families: the exact box of a full ellipse, or the box of the
/// control points of a periodic NURBS without a negative weight. `None` for
/// every other curve.
fn outside_curve_bounds(entity: &cadmpeg_ir::sketches::SketchEntity) -> Option<[Point2; 2]> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match entity.geometry.definition() {
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds: None,
        } => {
            let center = center.get();
            let (sine, cosine) = major_angle.get().sin_cos();
            let (major, minor) = (radii.major().get(), radii.minor().get());
            let reach_u = (major * cosine).hypot(minor * sine);
            let reach_v = (major * sine).hypot(minor * cosine);
            Some([
                Point2::new(center.u - reach_u, center.v - reach_v),
                Point2::new(center.u + reach_u, center.v + reach_v),
            ])
        }
        SketchGeometryDefinition::Nurbs { curve }
            if curve
                .weights()
                .is_none_or(|weights| weights.iter().all(|weight| weight.get() > 0.0)) =>
        {
            polyline_bounds(
                &curve
                    .control_points()
                    .into_iter()
                    .map(cadmpeg_ir::units::FinitePoint2::get)
                    .collect::<Vec<_>>(),
            )
        }
        _ => None,
    }
}

/// Whether the face holds a curve outside the arrangement with the box
/// `bounds`. The box, widened by `tolerance`, lies wholly inside or outside
/// the face when it meets and holds no boundary piece. `None` when the box
/// meets or holds a piece, or the curve has no box.
fn excluded_side(
    face: &SketchArrangementFace,
    bounds: Option<[Point2; 2]>,
    tolerance: f64,
) -> Option<bool> {
    let bounds = bounds?;
    if !bounds_meet(bounds, face.bounds, tolerance) {
        return Some(false);
    }
    let low = Point2::new(bounds[0].u - tolerance, bounds[0].v - tolerance);
    let high = Point2::new(bounds[1].u + tolerance, bounds[1].v + tolerance);
    let corners = [
        low,
        Point2::new(high.u, low.v),
        high,
        Point2::new(low.u, high.v),
    ];
    let sides = polygon_edges(&corners)
        .map(|(start, end)| CertifiedPiece::Exact(ProfileBoundarySegment::Line { start, end }))
        .collect::<Vec<_>>();
    let held =
        |point: Point2| low.u < point.u && point.u < high.u && low.v < point.v && point.v < high.v;
    if !certified_pieces_separated(&face.pieces, &sides)
        || face.pieces.iter().any(|piece| held(piece.start()))
    {
        return None;
    }
    let center = Point2::new(f64::midpoint(low.u, high.u), f64::midpoint(low.v, high.v));
    face.holds(center, tolerance)
}

/// How one curve outside the arrangement relates to each face, from its
/// side of each face in `sides`. When every side is decided, the curve lies
/// in the region of the smallest face that holds it and outside every
/// other face. Otherwise each face that holds or can hold it can have it as
/// an island.
fn excluded_islands(
    faces: &[SketchArrangementFace],
    sides: &[Option<bool>],
) -> Vec<ExcludedIsland> {
    if sides.iter().any(Option::is_none) {
        return sides
            .iter()
            .map(|side| match side {
                Some(false) => ExcludedIsland::Absent,
                Some(true) | None => ExcludedIsland::Possible,
            })
            .collect();
    }
    let smallest = sides
        .iter()
        .zip(faces)
        .enumerate()
        .filter(|(_, (side, _))| **side == Some(true))
        .min_by(|(_, (_, left)), (_, (_, right))| {
            signed_polygon_area(&left.polyline).total_cmp(&signed_polygon_area(&right.polyline))
        })
        .map(|(index, _)| index);
    (0..faces.len())
        .map(|index| {
            if Some(index) == smallest {
                ExcludedIsland::Present
            } else {
                ExcludedIsland::Absent
            }
        })
        .collect()
}

/// The lowest and highest corners of the box of `points`, or `None` when
/// there is no point.
fn polyline_bounds(points: &[Point2]) -> Option<[Point2; 2]> {
    let first = *points.first()?;
    Some(points.iter().fold([first, first], |[low, high], point| {
        [
            Point2::new(low.u.min(point.u), low.v.min(point.v)),
            Point2::new(high.u.max(point.u), high.v.max(point.v)),
        ]
    }))
}

/// Whether two boxes overlap or lie within `tolerance` of each other.
fn bounds_meet(left: [Point2; 2], right: [Point2; 2], tolerance: f64) -> bool {
    left[0].u <= right[1].u + tolerance
        && right[0].u <= left[1].u + tolerance
        && left[0].v <= right[1].v + tolerance
        && right[0].v <= left[1].v + tolerance
}

/// The bounded faces of the arrangement of the non-construction lines,
/// circles, circular arcs, bounded ellipses, and nonperiodic NURBS of
/// `sketch` when `all_curves` holds or the sketch has no profile loop, and
/// otherwise of the curves of its profile loops. The nodes are the end points
/// of these curves and the intersection points of every two non-construction
/// lines, circular arcs, or circles that are not collinear line segments. A
/// node splits a line, circular arc, or circle that passes through it.
/// `None` when the arrangement is undefined, which includes two edges that
/// meet away from their common nodes.
fn curve_arrangement_faces(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
    all_curves: bool,
) -> Result<Option<Vec<SketchArrangementFace>>, CodecError> {
    let segments = entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && !entity.construction)
        .filter_map(|entity| {
            if let cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { center, radius } =
                *entity.geometry.definition()
            {
                Some(ProfileBoundarySegment::Arc {
                    center: center.get(),
                    radius,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::TAU,
                })
            } else {
                profile_use_segment(entity, false)
            }
        })
        .collect::<Vec<_>>();
    let charged = segments
        .len()
        .checked_mul(segments.len())
        .is_some_and(|count| work.arrangement.charge_by(count));
    if !charged {
        work.arrangement_skipped.set(true);
        return Ok(None);
    }
    let mut nodes = Vec::new();
    for (index, left) in segments.iter().enumerate() {
        for right in segments.iter().skip(index + 1) {
            if collinear_line_segments(left, right, tolerance).is_some() {
                continue;
            }
            if let Some(points) = analytic_segment_intersections(left, right) {
                for point in points.into_iter().flatten() {
                    arrangement_node(&mut nodes, point, tolerance);
                }
            }
        }
    }
    sketch_arrangement_faces_with_nodes(sketch, entities, tolerance, work, ctx, nodes, all_curves)
}

/// A region that one arrangement face bounds.
pub(super) struct ArrangementRegion {
    pub(super) region: cadmpeg_ir::features::SketchProfileRegion,
    /// Whether the polyline side of a projected point within the tolerance
    /// of a NURBS tube of the face boundary decided the face.
    pub(super) polyline_side_assumed: bool,
    /// Whether a curve outside the arrangement can lie inside the face.
    pub(super) island_absence_assumed: bool,
}

/// The region `face` bounds when no arrangement component and no curve
/// outside the arrangement lies inside it.
fn arrangement_face_region(face: &SketchArrangementFace) -> Option<ArrangementRegion> {
    use cadmpeg_ir::features::SketchProfileRegion;

    if face.holes.as_ref().is_none_or(|holes| !holes.is_empty())
        || face.excluded_island == ExcludedIsland::Present
    {
        return None;
    }
    Some(ArrangementRegion {
        region: SketchProfileRegion::trimmed(face.ring.boundary.clone(), Vec::new()).ok()?,
        polyline_side_assumed: false,
        island_absence_assumed: face.excluded_island == ExcludedIsland::Possible,
    })
}

fn sketch_arrangement_faces(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<SketchArrangementFace>>, CodecError> {
    sketch_arrangement_faces_with_nodes(sketch, entities, tolerance, work, ctx, Vec::new(), false)
}

/// One bounded face of the arrangement every non-construction curve of a
/// sketch makes: its directed boundary and its boundary polyline.
pub(super) struct AtomicSketchRegion {
    pub(super) boundary: Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>,
    pub(super) polyline: Vec<Point2>,
    /// Whether an island lies inside the face, so its boundary is not the
    /// whole region.
    pub(super) encloses_island: bool,
    /// Whether a curve outside the arrangement can lie inside the face.
    pub(super) island_absence_assumed: bool,
}

/// The bounded faces of the arrangement of every non-construction curve of
/// `sketch`, profiled or not, split at every crossing.
pub(super) fn atomic_sketch_regions(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<AtomicSketchRegion>>, CodecError> {
    let segments = entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && !entity.construction)
        .filter_map(|entity| {
            if let cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { center, radius } =
                *entity.geometry.definition()
            {
                Some(ProfileBoundarySegment::Arc {
                    center: center.get(),
                    radius,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::TAU,
                })
            } else {
                profile_use_segment(entity, false)
            }
        })
        .collect::<Vec<_>>();
    let charged = segments
        .len()
        .checked_mul(segments.len())
        .is_some_and(|count| work.arrangement.charge_by(count));
    if !charged {
        work.arrangement_skipped.set(true);
        return Ok(None);
    }
    let mut nodes = Vec::new();
    for (index, left) in segments.iter().enumerate() {
        for right in segments.iter().skip(index + 1) {
            if let Some(points) = analytic_segment_intersections(left, right) {
                for point in points.into_iter().flatten() {
                    arrangement_node(&mut nodes, point, tolerance);
                }
            }
        }
    }
    let faces = geometric!(sketch_arrangement_faces_with_nodes(
        sketch, entities, tolerance, work, ctx, nodes, true
    )?);
    Ok(Some(
        faces
            .into_iter()
            .map(|face| AtomicSketchRegion {
                boundary: face.ring.boundary,
                polyline: face.polyline,
                encloses_island: face.holes.as_ref().is_none_or(|holes| !holes.is_empty())
                    || face.excluded_island == ExcludedIsland::Present,
                island_absence_assumed: face.excluded_island == ExcludedIsland::Possible,
            })
            .collect(),
    ))
}

/// Most atomic regions whose boundaries lie in one stored loop that the
/// subset search weighs.
const MAX_CROSSING_LOOP_CANDIDATES: usize = 16;

/// The regions a self-crossing stored loop bounds.
pub(super) struct CrossingLoopRegions {
    pub(super) regions: Vec<cadmpeg_ir::features::SketchProfileRegion>,
    /// Whether a curve outside the arrangement can lie inside a selected
    /// face.
    pub(super) island_absence_assumed: bool,
}

/// The atomic regions a stored loop bounds when the loop crosses itself.
///
/// The loop lists one sketch curve per boundary use. The selected regions are
/// the unique non-empty set of atomic regions of the sketch whose boundaries
/// use only loop curves and use every loop curve as often as the loop lists
/// it. `None` when a loop curve is not a line or circular arc, when a
/// selected region encloses an island, or when no set or more than one set
/// satisfies the counts.
pub(super) fn arrangement_regions_of_crossing_loop(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    program: &[cadmpeg_ir::sketches::SketchEntityId],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CrossingLoopRegions>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if program.len() < 2 {
        return Ok(None);
    }
    let mut wanted = HashMap::<&cadmpeg_ir::sketches::SketchEntityId, usize>::new();
    for curve in program {
        let carrier = entities.iter().find(|entity| {
            entity.sketch == sketch.id && entity.id() == curve && !entity.construction
        });
        if !carrier.is_some_and(|entity| {
            matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. }
            ) && sketch_geometry_parameter_range(&entity.geometry).is_some()
        }) {
            return Ok(None);
        }
        *wanted.entry(curve).or_insert(0) += 1;
    }
    let regions = geometric!(atomic_sketch_regions(
        sketch, entities, tolerance, work, ctx
    )?);
    // Boundary uses per entity of each candidate region, with consecutive
    // pieces of one carrier counted once, as the loop lists them.
    let mut candidates = Vec::new();
    let mut candidate_regions = Vec::new();
    for region in &regions {
        let mut curves = Vec::<&cadmpeg_ir::sketches::SketchEntityId>::new();
        for use_ in &region.boundary {
            if curves.last().copied() != Some(&use_.entity) {
                curves.push(&use_.entity);
            }
        }
        if curves.len() > 1 && curves.first() == curves.last() {
            curves.pop();
        }
        // A face that encloses an island is never selected, so it is no
        // candidate; the other faces are still weighed.
        if !region.encloses_island && curves.iter().all(|curve| wanted.contains_key(curve)) {
            candidates.push(curves);
            candidate_regions.push(region);
        }
    }
    if candidates.len() > MAX_CROSSING_LOOP_CANDIDATES {
        work.crossing_loop_skipped.set(true);
        return Ok(None);
    }
    if candidates.len() < 2 {
        return Ok(None);
    }
    // Each subset traverses the runs of its candidates and compares the
    // counts of every wanted curve.
    let runs = candidates
        .iter()
        .map(Vec::len)
        .try_fold(wanted.len(), usize::checked_add);
    let charged = runs
        .and_then(|runs| (1usize << candidates.len()).checked_mul(runs))
        .is_some_and(|count| work.arrangement.charge_by(count));
    if !charged {
        work.arrangement_skipped.set(true);
        return Ok(None);
    }
    let mut chosen = None;
    for mask in 1usize..(1usize << candidates.len()) {
        let mut counts = HashMap::<&cadmpeg_ir::sketches::SketchEntityId, usize>::new();
        for (index, curves) in candidates.iter().enumerate() {
            if mask >> index & 1 == 1 {
                for curve in curves {
                    *counts.entry(*curve).or_insert(0) += 1;
                }
            }
        }
        if counts.len() == wanted.len()
            && counts.iter().all(|(curve, n)| wanted.get(curve) == Some(n))
            && chosen.replace(mask).is_some()
        {
            return Ok(None);
        }
    }
    let mask = geometric!(chosen);
    // The mask indexes the same filtered candidates the search weighed.
    let mut selected = Vec::new();
    let mut island_absence_assumed = false;
    for (index, region) in candidate_regions.iter().enumerate() {
        if mask >> index & 1 == 1 {
            island_absence_assumed |= region.island_absence_assumed;
            selected.push(geometric!(SketchProfileRegion::trimmed(
                region.boundary.clone(),
                Vec::new()
            )
            .ok()));
        }
    }
    Ok(Some(CrossingLoopRegions {
        regions: selected,
        island_absence_assumed,
    }))
}

fn sketch_arrangement_faces_with_nodes(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
    mut nodes: Vec<Point2>,
    all_curves: bool,
) -> Result<Option<Vec<SketchArrangementFace>>, CodecError> {
    use cadmpeg_ir::features::SketchProfileBoundaryUse;
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometryDefinition};

    let mut pending = Vec::<SketchProfileBoundaryUse>::new();
    let mut circles = Vec::new();
    let mut candidate_uses = Vec::new();
    if all_curves || sketch.profiles.is_empty() {
        for entity in entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id && !entity.construction)
            .filter(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                ) || sketch_geometry_parameter_range(&entity.geometry).is_some()
            })
        {
            let entity = entity.id().clone();
            candidate_uses.push(SketchEntityUse {
                entity,
                reversed: false,
            });
        }
    } else {
        for use_ in sketch.profiles.iter().flat_map(|profile| profile.iter()) {
            let entity = use_.entity.clone();
            candidate_uses.push(SketchEntityUse {
                entity,
                reversed: use_.reversed,
            });
        }
    }
    for use_ in candidate_uses {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        if let SketchGeometryDefinition::Circle { center, radius } = *entity.geometry.definition() {
            circles.push((use_, center.get(), radius));
            continue;
        }
        let range = geometric!(sketch_geometry_parameter_range(&entity.geometry));
        for point in [
            geometric!(sketch_geometry_point(&entity.geometry, range[0], ctx)?),
            geometric!(sketch_geometry_point(&entity.geometry, range[1], ctx)?),
        ] {
            arrangement_node(&mut nodes, point, tolerance);
        }
        let id = entity.id().clone();
        pending.push(SketchProfileBoundaryUse {
            entity: id,
            parameter_range: geometric!(
                cadmpeg_ir::geometry::DirectedParameterRange::new(range).ok()
            ),
            reversed: use_.reversed,
        });
    }
    let mut synthetic_nodes = Vec::new();
    for (use_, center, radius) in circles {
        if radius.get() <= tolerance {
            return Ok(None);
        }
        let mut angles = arrangement_circle_angles(&nodes, center, radius, tolerance);
        if angles.len() < 2 {
            let additional = if angles.is_empty() {
                [Some(0.0), Some(std::f64::consts::PI)]
            } else {
                [Some(angles[0] + std::f64::consts::PI), None]
            };
            for angle in additional.into_iter().flatten() {
                let count = nodes.len();
                let index = arrangement_node(
                    &mut nodes,
                    Point2::new(
                        center.u + radius.get() * angle.cos(),
                        center.v + radius.get() * angle.sin(),
                    ),
                    tolerance,
                );
                if nodes.len() > count {
                    synthetic_nodes.push(index);
                }
            }
            angles = arrangement_circle_angles(&nodes, center, radius, tolerance);
        }
        if angles.len() < 2 {
            return Ok(None);
        }
        for index in 0..angles.len() {
            let start = angles[index];
            let mut end = angles[(index + 1) % angles.len()];
            if index + 1 == angles.len() {
                end += std::f64::consts::TAU;
            }
            let range = [start, end];
            let id = use_.entity.clone();
            pending.push(SketchProfileBoundaryUse {
                entity: id,
                parameter_range: geometric!(cadmpeg_ir::geometry::DirectedParameterRange::new(
                    range
                )
                .ok()),
                reversed: use_.reversed,
            });
        }
    }
    let mut split_pending = Vec::new();
    for boundary in pending {
        let entity = geometric!(entities
            .iter()
            .find(|entity| entity.id() == &boundary.entity));
        let parameters = geometric!(arrangement_split_parameters(
            &entity.geometry,
            boundary.parameter_range.endpoints(),
            &nodes,
            tolerance
        )?);
        for parameters in parameters.windows(2) {
            let range = [parameters[0], parameters[1]];
            let id = boundary.entity.clone();
            let parameter_range =
                geometric!(cadmpeg_ir::geometry::DirectedParameterRange::new(range).ok());
            let polyline = geometric!(profile_use_polyline(
                entity,
                range,
                boundary.reversed,
                tolerance,
                ctx,
            )?);
            split_pending.push((
                SketchProfileBoundaryUse {
                    entity: id,
                    parameter_range,
                    reversed: boundary.reversed,
                },
                polyline,
            ));
        }
    }
    let mut edges = Vec::<SketchArrangementEdge>::new();
    for (boundary, polyline) in split_pending {
        let edge = SketchArrangementEdge {
            nodes: [
                arrangement_node(&mut nodes, *geometric!(polyline.first()), tolerance),
                arrangement_node(&mut nodes, *geometric!(polyline.last()), tolerance),
            ],
            boundary,
            polyline,
            coincident: Vec::new(),
        };
        if edge.nodes[0] == edge.nodes[1] {
            return Ok(None);
        }
        let mut coincident = false;
        for candidate in &mut edges {
            if (candidate.nodes == edge.nodes || candidate.nodes == [edge.nodes[1], edge.nodes[0]])
                && arrangement_edges_coincident(candidate, &edge, entities, tolerance, ctx)?
            {
                if candidate.boundary.entity == edge.boundary.entity
                    || candidate
                        .coincident
                        .iter()
                        .any(|use_| use_.entity == edge.boundary.entity)
                {
                    return Ok(None);
                }
                candidate
                    .coincident
                    .push(cadmpeg_ir::sketches::SketchEntityUse {
                        entity: edge.boundary.entity.clone(),
                        reversed: edge.boundary.reversed != (candidate.nodes != edge.nodes),
                    });
                coincident = true;
                break;
            }
        }
        if coincident {
            continue;
        }
        edges.push(edge);
    }
    arrangement_retain_cycle_edges(&mut edges, nodes.len(), &work.arrangement, ctx)?;
    if work.arrangement.exhausted() {
        work.arrangement_skipped.set(true);
        return Ok(None);
    }
    if edges.len() < 3 {
        return Ok(None);
    }
    let mut edge_pieces = Vec::new();
    for edge in &edges {
        let Some(pieces) = arrangement_edge_pieces(edge, entities, tolerance, work, ctx)? else {
            return Ok(None);
        };
        edge_pieces.push(pieces);
    }
    let mut edge_bounds = Vec::new();
    for pieces in &edge_pieces {
        let bounds = geometric!(certified_pieces_bounds(pieces));
        edge_bounds.push(bounds);
    }
    for left_index in 0..edges.len() {
        for right_index in left_index + 1..edges.len() {
            let left = &edges[left_index];
            let right = &edges[right_index];
            let mut shared_nodes = [0; 2];
            let mut shared_count = 0;
            for node in left.nodes {
                if right.nodes.contains(&node) {
                    shared_nodes[shared_count] = node;
                    shared_count += 1;
                }
            }
            if shared_count != 0 {
                if !arrangement_edges_meet_only_at_nodes(
                    left,
                    right,
                    entities,
                    &nodes,
                    &shared_nodes[..shared_count],
                    tolerance,
                ) {
                    return Ok(None);
                }
                continue;
            }
            let left_bounds = edge_bounds[left_index];
            let right_bounds = edge_bounds[right_index];
            if left_bounds[1].u < right_bounds[0].u
                || right_bounds[1].u < left_bounds[0].u
                || left_bounds[1].v < right_bounds[0].v
                || right_bounds[1].v < left_bounds[0].v
            {
                continue;
            }
            if !certified_pieces_separated(&edge_pieces[left_index], &edge_pieces[right_index]) {
                return Ok(None);
            }
        }
    }
    let mut outgoing = ctx.alloc_filled(
        nodes.len(),
        Vec::<(usize, bool, f64)>::new(),
        "f3d_arrangement_outgoing",
    )?;
    let mut departures = ctx.alloc_filled(
        nodes.len(),
        Vec::<Departure>::new(),
        "f3d_arrangement_departures",
    )?;
    for (edge_index, edge) in edges.iter().enumerate() {
        for reverse in [false, true] {
            let node = edge.nodes[usize::from(reverse)];
            let departure = geometric!(arrangement_departure(edge, reverse, entities));
            outgoing[node].push((edge_index, reverse, departure.angle));
            departures[node].push(departure);
        }
    }
    if edges
        .iter()
        .any(|edge| edge.nodes.iter().any(|node| outgoing[*node].len() < 2))
    {
        return Ok(None);
    }
    for (uses, departures) in outgoing.iter_mut().zip(departures.iter()) {
        let mut order = (0..uses.len()).collect::<Vec<_>>();
        if !order_departures(&mut order, departures, tolerance) {
            return Ok(None);
        }
        let sorted = order.iter().map(|index| uses[*index]).collect::<Vec<_>>();
        *uses = sorted;
    }
    let mut component = ctx.alloc_filled(nodes.len(), usize::MAX, "f3d arrangement components")?;
    let mut component_roots = Vec::new();
    for root in 0..nodes.len() {
        if component[root] != usize::MAX || outgoing[root].is_empty() {
            continue;
        }
        component[root] = root;
        component_roots.push(root);
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            for (edge_index, reverse, _) in &outgoing[node] {
                let next = edges[*edge_index].nodes[usize::from(!*reverse)];
                if component[next] == usize::MAX {
                    component[next] = root;
                    pending.push(next);
                }
            }
        }
    }
    let mut edge_boxes = Vec::new();
    for (edge, certified) in edges.iter().zip(&edge_bounds) {
        edge_boxes.push(geometric!(arrangement_edge_box(edge, entities, *certified)));
    }
    let mut visited = ctx.alloc_filled(edges.len(), [false; 2], "f3d arrangement edge visits")?;
    let mut faces = Vec::new();
    let mut face_components = Vec::new();
    let mut outer_walks = Vec::new();
    for edge_index in 0..edges.len() {
        for reversed in [false, true] {
            if visited[edge_index][usize::from(reversed)] {
                continue;
            }
            let start = (edge_index, reversed);
            let mut current = start;
            let mut boundary = Vec::new();
            let mut runs = Vec::<ArrangementFragment>::new();
            let mut polyline = Vec::new();
            let mut pieces = Vec::new();
            let mut bounds = None::<[Point2; 2]>;
            loop {
                let (index, reverse) = current;
                if visited[index][usize::from(reverse)] {
                    if current != start {
                        return Ok(None);
                    }
                    break;
                }
                visited[index][usize::from(reverse)] = true;
                let edge = &edges[index];
                let [low, high] = edge_boxes[index];
                bounds = Some(bounds.map_or([low, high], |[face_low, face_high]| {
                    [
                        Point2::new(face_low.u.min(low.u), face_low.v.min(low.v)),
                        Point2::new(face_high.u.max(high.u), face_high.v.max(high.v)),
                    ]
                }));
                let mut use_ = edge.boundary.clone();
                if reverse {
                    use_.reversed = !use_.reversed;
                }
                let joint = edge.nodes[usize::from(reverse)];
                let coincident =
                    edge.coincident
                        .iter()
                        .map(|other| cadmpeg_ir::sketches::SketchEntityUse {
                            entity: other.entity.clone(),
                            reversed: other.reversed != reverse,
                        });
                match runs.last_mut() {
                    Some(run)
                        if synthetic_nodes.contains(&joint) && run.curve.entity == use_.entity =>
                    {
                        for other in coincident {
                            if !merge_coincident(&mut run.coincident, other) {
                                return Ok(None);
                            }
                        }
                    }
                    _ => runs.push(ArrangementFragment {
                        curve: cadmpeg_ir::sketches::SketchEntityUse {
                            entity: use_.entity.clone(),
                            reversed: use_.reversed,
                        },
                        coincident: coincident.collect(),
                    }),
                }
                boundary.push(use_);
                if reverse {
                    for point in edge.polyline.iter().rev().take(edge.polyline.len() - 1) {
                        polyline.push(*point);
                    }
                    pieces.extend(
                        edge_pieces[index]
                            .iter()
                            .rev()
                            .map(CertifiedPiece::reversed),
                    );
                } else {
                    for point in edge.polyline.iter().take(edge.polyline.len() - 1) {
                        polyline.push(*point);
                    }
                    pieces.extend(edge_pieces[index].iter().copied());
                }
                let destination = edge.nodes[usize::from(!reverse)];
                let uses = &outgoing[destination];
                let twin = geometric!(uses.iter().position(|(candidate, candidate_reverse, _)| {
                    *candidate == index && *candidate_reverse != reverse
                }));
                let next = uses[(twin + uses.len() - 1) % uses.len()];
                current = (next.0, next.1);
            }
            if runs.len() > 1
                && runs.first().map(|run| &run.curve.entity)
                    == runs.last().map(|run| &run.curve.entity)
                && synthetic_nodes.contains(&edges[edge_index].nodes[usize::from(reversed)])
            {
                if let Some(last) = runs.pop() {
                    for other in last.coincident {
                        if !merge_coincident(&mut runs[0].coincident, other) {
                            return Ok(None);
                        }
                    }
                }
            }
            let ring = ArrangementRing {
                boundary,
                fragments: runs,
            };
            if polyline.len() < 3 {
                continue;
            }
            let own = component[edges[edge_index].nodes[0]];
            let area = signed_polygon_area(&polyline);
            if area > tolerance * tolerance {
                faces.push(SketchArrangementFace {
                    ring,
                    polyline,
                    holes: Some(Vec::new()),
                    excluded_island: ExcludedIsland::Absent,
                    pieces,
                    bounds: geometric!(bounds),
                });
                face_components.push(own);
            } else if area < -(tolerance * tolerance) {
                outer_walks.push((own, ring));
            }
        }
    }
    let mut component_nodes = HashMap::<usize, Vec<Point2>>::new();
    for (node, owner) in component.iter().enumerate() {
        if *owner != usize::MAX {
            component_nodes.entry(*owner).or_default().push(nodes[node]);
        }
    }
    for root in &component_roots {
        let members = geometric!(component_nodes.get(root));
        let mut containers = Vec::new();
        let mut undecided = Vec::new();
        for (index, face) in faces.iter().enumerate() {
            if face_components[index] == *root {
                continue;
            }
            match face.holds_component(members, tolerance) {
                Some(true) => containers.push(index),
                Some(false) => {}
                None => undecided.push(index),
            }
        }
        // Containers nest, and the innermost lies inside every other one.
        let mut innermost = Vec::new();
        for candidate in &containers {
            let inner = geometric!(component_nodes.get(&face_components[*candidate]));
            let mut inside_every = Some(true);
            for other in containers.iter().filter(|other| *other != candidate) {
                match faces[*other].holds_component(inner, tolerance) {
                    Some(true) => {}
                    Some(false) => {
                        inside_every = Some(false);
                        break;
                    }
                    None => inside_every = None,
                }
            }
            match inside_every {
                Some(true) => innermost.push(*candidate),
                Some(false) => {}
                None => undecided.push(*candidate),
            }
        }
        let container = match innermost.as_slice() {
            [only] if undecided.is_empty() => Some(*only),
            _ => None,
        };
        if containers.is_empty() && undecided.is_empty() {
            continue;
        }
        let mut walks = outer_walks.iter().filter(|(own, _)| own == root);
        let walk = walks
            .next()
            .filter(|_| walks.next().is_none())
            .map(|(_, walk)| walk.clone());
        match (container, walk) {
            (Some(container), Some(walk)) => {
                if let Some(holes) = faces[container].holes.as_mut() {
                    holes.push(walk);
                }
            }
            (container, walk) => {
                let unwalked = walk.is_none();
                for (index, face) in faces.iter_mut().enumerate() {
                    let holds = match container {
                        Some(container) => index == container,
                        None => containers.contains(&index) || undecided.contains(&index),
                    };
                    if holds || (unwalked && face_components[index] == *root) {
                        face.holes = None;
                    }
                }
            }
        }
    }
    if all_curves || sketch.profiles.is_empty() {
        let outside = entities
            .iter()
            .filter(|entity| {
                entity.sketch == sketch.id && !entity.construction && !arrangement_family(entity)
            })
            .map(outside_curve_bounds)
            .collect::<Vec<_>>();
        let mut relations = faces
            .iter()
            .map(|_| ExcludedIsland::Absent)
            .collect::<Vec<_>>();
        for bounds in &outside {
            let sides = faces
                .iter()
                .map(|face| excluded_side(face, *bounds, tolerance))
                .collect::<Vec<_>>();
            for (relation, island) in relations.iter_mut().zip(excluded_islands(&faces, &sides)) {
                *relation = (*relation).max(island);
            }
        }
        for (face, relation) in faces.iter_mut().zip(relations) {
            face.excluded_island = relation;
        }
    }
    Ok((!faces.is_empty()).then_some(faces))
}

/// The direction in which an arrangement edge leaves a node, and its signed
/// curvature there, positive when it turns left.
#[derive(Clone, Copy)]
struct Departure {
    angle: f64,
    curvature: DepartureCurvature,
}

#[derive(Clone, Copy)]
enum DepartureCurvature {
    /// The signed curvature in inverse millimetres.
    Exact(f64),
    /// The sign of the curvature: -1, 0, or 1.
    Sign(f64),
    /// The departure is the direction of the polyline's first chord.
    Unknown,
}

impl DepartureCurvature {
    fn sign(self) -> Option<f64> {
        match self {
            Self::Exact(curvature) => Some(if curvature == 0.0 {
                0.0
            } else {
                curvature.signum()
            }),
            Self::Sign(sign) => Some(sign),
            Self::Unknown => None,
        }
    }
}

/// How `edge` leaves its start node, or its end node when `reverse`. A line
/// leaves along its direction with curvature zero, a circular arc along its
/// tangent with curvature `1 / r` signed by its turn, and a clamped NURBS end
/// along its first nonzero control leg with the sign of its first turn.
/// Another edge leaves along the first chord of its polyline with an unknown
/// curvature. `None` when the edge has no polyline step.
fn arrangement_departure(
    edge: &SketchArrangementEdge,
    reverse: bool,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<Departure> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let polyline = &edge.polyline;
    let (from, to) = if reverse {
        (
            *polyline.last()?,
            *polyline.get(polyline.len().checked_sub(2)?)?,
        )
    } else {
        (*polyline.first()?, *polyline.get(1)?)
    };
    let chord = (to.u - from.u, to.v - from.v);
    let chord_departure = Departure {
        angle: chord.1.atan2(chord.0),
        curvature: DepartureCurvature::Unknown,
    };
    let entity = entities
        .iter()
        .find(|entity| entity.id() == &edge.boundary.entity)?;
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some(Departure {
            angle: chord_departure.angle,
            curvature: DepartureCurvature::Exact(0.0),
        }),
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => {
            let center = center.get();
            let radial = (from.u - center.u, from.v - center.v);
            let counterclockwise = (-radial.1, radial.0);
            let turns_left = counterclockwise.0 * chord.0 + counterclockwise.1 * chord.1 > 0.0;
            let (u, v) = if turns_left {
                counterclockwise
            } else {
                (-counterclockwise.0, -counterclockwise.1)
            };
            let curvature = 1.0 / radius.get();
            Some(Departure {
                angle: v.atan2(u),
                curvature: DepartureCurvature::Exact(if turns_left {
                    curvature
                } else {
                    -curvature
                }),
            })
        }
        SketchGeometryDefinition::Nurbs { curve } => {
            let at_start = reverse == edge.boundary.reversed;
            Some(nurbs_departure(curve, at_start).unwrap_or(chord_departure))
        }
        _ => Some(chord_departure),
    }
}

/// How a clamped NURBS leaves its start, or its end: along its first nonzero
/// control leg, with the sign of the turn to the first later leg that is not
/// parallel to it. `None` when that end is not clamped, a weight is not
/// positive, or every leg is zero.
fn nurbs_departure(
    curve: &cadmpeg_ir::geometry::pcurve::PcurveNurbs,
    at_start: bool,
) -> Option<Departure> {
    if curve
        .weights()
        .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
    {
        return None;
    }
    let degree = usize::try_from(curve.degree()).ok()?;
    let knots = curve.knots().as_slice();
    let end = if at_start {
        knots.get(..=degree)?
    } else {
        knots.get(knots.len().checked_sub(degree + 1)?..)?
    };
    if end.iter().any(|knot| *knot != end[0]) {
        return None;
    }
    let mut points = curve
        .control_points()
        .into_iter()
        .map(cadmpeg_ir::units::FinitePoint2::get)
        .collect::<Vec<_>>();
    if !at_start {
        points.reverse();
    }
    let mut legs = points
        .windows(2)
        .map(|pair| (pair[1].u - pair[0].u, pair[1].v - pair[0].v))
        .filter(|leg| leg.0 != 0.0 || leg.1 != 0.0);
    let leg = legs.next()?;
    let turn = legs
        .map(|next| leg.0 * next.1 - leg.1 * next.0)
        .find(|cross| *cross != 0.0)
        .map_or(0.0, f64::signum);
    Some(Departure {
        angle: leg.1.atan2(leg.0),
        curvature: DepartureCurvature::Sign(turn),
    })
}

/// Order `order`, indices into `departures`, counterclockwise around their
/// node. A departure's key is its angle, plus half the turn of an exact
/// curvature over `tolerance`, where it leaves the node's ball. The cyclic
/// order starts after the widest gap between keys, so no group straddles
/// the start. Neighbours whose keys or tangent angles are equal form a
/// group, and groups order by increasing signed curvature. Two departures need no order. `false` when a departure along a
/// polyline chord meets more than one other edge, or when a group with the
/// same key cannot be ordered.
fn order_departures(order: &mut [usize], departures: &[Departure], tolerance: f64) -> bool {
    let key = |index: usize| {
        let departure = departures[index];
        match departure.curvature {
            DepartureCurvature::Exact(curvature) => departure.angle + curvature * tolerance / 2.0,
            DepartureCurvature::Sign(_) | DepartureCurvature::Unknown => departure.angle,
        }
    };
    order.sort_by(|left, right| key(*left).total_cmp(&key(*right)));
    let count = order.len();
    if count <= 2 {
        return true;
    }
    if order
        .iter()
        .any(|index| matches!(departures[*index].curvature, DepartureCurvature::Unknown))
    {
        return false;
    }
    let gap = |position: usize| {
        let next = (position + 1) % count;
        let mut gap = key(order[next]) - key(order[position]);
        if next == 0 {
            gap += std::f64::consts::TAU;
        }
        gap
    };
    let Some(widest) = (0..count).max_by(|left, right| gap(*left).total_cmp(&gap(*right))) else {
        return false;
    };
    order.rotate_left((widest + 1) % count);
    let mut keys = Vec::<(f64, f64)>::with_capacity(count);
    let mut turns = 0.0;
    for index in order.iter() {
        if keys
            .last()
            .is_some_and(|previous| key(*index) + turns < previous.0)
        {
            turns += std::f64::consts::TAU;
        }
        keys.push((key(*index) + turns, departures[*index].angle + turns));
    }
    let tied = |left: (f64, f64), right: (f64, f64)| {
        right.0 - left.0 <= ARRANGEMENT_TANGENT_TOLERANCE
            || (right.1 - left.1).abs() <= ARRANGEMENT_TANGENT_TOLERANCE
    };
    let mut start = 0;
    while start < count {
        let mut end = start + 1;
        while end < count && tied(keys[end - 1], keys[end]) {
            end += 1;
        }
        if end - start > 1 && !order_by_curvature(&mut order[start..end], departures) {
            return false;
        }
        start = end;
    }
    true
}

/// Order a group of departures with the same tangent by increasing signed
/// curvature. `false` when two of them cannot be told apart.
fn order_by_curvature(group: &mut [usize], departures: &[Departure]) -> bool {
    let exact = group
        .iter()
        .map(|index| match departures[*index].curvature {
            DepartureCurvature::Exact(curvature) => Some(curvature),
            DepartureCurvature::Sign(_) | DepartureCurvature::Unknown => None,
        })
        .collect::<Option<Vec<_>>>();
    if let Some(mut curvatures) = exact {
        let mut pairs = group
            .iter()
            .copied()
            .zip(curvatures.drain(..))
            .collect::<Vec<_>>();
        pairs.sort_by(|left, right| left.1.total_cmp(&right.1));
        if pairs
            .windows(2)
            .any(|pair| pair[1].1 - pair[0].1 <= ARRANGEMENT_CURVATURE_TOLERANCE)
        {
            return false;
        }
        for (slot, (index, _)) in group.iter_mut().zip(pairs) {
            *slot = index;
        }
        return true;
    }
    let signs = group
        .iter()
        .map(|index| departures[*index].curvature.sign())
        .collect::<Option<Vec<_>>>();
    let Some(signs) = signs else {
        return false;
    };
    let mut pairs = group.iter().copied().zip(signs).collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.1.total_cmp(&right.1));
    if pairs.windows(2).any(|pair| pair[1].1 <= pair[0].1) {
        return false;
    }
    for (slot, (index, _)) in group.iter_mut().zip(pairs) {
        *slot = index;
    }
    true
}

/// The lowest and highest corners of a box that holds `edge`: its end
/// points for a line, its end points and the axis extremes inside its swept
/// angle for a circular arc, and the `certified` tube bounds otherwise.
fn arrangement_edge_box(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    certified: [Point2; 2],
) -> Option<[Point2; 2]> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let entity = entities
        .iter()
        .find(|entity| entity.id() == &edge.boundary.entity)?;
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { .. } => {
            polyline_bounds(&[*edge.polyline.first()?, *edge.polyline.last()?])
        }
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => {
            let [start, end] = edge.boundary.parameter_range.endpoints();
            let (low, high) = (start.min(end), start.max(end));
            let quarter = std::f64::consts::FRAC_PI_2;
            let first = (low / quarter).ceil();
            let last = (high / quarter).floor();
            if !(first.is_finite() && last.is_finite()) || last - first > 8.0 {
                return Some(certified);
            }
            let mut angles = vec![start, end];
            let mut step = first;
            while step <= last {
                angles.push(step * quarter);
                step += 1.0;
            }
            let center = center.get();
            let points = angles
                .into_iter()
                .map(|angle| {
                    Point2::new(
                        center.u + radius.get() * angle.cos(),
                        center.v + radius.get() * angle.sin(),
                    )
                })
                .collect::<Vec<_>>();
            polyline_bounds(&points)
        }
        _ => Some(certified),
    }
}

/// Add `other` to the coincident curves of a fragment. `false` when the
/// fragment already holds the curve in the other direction.
fn merge_coincident(
    coincident: &mut Vec<cadmpeg_ir::sketches::SketchEntityUse>,
    other: cadmpeg_ir::sketches::SketchEntityUse,
) -> bool {
    match coincident.iter().find(|use_| use_.entity == other.entity) {
        Some(held) => held.reversed == other.reversed,
        None => {
            coincident.push(other);
            true
        }
    }
}

fn arrangement_edges_meet_only_at_nodes(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    if arrangement_line_nurbs_meet_only_at_endpoint(
        left,
        right,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) || arrangement_line_nurbs_meet_only_at_endpoint(
        right,
        left,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) {
        return true;
    }
    if arrangement_arc_nurbs_meet_only_at_endpoint(
        left,
        right,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) || arrangement_arc_nurbs_meet_only_at_endpoint(
        right,
        left,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) {
        return true;
    }
    if left.boundary.entity == right.boundary.entity
        && entities.iter().any(|entity| {
            entity.id() == &left.boundary.entity
                && matches!(
                    entity.geometry.definition(),
                    cadmpeg_ir::sketches::SketchGeometryDefinition::Line { .. }
                )
        })
    {
        let left = left.boundary.parameter_range.endpoints();
        let right = right.boundary.parameter_range.endpoints();
        return left[0].max(left[1]) <= right[0].min(right[1])
            || right[0].max(right[1]) <= left[0].min(left[1]);
    }
    let Some(left_segment) = arrangement_analytic_segment(left, entities) else {
        return false;
    };
    let Some(right_segment) = arrangement_analytic_segment(right, entities) else {
        return false;
    };
    if let Some(overlap) = collinear_line_segments(&left_segment, &right_segment, tolerance) {
        return overlap <= tolerance;
    }
    if let (
        ProfileBoundarySegment::Arc {
            center: left_center,
            radius: left_radius,
            start_angle: left_start,
            end_angle: left_end,
            ..
        },
        ProfileBoundarySegment::Arc {
            center: right_center,
            radius: right_radius,
            start_angle: right_start,
            end_angle: right_end,
            ..
        },
    ) = (&left_segment, &right_segment)
    {
        if left_center == right_center && left_radius == right_radius {
            let strictly_inside = |angle: f64, start: f64, end: f64| {
                angle_strictly_inside_arc(angle, start, end, left_radius.get(), tolerance)
            };
            return !strictly_inside(*left_start, *right_start, *right_end)
                && !strictly_inside(*left_end, *right_start, *right_end)
                && !strictly_inside(*right_start, *left_start, *left_end)
                && !strictly_inside(*right_end, *left_start, *left_end);
        }
    }
    if let Some(touch) = collinear_lines_touch_only(left, right, tolerance) {
        return touch;
    }
    analytic_segment_intersections(&left_segment, &right_segment).is_some_and(|intersections| {
        intersections.iter().flatten().all(|intersection| {
            shared_nodes
                .iter()
                .any(|node| point_distance(*intersection, nodes[*node]) <= tolerance)
        })
    })
}

/// For two straight edges on one line, whether they share no more than an end;
/// `None` when either edge is curved or the edges are not collinear.
fn collinear_lines_touch_only(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    tolerance: f64,
) -> Option<bool> {
    let straight = |edge: &SketchArrangementEdge| -> Option<(Point2, Point2)> {
        let (start, end) = (*edge.polyline.first()?, *edge.polyline.last()?);
        edge.polyline
            .iter()
            .all(|point| point_segment_distance(*point, (start, end)) <= tolerance)
            .then_some((start, end))
    };
    let (a, b) = straight(left)?;
    let (c, d) = straight(right)?;
    let length = point_distance(a, b);
    if length <= tolerance
        || point_line_distance(c, (a, b)) > tolerance
        || point_line_distance(d, (a, b)) > tolerance
    {
        return None;
    }
    let along = |p: Point2| ((p.u - a.u) * (b.u - a.u) + (p.v - a.v) * (b.v - a.v)) / length;
    let (c, d) = (along(c), along(d));
    let overlap = length.min(c.max(d)) - 0.0_f64.max(c.min(d));
    Some(overlap <= tolerance)
}

/// The distance from `point` to the infinite line through `a` and `b`.
fn point_line_distance(point: Point2, (a, b): (Point2, Point2)) -> f64 {
    let length = point_distance(a, b);
    ((b.u - a.u) * (a.v - point.v) - (a.u - point.u) * (b.v - a.v)).abs() / length
}

/// Whether `angle` lies strictly inside the directed arc from `start` to `end`
/// on a circle of radius `radius`, with `tolerance` a length on that circle.
///
/// The arc's length is positive and finite: a sketch circle or arc has a
/// positive finite radius and a parameter range with distinct finite
/// endpoints. An arc no longer than twice the tolerance has no strict
/// interior, because every point of it is then within `tolerance` of an
/// endpoint; that is the state the arc-length fraction has no answer for, and
/// it is answered here rather than capped.
fn angle_strictly_inside_arc(
    angle: f64,
    start: f64,
    end: f64,
    radius: f64,
    tolerance: f64,
) -> bool {
    let arc_length = radius * (end - start).abs();
    if arc_length <= 2.0 * tolerance {
        return false;
    }
    let parameter_tolerance = tolerance / arc_length;
    directed_angle_parameter(angle, start, end).is_some_and(|parameter| {
        parameter > parameter_tolerance && parameter < 1.0 - parameter_tolerance
    })
}

fn arrangement_arc_nurbs_meet_only_at_endpoint(
    arc: &SketchArrangementEdge,
    nurbs: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if shared_nodes.len() != 1 {
        return false;
    }
    let Some(arc_entity) = entities
        .iter()
        .find(|entity| entity.id() == &arc.boundary.entity)
    else {
        return false;
    };
    let Some(nurbs_entity) = entities
        .iter()
        .find(|entity| entity.id() == &nurbs.boundary.entity)
    else {
        return false;
    };
    let (center, radius) = match *arc_entity.geometry.definition() {
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => (center.get(), radius.get()),
        _ => return false,
    };
    let SketchGeometryDefinition::Nurbs { curve } = nurbs_entity.geometry.definition() else {
        return false;
    };
    if curve.periodic() {
        return false;
    }
    if curve
        .weights()
        .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
        || sketch_geometry_parameter_range(&nurbs_entity.geometry)
            != Some(nurbs.boundary.parameter_range.endpoints())
    {
        return false;
    }
    let shared = nodes[shared_nodes[0]];
    if (point_distance(center, shared) - radius).abs() > tolerance {
        return false;
    }
    let first_shared = point_distance(curve.control_points()[0].get(), shared) <= tolerance;
    let last_shared = curve
        .control_points()
        .last()
        .is_some_and(|point| point_distance(point.get(), shared) <= tolerance);
    if first_shared == last_shared {
        return false;
    }
    let endpoint = if first_shared {
        0
    } else {
        curve.pole_rows().count() - 1
    };
    let normal = Point2::new(shared.u - center.u, shared.v - center.v);
    let support = |point: Point2| normal.u * (point.u - shared.u) + normal.v * (point.v - shared.v);
    let threshold = tolerance * radius;
    support(curve.control_points()[endpoint].get()).abs() <= threshold
        && (curve
            .control_points()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != endpoint)
            .all(|(_, point)| support(point.get()) > threshold)
            || curve
                .control_points()
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != endpoint)
                .all(|(_, point)| point_distance(center, point.get()) < radius - tolerance))
}

fn arrangement_line_nurbs_meet_only_at_endpoint(
    line: &SketchArrangementEdge,
    nurbs: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if shared_nodes.len() != 1 {
        return false;
    }
    let Some(line_entity) = entities
        .iter()
        .find(|entity| entity.id() == &line.boundary.entity)
    else {
        return false;
    };
    let Some(nurbs_entity) = entities
        .iter()
        .find(|entity| entity.id() == &nurbs.boundary.entity)
    else {
        return false;
    };
    let SketchGeometryDefinition::Line { start, end } = *line_entity.geometry.definition() else {
        return false;
    };
    let (start, end) = (start.get(), end.get());
    let SketchGeometryDefinition::Nurbs { curve } = nurbs_entity.geometry.definition() else {
        return false;
    };
    if curve.periodic() {
        return false;
    }
    if curve
        .weights()
        .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
    {
        return false;
    }
    let Some(domain) = sketch_geometry_parameter_range(&nurbs_entity.geometry) else {
        return false;
    };
    if nurbs.boundary.parameter_range.endpoints() != domain {
        return false;
    }
    let shared = nodes[shared_nodes[0]];
    let control_points = curve.pole_rows().raw_points();
    let first_shared = point_distance(control_points[0], shared) <= tolerance;
    let last_shared = control_points
        .last()
        .is_some_and(|point| point_distance(*point, shared) <= tolerance);
    if first_shared == last_shared {
        return false;
    }
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let line_length = point_distance(start, end);
    if line_length <= tolerance {
        return false;
    }
    let side =
        |point: Point2| direction.u * (point.v - start.v) - direction.v * (point.u - start.u);
    let endpoint = if first_shared {
        0
    } else {
        control_points.len() - 1
    };
    let threshold = tolerance * line_length;
    if side(control_points[endpoint]).abs() > threshold {
        return false;
    }
    let mut signs = control_points
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != endpoint)
        .map(|(_, point)| side(*point));
    let Some(first_side) = signs.next() else {
        return false;
    };
    first_side.abs() > threshold
        && signs.all(|candidate| {
            candidate.abs() > threshold
                && candidate.is_sign_positive() == first_side.is_sign_positive()
        })
}

/// The overlap length of two line segments that lie on one line within
/// `tolerance`, or `None` when either segment is not a line or the lines
/// diverge by more than `tolerance`.
fn collinear_line_segments(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
    tolerance: f64,
) -> Option<f64> {
    let (
        ProfileBoundarySegment::Line { start: a, end: b },
        ProfileBoundarySegment::Line { start: c, end: d },
    ) = (left, right)
    else {
        return None;
    };
    let length = point_distance(*a, *b);
    if length <= tolerance {
        return None;
    }
    let direction = ((b.u - a.u) / length, (b.v - a.v) / length);
    let offset = |point: &Point2| {
        let (du, dv) = (point.u - a.u, point.v - a.v);
        (
            du * direction.0 + dv * direction.1,
            du * direction.1 - dv * direction.0,
        )
    };
    let (c_along, c_across) = offset(c);
    let (d_along, d_across) = offset(d);
    if c_across.abs() > tolerance || d_across.abs() > tolerance {
        return None;
    }
    Some(length.min(c_along.max(d_along)) - 0.0_f64.max(c_along.min(d_along)))
}

fn analytic_segment_intersections(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<[Option<Point2>; 2]> {
    match (left, right) {
        (
            ProfileBoundarySegment::Line { start: a, end: b },
            ProfileBoundarySegment::Line { start: c, end: d },
        ) => {
            let [parameter, other_parameter] =
                cadmpeg_ir::math::planar::line_line_parameters(*a, *b, *c, *d)?
                    .map(cadmpeg_ir::scalar::FiniteReal::get);
            if !(0.0..=1.0).contains(&parameter) || !(0.0..=1.0).contains(&other_parameter) {
                return Some([None; 2]);
            }
            Some([
                Some(Point2::new(
                    cadmpeg_ir::math::interpolate(a.u, b.u, parameter)?.get(),
                    cadmpeg_ir::math::interpolate(a.v, b.v, parameter)?.get(),
                )),
                None,
            ])
        }
        (ProfileBoundarySegment::Line { start, end }, arc @ ProfileBoundarySegment::Arc { .. })
        | (arc @ ProfileBoundarySegment::Arc { .. }, ProfileBoundarySegment::Line { start, end }) => {
            line_arc_intersection_points((*start, *end), arc)
        }
        (left @ ProfileBoundarySegment::Arc { .. }, right @ ProfileBoundarySegment::Arc { .. }) => {
            arc_intersection_points(left, right)
        }
    }
}

fn line_arc_intersection_points(
    (start, end): (Point2, Point2),
    arc: &ProfileBoundarySegment,
) -> Option<[Option<Point2>; 2]> {
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = arc
    else {
        return None;
    };
    let radius = radius.get();
    let offset = Point2::new(start.u - center.u, start.v - center.v);
    if start == end {
        return Some([
            (offset.u.hypot(offset.v) == radius
                && directed_angle_parameter(offset.v.atan2(offset.u), *start_angle, *end_angle)
                    .is_some())
            .then_some(start),
            None,
        ]);
    }
    let mut points = [None; 2];
    let mut point_count = 0;
    let Some(parameters) =
        cadmpeg_ir::math::planar::line_circle_intersections(start, end, *center, radius)
    else {
        return Some(points);
    };
    // A segment end that is not finite has no distance to measure.
    let segment =
        cadmpeg_ir::units::FinitePoint2::new(start).zip(cadmpeg_ir::units::FinitePoint2::new(end));
    for (_, finite_point) in parameters {
        let point = finite_point.get();
        if segment.is_some_and(|(start, end)| {
            cadmpeg_ir::math::planar::point_segment_distance(finite_point, start, end)
                <= 128.0 * f64::EPSILON * radius
        }) {
            let radial = (point.u - center.u).hypot(point.v - center.v);
            if (radial - radius).abs() <= 128.0 * f64::EPSILON * radius
                && directed_angle_parameter(
                    (point.v - center.v).atan2(point.u - center.u),
                    *start_angle,
                    *end_angle,
                )
                .is_some()
                && !points.contains(&Some(point))
            {
                points[point_count] = Some(point);
                point_count += 1;
            }
        }
    }
    Some(points)
}

fn arc_intersection_points(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<[Option<Point2>; 2]> {
    let ProfileBoundarySegment::Arc {
        center: lc,
        radius: lr,
        start_angle: ls,
        end_angle: le,
    } = left
    else {
        return None;
    };
    let ProfileBoundarySegment::Arc {
        center: rc,
        radius: rr,
        start_angle: rs,
        end_angle: re,
    } = right
    else {
        return None;
    };
    Some(
        cadmpeg_ir::math::planar::circle_intersections(*lc, lr.get(), *rc, rr.get())?.map(
            |point| {
                point
                    .map(cadmpeg_ir::units::FinitePoint2::get)
                    .filter(|point| {
                        directed_angle_parameter((point.v - lc.v).atan2(point.u - lc.u), *ls, *le)
                            .is_some()
                            && directed_angle_parameter(
                                (point.v - rc.v).atan2(point.u - rc.u),
                                *rs,
                                *re,
                            )
                            .is_some()
                    })
            },
        ),
    )
}

fn arrangement_split_parameters(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    range: [f64; 2],
    nodes: &[Point2],
    tolerance: f64,
) -> Result<Option<Vec<f64>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut parameters = Vec::new();
    parameters.extend(range);
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            if point_distance(start.get(), end.get()) <= tolerance {
                return Ok(None);
            }
            for point in nodes {
                let parameter = geometric!(cadmpeg_ir::math::planar::line_projection_parameter(
                    start.get(),
                    end.get(),
                    *point,
                ))
                .get();
                if parameter > 0.0
                    && parameter < 1.0
                    && point_segment_distance(*point, (start.get(), end.get())) <= tolerance
                {
                    parameters.push(parameter);
                }
            }
        }
        SketchGeometryDefinition::Arc { center, radius, .. } => {
            for point in nodes {
                if (point_distance(*point, center.get()) - radius.get()).abs() > tolerance {
                    continue;
                }
                let angle = (point.v - center.v).atan2(point.u - center.u);
                if let Some(parameter) = directed_angle_parameter(angle, range[0], range[1]) {
                    if parameter > 0.0 && parameter < 1.0 {
                        parameters.push(range[0] + parameter * (range[1] - range[0]));
                    }
                }
            }
        }
        _ => {}
    }
    parameters.sort_by(|left, right| {
        ((left - range[0]) / (range[1] - range[0]))
            .total_cmp(&((right - range[0]) / (range[1] - range[0])))
    });
    let parameter_tolerance =
        tolerance / geometric!(sketch_geometry_speed_bound(geometry, range)).max(tolerance);
    parameters.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    Ok((parameters.len() >= 2).then_some(parameters))
}

fn arrangement_circle_angles(
    nodes: &[Point2],
    center: Point2,
    radius: PositiveLength,
    tolerance: f64,
) -> Vec<f64> {
    let mut angles = Vec::new();
    for point in nodes
        .iter()
        .filter(|point| (point_distance(**point, center) - radius.get()).abs() <= tolerance)
    {
        let angle = (point.v - center.v)
            .atan2(point.u - center.u)
            .rem_euclid(std::f64::consts::TAU);
        angles.push(angle);
    }
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|left, right| (*left - *right).abs() <= tolerance / radius.get());
    angles
}

fn arrangement_node(nodes: &mut Vec<Point2>, point: Point2, tolerance: f64) -> usize {
    if let Some(index) = nodes
        .iter()
        .position(|candidate| point_distance(*candidate, point) <= tolerance)
    {
        return index;
    }
    nodes.push(point);
    nodes.len() - 1
}

/// The cycle-search work for `edge_count` edges over `node_count` nodes, or
/// `None` when the count leaves `usize`.
fn arrangement_cycle_work(edge_count: usize, node_count: usize) -> Option<usize> {
    edge_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(edge_count))
        .and_then(|count| {
            node_count
                .checked_add(1)
                .and_then(|nodes| count.checked_mul(nodes))
        })
}

/// Remove curve fragments that cannot bound a planar face.
///
/// A sketch can contain construction-like open branches in the solved entity
/// stream even when the source profile table is empty. Such branches are graph
/// bridges, not profile boundaries. Retaining only edges with an alternate
/// path between their endpoints leaves the bounded-face arrangement intact
/// while preserving intersections and nested loops.
fn arrangement_retain_cycle_edges(
    edges: &mut Vec<SketchArrangementEdge>,
    node_count: usize,
    budget: &WorkBudget,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    loop {
        // One BFS per edge scans all edges at each visited node: O(E² V).
        // The bound also admits visit initialization and retention scans.
        // A work count past `usize` exhausts the slice like any count above it.
        let charged = arrangement_cycle_work(edges.len(), node_count)
            .is_some_and(|work| budget.charge_by(work));
        if !charged {
            // An exhausted budget leaves the edges as they are; the caller
            // then projects no arrangement.
            budget.exhaust();
            return Ok(());
        }
        let mut keep = Vec::new();
        for (index, edge) in edges.iter().enumerate() {
            let has_cycle = arrangement_has_alternate_path(
                edges,
                index,
                edge.nodes[0],
                edge.nodes[1],
                node_count,
                ctx,
            )?;
            keep.push(has_cycle);
        }
        if keep.iter().all(|keep| *keep) {
            break;
        }
        let mut retained = Vec::new();
        for (edge, keep) in std::mem::take(edges).into_iter().zip(keep) {
            if keep {
                retained.push(edge);
            }
        }
        *edges = retained;
    }
    Ok(())
}

fn arrangement_has_alternate_path(
    edges: &[SketchArrangementEdge],
    excluded_edge: usize,
    start: usize,
    destination: usize,
    node_count: usize,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut visited = ctx.alloc_filled(node_count, false, "f3d arrangement visit marks")?;
    let mut pending = Vec::new();
    pending.push(start);
    visited[start] = true;
    while let Some(node) = pending.pop() {
        if node == destination {
            return Ok(true);
        }
        for (index, edge) in edges.iter().enumerate() {
            if index == excluded_edge {
                continue;
            }
            let Some(next) = (edge.nodes[0] == node)
                .then_some(edge.nodes[1])
                .or_else(|| (edge.nodes[1] == node).then_some(edge.nodes[0]))
            else {
                continue;
            };
            if !visited[next] {
                visited[next] = true;
                pending.push(next);
            }
        }
    }
    Ok(false)
}

fn arrangement_edges_coincident(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if left.boundary.entity == right.boundary.entity
        && left.boundary.parameter_range.endpoints() == right.boundary.parameter_range.endpoints()
    {
        return Ok(true);
    }
    let Some(left_entity) = entities
        .iter()
        .find(|entity| entity.id() == &left.boundary.entity)
    else {
        return Ok(false);
    };
    let Some(right_entity) = entities
        .iter()
        .find(|entity| entity.id() == &right.boundary.entity)
    else {
        return Ok(false);
    };
    Ok(
        match (
            left_entity.geometry.definition(),
            right_entity.geometry.definition(),
        ) {
            (SketchGeometryDefinition::Line { .. }, SketchGeometryDefinition::Line { .. }) => {
                let left_midpoint = sketch_geometry_point(
                    &left_entity.geometry,
                    (left.boundary.parameter_range.endpoints()[0]
                        + left.boundary.parameter_range.endpoints()[1])
                        * 0.5,
                    ctx,
                )?;
                left_midpoint
                    .zip(right.polyline.last().copied())
                    .is_some_and(|(point, end)| {
                        point_segment_distance(point, (right.polyline[0], end)) <= tolerance
                    })
            }
            (
                SketchGeometryDefinition::Circle {
                    center: left_center,
                    radius: left_radius,
                }
                | SketchGeometryDefinition::Arc {
                    center: left_center,
                    radius: left_radius,
                    ..
                },
                SketchGeometryDefinition::Circle {
                    center: right_center,
                    radius: right_radius,
                }
                | SketchGeometryDefinition::Arc {
                    center: right_center,
                    radius: right_radius,
                    ..
                },
            ) => {
                point_distance(left_center.get(), right_center.get()) <= tolerance
                    && (left_radius.get() - right_radius.get()).abs() <= tolerance
                    && ((left.boundary.parameter_range.endpoints()[1]
                        - left.boundary.parameter_range.endpoints()[0])
                        .abs()
                        - (right.boundary.parameter_range.endpoints()[1]
                            - right.boundary.parameter_range.endpoints()[0])
                            .abs())
                    .abs()
                        <= tolerance / left_radius.get()
                    && sketch_geometry_point(
                        &left_entity.geometry,
                        (left.boundary.parameter_range.endpoints()[0]
                            + left.boundary.parameter_range.endpoints()[1])
                            * 0.5,
                        ctx,
                    )?
                    .zip(sketch_geometry_point(
                        &right_entity.geometry,
                        (right.boundary.parameter_range.endpoints()[0]
                            + right.boundary.parameter_range.endpoints()[1])
                            * 0.5,
                        ctx,
                    )?)
                    .is_some_and(|(left, right)| point_distance(left, right) <= tolerance)
            }
            _ => false,
        },
    )
}

fn certified_pieces_bounds(pieces: &[CertifiedPiece]) -> Option<[Point2; 2]> {
    let mut bounds = pieces.iter().map(CertifiedPiece::bounds);
    let first = bounds.next()?;
    Some(bounds.fold(first, |[low, high], [piece_low, piece_high]| {
        [
            Point2::new(low.u.min(piece_low.u), low.v.min(piece_low.v)),
            Point2::new(high.u.max(piece_high.u), high.v.max(piece_high.v)),
        ]
    }))
}

/// Certify one arrangement edge in its direction: a line or arc exactly, or
/// a whole non-periodic NURBS as chord tubes.
fn arrangement_edge_pieces(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<CertifiedPiece>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let Some(segment) = arrangement_analytic_segment(edge, entities) {
        let piece = CertifiedPiece::Exact(segment);
        if edge.boundary.reversed && piece.is_arc() {
            return Ok(Some(vec![piece.reversed()]));
        }
        return Ok(Some(vec![piece]));
    }
    let entity = geometric!(entities
        .iter()
        .find(|entity| entity.id() == &edge.boundary.entity));
    let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() else {
        return Ok(None);
    };
    if curve.periodic()
        || sketch_geometry_parameter_range(&entity.geometry)
            != Some(edge.boundary.parameter_range.endpoints())
    {
        return Ok(None);
    }
    let scale = edge
        .polyline
        .iter()
        .flat_map(|point| [point.u.abs(), point.v.abs()])
        .fold(1.0_f64, f64::max);
    let target_error = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    let tubes = geometric!(certified_nurbs_tubes(curve, target_error, work, ctx)?);
    let pieces = tubes.into_iter().map(CertifiedPiece::Tube);
    if edge.boundary.reversed {
        return Ok(Some(pieces.rev().map(|piece| piece.reversed()).collect()));
    }
    Ok(Some(pieces.collect()))
}

fn arrangement_analytic_segment(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<ProfileBoundarySegment> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let entity = entities
        .iter()
        .find(|entity| entity.id() == &edge.boundary.entity)?;
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some(ProfileBoundarySegment::Line {
            start: edge.polyline[0],
            end: *edge.polyline.last()?,
        }),
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => {
            Some(ProfileBoundarySegment::Arc {
                center: center.get(),
                radius: *radius,
                start_angle: edge.boundary.parameter_range.endpoints()[0],
                end_angle: edge.boundary.parameter_range.endpoints()[1],
            })
        }
        _ => None,
    }
}

fn sketch_geometry_parameter_range(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<[f64; 2]> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some([0.0, 1.0]),
        SketchGeometryDefinition::Arc {
            start_angle,
            end_angle,
            ..
        } => Some([start_angle.get(), end_angle.get()]),
        SketchGeometryDefinition::Ellipse {
            bounds: Some([start, end]),
            ..
        } => Some([start.get(), end.get()]),
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => Some([
            curve.knots()[index_from_u32(curve.degree())],
            curve.knots()[curve.pole_rows().count()],
        ]),
        _ => None,
    }
}

fn profile_use_polyline(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    range: [f64; 2],
    reversed: bool,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point2>>, CodecError> {
    let travel = geometric!(sketch_geometry_speed_bound(&entity.geometry, range))
        * (range[1] - range[0]).abs();
    let ordinary_midpoint = (range[0] + range[1]) * 0.5;
    let midpoint = if ordinary_midpoint.is_finite() {
        ordinary_midpoint
    } else {
        geometric!(cadmpeg_ir::math::interpolate(range[0], range[1], 0.5)).get()
    };
    let scale = [
        geometric!(sketch_geometry_point(&entity.geometry, range[0], ctx)?),
        geometric!(sketch_geometry_point(&entity.geometry, range[1], ctx)?),
        geometric!(sketch_geometry_point(&entity.geometry, midpoint, ctx)?),
    ]
    .into_iter()
    .flat_map(|point| [point.u.abs(), point.v.abs()])
    .fold(1.0_f64, f64::max);
    let target = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    // This bounded polyline supplies tangent order, winding sign, and
    // intersection witnesses only. Exact output retains source parameter
    // intervals rather than this derived representation.
    let count = if travel.is_finite() {
        geometric!(truncate_f64_to_usize(
            (travel / target).ceil().clamp(2.0, 256.0)
        ))
    } else {
        256
    };
    let mut points = Vec::new();
    for index in 0..=count {
        let fraction = geometric!(f64_from_index(index)) / geometric!(f64_from_index(count));
        let ordinary = range[0] + (range[1] - range[0]) * fraction;
        let parameter = if ordinary.is_finite() {
            ordinary
        } else {
            geometric!(cadmpeg_ir::math::interpolate(range[0], range[1], fraction)).get()
        };
        let point = geometric!(sketch_geometry_point(&entity.geometry, parameter, ctx)?);
        points.push(point);
    }
    if reversed {
        points.reverse();
    }
    Ok(Some(points))
}

fn sketch_geometry_speed_bound(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    range: [f64; 2],
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            Some(point_distance(start.get(), end.get()))
        }
        SketchGeometryDefinition::Circle { radius, .. }
        | SketchGeometryDefinition::Arc { radius, .. } => Some(radius.get()),
        SketchGeometryDefinition::Ellipse { radii, .. } => {
            Some(radii.major().get().max(radii.minor().get()))
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => nurbs_speed_bound(curve),
        _ if range[0] == range[1] => None,
        _ => None,
    }
}

pub(super) fn sketch_geometry_point(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    parameter: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Point2>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    Ok(match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some(Point2::new(
            start.u + parameter * (end.u - start.u),
            start.v + parameter * (end.v - start.v),
        )),
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => Some(Point2::new(
            center.u + radius.get() * parameter.cos(),
            center.v + radius.get() * parameter.sin(),
        )),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            ..
        } => {
            let (axis_sine, axis_cosine) = major_angle.get().sin_cos();
            Some(Point2::new(
                center.u + radii.major().get() * parameter.cos() * axis_cosine
                    - radii.minor().get() * parameter.sin() * axis_sine,
                center.v
                    + radii.major().get() * parameter.cos() * axis_sine
                    + radii.minor().get() * parameter.sin() * axis_cosine,
            ))
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let lanes = PcurveEvaluatorLanes::new(curve.pole_rows());
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                curve.degree(),
                curve.knots(),
                lanes.points(),
                lanes.weights(),
                parameter,
            ))?
            .map(Point2::from)
        }
        _ => None,
    })
}

fn point_on_profile_boundary_use(
    point: Point2,
    use_: &cadmpeg_ir::features::SketchProfileBoundaryUse,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity) else {
        return false;
    };
    if !point_on_sketch_entity(point, entity, tolerance) {
        return false;
    }
    match entity.geometry.definition() {
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => {
            let angle = (point.v - center.v).atan2(point.u - center.u);
            directed_angle_parameter(
                angle,
                use_.parameter_range.endpoints()[0],
                use_.parameter_range.endpoints()[1],
            )
            .is_some_and(|parameter| {
                parameter >= -tolerance / radius.get()
                    && parameter <= 1.0 + tolerance / radius.get()
            })
        }
        _ => true,
    }
}

/// The exact carrier one profile boundary use traces over its parameter
/// interval, in the interval's stored direction.
pub(super) enum BoundaryCarrier {
    Line {
        start: Point2,
        end: Point2,
    },
    /// From `start` radians through `sweep` radians about `center`; a
    /// negative sweep turns clockwise.
    Arc {
        center: Point2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

/// `count + 1` points evenly spaced in parameter along `use_`, from its
/// first end to its last. `None` when the curve cannot be evaluated there.
pub(super) fn boundary_use_samples(
    use_: &cadmpeg_ir::features::SketchProfileBoundaryUse,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    count: u32,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point2>>, CodecError> {
    let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
    let [from, to] = use_.parameter_range.endpoints();
    let mut points = Vec::new();
    for k in 0..=count {
        let parameter = from + (to - from) * f64::from(k) / f64::from(count);
        points.push(geometric!(sketch_geometry_point(
            &entity.geometry,
            parameter,
            ctx
        )?));
    }
    Ok(Some(points))
}

/// The line or circular arc `use_` traces. `None` for any other curve.
pub(super) fn boundary_use_carrier(
    use_: &cadmpeg_ir::features::SketchProfileBoundaryUse,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<BoundaryCarrier> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let entity = entities.iter().find(|entity| entity.id() == &use_.entity)?;
    let [from, to] = use_.parameter_range.endpoints();
    match *entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let (start, end) = (start.get(), end.get());
            let at = |parameter: f64| {
                Point2::new(
                    start.u + parameter * (end.u - start.u),
                    start.v + parameter * (end.v - start.v),
                )
            };
            Some(BoundaryCarrier::Line {
                start: at(from),
                end: at(to),
            })
        }
        SketchGeometryDefinition::Circle { center, radius }
        | SketchGeometryDefinition::Arc { center, radius, .. } => Some(BoundaryCarrier::Arc {
            center: center.get(),
            radius: radius.get(),
            start: from,
            sweep: to - from,
        }),
        _ => None,
    }
}

fn signed_polygon_area(vertices: &[Point2]) -> f64 {
    cadmpeg_ir::math::planar::polygon_area_twice(vertices)
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get)
        * 0.5
}

pub(super) fn region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::SketchProfileRegion>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let mut boundaries = Vec::new();
    for profile in &sketch.profiles {
        let Some(boundary) = profile_boundary(profile, entities, tolerance, work, ctx)? else {
            return Ok(None);
        };
        boundaries.push(boundary);
    }
    let mut containment = Vec::new();
    for (outer_index, outer) in boundaries.iter().enumerate() {
        let mut row = Vec::new();
        for (inner_index, inner) in boundaries.iter().enumerate() {
            let contains = outer_index != inner_index && outer.strictly_contains(inner);
            row.push(contains);
        }
        containment.push(row);
    }
    let mut projected = Vec::new();
    for point in points {
        let projected_point = geometric!(project_to_sketch(sketch, *point));
        projected.push(projected_point);
    }
    let mut incidences = Vec::new();
    for point in &projected {
        let mut incident = HashSet::new();
        for (index, profile) in sketch.profiles.iter().enumerate() {
            for use_ in profile {
                let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity)
                else {
                    continue;
                };
                if point_on_sketch_entity(*point, entity, tolerance) {
                    incident.insert(index);
                    break;
                }
            }
        }
        incidences.push(incident);
    }
    let region = |outer: usize| -> Result<Option<(usize, Vec<usize>)>, CodecError> {
        let holes = immediate_containment_children(outer, &containment);
        Ok(projected
            .iter()
            .zip(&incidences)
            .all(|(point, incident)| {
                incident.contains(&outer)
                    || holes.iter().any(|hole| incident.contains(hole))
                    || incident.is_empty()
                        && boundaries[outer].contains_point(*point)
                        && holes
                            .iter()
                            .all(|hole| !boundaries[*hole].contains_point(*point))
            })
            .then_some((outer, holes)))
    };
    let mut closure_matches = Vec::new();
    for outer in 0..boundaries.len() {
        if let Some(candidate) = region(outer)? {
            closure_matches.push(candidate);
        }
    }
    if let [(outer, holes)] = closure_matches.as_slice() {
        let mut converted_holes = Vec::new();
        for hole in holes {
            let hole = geometric!(u32::try_from(*hole).ok());
            converted_holes.push(hole);
        }
        return Ok(SketchProfileRegion::loops(
            geometric!(u32::try_from(*outer).ok()),
            converted_holes,
        )
        .ok());
    }
    if incidences.iter().any(|incident| !incident.is_empty()) {
        return Ok(None);
    }
    let mut containing = Vec::new();
    for (index, boundary) in boundaries.iter().enumerate() {
        if projected
            .iter()
            .all(|point| boundary.contains_point(*point))
        {
            containing.push(index);
        }
    }
    if containing.iter().enumerate().any(|(left_index, left)| {
        containing
            .iter()
            .skip(left_index + 1)
            .any(|right| !containment[*left][*right] && !containment[*right][*left])
    }) {
        return Ok(None);
    }
    let &outer = geometric!(containing.iter().find(|candidate| {
        containing
            .iter()
            .all(|other| other == *candidate || containment[*other][**candidate])
    }));
    let holes = immediate_containment_children(outer, &containment);
    let mut converted_holes = Vec::new();
    for hole in holes {
        let hole = geometric!(u32::try_from(hole).ok());
        converted_holes.push(hole);
    }
    Ok(SketchProfileRegion::loops(geometric!(u32::try_from(outer).ok()), converted_holes).ok())
}

/// Return true when every selected closed profile bounds a disjoint region.
/// Nested or intersecting loops require explicit region semantics.
pub(super) fn profile_loops_are_independent(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    profiles: &[u32],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut boundaries = Vec::new();
    for profile in profiles {
        let Ok(index) = usize::try_from(*profile) else {
            return Ok(false);
        };
        let Some(profile) = sketch.profiles.get(index) else {
            return Ok(false);
        };
        let Some(boundary) = profile_boundary(profile, entities, tolerance, work, ctx)? else {
            return Ok(false);
        };
        boundaries.push(boundary);
    }
    for (left_index, left) in boundaries.iter().enumerate() {
        for right in boundaries.iter().skip(left_index + 1) {
            if !left.is_provably_disjoint(right) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn immediate_containment_children(outer: usize, containment: &[Vec<bool>]) -> Vec<usize> {
    let mut children = Vec::new();
    for candidate in 0..containment.len() {
        if candidate != outer
            && containment[outer][candidate]
            && !(0..containment.len()).any(|intermediate| {
                intermediate != outer
                    && intermediate != candidate
                    && containment[outer][intermediate]
                    && containment[intermediate][candidate]
            })
        {
            children.push(candidate);
        }
    }
    children
}

enum ProfileBoundary {
    Polygon(Vec<Point2>),
    CircularArcLoop(Vec<ProfileBoundarySegment>),
    Circle {
        center: Point2,
        radius: PositiveLength,
    },
    CertifiedLoop(CertifiedProfileLoop),
}

#[derive(Clone)]
struct CertifiedProfileLoop {
    pieces: Vec<CertifiedPiece>,
}

/// One piece of a certified boundary curve.
#[derive(Clone, Copy)]
enum CertifiedPiece {
    /// A sketch line or circular arc, which the tests use exactly.
    Exact(ProfileBoundarySegment),
    /// A chord within `error` of one NURBS span.
    Tube(CertifiedCurveTube),
}

#[derive(Clone, Copy)]
struct CertifiedCurveTube {
    start: Point2,
    end: Point2,
    error: f64,
}

#[derive(Clone, Copy)]
enum ProfileBoundarySegment {
    Line {
        start: Point2,
        end: Point2,
    },
    Arc {
        center: Point2,
        radius: PositiveLength,
        start_angle: f64,
        end_angle: f64,
    },
}

impl ProfileBoundary {
    fn contains_point(&self, point: Point2) -> bool {
        match self {
            Self::Polygon(vertices) => point_in_polygon(point, vertices),
            Self::CircularArcLoop(segments) => point_in_circular_arc_loop(point, segments),
            Self::Circle { center, radius } => point_distance(*center, point) < radius.get(),
            Self::CertifiedLoop(loop_) => loop_.contains_point(point),
        }
    }

    fn strictly_contains(&self, inner: &Self) -> bool {
        match (self, inner) {
            (Self::Polygon(outer), Self::Polygon(inner)) => polygon_strictly_contains(outer, inner),
            (
                Self::Circle {
                    center: outer_center,
                    radius: outer_radius,
                },
                Self::Circle {
                    center: inner_center,
                    radius: inner_radius,
                },
            ) => {
                point_distance(*outer_center, *inner_center) + inner_radius.get()
                    < outer_radius.get()
            }
            (Self::Polygon(outer), Self::Circle { center, radius }) => {
                point_in_polygon(*center, outer)
                    && polygon_edges(outer)
                        .all(|edge| point_segment_distance(*center, edge) > radius.get())
            }
            (Self::CircularArcLoop(outer), Self::Circle { center, radius }) => {
                point_in_circular_arc_loop(*center, outer)
                    && outer.iter().all(|segment| {
                        point_boundary_segment_distance(*center, segment) > radius.get()
                    })
            }
            (Self::Circle { center, radius }, Self::Polygon(inner)) => inner
                .iter()
                .all(|point| point_distance(*center, *point) < radius.get()),
            (Self::Circle { center, radius }, Self::CircularArcLoop(inner)) => inner
                .iter()
                .all(|segment| boundary_segment_max_distance(*center, segment) < radius.get()),
            (Self::Polygon(outer), Self::CircularArcLoop(inner)) => {
                !polygon_arc_loop_intersects(outer, inner)
                    && inner.first().is_some_and(|segment| {
                        point_in_polygon(boundary_segment_endpoints(segment).0, outer)
                    })
            }
            (Self::CircularArcLoop(outer), Self::Polygon(inner)) => {
                !polygon_arc_loop_intersects(inner, outer)
                    && inner
                        .first()
                        .is_some_and(|point| point_in_circular_arc_loop(*point, outer))
            }
            (Self::CircularArcLoop(outer), Self::CircularArcLoop(inner)) => {
                !arc_loops_intersect(outer, inner)
                    && inner.first().is_some_and(|segment| {
                        point_in_circular_arc_loop(boundary_segment_endpoints(segment).0, outer)
                    })
            }
            (outer, inner) => {
                let Some(outer) = outer.certified_loop() else {
                    return false;
                };
                let Some(inner) = inner.certified_loop() else {
                    return false;
                };
                outer.strictly_contains(&inner)
            }
        }
    }

    fn is_provably_disjoint(&self, other: &Self) -> bool {
        let intersects = match (self, other) {
            (Self::Polygon(left), Self::Polygon(right)) => polygon_edges(left).any(|left_edge| {
                polygon_edges(right).any(|right_edge| segments_intersect(left_edge, right_edge))
            }),
            (
                Self::Circle {
                    center: left_center,
                    radius: left_radius,
                },
                Self::Circle {
                    center: right_center,
                    radius: right_radius,
                },
            ) => {
                point_distance(*left_center, *right_center)
                    <= left_radius.get() + right_radius.get()
            }
            (Self::Polygon(polygon), Self::Circle { center, radius })
            | (Self::Circle { center, radius }, Self::Polygon(polygon)) => polygon_edges(polygon)
                .any(|edge| point_segment_distance(*center, edge) <= radius.get()),
            (Self::Polygon(polygon), Self::CircularArcLoop(arc_loop))
            | (Self::CircularArcLoop(arc_loop), Self::Polygon(polygon)) => {
                polygon_arc_loop_intersects(polygon, arc_loop)
            }
            (Self::CircularArcLoop(left), Self::CircularArcLoop(right)) => {
                arc_loops_intersect(left, right)
            }
            (Self::CircularArcLoop(arc_loop), Self::Circle { center, radius })
            | (Self::Circle { center, radius }, Self::CircularArcLoop(arc_loop)) => arc_loop
                .iter()
                .any(|segment| point_boundary_segment_distance(*center, segment) <= radius.get()),
            (Self::CertifiedLoop(_), _) | (_, Self::CertifiedLoop(_)) => return false,
        };
        !intersects && !self.strictly_contains(other) && !other.strictly_contains(self)
    }

    fn certified_loop(&self) -> Option<std::borrow::Cow<'_, CertifiedProfileLoop>> {
        use std::borrow::Cow;

        match self {
            Self::Polygon(vertices) => {
                CertifiedProfileLoop::from_vertices(vertices).map(Cow::Owned)
            }
            Self::CircularArcLoop(segments) => {
                CertifiedProfileLoop::from_segments(segments.iter().copied()).map(Cow::Owned)
            }
            Self::Circle { center, radius } => {
                CertifiedProfileLoop::from_segments([ProfileBoundarySegment::Arc {
                    center: *center,
                    radius: *radius,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::TAU,
                }])
                .map(Cow::Owned)
            }
            Self::CertifiedLoop(loop_) => Some(Cow::Borrowed(loop_)),
        }
    }
}

impl CertifiedProfileLoop {
    /// Fewer than three chords bound no area; one arc can close a loop.
    fn new(pieces: Vec<CertifiedPiece>) -> Option<Self> {
        (pieces.len() >= 3 || pieces.iter().any(CertifiedPiece::is_arc)).then_some(Self { pieces })
    }

    fn from_vertices(vertices: &[Point2]) -> Option<Self> {
        Self::from_segments(
            polygon_edges(vertices).map(|(start, end)| ProfileBoundarySegment::Line { start, end }),
        )
    }

    fn from_segments(segments: impl IntoIterator<Item = ProfileBoundarySegment>) -> Option<Self> {
        Self::new(segments.into_iter().map(CertifiedPiece::Exact).collect())
    }

    /// A point outside every tube has the winding number of the chord and
    /// exact-segment loop, because each span deforms to its chord inside its
    /// own tube without crossing the point.
    fn contains_point(&self, point: Point2) -> bool {
        self.pieces.iter().all(|piece| piece.clears_point(point))
            && point_in_segment_loop(point, self.pieces.iter().map(CertifiedPiece::segment))
    }

    fn strictly_contains(&self, inner: &Self) -> bool {
        certified_pieces_separated(&self.pieces, &inner.pieces)
            && inner
                .pieces
                .first()
                .is_some_and(|piece| self.contains_point(piece.start()))
    }
}

impl CertifiedPiece {
    fn is_arc(&self) -> bool {
        matches!(self, Self::Exact(ProfileBoundarySegment::Arc { .. }))
    }

    /// The exact segment, or the chord of a tube.
    fn segment(&self) -> ProfileBoundarySegment {
        match *self {
            Self::Exact(segment) => segment,
            Self::Tube(tube) => ProfileBoundarySegment::Line {
                start: tube.start,
                end: tube.end,
            },
        }
    }

    fn start(&self) -> Point2 {
        boundary_segment_endpoints(&self.segment()).0
    }

    fn end(&self) -> Point2 {
        boundary_segment_endpoints(&self.segment()).1
    }

    /// A box around the piece: an arc's whole circle, or a tube's chord
    /// widened by its error.
    fn bounds(&self) -> [Point2; 2] {
        let (first, second, margin) = match *self {
            Self::Exact(ProfileBoundarySegment::Line { start, end }) => (start, end, 0.0),
            Self::Exact(ProfileBoundarySegment::Arc { center, radius, .. }) => {
                (center, center, radius.get())
            }
            Self::Tube(tube) => (tube.start, tube.end, tube.error),
        };
        [
            Point2::new(
                first.u.min(second.u) - margin,
                first.v.min(second.v) - margin,
            ),
            Point2::new(
                first.u.max(second.u) + margin,
                first.v.max(second.v) + margin,
            ),
        ]
    }

    /// Whether `point` is off the piece, or outside the tube of a NURBS span.
    fn clears_point(&self, point: Point2) -> bool {
        self.clears_point_by(point, 0.0)
    }

    /// Whether `point` lies farther than `margin` from the piece, or from the
    /// tube of a NURBS span.
    fn clears_point_by(&self, point: Point2, margin: f64) -> bool {
        match self {
            Self::Exact(segment) => point_boundary_segment_distance(point, segment) > margin,
            Self::Tube(tube) => {
                point_segment_distance(point, (tube.start, tube.end)) > tube.error + margin
            }
        }
    }

    /// The piece traversed from its end to its start.
    fn reversed(&self) -> Self {
        match *self {
            Self::Exact(ProfileBoundarySegment::Line { start, end }) => {
                Self::Exact(ProfileBoundarySegment::Line {
                    start: end,
                    end: start,
                })
            }
            Self::Exact(ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => Self::Exact(ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle: end_angle,
                end_angle: start_angle,
            }),
            Self::Tube(tube) => Self::Tube(CertifiedCurveTube {
                start: tube.end,
                end: tube.start,
                error: tube.error,
            }),
        }
    }

    /// Whether the two pieces provably have no common point.
    fn separated_from(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Exact(left), Self::Exact(right)) => !boundary_segments_intersect(left, right),
            (Self::Exact(segment), Self::Tube(tube)) | (Self::Tube(tube), Self::Exact(segment)) => {
                chord_boundary_segment_distance((tube.start, tube.end), segment) > tube.error
            }
            (Self::Tube(left), Self::Tube(right)) => {
                segment_distance((left.start, left.end), (right.start, right.end))
                    > left.error + right.error
            }
        }
    }
}

fn certified_pieces_separated(left: &[CertifiedPiece], right: &[CertifiedPiece]) -> bool {
    left.iter()
        .all(|left| right.iter().all(|right| left.separated_from(right)))
}

/// Whether each of `points` lies inside the closed profile loop `profile`;
/// `None` when the loop's boundary is not resolved.
pub(super) fn profile_loop_contains_points(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point2],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<bool>>, CodecError> {
    let boundary = geometric!(profile_boundary(profile, entities, tolerance, work, ctx)?);
    Ok(Some(
        points
            .iter()
            .map(|point| boundary.contains_point(*point))
            .collect(),
    ))
}

fn profile_boundary(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ProfileBoundary>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let [use_] = profile {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        if let SketchGeometryDefinition::Circle { center, radius } = *entity.geometry.definition() {
            return Ok(Some(ProfileBoundary::Circle {
                center: center.get(),
                radius,
            }));
        }
    }
    if let Some(polygon) = line_profile_vertices(profile, entities, tolerance) {
        return Ok(Some(ProfileBoundary::Polygon(polygon)));
    }
    if let Some(arc_loop) = circular_arc_profile_segments(profile, entities, tolerance) {
        return Ok(Some(ProfileBoundary::CircularArcLoop(arc_loop)));
    }
    Ok(
        certified_profile_loop(profile, entities, tolerance, work, ctx)?
            .map(ProfileBoundary::CertifiedLoop),
    )
}

fn certified_profile_loop(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CertifiedProfileLoop>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut scale = 1.0_f64;
    for entity in entities {
        if let Some(ends) = sketch_entity_endpoints(entity, ctx)? {
            for point in ends {
                scale = scale.max(point.u.abs()).max(point.v.abs());
            }
        }
    }
    // The tube need only separate the selected point and peer boundaries; it
    // is not a geometric approximation exposed by the codec.  A square-root
    // scale keeps the conservative tube practical while exact boundary tests
    // continue to govern the source linear tolerance.
    let target_error = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    let mut pieces = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = geometric!(entities.iter().find(|entity| entity.id() == &use_.entity));
        let entity_pieces = if let Some(segment) = profile_use_segment(entity, use_.reversed) {
            vec![CertifiedPiece::Exact(segment)]
        } else {
            let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() else {
                return Ok(None);
            };
            if curve.periodic() {
                return Ok(None);
            }
            let mut tubes = geometric!(certified_nurbs_tubes(curve, target_error, work, ctx)?);
            if use_.reversed {
                tubes.reverse();
                for tube in &mut tubes {
                    std::mem::swap(&mut tube.start, &mut tube.end);
                }
            }
            tubes.into_iter().map(CertifiedPiece::Tube).collect()
        };
        let first = geometric!(entity_pieces.first()).start();
        if previous_end.is_some_and(|end| point_distance(end, first) > tolerance) {
            return Ok(None);
        }
        previous_end = entity_pieces.last().map(CertifiedPiece::end);
        pieces.extend(entity_pieces);
    }
    if previous_end.is_none_or(|end| {
        pieces
            .first()
            .is_none_or(|first| point_distance(end, first.start()) > tolerance)
    }) {
        return Ok(None);
    }
    Ok(CertifiedProfileLoop::new(pieces))
}

fn certified_nurbs_tubes(
    curve: &PcurveNurbs,
    target_error: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<CertifiedCurveTube>>, CodecError> {
    let speed = geometric!(nurbs_speed_bound(curve));
    let degree = index_from_u32(curve.degree());
    let knots = curve.knots();
    let lanes = PcurveEvaluatorLanes::new(curve.pole_rows());
    let count = lanes.points().len();
    let mut tubes = Vec::new();
    for span in knots[degree..=count].windows(2) {
        if span[0] == span[1] {
            continue;
        }
        let width = span[1] - span[0];
        let travel_bound = if width.is_finite() {
            speed * width
        } else if speed <= 1.0 {
            speed * span[1] - speed * span[0]
        } else {
            return Ok(None);
        };
        let subdivisions = geometric!(subdivision_count(travel_bound, target_error, work));
        let subdivisions_float = geometric!(f64_from_index(subdivisions));
        let error = travel_bound / subdivisions_float;
        for index in 0..subdivisions {
            let parameter = |ordinal: usize| {
                let ordinal = f64_from_index(ordinal)?;
                let fraction = ordinal / subdivisions_float;
                if width.is_finite() {
                    Some(span[0] + width * ordinal / subdivisions_float)
                } else {
                    Some(span[0].mul_add(1.0 - fraction, span[1] * fraction))
                }
            };
            let start = *geometric!(cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::nurbs_pcurve_uv(
                    ctx,
                    curve.degree(),
                    knots,
                    lanes.points(),
                    lanes.weights(),
                    geometric!(parameter(index)),
                )
            )?)
            .as_raw();
            let end = *geometric!(cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::nurbs_pcurve_uv(
                    ctx,
                    curve.degree(),
                    knots,
                    lanes.points(),
                    lanes.weights(),
                    geometric!(parameter(index + 1)),
                )
            )?)
            .as_raw();
            tubes.push(CertifiedCurveTube { start, end, error });
        }
    }
    Ok((!tubes.is_empty()).then_some(tubes))
}

fn subdivision_count(
    travel_bound: f64,
    target_error: f64,
    work: &ProfileGeometryWork,
) -> Option<usize> {
    if !travel_bound.is_finite()
        || travel_bound < 0.0
        || !target_error.is_finite()
        || target_error <= 0.0
    {
        return None;
    }
    let count = (travel_bound / target_error).ceil().max(1.0);
    if count > MAX_SUBDIVISIONS {
        work.densification_skipped.set(true);
        return None;
    }
    truncate_f64_to_usize(count)
}

fn nurbs_speed_bound(curve: &PcurveNurbs) -> Option<f64> {
    cadmpeg_ir::geometry::nurbs::bounds::speed_bound_by(
        curve.degree(),
        curve.knots(),
        curve.pole_rows().count(),
        |index| {
            curve.pole_rows().point_at(index).map(|point| {
                let point = point.get();
                [point.u, point.v]
            })
        },
        |index| curve.pole_rows().weight_at(index).unwrap_or(1.0),
        [0.0, 0.0],
    )
    .map(cadmpeg_ir::scalar::FiniteReal::get)
}

fn circular_arc_profile_segments(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<ProfileBoundarySegment>> {
    let mut segments = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = entities.iter().find(|entity| entity.id() == &use_.entity)?;
        let segment = profile_use_segment(entity, use_.reversed)?;
        let (start, end) = boundary_segment_endpoints(&segment);
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return None;
        }
        previous_end = Some(end);
        segments.push(segment);
    }
    let first_start = segments.first().map(boundary_segment_endpoints)?.0;
    (segments.len() >= 2
        && previous_end.is_some_and(|end| point_distance(end, first_start) <= tolerance))
    .then_some(segments)
}

/// The sketch line or circular arc of one profile use, in use direction.
fn profile_use_segment(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    reversed: bool,
) -> Option<ProfileBoundarySegment> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match *entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let [start, end] = if reversed { [end, start] } else { [start, end] };
            Some(ProfileBoundarySegment::Line {
                start: start.get(),
                end: end.get(),
            })
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let (start_angle, end_angle) = if reversed {
                (end_angle.get(), start_angle.get())
            } else {
                (start_angle.get(), end_angle.get())
            };
            Some(ProfileBoundarySegment::Arc {
                center: center.get(),
                radius,
                start_angle,
                end_angle,
            })
        }
        _ => None,
    }
}

fn line_profile_vertices(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<Point2>> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut vertices = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = entities.iter().find(|entity| entity.id() == &use_.entity)?;
        let SketchGeometryDefinition::Line { start, end } = *entity.geometry.definition() else {
            return None;
        };
        let (start, end) = (start.get(), end.get());
        let [start, end] = if use_.reversed {
            [end, start]
        } else {
            [start, end]
        };
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return None;
        }
        vertices.push(start);
        previous_end = Some(end);
    }
    if vertices.len() < 3
        || previous_end.is_none_or(|end| point_distance(end, vertices[0]) > tolerance)
    {
        return None;
    }
    Some(vertices)
}

fn boundary_segment_endpoints(segment: &ProfileBoundarySegment) -> (Point2, Point2) {
    match segment {
        ProfileBoundarySegment::Line { start, end } => (*start, *end),
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radius = radius.get();
            (
                Point2::new(
                    center.u + radius * start_angle.cos(),
                    center.v + radius * start_angle.sin(),
                ),
                Point2::new(
                    center.u + radius * end_angle.cos(),
                    center.v + radius * end_angle.sin(),
                ),
            )
        }
    }
}

fn point_in_circular_arc_loop(point: Point2, segments: &[ProfileBoundarySegment]) -> bool {
    point_in_segment_loop(point, segments.iter().copied())
}

fn point_in_segment_loop(
    point: Point2,
    segments: impl Iterator<Item = ProfileBoundarySegment>,
) -> bool {
    segments
        .map(|segment| match segment {
            ProfileBoundarySegment::Line { start, end } => {
                let crosses_up = start.v <= point.v && point.v < end.v;
                let crosses_down = end.v <= point.v && point.v < start.v;
                if !(crosses_up || crosses_down)
                    || point.u
                        >= start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
                {
                    0
                } else if crosses_up {
                    1
                } else {
                    -1
                }
            }
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => horizontal_ray_arc_winding(point, center, radius.get(), start_angle, end_angle),
        })
        .sum::<i32>()
        != 0
}

fn horizontal_ray_arc_winding(
    point: Point2,
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> i32 {
    let ordinate = (point.v - center.v) / radius;
    if ordinate.abs() > 1.0 {
        return 0;
    }
    let principal = ordinate.asin();
    let sweep = end_angle - start_angle;
    let angles = if principal.cos().abs() <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
        [Some(principal), None]
    } else {
        [Some(principal), Some(std::f64::consts::PI - principal)]
    };
    angles
        .into_iter()
        .flatten()
        .filter(|angle| center.u + radius * angle.cos() > point.u)
        .filter_map(|angle| {
            let parameter = directed_angle_parameter(angle, start_angle, end_angle)?;
            let derivative = radius * angle.cos() * sweep;
            let second_derivative = -radius * angle.sin() * sweep * sweep;
            let direction = if parameter <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
                && derivative.abs() <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
            {
                second_derivative
            } else if parameter >= 1.0 - EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
                && derivative.abs() <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12
            {
                -second_derivative
            } else {
                derivative
            };
            if parameter <= EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                (direction > 0.0).then_some(1)
            } else if parameter >= 1.0 - EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                (direction < 0.0).then_some(-1)
            } else if direction > EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                Some(1)
            } else if direction < -EPS_GEOMETRY_HORIZONTAL_RAY_ARC_WINDING_E12 {
                Some(-1)
            } else {
                None
            }
        })
        .sum()
}

fn directed_angle_parameter(angle: f64, start: f64, end: f64) -> Option<f64> {
    let sweep = end - start;
    if sweep == 0.0 || sweep.abs() > std::f64::consts::TAU {
        return None;
    }
    let displacement = if sweep > 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        -(start - angle).rem_euclid(std::f64::consts::TAU)
    };
    (displacement.abs() <= sweep.abs()).then_some(displacement / sweep)
}

fn point_boundary_segment_distance(point: Point2, segment: &ProfileBoundarySegment) -> f64 {
    match segment {
        ProfileBoundarySegment::Line { start, end } => {
            point_segment_distance(point, (*start, *end))
        }
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radius = radius.get();
            let endpoint_distance = [*start_angle, *end_angle]
                .into_iter()
                .map(|angle| {
                    point_distance(
                        point,
                        Point2::new(
                            center.u + radius * angle.cos(),
                            center.v + radius * angle.sin(),
                        ),
                    )
                })
                .fold(f64::INFINITY, f64::min);
            let angle = (point.v - center.v).atan2(point.u - center.u);
            if directed_angle_parameter(angle, *start_angle, *end_angle).is_some() {
                endpoint_distance.min((point_distance(point, *center) - radius).abs())
            } else {
                endpoint_distance
            }
        }
    }
}

/// Answer the distance from the segment `chord` to a sketch line or arc.
///
/// A chord that does not meet the arc is nearest to it at an end of either
/// curve, or at the arc point nearest the chord's supporting line: an interior
/// pair is a local minimum only where the joining segment is normal to both
/// curves, which puts the arc point on the line's perpendicular through the
/// center, on the side toward the line.
fn chord_boundary_segment_distance(
    chord: (Point2, Point2),
    segment: &ProfileBoundarySegment,
) -> f64 {
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = *segment
    else {
        let (start, end) = boundary_segment_endpoints(segment);
        return segment_distance(chord, (start, end));
    };
    // A chord wholly outside or wholly inside the circle cannot meet the arc;
    // only a chord across it needs the exact intersection test.
    let farthest = point_distance(center, chord.0).max(point_distance(center, chord.1));
    if point_segment_distance(center, chord) <= radius.get()
        && radius.get() <= farthest
        && line_arc_intersection_points(chord, segment)
            .is_some_and(|points| points.iter().any(Option::is_some))
    {
        return 0.0;
    }
    let (arc_start, arc_end) = boundary_segment_endpoints(segment);
    let mut distance = [
        point_boundary_segment_distance(chord.0, segment),
        point_boundary_segment_distance(chord.1, segment),
        point_segment_distance(arc_start, chord),
        point_segment_distance(arc_end, chord),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min);
    let foot = cadmpeg_ir::math::planar::line_projection_parameter(chord.0, chord.1, center)
        .and_then(|parameter| {
            Some(Point2::new(
                cadmpeg_ir::math::interpolate(chord.0.u, chord.1.u, parameter.get())?.get(),
                cadmpeg_ir::math::interpolate(chord.0.v, chord.1.v, parameter.get())?.get(),
            ))
        });
    if let Some(foot) = foot {
        let angle = (foot.v - center.v).atan2(foot.u - center.u);
        if directed_angle_parameter(angle, start_angle, end_angle).is_some() {
            let nearest = Point2::new(
                center.u + radius.get() * angle.cos(),
                center.v + radius.get() * angle.sin(),
            );
            distance = distance.min(point_segment_distance(nearest, chord));
        }
    }
    distance
}

fn boundary_segment_max_distance(point: Point2, segment: &ProfileBoundarySegment) -> f64 {
    let (start, end) = boundary_segment_endpoints(segment);
    let endpoint_distance = point_distance(point, start).max(point_distance(point, end));
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = segment
    else {
        return endpoint_distance;
    };
    let radius = radius.get();
    let farthest_angle = (point.v - center.v).atan2(point.u - center.u) + std::f64::consts::PI;
    if directed_angle_parameter(farthest_angle, *start_angle, *end_angle).is_some() {
        endpoint_distance.max(point_distance(point, *center) + radius)
    } else {
        endpoint_distance
    }
}

pub(super) fn point_in_polygon(point: Point2, vertices: &[Point2]) -> bool {
    point_in_polygon_edges(point, polygon_edges(vertices))
}

fn point_in_polygon_edges(point: Point2, edges: impl Iterator<Item = (Point2, Point2)>) -> bool {
    edges
        .filter(|(start, end)| {
            (start.v > point.v) != (end.v > point.v)
                && point.u < start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
        })
        .count()
        % 2
        == 1
}

fn polygon_strictly_contains(outer: &[Point2], inner: &[Point2]) -> bool {
    inner.iter().all(|point| point_in_polygon(*point, outer))
        && !polygon_edges(outer).any(|outer_edge| {
            polygon_edges(inner).any(|inner_edge| segments_intersect(outer_edge, inner_edge))
        })
}

fn polygon_edges(vertices: &[Point2]) -> impl Iterator<Item = (Point2, Point2)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

/// Answer the distance from `point` to the segment `start`-`end`.
///
/// The segment carries the parameter interval `[0, 1]`, so restricting the
/// projection parameter to that interval is the segment's own definition
/// rather than a correction. A projection outside the interval is not an
/// error: it states that the point projects onto the supporting line beyond an
/// end, and the nearest point of the segment is then that end.
pub(super) fn point_segment_distance(point: Point2, (start, end): (Point2, Point2)) -> f64 {
    let Some(parameter) = cadmpeg_ir::math::planar::line_projection_parameter(start, end, point)
    else {
        return point_distance(point, start).min(point_distance(point, end));
    };
    let parameter = parameter.get().clamp(0.0, 1.0);
    let (Some(u), Some(v)) = (
        cadmpeg_ir::math::interpolate(start.u, end.u, parameter),
        cadmpeg_ir::math::interpolate(start.v, end.v, parameter),
    ) else {
        return f64::INFINITY;
    };
    point_distance(point, Point2::new(u.get(), v.get()))
}

fn segment_distance(left: (Point2, Point2), right: (Point2, Point2)) -> f64 {
    if segments_intersect(left, right) {
        0.0
    } else {
        [
            point_segment_distance(left.0, right),
            point_segment_distance(left.1, right),
            point_segment_distance(right.0, left),
            point_segment_distance(right.1, left),
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min)
    }
}

fn segments_intersect(left: (Point2, Point2), right: (Point2, Point2)) -> bool {
    use cadmpeg_ir::math::planar::orientation;
    use std::cmp::Ordering::Equal;
    let overlaps = |a: f64, b: f64, c: f64, d: f64| a.min(b) <= c.max(d) && c.min(d) <= a.max(b);
    if !overlaps(left.0.u, left.1.u, right.0.u, right.1.u)
        || !overlaps(left.0.v, left.1.v, right.0.v, right.1.v)
    {
        return false;
    }
    let [Some(a), Some(b), Some(c), Some(d)] = [
        orientation(left.0, left.1, right.0),
        orientation(left.0, left.1, right.1),
        orientation(right.0, right.1, left.0),
        orientation(right.0, right.1, left.1),
    ] else {
        return false;
    };
    (a == Equal || b == Equal || a != b) && (c == Equal || d == Equal || c != d)
}

fn polygon_arc_loop_intersects(polygon: &[Point2], arc_loop: &[ProfileBoundarySegment]) -> bool {
    polygon_edges(polygon).any(|(start, end)| {
        let line = ProfileBoundarySegment::Line { start, end };
        arc_loop
            .iter()
            .any(|segment| boundary_segments_intersect(&line, segment))
    })
}

fn arc_loops_intersect(left: &[ProfileBoundarySegment], right: &[ProfileBoundarySegment]) -> bool {
    left.iter().any(|left| {
        right
            .iter()
            .any(|right| boundary_segments_intersect(left, right))
    })
}

fn boundary_segments_intersect(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> bool {
    match (left, right) {
        (
            ProfileBoundarySegment::Line {
                start: left_start,
                end: left_end,
            },
            ProfileBoundarySegment::Line {
                start: right_start,
                end: right_end,
            },
        ) => segments_intersect((*left_start, *left_end), (*right_start, *right_end)),
        (ProfileBoundarySegment::Line { start, end }, arc @ ProfileBoundarySegment::Arc { .. })
        | (arc @ ProfileBoundarySegment::Arc { .. }, ProfileBoundarySegment::Line { start, end }) => {
            line_arc_intersection_points((*start, *end), arc)
                .is_some_and(|points| points.iter().any(Option::is_some))
        }
        (
            ProfileBoundarySegment::Arc {
                center: left_center,
                radius: left_radius,
                start_angle: left_start,
                end_angle: left_end,
            },
            ProfileBoundarySegment::Arc {
                center: right_center,
                radius: right_radius,
                start_angle: right_start,
                end_angle: right_end,
            },
        ) => arcs_intersect(
            (*left_center, *left_radius, *left_start, *left_end),
            (*right_center, *right_radius, *right_start, *right_end),
        ),
    }
}

fn arcs_intersect(
    left: (Point2, PositiveLength, f64, f64),
    right: (Point2, PositiveLength, f64, f64),
) -> bool {
    let (left_center, left_radius, left_start, left_end) = left;
    let (right_center, right_radius, right_start, right_end) = right;
    if left_center == right_center && left_radius == right_radius {
        return [left_start, left_end]
            .into_iter()
            .any(|angle| directed_angle_parameter(angle, right_start, right_end).is_some())
            || [right_start, right_end]
                .into_iter()
                .any(|angle| directed_angle_parameter(angle, left_start, left_end).is_some());
    }
    let left = ProfileBoundarySegment::Arc {
        center: left_center,
        radius: left_radius,
        start_angle: left_start,
        end_angle: left_end,
    };
    let right = ProfileBoundarySegment::Arc {
        center: right_center,
        radius: right_radius,
        start_angle: right_start,
        end_angle: right_end,
    };
    arc_intersection_points(&left, &right).is_some_and(|points| points.iter().any(Option::is_some))
}

pub(super) fn historical_member_points_in_state(
    member: &DesignExtrudeSelectionMember,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let (kind, entity_ref) = match &member.historical {
        Some(binding) => (binding.kind, binding.entity_ref),
        None => {
            let geometry = member.resolved_geometry.as_ref()?;
            let kind = match geometry {
                SketchRelationOperand::Point { .. } => AsmHistoricalEntityKind::Point,
                SketchRelationOperand::Curve { .. } => AsmHistoricalEntityKind::Curve,
                SketchRelationOperand::Surface { .. } | SketchRelationOperand::Record { .. } => {
                    return None;
                }
            };
            let Ok(entity_ref) = i64::try_from(member.local_id) else {
                return None;
            };
            (kind, entity_ref)
        }
    };
    historical_entity_positions(kind, entity_ref, topology)
}

fn historical_entity_positions(
    kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let mut positions = Vec::new();
    let mut edge_refs = Vec::new();
    match kind {
        AsmHistoricalEntityKind::Coedge => {
            for coedge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedge.coedge == local_id)
            {
                edge_refs.push(coedge.edge);
            }
        }
        AsmHistoricalEntityKind::Edge => {
            edge_refs.push(local_id);
        }
        AsmHistoricalEntityKind::Curve => {
            for binding in topology
                .edge_curves
                .iter()
                .filter(|binding| binding.carrier == Some(local_id))
            {
                edge_refs.push(binding.entity);
            }
        }
        AsmHistoricalEntityKind::Loop => {
            for coedge in topology
                .coedge_topology
                .iter()
                .filter(|coedge| coedge.owner_loop == local_id)
            {
                edge_refs.push(coedge.edge);
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            for binding in topology
                .coedge_pcurves
                .iter()
                .filter(|binding| binding.carrier == Some(local_id))
            {
                if let Some(coedge) = topology
                    .coedge_topology
                    .iter()
                    .find(|coedge| coedge.coedge == binding.entity)
                {
                    edge_refs.push(coedge.edge);
                }
            }
        }
        AsmHistoricalEntityKind::Vertex => {
            for point in historical_vertex_positions(topology, local_id) {
                positions.push(point);
            }
        }
        AsmHistoricalEntityKind::Point => {
            for point in topology
                .point_positions
                .iter()
                .filter(|point| point.point == local_id)
            {
                positions.push(point.position);
            }
        }
        AsmHistoricalEntityKind::Face => {
            let points = historical_face_points(local_id, topology)?;
            for point in points {
                positions.push(point);
            }
        }
        AsmHistoricalEntityKind::Surface => {
            let mut found = false;
            for binding in topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.carrier == local_id)
            {
                found = true;
                let points = historical_face_points(binding.entity, topology)?;
                for point in points {
                    positions.push(point);
                }
            }
            if !found {
                return None;
            }
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell => {
            let faces = historical_owned_faces(kind, local_id, topology)?;
            for face in faces {
                let points = historical_face_points(face, topology)?;
                for point in points {
                    positions.push(point);
                }
            }
        }
    }
    for edge_ref in edge_refs {
        let edge = topology
            .edge_vertices
            .iter()
            .find(|edge| edge.edge == edge_ref)?;
        let mut start = historical_vertex_positions(topology, edge.start_vertex).peekable();
        let mut end = historical_vertex_positions(topology, edge.end_vertex).peekable();
        if start.peek().is_none() || end.peek().is_none() {
            return None;
        }
        for point in start.chain(end) {
            positions.push(point);
        }
    }
    (!positions.is_empty()).then_some(positions)
}

fn historical_owned_faces(
    kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let mut faces = Vec::new();
    match kind {
        AsmHistoricalEntityKind::Body => {
            let regions = historical_relation_members(&topology.body_regions, local_id)?;
            for region in regions {
                let shells = historical_relation_members(&topology.region_shells, *region)?;
                for shell in shells {
                    if !append_historical_shell_faces(*shell, topology, &mut faces) {
                        return None;
                    }
                }
            }
        }
        AsmHistoricalEntityKind::Region => {
            let shells = historical_relation_members(&topology.region_shells, local_id)?;
            for shell in shells {
                if !append_historical_shell_faces(*shell, topology, &mut faces) {
                    return None;
                }
            }
        }
        AsmHistoricalEntityKind::Shell => {
            if !append_historical_shell_faces(local_id, topology, &mut faces) {
                return None;
            }
        }
        _ => return None,
    }
    faces.sort_unstable_by(Ord::cmp);
    faces.dedup();
    (!faces.is_empty()).then_some(faces)
}

fn historical_relation_members(
    relations: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut matches = relations
        .iter()
        .filter(|relation| relation.owner_ref == owner);
    let members = &matches.next()?.member_refs;
    matches.next().is_none().then_some(members)
}

fn append_historical_shell_faces(
    shell: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
    faces: &mut Vec<i64>,
) -> bool {
    let Some(members) = historical_relation_members(&topology.shell_faces, shell) else {
        return false;
    };
    for face in members {
        faces.push(*face);
    }
    true
}

fn historical_vertex_positions(
    topology: &crate::history_records::AsmHistoricalTopology,
    vertex_ref: i64,
) -> impl Iterator<Item = Point3> + '_ {
    topology
        .vertex_points
        .iter()
        .filter(move |binding| binding.entity == vertex_ref)
        .filter_map(|binding| {
            topology
                .point_positions
                .iter()
                .find(|point| point.point == binding.carrier)
                .map(|point| point.position)
        })
}

pub(super) fn project_to_sketch(
    sketch: &cadmpeg_ir::sketches::Sketch,
    point: Point3,
) -> Option<Point2> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let offset = point.vector_from(origin.get());
    let v_axis = normal.cross(u_axis.get());
    Some(Point2::new(offset.dot(u_axis.get()), offset.dot(v_axis)))
}

/// Model-space position of the sketch-plane point `point`.
pub(super) fn sketch_point_to_model(
    sketch: &cadmpeg_ir::sketches::Sketch,
    point: Point2,
) -> Option<cadmpeg_ir::features::FinitePoint3> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let v_axis = normal.cross(u_axis.get());
    cadmpeg_ir::features::FinitePoint3::new(
        origin
            .get()
            .translated(u_axis.get(), point.u)
            .translated(v_axis, point.v),
    )
}

pub(super) fn point_on_sketch_entity(
    point: Point2,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() {
        if curve.periodic() {
            return false;
        }
        let lanes = PcurveEvaluatorLanes::new(curve.pole_rows());
        return cadmpeg_ir::eval::nurbs_pcurve_contains_point(
            curve.degree(),
            curve.knots(),
            lanes.points(),
            lanes.weights(),
            point,
            tolerance,
        )
        .unwrap_or(false);
    }
    (|| match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let dx = end.u - start.u;
            let dy = end.v - start.v;
            let length_squared = dx * dx + dy * dy;
            if length_squared == 0.0 {
                return point_distance(point, start.get()) <= tolerance;
            }
            let t = ((point.u - start.u) * dx + (point.v - start.v) * dy) / length_squared;
            if !(-tolerance..=1.0 + tolerance).contains(&t) {
                return false;
            }
            point_distance(point, Point2::new(start.u + t * dx, start.v + t * dy)) <= tolerance
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            (point_distance(point, center.get()) - radius.get()).abs() <= tolerance
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radial_error = (point_distance(point, center.get()) - radius.get()).abs();
            if radial_error > tolerance {
                return false;
            }
            let angle = (point.v - center.v).atan2(point.u - center.u);
            angle_in_sweep(
                angle,
                start_angle.get(),
                end_angle.get(),
                tolerance / radius.get(),
            )
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let local_u = du * cosine + dv * sine;
            let local_v = -du * sine + dv * cosine;
            let parameter = (local_v / radii.minor().get()).atan2(local_u / radii.major().get());
            let boundary = Point2::new(
                center.u + radii.major().get() * parameter.cos() * cosine
                    - radii.minor().get() * parameter.sin() * sine,
                center.v
                    + radii.major().get() * parameter.cos() * sine
                    + radii.minor().get() * parameter.sin() * cosine,
            );
            if point_distance(point, boundary) > tolerance {
                return false;
            }
            match bounds {
                None => true,
                Some([start, end]) => angle_in_sweep(
                    parameter,
                    start.get(),
                    end.get(),
                    tolerance / radii.major().get().min(radii.minor().get()),
                ),
            }
        }
        _ => false,
    })()
}

pub(super) fn angle_in_sweep(angle: f64, start: f64, end: f64, tolerance: f64) -> bool {
    let sweep = end - start;
    if sweep.abs() >= std::f64::consts::TAU - tolerance {
        return true;
    }
    if sweep >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep + tolerance
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU) <= -sweep + tolerance
    }
}

fn point_distance(a: Point2, b: Point2) -> f64 {
    (a.u - b.u).hypot(a.v - b.v)
}

/// Closed profile loops and unprofiled curves for one planar sketch.
#[derive(Debug, Default, Clone)]
pub(super) struct ClosedSketchProfiles {
    pub(super) profiles: Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>>,
    /// Curves in no closed profile loop, each with its loss. The caller emits
    /// the loss after it knows whether a feature selects a region the curve bounds.
    pub(super) unprofiled: Vec<(
        cadmpeg_ir::sketches::SketchEntityId,
        cadmpeg_ir::report::loss::LossNote,
    )>,
}

impl std::ops::Deref for ClosedSketchProfiles {
    type Target = [Vec<cadmpeg_ir::sketches::SketchEntityUse>];

    fn deref(&self) -> &Self::Target {
        &self.profiles
    }
}

impl IntoIterator for ClosedSketchProfiles {
    type Item = Vec<cadmpeg_ir::sketches::SketchEntityUse>;
    type IntoIter = std::vec::IntoIter<Vec<cadmpeg_ir::sketches::SketchEntityUse>>;

    fn into_iter(self) -> Self::IntoIter {
        self.profiles.into_iter()
    }
}

impl<'a> IntoIterator for &'a ClosedSketchProfiles {
    type Item = &'a Vec<cadmpeg_ir::sketches::SketchEntityUse>;
    type IntoIter = std::slice::Iter<'a, Vec<cadmpeg_ir::sketches::SketchEntityUse>>;

    fn into_iter(self) -> Self::IntoIter {
        self.profiles.iter()
    }
}

pub(super) fn closed_sketch_profiles(
    ctx: &DecodeContext<'_>,
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> Result<ClosedSketchProfiles, CodecError> {
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometryDefinition};

    if !linear_tolerance.is_finite() || linear_tolerance <= 0.0 {
        return Ok(ClosedSketchProfiles::default());
    }
    let mut profiles = Vec::new();
    let mut unprofiled_notes = Vec::new();
    let mut edges = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && !entity.construction)
    {
        if matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
                | SketchGeometryDefinition::Ellipse { bounds: None, .. }
        ) {
            let id = entity.id().clone();
            let profile = vec![SketchEntityUse {
                entity: id,
                reversed: false,
            }];
            profiles.push(profile);
        }
        if let Some(ends) = sketch_entity_endpoints(entity, ctx)? {
            edges.push((entity, ends));
        }
    }
    if edges.is_empty() {
        profiles.sort_by(|a, b| a[0].entity.cmp(&b[0].entity));
        return Ok(ClosedSketchProfiles {
            profiles,
            unprofiled: Vec::new(),
        });
    }

    let mut endpoints = Vec::new();
    for (_, [start, end]) in &edges {
        endpoints.push(*start);
        endpoints.push(*end);
    }
    let mut parents = Vec::new();
    for index in 0..endpoints.len() {
        parents.push(index);
    }
    let mut endpoint_cells = HashMap::<(i64, i64), Vec<usize>>::new();
    for (endpoint, point) in endpoints.iter().copied().enumerate() {
        let Some(u) = truncate_f64_to_i64((point.u / linear_tolerance).floor()) else {
            return Ok(ClosedSketchProfiles::default());
        };
        let Some(v) = truncate_f64_to_i64((point.v / linear_tolerance).floor()) else {
            return Ok(ClosedSketchProfiles::default());
        };
        let cell = (u, v);
        for u_offset in -1..=1 {
            for v_offset in -1..=1 {
                let Some(adjacent_u) = cell.0.checked_add(u_offset) else {
                    continue;
                };
                let Some(adjacent_v) = cell.1.checked_add(v_offset) else {
                    continue;
                };
                let adjacent = (adjacent_u, adjacent_v);
                for candidate in endpoint_cells.get(&adjacent).into_iter().flatten() {
                    if sketch_endpoints_close(point, endpoints[*candidate], linear_tolerance) {
                        union_endpoint_nodes(&mut parents, endpoint, *candidate);
                    }
                }
            }
        }
        endpoint_cells.entry(cell).or_default().push(endpoint);
    }
    let mut edge_nodes = Vec::new();
    for edge in 0..edges.len() {
        let nodes = [
            endpoint_root(&mut parents, edge * 2),
            endpoint_root(&mut parents, edge * 2 + 1),
        ];
        edge_nodes.push(nodes);
    }
    let mut adjacency = HashMap::<usize, Vec<usize>>::new();
    for (edge, [start, end]) in edge_nodes.iter().copied().enumerate() {
        adjacency.entry(start).or_default().push(edge);
        adjacency.entry(end).or_default().push(edge);
    }
    for incident in adjacency.values_mut() {
        incident.sort_by(|a, b| edges[*a].0.id().cmp(edges[*b].0.id()));
    }

    let mut visited = ctx.alloc_filled(edges.len(), false, "f3d edge component marks")?;
    let mut order = Vec::new();
    for edge in 0..edges.len() {
        order.push(edge);
    }
    order.sort_by(|a, b| edges[*a].0.id().cmp(edges[*b].0.id()));
    for first_edge in order {
        if visited[first_edge] {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = Vec::new();
        pending.push(first_edge);
        let mut component_seen = HashSet::new();
        while let Some(edge) = pending.pop() {
            if !component_seen.insert(edge) {
                continue;
            }
            component.push(edge);
            for node in edge_nodes[edge] {
                for next in adjacency[&node].iter().copied() {
                    pending.push(next);
                }
            }
        }
        if component
            .iter()
            .flat_map(|edge| edge_nodes[*edge])
            .any(|node| adjacency[&node].len() != 2)
        {
            if component.iter().all(|edge| {
                matches!(
                    (edges[*edge].0.geometry).definition(),
                    SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. }
                )
            }) {
                let mut pruned = component.clone();
                pruned.sort_by(|a, b| edges[*a].0.id().cmp(edges[*b].0.id()));
                let mut deduped: Vec<usize> = Vec::with_capacity(pruned.len());
                let mut dup_replacement = HashMap::<
                    cadmpeg_ir::sketches::SketchEntityId,
                    Vec<(cadmpeg_ir::sketches::SketchEntityId, bool)>,
                >::new();
                for &edge in &pruned {
                    let [u0, v0] = edge_nodes[edge];
                    let pair = if u0 <= v0 { [u0, v0] } else { [v0, u0] };
                    let dup_of = deduped.iter().copied().find(|&other| {
                        let [ou0, ov0] = edge_nodes[other];
                        let opair = if ou0 <= ov0 { [ou0, ov0] } else { [ov0, ou0] };
                        if pair != opair {
                            return false;
                        }
                        match (
                            (edges[edge].0.geometry).definition(),
                            (edges[other].0.geometry).definition(),
                        ) {
                            (
                                SketchGeometryDefinition::Line { .. },
                                SketchGeometryDefinition::Line { .. },
                            ) => true,
                            (
                                SketchGeometryDefinition::Arc {
                                    center: c1,
                                    radius: r1,
                                    start_angle: s1,
                                    end_angle: e1,
                                },
                                SketchGeometryDefinition::Arc {
                                    center: c2,
                                    radius: r2,
                                    start_angle: s2,
                                    end_angle: e2,
                                },
                            ) => {
                                if point_distance(c1.get(), c2.get()) > linear_tolerance
                                    || (r1.get() - r2.get()).abs() > linear_tolerance
                                {
                                    return false;
                                }
                                let angular_tolerance = (linear_tolerance
                                    / r1.get().max(linear_tolerance))
                                .max(EPS_GEOMETRY_ARC_ANGULAR_TOLERANCE_E9);
                                let sweep1 = e1.get() - s1.get();
                                let sweep2 = e2.get() - s2.get();
                                let angle_dist = |a: f64, b: f64| {
                                    let diff = (a - b).rem_euclid(std::f64::consts::TAU);
                                    diff.min(std::f64::consts::TAU - diff)
                                };
                                let same_sweep =
                                    (sweep1.abs() - sweep2.abs()).abs() <= angular_tolerance;
                                let same_dir = (sweep1 > 0.0) == (sweep2 > 0.0)
                                    && angle_dist(s1.get(), s2.get()) <= angular_tolerance
                                    && angle_dist(e1.get(), e2.get()) <= angular_tolerance;
                                let opp_dir = (sweep1 > 0.0) != (sweep2 > 0.0)
                                    && angle_dist(s1.get(), e2.get()) <= angular_tolerance
                                    && angle_dist(e1.get(), s2.get()) <= angular_tolerance;
                                same_sweep && (same_dir || opp_dir)
                            }
                            _ => false,
                        }
                    });
                    if let Some(canon_edge) = dup_of {
                        let flip_dir = edge_nodes[edge][0] != edge_nodes[canon_edge][0];
                        dup_replacement
                            .entry(edges[canon_edge].0.id().clone())
                            .or_default()
                            .push((edges[edge].0.id().clone(), flip_dir));
                    } else {
                        deduped.push(edge);
                    }
                }
                pruned = deduped;
                loop {
                    let mut deg = HashMap::<usize, usize>::new();
                    for &edge in &pruned {
                        for node in edge_nodes[edge] {
                            *deg.entry(node).or_insert(0) += 1;
                        }
                    }
                    let before = pruned.len();
                    pruned.retain(|&edge| {
                        let [a, b] = edge_nodes[edge];
                        deg.get(&a).copied().unwrap_or(0) >= 2
                            && deg.get(&b).copied().unwrap_or(0) >= 2
                    });
                    if pruned.len() == before {
                        break;
                    }
                }
                let mut b_profiles: Vec<Vec<SketchEntityUse>> = Vec::new();
                for profile in
                    branched_line_profiles(&pruned, &edges, &edge_nodes, linear_tolerance)
                {
                    let mut counts = HashMap::<cadmpeg_ir::sketches::SketchEntityId, usize>::new();
                    for u in &profile {
                        *counts.entry(u.entity.clone()).or_insert(0) += 1;
                    }
                    if counts.values().all(|&c| c == 1) {
                        b_profiles.push(profile);
                    }
                }
                if b_profiles.len() > 1 && !dup_replacement.is_empty() {
                    for (canon_id, dups) in &dup_replacement {
                        let sharing: Vec<usize> = b_profiles
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| {
                                if p.iter().any(|u| &u.entity == canon_id) {
                                    Some(i)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if sharing.len() >= 2 {
                            for (dup_idx, (dup_id, flip_dir)) in dups.iter().enumerate() {
                                if let Some(&target_idx) = sharing.get(1 + dup_idx) {
                                    for u in &mut b_profiles[target_idx] {
                                        if &u.entity == canon_id {
                                            u.entity = dup_id.clone();
                                            if *flip_dir {
                                                u.reversed = !u.reversed;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                for profile in &mut b_profiles {
                    if let Some(min_idx) = profile
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| a.entity.cmp(&b.entity))
                        .map(|(i, _)| i)
                    {
                        profile.rotate_left(min_idx);
                    }
                }
                b_profiles.sort_by(|a, b| a[0].entity.cmp(&b[0].entity));

                if b_profiles.is_empty()
                    && component.iter().all(|edge| {
                        matches!(
                            (edges[*edge].0.geometry).definition(),
                            SketchGeometryDefinition::Line { .. }
                        )
                    })
                {
                    if let Some(profile) = tangent_nested_line_profile(
                        &component,
                        &edges,
                        &edge_nodes,
                        &adjacency,
                        linear_tolerance,
                    ) {
                        b_profiles.push(profile);
                    }
                }

                let mut candidate_entities = HashSet::<cadmpeg_ir::sketches::SketchEntityId>::new();
                for &edge in &component {
                    candidate_entities.insert(edges[edge].0.id().clone());
                }
                for dups in dup_replacement.values() {
                    for (dup_id, _) in dups {
                        candidate_entities.insert(dup_id.clone());
                    }
                }
                let mut profiled_entities = HashSet::<cadmpeg_ir::sketches::SketchEntityId>::new();
                for profile in &b_profiles {
                    for u in profile {
                        profiled_entities.insert(u.entity.clone());
                    }
                }
                let mut unprofiled: Vec<_> = candidate_entities
                    .into_iter()
                    .filter(|id| !profiled_entities.contains(id))
                    .collect();
                unprofiled.sort();
                for id in unprofiled {
                    let note = crate::loss::F3dLossCode::SketchCurveUnprofiled.note(format!(
                        "sketch entity {id}: curve is not part of any closed profile loop"
                    ));
                    unprofiled_notes.push((id, note));
                }
                profiles.extend(b_profiles);
            }
            for edge in component {
                visited[edge] = true;
            }
            continue;
        }

        component.sort_by(|a, b| edges[*a].0.id().cmp(edges[*b].0.id()));
        let first_edge = component[0];
        let start_node = edge_nodes[first_edge][0];
        let mut current_node = edge_nodes[first_edge][1];
        let mut profile = Vec::new();
        let first_id = edges[first_edge].0.id().clone();
        profile.push(SketchEntityUse {
            entity: first_id,
            reversed: false,
        });
        visited[first_edge] = true;
        while current_node != start_node {
            let Some(next_edge) = adjacency[&current_node]
                .iter()
                .copied()
                .find(|edge| !visited[*edge])
            else {
                profile.clear();
                break;
            };
            let [stored_start, stored_end] = edge_nodes[next_edge];
            let reversed = stored_end == current_node;
            current_node = if reversed { stored_start } else { stored_end };
            visited[next_edge] = true;
            let id = edges[next_edge].0.id().clone();
            profile.push(SketchEntityUse {
                entity: id,
                reversed,
            });
        }
        if !profile.is_empty() && component.iter().all(|edge| visited[*edge]) {
            profiles.push(profile);
        }
    }
    profiles.sort_by(|a, b| a[0].entity.cmp(&b[0].entity));
    Ok(ClosedSketchProfiles {
        profiles,
        unprofiled: unprofiled_notes,
    })
}

/// Direction and signed curvature of a half-edge where it leaves its start node.
fn outgoing_half_edge_ray(
    half_edge: usize,
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
) -> (f64, f64) {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let edge = half_edge / 2;
    let reversed = !half_edge.is_multiple_of(2);
    let [start, end] = edges[edge].1;
    match (edges[edge].0.geometry).definition() {
        SketchGeometryDefinition::Arc {
            radius,
            start_angle,
            end_angle,
            ..
        } => {
            let counterclockwise = (end_angle.get() >= start_angle.get()) != reversed;
            let at = if reversed {
                end_angle.get()
            } else {
                start_angle.get()
            };
            let turn = if counterclockwise { 1.0 } else { -1.0 };
            (
                (turn * at.cos()).atan2(-turn * at.sin()),
                turn / radius.get(),
            )
        }
        _ => {
            let (from, to) = if reversed { (end, start) } else { (start, end) };
            ((to.v - from.v).atan2(to.u - from.u), 0.0)
        }
    }
}

/// Sort the half-edges leaving one node counterclockwise. Rays with one
/// direction separate by curvature, because the ray that turns further left
/// lies counterclockwise of the other next to the node.
fn order_outgoing_half_edges(
    half_edges: &mut [usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
) {
    half_edges.sort_by(|first, second| {
        outgoing_half_edge_ray(*first, edges)
            .0
            .total_cmp(&outgoing_half_edge_ray(*second, edges).0)
    });
    let gap_after = |index: usize, half_edges: &[usize]| {
        let next = (index + 1) % half_edges.len();
        (outgoing_half_edge_ray(half_edges[next], edges).0
            - outgoing_half_edge_ray(half_edges[index], edges).0)
            .rem_euclid(std::f64::consts::TAU)
    };
    if let Some(group_end) = (0..half_edges.len())
        .find(|index| gap_after(*index, half_edges) > EPS_GEOMETRY_TANGENT_RAY_ANGLE_E9)
    {
        half_edges.rotate_left(group_end + 1);
    }
    let mut group_start = 0;
    for index in 0..half_edges.len() {
        if index + 1 == half_edges.len()
            || gap_after(index, half_edges) > EPS_GEOMETRY_TANGENT_RAY_ANGLE_E9
        {
            half_edges[group_start..=index].sort_by(|first, second| {
                outgoing_half_edge_ray(*first, edges)
                    .1
                    .total_cmp(&outgoing_half_edge_ray(*second, edges).1)
                    .then_with(|| edges[*first / 2].0.id().cmp(edges[*second / 2].0.id()))
                    .then_with(|| first.cmp(second))
            });
            group_start = index + 1;
        }
    }
}

/// Half-edge walk over one branched line component. The local `outgoing` map
/// is the only adjacency this walk needs: it is built here from `edge_nodes`,
/// so every half-edge reaches its twin by construction.
fn branched_line_profiles(
    component: &[usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
    edge_nodes: &[[usize; 2]],
    linear_tolerance: f64,
) -> Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>> {
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometryDefinition};

    let mut component_set = HashSet::new();
    for edge in component.iter().copied() {
        component_set.insert(edge);
    }
    let mut outgoing = HashMap::<usize, Vec<usize>>::new();
    for edge in &component_set {
        outgoing
            .entry(edge_nodes[*edge][0])
            .or_default()
            .push(edge * 2);
        outgoing
            .entry(edge_nodes[*edge][1])
            .or_default()
            .push(edge * 2 + 1);
    }
    for half_edges in outgoing.values_mut() {
        order_outgoing_half_edges(half_edges, edges);
    }

    let mut next = HashMap::new();
    for around in outgoing.values() {
        for (&previous, &twin) in around.iter().zip(around.iter().cycle().skip(1)) {
            next.insert(twin ^ 1, previous);
        }
    }

    let mut profiles = Vec::new();
    let mut visited = HashSet::new();
    let mut starts = Vec::new();
    for edge in &component_set {
        starts.push(edge * 2);
        starts.push(edge * 2 + 1);
    }
    starts.sort_by(|a, b| (edges[*a / 2].0.id(), *a % 2).cmp(&(edges[*b / 2].0.id(), *b % 2)));
    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let mut current = start;
        let mut profile = Vec::new();
        let mut twice_area = 0.0;
        loop {
            if !visited.insert(current) {
                if current != start {
                    profile.clear();
                }
                break;
            }
            let edge = current / 2;
            let [stored_start, stored_end] = edges[edge].1;
            let reversed = !current.is_multiple_of(2);
            let (from, to) = if reversed {
                (stored_end, stored_start)
            } else {
                (stored_start, stored_end)
            };
            match (edges[edge].0.geometry).definition() {
                SketchGeometryDefinition::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => {
                    let s = start_angle.get();
                    let e = end_angle.get();
                    let d_theta = if reversed { s - e } else { e - s };
                    let r = radius.get();
                    let c = center.get();
                    twice_area += r * r * d_theta + c.u * (to.v - from.v) - c.v * (to.u - from.u);
                }
                _ => {
                    twice_area += from.u * to.v - from.v * to.u;
                }
            }
            let id = edges[edge].0.id().clone();
            profile.push(SketchEntityUse {
                entity: id,
                reversed,
            });
            current = next[&current];
        }
        if !profile.is_empty() && twice_area > 2.0 * linear_tolerance * linear_tolerance {
            profiles.push(profile);
        }
    }

    profiles
}

/// Resolve a tangent nested pair that has one shared corner.
///
/// The ordinary half-edge walk needs a strict angular order at every node.
/// Two profile boundaries that touch at a corner have coincident outgoing
/// rays, so that walk can consume both boundaries as one zero-area exterior
/// traversal. A neutral profile can still represent the saved face when the
/// graph proves one source-oriented outer cycle and one contained
/// oppositely-oriented inner cycle. The two cycles are then one connected
/// boundary with a tangent notch. Other tangent branch shapes remain
/// unresolved rather than choosing a profile by geometry alone.
fn tangent_nested_line_profile(
    component: &[usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
    edge_nodes: &[[usize; 2]],
    adjacency: &HashMap<usize, Vec<usize>>,
    tolerance: f64,
) -> Option<Vec<cadmpeg_ir::sketches::SketchEntityUse>> {
    use cadmpeg_ir::sketches::SketchEntityUse;

    let mut junction = None;
    for edge in component {
        for node in edge_nodes[*edge] {
            let degree = adjacency[&node].len();
            if !matches!(degree, 2 | 4) {
                return None;
            }
            if degree > 2 {
                if junction.is_some_and(|existing| existing != node) {
                    return None;
                }
                junction = Some(node);
            }
        }
    }
    let junction = junction?;
    if adjacency[&junction].len() != 4 {
        return None;
    }
    let mut directions = [(0.0, 0.0); 4];
    for (index, edge) in adjacency[&junction].iter().enumerate() {
        let [start, end] = edges[*edge].1;
        let (from, to) = if edge_nodes[*edge][0] == junction {
            (start, end)
        } else {
            (end, start)
        };
        let (du, dv) = (to.u - from.u, to.v - from.v);
        let length = du.hypot(dv);
        if !length.is_finite() || length <= tolerance {
            return None;
        }
        directions[index] = (du / length, dv / length);
    }
    let same_ray = |left: (f64, f64), right: (f64, f64)| {
        let dot = left.0 * right.0 + left.1 * right.1;
        let cross = left.0 * right.1 - left.1 * right.0;
        dot > 1.0 - EPS_GEOMETRY_TANGENT_NESTED_LINE_PROFILE_E10
            && cross.abs() <= EPS_GEOMETRY_TANGENT_NESTED_LINE_PROFILE_E10
    };
    if directions.iter().enumerate().any(|(index, direction)| {
        directions
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .filter(|(_, other)| same_ray(*direction, **other))
            .count()
            != 1
    }) {
        return None;
    }

    let mut cycles = Vec::new();
    for first_edge in component
        .iter()
        .copied()
        .filter(|edge| edge_nodes[*edge][0] == junction)
    {
        let mut used = HashSet::new();
        let mut current = junction;
        let mut cycle = Vec::new();
        loop {
            let mut candidates = component
                .iter()
                .copied()
                .filter(|edge| !used.contains(edge) && edge_nodes[*edge][0] == current);
            let edge = if cycle.is_empty() {
                if edge_nodes[first_edge][0] != current {
                    return None;
                }
                first_edge
            } else {
                let edge = candidates.next()?;
                if candidates.next().is_some() {
                    cycle.clear();
                    break;
                }
                edge
            };
            if !used.insert(edge) {
                cycle.clear();
                break;
            }
            cycle.push(edge);
            current = edge_nodes[edge][1];
            if current == junction {
                break;
            }
        }
        if !cycle.is_empty()
            && cycle.len() >= 3
            && cycle.iter().all(|edge| component.contains(edge))
            && !cycles
                .iter()
                .any(|candidate: &Vec<usize>| candidate == &cycle)
        {
            cycles.push(cycle);
        }
    }
    if cycles.len() != 2
        || cycles[0].iter().any(|edge| cycles[1].contains(edge))
        || cycles.iter().flatten().count() != component.len()
    {
        return None;
    }

    let cycle_points = |cycle: &[usize]| {
        let mut points = Vec::new();
        for edge in cycle {
            points.push(edges[*edge].1[0]);
        }
        points
    };
    let points = [cycle_points(&cycles[0]), cycle_points(&cycles[1])];
    let areas = [
        signed_polygon_area(&points[0]),
        signed_polygon_area(&points[1]),
    ];
    if areas
        .iter()
        .any(|area| !area.is_finite() || area.abs() <= tolerance * tolerance)
        || areas[0].signum() == areas[1].signum()
    {
        return None;
    }

    let probe = |cycle: &[usize]| {
        cycle.iter().find_map(|edge| {
            let [start, end] = edges[*edge].1;
            if edge_nodes[*edge].contains(&junction) {
                return None;
            }
            Some(Point2::new(
                (start.u + end.u) * 0.5,
                (start.v + end.v) * 0.5,
            ))
        })
    };
    let contains = |outer: usize, inner: usize| {
        probe(&cycles[inner]).is_some_and(|point| point_in_polygon(point, &points[outer]))
    };
    let outer = if areas[0].abs() > areas[1].abs() && contains(0, 1) {
        0
    } else if areas[1].abs() > areas[0].abs() && contains(1, 0) {
        1
    } else {
        return None;
    };
    let inner = 1 - outer;

    let mut profile = Vec::new();
    for (index, positive) in [(outer, true), (inner, false)] {
        let reverse = (areas[index] > 0.0) != positive;
        for position in 0..cycles[index].len() {
            let offset = if reverse {
                cycles[index].len() - 1 - position
            } else {
                position
            };
            let edge = cycles[index][offset];
            let entity = edges[edge].0.id().clone();
            profile.push(SketchEntityUse {
                entity,
                reversed: reverse,
            });
        }
    }
    let mut profile_points = Vec::new();
    for use_ in &profile {
        let Some(entity) = edges
            .iter()
            .find(|(entity, _)| entity.id() == &use_.entity)
            .map(|(entity, _)| entity)
        else {
            continue;
        };
        if let cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } =
            *entity.geometry.definition()
        {
            let point = if use_.reversed {
                end.get()
            } else {
                start.get()
            };
            profile_points.push(point);
        }
    }
    (profile_points.len() == profile.len()
        && signed_polygon_area(&profile_points) > 2.0 * tolerance * tolerance)
        .then_some(profile)
}

pub(super) fn sketch_entity_endpoints(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[Point2; 2]>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    Ok(match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some([start.get(), end.get()]),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            Point2::new(
                center.u + radius.get() * start_angle.get().cos(),
                center.v + radius.get() * start_angle.get().sin(),
            ),
            Point2::new(
                center.u + radius.get() * end_angle.get().cos(),
                center.v + radius.get() * end_angle.get().sin(),
            ),
        ]),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds: Some([start_angle, end_angle]),
        } => {
            let point_at = |parameter: f64| {
                let x = radii.major().get() * parameter.cos();
                let y = radii.minor().get() * parameter.sin();
                Point2::new(
                    center.u + x * major_angle.get().cos() - y * major_angle.get().sin(),
                    center.v + x * major_angle.get().sin() + y * major_angle.get().cos(),
                )
            };
            Some([point_at(start_angle.get()), point_at(end_angle.get())])
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let lanes = PcurveEvaluatorLanes::new(curve.pole_rows());
            let start_parameter = curve.knots()[index_from_u32(curve.degree())];
            let end_parameter = curve.knots()[lanes.points().len()];
            let start = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                curve.degree(),
                curve.knots(),
                lanes.points(),
                lanes.weights(),
                start_parameter,
            ))?;
            let end = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                curve.degree(),
                curve.knots(),
                lanes.points(),
                lanes.weights(),
                end_parameter,
            ))?;
            start
                .zip(end)
                .map(|(start, end)| [*start.as_raw(), *end.as_raw()])
        }
        _ => None,
    })
}

fn sketch_endpoints_close(first: Point2, second: Point2, tolerance: f64) -> bool {
    (first.u - second.u).hypot(first.v - second.v) <= tolerance
}

fn endpoint_root(parents: &mut [usize], node: usize) -> usize {
    if parents[node] != node {
        parents[node] = endpoint_root(parents, parents[node]);
    }
    parents[node]
}

fn union_endpoint_nodes(parents: &mut [usize], first: usize, second: usize) {
    let first = endpoint_root(parents, first);
    let second = endpoint_root(parents, second);
    if first != second {
        parents[second] = first;
    }
}

#[cfg(test)]
mod tests;
