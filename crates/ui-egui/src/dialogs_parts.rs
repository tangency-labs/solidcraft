//! Insert Part: the standard parts library (screws, bolts, nuts, washers, set screws, pins,
//! bearings) by family, size and length, with the size's table values and a picture of the part.
//! Picking a hole (its wall or rim) seats the part in it with a rigid joint; without one the part
//! goes at the origin. The live preview shows it placed.

use crate::ui_kit::{choice_row, text_row};
use egui::vec2;
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::{Kind, combo, form_row};
use crate::selection::{EDGES, FACES, SelInput};
use crate::theme::Tokens;

/// One family of the library.
#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    pub id: String,
    pub name: String,
    pub standard: String,
    /// Size names and their table rows.
    pub sizes: Vec<(String, Value)>,
    pub lengths: Option<Vec<f64>>,
}

/// The library as `parts.library` gives it.
pub fn families(v: &Value) -> Vec<Family> {
    v.get("families")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let id = f.get("family")?.as_str()?.to_string();
            let sizes = f
                .get("sizes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|s| Some((s.get("size")?.as_str().map(str::to_string).or_else(|| s.get("size").map(Value::to_string))?, s.clone())))
                .collect();
            Some(Family {
                name: f.get("name").and_then(Value::as_str).unwrap_or(&id).to_string(),
                standard: f.get("standard").and_then(Value::as_str).unwrap_or("").to_string(),
                sizes,
                lengths: f.get("lengths").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()),
                id,
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Pt {
    pub family: usize,
    pub size: usize,
    /// Index into the family's lengths (none: the default length).
    pub length: Option<usize>,
    pub name: String,
    /// The (family, size, length) the picture shows (the texture is kept in egui's memory).
    pub picture: Option<(usize, usize, Option<usize>)>,
    /// The library, read once.
    pub lib: Option<Vec<Family>>,
}

/// The dialog for Insert Part.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    matches!(id, "parts.fastener" | "parts.insert").then(|| {
        (
            Kind::Part(Pt { family: 0, size: 0, length: None, name: String::new(), picture: None, lib: None }),
            vec![SelInput::new("Hole (optional)", FACES | EDGES, false)],
        )
    })
}

fn library(app: &mut SolveApp) -> Vec<Family> {
    app.session.execute("parts.library", &json!({})).map(|v| families(&v)).unwrap_or_default()
}

/// The insert command's parameters for the picks.
pub fn insert_params(lib: &[Family], k: &Pt, hole: Option<&Sel>) -> Result<Value, String> {
    let f = lib.get(k.family).ok_or("the parts library is empty")?;
    let (size, _) = f.sizes.get(k.size).ok_or("pick a size")?;
    let mut p = json!({"family": f.id, "size": size});
    if let Some(l) = k.length.and_then(|i| f.lengths.as_ref().and_then(|ls| ls.get(i))) {
        p["length"] = json!(l);
    }
    match hole {
        Some(Sel::Face { point, .. } | Sel::Edge { point, .. }) => p["at"] = json!([point.x, point.y, point.z]),
        _ => {
            p["point"] = json!([0.0, 0.0, 0.0]);
            p["direction"] = json!([0.0, 0.0, 1.0]);
        }
    }
    if !k.name.trim().is_empty() {
        p["name"] = json!(k.name.trim());
    }
    Ok(p)
}

/// A picture of the part alone.
fn picture(p: &Value) -> Option<egui::ColorImage> {
    let mut s = Session::default();
    let mut q = p.clone();
    if let Value::Object(m) = &mut q {
        m.remove("at");
        m.insert("point".into(), json!([0.0, 0.0, 0.0]));
        m.insert("direction".into(), json!([0.0, 0.0, 1.0]));
    }
    s.execute("parts.insert", &q).ok()?;
    crate::home::thumbnail(&s)
}

