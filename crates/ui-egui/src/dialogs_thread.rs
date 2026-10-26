//! The thread dialog: form, size, hand, starts, extent, runouts, tip and the modelled option of
//! `solid.thread`. Choices only build the command's parameters: the command checks the geometry
//! and its error text is what the dialog shows. Browsing choices does not edit the model or
//! freeze an automatic feature.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{
    Document, TappedThreadFamily, ThreadHandedness, metric_thread_sizes, parse_metric_thread, parse_tapped_thread, tapped_thread_sizes,
};

use crate::dialogs::{combo, form_row, object_point, put, set_object_accepting};
use crate::i18n::tr;
use crate::selection::{Accept, EDGES, FACES, PLANES, SelInput};
use crate::ui_kit::{choice_row, expr_row, hint_row, text_row};

/// Common metric pitch presets, not an exhaustive catalogue of standard diameter/pitch pairs.
/// The coarse and current pitches are included separately; custom pairs remain editable.
const PITCHES: &[f64] = &[0.2, 0.25, 0.35, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

/// Generated metric designations omit only the shared catalogue's coarse pitch.
/// Authored designation text is never rewritten just by displaying a dialog.
pub(crate) fn designation(diameter: f64, pitch: f64) -> String {
    let coarse = metric_thread_sizes().iter().any(|(d, p)| (d - diameter).abs() < 1e-9 && (p - pitch).abs() < 1e-9);
    if coarse { format!("M{diameter}") } else { format!("M{diameter}x{pitch}") }
}

/// Unsaved numeric inputs for a custom metric designation. The document still stores only
/// the existing designation; an invalid draft must not apply its last valid value.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleThreadDraft {
    source: String,
    diameter: String,
    pitch: String,
    pub(crate) error: Option<String>,
}

impl HoleThreadDraft {
    fn new(source: &str) -> Self {
        let (diameter, pitch) = match parse_metric_thread(source) {
            Some((d, p)) => (format!("{d} mm"), format!("{p} mm")),
            None => (String::new(), String::new()),
        };
        Self { source: source.into(), diameter, pitch, error: None }
    }

