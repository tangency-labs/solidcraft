//! Shaft Spline (`shaft_spline.insert`): a spline cut into a picked shaft, a picked bore or both,
//! or, with nothing picked, a new splined shaft, the hub that fits it, or both, with an optional
//! master spline. Two optional picks (Shaft, Bore), then the kind of teeth and what to make, the
//! count, the length and the numbers that size the teeth; under them, in groups that fold, the rest
//! of the tooth's form, the fit, the tip corners, the chamfers, the groove ends, the master spline
//! and the hub's outside, each row one parameter of the command. The rows are sent as typed, so the
//! feature's numbers stay parameters. A drawing at the top shows the outlines the command would
//! make, worked out by the engine's own preview ([`shaft_spline_preview`]) from the rows' values,
//! the same function the command runs before it builds, so the dialog refuses what the command
//! refuses, in the same words. A double-click on the feature in the History opens it again filled
//! in ([`reopen`]).

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::cmd::{SplineCircle, SplinePart, SplinePreview, shaft_spline_preview};
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{Document, Expr, Feature, FeatureKind, SplineCorner, SplineForm, SplineMake, SplineMaster};
use solidcraft_engine::schema::shaft_spline::MasterSplineParam;
use solidcraft_engine::{Sel, Session};
use solidcraft_theme::Tokens;
use solidcraft_ui_text::{format, tr};

use crate::SolveApp;
use crate::dialogs::{CHAMFER_TYPES, Dialog, Kind, face_sel, face_sel_on, form_row, named_refill, shown_expr};
use crate::selection::{FACES, SelInput, point_value as pt};
use crate::ui_kit::BackingTooltipExt as _;

/// The command the dialog fills in.
pub const COMMAND: &str = "shaft_spline.insert";

/// The pick inputs, in the dialog's order: the outside of a shaft and the face its teeth start from, then a
/// bore and the face its teeth start from.
const SHAFT: usize = 0;
const SHAFT_START: usize = 1;
const BORE: usize = 2;
const BORE_START: usize = 3;
/// The pick inputs' labels and tips (message ids).
const PICK_SHAFT: &str = "shaft_spline.pick_shaft";
const PICK_SHAFT_START: &str = "shaft_spline.pick_shaft_start";
const PICK_BORE: &str = "shaft_spline.pick_bore";
const PICK_BORE_START: &str = "shaft_spline.pick_bore_start";

/// The faces picked: the outside of a shaft and a bore, each optional, and for each the face its teeth start
/// from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Picks<'a> {
    pub shaft: Option<&'a Sel>,
    pub shaft_start: Option<&'a Sel>,
    pub bore: Option<&'a Sel>,
    pub bore_start: Option<&'a Sel>,
}

impl<'a> Picks<'a> {
    /// The faces in the dialog's pick inputs.
    pub fn of(inputs: &'a [SelInput]) -> Picks<'a> {
        Picks {
            shaft: face_in(inputs, SHAFT),
            shaft_start: face_in(inputs, SHAFT_START),
            bore: face_in(inputs, BORE),
            bore_start: face_in(inputs, BORE_START),
        }
    }

    /// Whether a face is picked: the spline is then cut into it, and no new part is made.
    pub fn any(&self) -> bool {
        self.shaft.is_some() || self.bore.is_some()
    }
}

/// The face picked into input `i`.
fn face_in(inputs: &[SelInput], i: usize) -> Option<&Sel> {
    inputs.get(i)?.items.iter().find(|s| matches!(s, Sel::Face { .. }))
}

/// A picked face as the command's point, or the words saying it must be picked again (the face
/// was not found again when the dialog reopened).
fn face_point(sel: &Sel, label: &str) -> Result<Value, Bad> {
    match sel {
        Sel::Face { index, .. } if *index == usize::MAX => {
            Err(Bad { key: None, words: solidcraft_engine::doc::DocError::reselect(tr(label)).to_string() })
        }
        Sel::Face { point, .. } => Ok(pt(*point)),
        _ => Err(Bad { key: None, words: solidcraft_engine::doc::DocError::reselect(tr(label)).to_string() }),
    }
}

/// The radius of the round shaft face picked, read from its exact surface (none for a bore or a
/// face that is not round).
fn picked_radius(s: &Session, sel: &Sel) -> Option<f64> {
    let Sel::Face { body, point, .. } = sel else { return None };
    let st = crate::open_meshing::session_world(s);
    solidcraft_engine::kernel::cylinder_face_at(&st.body(body)?.body, *point).filter(|c| !c.internal).map(|c| c.radius)
}

/// Whether the round face picked is the inside of a bore (false: the outside of a shaft; none for a
/// face that is not round).
fn picked_inside(s: &Session, sel: &Sel) -> Option<bool> {
    let Sel::Face { body, point, .. } = sel else { return None };
    let st = crate::open_meshing::session_world(s);
    solidcraft_engine::kernel::cylinder_face_at(&st.body(body)?.body, *point).map(|c| c.internal)
}

/// What each pick row holds, in the rows' order.
fn picked(inputs: &[SelInput]) -> [Option<Sel>; 4] {
    [SHAFT, SHAFT_START, BORE, BORE_START].map(|i| inputs.get(i).and_then(|i| i.items.first().cloned()))
}

/// Exchanges what two pick rows hold.
fn swap_rows(inputs: &mut [SelInput], a: usize, b: usize) {
    let (Some(first), Some(second)) = (inputs.get(a).map(|i| i.items.clone()), inputs.get(b).map(|i| i.items.clone())) else { return };
    if let Some(row) = inputs.get_mut(a) {
        row.items = second;
    }
    if let Some(row) = inputs.get_mut(b) {
        row.items = first;
    }
}

/// Puts each face in the pick row it belongs in, whichever row took it: the inside of a bore in
/// Bore, the outside of a shaft in Shaft. Faces selected before the dialog opened land in the first
/// rows in order, and a click lands in the row that is waiting. A round face is moved when its own
/// row is free, and two round faces each in the other's row change places, the faces their teeth
/// start from with them. A round face of the other kind in a start row, its own row free, is that
/// part's round face. A face that is not round, taken by a round face's row while the other part
/// waits for the face its teeth start from, is that face. None while the picks are as they were at
/// the last look; else whether a face was moved. The faces are read once for each set of picks,
/// not on every frame.
pub fn sort_picks(app: &SolveApp, s: &mut Spline, inputs: &mut [SelInput]) -> Option<bool> {
    if s.sorted.as_ref() == Some(&picked(inputs)) {
        return None;
    }
    let inside = |sel: &Option<Sel>| sel.as_ref().and_then(|sel| picked_inside(&app.session, sel));
    let flat = |sel: &Option<Sel>| sel.is_some() && inside(sel).is_none();
    let mut moved = false;
    // Each move puts a face in a free row or exchanges two parts' rows, so a few passes settle any picks.
    for _ in 0..4 {
        let [shaft, shaft_start, bore, bore_start] = picked(inputs);
        let (in_shaft, in_bore) = (inside(&shaft), inside(&bore));
        let crossed = (in_shaft, in_bore) == (Some(true), Some(false));
        let step: &[(usize, usize)] = if crossed || (in_shaft == Some(true) && bore.is_none()) || (in_bore == Some(false) && shaft.is_none()) {
            // A bore in Shaft with Bore free, a shaft in Bore with Shaft free, or each in the other's row.
            &[(SHAFT, BORE), (SHAFT_START, BORE_START)]
        } else if inside(&shaft_start) == Some(true) && bore.is_none() {
            &[(SHAFT_START, BORE)]
        } else if inside(&bore_start) == Some(false) && shaft.is_none() {
            &[(BORE_START, SHAFT)]
        } else if flat(&shaft) && in_bore == Some(true) && bore_start.is_none() {
            &[(SHAFT, BORE_START)]
        } else if flat(&bore) && in_shaft == Some(false) && shaft_start.is_none() {
            &[(BORE, SHAFT_START)]
        } else {
            break;
        };
        for (a, b) in step {
            swap_rows(inputs, *a, *b);
        }
        moved = true;
    }
    let [shaft, _, bore, _] = picked(inputs);
    s.not_round = [shaft.is_some() && inside(&shaft) != Some(false), bore.is_some() && inside(&bore) != Some(true)];
    s.sorted = Some(picked(inputs));
    Some(moved)
}

/// The pick rows to show, as runs of the dialog's inputs: each round face's row, and after it the
/// row of the face its teeth start from once the round face is picked (or while that row holds a
/// face).
pub fn shown_picks(inputs: &[SelInput]) -> Vec<std::ops::Range<usize>> {
    let filled = |i: usize| inputs.get(i).is_some_and(|i| !i.items.is_empty());
    let mut rows = Vec::new();
    for (face, start) in [(SHAFT, SHAFT_START), (BORE, BORE_START)] {
        rows.push(face..face + 1);
        if filled(face) || filled(start) {
            rows.push(start..start + 1);
        }
    }
    rows
}

/// The pick row that waits for the next click: the face a picked part's teeth start from, before
/// any other row that is free.
pub fn next_pick(inputs: &[SelInput]) -> Option<usize> {
    let filled = |i: usize| inputs.get(i).is_some_and(|i| !i.items.is_empty());
    [(SHAFT, SHAFT_START), (BORE, BORE_START)]
        .into_iter()
        .find(|(face, start)| filled(*face) && !filled(*start))
        .map(|(_, start)| start)
        .or_else(|| inputs.iter().position(|i| i.items.is_empty()))
}

/// The length along its axis of the round face picked, the inside of a bore (`inside`) or the
/// outside of a shaft (none for a face of the other kind or one that is not round): a spline
/// cannot be longer.
fn picked_room(s: &Session, sel: &Sel, inside: bool) -> Option<f64> {
    let Sel::Face { body, point, .. } = sel else { return None };
    let st = crate::open_meshing::session_world(s);
    solidcraft_engine::kernel::cylinder_face_at(&st.body(body)?.body, *point).filter(|c| c.internal == inside).map(|c| c.end - c.start)
}

/// The round face of the kind a pick row holds (`inside`: a bore, else the outside of a shaft) at
/// the saved place `p`, on whichever body has one there.
fn round_face_of_kind(s: &Session, p: Vec3, inside: bool) -> Option<Sel> {
    let st = crate::open_meshing::session_world(s);
    let body = st.bodies.iter().find(|b| solidcraft_engine::kernel::cylinder_face_at(&b.body, p).is_some_and(|c| c.internal == inside))?;
    face_sel_on(s, Some(&body.name), p)
}

/// The Length the dialog opens with (mm): a picked shaft then sets its own diameter.
const FIRST_LENGTH: f64 = 20.0;

/// The kinds of teeth: the command's words and the labels.
const TYPES: [(&str, &str); 3] =
    [("involute", "shaft_spline.type_involute"), ("straight", "shaft_spline.type_straight"), ("serration", "shaft_spline.type_serration")];
/// What to make.
const MAKES: [(&str, &str); 3] = [("shaft", "shaft_spline.make_shaft"), ("hub", "shaft_spline.make_hub"), ("both", "shaft_spline.make_both")];
/// How the tips' corners are finished.
const CORNERS: [(&str, &str); 3] =
    [("plain", "shaft_spline.corner_plain"), ("chamfered", "shaft_spline.corner_chamfered"), ("rounded", "shaft_spline.corner_rounded")];
/// The forms of a master spline.
const FORMS: [(&str, &str); 2] = [("tooth_out", "shaft_spline.form_tooth_out"), ("wide_tooth", "shaft_spline.form_wide_tooth")];
/// What the drawing shows.
const SHOWS: [&str; 3] = ["shaft_spline.show_whole", "shaft_spline.show_teeth", "shaft_spline.master"];

/// How a row's text is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    /// A length in the design's unit, sent in mm.
    Length,
    /// An angle, sent in degrees.
    Angle,
    /// A number without a unit (a share of the module).
    Number,
}

