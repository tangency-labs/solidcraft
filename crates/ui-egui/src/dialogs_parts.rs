//! Insert Part: the standard parts library (screws, bolts, nuts, washers, set screws, pins,
//! bearings) by family, size and length, with the size's table values and a picture of the part.
//! Picking a hole (its wall or rim) seats the part in it with a rigid joint, and picking several
//! seats the same part in each; without one the part goes at the origin. The live preview shows it
//! placed.

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

/// A family's name in the user's language, by its id in the library: the library's own (English)
/// name for a family the catalogs don't know.
pub(crate) fn family_name(f: &Family) -> String {
    let id = format!("parts.family.{}", f.id);
    match crate::i18n::tr(&id) {
        shown if shown == id => f.name.clone(),
        shown => shown.to_string(),
    }
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
    // Standard Parts itself only lists the library: started from the app it opens the dialog
    // that shows it (#8735), which did nothing before.
    matches!(id, "parts.fastener" | "parts.insert" | "parts.library").then(|| {
        (
            Kind::Part(Pt { family: 0, size: 0, length: None, name: String::new(), picture: None, lib: None }),
            vec![SelInput::new("parts.holes", FACES | EDGES, true)],
        )
    })
}

fn library(app: &mut SolveApp) -> Vec<Family> {
    app.session.execute("parts.library", &json!({})).map(|v| families(&v)).unwrap_or_default()
}

/// The insert command's parameters for the hole picks: one part seated in each.
pub fn insert_params(lib: &[Family], k: &Pt, holes: &[Sel]) -> Result<Value, String> {
    let f = lib.get(k.family).ok_or("the parts library is empty")?;
    let (size, _) = f.sizes.get(k.size).ok_or("pick a size")?;
    let mut p = json!({"family": f.id, "size": size});
    if let Some(l) = k.length.and_then(|i| f.lengths.as_ref().and_then(|ls| ls.get(i))) {
        p["length"] = json!(l);
    }
    let at: Vec<Value> = holes
        .iter()
        .filter_map(|h| match h {
            Sel::Face { point, .. } | Sel::Edge { point, .. } => Some(json!([point.x, point.y, point.z])),
            _ => None,
        })
        .collect();
    match at.len() {
        0 => {
            p["point"] = json!([0.0, 0.0, 0.0]);
            p["direction"] = json!([0.0, 0.0, 1.0]);
        }
        // One hole keeps the single pick's form.
        1 => p["at"] = at.into_iter().next().unwrap_or(Value::Null),
        _ => p["at"] = Value::Array(at),
    }
    if !k.name.trim().is_empty() {
        p["name"] = json!(k.name.trim());
    }
    Ok(p)
}