    fn update(&mut self, doc: &Document, authored: &mut String) {
        let candidate = || -> Result<String, String> {
            let d = doc.eval(&self.diameter, solidcraft_engine::doc::expr::Kind::Length).map_err(|e| format!("nominal diameter: {e}"))?;
            let p = doc.eval(&self.pitch, solidcraft_engine::doc::expr::Kind::Length).map_err(|e| format!("pitch: {e}"))?;
            let text = designation(d, p);
            parse_metric_thread(&text).ok_or("use a positive nominal diameter and a pitch smaller than it")?;
            Ok(text)
        };
        match candidate() {
            Ok(text) => {
                *authored = text;
                self.source = authored.clone();
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

/// Tapped holes use a shared, finite catalogue; the engine derives the physical drill.
/// Custom remains a dialog mode even when its current values match a catalogue size.
pub(crate) fn hole_rows(ui: &mut egui::Ui, doc: &Document, designation: &mut String, custom: &mut Option<HoleThreadDraft>) -> bool {
    let designation_id = ui.make_persistent_id("hole_thread_designation");
    let families = [TappedThreadFamily::Metric, TappedThreadFamily::Unc, TappedThreadFamily::Unf, TappedThreadFamily::G, TappedThreadFamily::Rp];
    let family = parse_tapped_thread(designation).map(|s| s.family).unwrap_or(TappedThreadFamily::Metric);
    let mut profile = families.iter().position(|f| *f == family).unwrap_or(0);
    let before_profile = profile;
    form_row(ui, "Profile", |ui| combo(ui, "hole_thread_profile", &["ISO metric", "UNC", "UNF", "G / BSPP", "Rp"], &mut profile));
    let family = families.get(profile).copied().unwrap_or(TappedThreadFamily::Metric);
    let sizes = tapped_thread_sizes(family);
    if family != TappedThreadFamily::Metric {
        *custom = None;
    }
    if profile != before_profile
        && let Some(size) = sizes.first()
    {
        *designation = size.designation.clone();
        *custom = None;
    }
    let current = parse_tapped_thread(designation);
    let catalogue = current.as_ref().and_then(|s| sizes.iter().position(|p| p.designation == s.designation));
    if family == TappedThreadFamily::Metric
        && (custom.as_ref().is_some_and(|draft| draft.source != *designation) || (custom.is_none() && catalogue.is_none()))
    {
        *custom = Some(HoleThreadDraft::new(designation));
    }
    form_row(ui, "Size", |ui| {
        let mut selected = if custom.is_some() { sizes.len() } else { catalogue.unwrap_or(sizes.len()) };
        let before = selected;
        let mut labels: Vec<String> = sizes.iter().map(|s| s.designation.clone()).collect();
        if family == TappedThreadFamily::Metric {
            labels.push("Custom".to_string());
        }
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let catalogue_id = match family {
            TappedThreadFamily::Metric => "hole_thread_catalogue_metric",
            TappedThreadFamily::Unc => "hole_thread_catalogue_unc",
            TappedThreadFamily::Unf => "hole_thread_catalogue_unf",
            TappedThreadFamily::G => "hole_thread_catalogue_g",
            TappedThreadFamily::Rp => "hole_thread_catalogue_rp",
        };
        combo(ui, catalogue_id, &labels, &mut selected);
        if selected != before {
            if let Some(size) = sizes.get(selected) {
                *designation = size.designation.clone();
                *custom = None;
            } else {
                *custom = Some(HoleThreadDraft::new(designation));
            }
        }
    });
    if matches!(family, TappedThreadFamily::G | TappedThreadFamily::Rp) {
        form_row(ui, "Thread diameter", |ui| {
            if let Some(size) = parse_tapped_thread(designation) {
                ui.label(format!("{:.3} mm, 55°", size.major_diameter));
            }
        });
        form_row(ui, "Pitch", |ui| {
            if let Some(size) = parse_tapped_thread(designation) {
                ui.label(format!("{:.4} mm ({} TPI)", size.pitch, (25.4 / size.pitch).round()));
            }
        });
    }
    let mut enter = false;
    if let Some(draft) = custom {
        let (diameter_enter, diameter_changed) = text_row(ui, "Nominal diameter", &mut draft.diameter, "");
        let (pitch_enter, pitch_changed) = text_row(ui, "Pitch", &mut draft.pitch, "");
        if diameter_changed || pitch_changed {
            draft.update(doc, designation);
        }
        enter |= diameter_enter || pitch_enter;
    }
    let response = form_row(ui, "Designation", |ui| crate::ui_kit::cell_editor(ui, designation_id, designation, "M5, M5x0.5, 1/4-20 UNC", false));
    if response.changed() && custom.is_some() {
        *custom = Some(HoleThreadDraft::new(designation));
    }
    form_row(ui, "Tap drill diameter", |ui| {
        match parse_tapped_thread(designation).filter(|_| custom.as_ref().is_none_or(|draft| draft.error.is_none())) {
            Some(size) => {
                ui.label(format!("{} mm", size.tap_drill));
            }
            None => {
                ui.label(tr("thread.select_size"));
            }
        }
    });
    enter || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
}

pub(crate) fn handedness_row(ui: &mut egui::Ui, handedness: &mut ThreadHandedness, id: &str) {
    let mut selected = usize::from(*handedness == ThreadHandedness::Left);
    form_row(ui, "thread.direction", |ui| {
        combo(ui, id, &["thread.right_hand", "thread.left_hand"], &mut selected);
    });
    *handedness = if selected == 1 { ThreadHandedness::Left } else { ThreadHandedness::Right };
}

/// ISO 2904 (DIN 103-2) trapezoidal diameter/pitch combinations, the preferred (medium) pitch
/// first. ISO 3030 buttress threads use the same combinations from 10 mm up.
const TRAPEZOIDAL: &[(f64, &[f64])] = &[
    (8.0, &[1.5]),
    (9.0, &[2.0, 1.5]),
    (10.0, &[2.0, 1.5]),
    (11.0, &[3.0, 2.0]),
    (12.0, &[3.0, 2.0]),
    (14.0, &[3.0, 2.0]),
    (16.0, &[4.0, 2.0]),
    (18.0, &[4.0, 2.0]),
    (20.0, &[4.0, 2.0]),
    (22.0, &[5.0, 3.0, 8.0]),
    (24.0, &[5.0, 3.0, 8.0]),
    (26.0, &[5.0, 3.0, 8.0]),
    (28.0, &[5.0, 3.0, 8.0]),
    (30.0, &[6.0, 3.0, 10.0]),
    (32.0, &[6.0, 3.0, 10.0]),
    (36.0, &[6.0, 3.0, 10.0]),
    (40.0, &[7.0, 3.0, 10.0]),
    (44.0, &[7.0, 3.0, 12.0]),
    (48.0, &[8.0, 3.0, 12.0]),
    (50.0, &[8.0, 3.0, 12.0]),
    (52.0, &[8.0, 3.0, 12.0]),
    (60.0, &[9.0, 3.0, 14.0]),
    (70.0, &[10.0, 4.0, 16.0]),
    (80.0, &[10.0, 4.0, 16.0]),
    (90.0, &[12.0, 4.0, 18.0]),
    (100.0, &[12.0, 4.0, 20.0]),
];

/// A thread form: the command's `form` value, the dialog's label, and the designation prefix of
/// the forms whose sizes the dialog lists (the others take a typed designation, or the size
/// that fits the face).
pub(crate) struct Form {
    pub value: &'static str,
    pub label: &'static str,
    prefix: Option<&'static str>,
    buttress: bool,
}

pub(crate) const FORMS: &[Form] = &[
    Form { value: "metric", label: "ISO metric", prefix: Some("M"), buttress: false },
    Form { value: "trapezoidal", label: "Trapezoidal (ISO 2904)", prefix: Some("Tr"), buttress: false },
    Form { value: "acme", label: "Acme (ASME B1.5)", prefix: None, buttress: false },
    Form { value: "buttress", label: "Buttress (ISO 3030)", prefix: Some("S"), buttress: true },
    Form { value: "buttress_asme", label: "Buttress (ASME B1.9)", prefix: None, buttress: true },
    Form { value: "square", label: "Square", prefix: None, buttress: false },
];

const HANDS: &[(&str, &str)] = &[("Right", "right"), ("Left", "left"), ("Both", "both")];
const STARTS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];
const FULL: usize = 0;
const LENGTH: usize = 1;
const UP_TO_EXTENT: usize = 2;
const EXTENTS: &[(&str, &str)] = &[("Full", "full"), ("Length", "length"), ("Up to", "up_to")];
/// Runout choices (ISO 3508); the last one is a typed length.
const EASES: &[(&str, &str)] = &[("None", ""), ("Standard (2.5 P)", "standard"), ("Short (1.25 P)", "short"), ("Length", "")];
const EASE_LENGTH: usize = 3;
const TIPS: &[(&str, &str)] = &[("None", "none"), ("Chamfer", "chamfer")];
const FLANKS: &[(&str, &str)] = &[("End", "end"), ("Start", "start")];

/// The pick of the end the thread is measured from (`from`).
const START_END: &str = "Start end (optional)";
/// The face or plane an Up To extent runs to (`up_to`).
const UP_TO: &str = "Up to";
/// What the Up To pick accepts.
const UP_TO_ACCEPT: Accept = FACES | PLANES;

/// The selection inputs of a new thread dialog.
pub(crate) fn inputs() -> Vec<SelInput> {
    vec![SelInput::new("Cylindrical face", FACES, false)]
}

/// Add the picks the options use, and take away the others: the end to start from (optional,
/// so it does not take the next click), and the Up To place (which does).
pub(crate) fn sync_inputs(inputs: &mut Vec<SelInput>, active: &mut usize, o: &Options) {
    let was = *active;
    set_object_accepting(inputs, active, START_END, EDGES, o.starts_at_an_end());
    *active = was.min(inputs.len().saturating_sub(1));
    set_object_accepting(inputs, active, UP_TO, UP_TO_ACCEPT, o.extent == UP_TO_EXTENT);
}

/// A runout at one end: a choice from the runout list and the typed length.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ease {
    pub choice: usize,
    pub length: String,
}

/// The thread options beyond the size and length (indices into this module's choice lists).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub form: usize,
    pub hand: usize,
    /// The number of starts less one.
    pub starts: usize,
    pub extent: usize,
    pub ease_in: Ease,
    pub ease_out: Ease,
    pub tip: usize,
    pub chamfer_angle: String,
    pub chamfer_depth: String,
    pub steep_flank: usize,
}

impl Options {
    /// The options of a saved thread: the form its designation names (M, Tr or S), and a
    /// Length extent when it has a length.
    pub fn for_saved(designation: &str, length: &str) -> Options {
        Options { form: form_of(designation), extent: if length.trim().is_empty() { FULL } else { LENGTH }, ..Options::default() }
    }