/// A number row: its label (a message id), the command's key, how its text is read and its help.
#[derive(Clone, Copy, Debug)]
struct Field {
    label: &'static str,
    key: &'static str,
    unit: Unit,
    help: &'static str,
}

const fn field(label: &'static str, key: &'static str, unit: Unit, help: &'static str) -> Field {
    Field { label, key, unit, help }
}

const LENGTH: Field = field("shaft_spline.length", "length", Unit::Length, "shaft_spline.length_help");
const INVOLUTE: [Field; 7] = [
    field("shaft_spline.module", "module", Unit::Length, "shaft_spline.module_help"),
    field("shaft_spline.pressure_angle", "pressure_angle", Unit::Angle, "shaft_spline.pressure_angle_help"),
    field("shaft_spline.addendum", "addendum", Unit::Number, "shaft_spline.addendum_help"),
    field("shaft_spline.dedendum", "dedendum", Unit::Number, "shaft_spline.dedendum_help"),
    field("shaft_spline.root_fillet", "root_fillet", Unit::Number, "shaft_spline.root_fillet_help"),
    field("shaft_spline.profile_shift", "profile_shift", Unit::Number, "shaft_spline.profile_shift_help"),
    field("shaft_spline.thinning", "thinning", Unit::Length, "shaft_spline.thinning_help"),
];
const MAJOR: Field = field("shaft_spline.major_diameter", "major_diameter", Unit::Length, "shaft_spline.major_diameter_help");
const MINOR: Field = field("shaft_spline.minor_diameter", "minor_diameter", Unit::Length, "shaft_spline.minor_diameter_help");
const ROOT_ROUND: Field = field("shaft_spline.root_round", "root_round", Unit::Length, "shaft_spline.root_round_help");
const STRAIGHT: [Field; 4] =
    [MAJOR, MINOR, field("shaft_spline.tooth_width", "tooth_width", Unit::Length, "shaft_spline.tooth_width_help"), ROOT_ROUND];
const SERRATION: [Field; 4] =
    [MAJOR, MINOR, field("shaft_spline.flank_angle", "flank_angle", Unit::Angle, "shaft_spline.flank_angle_help"), ROOT_ROUND];
const SIDE: Field = field("shaft_spline.side_clearance", "side_clearance", Unit::Length, "shaft_spline.side_clearance_help");
const HUB_ROOT: Field = field("shaft_spline.hub_root_round", "hub_root_round", Unit::Length, "shaft_spline.hub_root_round_help");
const FIT_INVOLUTE: [Field; 5] = [
    field("shaft_spline.side_clearance", "side_clearance", Unit::Length, "shaft_spline.side_clearance_involute_help"),
    field("shaft_spline.hub_addendum", "hub_addendum", Unit::Number, "shaft_spline.hub_addendum_help"),
    field("shaft_spline.tip_clearance", "tip_clearance", Unit::Length, "shaft_spline.tip_clearance_help"),
    HUB_ROOT,
    field("shaft_spline.form_clearance", "form_clearance", Unit::Length, "shaft_spline.form_clearance_help"),
];
const FIT_STRAIGHT: [Field; 5] = [
    SIDE,
    field("shaft_spline.major_clearance", "major_clearance", Unit::Length, "shaft_spline.major_clearance_help"),
    field("shaft_spline.minor_clearance", "minor_clearance", Unit::Length, "shaft_spline.minor_clearance_help"),
    HUB_ROOT,
    field("shaft_spline.hub_tip_round", "hub_tip_round", Unit::Length, "shaft_spline.hub_tip_round_help"),
];
const TIP_SIZE: Field = field("shaft_spline.corner_size", "tip_size", Unit::Length, "shaft_spline.corner_size_help");
const MASTER_ANGLE: Field = field("shaft_spline.master_angle", "master_angle", Unit::Angle, "shaft_spline.master_angle_help");
const HUB_DIAMETER: Field = field("shaft_spline.hub_diameter", "hub_diameter", Unit::Length, "shaft_spline.hub_diameter_help");
const CUTTER: Field = field("shaft_spline.cutter_diameter", "cutter_diameter", Unit::Length, "shaft_spline.cutter_diameter_help");
const END_CHAMFER: Field = field("shaft_spline.end_chamfer", "end_chamfer", Unit::Length, "shaft_spline.end_chamfer_help");
const ENTRY_CHAMFER: Field = field("shaft_spline.entry_chamfer", "entry_chamfer", Unit::Length, "shaft_spline.entry_chamfer_help");

/// A chamfer's rows: its size, its Type (the key of the choice's handle) and the row each type past Equal Distance
/// adds under it.
#[derive(Clone, Copy, Debug)]
struct ChamferRows {
    size: Field,
    choice: &'static str,
    distance2: Field,
    angle: Field,
}

impl ChamferRows {
    /// The row a chamfer of type `kind` (an index into [`CHAMFER_TYPES`]) adds under its size: a second distance
    /// for Two Distances, an angle for Distance and Angle, none for Equal Distance.
    fn second(&self, kind: usize) -> Option<Field> {
        match kind {
            1 => Some(self.distance2),
            2 => Some(self.angle),
            _ => None,
        }
    }
}

/// The chamfer on the ends of the shaft's teeth.
const END_CHAMFERS: ChamferRows = ChamferRows {
    size: END_CHAMFER,
    choice: "end_chamfer_type",
    distance2: field("ui.distance_2", "end_chamfer_distance2", Unit::Length, "shaft_spline.end_chamfer_distance2_help"),
    angle: field("ui.angle", "end_chamfer_angle", Unit::Angle, "shaft_spline.end_chamfer_angle_help"),
};
/// The chamfer at the entries of the hub's bore.
const ENTRY_CHAMFERS: ChamferRows = ChamferRows {
    size: ENTRY_CHAMFER,
    choice: "entry_chamfer_type",
    distance2: field("ui.distance_2", "entry_chamfer_distance2", Unit::Length, "shaft_spline.entry_chamfer_distance2_help"),
    angle: field("ui.angle", "entry_chamfer_angle", Unit::Angle, "shaft_spline.entry_chamfer_angle_help"),
};

/// The label of the Teeth row.
const TEETH_LABEL: &str = "shaft_spline.teeth";

/// What a bad value is: the command's key it is for (when one), and the words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bad {
    pub key: Option<String>,
    pub words: String,
}

/// What the form's values make: the command's parameters and the engine's preview, or why not.
#[derive(Debug)]
pub struct Worked {
    /// The parameters as text: what the result was worked out for.
    key: String,
    pub result: Result<(Value, Arc<SplinePreview>), Bad>,
}

