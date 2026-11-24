//! The geometric measurements and kernel primitives a finger-joint recipe uses.
//!
//! Numeric geometry is computed by `solidcore`; this module only adapts the application's
//! coordinate arrays to the kernel types and exposes the resulting primitive API.

use crate::Result;
use solidcore::{Point3, Vec3};

/// The stage of finger-joint generation that failed, with the original operation error.
#[derive(Debug)]
pub struct Failure {
    /// The operation being performed when the error occurred.
    pub stage: &'static str,
    /// The underlying `SolidCore` operation error.
    pub source: crate::Error,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.source)
    }
}

impl std::error::Error for Failure {}

impl From<crate::Error> for Failure {
    fn from(source: crate::Error) -> Self {
        Self {
            stage: "finger-joint recipe",
            source,
        }
    }
}

fn at_stage<T>(stage: &'static str, result: Result<T>) -> std::result::Result<T, Failure> {
    result.map_err(|source| Failure { stage, source })
}

pub use solidcore::finger_joint::{allowances, cutter_box, pattern, Interval};

/// Cutter bodies generated for the first and second plates of a pair.
pub type PairCutterTools = (Option<crate::Body>, Option<crate::Body>);

/// The dominant plate axis from outward planar face normals and areas.
#[must_use]
pub fn plate_axis(faces: &[([f64; 3], f64)]) -> Option<[f64; 3]> {
    let core_faces: Vec<_> = faces
        .iter()
        .map(|(normal, area)| (Vec3::from(*normal), *area))
        .collect();
    solidcore::finger_joint::plate_axis(&core_faces).map(Vec3::to_array)
}

/// The unit direction where two plate planes cross, or `None` for parallel planes.
#[must_use]
pub fn joint_line(first: [f64; 3], second: [f64; 3]) -> Option<[f64; 3]> {
    solidcore::finger_joint::joint_line(Vec3::from(first), Vec3::from(second)).map(Vec3::to_array)
}

/// A right-handed finger-joint frame, rows as images of x, y and z and then translation.
#[must_use]
pub fn frame(line: [f64; 3], normal: [f64; 3], origin: [f64; 3]) -> Option<[[f64; 4]; 4]> {
    solidcore::finger_joint::frame(Vec3::from(line), Vec3::from(normal), Point3::from(origin))
        .map(|transform| transform.m)
}

/// The midpoint of a finite ordered box, as measured by the kernel.
#[must_use]
pub fn bounds_center(lower: [f64; 3], upper: [f64; 3]) -> Option<[f64; 3]> {
    solidcore::finger_joint::bounds_center(lower, upper).map(Vec3::to_array)
}

