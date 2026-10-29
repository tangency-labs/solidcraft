//! Lighten / Auto-Pocket parameter dialog: Wall Offset / Rib Clearance, Pocket Depth /
//! Floor Thickness, Draft Angle, Corner Fillet Radius, and Manufacturing Method.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::selection::{FACES, PLANAR_FACES, SelInput, point_value as pt};
use crate::ui_kit::{Button, button, choice_row, expr_row};

pub const LOCATIONS: [&str; 3] = ["Top & Bottom", "Top", "Bottom"];

#[derive(Clone, Debug, PartialEq)]
pub struct LightenDialog {
    pub wall_offset: String,
    pub pocket_depth: String,
    pub draft_angle: String,
    pub corner_fillet: String,
    // Auto Fillet:
    pub auto_fillet: bool,
    pub fillet_radius: String,
    pub fillet_location: usize,
    // Auto Chamfer:
    pub auto_chamfer: bool,
    pub chamfer_size: String,
    pub chamfer_location: usize,
}

impl Default for LightenDialog {
    fn default() -> Self {
        Self {
            wall_offset: "2.0 mm".into(),
            pocket_depth: "3.0 mm".into(),
            draft_angle: "0 deg".into(),
            corner_fillet: "1.0 mm".into(),
            auto_fillet: false,
            fillet_radius: "1.0 mm".into(),
            fillet_location: 2, // default: Bottom
            auto_chamfer: false,
            chamfer_size: "0.5 mm".into(),
            chamfer_location: 1, // default: Top
        }
    }
}

impl LightenDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(&self) -> &'static str {
        "LIGHTEN"
    }

    pub fn previews(&self) -> bool {
        true
    }

    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        Some(("Pocket Depth", ValueKind::Length, &mut self.pocket_depth))
    }
}

/// Start the lighten dialog for a lighten / pocket command.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    if !matches!(id, "solid.lighten" | "solid.pocket" | "solid.auto_pocket" | "lighten" | "auto_pocket" | "pocket") {
        return None;
    }
    Some((
        Kind::Lighten(LightenDialog::default()),
        vec![SelInput::new("Faces", FACES | PLANAR_FACES, true)],
    ))
}

/// The dialog editing a lighten feature, filled from it.
pub fn for_feature(app: &SolveApp, kind: &solidcraft_engine::doc::FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    if let solidcraft_engine::doc::FeatureKind::Lighten {
        face,
        body,
        wall,
        depth,
        draft,
        corner_radius,
        fillet,
        fillet_at,
        chamfer,
        chamfer_at,
    } = kind
    {
        let mut d = LightenDialog::default();
        d.wall_offset = wall.to_string();
        d.pocket_depth = depth.to_string();
        if let Some(dr) = draft {
            d.draft_angle = dr.to_string();
        }
        d.corner_fillet = corner_radius.to_string();
        if let Some(f) = fillet {
            d.auto_fillet = true;
            d.fillet_radius = f.to_string();
            d.fillet_location = match fillet_at.as_str() {
                "both" => 0,
                "top" => 1,
                _ => 2,
            };
        }
        if let Some(c) = chamfer {
            d.auto_chamfer = true;
            d.chamfer_size = c.to_string();
            d.chamfer_location = match chamfer_at.as_str() {
                "both" => 0,
                "top" => 1,
                _ => 2,
            };
        }
        let mut extra = Map::new();
        if let Some(b) = body {
            extra.insert("body".into(), json!(b));
        }
        let sel = match crate::dialogs::face_sel(&app.session, face.0) {
            Some(s) => {
                let mut inp = SelInput::new("Faces", FACES | PLANAR_FACES, true);
                inp.items.push(s);
                vec![inp]
            }
            None => vec![SelInput::new("Faces", FACES | PLANAR_FACES, true)],
        };
        Some((Kind::Lighten(d), sel, extra))
    } else {
        None
    }
}