/// The Shaft Spline dialog's values. Each kind of teeth keeps its own numbers (`values`, by the
/// command's key), so switching back finds them again.
#[derive(Clone, Debug)]
pub struct Spline {
    /// Index into [`TYPES`].
    pub kind: usize,
    /// Index into [`MAKES`].
    pub make: usize,
    /// The numbers of each kind of teeth, by the command's key, as typed.
    pub values: [BTreeMap<&'static str, String>; 3],
    pub length: String,
    /// Index into [`CORNERS`], for straight-sided teeth and serrations.
    pub corner: [usize; 3],
    pub master: bool,
    /// Index into [`FORMS`].
    pub form: usize,
    pub master_angle: String,
    /// Index into [`SHOWS`].
    pub show: usize,
    /// What the values last made, kept by the parameters it was for.
    pub worked: Option<Arc<Worked>>,
    /// Why the command cannot be made though the teeth can be drawn: a row that goes to the command only (the
    /// cutter's diameter, a chamfer) that is not a value.
    pub command_bad: Option<Bad>,
    /// The last preview the values made, drawn dimmed while they make none.
    pub good: Option<Arc<SplinePreview>>,
    /// Whether each tooth space cut into a picked shaft ends in the curve the cutter leaves.
    pub cutter: bool,
    /// The cutter's diameter, as typed.
    pub cutter_diameter: String,
    /// The chamfer on the free end of a picked shaft's teeth, as typed (0: none).
    pub end_chamfer: String,
    /// The chamfer at the entry of a picked bore, as typed (0: none).
    pub entry_chamfer: String,
    /// How the end chamfer is set, an index into [`CHAMFER_TYPES`]: as far across the end as along the teeth (45
    /// degrees), a second distance across the end, or an angle to the round face.
    pub end_chamfer_type: usize,
    /// How far the end chamfer reaches across the end, as typed (Two Distances).
    pub end_chamfer_distance2: String,
    /// The end chamfer's angle to the round face, as typed (Distance and Angle).
    pub end_chamfer_angle: String,
    /// The same three for the entry chamfer.
    pub entry_chamfer_type: usize,
    pub entry_chamfer_distance2: String,
    pub entry_chamfer_angle: String,
    /// Whether the spline keeps a picked shaft's own diameter: the Module (involute teeth) or the
    /// Major diameter (straight-sided teeth, serrations) is then worked out from the shaft and not
    /// sent, so the command takes the shaft's diameter. Off to start with: the size row is then a
    /// number like any other, and the shaft's diameter follows it (more teeth of one size make a
    /// larger shaft).
    pub fit: bool,
    /// The text the dialog last put itself into each kind's size row (the Module of involute
    /// teeth, the Major diameter of the others): its starting value, then the size that fits a
    /// picked shaft as it is. A newly picked shaft sets the row again only while its text is
    /// still this, so a size the user typed stays. None when the rows come from a feature being
    /// edited.
    pub started_size: [Option<String>; 3],
    /// The shaft face picked last and its radius read from the exact surface (none when it could
    /// not be read), so the face is read once per pick and not on every frame.
    pub shaft_radius: Option<(Sel, Option<f64>)>,
    /// The bore face the rows last read (how long it is limits the Length the dialog puts in
    /// itself), so it too is read once per pick.
    pub bore_read: Option<Sel>,
    /// The Length the dialog last put in itself (its starting value, then a picked shaft's
    /// diameter, as far as the picked faces are long): a newly picked face sets the row again
    /// only while its text is still this, so a length the user typed stays. None when the row
    /// comes from a feature being edited.
    pub started_length: Option<String>,
    /// The same for the Cutter diameter (twice the picked shaft's diameter).
    pub started_cutter: Option<String>,
    /// The picks, in the rows' order, as they were when [`sort_picks`] last looked at them.
    pub sorted: Option<[Option<Sel>; 4]>,
    /// For the shaft, then the bore: whether OK may run with the part picked and no face its teeth start from.
    /// Only for a feature saved without one that is found again (a script may leave it out): it keeps the end
    /// it has until a face is picked. A new spline needs the face.
    pub start_optional: [bool; 2],
    /// For the shaft, then the bore of a reopened feature whose start face is not shown: the round face the
    /// dialog reopened with and the start the feature has saved. It is sent while that round face is picked and
    /// no start face is, so the spline keeps its end; a start picked in the row wins, and picking another round
    /// face drops it.
    pub saved_start: [Option<(Sel, Vec3)>; 2],
    /// For the Shaft row, then the Bore row: the row holds a face that is not a round face of its kind, as
    /// [`sort_picks`] last read it. The command then goes to the engine as it is, which says what is wrong
    /// with the pick; asking for the start face first would point at the wrong row.
    pub not_round: [bool; 2],
}

impl Spline {
    /// The dialog's values to start with: the prototype's examples (a 20-tooth involute, a
    /// 6-tooth straight-sided spline, a 36-tooth serration), with the command's own defaults
    /// where it has one. Lengths are shown in the design's unit.
    pub fn new(doc: &Document) -> Spline {
        let mm = |v: f64| crate::length_input::shown(doc, v);
        let deg = |v: f64| format!("{v} deg");
        let set = |pairs: &[(&'static str, String)]| pairs.iter().cloned().collect::<BTreeMap<_, _>>();
        let involute = set(&[
            ("teeth", "20".into()),
            ("module", mm(1.5)),
            ("pressure_angle", deg(30.0)),
            ("addendum", "0.5".into()),
            ("dedendum", "0.85".into()),
            ("root_fillet", "0.4".into()),
            ("profile_shift", "0".into()),
            ("thinning", mm(0.0)),
            ("side_clearance", mm(0.06)),
            ("hub_addendum", "0.5".into()),
            ("tip_clearance", mm(0.2)),
            ("hub_root_round", mm(0.0)),
            ("form_clearance", mm(0.0)),
            ("hub_diameter", mm(44.0)),
        ]);
        let straight = set(&[
            ("teeth", "6".into()),
            ("major_diameter", mm(28.0)),
            ("minor_diameter", mm(23.0)),
            ("tooth_width", mm(6.0)),
            ("root_round", mm(0.0)),
            ("tip_size", mm(0.3)),
            ("side_clearance", mm(0.05)),
            ("major_clearance", mm(0.3)),
            ("minor_clearance", mm(0.2)),
            ("hub_root_round", mm(0.0)),
            ("hub_tip_round", mm(0.0)),
            ("hub_diameter", mm(40.0)),
        ]);
        let serration = set(&[
            ("teeth", "36".into()),
            ("major_diameter", mm(20.0)),
            ("minor_diameter", mm(18.8)),
            ("flank_angle", deg(90.0)),
            ("root_round", mm(0.0)),
            ("tip_size", mm(0.1)),
            ("side_clearance", mm(0.02)),
            ("major_clearance", mm(0.05)),
            ("minor_clearance", mm(0.05)),
            ("hub_root_round", mm(0.0)),
            ("hub_tip_round", mm(0.0)),
            ("hub_diameter", mm(30.0)),
        ]);
        Spline {
            kind: 0,
            make: 0,
            values: [involute, straight, serration],
            length: mm(FIRST_LENGTH),
            corner: [0; 3],
            master: false,
            form: 0,
            master_angle: deg(0.0),
            show: 0,
            worked: None,
            command_bad: None,
            good: None,
            cutter: true,
            cutter_diameter: mm(40.0),
            end_chamfer: mm(0.0),
            entry_chamfer: mm(0.0),
            end_chamfer_type: 0,
            end_chamfer_distance2: mm(0.5),
            end_chamfer_angle: deg(45.0),
            entry_chamfer_type: 0,
            entry_chamfer_distance2: mm(0.5),
            entry_chamfer_angle: deg(45.0),
            fit: false,
            started_size: [Some(mm(1.5)), Some(mm(28.0)), Some(mm(20.0))],
            shaft_radius: None,
            bore_read: None,
            started_length: Some(mm(FIRST_LENGTH)),
            started_cutter: Some(mm(40.0)),
            sorted: None,
            start_optional: [false; 2],
            saved_start: [None, None],
            not_round: [false; 2],
        }
    }

    /// The text of the row for `key` of the kind chosen now (empty when it has none).
    pub fn value(&self, key: &str) -> &str {
        match key {
            "length" => &self.length,
            "master_angle" => &self.master_angle,
            "cutter_diameter" => &self.cutter_diameter,
            "end_chamfer" => &self.end_chamfer,
            "entry_chamfer" => &self.entry_chamfer,
            "end_chamfer_distance2" => &self.end_chamfer_distance2,
            "end_chamfer_angle" => &self.end_chamfer_angle,
            "entry_chamfer_distance2" => &self.entry_chamfer_distance2,
            "entry_chamfer_angle" => &self.entry_chamfer_angle,
            _ => self.values.get(self.kind).and_then(|v| v.get(key)).map_or("", String::as_str),
        }
    }

    /// Sets the text of the row for `key` of the kind chosen now.
    pub fn set(&mut self, key: &str, text: &str) {
        match key {
            "length" => self.length = text.to_string(),
            "master_angle" => self.master_angle = text.to_string(),
            "cutter_diameter" => self.cutter_diameter = text.to_string(),
            "end_chamfer" => self.end_chamfer = text.to_string(),
            "entry_chamfer" => self.entry_chamfer = text.to_string(),
            "end_chamfer_distance2" => self.end_chamfer_distance2 = text.to_string(),
            "end_chamfer_angle" => self.end_chamfer_angle = text.to_string(),
            "entry_chamfer_distance2" => self.entry_chamfer_distance2 = text.to_string(),
            "entry_chamfer_angle" => self.entry_chamfer_angle = text.to_string(),
            _ => {
                if let Some(known) = ALL_KEYS.iter().find(|k| **k == key)
                    && let Some(values) = self.values.get_mut(self.kind)
                {
                    values.insert(known, text.to_string());
                }
            }
        }
    }

    /// The text of the row for `key`, to edit (a key never typed starts empty).
    fn text_mut(&mut self, key: &'static str) -> &mut String {
        let [involute, straight, serration] = &mut self.values;
        match key {
            "length" => &mut self.length,
            "master_angle" => &mut self.master_angle,
            "cutter_diameter" => &mut self.cutter_diameter,
            "end_chamfer" => &mut self.end_chamfer,
            "entry_chamfer" => &mut self.entry_chamfer,
            "end_chamfer_distance2" => &mut self.end_chamfer_distance2,
            "end_chamfer_angle" => &mut self.end_chamfer_angle,
            "entry_chamfer_distance2" => &mut self.entry_chamfer_distance2,
            "entry_chamfer_angle" => &mut self.entry_chamfer_angle,
            _ => match self.kind {
                0 => involute,
                1 => straight,
                _ => serration,
            }
            .entry(key)
            .or_default(),
        }
    }

    /// The rows of numbers that apply now, in the dialog's order (the selections, the check boxes
    /// and the cutter apart). With a face `picked` no new hub is made, so it has no outside.
    fn fields(&self, picked: bool) -> Vec<Field> {
        let mut out = vec![LENGTH];
        let straight = self.kind != 0;
        match self.kind {
            0 => out.extend(INVOLUTE),
            1 => out.extend(STRAIGHT),
            _ => out.extend(SERRATION),
        }
        out.extend(if straight { FIT_STRAIGHT } else { FIT_INVOLUTE });
        if straight && self.corner.get(self.kind).is_some_and(|c| *c != 0) {
            out.push(TIP_SIZE);
        }
        if self.master {
            out.push(MASTER_ANGLE);
        }
        if self.make != 0 && !picked {
            out.push(HUB_DIAMETER);
        }
        out
    }

    /// The preview's parameters for the values as they are with nothing picked, or the first that
    /// is not a value.
    pub fn params(&self, doc: &Document) -> Result<Value, Bad> {
        self.params_for(doc, Picks::default())
    }

    /// The preview's parameters for the values as they are, as numbers (mm, degrees, plain), or
    /// the first that is not a value. With a face picked the preview draws the shaft's spline
    /// with the hub as its outline only (no new hub is made, so it has no outside diameter).
    pub fn params_for(&self, doc: &Document, picks: Picks<'_>) -> Result<Value, Bad> {
        let (kind, _) = TYPES.get(self.kind).copied().unwrap_or(TYPES[0]);
        let (make, _) = if picks.any() { MAKES[0] } else { MAKES.get(self.make).copied().unwrap_or(MAKES[0]) };
        let mut p = json!({"type": kind, "make": make});
        let teeth_text = self.value("teeth");
        let teeth = read(doc, teeth_text, Unit::Number).map_err(|why| bad("teeth", TEETH_LABEL, &why))?;
        if !(teeth >= 0.0 && teeth.fract() == 0.0 && teeth <= f64::from(u32::MAX)) {
            return Err(Bad { key: Some("teeth".into()), words: format("shaft_spline.not_whole", &[("label", tr(TEETH_LABEL))]) });
        }
        p["teeth"] = json!(teeth as u64);
        if self.kind != 0 {
            let (corner, _) = self.corner.get(self.kind).and_then(|c| CORNERS.get(*c)).copied().unwrap_or(CORNERS[0]);
            p["tip_corner"] = json!(corner);
        }
        if self.master {
            let (form, _) = FORMS.get(self.form).copied().unwrap_or(FORMS[0]);
            p["master_spline"] = json!(form);
        }
        let fitting = self.fit_radius(picks).is_some();
        let fitted = self.fit_field();
        for f in self.fields(picks.any()) {
            if fitting && f.key == fitted.key {
                continue;
            }
            let v = read(doc, self.value(f.key), f.unit).map_err(|why| bad(f.key, f.label, &why))?;
            p[f.key] = json!(v);
        }
        // Keeping the shaft's diameter: the preview gets the number that fits, worked out after
        // the other rows, so a row that is not a value is named before this one.
        if fitting {
            let v = self.fitted(doc, picks).ok_or_else(|| bad(fitted.key, fitted.label, tr("shaft_spline.empty")))?;
            p[fitted.key] = json!(v);
        }
        Ok(p)
    }

    /// The row the picked shaft's diameter sets while the spline keeps it: the Module for involute
    /// teeth, the Major diameter for straight-sided teeth and serrations.
    fn fit_field(&self) -> Field {
        if self.kind == 0 { INVOLUTE[0] } else { MAJOR }
    }

    /// The picked shaft's radius while the spline keeps its diameter: the box ticked, a shaft
    /// picked and its radius read. None otherwise, and the rows are typed as always.
    fn fit_radius(&self, picks: Picks<'_>) -> Option<f64> {
        let shaft = picks.shaft?;
        if !self.fit {
            return None;
        }
        self.shaft_radius.as_ref().filter(|c| &c.0 == shaft).and_then(|c| c.1).filter(|r| r.is_finite() && *r > 0.0)
    }

    /// The number of [`Spline::fit_field`] that keeps the picked shaft's diameter, in mm: the
    /// diameter itself, or for involute teeth the module that gives it as the tips' circle with
    /// the teeth, the addendum and the profile shift typed now. None while not fitting, or when
    /// those rows make no module.
    fn fitted(&self, doc: &Document, picks: Picks<'_>) -> Option<f64> {
        self.size_fitting(doc, self.kind, 2.0 * self.fit_radius(picks)?)
    }

    /// The size of the teeth of kind `kind` that makes the spline's major diameter `diameter`
    /// (mm): the diameter itself, or for involute teeth the module that gives it as the tips'
    /// circle with the teeth, the addendum and the profile shift typed now. None when those rows
    /// make no module.
    fn size_fitting(&self, doc: &Document, kind: usize, diameter: f64) -> Option<f64> {
        if kind != 0 {
            return Some(diameter);
        }
        let number = |key: &str| self.values.get(kind).and_then(|v| v.get(key)).and_then(|text| read(doc, text, Unit::Number).ok());
        let teeth = number("teeth")?;
        let addendum = number("addendum")?;
        let shift = number("profile_shift").unwrap_or(0.0);
        // The kernel's own sizing: the module that puts the tips on the shaft's circle.
        solidcraft_engine::doc::shaft_spline::module_for_major_radius(teeth, addendum, shift, diameter / 2.0)
    }

    /// The command's parameters: each row as typed (its unit made explicit as the design reads
    /// it), so the feature keeps it as an expression; the picked faces as points; the cutter's
    /// diameter only while its box is ticked for a picked shaft. With a face picked, `make` and
    /// the new hub's outside are not sent: the picks say what is cut.
    pub fn command_params(&self, doc: &Document, picks: Picks<'_>) -> Result<Value, Bad> {
        let typed = |text: &str, unit: Unit| crate::length_input::expression(doc, text, value_kind(unit));
        let (kind, _) = TYPES.get(self.kind).copied().unwrap_or(TYPES[0]);
        let mut p = json!({"type": kind});
        if !picks.any() {
            let (make, _) = MAKES.get(self.make).copied().unwrap_or(MAKES[0]);
            p["make"] = json!(make);
        }
        p["teeth"] = json!(typed(self.value("teeth"), Unit::Number));
        if self.kind != 0 {
            let (corner, _) = self.corner.get(self.kind).and_then(|c| CORNERS.get(*c)).copied().unwrap_or(CORNERS[0]);
            p["tip_corner"] = json!(corner);
        }
        if self.master {
            let (form, _) = FORMS.get(self.form).copied().unwrap_or(FORMS[0]);
            p["master_spline"] = json!(form);
        }
        // Keeping the shaft's diameter, its row is left out: the command takes the shaft's own.
        let fitting = self.fit_radius(picks).is_some();
        let fitted = self.fit_field();
        for f in self.fields(picks.any()) {
            if fitting && f.key == fitted.key {
                continue;
            }
            p[f.key] = json!(typed(self.value(f.key), f.unit));
        }
        // Each picked part with the face its teeth start from: the round face alone does not say which end.
        let starts = [
            (picks.shaft, picks.shaft_start, "shaft_face", "shaft_start", PICK_SHAFT, PICK_SHAFT_START, "shaft_spline.need_shaft_start"),
            (picks.bore, picks.bore_start, "bore_face", "bore_start", PICK_BORE, PICK_BORE_START, "shaft_spline.need_bore_start"),
        ];
        for (part, (face, start, face_key, start_key, face_label, start_label, needed)) in starts.into_iter().enumerate() {
            let Some(face) = face else { continue };
            p[face_key] = face_point(face, face_label)?;
            let flag = |flags: [bool; 2]| flags.get(part).copied().unwrap_or(false);
            // A reopened feature's saved start, while the round face it was reopened with is picked.
            let saved = self.saved_start.get(part).and_then(Option::as_ref).filter(|(sel, _)| flag(self.start_optional) && sel == face);
            match (start, saved) {
                (Some(start), _) => p[start_key] = face_point(start, start_label)?,
                (None, Some((_, at))) => p[start_key] = pt(*at),
                (None, None) if flag(self.start_optional) || flag(self.not_round) => {}
                (None, None) => return Err(Bad { key: None, words: tr(needed).to_string() }),
            }
        }
        if picks.shaft.is_some() && self.cutter {
            read(doc, &self.cutter_diameter, CUTTER.unit).map_err(|why| bad(CUTTER.key, CUTTER.label, &why))?;
            p[CUTTER.key] = json!(typed(&self.cutter_diameter, CUTTER.unit));
        }
        // Each chamfer goes with the part it is cut on, unless its row is a plain 0. An expression that works
        // out to 0 today is sent all the same, so the feature keeps it: the rebuild cuts no chamfer of size 0,
        // and one cut later when a parameter it names grows. A negative size is sent, and the command refuses it.
        // The row its Type adds (a second distance, an angle) goes with the chamfer only; Equal Distance adds none.
        for (hub, rows, text) in [(false, END_CHAMFERS, &self.end_chamfer), (true, ENTRY_CHAMFERS, &self.entry_chamfer)] {
            if !self.has_part(picks, hub) {
                continue;
            }
            let f = rows.size;
            read(doc, text, f.unit).map_err(|why| bad(f.key, f.label, &why))?;
            if plain_zero(text) {
                continue;
            }
            p[f.key] = json!(typed(text, f.unit));
            if let Some(second) = rows.second(self.chamfer_type(hub)) {
                let value = self.value(second.key);
                read(doc, value, second.unit).map_err(|why| bad(second.key, second.label, &why))?;
                p[second.key] = json!(typed(value, second.unit));
            }
        }
        Ok(p)
    }

    /// Reads a newly picked face, once per pick: the shaft's radius (see [`Spline::fit_radius`]), and
    /// how long each picked face is, which the Length the dialog put in itself must fit.
    fn sync_picks(&mut self, s: &Session, picks: Picks<'_>) {
        // Another round face picked: the start a reopened feature saved for that part no longer applies.
        for (slot, pick) in self.saved_start.iter_mut().zip([picks.shaft, picks.bore]) {
            if slot.as_ref().is_some_and(|(sel, _)| Some(sel) != pick) {
                *slot = None;
            }
        }
        let new_shaft = match picks.shaft {
            None => {
                self.shaft_radius = None;
                false
            }
            Some(shaft) if self.shaft_radius.as_ref().is_some_and(|c| &c.0 == shaft) => false,
            Some(shaft) => {
                self.shaft_radius = Some((shaft.clone(), picked_radius(s, shaft)));
                true
            }
        };
        let new_bore = self.bore_read.as_ref() != picks.bore;
        if new_bore {
            self.bore_read = picks.bore.cloned();
        }
        if !(new_shaft || new_bore) {
            return;
        }
        // The rows the dialog filled itself follow the faces picked: the length is the shaft's
        // diameter (the dialog's first length when only a bore is picked), as far as each picked
        // face is long, so the first spline shown is never longer than its shaft or its bore; the
        // cutter is twice the shaft's diameter. A row the user typed in stays.
        let radius = self.shaft_radius.as_ref().and_then(|c| c.1).filter(|r| r.is_finite() && *r > 0.0);
        let shown = |mm: f64| crate::length_input::shown(&s.doc, mm);
        if self.started_length.as_deref() == Some(self.length.as_str()) && (radius.is_some() || picks.bore.is_some()) {
            let room = |face: Option<&Sel>, inside: bool| {
                face.and_then(|sel| picked_room(s, sel, inside)).filter(|l| l.is_finite() && *l > 0.0).unwrap_or(f64::INFINITY)
            };
            let length = radius.map_or(FIRST_LENGTH, |r| 2.0 * r).min(room(picks.shaft, false)).min(room(picks.bore, true));
            self.length = shown(length);
            self.started_length = Some(self.length.clone());
        }
        if new_shaft
            && let Some(radius) = radius
            && self.started_cutter.as_deref() == Some(self.cutter_diameter.as_str())
        {
            self.cutter_diameter = shown(4.0 * radius);
            self.started_cutter = Some(self.cutter_diameter.clone());
        }
        // The size rows the dialog filled itself start from the shaft picked, so the spline first
        // fits the shaft as it is: the Module that gives the shaft's diameter with the teeth typed
        // now, the Major diameter the shaft's own. From then on the row is a number like any
        // other, and more teeth of that size make a larger shaft. A size the user typed stays.
        if new_shaft && let Some(radius) = radius {
            for kind in 0..self.values.len() {
                let key = if kind == 0 { INVOLUTE[0].key } else { MAJOR.key };
                let own = self.started_size.get(kind).and_then(|t| t.as_deref());
                let now = self.values.get(kind).and_then(|v| v.get(key)).map(String::as_str);
                if own.is_none() || own != now {
                    continue;
                }
                let Some(size) = self.size_fitting(&s.doc, kind, 2.0 * radius) else { continue };
                let text = shown(size);
                if let Some(values) = self.values.get_mut(kind) {
                    values.insert(key, text.clone());
                }
                if let Some(slot) = self.started_size.get_mut(kind) {
                    *slot = Some(text);
                }
            }
        }
    }

    /// What the values make now with nothing picked (see [`Spline::work_for`]).
    pub fn work(&self, doc: &Document) -> Arc<Worked> {
        self.work_for(doc, Picks::default())
    }

    /// What the values make now: kept when the parameters are the ones it was worked out for,
    /// otherwise worked out again by the engine.
    pub fn work_for(&self, doc: &Document, picks: Picks<'_>) -> Arc<Worked> {
        let params = self.params_for(doc, picks);
        let key = work_key(&params);
        if let Some(w) = self.worked.as_ref().filter(|w| w.key == key) {
            return Arc::clone(w);
        }
        let result = params.and_then(|p| {
            let preview = shaft_spline_preview(&p).map_err(|r| Bad { key: r.key, words: r.words })?;
            Ok((p, Arc::new(preview)))
        });
        Arc::new(Worked { key, result })
    }

    /// Whether the values changed since they were last worked out, refused or not: a refused value left as it is
    /// asks for no more frames.
    fn stale(&self, doc: &Document, picks: Picks<'_>) -> bool {
        self.worked.as_ref().is_none_or(|w| w.key != work_key(&self.params_for(doc, picks)))
    }

    /// Works out what the values make when they changed, and keeps the last good preview.
    fn refresh(&mut self, doc: &Document, picks: Picks<'_>) {
        let w = self.work_for(doc, picks);
        if let Ok((_, preview)) = &w.result {
            self.good = Some(Arc::clone(preview));
        }
        self.worked = Some(w);
    }

    /// Whether a shaft's teeth are cut or made (`hub`: a hub's): the part a chamfer's row is for. With a face
    /// picked the picks say, else Make does.
    fn has_part(&self, picks: Picks<'_>, hub: bool) -> bool {
        if picks.any() {
            if hub { picks.bore.is_some() } else { picks.shaft.is_some() }
        } else {
            // [`MAKES`]: the shaft, the hub, both. A part is made unless Make is the other one alone.
            self.make != usize::from(!hub)
        }
    }

    /// The Type of the chamfer of a shaft's teeth (`hub`: of a hub's), an index into [`CHAMFER_TYPES`].
    fn chamfer_type(&self, hub: bool) -> usize {
        if hub { self.entry_chamfer_type } else { self.end_chamfer_type }
    }

    /// The same, to change.
    fn chamfer_type_mut(&mut self, hub: bool) -> &mut usize {
        if hub { &mut self.entry_chamfer_type } else { &mut self.end_chamfer_type }
    }

    /// The refusal of the values as they were last worked out: of the teeth, else of a row that goes to the
    /// command only.
    fn refusal(&self) -> Option<&Bad> {
        self.worked.as_ref().and_then(|w| w.result.as_ref().err()).or(self.command_bad.as_ref())
    }

    /// The preview of the values as they were last worked out.
    fn preview(&self) -> Option<&Arc<SplinePreview>> {
        self.worked.as_ref().and_then(|w| w.result.as_ref().ok()).map(|(_, p)| p)
    }
}

/// Every key a row of numbers can have, for [`Spline::set`] (whose maps keep `'static` keys).
const ALL_KEYS: &[&str] = &[
    "addendum",
    "dedendum",
    "flank_angle",
    "form_clearance",
    "hub_addendum",
    "hub_diameter",
    "hub_root_round",
    "hub_tip_round",
    "major_clearance",
    "major_diameter",
    "minor_clearance",
    "minor_diameter",
    "module",
    "pressure_angle",
    "profile_shift",
    "root_fillet",
    "root_round",
    "side_clearance",
    "teeth",
    "thinning",
    "tip_clearance",
    "tip_size",
    "tooth_width",
];

/// The key a result is kept by ([`Worked`]): the parameters as text, or the refusal of a row.
fn work_key(params: &Result<Value, Bad>) -> String {
    match params {
        Ok(p) => p.to_string(),
        Err(e) => format!("{e:?}"),
    }
}

/// A row's text as the command's number: a length in mm, an angle in degrees, or a plain number.
fn read(doc: &Document, text: &str, unit: Unit) -> Result<f64, String> {
    if text.trim().is_empty() {
        return Err(tr("shaft_spline.empty").to_string());
    }
    let v = crate::length_input::eval(doc, text, value_kind(unit)).map_err(|e| e.to_string())?;
    let v = if unit == Unit::Angle { v.to_degrees() } else { v };
    if v.is_finite() { Ok(v) } else { Err(tr("shaft_spline.empty").to_string()) }
}

/// Whether a row's text is a plain number 0, with or without a unit ("0", "0 mm", "0.0in"): no chamfer. An
/// expression, one that names a parameter or works something out, is not, even where it comes to 0.
fn plain_zero(text: &str) -> bool {
    let text = text.trim();
    let digits = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (Some(number), Some(unit)) = (text.get(..digits), text.get(digits..)) else { return false };
    number.parse::<f64>().is_ok_and(|v| v == 0.0) && solidcraft_engine::doc::expr::unit_info(unit).is_some()
}

/// How the expressions of a row are read.
fn value_kind(unit: Unit) -> ValueKind {
    match unit {
        Unit::Length => ValueKind::Length,
        Unit::Angle => ValueKind::Angle,
        Unit::Number => ValueKind::Unitless,
    }
}

/// A row's value that is not a value, in words.
fn bad(key: &str, label: &str, why: &str) -> Bad {
    Bad { key: Some(key.to_string()), words: format("shaft_spline.not_a_value", &[("label", tr(label)), ("why", why)]) }
}

/// The pick inputs: the outside of a shaft and a bore, one round face each, both optional, and after each the
/// face its teeth start from, which a picked part needs.
fn inputs() -> Vec<SelInput> {
    vec![
        SelInput::new(PICK_SHAFT, FACES, false).tip("shaft_spline.pick_shaft_help"),
        SelInput::new(PICK_SHAFT_START, FACES, false).tip("shaft_spline.pick_shaft_start_help"),
        SelInput::new(PICK_BORE, FACES, false).tip("shaft_spline.pick_bore_help"),
        SelInput::new(PICK_BORE_START, FACES, false).tip("shaft_spline.pick_bore_start_help"),
    ]
}

/// The dialog for Shaft Spline.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == COMMAND).then(|| (Kind::ShaftSpline(Box::new(Spline::new(&app.session.doc))), inputs()))
}

/// A saved Shaft Spline's dialog: every row, both picks, the master spline and the groove end as
/// saved, so that OK edits the feature (`d.editing`, set by the caller) instead of adding one.
pub(crate) fn reopen(d: &mut Dialog, s: &Session, feature: &Feature) {
    let FeatureKind::ShaftSpline {
        shaft,
        shaft_start,
        bore,
        bore_start,
        form,
        make,
        teeth,
        length,
        module,
        pressure_angle,
        addendum,
        dedendum,
        root_fillet,
        profile_shift,
        thinning,
        hub_addendum,
        tip_clearance,
        form_clearance,
        major_diameter,
        minor_diameter,
        tooth_width,
        flank_angle,
        root_round,
        tip_corner,
        tip_size,
        major_clearance,
        minor_clearance,
        hub_tip_round,
        side_clearance,
        hub_root_round,
        hub_diameter,
        master,
        master_angle,
        cutter_diameter,
        end_chamfer,
        entry_chamfer,
        end_chamfer_distance2,
        end_chamfer_angle,
        entry_chamfer_distance2,
        entry_chamfer_angle,
        ..
    } = &feature.kind
    else {
        return;
    };
    let doc = &s.doc;
    // A bare stored length is millimetres and a bare angle degrees: the rows say so.
    let text = |e: &str, unit: Unit| match unit {
        Unit::Length => crate::length_input::stored(doc, e),
        Unit::Angle => shown_expr(e, ValueKind::Angle),
        Unit::Number => e.to_string(),
    };
    let mut spline = Spline::new(doc);
    // The kind first: the rows of numbers are kept for each kind.
    spline.kind = match form {
        SplineForm::Involute => 0,
        SplineForm::Straight => 1,
        SplineForm::Serration => 2,
    };
    spline.make = match make {
        SplineMake::Shaft => 0,
        SplineMake::Hub => 1,
        SplineMake::Both => 2,
    };
    spline.set("teeth", &text(teeth.as_str(), Unit::Number));
    spline.length = text(length.as_str(), Unit::Length);
    // The rows are the feature's own: picking its face again must not put starting values in.
    spline.started_length = None;
    spline.started_cutter = None;
    spline.started_size = [None, None, None];
    spline.set("side_clearance", &text(side_clearance.as_str(), Unit::Length));
    let optional: &[(&str, &Option<Expr>, Unit)] = &[
        ("module", module, Unit::Length),
        ("pressure_angle", pressure_angle, Unit::Angle),
        ("addendum", addendum, Unit::Number),
        ("dedendum", dedendum, Unit::Number),
        ("root_fillet", root_fillet, Unit::Number),
        ("profile_shift", profile_shift, Unit::Number),
        ("thinning", thinning, Unit::Length),
        ("hub_addendum", hub_addendum, Unit::Number),
        ("tip_clearance", tip_clearance, Unit::Length),
        ("form_clearance", form_clearance, Unit::Length),
        ("major_diameter", major_diameter, Unit::Length),
        ("minor_diameter", minor_diameter, Unit::Length),
        ("tooth_width", tooth_width, Unit::Length),
        ("flank_angle", flank_angle, Unit::Angle),
        ("root_round", root_round, Unit::Length),
        ("tip_size", tip_size, Unit::Length),
        ("major_clearance", major_clearance, Unit::Length),
        ("minor_clearance", minor_clearance, Unit::Length),
        ("hub_tip_round", hub_tip_round, Unit::Length),
        ("hub_root_round", hub_root_round, Unit::Length),
        // Without one (a bore was picked) the row keeps its example, for the preview.
        ("hub_diameter", hub_diameter, Unit::Length),
    ];
    for &(key, saved, unit) in optional {
        if let Some(e) = saved.as_deref() {
            spline.set(key, &text(e, unit));
        }
    }
    let kind = spline.kind;
    if let Some(corner) = spline.corner.get_mut(kind) {
        *corner = match tip_corner {
            SplineCorner::Plain => 0,
            SplineCorner::Chamfered => 1,
            SplineCorner::Rounded => 2,
        };
    }
    (spline.master, spline.form) = match master {
        SplineMaster::None => (false, 0),
        SplineMaster::ToothOut => (true, 0),
        SplineMaster::WideTooth => (true, 1),
    };
    if let Some(e) = master_angle.as_deref() {
        spline.master_angle = text(e, Unit::Angle);
    }
    spline.cutter = cutter_diameter.is_some();
    if let Some(e) = cutter_diameter.as_deref() {
        spline.cutter_diameter = text(e, Unit::Length);
    }
    if let Some(e) = end_chamfer.as_deref() {
        spline.end_chamfer = text(e, Unit::Length);
    }
    if let Some(e) = entry_chamfer.as_deref() {
        spline.entry_chamfer = text(e, Unit::Length);
    }
    // Each chamfer's Type follows the number the feature has beside its size: a second distance, an angle, or
    // neither (Equal Distance). The rows of the other types keep their starting values.
    spline.end_chamfer_type = chamfer_type_of(end_chamfer_distance2.as_ref(), end_chamfer_angle.as_ref());
    spline.entry_chamfer_type = chamfer_type_of(entry_chamfer_distance2.as_ref(), entry_chamfer_angle.as_ref());
    for (key, saved, unit) in [
        ("end_chamfer_distance2", end_chamfer_distance2, Unit::Length),
        ("end_chamfer_angle", end_chamfer_angle, Unit::Angle),
        ("entry_chamfer_distance2", entry_chamfer_distance2, Unit::Length),
        ("entry_chamfer_angle", entry_chamfer_angle, Unit::Angle),
    ] {
        if let Some(e) = saved.as_deref() {
            spline.set(key, &text(e, unit));
        }
    }
    // Face names follow the geometry walk: the shaft's pick and the face its teeth start from, then the
    // bore's two, each when there is one. A round face no name or point finds again is kept as the place
    // saved, to be picked again. A start that no face is found at leaves its row empty (a script may leave
    // the start out, and the end stored for it need not lie on a face): the spline keeps its end.
    let mut slot = 0usize;
    for (input, pick, start) in [(SHAFT, shaft, false), (SHAFT_START, shaft_start, true), (BORE, bore, false), (BORE_START, bore_start, true)] {
        let Some(pick) = pick else { continue };
        let saved = pick.0;
        let found = feature
            .face_names
            .get(slot)
            .and_then(|name| named_refill(s, feature.kind.face_body(), name, saved, false))
            .or_else(|| face_sel(s, saved));
        slot += 1;
        let sel = if start {
            let Some(sel) = found else { continue };
            sel
        } else {
            let sel = found.unwrap_or(Sel::Face { body: String::new(), index: usize::MAX, point: saved });
            // A hub that sits on its shaft with no play has its bore on the shaft's own surface, so the
            // face found at the saved place can be the other body's: the row shows the face of its own
            // kind there, the one the rebuild cuts into.
            let inside = input == BORE;
            match picked_inside(s, &sel) {
                Some(kind) if kind != inside => round_face_of_kind(s, saved, inside).unwrap_or(sel),
                _ => sel,
            }
        };
        if let Some(inp) = d.inputs.get_mut(input) {
            inp.items = vec![sel];
        }
    }
    // A saved spline keeps the end it has where no start face is shown: OK runs without one there.
    let shown = |input: usize| d.inputs.get(input).is_some_and(|i| !i.items.is_empty());
    spline.start_optional = [shaft.is_some() && !shown(SHAFT_START), bore.is_some() && !shown(BORE_START)];
    // Where the saved start face is not shown, the start the feature has goes with the command again, so OK
    // does not move the end of the teeth to the end nearest the round face's pick.
    let held = |input: usize| d.inputs.get(input).and_then(|i| i.items.first().cloned());
    spline.saved_start = [(SHAFT, SHAFT_START, shaft_start), (BORE, BORE_START, bore_start)]
        .map(|(face, start, saved)| saved.as_ref().filter(|_| !shown(start)).and_then(|at| held(face).map(|sel| (sel, at.0))));
    // A shaft spline saved without its size keeps the shaft's diameter; one saved with it keeps
    // the number typed.
    let sized = if spline.kind == 0 { module.is_some() } else { major_diameter.is_some() };
    spline.fit = shaft.is_some() && !sized;
    d.active = 0;
    d.kind = Kind::ShaftSpline(Box::new(spline));
}

/// A saved chamfer's Type, an index into [`CHAMFER_TYPES`]: Two Distances with a second distance, Distance and
/// Angle with an angle, else Equal Distance.
fn chamfer_type_of(distance2: Option<&Expr>, angle: Option<&Expr>) -> usize {
    if distance2.is_some() {
        1
    } else if angle.is_some() {
        2
    } else {
        0
    }
}

pub fn title() -> &'static str {
    "cmd.shaft_spline_insert"
}

/// A number row: its field, flagged when the refusal names its key. Returns accepted Enter.
fn number_row(ui: &mut egui::Ui, s: &mut Spline, f: Field, flagged: bool) -> bool {
    let text = s.text_mut(f.key);
    form_row(ui, f.label, |ui| {
        let (enter, response) = crate::ui_kit::checked_field(ui, text, "", flagged);
        let centre = response.rect.center();
        response.backing_on_hover_text(tr(f.help));
        crate::scenario::publish_handle(&format!("dialog:shaft_spline:{}", f.key), centre);
        enter
    })
}

/// The row a picked shaft's diameter sets while the spline keeps it: the number that fits (a dash
/// while the rows make none), shown in a field that cannot be typed in.
fn fitted_row(ui: &mut egui::Ui, f: Field, shown: Option<String>, flagged: bool) {
    let mut text = shown.unwrap_or_else(|| "\u{2013}".to_string());
    form_row(ui, f.label, |ui| {
        let response = ui.add_enabled_ui(false, |ui| crate::ui_kit::checked_field(ui, &mut text, "", flagged).1).inner;
        crate::scenario::publish_handle(&format!("dialog:shaft_spline:{}", f.key), response.rect.center());
    });
}

/// A choice row. Returns whether the choice changed.
fn choice(ui: &mut egui::Ui, label: &str, id: &str, selected: &mut usize, options: &[&str]) -> bool {
    let before = *selected;
    crate::ui_kit::choice_row(ui, label, id, selected, options);
    *selected != before
}

/// A chamfer's Type: the Chamfer tool's types, in its order. The row under it follows the choice.
fn chamfer_type_row(ui: &mut egui::Ui, selected: &mut usize, rows: &ChamferRows) {
    let response = crate::ui_kit::choice_row(ui, "shaft_spline.chamfer_type", &format!("ss_{}", rows.choice), selected, &CHAMFER_TYPES);
    let centre = response.rect.center();
    response.backing_on_hover_text(tr("shaft_spline.chamfer_type_help"));
    crate::scenario::publish_handle(&format!("dialog:shaft_spline:{}", rows.choice), centre);
}

/// The dialog's rows (under the pick inputs, which the dialog shows itself): the drawing, the
/// selections, the numbers, the sections and the read-outs. Returns whether Enter was accepted in
/// a value.
pub fn rows(app: &SolveApp, ui: &mut egui::Ui, s: &mut Spline, inputs: &[SelInput]) -> bool {
    let doc = &app.session.doc;
    let picks = Picks::of(inputs);
    s.sync_picks(&app.session, picks);
    // On a picked shaft, whether the spline keeps the shaft's own diameter. Unticked, the row it
    // set starts from the size that fitted.
    if picks.shaft.is_some() {
        let fitted = s.fitted(doc, picks);
        form_row(ui, "", |ui| {
            let r = crate::ui_kit::checkbox(ui, tr("shaft_spline.fit_shaft"), &mut s.fit);
            let rect = r.rect;
            r.backing_on_hover_text(tr("shaft_spline.fit_shaft_help"));
            crate::scenario::publish_handle("dialog:shaft_spline:fit", rect.center());
        });
        if !s.fit
            && let Some(v) = fitted
        {
            let key = s.fit_field().key;
            s.set(key, &crate::length_input::shown(doc, v));
        }
    }
    s.refresh(doc, picks);
    // The rows that go to the command only (the cutter's diameter, the chamfers) are refused in words like the
    // rest: their row is marked, and the line under the drawing says why.
    s.command_bad = if s.worked.as_ref().is_some_and(|w| w.result.is_ok()) { s.command_params(doc, picks).err() } else { None };
    drawing(ui, doc, s);
    let flagged = s.refusal().and_then(|b| b.key.clone());
    let is = |key: &str| flagged.as_deref() == Some(key);
    let mut enter = false;
    let kinds: Vec<&str> = TYPES.iter().map(|t| t.1).collect();
    choice(ui, "shaft_spline.type", "ss_type", &mut s.kind, &kinds);
    // A picked face says what is cut: Make is for a new part only.
    if !picks.any() {
        let makes: Vec<&str> = MAKES.iter().map(|m| m.1).collect();
        choice(ui, "shaft_spline.make", "ss_make", &mut s.make, &makes);
    }
    let eval = |text: &str| crate::length_input::eval(doc, text, ValueKind::Unitless).ok();
    // Involute teeth take three or more, the others two or more.
    let least_teeth = if s.kind == 0 { 3 } else { 2 };
    let teeth = s.text_mut("teeth");
    enter |= form_row(ui, TEETH_LABEL, |ui| {
        let enter = crate::ui_kit::count_field(ui, teeth, &crate::ui_kit::Count { min: least_teeth, max: 2000, eval: &eval });
        let rect = ui.min_rect();
        if is("teeth") {
            let t = Tokens::get();
            ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(1.5, t.warning), egui::StrokeKind::Inside);
        }
        crate::scenario::publish_handle("dialog:shaft_spline:teeth", rect.center());
        enter
    });
    let shaft: &[Field] = match s.kind {
        0 => &INVOLUTE,
        1 => &STRAIGHT,
        _ => &SERRATION,
    };
    // On top, after the Length, the numbers that size the teeth: the Module and the Pressure angle of involute
    // teeth, the two diameters and the Tooth width or the Flank angle of the others. The rest of the tooth's form
    // folds away under them.
    let sizing = if s.kind == 0 { 2 } else { 3 };
    let no_rows: &[Field] = &[];
    let (sizes, form) = shaft.split_at_checked(sizing).unwrap_or((shaft, no_rows));
    // Keeping the shaft's diameter, the row it sets shows the number that fits, not to be typed.
    let fitting = s.fit_radius(picks).is_some();
    let fitted = s.fitted(doc, picks);
    let fit_key = s.fit_field().key;
    for f in std::iter::once(&LENGTH).chain(sizes) {
        if fitting && f.key == fit_key {
            fitted_row(ui, *f, fitted.map(|v| crate::length_input::shown(doc, v)), is(f.key));
            continue;
        }
        enter |= number_row(ui, s, *f, is(f.key));
    }
    if let Some(e) = crate::ui_kit::section(ui, "ss_form", "shaft_spline.tooth_form", true, |ui| {
        let mut enter = false;
        for f in form {
            enter |= number_row(ui, s, *f, is(f.key));
        }
        enter
    }) {
        enter |= e;
    }
    let straight = s.kind != 0;
    let fit: &[Field] = if straight { &FIT_STRAIGHT } else { &FIT_INVOLUTE };
    if let Some(e) = crate::ui_kit::section(ui, "ss_fit", "shaft_spline.fit", true, |ui| {
        let mut enter = false;
        for f in fit {
            enter |= number_row(ui, s, *f, is(f.key));
        }
        enter
    }) {
        enter |= e;
    }
    if straight
        && let Some(e) = crate::ui_kit::section(ui, "ss_corners", "shaft_spline.tip_corners", false, |ui| {
            let corners: Vec<&str> = CORNERS.iter().map(|c| c.1).collect();
            let kind = s.kind;
            if let Some(corner) = s.corner.get_mut(kind) {
                choice(ui, "shaft_spline.corners", "ss_corner", corner, &corners);
            }
            if s.corner.get(kind).is_some_and(|c| *c != 0) { number_row(ui, s, TIP_SIZE, is(TIP_SIZE.key)) } else { false }
        })
    {
        enter |= e;
    }
    // The spline's own chamfers, each where its part is cut or made: on the ends of the shaft's teeth, at the
    // entries of the hub's bore. Each has its size, its Type and the row the type adds.
    let chamfers: Vec<(bool, ChamferRows)> =
        [(false, END_CHAMFERS), (true, ENTRY_CHAMFERS)].into_iter().filter(|(hub, _)| s.has_part(picks, *hub)).collect();
    if !chamfers.is_empty()
        && let Some(e) = crate::ui_kit::section(ui, "ss_chamfers", "shaft_spline.chamfers", false, |ui| {
            let mut enter = false;
            for (hub, rows) in chamfers {
                enter |= number_row(ui, s, rows.size, is(rows.size.key));
                chamfer_type_row(ui, s.chamfer_type_mut(hub), &rows);
                if let Some(second) = rows.second(s.chamfer_type(hub)) {
                    enter |= number_row(ui, s, second, is(second.key));
                }
            }
            enter
        })
    {
        enter |= e;
    }
    // On a picked shaft each tooth space can end in the curve the cutter leaves.
    if picks.shaft.is_some()
        && let Some(e) = crate::ui_kit::section(ui, "ss_groove", "shaft_spline.groove_end", false, |ui| {
            form_row(ui, "", |ui| {
                let r = crate::ui_kit::checkbox(ui, tr("shaft_spline.cutter_round"), &mut s.cutter);
                let rect = r.rect;
                r.backing_on_hover_text(tr("shaft_spline.cutter_round_help"));
                crate::scenario::publish_handle("dialog:shaft_spline:cutter", rect.center());
            });
            if s.cutter { number_row(ui, s, CUTTER, is(CUTTER.key)) } else { false }
        })
    {
        enter |= e;
    }
    if let Some(e) = crate::ui_kit::section(ui, "ss_master", "shaft_spline.master", false, |ui| {
        form_row(ui, "", |ui| {
            let r = crate::ui_kit::checkbox(ui, tr("shaft_spline.master_cut"), &mut s.master);
            let rect = r.rect;
            r.backing_on_hover_text(tr("shaft_spline.master_help"));
            if is("master_spline") {
                let t = Tokens::get();
                ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(1.5, t.warning), egui::StrokeKind::Inside);
            }
            crate::scenario::publish_handle("dialog:shaft_spline:master", rect.center());
        });
        if !s.master {
            return false;
        }
        let forms: Vec<&str> = FORMS.iter().map(|f| f.1).collect();
        choice(ui, "shaft_spline.master_form", "ss_master_form", &mut s.form, &forms);
        number_row(ui, s, MASTER_ANGLE, is(MASTER_ANGLE.key))
    }) {
        enter |= e;
    }
    if s.make != 0
        && !picks.any()
        && let Some(e) = crate::ui_kit::section(ui, "ss_hub", "shaft_spline.hub", true, |ui| number_row(ui, s, HUB_DIAMETER, is(HUB_DIAMETER.key)))
    {
        enter |= e;
    }
    readouts(ui, doc, s);
    // A changed value is worked out on the next frame, which this asks for.
    if s.stale(doc, picks) {
        ui.ctx().request_repaint();
    }
    enter
}

