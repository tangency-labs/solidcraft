//! The Midsurface dialog (#11439): the solid, the thickest wall to pair across (prefilled with the
//! analysis's suggestion), the pairs the analysis proposes, each with a checkbox (those it is sure
//! of ticked, those that need a decision not, each with its notes), the options, and what the
//! result leaves out. It builds `surface.midsurface`'s parameters with the ticked pairs; the
//! proposal comes from the read-only `surface.midsurface_analyze`, asked when a solid is picked
//! and when Find Pairs is pressed (not on every keystroke: it reads every face of the solid).

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;

use crate::SolveApp;
use crate::selection::{BODIES, FACES, SelInput, items};
use crate::ui_kit;
use crate::ui_kit::expr_row;

/// A pair the analysis proposes.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposed {
    /// The two face points `surface.midsurface` takes for it.
    pub points: Value,
    /// What the row says: the faces, the thickness and the area.
    pub label: String,
    /// Why it needs a decision (empty for a confident pair).
    pub notes: Vec<String>,
    /// Whether to build it.
    pub build: bool,
}

/// The dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub struct MidOpts {
    /// The thickest wall to pair across; empty asks the analysis for its suggestion.
    pub max_thickness: String,
    /// The thinnest wall to pair across, kept from an edited feature (no box); empty means 0. Sent
    /// to the analysis too, so Find Pairs proposes what the command will accept.
    pub min_thickness: String,
    /// How many times its thickness a wall's span must be before it needs no decision, kept from
    /// an edited feature (no box); `None` is the engine's default. Sent to the analysis too, so
    /// Find Pairs ticks what the feature was made with.
    pub min_aspect: Option<f64>,
    pub carry_holes: bool,
    /// Holes no wider than this are filled; empty keeps all.
    pub suppress_holes_below: String,
    pub keep_original: bool,
    pub stitch: bool,
    /// The proposal for `analysed`.
    pub pairs: Vec<Proposed>,
    /// The body the pairs were found for.
    pub analysed: Option<String>,
    /// The `max_thickness` the pairs were found with: a different one asks for Find Pairs again
    /// (the ticked pairs may not be pairs under the new limit).
    pub proposed_with: String,
    /// What building the ticked pairs the analysis is sure of leaves out, in its words, or why
    /// the analysis failed.
    pub summary: Vec<String>,
}

impl Default for MidOpts {
    fn default() -> MidOpts {
        MidOpts {
            max_thickness: String::new(),
            min_thickness: String::new(),
            min_aspect: None,
            carry_holes: true,
            suppress_holes_below: String::new(),
            keep_original: true,
            stitch: true,
            pairs: Vec::new(),
            analysed: None,
            proposed_with: String::new(),
            summary: Vec::new(),
        }
    }
}

/// The dialog's one pick: the solid (or a face of it), under the catalog's existing "Body" label.
pub fn inputs() -> Vec<SelInput> {
    vec![SelInput::new("Body", BODIES | FACES, false)]
}

fn body_of(inputs: &[SelInput]) -> Option<String> {
    items(inputs, 0).first().and_then(|s| match s {
        Sel::Body { name } => Some(name.clone()),
        Sel::Face { body, .. } => Some(body.clone()),
        _ => None,
    })
}