/// Build the rectangular through-cut tools for two intersecting plates in their shared frame.
///
/// Each connected overlap region gets its own pattern. The returned bodies are disconnected
/// compounds of the box cutters for the first and second plates, respectively. `None` means the
/// plates have no positive-volume overlap or their axes are parallel. A body without a unique
/// dominant plate axis is refused with an error.
///
/// # Errors
/// The kernel's errors measuring, intersecting, transforming or constructing cutter bodies;
/// `Refused` when no positive segment width remains after applying offsets and clearance.
pub fn pair_tools(
    first: &crate::Body,
    second: &crate::Body,
    first_world: [[f64; 4]; 4],
    second_world: [[f64; 4]; 4],
    feature_world: [[f64; 4]; 4],
    count: usize,
    clearance: f64,
    edge_offset: f64,
    inverted: bool,
) -> std::result::Result<Option<PairCutterTools>, Failure> {
    if count == 0
        || count > 64
        || !clearance.is_finite()
        || clearance < 0.0
        || !edge_offset.is_finite()
        || edge_offset < 0.0
    {
        return Err(crate::Error::Argument.into());
    }
    let inverse_feature = crate::affine::inverse(&feature_world)?;
    let first_to_feature = crate::affine::then(&inverse_feature, &first_world)?;
    let second_to_feature = crate::affine::then(&inverse_feature, &second_world)?;
    let first_feature = first.copy()?;
    let second_feature = second.copy()?;
    at_stage(
        "transforming the first plate into feature coordinates",
        first_feature.transform(&first_to_feature),
    )?;
    at_stage(
        "transforming the second plate into feature coordinates",
        second_feature.transform(&second_to_feature),
    )?;
    let (_, first_order) = at_stage(
        "reading the first plate's planar faces",
        first_feature.to_record(),
    )?;
    let (_, second_order) = at_stage(
        "reading the second plate's planar faces",
        second_feature.to_record(),
    )?;
    let first_faces = at_stage(
        "measuring the first plate's planar faces",
        first_feature.planar_face_areas(&first_order),
    )?;
    let second_faces = at_stage(
        "measuring the second plate's planar faces",
        second_feature.planar_face_areas(&second_order),
    )?;
    let Some(first_normal) = plate_axis(&first_faces) else {
        return Err(crate::Error::Refused {
            operation: "plate axis inference",
            reason: "the first plate has no unique dominant axis",
        }
        .into());
    };
    let Some(second_normal) = plate_axis(&second_faces) else {
        return Err(crate::Error::Refused {
            operation: "plate axis inference",
            reason: "the second plate has no unique dominant axis",
        }
        .into());
    };
    // Rectangular cutters are exact for rectangular footprints, but a zero-clearance cutter
    // must not extend across a notch in an orthogonal overlap. Keep angled and clearance-expanded
    // joints on the established through-cut path.
    let normal_dot =
        solidcore::finger_joint::normal_dot(Vec3::from(first_normal), Vec3::from(second_normal));
    let clip_stepped_footprint = clearance == 0.0 && normal_dot.abs() <= 1e-6;
    let Some(line) = joint_line(first_normal, second_normal) else {
        return Ok(None);
    };
    let Some(overlap) = at_stage(
        "intersecting the two plates",
        crate::boolean::boolean(
            first_feature.copy()?,
            second_feature.copy()?,
            crate::BooleanOp::Intersect,
        ),
    )?
    else {
        return Ok(None);
    };
    let (overlap_record, _) = at_stage(
        "converting the plate intersection to a record",
        overlap.to_record(),
    )?;
    let mut first_tools = Vec::new();
    let mut second_tools = Vec::new();
    for region_index in overlap_record.material_regions() {
        let region_record = overlap_record
            .part(&[region_index])
            .map_err(|_| crate::Error::Argument)?
            .brep;
        let (region, _) = at_stage(
            "rebuilding an intersection region from its record",
            crate::Body::from_record(&region_record, true),
        )?;
        let (lower, upper) = at_stage("measuring an intersection region", region.bounds())?;
        let Some(origin) = bounds_center(lower, upper) else {
            return Err(crate::Error::Argument.into());
        };
        let first_frame = frame(line, first_normal, origin).ok_or(crate::Error::Argument)?;
        let second_frame = frame(line, second_normal, origin).ok_or(crate::Error::Argument)?;
        let first_pattern = at_stage(
            "measuring the first plate's finger pattern",
            pattern_in_frame(&region, first_frame, count, edge_offset, clearance),
        )?;
        let second_pattern = at_stage(
            "measuring the second plate's finger pattern",
            pattern_in_frame(&region, second_frame, count, edge_offset, clearance),
        )?;
        first_tools.extend(at_stage(
            "building cutters for the first plate",
            cutters_in_frame(
                &region,
                &first_feature,
                first_frame,
                &first_pattern,
                !inverted,
                clearance,
                clip_stepped_footprint,
            ),
        )?);
        second_tools.extend(at_stage(
            "building cutters for the second plate",
            cutters_in_frame(
                &region,
                &second_feature,
                second_frame,
                &second_pattern,
                inverted,
                clearance,
                clip_stepped_footprint,
            ),
        )?);
    }
    let first_tools = at_stage(
        "combining the first plate's cutters",
        compound_tools(&first_tools),
    )?;
    let second_tools = at_stage(
        "combining the second plate's cutters",
        compound_tools(&second_tools),
    )?;
    let first_to_body =
        crate::affine::then(&crate::affine::inverse(&first_world)?, &feature_world)?;
    let second_to_body =
        crate::affine::then(&crate::affine::inverse(&second_world)?, &feature_world)?;
    if let Some(tools) = &first_tools {
        at_stage(
            "placing the first plate's cutters in body coordinates",
            tools.transform(&first_to_body),
        )?;
    }
    if let Some(tools) = &second_tools {
        at_stage(
            "placing the second plate's cutters in body coordinates",
            tools.transform(&second_to_body),
        )?;
    }
    Ok(Some((first_tools, second_tools)))
}