/// The numbers the values make, under the rows: in how many positions the hub goes on, how far it
/// turns, and the clearances as they come out.
fn readouts(ui: &mut egui::Ui, doc: &Document, s: &Spline) {
    let preview = s.preview();
    let n = preview.map(|p| &p.numbers);
    let none = || "\u{2013}".to_string();
    let length = |mm: Option<f64>| mm.map_or_else(none, |v| crate::length_input::shown(doc, v));
    crate::ui_kit::section(ui, "ss_numbers", "shaft_spline.as_made", true, |ui| {
        crate::ui_kit::value_row(ui, "shaft_spline.positions", &n.map_or_else(none, |n| n.positions.to_string()));
        crate::ui_kit::value_row(ui, "shaft_spline.turning_play", &n.map_or_else(none, |n| format!("\u{b1}{}\u{b0}", plain(n.turning_play, 3))));
        crate::ui_kit::value_row(ui, "shaft_spline.side_each", &length(n.map(|n| n.side_clearance_each)));
        crate::ui_kit::value_row(ui, "shaft_spline.tip_gap", &length(n.map(|n| n.tip_gap)));
        crate::ui_kit::value_row(ui, "shaft_spline.root_gap", &length(n.map(|n| n.root_gap)));
        if s.kind == 0 {
            crate::ui_kit::value_row(ui, "shaft_spline.form_gap", &length(n.and_then(|n| n.form_gap)));
        }
    });
}