    /// Does the thread start at one end of the face (a length or an Up To extent, a runout or a
    /// tip)? Then the end to start from can be picked.
    pub fn starts_at_an_end(&self) -> bool {
        self.extent != FULL || self.ease_in.choice != 0 || self.ease_out.choice != 0 || self.tip != 0
    }

    /// Does the dialog suggest a size from the face? The face's suggestion is an ISO metric size.
    pub fn suggests(&self) -> bool {
        self.form == 0
    }
}

/// The form a designation's prefix names: Tr is trapezoidal, S buttress, anything else metric.
fn form_of(designation: &str) -> usize {
    let t = designation.trim();
    let index = |value: &str| FORMS.iter().position(|f| f.value == value).unwrap_or(0);
    if t.starts_with("Tr") {
        index("trapezoidal")
    } else if t.strip_prefix('S').is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit())) {
        index("buttress")
    } else {
        0
    }
}

/// The sizes a form lists: each diameter with its pitches, the standard pitch first.
fn sizes(form: &Form) -> Vec<(f64, Vec<f64>)> {
    match form.value {
        "metric" => metric_thread_sizes().iter().map(|(d, coarse)| (*d, vec![*coarse])).collect(),
        "trapezoidal" => TRAPEZOIDAL.iter().map(|(d, p)| (*d, p.to_vec())).collect(),
        "buttress" => TRAPEZOIDAL.iter().filter(|(d, _)| *d >= 10.0).map(|(d, p)| (*d, p.to_vec())).collect(),
        _ => Vec::new(),
    }
}

/// The diameter and pitch a designation of this form names. A multi-start designation
/// (`Tr40x14(P7)`) names the lead and, in brackets, the pitch.
fn parse(form: &Form, designation: &str) -> Option<(f64, f64)> {
    let prefix = form.prefix?;
    if prefix == "M" {
        return parse_metric_thread(designation);
    }
    let rest = designation.trim().strip_prefix(prefix)?;
    let (d, rest) = rest.split_once(['x', 'X', '×'])?;
    let pitch = match rest.split_once('(') {
        Some((_, p)) => p.trim().strip_suffix(')')?.trim().strip_prefix(['P', 'p'])?,
        None => rest,
    };
    let (d, p) = (d.trim().parse::<f64>().ok()?, pitch.trim().parse::<f64>().ok()?);
    (d > 0.0 && p > 0.0).then_some((d, p))
}

fn designation_of(form: &Form, d: f64, p: f64) -> String {
    format!("{}{d}x{p}", form.prefix.unwrap_or_default())
}

/// The pitches the Pitch choice offers for diameter `d` (largest first), with the current one.
fn pitches(form: &Form, d: f64, current: f64) -> Vec<f64> {
    let listed = sizes(form).into_iter().find(|(n, _)| *n == d).map(|(_, p)| p);
    let mut pitches = Vec::new();
    if form.value == "metric" {
        let coarse = listed.as_ref().and_then(|p| p.first().copied());
        pitches.extend(PITCHES.iter().copied().filter(|p| *p < d && *p <= coarse.unwrap_or(d)));
        pitches.extend(coarse);
    } else {
        pitches.extend(listed.unwrap_or_default());
    }
    pitches.push(current);
    pitches.sort_by(|a, b| b.total_cmp(a));
    pitches.dedup();
    pitches
}

/// A new form keeps a manual size's diameter when the form lists it (with the form's standard
/// pitch); any other size goes back to the size that fits the face. True when the size went
/// back to automatic.
fn change_form(designation: &mut String, suggested: &mut Option<String>, from: usize, to: usize) -> bool {
    let automatic = designation.trim().is_empty() || suggested.as_deref() == Some(designation.as_str());
    let kept = (!automatic)
        .then(|| parse(&FORMS[from], designation))
        .flatten()
        .and_then(|(d, _)| sizes(&FORMS[to]).into_iter().find(|(n, _)| *n == d))
        .and_then(|(d, p)| p.first().map(|p| designation_of(&FORMS[to], d, *p)));
    *suggested = None;
    match kept {
        Some(kept) => {
            *designation = kept;
            false
        }
        None => {
            designation.clear();
            true
        }
    }
}

