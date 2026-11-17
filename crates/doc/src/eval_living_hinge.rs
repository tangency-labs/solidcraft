//! The living hinge feature (#17484): solidcore plans staggered slots over the sketch's area inside
//! the plate's outline, each slot becomes a prism through the plate along the sketch normal, and
//! the prisms, apart from each other, are one tool: the plate is cut once, not once per slot.

use super::*;
use solidcore_record::Loop2;
use solidcore_record::living_hinge::{LivingHingePlan, LivingHingeSpec};

/// The most slots one living hinge cuts into a body. The kernel's boolean and its check take time
/// that grows with the square of the faces they meet, as for a cell grid's cells.
pub const MAX_CUT_SLOTS: usize = 250;

/// Past this many slots the cut takes noticeable time, and the feature says so.
pub const SLOW_CUT_SLOTS: usize = 100;

/// What a living hinge made: the counts of its plan.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HingeReport {
    /// Slots cut.
    pub slots: usize,
    /// Rows holding at least one slot.
    pub rows: usize,
    /// Lattice slots that met the material and were left out.
    pub left_out: usize,
}

impl HingeReport {
    /// The counts of `plan`.
    #[must_use]
    pub fn of(plan: &LivingHingePlan) -> Self {
        Self { slots: plan.slots.len(), rows: plan.rows, left_out: plan.left_out }
    }
}

/// The inputs of a living hinge, resolved: the pattern, the slots' angle (radians from the
/// sketch's X axis), the anchor in the sketch plane, the area's loops and the sketch.
pub(crate) struct HingeInputs {
    pub spec: LivingHingeSpec,
    pub angle: f64,
    pub anchor: Option<Vec2>,
    pub field: Vec<Loop2>,
    pub sketch: SolvedSketch,
}

