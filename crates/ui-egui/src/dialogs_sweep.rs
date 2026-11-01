//! The options Sweep and Surface Sweep share (#771): how the profile turns along the path, a
//! twist over the path's length, the profile's scale at the end, and guide rails with how they
//! reshape the profile. Loft and Surface Loft take the same guide rails (#201).

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Expr, Mat, RailFit, SweepOrientation, SweepRail};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::selection::{AXES, CURVES, EDGES, SelInput};

/// The orientation choices, in combo order: perpendicular, parallel, a direction.
const ORIENTATIONS: [&str; 3] = ["Turn with Path", "Keep as Drawn", "Follow Direction"];
/// The orientation choice that takes a picked direction.
const DIRECTION: usize = 2;
/// A twist field that means no twist.
const NO_TWIST: &str = "0 deg";
/// An end scale field that means no scaling.
const NO_SCALE: &str = "1";
/// The rail fits, in combo order: scale, stretch.
const RAIL_FITS: [&str; 2] = ["Scale", "Stretch"];

/// A sweep dialog's options.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepOpts {
    /// An index into [`ORIENTATIONS`].
    pub orientation: usize,
    pub twist: String,
    pub scale: String,
    /// A stored direction (world), used while no direction is picked.
    pub direction: Option<Vec3>,
    /// An index into [`RAIL_FITS`].
    pub rail_fit: usize,
}

impl Default for SweepOpts {
    fn default() -> Self {
        SweepOpts { orientation: 0, twist: NO_TWIST.into(), scale: NO_SCALE.into(), direction: None, rail_fit: 0 }
    }
}

impl SweepOpts {
    /// A feature's stored options as dialog values; `frame` maps the feature's component frame to
    /// the world (a stored direction is in the component's frame). A direction along an origin
    /// axis comes back as a pick of that axis.
    pub fn of(
        orientation: &SweepOrientation,
        twist: &Option<Expr>,
        scale: &Option<Expr>,
        rail_fit: RailFit,
        frame: &Mat,
    ) -> (SweepOpts, Option<Sel>) {
        let mut o = SweepOpts {
            twist: twist.as_ref().map_or_else(|| NO_TWIST.into(), |t| t.to_string()),
            scale: scale.as_ref().map_or_else(|| NO_SCALE.into(), |k| k.to_string()),
            rail_fit: rail_fit_index(rail_fit),
            ..SweepOpts::default()
        };
        let mut pick = None;
        match orientation {
            SweepOrientation::Perpendicular => {}
            SweepOrientation::Parallel => o.orientation = 1,
            SweepOrientation::Direction(d) => {
                o.orientation = DIRECTION;
                let world = solidcraft_engine::doc::apply_vector(frame, d.0);
                match world.normalized().and_then(world_axis) {
                    Some(name) => pick = Some(Sel::Axis { name: name.into() }),
                    None => o.direction = Some(world),
                }
            }
        }
        (o, pick)
    }
}

/// The origin axis a direction runs along (its way), if any.
fn world_axis(d: Vec3) -> Option<&'static str> {
    [("X", Vec3::X), ("Y", Vec3::Y), ("Z", Vec3::Z)].into_iter().find(|(_, a)| d.cross(*a).len() < 1e-9 && d.dot(*a) > 0.0).map(|(n, _)| n)
}

/// The input that picks a Follow Direction sweep's direction: an axis, a sketch line or a
/// straight edge.
pub fn direction_input() -> SelInput {
    SelInput::new("Direction", AXES | EDGES, false)
}

/// The inputs that pick a sweep's guide rails: the first rail's curves and the second's, in order
/// along each rail; both optional.
pub fn rail_inputs() -> [SelInput; 2] {
    [SelInput::new("Rail", CURVES, true), SelInput::new("Second Rail", CURVES, true)]
}