/// The dialog's rows.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Pt, inputs: &[SelInput]) -> bool {
    let t = Tokens::get();
    let lib = match &k.lib {
        Some(l) => l.clone(),
        None => {
            let l = library(app);
            k.lib = Some(l.clone());
            l
        }
    };
    if lib.is_empty() {
        ui.vertical(|ui| {
            crate::ui_kit::text(ui, crate::i18n::tr_label("the parts library is not available"), crate::ui_kit::Text::Note("parts_unavailable"));
        });
        return false;
    }
    let names: Vec<String> =
        lib.iter().map(|f| if f.standard.is_empty() { family_name(f) } else { format!("{} ({})", family_name(f), f.standard) }).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let before = k.family;
    choice_row(ui, "Type", "pt_family", &mut k.family, &refs);
    if k.family != before {
        k.size = 0;
        k.length = None;
    }
    let Some(f) = lib.get(k.family) else { return false };
    let sizes: Vec<&str> = f.sizes.iter().map(|s| s.0.as_str()).collect();
    choice_row(ui, "Size", "pt_size", &mut k.size, &sizes);
    if let Some(ls) = &f.lengths {
        let mut labels = vec!["Default".to_string()];
        labels.extend(ls.iter().map(|l| crate::prefs::show_mm(app, *l, 1)));
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut i = k.length.map_or(0, |x| x + 1);
        form_row(ui, "Length", |ui| {
            combo(ui, "pt_len", &refs, &mut i);
            k.length = i.checked_sub(1);
        });
    }
    text_row(ui, "Name", &mut k.name, "automatic");
    // The part, and its table values.
    let key = (k.family, k.size, k.length);
    let tex_id = egui::Id::new("sc_part_picture");
    if k.picture != Some(key) {
        let tex = insert_params(&lib, k, None)
            .ok()
            .and_then(|p| picture(&p))
            .map(|img| ui.ctx().load_texture("sc_part_picture", img, egui::TextureOptions::LINEAR));
        ui.data_mut(|d| match tex {
            Some(t) => {
                d.insert_temp(tex_id, t);
            }
            None => d.remove::<egui::TextureHandle>(tex_id),
        });
        k.picture = Some(key);
    }
    ui.vertical(|ui| match ui.data(|d| d.get_temp::<egui::TextureHandle>(tex_id)) {
        Some(tx) => {
            ui.add(egui::Image::new(&tx).fit_to_exact_size(vec2(180.0, 112.0)).corner_radius(t.radius_sm));
        }
        None => {
            crate::ui_kit::text(ui, crate::i18n::tr_label("no picture"), crate::ui_kit::Text::Note("parts_picture"));
        }
    });
    if let Some((_, row)) = f.sizes.get(k.size)
        && let Some(o) = row.as_object()
    {
        for (key, v) in o.iter().filter(|(key, _)| key.as_str() != "size") {
            form_row(ui, &key.replace('_', " "), |ui| {
                crate::ui_kit::text(
                    ui,
                    &v.as_f64().map(|x| crate::prefs::show_mm(app, x, 1)).unwrap_or_else(|| v.to_string()),
                    crate::ui_kit::Text::Cell,
                );
            });
        }
    }
    if inputs.first().is_some_and(|i| i.items.is_empty()) {
        ui.vertical(|ui| {
            crate::ui_kit::text(
                ui,
                crate::i18n::tr_label("pick a hole to seat it, or OK places it at the origin"),
                crate::ui_kit::Text::Note("parts_placement"),
            );
        });
    }
    false
}

/// The commands OK runs.
pub fn commands(app: &SolveApp, k: &Pt, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let lib = match &k.lib {
        Some(l) => l.clone(),
        None => app.session.scratch().execute("parts.library", &json!({})).map(|v| families(&v)).unwrap_or_default(),
    };
    let p = insert_params(&lib, k, inputs.first().and_then(|i| i.items.first()))?;
    // The toolbar's command when the engine has it (same parameters).
    let id = if solidcraft_engine::find_command("parts.fastener").is_some() { "parts.fastener" } else { "parts.insert" };
    Ok(vec![(id.into(), p)])
}

/// A family's name in the user's language, by its id in the library: the library's own (English)
/// name for a family the catalogs don't know.
pub(crate) fn family_name(f: &Family) -> String {
    let id = format!("parts.family.{}", f.id);
    match crate::i18n::tr(&id) {
        shown if shown == id => f.name.clone(),
        shown => shown.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::geom::Vec3;

    fn lib() -> Vec<Family> {
        families(&json!({"families": [
            {"family": "socket_head_cap_screw", "name": "Socket Head Cap Screw", "standard": "ISO 4762",
             "sizes": [{"size": "M5", "head_diameter": 8.5}, {"size": "M6", "head_diameter": 10}], "lengths": [10, 16, 20]},
            {"family": "bearing", "name": "Deep Groove Ball Bearing", "standard": "", "sizes": [{"size": "608", "bore": 8}], "lengths": null},
        ]}))
    }

    #[test]
    fn library_rows_parse() {
        let l = lib();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].sizes[1].0, "M6");
        assert_eq!(l[0].lengths.as_deref(), Some(&[10.0, 16.0, 20.0][..]));
        assert!(l[1].lengths.is_none());
    }

    /// A hole pick seats the part (`at`); without one it goes to the origin along Z.
    #[test]
    fn insert_params_for_picks() {
        let l = lib();
        let k = Pt { family: 0, size: 1, length: Some(2), name: String::new(), picture: None, lib: None };
        let hole = Sel::Face { body: "Body1".into(), index: 3, point: Vec3::new(5.0, 0.0, 10.0) };
        let p = insert_params(&l, &k, Some(&hole)).unwrap();
        assert_eq!(p, json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20.0, "at": [5.0, 0.0, 10.0]}));
        let k = Pt { family: 1, size: 0, length: None, name: "Front bearing".into(), picture: None, lib: None };
        let p = insert_params(&l, &k, None).unwrap();
        assert_eq!(p["point"], json!([0.0, 0.0, 0.0]));
        assert_eq!(p["name"], "Front bearing");
        assert!(p.get("length").is_none());
        assert!(insert_params(&[], &k, None).is_err());
    }

    /// The library's families are named in the user's language, and a family the catalogs
    /// don't know keeps the library's name.
    #[test]
    fn family_names_follow_the_language() {
        let l = families(&solidcraft_engine::Session::default().execute("parts.library", &json!({})).unwrap());
        assert!(l.len() >= 7, "{}", l.len());
        let _german = crate::i18n::scope("de");
        for f in &l {
            assert_ne!(family_name(f), f.name, "{}", f.id);
        }
        let unknown = Family { id: "spring".into(), name: "Spring".into(), standard: String::new(), sizes: vec![], lengths: None };
        assert_eq!(family_name(&unknown), "Spring");
    }
}