/// Where the Length list opens for a size: the index of the length an insert of it gets when
/// none is asked for, which `parts.library` reports beside the size's table values. The list
/// carries no item of its own for it, so a size whose default is missing opens on the first.
fn default_length(f: &Family, size: usize, lengths: &[f64]) -> usize {
    f.sizes
        .get(size)
        .and_then(|(_, row)| row.get("default_length"))
        .and_then(Value::as_f64)
        .and_then(|d| lengths.iter().position(|l| (*l - d).abs() < 1e-9))
        .unwrap_or(0)
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
    let was_size = k.size;
    choice_row(ui, "Size", "pt_size", &mut k.size, &sizes);
    // Each size has a default length of its own, so a new size resolves the Length list again.
    if k.size != was_size {
        k.length = None;
    }
    if let Some(ls) = &f.lengths {
        let labels: Vec<String> = ls.iter().map(|l| crate::prefs::show_mm(app, *l, 1)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut i = k.length.unwrap_or_else(|| default_length(f, k.size, ls));
        form_row(ui, "Length", |ui| {
            combo(ui, "pt_len", &refs, &mut i);
            k.length = Some(i);
        });
    }
    text_row(ui, "Name", &mut k.name, crate::i18n::tr("parts.automatic_name"));
    // The part, and its table values.
    let key = (k.family, k.size, k.length);
    let tex_id = egui::Id::new("sc_part_picture");
    if k.picture != Some(key) {
        let tex = insert_params(&lib, k, &[])
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
        // `default_length` picks the Length list's opening value; it is not a dimension.
        for (key, v) in o.iter().filter(|(key, _)| !matches!(key.as_str(), "size" | "default_length")) {
            let label = dimension_label(key).map_or_else(|| key.replace('_', " "), str::to_string);
            form_row(ui, &label, |ui| {
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
            crate::ui_kit::text(ui, crate::i18n::tr("parts.placement_hint"), crate::ui_kit::Text::Note("parts_placement"));
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
    let p = insert_params(&lib, k, inputs.first().map_or(&[][..], |i| &i.items))?;
    // The toolbar's command when the engine has it (same parameters).
    let id = if solidcraft_engine::find_command("parts.fastener").is_some() { "parts.fastener" } else { "parts.insert" };
    Ok(vec![(id.into(), p)])
}

/// The message a size's dimension is labelled with, by its key in the library: "Head height",
/// not `head_height`. A key the library adds later shows as it is until it has one here.
fn dimension_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "across_flats" => "parts.dimension.across_flats",
        "bore" => "parts.dimension.bore",
        "d" => "parts.dimension.d",
        "head_diameter" => "parts.dimension.head_diameter",
        "head_height" => "parts.dimension.head_height",
        "height" => "parts.dimension.height",
        "inner" => "parts.dimension.inner",
        "outer" => "parts.dimension.outer",
        "pitch" => "parts.dimension.pitch",
        "socket" => "parts.dimension.socket",
        "socket_depth" => "parts.dimension.socket_depth",
        "thickness" => "parts.dimension.thickness",
        "width" => "parts.dimension.width",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::geom::Vec3;

    fn lib() -> Vec<Family> {
        families(&json!({"families": [
            {"family": "socket_head_cap_screw", "name": "Socket Head Cap Screw", "standard": "ISO 4762",
             "sizes": [{"size": "M5", "head_diameter": 8.5, "default_length": 16},
                       {"size": "M6", "head_diameter": 10, "default_length": 20}], "lengths": [10, 16, 20]},
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

    /// The Length list is lengths only: it opens on the size's default rather than on an item
    /// called “Default”, and a size that reports none opens on the first length.
    #[test]
    fn the_length_list_opens_on_the_size_s_default() {
        let l = lib();
        let ls = l[0].lengths.clone().unwrap_or_default();
        assert_eq!(default_length(&l[0], 0, &ls), 1, "M5 opens on 16");
        assert_eq!(default_length(&l[0], 1, &ls), 2, "M6 opens on 20");
        assert_eq!(default_length(&l[1], 0, &ls), 0, "a size without one opens on the first length");
        assert_eq!(default_length(&l[0], 99, &ls), 0, "so does a size that is not there");
    }

    /// A hole pick seats the part (`at`), several picks seat it in each hole (#9341); without one
    /// it goes to the origin along Z.
    #[test]
    fn insert_params_for_picks() {
        let l = lib();
        let k = Pt { family: 0, size: 1, length: Some(2), name: String::new(), picture: None, lib: None };
        let hole = Sel::Face { body: "Body1".into(), index: 3, point: Vec3::new(5.0, 0.0, 10.0) };
        let p = insert_params(&l, &k, std::slice::from_ref(&hole)).unwrap();
        assert_eq!(p, json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20.0, "at": [5.0, 0.0, 10.0]}));
        let rim = Sel::Edge { body: "Body1".into(), index: 7, point: Vec3::new(25.0, 0.0, 10.0) };
        let p = insert_params(&l, &k, &[hole, rim]).unwrap();
        assert_eq!(p["at"], json!([[5.0, 0.0, 10.0], [25.0, 0.0, 10.0]]));
        assert!(p.get("point").is_none());
        let k = Pt { family: 1, size: 0, length: None, name: "Front bearing".into(), picture: None, lib: None };
        let p = insert_params(&l, &k, &[]).unwrap();
        assert_eq!(p["point"], json!([0.0, 0.0, 0.0]));
        assert_eq!(p["name"], "Front bearing");
        assert!(p.get("length").is_none());
        assert!(insert_params(&[], &k, &[]).is_err());
    }

    /// Every dimension the library gives for a size is labelled in words, in each family.
    #[test]
    fn every_library_dimension_has_a_label_in_words() {
        let mut s = solidcraft_engine::Session::default();
        let l = families(&s.execute("parts.library", &json!({})).unwrap());
        assert!(l.len() >= 7, "{}", l.len());
        for f in &l {
            for (size, row) in &f.sizes {
                for key in row.as_object().into_iter().flat_map(|o| o.keys()).filter(|k| !matches!(k.as_str(), "size" | "default_length")) {
                    let label = dimension_label(key).map(crate::i18n::tr);
                    assert!(
                        label.is_some_and(|l| l.starts_with(char::is_uppercase) && !l.contains('_') && !l.starts_with("parts.")),
                        "{} {size}: {key} is labelled {label:?}",
                        f.name
                    );
                }
            }
        }
    }
}