/// A feature's stored rails as the rail inputs' selections (`active`: the sketch being edited).
pub fn rail_sels(rails: &[SweepRail], active: Option<u64>) -> [Vec<Sel>; 2] {
    let sels =
        |i: usize| rails.get(i).map(|r| r.curves.iter().map(|id| crate::selection::curve_pick(id, r.sketch.0, active)).collect()).unwrap_or_default();
    [sels(0), sels(1)]
}

/// The option rows; true when Enter was pressed in a field.
pub fn rows(ui: &mut egui::Ui, o: &mut SweepOpts, id: &str) -> bool {
    rows_with(ui, o, id, |_| {})
}

/// [`rows`] with extra rows between the orientation and the collapsed Advanced section, so the
/// feature's own rows (such as its operation) stay above the section.
pub fn rows_with(ui: &mut egui::Ui, o: &mut SweepOpts, id: &str, between: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut enter = false;
    choice_row(ui, "Orientation", &format!("{id}_orient"), &mut o.orientation, &ORIENTATIONS);
    between(ui);
    crate::ui_kit::section(ui, id, "Advanced", false, |ui| {
        enter |= expr_row(ui, "Twist", &mut o.twist);
        enter |= expr_row(ui, "End Scale", &mut o.scale);
        rail_fit_row(ui, id, &mut o.rail_fit);
    });
    enter
}

/// The Rail Fit row: `fit` an index into [`RAIL_FITS`].
pub fn rail_fit_row(ui: &mut egui::Ui, id: &str, fit: &mut usize) {
    choice_row(ui, "Rail Fit", &format!("{id}_rail_fit"), fit, &RAIL_FITS);
}

/// A stored rail fit as an index into [`RAIL_FITS`].
pub fn rail_fit_index(fit: RailFit) -> usize {
    usize::from(fit == RailFit::Stretch)
}

/// The picked rails (each in the sketch its curves are in) and the fit (`fit`, an index into
/// [`RAIL_FITS`]), added to `p`; whether any rail was picked.
pub fn rail_params(app: &SolveApp, fit: usize, rails: [&[Sel]; 2], p: &mut Value) -> Result<bool, String> {
    let mut given = Vec::new();
    for sels in rails {
        let curves: Vec<String> = sels.curve_ids();
        if curves.is_empty() {
            continue;
        }
        let sketch = crate::dialogs::picks_sketch(app, sels, None).ok_or("each rail's curves must be in one sketch")?;
        given.push(json!({"sketch": sketch, "curves": curves}));
    }
    if given.is_empty() {
        return Ok(false);
    }
    p["rails"] = json!(given);
    if fit == 1 {
        p["rail_fit"] = json!("stretch");
    }
    Ok(true)
}

/// The options as command parameters, added to `p`: the orientation, and the twist and end
/// scale when they change anything; with rails picked, the rails (each in the sketch its curves
/// are in) and the fit instead. `picked` is the direction input's selection, `rails` the rail
/// inputs'.
pub fn params(app: &SolveApp, o: &SweepOpts, picked: Option<&Sel>, rails: [&[Sel]; 2], p: &mut Value) -> Result<(), String> {
    // Rails set the profile's turn and size: the other options don't apply.
    if rail_params(app, o.rail_fit, rails, p)? {
        return Ok(());
    }
    match o.orientation {
        0 => {}
        1 => p["orientation"] = json!("parallel"),
        _ => {
            let d = match (picked, o.direction) {
                (Some(x), _) => crate::dialogs::axis_of(app, x).ok_or("the direction must be an axis, a sketch line or a straight edge")?.1,
                (None, Some(d)) => d,
                (None, None) => return Err("select a direction (an axis, a sketch line or a straight edge) for the profile to follow".into()),
            };
            p["orientation"] = json!([d.x, d.y, d.z]);
        }
    }
    let twist = o.twist.trim();
    if !twist.is_empty() && twist != NO_TWIST {
        p["twist"] = json!(twist);
    }
    let scale = o.scale.trim();
    if !scale.is_empty() && scale != NO_SCALE {
        p["scale"] = json!(scale);
    }
    Ok(())
}