/// Render the parameter inputs using `ui_kit` components.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut LightenDialog, inputs: &[SelInput]) -> bool {
    let mut enter = false;
    enter |= expr_row(ui, "Wall Offset", &mut d.wall_offset);
    enter |= expr_row(ui, "Pocket Depth", &mut d.pocket_depth);
    enter |= expr_row(ui, "Draft Angle", &mut d.draft_angle);
    enter |= expr_row(ui, "Corner Fillet", &mut d.corner_fillet);

    ui.add_space(4.0);
    ui.checkbox(&mut d.auto_fillet, "Auto Fillet");
    if d.auto_fillet {
        ui.indent("fillet_options", |ui| {
            enter |= expr_row(ui, "Fillet Radius", &mut d.fillet_radius);
            choice_row(ui, "Location", "fillet_location", &mut d.fillet_location, &LOCATIONS);
        });
    }

    ui.checkbox(&mut d.auto_chamfer, "Auto Chamfer");
    if d.auto_chamfer {
        ui.indent("chamfer_options", |ui| {
            enter |= expr_row(ui, "Chamfer Size", &mut d.chamfer_size);
            choice_row(ui, "Location", "chamfer_location", &mut d.chamfer_location, &LOCATIONS);
        });
    }

    // Mutual exclusion: prevent fillet and chamfer from fighting each other on the same edges
    if d.auto_fillet && d.auto_chamfer {
        if d.fillet_location == 0 && d.chamfer_location == 0 {
            // Both selected "Top & Bottom": split into Fillet Bottom, Chamfer Top
            d.fillet_location = 2;
            d.chamfer_location = 1;
        } else if d.fillet_location == 0 {
            // Fillet wants both: yield to chamfer's choice
            if d.chamfer_location == 1 {
                d.fillet_location = 2; // Chamfer Top -> Fillet Bottom
            } else {
                d.fillet_location = 1; // Chamfer Bottom -> Fillet Top
            }
        } else if d.chamfer_location == 0 {
            // Chamfer wants both: yield to fillet's choice
            if d.fillet_location == 1 {
                d.chamfer_location = 2; // Fillet Top -> Chamfer Bottom
            } else {
                d.chamfer_location = 1; // Fillet Bottom -> Chamfer Top
            }
        } else if d.fillet_location == d.chamfer_location {
            // Both selected the same single location (e.g. both Top)
            if d.fillet_location == 1 {
                d.chamfer_location = 2;
            } else {
                d.chamfer_location = 1;
            }
        }
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if button(ui, "Apply", Button::Primary).clicked() || button(ui, "Generate", Button::Normal).clicked() {
            let _ = dispatch(app, d, inputs, &Map::new());
        }
    });

    enter
}

/// Dispatch the pocket cut command to the active session/engine.
pub fn dispatch(
    app: &mut SolveApp,
    d: &LightenDialog,
    inputs: &[SelInput],
    extra: &Map<String, Value>,
) -> Result<Value, String> {
    let cmds = commands(app, d, inputs, extra)?;
    let mut last = Ok(Value::Null);
    for (cmd, params) in cmds {
        last = app.run(&cmd, params).map_err(|e| e.to_string());
    }
    last
}