/// Pattern and allowances for one overlap region, measured in its plate frame.
fn pattern_in_frame(
    region: &crate::Body,
    frame: [[f64; 4]; 4],
    count: usize,
    edge_offset: f64,
    clearance: f64,
) -> Result<Vec<Interval>> {
    let inverse = crate::affine::inverse(&frame)?;
    let local_region = region.copy()?;
    local_region.transform(&inverse)?;
    let (lower, upper) = local_region.bounds()?;
    let span = [lower[0], upper[0]];
    let mut intervals = pattern(span, count).map_err(crate::error::status)?;
    allowances(&mut intervals, span, edge_offset, clearance).map_err(crate::error::status)?;
    Ok(intervals)
}

/// Whether two placed bodies still have a positive-volume intersection in feature coordinates.
///
/// # Errors
/// The errors transforming or intersecting the bodies.
pub fn pair_intersects(
    first: &crate::Body,
    second: &crate::Body,
    first_world: [[f64; 4]; 4],
    second_world: [[f64; 4]; 4],
    feature_world: [[f64; 4]; 4],
) -> std::result::Result<bool, Failure> {
    let inverse_feature = crate::affine::inverse(&feature_world)?;
    let first_to_feature = crate::affine::then(&inverse_feature, &first_world)?;
    let second_to_feature = crate::affine::then(&inverse_feature, &second_world)?;
    let first = first.copy()?;
    let second = second.copy()?;
    at_stage(
        "transforming the first plate for the post-cut intersection check",
        first.transform(&first_to_feature),
    )?;
    at_stage(
        "transforming the second plate for the post-cut intersection check",
        second.transform(&second_to_feature),
    )?;
    Ok(at_stage(
        "checking the post-cut intersection",
        crate::boolean::boolean(first, second, crate::BooleanOp::Intersect),
    )?
    .is_some())
}

/// Build one plate's cutter boxes in the shared feature frame.
fn cutters_in_frame(
    overlap: &crate::Body,
    plate: &crate::Body,
    frame: [[f64; 4]; 4],
    intervals: &[Interval],
    retain_first: bool,
    clearance: f64,
    clip_stepped_footprint: bool,
) -> Result<Vec<crate::Body>> {
    let inverse = crate::affine::inverse(&frame)?;
    let local_overlap = overlap.copy()?;
    let local_plate = plate.copy()?;
    local_overlap.transform(&inverse)?;
    local_plate.transform(&inverse)?;
    let (overlap_lower, overlap_upper) = local_overlap.bounds()?;
    let (plate_lower, plate_upper) = local_plate.bounds()?;
    let clip_to_overlap = if clip_stepped_footprint {
        let size = solidcore::finger_joint::bounds_size(overlap_lower, overlap_upper)
            .ok_or(crate::Error::Argument)?;
        let bounds_box = crate::make_box(overlap_lower, size)?;
        crate::boolean::boolean(
            bounds_box,
            local_overlap.copy()?,
            crate::BooleanOp::Subtract,
        )?
        .is_some()
    } else {
        false
    };
    let mut tools = Vec::new();
    for interval in intervals {
        if interval.retained_by_first == retain_first {
            continue;
        }
        let (origin, size) = cutter_box(
            *interval,
            plate_lower,
            plate_upper,
            overlap_lower,
            overlap_upper,
            clearance,
        )
        .map_err(crate::error::status)?;
        let tool = crate::make_box(origin, size)?;
        let tool = if clip_to_overlap {
            let Some(clipped) =
                crate::boolean::boolean(tool, local_overlap.copy()?, crate::BooleanOp::Intersect)?
            else {
                continue;
            };
            clipped
        } else {
            tool
        };
        tool.transform(&frame)?;
        tools.push(tool);
    }
    Ok(tools)
}

/// The cutters gathered without sewing, or `None` when a side has no slots to cut.
fn compound_tools(tools: &[crate::Body]) -> Result<Option<crate::Body>> {
    match tools {
        [] => Ok(None),
        [tool] => tool.copy().map(Some),
        _ => crate::stitch::gather(&tools.iter().collect::<Vec<_>>()).map(Some),
    }
}