/// A number with at most `places` decimals and no trailing zeros.
fn plain(v: f64, places: usize) -> String {
    let text = format!("{v:.places$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() { "0".to_string() } else { text.to_string() }
}

/// The Show choice, the drawing and its caption line.
fn drawing(ui: &mut egui::Ui, doc: &Document, s: &mut Spline) {
    let t = Tokens::get();
    let shows: Vec<&str> = if s.master { SHOWS.to_vec() } else { SHOWS.iter().take(2).copied().collect() };
    if s.show >= shows.len() {
        s.show = 0;
    }
    choice(ui, "shaft_spline.show", "ss_show", &mut s.show, &shows);
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, (width * 0.75).round()), Sense::hover());
    crate::scenario::publish_rect("dialog:shaft_spline:drawing", rect);
    let refused = s.refusal().cloned();
    let shown = s.preview().or(s.good.as_ref()).cloned();
    let unit = crate::length_input::unit(doc).to_string();
    let scale = solidcraft_engine::doc::expr::unit_info(&unit).map_or(1.0, |(_, scale)| scale);
    let view = View::from_index(s.show);
    let look = Look { dim: refused.is_some(), unit: (&unit, scale) };
    match &shown {
        Some(preview) => paint(ui.painter(), rect, preview, view, &look),
        None => frame(ui.painter(), rect),
    }
    let caption = match (&refused, &shown) {
        (Some(b), _) => b.words.clone(),
        (None, Some(p)) => caption(doc, p, view),
        (None, None) => String::new(),
    };
    let tone = if refused.is_some() { crate::ui_kit::StatusTone::Warning } else { crate::ui_kit::StatusTone::Dim };
    let r = crate::ui_kit::status_line(ui, &caption, tone);
    crate::scenario::publish_rect("dialog:shaft_spline:caption", r.rect);
    ui.add_space(t.space_4);
}