/// The commands the dialog's OK / Apply runs.
pub fn commands(
    _app: &SolveApp,
    d: &LightenDialog,
    inputs: &[SelInput],
    extra: &Map<String, Value>,
) -> Result<Vec<(String, Value)>, String> {
    let faces: Vec<Value> = inputs
        .first()
        .map(|i| {
            i.items
                .iter()
                .filter_map(|s| match s {
                    Sel::Face { point, .. } => Some(pt(*point)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    let (fillet_top, fillet_bottom) = if d.auto_fillet {
        match d.fillet_location {
            0 => (true, true),
            1 => (true, false),
            2 => (false, true),
            _ => (false, true),
        }
    } else {
        (false, false)
    };

    let (chamfer_top, chamfer_bottom) = if d.auto_chamfer {
        match d.chamfer_location {
            0 => (true, true),
            1 => (true, false),
            2 => (false, true),
            _ => (true, false),
        }
    } else {
        (false, false)
    };

    let mut params = json!({
        "wall_offset": d.wall_offset,
        "rib_clearance": d.wall_offset,
        "pocket_depth": d.pocket_depth,
        "floor_thickness": d.pocket_depth,
        "draft_angle": d.draft_angle,
        "corner_fillet": d.corner_fillet,
        "corner_radius": d.corner_fillet,
        "auto_fillet": d.auto_fillet,
        "auto_chamfer": d.auto_chamfer,
        "faces": faces,
    });

    if d.auto_fillet {
        params["fillet_radius"] = json!(d.fillet_radius);
        params["fillet_top"] = json!(fillet_top);
        params["fillet_bottom"] = json!(fillet_bottom);
    }
    if d.auto_chamfer {
        params["chamfer_size"] = json!(d.chamfer_size);
        params["chamfer_top"] = json!(chamfer_top);
        params["chamfer_bottom"] = json!(chamfer_bottom);
    }

    if let Value::Object(m) = &mut params {
        for (k, v) in extra {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }

    Ok(vec![("solid.lighten".to_string(), params)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use solidcraft_engine::Session;

    fn test_app() -> SolveApp {
        SolveApp::new(Session::default(), Services::default())
    }

    #[test]
    fn lighten_dialog_defaults() {
        let d = LightenDialog::default();
        assert_eq!(d.wall_offset, "2.0 mm");
        assert_eq!(d.pocket_depth, "3.0 mm");
        assert_eq!(d.draft_angle, "0 deg");
        assert_eq!(d.corner_fillet, "1.0 mm");
        assert!(!d.auto_fillet);
        assert!(!d.auto_chamfer);
        assert_eq!(d.title(), "LIGHTEN");
        assert!(d.previews());
    }

    #[test]
    fn lighten_start_recognizes_commands() {
        let app = test_app();
        for id in ["solid.lighten", "solid.pocket", "solid.auto_pocket", "lighten"] {
            let (kind, inputs) = start(&app, id).unwrap();
            assert!(matches!(kind, Kind::Lighten(_)));
            assert_eq!(inputs.len(), 1);
            assert_eq!(inputs[0].label, "Faces");
        }
        assert!(start(&app, "unknown.command").is_none());
    }

    #[test]
    fn lighten_clicking_or_start_does_not_execute_immediate_cut() {
        let mut app = test_app();
        let features_before = app.session.doc.features.len();
        app.start("solid.lighten");
        assert!(app.dialog.is_some());
        assert!(matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Lighten(_))));
        assert_eq!(app.session.doc.features.len(), features_before);
    }

    #[test]
    fn lighten_commands_generation() {
        let app = test_app();
        let mut d = LightenDialog::default();
        d.auto_fillet = true;
        d.fillet_location = 2; // Bottom
        d.auto_chamfer = true;
        d.chamfer_location = 1; // Top
        let c = commands(&app, &d, &[], &Map::new()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].0, "solid.lighten");
        assert_eq!(c[0].1["wall_offset"], "2.0 mm");
        assert_eq!(c[0].1["pocket_depth"], "3.0 mm");
        assert_eq!(c[0].1["auto_fillet"], true);
        assert_eq!(c[0].1["fillet_bottom"], true);
        assert_eq!(c[0].1["auto_chamfer"], true);
        assert_eq!(c[0].1["chamfer_top"], true);
    }

    #[test]
    fn lighten_commands_auto_chamfer_only() {
        let app = test_app();
        let mut d = LightenDialog::default();
        d.auto_fillet = false;
        d.auto_chamfer = true;
        d.chamfer_location = 1; // Top
        let c = commands(&app, &d, &[], &Map::new()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].1["auto_fillet"], false);
        assert_eq!(c[0].1.get("fillet_radius"), None);
        assert_eq!(c[0].1.get("fillet_bottom"), None);
        assert_eq!(c[0].1["auto_chamfer"], true);
        assert_eq!(c[0].1["chamfer_top"], true);
    }

    #[test]
    fn lighten_for_feature_populates_dialog() {
        let app = test_app();
        let feat = solidcraft_engine::doc::FeatureKind::Lighten {
            face: solidcraft_engine::doc::FacePick(Vec3::new(10.0, 20.0, 30.0)),
            body: Some(solidcraft_engine::doc::BodyName::from("Plate")),
            wall: "3.5 mm".into(),
            depth: "4.0 mm".into(),
            draft: Some("2 deg".into()),
            corner_radius: "1.5 mm".into(),
            fillet: Some("0.8 mm".into()),
            fillet_at: "bottom".into(),
            chamfer: Some("0.5 mm".into()),
            chamfer_at: "top".into(),
        };
        let (kind, inputs, extra) = for_feature(&app, &feat).expect("for_feature must succeed for Lighten");
        if let Kind::Lighten(d) = kind {
            assert_eq!(d.wall_offset, "3.5 mm");
            assert_eq!(d.pocket_depth, "4.0 mm");
            assert_eq!(d.draft_angle, "2 deg");
            assert_eq!(d.corner_fillet, "1.5 mm");
            assert!(d.auto_fillet);
            assert_eq!(d.fillet_radius, "0.8 mm");
            assert_eq!(d.fillet_location, 2); // bottom
            assert!(d.auto_chamfer);
            assert_eq!(d.chamfer_size, "0.5 mm");
            assert_eq!(d.chamfer_location, 1); // top
        } else {
            panic!("expected Kind::Lighten");
        }
        assert_eq!(inputs.len(), 1);
        assert_eq!(extra.get("body"), Some(&json!("Plate")));
    }
}