/// The thread's rows. Returns (Enter pressed, automatic sizing requested). The picks are the
/// caller's: [`sync_inputs`] adds or removes them by the options.
pub(crate) fn rows(
    ui: &mut egui::Ui,
    designation: &mut String,
    suggested: &mut Option<String>,
    length: &mut String,
    o: &mut Options,
    modeled: &mut bool,
) -> (bool, bool) {
    let mut enter = false;
    let mut reset = false;
    let form_labels: Vec<&str> = FORMS.iter().map(|f| f.label).collect();
    let mut form = o.form;
    choice_row(ui, "Form", "thread_form", &mut form, &form_labels);
    if form != o.form {
        reset |= change_form(designation, suggested, o.form, form);
        o.form = form;
    }
    let f = &FORMS[o.form];

    let before = designation.trim().is_empty() || suggested.as_deref() == Some(designation.as_str());
    let mut automatic = before;
    crate::ui_kit::section(ui, "thread_options", "Options", true, |ui| {
        crate::ui_kit::checkbox_grid(ui, &mut [("Modeled", modeled)], 2);
        ui.add_enabled_ui(!designation.trim().is_empty(), |ui| crate::ui_kit::checkbox_grid(ui, &mut [("Automatic size", &mut automatic)], 2));
    });
    reset |= automatic && !before;
    if automatic != before {
        *suggested = None;
        if automatic {
            designation.clear();
        }
    }

    if f.prefix.is_some() {
        size_rows(ui, f, designation, suggested);
    }
    let hint = match f.prefix {
        Some(prefix) => format!("{}{}", tr("fits the face, e.g. "), designation_of(f, 20.0, if prefix == "M" { 2.5 } else { 4.0 })),
        None => tr("fits the face").to_string(),
    };
    let (submitted, changed) = text_row(ui, "Designation", designation, &hint);
    enter |= submitted;
    if let Some(at) = crate::scenario::handle_at("dialog:Designation") {
        crate::scenario::publish_handle("dialog:thread_size", at);
    }
    if changed {
        *suggested = None;
    }

    let hands: Vec<&str> = HANDS.iter().map(|h| h.0).collect();
    choice_row(ui, "Hand", "thread_hand", &mut o.hand, &hands);
    choice_row(ui, "Starts", "thread_starts", &mut o.starts, STARTS);
    if f.buttress {
        let flanks: Vec<&str> = FLANKS.iter().map(|x| x.0).collect();
        choice_row(ui, "Steep Flank", "thread_flank", &mut o.steep_flank, &flanks);
    }

    let extents: Vec<&str> = EXTENTS.iter().map(|x| x.0).collect();
    let extent = o.extent;
    choice_row(ui, "Extent", "thread_extent", &mut o.extent, &extents);
    if o.extent == LENGTH {
        if extent != LENGTH && length.trim().is_empty() {
            *length = "10 mm".into();
        }
        enter |= expr_row(ui, "Length", length);
    }

    crate::ui_kit::section(ui, "thread_ends", "Ends", true, |ui| {
        enter |= ease_rows(ui, "Ease In", "thread_ease_in", &mut o.ease_in);
        enter |= ease_rows(ui, "Ease Out", "thread_ease_out", &mut o.ease_out);
        let tips: Vec<&str> = TIPS.iter().map(|x| x.0).collect();
        choice_row(ui, "Tip", "thread_tip", &mut o.tip, &tips);
        if o.tip == 1 {
            enter |= hint_row(ui, "Chamfer Angle", &mut o.chamfer_angle, tr("ISO 4753"));
            enter |= hint_row(ui, "Chamfer Depth", &mut o.chamfer_depth, tr("ISO 4753"));
        }
    });
    (enter, reset)
}

/// A runout choice, with its length when typed.
fn ease_rows(ui: &mut egui::Ui, label: &'static str, id: &str, e: &mut Ease) -> bool {
    let labels: Vec<&str> = EASES.iter().map(|x| x.0).collect();
    let before = e.choice;
    choice_row(ui, label, id, &mut e.choice, &labels);
    if e.choice != EASE_LENGTH {
        return false;
    }
    if before != EASE_LENGTH && e.length.trim().is_empty() {
        e.length = "1 mm".into();
    }
    expr_row(ui, if label == "Ease In" { "Ease In Length" } else { "Ease Out Length" }, &mut e.length)
}

/// The Size and Pitch choices of a form with a size list.
fn size_rows(ui: &mut egui::Ui, f: &Form, designation: &mut String, suggested: &mut Option<String>) {
    let table = sizes(f);
    let current = parse(f, designation);
    let mut diameters: Vec<f64> = table.iter().map(|(d, _)| *d).collect();
    if let Some((d, _)) = current
        && !diameters.contains(&d)
    {
        diameters.push(d);
        diameters.sort_by(f64::total_cmp);
    }
    form_row(ui, "Size", |ui| {
        let labels: Vec<String> = diameters.iter().map(|d| format!("{d} mm")).collect();
        let mut selected = current.and_then(|(d, _)| diameters.iter().position(|n| *n == d)).unwrap_or(diameters.len());
        let before = selected;
        let mut labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        if current.is_none() {
            labels.push("Select size");
        }
        combo(ui, "thread_diameter", &labels, &mut selected);
        // A new diameter starts with its standard pitch. The current custom diameter, if any,
        // is already selected and retains its explicit pitch.
        if selected != before
            && let Some(d) = diameters.get(selected)
            && let Some(p) = table.iter().find(|(n, _)| n == d).and_then(|(_, p)| p.first())
        {
            *designation = designation_of(f, *d, *p);
            *suggested = None;
        }
    });

    let current = parse(f, designation);
    form_row(ui, "Pitch", |ui| {
        ui.add_enabled_ui(current.is_some(), |ui| {
            let pitches = current.map(|(d, p)| pitches(f, d, p)).unwrap_or_default();
            let labels: Vec<String> = pitches.iter().map(|p| format!("{p} mm")).collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut selected = current.and_then(|(_, p)| pitches.iter().position(|n| *n == p)).unwrap_or(0);
            let before = selected;
            combo(ui, "thread_pitch", &labels, &mut selected);
            if selected != before
                && let (Some((d, _)), Some(p)) = (current, pitches.get(selected))
            {
                *designation = designation_of(f, d, *p);
                *suggested = None;
            }
        });
    });
}