/// The caption under the drawing: what the view shows, in numbers.
fn caption(doc: &Document, p: &SplinePreview, view: View) -> String {
    let n = &p.numbers;
    let mm = |v: f64| crate::length_input::shown(doc, v);
    match (view, &p.master) {
        (View::Master, Some(m)) => {
            let id = if m.form == MasterSplineParam::ToothOut { "shaft_spline.caption_tooth_out" } else { "shaft_spline.caption_wide_tooth" };
            format(id, &[("angle", &plain(m.angle, 3)), ("turn", &plain(m.turn, 3))])
        }
        (View::Teeth, _) => format("shaft_spline.caption_teeth", &[("side", &mm(n.side_clearance_each)), ("tip", &mm(n.tip_gap))]),
        _ => {
            let args = [
                ("major", mm(n.shaft_major_diameter)),
                ("minor", mm(n.shaft_minor_diameter)),
                ("hub_major", mm(n.hub_major_diameter)),
                ("hub_minor", mm(n.hub_minor_diameter)),
                ("pitch", n.pitch_diameter.map(mm).unwrap_or_default()),
            ];
            let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
            format(if n.pitch_diameter.is_some() { "shaft_spline.caption_whole_involute" } else { "shaft_spline.caption_whole" }, &args)
        }
    }
}