fn number(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Reads `surface.midsurface_analyze`'s reply into the dialog: the pairs (confident ones ticked),
/// the suggested limit when none was typed, and the faces left uncovered.
fn take(o: &mut MidOpts, reply: &Value) {
    // Rounded to a nanometre, as the engine stores the suggestion: a coarser rounding could turn
    // a tiny part's limit into 0 mm, which the command refuses.
    if o.max_thickness.trim().is_empty() {
        o.max_thickness = format!("{} mm", (number(reply, "max_thickness") * 1e6).round() / 1e6);
    }
    o.proposed_with = o.max_thickness.trim().to_string();
    o.pairs = reply
        .get("pairs")
        .and_then(Value::as_array)
        .map(|pairs| {
            pairs
                .iter()
                .map(|p| {
                    let faces = p.get("faces").cloned().unwrap_or(Value::Null);
                    let round = if p.get("round").and_then(Value::as_bool).unwrap_or(false) {
                        solidcraft_ui_text::tr("ui.midsurface.round").to_string()
                    } else {
                        String::new()
                    };
                    let label = solidcraft_ui_text::format(
                        "ui.midsurface.pair",
                        &[
                            ("faces", &faces.to_string()),
                            ("thickness", &format!("{:.2}", number(p, "thickness"))),
                            ("area", &format!("{:.0}", number(p, "area"))),
                            ("round", &round),
                        ],
                    );
                    let notes: Vec<String> = p
                        .get("notes")
                        .and_then(Value::as_array)
                        .map(|n| n.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default();
                    let build = p.get("confidence").and_then(Value::as_str) == Some("high");
                    Proposed { points: p.get("points").cloned().unwrap_or(Value::Null), label, notes, build }
                })
                .collect()
        })
        .unwrap_or_default();
    let c = reply.get("completeness");
    let uncovered = c.and_then(|c| c.get("uncovered")).and_then(Value::as_array).map_or(0, Vec::len);
    let faces = c.and_then(|c| c.get("faces")).and_then(Value::as_u64).unwrap_or(0);
    o.summary = vec![solidcraft_ui_text::format("ui.midsurface.uncovered", &[("uncovered", &uncovered.to_string()), ("faces", &faces.to_string())])];
    for u in c.and_then(|c| c.get("uncovered")).and_then(Value::as_array).into_iter().flatten().take(8) {
        let face = u.get("face").and_then(Value::as_u64).unwrap_or(0);
        let why = u.get("why").and_then(Value::as_str).unwrap_or("");
        o.summary.push(format!("{face}: {why}"));
    }
}

/// Asks the engine for the proposal of `body` (read-only: the design does not change).
fn analyse(app: &mut SolveApp, o: &mut MidOpts, body: Option<String>) {
    o.analysed = body.clone();
    o.pairs.clear();
    o.summary.clear();
    let Some(body) = body else { return };
    let mut p = json!({"body": body});
    if !o.max_thickness.trim().is_empty() {
        p["max_thickness"] = json!(o.max_thickness);
    }
    if !o.min_thickness.trim().is_empty() {
        p["min_thickness"] = json!(o.min_thickness);
    }
    if let Some(a) = o.min_aspect {
        p["min_aspect"] = json!(a);
    }
    match app.session.execute("surface.midsurface_analyze", &p) {
        Ok(reply) => take(o, &reply),
        Err(e) => o.summary.push(e.to_string()),
    }
}

/// The rows: the limit, Find Pairs, the pairs with their checkboxes and notes, the options and
/// the summary. True when Enter was pressed in a field.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, o: &mut MidOpts, inputs: &[SelInput]) -> bool {
    let body = body_of(inputs);
    // A cleared Body clears the proposal too, so picking the solid again asks afresh.
    if body != o.analysed {
        analyse(app, o, body.clone());
    }
    let mut enter = expr_row(ui, solidcraft_ui_text::tr("ui.midsurface.max_thickness"), &mut o.max_thickness);
    if ui_kit::button(ui, solidcraft_ui_text::tr("ui.midsurface.find_pairs"), ui_kit::Button::Small).clicked() {
        analyse(app, o, body);
    }
    if !o.pairs.is_empty() {
        ui_kit::text(ui, solidcraft_ui_text::tr("ui.midsurface.pairs_hint"), ui_kit::Text::Hint("midsurface_pairs"));
        for pair in &mut o.pairs {
            ui_kit::checkbox(ui, &pair.label, &mut pair.build);
            for note in &pair.notes {
                ui_kit::text(ui, note, ui_kit::Text::Note("midsurface_note"));
            }
        }
    }
    enter |= expr_row(ui, solidcraft_ui_text::tr("ui.midsurface.suppress_holes_below"), &mut o.suppress_holes_below);
    let (carry, stitch, keep) = (
        solidcraft_ui_text::tr("ui.midsurface.carry_holes"),
        solidcraft_ui_text::tr("ui.midsurface.stitch"),
        solidcraft_ui_text::tr("ui.midsurface.keep_original"),
    );
    ui_kit::options(ui, "midsurface_options", &mut [(carry, &mut o.carry_holes), (stitch, &mut o.stitch), (keep, &mut o.keep_original)]);
    for line in &o.summary {
        ui_kit::text(ui, line, ui_kit::Text::Note("midsurface_summary"));
    }
    enter
}

/// `surface.midsurface`'s parameters: the solid, the ticked pairs, the limit and the options.
pub fn command(o: &MidOpts, inputs: &[SelInput]) -> Result<(&'static str, Value), String> {
    let body = body_of(inputs).ok_or_else(|| solidcraft_ui_text::tr("ui.midsurface.pick_solid"))?;
    let mut p = json!({"body": body, "carry_holes": o.carry_holes, "keep_original": o.keep_original, "stitch": o.stitch});
    // Always the ticked pairs: with none (nothing proposed, or none ticked) the engine would build
    // the pairs it is sure of, which the user has not seen.
    let ticked: Vec<Value> = o.pairs.iter().filter(|x| x.build).map(|x| x.points.clone()).collect();
    if ticked.is_empty() {
        return Err(solidcraft_ui_text::tr("ui.midsurface.tick_a_pair").to_string());
    }
    if o.max_thickness.trim() != o.proposed_with {
        return Err(solidcraft_ui_text::tr("ui.midsurface.find_pairs_again").to_string());
    }
    p["pairs"] = json!(ticked);
    if !o.max_thickness.trim().is_empty() {
        p["max_thickness"] = json!(o.max_thickness);
    }
    if !o.min_thickness.trim().is_empty() {
        p["min_thickness"] = json!(o.min_thickness);
    }
    if let Some(a) = o.min_aspect {
        p["min_aspect"] = json!(a);
    }
    if !o.suppress_holes_below.trim().is_empty() {
        p["suppress_holes_below"] = json!(o.suppress_holes_below);
    }
    Ok(("surface.midsurface", p))
}

/// The dialog of a Midsurface feature being edited: its stored pairs ticked (Find Pairs proposes
/// afresh), its limit and options, and its solid picked.
pub fn for_feature(kind: &FeatureKind) -> Option<(MidOpts, Vec<SelInput>)> {
    let FeatureKind::SurfaceMidsurface {
        body,
        faces,
        max_thickness,
        min_thickness,
        min_aspect,
        carry_holes,
        suppress_holes_below,
        stitch,
        remove_original,
    } = kind
    else {
        return None;
    };
    let pairs = faces
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .map(|(i, [a, b])| Proposed {
            points: json!([[a.0.x, a.0.y, a.0.z], [b.0.x, b.0.y, b.0.z]]),
            label: solidcraft_ui_text::format("ui.midsurface.stored_pair", &[("number", &(i + 1).to_string())]),
            notes: Vec::new(),
            build: true,
        })
        .collect();
    let o = MidOpts {
        max_thickness: max_thickness.to_string(),
        proposed_with: max_thickness.to_string().trim().to_string(),
        min_thickness: min_thickness.as_ref().map(|e| e.to_string()).unwrap_or_default(),
        min_aspect: *min_aspect,
        carry_holes: *carry_holes,
        suppress_holes_below: suppress_holes_below.as_ref().map(|e| e.to_string()).unwrap_or_default(),
        keep_original: !*remove_original,
        stitch: *stitch,
        pairs,
        analysed: body.as_ref().map(|b| b.to_string()),
        summary: Vec::new(),
    };
    let mut ins = inputs();
    if let (Some(input), Some(name)) = (ins.first_mut(), body) {
        input.items = vec![Sel::Body { name: name.to_string() }];
    }
    Some((o, ins))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_analysis_reply_ticks_the_confident_pairs_and_prefills_the_limit() {
        let reply = json!({
            "body": "Plate", "max_thickness": 6.0, "min_thickness": 0.0,
            "pairs": [
                {"faces": [0, 1], "points": [[1, 1, 2], [1, 1, 0]], "thickness": 2.0, "area": 1200.0, "coverage": 1.0, "aspect": 17.3,
                 "confidence": "high", "round": false, "filled_over": 0.0, "notes": []},
                {"faces": [2, 3], "points": [[0, 1, 1], [40, 1, 1]], "thickness": 40.0, "area": 60.0, "coverage": 1.0, "aspect": 0.2,
                 "confidence": "review", "round": false, "filled_over": 0.0, "notes": ["chunky"]}
            ],
            "leftovers": [], "clusters": [],
            "completeness": {"faces": 6, "paired": 2, "rims": 4, "uncovered": []}
        });
        let mut o = MidOpts::default();
        take(&mut o, &reply);
        assert_eq!(o.max_thickness, "6 mm");
        assert_eq!(o.pairs.len(), 2);
        assert!(o.pairs[0].build && !o.pairs[1].build);
        assert_eq!(o.pairs[1].notes, vec!["chunky".to_string()]);
        let mut ins = inputs();
        ins[0].items = vec![Sel::Body { name: "Plate".into() }];
        let (cmd, p) = command(&o, &ins).unwrap();
        assert_eq!(cmd, "surface.midsurface");
        assert_eq!(p["pairs"], json!([[[1, 1, 2], [1, 1, 0]]]));
        assert_eq!(p["max_thickness"], json!("6 mm"));
        // The limit changed after Find Pairs: the ticked pairs were found with 6 mm, so they are
        // not sent with 1 mm (they may be no pair under it); the same limit typed back is sent.
        o.max_thickness = "1 mm".into();
        assert!(command(&o, &ins).is_err());
        o.max_thickness = " 6 mm".into();
        assert!(command(&o, &ins).is_ok());
        // A tiny part's suggestion keeps its value: 0.00045 mm rounded to a micrometre would be
        // 0 mm, a limit the command refuses.
        let mut tiny = MidOpts::default();
        take(&mut tiny, &json!({"max_thickness": 0.00045, "pairs": []}));
        assert_eq!(tiny.max_thickness, "0.00045 mm");
        // Nothing ticked: refused, not built from the defaults.
        o.pairs[0].build = false;
        assert!(command(&o, &ins).is_err());
        assert!(command(&MidOpts::default(), &inputs()).is_err());
        // A solid picked but nothing proposed (say an empty proposal, then a larger limit typed
        // without Find Pairs): refused too, never sent without `pairs` (the engine would then
        // build its confident pairs unseen).
        let unproposed = MidOpts { max_thickness: "10 mm".into(), ..MidOpts::default() };
        assert!(command(&unproposed, &ins).is_err());
    }

    #[test]
    fn a_stored_min_thickness_is_sent_with_the_command() {
        // An edited feature's thinnest wall has no box but must reach the command, so the pairs
        // ticked from Find Pairs (asked with the same bound) are ones the command accepts.
        let o = MidOpts {
            min_thickness: "1.5 mm".into(),
            pairs: vec![Proposed { points: json!([[1, 1, 2], [1, 1, 0]]), label: String::new(), notes: Vec::new(), build: true }],
            ..MidOpts::default()
        };
        let mut ins = inputs();
        ins[0].items = vec![Sel::Body { name: "Plate".into() }];
        let (_, p) = command(&o, &ins).unwrap();
        assert_eq!(p["min_thickness"], json!("1.5 mm"));
        // None stored: the key is left out (the engine's default of 0).
        let (_, p) = command(&MidOpts { min_thickness: String::new(), ..o.clone() }, &ins).unwrap();
        assert!(p.get("min_thickness").is_none());
        // A stored min_aspect (no box either) is sent back unchanged; none stored leaves it out.
        assert!(p.get("min_aspect").is_none());
        let (_, p) = command(&MidOpts { min_aspect: Some(8.0), ..o }, &ins).unwrap();
        assert_eq!(p["min_aspect"], json!(8.0));
    }
}