/// The pattern and the area of the living hinge `f`.
pub(crate) fn spec_and_field(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<HingeInputs> {
    let FeatureKind::LivingHinge { sketch, profiles, slot_length, bridge, row_spacing, slot_width, angle, anchor, clip, margin, .. } = &f.kind else {
        return Err(DocError::Invalid("not a living hinge".into()));
    };
    let ss = constructor_profile_in_frame(
        doc,
        st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
        f,
    )?;
    let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
    let field: Vec<Loop2> = regions.iter().flat_map(|r| r.loops().cloned()).collect();
    let defaults = kernel::living_hinge::default_spec();
    let spec = LivingHingeSpec {
        slot_length: val(vals, slot_length, Kind::Length)?,
        bridge: val(vals, bridge, Kind::Length)?,
        row_spacing: val(vals, row_spacing, Kind::Length)?,
        slot_width: match slot_width {
            Some(e) => val(vals, e, Kind::Length)?,
            None => defaults.slot_width,
        },
        margin: match margin {
            Some(e) => val(vals, e, Kind::Length)?,
            None => defaults.margin,
        },
        clip: *clip,
    };
    let angle = match angle {
        Some(e) => val(vals, e, Kind::Angle)?,
        None => 0.0,
    };
    // The anchor is a point of the component's space: its place in the sketch's plane.
    let anchor = match anchor {
        Some(p) => Some(ss.plane.to_local(point(vals, p)?)),
        None => None,
    };
    Ok(HingeInputs { spec, angle, anchor, field, sketch: ss })
}

/// The plate's outline seen along `plane`'s normal: the loops of every body of `scope` in `shown`.
/// A body with no flat face facing along the normal is refused; one with faces neither flat nor
/// square to the sketch is read from its flat faces, with a warning.
pub(crate) fn plate(shown: &ModelState, plane: &Plane, scope: &[String], warning: &mut Option<String>) -> Result<Vec<Loop2>> {
    let mut loops = Vec::new();
    let mut any = false;
    for b in shown.bodies.iter().filter(|b| scope.contains(&b.name)) {
        any = true;
        let outline = kernel::living_hinge::plate_outline(&b.body, plane)?;
        if outline.regions.is_empty() {
            return Err(DocError::Invalid(format!("{} has no flat face parallel to the sketch: a living hinge cuts a flat plate", b.name)));
        }
        if outline.other > 0 {
            append_warning(
                warning,
                format!(
                    "{} has faces that are neither flat nor square to the sketch (a chamfer, a round edge): its outline is read from its flat faces only",
                    b.name
                ),
            );
        }
        loops.extend(outline.regions.iter().flat_map(|r| r.loops().cloned()));
    }
    if !any {
        return Err(DocError::Invalid("a living hinge needs a plate to cut".into()));
    }
    Ok(loops)
}

/// The slots over `field` in `plate`, checked against the cap.
pub(crate) fn plan(inputs: &HingeInputs, plate: &[Loop2], warning: &mut Option<String>) -> Result<LivingHingePlan> {
    let plan = kernel::living_hinge::plan(&inputs.field, plate, &inputs.spec, inputs.angle, inputs.anchor)?;
    if plan.slots.is_empty() {
        return Err(DocError::Invalid("no slot fits in the area: use shorter slots, a smaller bridge or margin, or a larger area".into()));
    }
    if plan.slots.len() > MAX_CUT_SLOTS {
        return Err(DocError::Invalid(format!(
            "the hinge has {} slots, more than the {MAX_CUT_SLOTS} a living hinge cuts into a body: use longer slots, a wider row spacing or a smaller area",
            plan.slots.len()
        )));
    }
    if plan.slots.len() > SLOW_CUT_SLOTS {
        append_warning(warning, format!("{} slots: cutting more than {SLOW_CUT_SLOTS} takes a while", plan.slots.len()));
    }
    Ok(plan)
}

/// The cutting tool of `plan`: every slot a prism from `-reach` to `reach` along the normal of
/// `plane`, as one body. The slots are in the hinge frame, the sketch frame turned by `angle`.
pub(crate) fn cut(plane: &Plane, angle: f64, plan: &LivingHingePlan, slot_width: f64, reach: f64) -> Result<Vec<Traced>> {
    let (s, c) = angle.sin_cos();
    let frame = Plane { origin: plane.origin, x: plane.x * c + plane.y * s, y: plane.y * c - plane.x * s };
    // The slots lie apart: the planner keeps material between them.
    let regions = plan.slots.iter().map(|slot| kernel::living_hinge::slot_region(slot, slot_width)).collect::<std::result::Result<Vec<_>, _>>()?;
    cell_grid::prism_tool(&frame, &regions, -reach, reach)
}

/// The tool of a living hinge feature and its report: plan, checks, cut.
pub(crate) fn tools(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    warning: &mut Option<String>,
) -> Result<(Vec<Traced>, HingeReport)> {
    let FeatureKind::LivingHinge { targets, .. } = &f.kind else {
        return Err(DocError::Invalid("not a living hinge".into()));
    };
    // Sheet metal cuts become cut-outs of the flat pattern, which slots cut through a solid are not.
    let (_, scope) = boolean_scope(st, f, &crate::plain(targets));
    if st.sheets.iter().any(|sheet| scope.contains(&sheet.body)) {
        return Err(DocError::Invalid("not supported yet: a living hinge cannot cut sheet metal".into()));
    }
    let inputs = spec_and_field(doc, vals, f, st)?;
    let shown = targets_as_shown(st, f, &crate::plain(targets))?;
    let plane = inputs.sketch.plane;
    let outline = plate(&shown, &plane, &scope, warning)?;
    let plan = plan(&inputs, &outline, warning)?;
    let reach = cell_grid::exact_reach(&shown, &plane, &scope)?;
    let tools = cut(&plane, inputs.angle, &plan, inputs.spec.slot_width, reach)?;
    Ok((tools, HingeReport::of(&plan)))
}