/// What the drawing shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The whole spline about its axis, +X to the right.
    Whole,
    /// Two or three ordinary teeth close up, the middle one pointing up.
    Teeth,
    /// The master spline close up, pointing up.
    Master,
}

impl View {
    fn from_index(i: usize) -> View {
        match i {
            1 => View::Teeth,
            2 => View::Master,
            _ => View::Whole,
        }
    }
}

/// How the drawing is painted: dimmed (the values make no spline; this is the last that did) and
/// the design's length unit with its size in mm, for the scale bar.
pub struct Look<'a> {
    pub dim: bool,
    pub unit: (&'a str, f64),
}

/// The empty frame of the drawing.
fn frame(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    p.rect_filled(rect, t.control_radius, t.control_fill);
    p.rect_stroke(rect, t.control_radius, Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
}

/// Where the drawing's millimetres go on the screen: the point at the middle of the rectangle, the
/// direction that points up (radians from +X) and the pixels per millimetre.
#[derive(Clone, Copy, Debug)]
struct Place {
    centre: [f64; 2],
    up: f64,
    scale: f64,
    middle: Pos2,
}

impl Place {
    fn at(&self, p: [f64; 2]) -> Pos2 {
        let (dx, dy) = (p[0] - self.centre[0], p[1] - self.centre[1]);
        let (s, c) = (FRAC_PI_2 - self.up).sin_cos();
        let (x, y) = (dx * c - dy * s, dx * s + dy * c);
        pos2(self.middle.x + (x * self.scale) as f32, self.middle.y - (y * self.scale) as f32)
    }
}

fn polar(r: f64, a: f64) -> [f64; 2] {
    [r * a.cos(), r * a.sin()]
}

/// The radius of a circle of the preview.
fn radius(p: &SplinePreview, which: SplineCircle) -> Option<f64> {
    p.circles.iter().find(|(c, _)| *c == which).map(|(_, r)| *r)
}

/// Where the view puts the spline.
fn place(rect: Rect, p: &SplinePreview, view: View) -> Place {
    let (w, h) = (f64::from(rect.width()).max(1.0), f64::from(rect.height()).max(1.0));
    let middle = rect.center();
    let outer = p.circles.iter().map(|(_, r)| *r).fold(0.0_f64, f64::max).max(1e-6);
    let close = |at: f64, pitches: f64| {
        let low = radius(p, SplineCircle::ShaftMinor).unwrap_or(0.0);
        let high = radius(p, SplineCircle::HubMajor).or_else(|| radius(p, SplineCircle::ShaftMajor)).unwrap_or(outer);
        let depth = (high - low).max(outer * 1e-3);
        let mid = (low + high) / 2.0;
        let across = (pitches * mid * p.pitch).max(depth * 1.2);
        let tall = depth * 2.0;
        let scale = (w / across).min(h / tall);
        Place { centre: polar(mid, at), up: at, scale, middle }
    };
    match (view, &p.master) {
        (View::Teeth, _) => close(p.tooth_at, 2.6),
        (View::Master, Some(m)) => close(m.angle.to_radians(), 3.4),
        _ => Place { centre: [0.0, 0.0], up: FRAC_PI_2, scale: 0.46 * w.min(h) / outer, middle },
    }
}

/// Points of a circle about the axis within the view, as screen points: the whole circle in the whole view
/// (centred on the axis), the arc that can be seen in a close-up.
fn circle_points(place: &Place, r: f64, rect: Rect) -> Vec<Pos2> {
    let whole = place.centre == [0.0, 0.0];
    // Across the view from its centre: the arc that can be seen, with some to spare.
    let seen = (f64::from(rect.width().max(rect.height())) / place.scale).max(0.0);
    let half = if r > 0.0 && !whole { (seen / r).min(std::f64::consts::PI) } else { std::f64::consts::PI };
    // A close-up is centred on the direction that points up in it.
    let at = if whole { 0.0 } else { place.up };
    let steps = 160;
    (0..=steps).map(|k| place.at(polar(r, at - half + 2.0 * half * f64::from(k) / f64::from(steps)))).collect()
}

/// Draws the preview into `rect`: the shaft lightly filled with a strong outline, the hub in a
/// quieter stroke, the named circles dashed and thin, the master spline's arcs thick with a mark
/// for its angle from +X, and a scale bar. Every colour is a theme token; nothing is drawn outside
/// `rect`.
pub fn paint(painter: &egui::Painter, rect: Rect, p: &SplinePreview, view: View, look: &Look<'_>) {
    let t = Tokens::get();
    frame(painter, rect);
    let painter = painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
    let place = place(rect, p, view);
    let fade = |c: Color32| if look.dim { c.gamma_multiply(0.35) } else { c };
    let ink = fade(t.form_ink);
    let quiet = fade(t.form_dim);
    let accent = fade(t.form_primary);
    // The shaft's fill: a fan from the axis, which every ray from it crosses once.
    for o in p.outlines.iter().filter(|o| o.part == SplinePart::Shaft) {
        let fill = if o.made { fade(t.form_dim.gamma_multiply(0.22)) } else { fade(t.form_dim.gamma_multiply(0.1)) };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(place.at([0.0, 0.0]), fill);
        for q in &o.points {
            mesh.colored_vertex(place.at(*q), fill);
        }
        let n = u32::try_from(o.points.len()).unwrap_or(0);
        for i in 1..=n {
            let next = if i == n { 1 } else { i + 1 };
            mesh.add_triangle(0, i, next);
        }
        painter.add(Shape::mesh(mesh));
    }
    // The named circles, dashed and thin.
    let dash = Stroke::new(0.8, quiet.gamma_multiply(0.8));
    let mut labelled: Vec<f32> = Vec::new();
    for (which, r) in &p.circles {
        let points = circle_points(&place, *r, rect);
        if *which == SplineCircle::HubOutside {
            painter.add(Shape::line(points, Stroke::new(1.2, quiet)));
            continue;
        }
        painter.extend(crate::ui_kit::dashed_line(&points, rect, dash, 4.0, 3.0));
        if view != View::Whole
            && let Some(end) = points.iter().filter(|q| rect.shrink(2.0).contains(**q)).max_by(|a, b| a.x.total_cmp(&b.x))
        {
            let font = egui::FontId::proportional(t.font_small);
            if labelled.iter().all(|y| (y - end.y).abs() > t.font_small) {
                labelled.push(end.y);
                painter.text(pos2(end.x - 2.0, end.y - 1.0), egui::Align2::RIGHT_BOTTOM, tr(circle_name(*which)), font, quiet);
            }
        }
    }
    // The outlines: the hub quieter, the shaft strong.
    for o in p.outlines.iter().filter(|o| o.part == SplinePart::Hub) {
        let stroke = if o.made { Stroke::new(1.2, quiet) } else { Stroke::new(0.8, quiet.gamma_multiply(0.6)) };
        painter.add(Shape::closed_line(o.points.iter().map(|q| place.at(*q)).collect(), stroke));
    }
    for o in p.outlines.iter().filter(|o| o.part == SplinePart::Shaft) {
        let stroke = if o.made { Stroke::new(1.6, ink) } else { Stroke::new(1.0, ink.gamma_multiply(0.6)) };
        painter.add(Shape::closed_line(o.points.iter().map(|q| place.at(*q)).collect(), stroke));
    }
    // The master spline: its arcs thick, and in the whole view its angle from +X.
    if let Some(m) = &p.master {
        for arc in [m.shaft_arc, m.hub_arc] {
            let steps = 48;
            let points: Vec<Pos2> =
                (0..=steps).map(|k| place.at(polar(arc.radius, arc.start + (arc.end - arc.start) * f64::from(k) / f64::from(steps)))).collect();
            painter.add(Shape::line(points, Stroke::new(3.0, accent)));
        }
        if view == View::Whole {
            let reach = radius(p, SplineCircle::ShaftMinor).unwrap_or(0.0) * 0.6;
            let angle = m.angle.to_radians();
            let mark = Stroke::new(1.0, accent);
            let origin = place.at([0.0, 0.0]);
            painter.line_segment([origin, place.at([reach * 1.25, 0.0])], Stroke::new(0.8, quiet));
            painter.line_segment([origin, place.at(polar(reach * 1.25, angle))], mark);
            let steps = 32;
            let points: Vec<Pos2> = (0..=steps).map(|k| place.at(polar(reach, angle * f64::from(k) / f64::from(steps)))).collect();
            painter.add(Shape::line(points, mark));
            painter.circle_filled(place.at(polar(reach * 1.25, angle)), 2.0, accent);
        }
    }
    scale_bar(&painter, rect, &place, look, quiet);
}

/// The word for a circle in the drawing.
fn circle_name(which: SplineCircle) -> &'static str {
    match which {
        SplineCircle::ShaftMajor => "shaft_spline.circle_major",
        SplineCircle::ShaftMinor => "shaft_spline.circle_minor",
        SplineCircle::Pitch => "shaft_spline.circle_pitch",
        SplineCircle::HubMajor => "shaft_spline.circle_hub_major",
        SplineCircle::HubMinor => "shaft_spline.circle_hub_minor",
        SplineCircle::HubOutside => "shaft_spline.circle_hub_outside",
    }
}