/// Put the options into the command's parameters. Defaults are left out, so a thread without
/// them means what it always did.
pub(crate) fn put_params(p: &mut Value, length: &str, o: &Options, inputs: &[SelInput]) {
    let f = &FORMS[o.form];
    if o.form != 0 {
        p["form"] = json!(f.value);
    }
    if o.hand != 0 {
        p["hand"] = json!(HANDS[o.hand].1);
    }
    if o.starts != 0 {
        p["starts"] = json!(o.starts + 1);
    }
    match o.extent {
        LENGTH => {
            p["extent"] = json!(EXTENTS[LENGTH].1);
            put(p, &[("length", length)]);
        }
        UP_TO_EXTENT => {
            p["extent"] = json!(EXTENTS[UP_TO_EXTENT].1);
            if let Some(to) = up_to(inputs) {
                p["up_to"] = to;
            }
        }
        _ => {}
    }
    if let Some(from) = object_point(inputs, START_END) {
        p["from"] = from;
    }
    for (key, e) in [("ease_in", &o.ease_in), ("ease_out", &o.ease_out)] {
        match e.choice {
            EASE_LENGTH => put(p, &[(key, e.length.as_str())]),
            0 => {}
            c => p[key] = json!(EASES[c].1),
        }
    }
    if o.tip != 0 {
        p["tip"] = json!(TIPS[o.tip].1);
        put(p, &[("chamfer_angle", o.chamfer_angle.as_str()), ("chamfer_depth", o.chamfer_depth.as_str())]);
    }
    if f.buttress && o.steep_flank != 0 {
        p["steep_flank"] = json!(FLANKS[o.steep_flank].1);
    }
}

/// The picked Up To place: a plane's name, or a point on a face.
fn up_to(inputs: &[SelInput]) -> Option<Value> {
    match inputs.iter().find(|i| i.label == UP_TO)?.items.first()? {
        Sel::Plane { name } => Some(json!(name)),
        _ => object_point(inputs, UP_TO),
    }
}

