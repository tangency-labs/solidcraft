//! The thread dialog: form, size, hand, starts, extent, runouts, tip and the modelled option of
//! `solid.thread`. Choices only build the command's parameters: the command checks the geometry
//! and its error text is what the dialog shows. Browsing choices does not edit the model or
//! freeze an automatic feature. Sizes, designations and the face's size come from the document's
//! thread forms ([`ThreadForm`]).

use serde_json::{Value, json};
use solidcraft_engine::doc::thread_forms::{SteepFlank, ThreadForm, ThreadHand, ThreadTip};
use solidcraft_engine::doc::{
    Document, Expr, FeatureKind, TappedThreadFamily, ThreadLimit, ThreadRunout, metric_thread_sizes, parse_metric_thread, parse_tapped_thread,
    tapped_thread_sizes,
};
use solidcraft_engine::{Sel, Session};

use crate::dialogs::{Dialog, Kind, combo, edge_sel, face_sel, form_row, object_point, plane_sel, put, set_object_accepting};
use crate::i18n::tr;
use crate::selection::{Accept, EDGES, FACES, PLANES, SelInput};
use crate::ui_kit::{choice_row, expr_row, hint_row, text_row};

/// Common metric pitch presets besides the coarse one, not an exhaustive catalogue of standard
/// diameter/pitch pairs; the current pitch is included separately, custom pairs remain editable.
const METRIC_PITCHES: &[f64] = &[0.2, 0.25, 0.35, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

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
pub(crate) fn hole_rows(
    ui: &mut egui::Ui,
    doc: &Document,
    designation: &mut String,
    custom: &mut Option<HoleThreadDraft>,
    drill: &mut Option<String>,
) -> bool {
    let before = designation.clone();
    let designation_id = ui.make_persistent_id("hole_thread_designation");
    let families = [
        TappedThreadFamily::Metric,
        TappedThreadFamily::Unc,
        TappedThreadFamily::Unf,
        TappedThreadFamily::G,
        TappedThreadFamily::Rp,
        TappedThreadFamily::Rc,
        TappedThreadFamily::Npt,
    ];
    let family = parse_tapped_thread(designation).map(|s| s.family).unwrap_or(TappedThreadFamily::Metric);
    let mut profile = families.iter().position(|f| *f == family).unwrap_or(0);
    let before_profile = profile;
    form_row(ui, "Profile", |ui| combo(ui, "hole_thread_profile", &["ISO metric", "UNC", "UNF", "G / BSPP", "Rp", "Rc", "NPT"], &mut profile));
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
            TappedThreadFamily::Rc => "hole_thread_catalogue_rc",
            TappedThreadFamily::Npt => "hole_thread_catalogue_npt",
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
    if matches!(family, TappedThreadFamily::G | TappedThreadFamily::Rp | TappedThreadFamily::Rc | TappedThreadFamily::Npt) {
        form_row(ui, "Thread diameter", |ui| {
            if let Some(size) = parse_tapped_thread(designation) {
                ui.label(format!("{:.3} mm, {}°", size.major_diameter, size.profile_degrees()));
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
    // The drill: the tap maker's for the size, or one the user types, from the thread's minor
    // diameter to 5% over the tap maker's. A new size goes back to its own drill.
    if *designation != before {
        *drill = None;
    }
    let mut drill_enter = false;
    form_row(ui, "Tap drill diameter", |ui| {
        match parse_tapped_thread(designation).filter(|_| custom.as_ref().is_none_or(|draft| draft.error.is_none())) {
            Some(size) => {
                let default = format!("{} mm", size.tap_drill);
                let (lo, hi) = size.drill_range();
                let mm = |v: f64| format!("{:.3}", v);
                let args = [("drill", mm(size.tap_drill)), ("minor", mm(size.minor_diameter())), ("low", mm(lo)), ("high", mm(hi))];
                let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                let id = match (size.taper_drill().is_some(), size.drill_is_tabulated()) {
                    (true, _) => "thread.drill_range_taper",
                    (false, true) => "thread.drill_range",
                    (false, false) => "thread.drill_range_custom",
                };
                let tip = crate::i18n::format(id, &args);
                let mut text = drill.clone().unwrap_or_else(|| default.clone());
                let (submitted, changed) = crate::ui_kit::text_field_tip(ui, &mut text, &default, &tip);
                if changed {
                    *drill = (text.trim() != default && !text.trim().is_empty()).then_some(text);
                }
                drill_enter = submitted;
            }
            None => {
                ui.label(tr("thread.select_size"));
            }
        }
    });
    enter || drill_enter || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
}

/// The forms offered. The square thread is left out: no standard defines it, and the command
/// refuses it ([`ThreadForm::refused`]).
const FORMS: &[(&str, ThreadForm)] = &[
    ("ISO metric", ThreadForm::Metric),
    ("Trapezoidal (ISO 2904)", ThreadForm::Trapezoidal),
    ("Acme (ASME B1.5)", ThreadForm::Acme),
    ("Buttress (DIN 513)", ThreadForm::Buttress),
    ("Buttress (ASME B1.9)", ThreadForm::ButtressAsme),
    ("thread.form_bspp", ThreadForm::Bspp),
    ("thread.form_rp", ThreadForm::Rp),
    ("thread.form_bspt", ThreadForm::Bspt),
    ("thread.form_npt", ThreadForm::Npt),
];
const HANDS: &[(&str, ThreadHand)] = &[("Right", ThreadHand::Right), ("Left", ThreadHand::Left), ("Both", ThreadHand::Both)];
/// A tapped hole's and a pipe thread's hands: one groove, so never both.
const HOLE_HANDS: &[(&str, ThreadHand)] = &[("Right", ThreadHand::Right), ("Left", ThreadHand::Left)];
const STARTS: &[(&str, u32)] = &[("1", 1), ("2", 2), ("3", 3), ("4", 4), ("5", 5), ("6", 6), ("7", 7), ("8", 8)];
const FULL: usize = 0;
const LENGTH: usize = 1;
const UP_TO_EXTENT: usize = 2;
const EXTENTS: &[(&str, &str)] = &[("Full", "full"), ("Length", "length"), ("Up to", "up_to")];
/// The thread's start, its free end: run off it plain, or chamfered (ISO 4753).
const STARTS_AT: &[&str] = &["None", "Chamfer"];
/// The thread's inner end: a square stop (the groove ends in its own profile) or a fade
/// (ISO 3508's runout).
const ENDS_AT: &[&str] = &["thread.end_square", "ui.fade"];
const FLANKS: &[(&str, SteepFlank)] = &[("End", SteepFlank::End), ("Start", SteepFlank::Start)];

/// The pick of the end the thread is measured from (`from`).
const START_END: &str = "Start end (optional)";
/// The face or plane an Up To extent runs to (`up_to`).
const UP_TO: &str = "Up to";
const UP_TO_ACCEPT: Accept = FACES | PLANES;

/// The selection inputs of a new thread dialog.
pub(crate) fn inputs() -> Vec<SelInput> {
    vec![SelInput::new("Cylindrical face", FACES, false)]
}

/// The inner end: square or a fade, the fade's typed length (empty: ISO 3508's runout, the
/// standard 2.5 P unless the thread was saved with the short 1.25 P).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ease {
    pub fade: bool,
    pub length: String,
    pub short: bool,
}

impl Ease {
    pub(crate) fn of(r: &Option<ThreadRunout>) -> Ease {
        match r {
            None => Ease::default(),
            Some(ThreadRunout::Standard) => Ease { fade: true, ..Ease::default() },
            Some(ThreadRunout::Short) => Ease { fade: true, short: true, ..Ease::default() },
            Some(ThreadRunout::Length { length }) => Ease { fade: true, length: expr(length), short: false },
        }
    }

    /// The command's `ease_out`, none for a square end.
    pub(crate) fn param(&self) -> Option<Value> {
        let typed = self.length.trim();
        self.fade.then(|| match (typed.is_empty(), self.short) {
            (false, _) => json!(typed),
            (true, true) => json!("short"),
            (true, false) => json!("standard"),
        })
    }
}

fn expr(e: &Expr) -> String {
    e.to_string()
}

/// The thread options beyond the size and length.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub form: ThreadForm,
    pub hand: ThreadHand,
    pub starts: u32,
    /// An index into the extents (full, length, up to).
    pub extent: usize,
    pub ease_out: Ease,
    pub tip: ThreadTip,
    pub chamfer_angle: String,
    pub chamfer_depth: String,
    pub steep_flank: SteepFlank,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            form: ThreadForm::Metric,
            hand: ThreadHand::default(),
            starts: 1,
            extent: FULL,
            ease_out: Ease::default(),
            tip: ThreadTip::default(),
            chamfer_angle: String::new(),
            chamfer_depth: String::new(),
            steep_flank: SteepFlank::default(),
        }
    }
}

impl Options {
    /// Does the thread start at one end of the face (a length or an Up To extent, a runout or a
    /// tip)? Then the end to start from can be picked.
    pub fn starts_at_an_end(&self) -> bool {
        self.extent != FULL || self.ease_out.fade || self.tip != ThreadTip::None
    }
}

fn is_buttress(form: ThreadForm) -> bool {
    matches!(form, ThreadForm::Buttress | ThreadForm::ButtressAsme)
}

/// Sizes in inches are listed by designation, and their pitch is part of it (threads per inch):
/// the inch forms and the pipe threads, whose nominal sizes are pipe sizes.
fn inch(form: ThreadForm) -> bool {
    matches!(form, ThreadForm::Acme | ThreadForm::ButtressAsme) || is_pipe(form)
}

/// A pipe thread: one start, turning one way.
fn is_pipe(form: ThreadForm) -> bool {
    !form.pipe_sizes().is_empty()
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// The diameter and pitch (mm) a designation of this form names.
fn parse(form: ThreadForm, designation: &str) -> Option<(f64, f64)> {
    form.parse(designation, 1).ok().map(|t| (t.major, t.pitch))
}

fn designation_of(form: ThreadForm, d: f64, p: f64) -> String {
    form.format(d, p, p)
}

/// The pitches the Pitch choice offers for diameter `d` (largest first), with the current one.
fn pitches(form: ThreadForm, d: f64, current: f64) -> Vec<f64> {
    let standard = form.sizes().into_iter().find(|(n, _)| same(*n, d)).map(|(_, p)| p);
    let mut pitches = Vec::new();
    if form == ThreadForm::Metric {
        pitches.extend(METRIC_PITCHES.iter().copied().filter(|p| *p < d && *p <= standard.unwrap_or(d)));
    }
    pitches.extend(standard);
    pitches.push(current);
    pitches.sort_by(|a, b| b.total_cmp(a));
    pitches.dedup_by(|a, b| same(*a, *b));
    pitches
}

/// A typical designation of the form, for the field's hint.
fn example(form: ThreadForm) -> String {
    let sizes = form.sizes();
    match sizes.iter().min_by(|a, b| (a.0 - 20.0).abs().total_cmp(&(b.0 - 20.0).abs())) {
        Some((d, p)) => designation_of(form, *d, *p),
        // ASME B1.9 has no size table: 2.5 in, 8 threads per inch.
        None => designation_of(form, 2.5 * 25.4, 25.4 / 8.0),
    }
}

/// A new form keeps a manual size's diameter when the form lists it (with the form's standard
/// pitch); any other size goes back to the size that fits the face. True when the size went
/// back to automatic.
fn change_form(designation: &mut String, suggested: &mut Option<String>, from: ThreadForm, to: ThreadForm) -> bool {
    let automatic = designation.trim().is_empty() || suggested.as_deref() == Some(designation.as_str());
    let kept = (!automatic)
        .then(|| parse(from, designation))
        .flatten()
        .and_then(|(d, _)| to.sizes().into_iter().find(|(n, _)| same(*n, d)))
        .map(|(d, p)| designation_of(to, d, p));
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

/// A choice row over typed values.
/// The hole dialog's Hand row for a tapped hole, the same choice the thread dialog offers.
pub(crate) fn hole_hand_row(ui: &mut egui::Ui, hand: &mut ThreadHand) {
    pick(ui, "Hand", "hole_hand", hand, HOLE_HANDS);
}

fn pick<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, id: &str, value: &mut T, choices: &[(&str, T)]) {
    let labels: Vec<&str> = choices.iter().map(|c| c.0).collect();
    let mut index = choices.iter().position(|c| c.1 == *value).unwrap_or(0);
    choice_row(ui, label, id, &mut index, &labels);
    if let Some(c) = choices.get(index) {
        *value = c.1;
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
    let mut form = o.form;
    pick(ui, "Form", "thread_form", &mut form, FORMS);
    if form != o.form {
        reset |= change_form(designation, suggested, o.form, form);
        o.form = form;
    }
    let f = o.form;

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

    if !f.sizes().is_empty() {
        size_rows(ui, f, designation, suggested);
    }
    let hint = crate::i18n::format("thread.fits_the_face", &[("example", &example(f))]);
    let (submitted, changed) = text_row(ui, "Designation", designation, &hint);
    enter |= submitted;
    if let Some(at) = crate::scenario::handle_at("dialog:Designation") {
        crate::scenario::publish_handle("dialog:thread_size", at);
    }
    if changed {
        *suggested = None;
    }

    if is_pipe(f) {
        // A pipe thread has one start and turns one way.
        o.starts = 1;
        if o.hand == ThreadHand::Both {
            o.hand = ThreadHand::Right;
        }
        pick(ui, "Hand", "thread_hand", &mut o.hand, HOLE_HANDS);
    } else {
        pick(ui, "Hand", "thread_hand", &mut o.hand, HANDS);
        pick(ui, "Starts", "thread_starts", &mut o.starts, STARTS);
    }
    if is_buttress(f) {
        pick(ui, "Steep Flank", "thread_flank", &mut o.steep_flank, FLANKS);
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
        // The start, a free end: plain or chamfered. A thread saved chamfered at both ends
        // keeps that.
        let mut chamfered = usize::from(o.tip != ThreadTip::None);
        choice_row(ui, "Start", "thread_start", &mut chamfered, STARTS_AT);
        o.tip = match (chamfered, o.tip) {
            (0, _) => ThreadTip::None,
            (_, ThreadTip::None) => ThreadTip::Start,
            (_, kept) => kept,
        };
        if o.tip != ThreadTip::None {
            enter |= hint_row(ui, "Chamfer Angle", &mut o.chamfer_angle, tr("ui.iso_4753"));
            // ISO 4753's depth is for ISO metric threads; the other forms give theirs.
            let depth = if f == ThreadForm::Metric { "ui.iso_4753" } else { "thread.chamfer_depth_needed" };
            enter |= hint_row(ui, "Chamfer Depth", &mut o.chamfer_depth, tr(depth));
        }
        enter |= end_rows(ui, &mut o.ease_out);
    });
    (enter, reset)
}

/// The inner end of a modelled thread (a thread's, or a tapped hole's towards its bottom):
/// square, or a fade over its length.
pub(crate) fn end_rows(ui: &mut egui::Ui, ease: &mut Ease) -> bool {
    let mut fade = usize::from(ease.fade);
    choice_row(ui, "End", "thread_end", &mut fade, ENDS_AT);
    ease.fade = fade == 1;
    if !ease.fade {
        return false;
    }
    let hint = if ease.short { tr("thread.fade_short") } else { tr("thread.fade_standard") };
    hint_row(ui, "Fade Length", &mut ease.length, hint)
}

/// The Size choice of a form with a size table, and the Pitch choice of the metric ones (an
/// inch designation names its threads per inch).
fn size_rows(ui: &mut egui::Ui, f: ThreadForm, designation: &mut String, suggested: &mut Option<String>) {
    let table = f.sizes();
    let current = parse(f, designation);
    // ISO 7-1 writes the internal taper thread Rc: a size picked for a hole the dialog sized Rc
    // stays Rc.
    let internal = f == ThreadForm::Bspt && ThreadForm::pipe_internal(designation);
    let designation_of = |f: ThreadForm, d: f64, p: f64| {
        let name = designation_of(f, d, p);
        if internal { name.replacen("R ", "Rc ", 1) } else { name }
    };
    let mut diameters: Vec<f64> = table.iter().map(|(d, _)| *d).collect();
    if let Some((d, _)) = current
        && !diameters.iter().any(|n| same(*n, d))
    {
        diameters.push(d);
        diameters.sort_by(f64::total_cmp);
    }
    form_row(ui, "Size", |ui| {
        let labels: Vec<String> = diameters
            .iter()
            .map(|d| match table.iter().find(|(n, _)| same(*n, *d)) {
                Some((n, p)) if inch(f) => designation_of(f, *n, *p),
                _ if inch(f) => designation.trim().to_string(),
                _ => format!("{d} mm"),
            })
            .collect();
        let mut selected = current.and_then(|(d, _)| diameters.iter().position(|n| same(*n, d))).unwrap_or(diameters.len());
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
            && let Some((_, p)) = table.iter().find(|(n, _)| same(*n, *d))
        {
            *designation = designation_of(f, *d, *p);
            *suggested = None;
        }
    });
    if inch(f) {
        return;
    }

    let current = parse(f, designation);
    form_row(ui, "Pitch", |ui| {
        ui.add_enabled_ui(current.is_some(), |ui| {
            let pitches = current.map(|(d, p)| pitches(f, d, p)).unwrap_or_default();
            let labels: Vec<String> = pitches.iter().map(|p| format!("{p} mm")).collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut selected = current.and_then(|(_, p)| pitches.iter().position(|n| same(*n, p))).unwrap_or(0);
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

/// Add the picks the options use, and take away the others: the end to start from (optional,
/// so it does not take the next click), and the Up To place (which does).
pub(crate) fn sync_inputs(inputs: &mut Vec<SelInput>, active: &mut usize, o: &Options) {
    let was = *active;
    set_object_accepting(inputs, active, START_END, EDGES, o.starts_at_an_end());
    *active = was.min(inputs.len().saturating_sub(1));
    set_object_accepting(inputs, active, UP_TO, UP_TO_ACCEPT, o.extent == UP_TO_EXTENT);
}

/// Put the options into the command's parameters. Defaults are left out, so a thread without
/// them means what it always did.
pub(crate) fn put_params(p: &mut Value, length: &str, o: &Options, inputs: &[SelInput]) {
    if o.form != ThreadForm::Metric {
        p["form"] = json!(o.form.name());
    }
    if o.hand != ThreadHand::default() {
        p["hand"] = json!(o.hand);
    }
    if o.starts != 1 {
        p["starts"] = json!(o.starts);
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
    if let Some(e) = o.ease_out.param() {
        p["ease_out"] = e;
    }
    if o.tip != ThreadTip::None {
        p["tip"] = json!(o.tip);
        put(p, &[("chamfer_angle", o.chamfer_angle.as_str()), ("chamfer_depth", o.chamfer_depth.as_str())]);
    }
    if is_buttress(o.form) && o.steep_flank != SteepFlank::default() {
        p["steep_flank"] = json!(o.steep_flank);
    }
}

/// The picked Up To place: a plane's name, or a point on a face.
fn up_to(inputs: &[SelInput]) -> Option<Value> {
    match inputs.iter().find(|i| i.label == UP_TO)?.items.first()? {
        Sel::Plane { name } => Some(json!(name)),
        _ => object_point(inputs, UP_TO),
    }
}

/// The face's size in the dialog's form, for an automatic size.
pub(crate) fn suggested(s: &Session, picks: &[Sel], o: &Options) -> Option<String> {
    picks.first().and_then(|sel| match sel {
        Sel::Face { point, .. } => solidcraft_engine::doc::suggested_thread_in(&crate::open_meshing::session_world(s), *point, o.form).ok(),
        _ => None,
    })
}

/// A saved thread's dialog: every option as saved, with its picks.
pub(crate) fn reopen(d: &mut Dialog, s: &Session, kind: &FeatureKind) {
    let FeatureKind::Thread {
        face,
        designation,
        length,
        modeled,
        from,
        up_to,
        form,
        hand,
        starts,
        ease_out,
        tip,
        chamfer_angle,
        chamfer_depth,
        steep_flank,
    } = kind
    else {
        return;
    };
    if *modeled {
        d.extra.insert("modeled".into(), json!(true));
    }
    let options = Options {
        form: *form,
        hand: *hand,
        starts: *starts,
        extent: if length.is_some() {
            LENGTH
        } else if up_to.is_some() {
            UP_TO_EXTENT
        } else {
            FULL
        },
        ease_out: Ease::of(ease_out),
        tip: *tip,
        chamfer_angle: chamfer_angle.as_ref().map(expr).unwrap_or_default(),
        chamfer_depth: chamfer_depth.as_ref().map(expr).unwrap_or_default(),
        steep_flank: *steep_flank,
    };
    if let Some(inp) = d.inputs.first_mut() {
        inp.items = face_sel(s, face.0).into_iter().collect();
    }
    sync_inputs(&mut d.inputs, &mut d.active, &options);
    // A place no pick finds again (the model changed) is still the place saved.
    let place = |sel: Option<Sel>, p| sel.unwrap_or(Sel::Vertex { body: String::new(), point: p });
    let set = |d: &mut Dialog, label: &str, sel: Sel| {
        if let Some(inp) = d.inputs.iter_mut().find(|i| i.label == label) {
            inp.items = vec![sel];
        }
    };
    if let Some(from) = from {
        set(d, START_END, place(edge_sel(s, from.0), from.0));
    }
    match up_to {
        Some(ThreadLimit::Face(at)) => set(d, UP_TO, place(face_sel(s, at.0), at.0)),
        Some(ThreadLimit::Plane(plane)) => {
            if let Some(sel) = plane_sel(s, plane) {
                set(d, UP_TO, sel);
            }
        }
        None => {}
    }
    d.active = 0;
    d.kind = Kind::Thread {
        designation: designation.clone().unwrap_or_default(),
        length: length.as_ref().map(expr).unwrap_or_default(),
        suggested: None,
        of: Vec::new(),
        options,
    };
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
        let world = crate::open_meshing::session_world(&reopened);
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

    /// A tapped hole offers what a thread does: Modeled, and then its inner end, square or a
    /// fade, towards the bottom. OK cuts it; editing the hole brings them back.
    #[test]
    fn a_tapped_hole_is_modelled_with_the_threads_end() {
        let (app, dialog) = hole();
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.dialog = Some(dialog);
        h.frames(3);
        for handle in ["dialog:Hole Tap Type", "dialog:Hole Tap Type:Tapped"] {
            h.step(&json!({"click":{"handle":handle}})).unwrap();
        }
        h.frames(2);
        assert!(crate::scenario::rect_of("dialog:row:End").is_none(), "a listed thread has no end to finish");
        h.step(&json!({"click":{"handle":"dialog:Modeled"}})).unwrap();
        h.frames(2);
        for handle in ["dialog:End", "dialog:End:ui.fade"] {
            h.step(&json!({"click":{"handle":handle}})).unwrap();
        }
        h.frames(2);
        let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        assert_eq!((&p["modeled"], &p["ease_out"]), (&json!(true), &json!("standard")), "{p}");
        let id = h.app.run(&cmd, p.clone()).unwrap()["feature"].as_u64().unwrap();
        h.app.dialog = None;
        h.app.edit_feature(id);
        h.frames(2);
        let (_, again) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        let again = if again.get("params").is_some() { again["params"].clone() } else { again };
        assert_eq!((&again["modeled"], &again["ease_out"]), (&json!(true), &json!("standard")), "{again}");
    }

    /// A tapped hole's drill: the tap maker's for the size (M8: 6.8 mm), or a typed one from the
    /// thread's minor diameter to 5% over it; outside that the command names the range. A new
    /// size takes its own drill again, and a saved hole drilled otherwise (an older design's
    /// d - P) reopens with its drill.
    #[test]
    fn a_tapped_holes_drill_is_the_tap_makers_or_one_within_its_range() {
        let (app, dialog) = hole();
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.dialog = Some(dialog);
        h.frames(3);
        for handle in ["dialog:Hole Tap Type", "dialog:Hole Tap Type:Tapped"] {
            h.step(&json!({"click":{"handle":handle}})).unwrap();
        }
        let set = |h: &mut crate::scenario::Harness, field: &str, text: &str| {
            h.step(&json!({"click":{"handle":format!("dialog:{field}")}})).unwrap();
            h.step(&json!({"key":"A","cmd":true})).unwrap();
            h.step(&json!({"text":text})).unwrap();
            h.frames(2);
        };
        set(&mut h, "Designation", "M8");
        let params = |h: &crate::scenario::Harness| apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0).1;
        assert!(params(&h).get("diameter").is_none(), "the tap maker's drill is the command's own");
        set(&mut h, "Tap drill diameter", "7 mm");
        assert_eq!(params(&h)["diameter"], json!("7 mm"));
        let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        let id = h.app.run(&cmd, p.clone()).unwrap()["feature"].as_u64().unwrap();
        set(&mut h, "Tap drill diameter", "7.5 mm");
        let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        let e = h.app.session.execute(&cmd, &p).unwrap_err().to_string();
        assert!(e.contains("from 6.647 to 7.14 mm") && e.contains("too little of the thread"), "{e}");
        set(&mut h, "Designation", "M10");
        assert!(params(&h).get("diameter").is_none(), "a new size takes its own drill");

        // An older design's M8 hole, drilled 6.75 mm (d - P), keeps that drill when edited.
        let solidcraft_engine::doc::FeatureKind::Hole { diameter, .. } = &mut h.app.session.doc_mut().feature_mut(id).unwrap().kind else {
            panic!("not Hole")
        };
        *diameter = "6.75 mm".into();
        h.app.dialog = None;
        h.app.edit_feature(id);
        h.frames(2);
        let (_, again) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        let again = if again.get("params").is_some() { again["params"].clone() } else { again };
        assert_eq!(again["diameter"], json!("6.75 mm"), "{again}");
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
                    hole_rows(ui, &doc, &mut designation, &mut custom, &mut None);
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
            ("M8", 8.0, 1.25, 6.8),
            ("M8x1", 8.0, 1.0, 7.0),
            ("M8.5x1", 8.5, 1.0, 7.5),
        ] {
            let (mut app, _) = hole();
            let out = app.run("solid.hole", json!({"position":[10,10,10], "thread":name})).unwrap();
            let id = out["feature"].as_u64().unwrap();
            let saved = app.session.doc.to_json();
            let reopened = Session::new(Document::from_json(&saved).unwrap());
            let world = crate::open_meshing::session_world(&reopened);
            let t = &world.threads[0];
            assert_eq!(t.designation, name);
            assert!((t.major_diameter - major).abs() < 1e-9 && (t.pitch - pitch).abs() < 1e-9);
            let body = &world.bodies[0].body;
            assert!(body.validity().is_empty());
            let face = solidcraft_engine::kernel::cylinder_face_at(body, Vec3::new(10.0 + drill / 2.0, 10.0, 5.0)).unwrap();
            assert!(face.internal && (2.0 * face.radius - drill).abs() < 1e-8);
            let actual = body.mesh(&solidcraft_engine::kernel::MeshTolerance::chord(0.001)).unwrap().measure().volume;
            let expected = 4000.0 - std::f64::consts::PI * (drill / 2.0).powi(2) * 10.0;
            assert!((actual - expected).abs() < 0.5, "{name}: {actual} versus {expected}");
            // A saved explicit drill remains authoritative, including a legacy metric bore.
            let mut doc = Document::from_json(&saved).unwrap();
            let solidcraft_engine::doc::FeatureKind::Hole { diameter, .. } = &mut doc.feature_mut(id).unwrap().kind else { panic!("not Hole") };
            *diameter = format!("{} mm", drill + 0.05).as_str().into();
            let reopened = Session::new(Document::from_json(&doc.to_json()).unwrap());
            let world = crate::open_meshing::session_world(&reopened);
            let face = solidcraft_engine::kernel::cylinder_face_at(&world.bodies[0].body, Vec3::new(10.0 + (drill + 0.05) / 2.0, 10.0, 5.0)).unwrap();
            assert!((2.0 * face.radius - drill - 0.05).abs() < 1e-8);
        }
    }

    #[test]
    fn expanded_catalogue_rejects_external_taper_holes_and_nonmetric_thread_features() {
        let (mut app, _) = hole();
        let before = app.session.doc.to_json();
        for name in ["R1/4", "1/4-20 UNF"] {
            assert!(app.run("solid.hole", json!({"position":[10,10,10],"thread":name})).is_err());
            assert_eq!(app.session.doc.to_json(), before);
        }
        app.run("solid.hole", json!({"position":[10,10,10],"thread":"M8"})).unwrap();
        let before = app.session.doc.to_json();
        for designation in ["1/4-20 UNC", "G 1/4", "Rp 1/4"] {
            for modeled in [false, true] {
                assert!(app.run("solid.thread", json!({"face":[13.4,10,5],"designation":designation,"modeled":modeled})).is_err());
                assert_eq!(app.session.doc.to_json(), before);
            }
        }
        assert_eq!(app.session.doc.to_json(), before);
        let out = app.run("solid.thread", json!({"face":[13.4,10,5],"designation":"M8"})).unwrap();
        let solidcraft_engine::doc::FeatureKind::Thread { designation, modeled, .. } =
            &mut app.session.doc_mut().feature_mut(out["feature"].as_u64().unwrap()).unwrap().kind
        else {
            panic!("not Thread")
        };
        *designation = Some("1/4-20 UNC".into());
        *modeled = true;
        let reopened = Session::new(Document::from_json(&app.session.doc.to_json()).unwrap());
        // The thread is metric (the M8 it was made as): a unified designation is not one of its.
        let error = reopened.model.results.last().unwrap().error.clone().unwrap();
        assert!(error.contains("unknown metric thread `1/4-20 UNC`"), "{error}");
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
        // The tapped hole's rows; the thread dialog's own tests cover its designations.
        let context = egui::Context::default();
        crate::fonts::setup_bundled_fonts(&context);
        for original in ["M8x1.25", "M8x1", "M33x1", "M5×0.5", " m8x1.25 ", "invalid"] {
            let mut hole = original.to_string();
            let mut custom = None;
            let doc = Document::default();
            let _ = context.run_ui(Default::default(), |ui| {
                egui::Grid::new("authored_hole").show(ui, |ui| {
                    hole_rows(ui, &doc, &mut hole, &mut custom, &mut None);
                });
            });
            assert_eq!(hole, original);
        }
    }

    #[test]
    fn contribution_metric_designation_keeps_domain_pitch_drill_units_and_reopen() {
        for (diameter, pitch, drill) in [(8., 1.25, 6.8), (8., 1., 7.), (5., 0.5, 4.5)] {
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
            let state = crate::open_meshing::session_world(&reopened);
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
                let state = crate::open_meshing::session_world(&reopened);
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
                let actual = body.mesh(&solidcraft_engine::kernel::MeshTolerance::chord(0.001)).unwrap().measure().volume;
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
                    let state = crate::open_meshing::session_world(&reopened);
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