/// A bar of a round length in the design's unit, about a quarter of the drawing wide, at its
/// lower left, with its length.
fn scale_bar(painter: &egui::Painter, rect: Rect, place: &Place, look: &Look<'_>, color: Color32) {
    let t = Tokens::get();
    let (unit, mm_per_unit) = look.unit;
    let want = f64::from(rect.width()) * 0.25 / place.scale / mm_per_unit.max(1e-12);
    if !(want.is_finite() && want > 0.0) {
        return;
    }
    let power = 10f64.powf(want.log10().floor());
    let step = [1.0, 2.0, 5.0].into_iter().rev().find(|m| m * power <= want).unwrap_or(1.0) * power;
    let pixels = (step * mm_per_unit * place.scale) as f32;
    let left = pos2(rect.left() + t.space_8, rect.bottom() - t.space_8);
    let right = left + vec2(pixels, 0.0);
    let stroke = Stroke::new(1.0, color);
    painter.line_segment([left, right], stroke);
    for x in [left, right] {
        painter.line_segment([x - vec2(0.0, 3.0), x + vec2(0.0, 3.0)], stroke);
    }
    painter.text(
        right + vec2(t.space_4, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{} {unit}", plain(step, 6)),
        egui::FontId::proportional(t.font_small),
        color,
    );
}

/// The command OK runs with nothing picked (see [`commands_with`]).
pub fn commands(app: &SolveApp, s: &Spline) -> Result<Vec<(String, Value)>, String> {
    commands_with(app, s, &[])
}

/// The command OK runs: Shaft Spline with the dialog's rows as typed and its picks, once the
/// engine's preview takes the rows' values. Editing a feature, the dialog wraps it in
/// `timeline.redefine` (`dialogs::apply_commands`).
pub fn commands_with(app: &SolveApp, s: &Spline, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let doc = &app.session.doc;
    let picks = Picks::of(inputs);
    // A newly picked face is read by the dialog's rows within the frame (a shaft's diameter and
    // each face's length give the rows their starting values). Until then there is nothing to
    // run: a preview started now would be worked out for the values the rows are about to lose.
    let shaft_unread = picks.shaft.is_some_and(|shaft| !s.shaft_radius.as_ref().is_some_and(|c| &c.0 == shaft));
    if shaft_unread || picks.bore != s.bore_read.as_ref() {
        return Err(String::new());
    }
    let worked = s.work_for(doc, picks);
    if let Err(b) = &worked.result {
        return Err(b.words.clone());
    }
    let p = s.command_params(doc, picks).map_err(|b| b.words)?;
    Ok(vec![(COMMAND.to_string(), p)])
}

#[cfg(test)]
#[path = "dialogs_shaft_spline_tests.rs"]
mod tests;