#[cfg(test)]
#[path = "dialogs_thread_tests.rs"]
mod tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod contribution_tests {
    use super::*;
    use crate::{
        Services, SolveApp,
        dialogs::{Dialog, Kind, apply_commands},
    };
    use serde_json::json;
    use solidcraft_engine::{Sel, Session, geom::Vec3};
    fn hole() -> (SolveApp, Dialog) {
        let mut app = SolveApp::new(Session::default(), Services::default());
        app.run("solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Plate"})).unwrap();
        app.session.selection = vec![Sel::Face { body: "Plate".into(), index: 5, point: Vec3::new(10.0, 10.0, 10.0) }];
        let d = Dialog::for_command(&app, "solid.hole").unwrap();
        (app, d)
    }
    fn custom_hole_widget() -> crate::scenario::Harness {
        let (app, dialog) = hole();
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.dialog = Some(dialog);
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:Hole Tap Type"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Hole Tap Type:Tapped"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Size"}})).unwrap();
        let at = crate::scenario::handle_at("dialog:Size:M5").unwrap();
        h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":-2000}));
        h.frames(20);
        h.step(&json!({"widget":"Custom"})).unwrap();
        h.frames(3);
        h
    }

    fn replace_field(h: &mut crate::scenario::Harness, field: &str, value: &str) {
        h.step(&json!({"click":{"handle":format!("dialog:{field}")}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":value})).unwrap();
        h.frames(2);
    }

    #[test]
    fn custom_tapped_size_keeps_its_choice_and_exposes_nominal_diameter() {
        let h = custom_hole_widget();
        assert!(h.widget("Custom").is_some(), "Custom must remain the selected size after the popup closes");
        assert!(crate::scenario::handle_at("dialog:Nominal diameter").is_some(), "a custom tapped hole needs an editable nominal diameter");
        let card = crate::scenario::rect_of("dialog:card").unwrap();
        for label in ["Profile", "Size", "Nominal diameter", "Pitch", "Designation", "Tap drill diameter"] {
            let row = crate::scenario::rect_of(&format!("dialog:row:{label}")).unwrap();
            let control = crate::scenario::rect_of(&format!("dialog:control:{label}")).unwrap();
            assert_eq!(row.height(), crate::theme::Tokens::get().control_height, "{label}");
            assert!(card.contains_rect(row) && row.contains_rect(control), "{label}: {row:?} {control:?} outside {card:?}");
        }
    }

    #[test]
    fn custom_nominal_diameter_and_pitch_change_the_real_hole_and_reopen() {
        let mut h = custom_hole_widget();
        replace_field(&mut h, "Nominal diameter", "8.5 mm");
        replace_field(&mut h, "Pitch", "1 mm");
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands[0].1["thread"], "M8.5x1");
        assert!(commands[0].1.get("diameter").is_none(), "the nominal size must reach the engine through the thread designation");
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        h.frames(3);
        assert!(h.app.dialog.is_none());
        let faces = h.app.run("model.faces", json!({"body":"Plate"})).unwrap();
        assert!(
            faces["faces"].as_array().unwrap().iter().any(|face| { face["cylinder"]["radius"].as_f64().is_some_and(|r| (r - 3.75).abs() < 1e-9) }),
            "the physical tap drill must be 8.5 - 1 = 7.5 mm"
        );
        let saved = h.app.session.doc.to_json();
        let reopened = Session::new(Document::from_json(&saved).unwrap());
        let world = reopened.world_state();
        assert_eq!(world.threads.len(), 1);
        assert_eq!(world.threads[0].designation, "M8.5x1");
        assert!((world.threads[0].pitch - 1.).abs() < 1e-9);
    }

    #[test]
    fn an_invalid_custom_pitch_blocks_the_old_valid_size_and_cancel_keeps_the_model() {
        let mut h = custom_hole_widget();
        let before = h.app.session.doc.to_json();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        replace_field(&mut h, "Pitch", "5 mm");
        assert!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).is_err());
        assert_eq!(crate::scenario::count_of("dialog:ok:enabled"), Some(0.0));
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        assert!(h.app.dialog.is_some(), "an invalid custom draft must keep the dialog open");
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    }

    #[test]
    fn manual_designation_keeps_keyboard_focus_when_custom_fields_appear() {
        let (app, dialog) = hole();
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.dialog = Some(dialog);
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:Hole Tap Type"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Hole Tap Type:Tapped"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Designation"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        for text in ["M", "8", "x", "1"] {
            h.step(&json!({"text":text})).unwrap();
            h.frames(2);
        }
        assert_eq!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["thread"], "M8x1");
        assert!(crate::scenario::handle_at("dialog:Nominal diameter").is_some());
    }

    #[test]
    fn custom_lengths_use_explicit_units_and_invalid_drafts_never_apply_stale_values() {
        let (mut app, mut dialog) = hole();
        app.run("document.units", json!({"units":"in"})).unwrap();
        let Kind::Hole { opts, .. } = &mut dialog.kind else { panic!("not a hole") };
        opts.tapped = true;
        opts.all = true;
        let mut draft = HoleThreadDraft::new(&opts.thread);
        draft.diameter = "0.25 in".into();
        draft.pitch = "0.05 in".into();
        draft.update(&app.session.doc, &mut opts.thread);
        assert!(draft.error.is_none());
        let (d, p) = parse_metric_thread(&opts.thread).unwrap();
        assert!((d - 6.35).abs() < 1e-9 && (p - 1.27).abs() < 1e-9);
        opts.custom_thread = Some(draft);
        let commands = apply_commands(&app, &dialog).unwrap();
        app.run(&commands[0].0, commands[0].1.clone()).unwrap();
        let Kind::Hole { opts, .. } = &mut dialog.kind else { panic!("not a hole") };
        for invalid in ["", "0", "-1 mm", "unknown_size", "NaN"] {
            let draft = opts.custom_thread.as_mut().unwrap();
            draft.diameter = invalid.into();
            draft.update(&app.session.doc, &mut opts.thread);
            assert!(draft.error.is_some(), "{invalid}");
        }
        assert!(apply_commands(&app, &dialog).is_err());
    }
    #[test]
    fn contribution_tapped_holes_derive_the_drill_from_the_catalogue() {
        for (designation, expected) in [("M5", 4.2), ("M6x1", 5.0), ("M5x0.5", 4.5)] {
            let (mut app, mut d) = hole();
            if let Kind::Hole { diameter, opts, .. } = &mut d.kind {
                *diameter = "17 mm".into();
                opts.tapped = true;
                opts.all = true;
                opts.thread = designation.into();
            }
            assert!(d.primary().is_none(), "no independent diameter editor for a tapped hole");
            let commands = apply_commands(&app, &d).unwrap();
            let (id, p) = &commands[0];
            assert!(p.get("diameter").is_none());
            assert_eq!(p["thread"], designation);
            let out = app.run(id, p.clone()).unwrap();
            let f = app.session.doc.feature(out["feature"].as_u64().unwrap()).unwrap();
            let solidcraft_engine::doc::FeatureKind::Hole { diameter, .. } = &f.kind else { panic!("not a Hole") };
            let drill = app.session.doc.eval(diameter, solidcraft_engine::doc::expr::Kind::Length).unwrap();
            assert!((drill - expected).abs() < 1e-9, "{designation}: {drill}");
        }
    }
    #[test]
    fn contribution_invalid_tapped_holes_are_refused_and_plain_holes_keep_diameter() {
        let (app, mut d) = hole();
        assert!(d.primary().is_some());
        assert!(apply_commands(&app, &d).unwrap()[0].1.get("diameter").is_some());
        for designation in ["", "M0", "M5x5", "M5xNaN", "not a thread"] {
            if let Kind::Hole { opts, .. } = &mut d.kind {
                opts.tapped = true;
                opts.thread = designation.into();
            }
            assert!(apply_commands(&app, &d).is_err(), "{designation}");
        }
    }
    #[test]
    fn contribution_thread_catalogue_is_parseable_and_rendering_keeps_custom_designations() {
        for (d, p) in metric_thread_sizes() {
            assert_eq!(parse_metric_thread(&format!("M{d}")), Some((*d, *p)));
            assert_eq!(parse_metric_thread(&format!("M{d}x{p}")), Some((*d, *p)));
        }
        let ctx = egui::Context::default();
        for original in ["M5", "M5x0.5", "M33x1", "1/4-20 UNC", "#10-32 UNF", "invalid"] {
            let mut designation = original.to_string();
            let mut custom = None;
            let doc = Document::default();
            let _ = ctx.run_ui(Default::default(), |ui| {
                egui::Grid::new("hole_catalogue").show(ui, |ui| {
                    hole_rows(ui, &doc, &mut designation, &mut custom);
                });
            });
            assert_eq!(designation, original);
        }
    }

    #[test]
    fn expanded_catalogue_profiles_select_unified_sizes_and_keep_metric_custom_mode() {
        let mut h = custom_hole_widget();
        h.step(&json!({"click":{"handle":"dialog:Profile"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Profile:UNC"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Size"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Size:1/4-20 UNC"}})).unwrap();
        h.frames(3);
        assert_eq!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["thread"], "1/4-20 UNC");
        assert!(crate::scenario::handle_at("dialog:Nominal diameter").is_none());
        assert!(h.widget("5.1054 mm").is_some());
        h.step(&json!({"click":{"handle":"dialog:Profile"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Profile:UNF"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Size"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Size:#10-32 UNF"}})).unwrap();
        h.frames(3);
        assert_eq!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["thread"], "#10-32 UNF");
        h.step(&json!({"click":{"handle":"dialog:Profile"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Profile:ISO metric"}})).unwrap();
        if let Some(Dialog { kind: Kind::Hole { opts, .. }, .. }) = h.app.dialog.as_mut() {
            opts.thread = "M8x1".into();
        }
        h.frames(3);
        assert!(h.widget("M8x1").is_some());
        assert!(crate::scenario::handle_at("dialog:Nominal diameter").is_none());
        // Choosing Custom is persistent even at a now-catalogued fine pitch.
        if let Some(Dialog { kind: Kind::Hole { opts, .. }, .. }) = h.app.dialog.as_mut() {
            opts.custom_thread = Some(HoleThreadDraft::new(&opts.thread));
        }
        h.frames(3);
        assert!(h.widget("Custom").is_some());
        assert!(crate::scenario::handle_at("dialog:Nominal diameter").is_some());
    }

    #[test]
    fn expanded_catalogue_drills_real_cylinders_and_reopens_without_resizing() {
        for (name, major, pitch, drill) in [
            ("1/4-20 UNC", 6.35, 1.27, 5.1054),
            ("#10-32 UNF", 4.826, 25.4 / 32.0, 4.0386),
            ("M8", 8.0, 1.25, 6.75),
            ("M8x1", 8.0, 1.0, 7.0),
            ("M8.5x1", 8.5, 1.0, 7.5),
        ] {
            let (mut app, _) = hole();
            let out = app.run("solid.hole", json!({"position":[10,10,10], "thread":name})).unwrap();
            let id = out["feature"].as_u64().unwrap();
            let saved = app.session.doc.to_json();
            let reopened = Session::new(Document::from_json(&saved).unwrap());
            let world = reopened.world_state();
            let t = &world.threads[0];
            assert_eq!(t.designation, name);
            assert!((t.major_diameter - major).abs() < 1e-9 && (t.pitch - pitch).abs() < 1e-9);
            let body = &world.bodies[0].body;
            assert!(body.validity().is_empty());
            let face = solidcraft_engine::kernel::cylinder_face_at(body, Vec3::new(10.0 + drill / 2.0, 10.0, 5.0)).unwrap();
            assert!(face.internal && (2.0 * face.radius - drill).abs() < 1e-8);
            let actual = body.tessellate(0.001).unwrap().measure().volume;
            let expected = 4000.0 - std::f64::consts::PI * (drill / 2.0).powi(2) * 10.0;
            assert!((actual - expected).abs() < 0.5, "{name}: {actual} versus {expected}");
            // A saved explicit drill remains authoritative, including a legacy metric bore.
            let mut doc = Document::from_json(&saved).unwrap();
            let solidcraft_engine::doc::FeatureKind::Hole { diameter, .. } = &mut doc.feature_mut(id).unwrap().kind else { panic!("not Hole") };
            *diameter = format!("{} mm", drill + 0.05).as_str().into();
            let reopened = Session::new(Document::from_json(&doc.to_json()).unwrap());
            let world = reopened.world_state();
            let face = solidcraft_engine::kernel::cylinder_face_at(&world.bodies[0].body, Vec3::new(10.0 + (drill + 0.05) / 2.0, 10.0, 5.0)).unwrap();
            assert!((2.0 * face.radius - drill - 0.05).abs() < 1e-8);
        }
    }

    #[test]
    fn expanded_catalogue_rejects_tapered_holes_and_nonmetric_thread_features() {
        let (mut app, _) = hole();
        let before = app.session.doc.to_json();
        for name in ["1/4-18 NPT", "Rc1/4", "R1/4", "1/4-20 UNF"] {
            assert!(app.run("solid.hole", json!({"position":[10,10,10],"thread":name})).is_err());
            assert_eq!(app.session.doc.to_json(), before);
        }
        app.run("solid.hole", json!({"position":[10,10,10],"thread":"M8"})).unwrap();
        let before = app.session.doc.to_json();
        for designation in ["1/4-20 UNC", "G 1/4", "Rp 1/4"] {
            for modeled in [false, true] {
                assert!(app.run("solid.thread", json!({"face":[13.375,10,5],"designation":designation,"modeled":modeled})).is_err());
                assert_eq!(app.session.doc.to_json(), before);
            }
        }
        assert_eq!(app.session.doc.to_json(), before);
        let out = app.run("solid.thread", json!({"face":[13.375,10,5],"designation":"M8"})).unwrap();
        let solidcraft_engine::doc::FeatureKind::Thread { designation, modeled, .. } =
            &mut app.session.doc_mut().feature_mut(out["feature"].as_u64().unwrap()).unwrap().kind
        else {
            panic!("not Thread")
        };
        *designation = Some("1/4-20 UNC".into());
        *modeled = true;
        let reopened = Session::new(Document::from_json(&app.session.doc.to_json()).unwrap());
        assert!(reopened.model.results.last().unwrap().error.as_ref().unwrap().contains("ISO metric"));
    }

    #[test]
    fn expanded_catalogue_public_plate_evidence_uses_only_headless_model_rendering() {
        let Some(dir) = std::env::var_os("SOLIDCRAFT_CATALOGUE_EVIDENCE") else { return };
        crate::gpu::install_cpu_picture_renderer();
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":50,"width":25,"height":6,"body_name":"ThreadPlate"})).unwrap();
        for (x, name) in [(10, "M8x1"), (25, "1/4-20 UNC"), (40, "#10-32 UNF")] {
            h.app.run("solid.hole", json!({"position":[x,12.5,6],"thread":name})).unwrap();
        }
        h.call("ui.view", json!({"view":"iso"}));
        let path = std::path::Path::new(&dir).join("catalogue-plate.png");
        let out = h.call("ui.render", json!({"path":path,"width":1000,"height":700,"grid":false,"axes":false}));
        assert_eq!(out["ok"], true, "{out}");
        std::fs::write(std::path::Path::new(&dir).join("catalogue-plate.solidcraft"), h.app.session.doc.to_json()).unwrap();
    }

    #[test]
    fn contribution_metric_designation_omits_only_the_catalogue_coarse_pitch() {
        for &(d, p) in metric_thread_sizes() {
            assert_eq!(designation(d, p), format!("M{d}"));
            assert_eq!(parse_metric_thread(&designation(d, p)), Some((d, p)));
            assert_eq!(parse_metric_thread(&designation(d, p / 2.)), Some((d, p / 2.)));
            assert!(designation(d, p / 2.).contains('x'));
        }
        assert_eq!(designation(8., 1.25), "M8");
        assert_eq!(designation(8., 1.), "M8x1");
        assert_eq!(designation(5., 0.5), "M5x0.5");
        assert_eq!(designation(33., 1.), "M33x1");
        assert_eq!(designation(8., 1.25 + 1e-12), "M8");
        assert!(designation(8., 1.25 + 1e-6).contains('x'));
    }

    #[test]
    fn contribution_metric_designation_rendering_preserves_authored_legacy_and_custom_text() {
        let context = egui::Context::default();
        crate::fonts::setup_bundled_fonts(&context);
        for original in ["M8x1.25", "M8x1", "M33x1", "M5×0.5", " m8x1.25 ", "invalid"] {
            let mut hole = original.to_string();
            let mut thread = original.to_string();
            let mut suggested = None;
            let mut modeled = false;
            let mut full = true;
            let mut custom = None;
            let doc = Document::default();
            let _ = context.run_ui(Default::default(), |ui| {
                egui::Grid::new("authored_hole").show(ui, |ui| {
                    hole_rows(ui, &doc, &mut hole, &mut custom);
                });
                egui::Grid::new("authored_thread").show(ui, |ui| {
                    rows(ui, &mut thread, &mut suggested, &mut modeled, &mut full);
                });
            });
            assert_eq!(hole, original);
            assert_eq!(thread, original);
        }
    }

    #[test]
    fn contribution_metric_designation_keeps_domain_pitch_drill_units_and_reopen() {
        for (diameter, pitch, drill) in [(8., 1.25, 6.75), (8., 1., 7.), (5., 0.5, 4.5)] {
            let (mut app, mut dialog) = hole();
            app.run("document.units", json!({"units":"in"})).unwrap();
            if let Kind::Hole { opts, .. } = &mut dialog.kind {
                opts.tapped = true;
                opts.all = true;
                opts.thread = designation(diameter, pitch);
            }
            let commands = apply_commands(&app, &dialog).unwrap();
            let result = app.run(&commands[0].0, commands[0].1.clone()).unwrap();
            let feature = app.session.doc.feature(result["feature"].as_u64().unwrap()).unwrap();
            let solidcraft_engine::doc::FeatureKind::Hole { diameter, .. } = &feature.kind else { panic!("not a hole") };
            assert!((app.session.doc.eval(diameter, solidcraft_engine::doc::expr::Kind::Length).unwrap() - drill).abs() < 1e-9);
            let saved = app.session.doc.to_json();
            let reopened = Session::new(solidcraft_engine::doc::Document::from_json(&saved).unwrap());
            let state = reopened.world_state();
            assert_eq!(state.threads.len(), 1);
            assert!((state.threads[0].pitch - pitch).abs() < 1e-9);
        }
    }
    #[test]
    fn parallel_pipe_profiles_switch_and_cancel_without_changing_the_document() {
        let mut h = custom_hole_widget();
        let before = h.app.session.doc.to_json();
        for (profile, name, drill) in [("G / BSPP", "G 1/4", "11.75 mm"), ("Rp", "Rp 1/4", "11.4 mm")] {
            h.step(&json!({"click":{"handle":"dialog:Profile"}})).unwrap();
            h.step(&json!({"click":{"handle":format!("dialog:Profile:{profile}")}})).unwrap();
            h.step(&json!({"click":{"handle":"dialog:Size"}})).unwrap();
            h.step(&json!({"click":{"handle":format!("dialog:Size:{name}")}})).unwrap();
            h.frames(3);
            let command = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
            assert_eq!(command[0].1["thread"], name);
            assert!(command[0].1.get("diameter").is_none());
            assert!(h.widget(drill).is_some());
            assert!(crate::scenario::handle_at("dialog:Nominal diameter").is_none());
        }
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn parallel_pipe_holes_have_the_tabulated_cylinders_depth_and_saved_designations() {
        use solidcraft_engine::doc::{TappedThreadFamily, tapped_thread_sizes};
        for family in [TappedThreadFamily::G, TappedThreadFamily::Rp] {
            for size in tapped_thread_sizes(family) {
                let mut app = SolveApp::new(Session::default(), Services::default());
                app.run("solid.box", json!({"length":80,"width":80,"height":15,"body_name":"Plate"})).unwrap();
                let result = app.run("solid.hole", json!({"position":[40,40,15],"thread":size.designation,"depth":10})).unwrap();
                let saved = app.session.doc.to_json();
                let reopened = Session::new(Document::from_json(&saved).unwrap());
                let state = reopened.world_state();
                assert_eq!(state.threads.len(), 1);
                let t = &state.threads[0];
                assert_eq!(t.designation, size.designation);
                assert!((t.major_diameter - size.major_diameter).abs() < 1e-9 && (t.pitch - size.pitch).abs() < 1e-9);
                assert!((t.end - t.start - 10.0).abs() < 1e-6);
                let body = &state.bodies[0].body;
                assert!(body.validity().is_empty());
                for z in [6.0, 10.0, 14.0] {
                    let c = solidcraft_engine::kernel::cylinder_face_at(body, Vec3::new(40.0 + size.tap_drill / 2.0, 40.0, z)).unwrap();
                    assert!(c.internal && (2.0 * c.radius - size.tap_drill).abs() < 1e-8);
                }
                let actual = body.tessellate(0.001).unwrap().measure().volume;
                let removed = std::f64::consts::PI * (size.tap_drill / 2.0).powi(2) * 10.0;
                assert!((actual - (96000.0 - removed)).abs() / removed < 0.002, "{}: {actual}", size.designation);
                assert_eq!(reopened.doc.to_json(), saved);
                if size.designation.ends_with(" 1/4") {
                    let mut doc = Document::from_json(&saved).unwrap();
                    let solidcraft_engine::doc::FeatureKind::Hole { diameter, .. } =
                        &mut doc.feature_mut(result["feature"].as_u64().unwrap()).unwrap().kind
                    else {
                        panic!("not Hole")
                    };
                    *diameter = format!("{} mm", size.tap_drill + 0.03).as_str().into();
                    let reopened = Session::new(Document::from_json(&doc.to_json()).unwrap());
                    let state = reopened.world_state();
                    let c = solidcraft_engine::kernel::cylinder_face_at(
                        &state.bodies[0].body,
                        Vec3::new(40.0 + (size.tap_drill + 0.03) / 2.0, 40.0, 10.0),
                    )
                    .unwrap();
                    assert!((2.0 * c.radius - size.tap_drill - 0.03).abs() < 1e-8);
                }
            }
        }
    }
}
