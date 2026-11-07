//! Insert PCB (Gerber): once a board's zip, or any file in its folder, is picked, the dialog lists
//! the Gerber and Excellon files found, with the layer each is read as (changeable), and the
//! board's name, thickness, finish and placement; OK inserts it (#12797). What is found comes from
//! `file.gerber_files`; OK runs `file.insert_gerber` with the set and the layers as listed.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, combo, form_row};
use crate::i18n::tr;

/// Each file's layer: its key in the commands and its label's message id.
const ROLES: [(&str, &str); 9] = [
    ("outline", "ui.insert_pcb.role.outline"),
    ("drill", "ui.insert_pcb.role.drill"),
    ("top_copper", "ui.insert_pcb.role.top_copper"),
    ("bottom_copper", "ui.insert_pcb.role.bottom_copper"),
    ("top_mask", "ui.insert_pcb.role.top_mask"),
    ("bottom_mask", "ui.insert_pcb.role.bottom_mask"),
    ("top_silk", "ui.insert_pcb.role.top_silk"),
    ("bottom_silk", "ui.insert_pcb.role.bottom_silk"),
    ("other", "ui.insert_pcb.role.other"),
];

/// The board being inserted: its set of files and the choices made for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    /// What was picked: a zip, or a file in the board's folder.
    pub path: String,
    /// The zip or folder read.
    pub source: String,
    /// Each file's name in the set and its layer, an index into [`ROLES`].
    pub files: Vec<(String, usize)>,
    pub name: String,
    /// mm.
    pub thickness: f64,
    /// 0: HASL, 1: ENIG.
    pub finish: usize,
    /// 0: where the files put it, 1: its lower-left corner at the origin.
    pub origin: usize,
    pub warnings: Vec<String>,
}

/// Open the dialog for the picked `path`: a zip, or any file in the board's folder.
pub fn open(app: &mut SolveApp, path: String) {
    let found = app.session.execute("file.gerber_files", &json!({"path": path}));
    let mut board = Board {
        path: path.clone(),
        source: path,
        files: Vec::new(),
        name: String::new(),
        thickness: 1.6,
        finish: 0,
        origin: 0,
        warnings: Vec::new(),
    };
    let error = match found {
        Ok(r) => {
            fill(&mut board, &r);
            None
        }
        Err(e) => Some(e.to_string()),
    };
    let mut d = Dialog::new(Kind::InsertPcb(board), Vec::new());
    d.error = error;
    app.dialog = Some(d);
}

/// Take what `file.gerber_files` found.
fn fill(b: &mut Board, r: &Value) {
    if let Some(s) = r["source"].as_str() {
        b.source = s.to_string();
    }
    b.files = r["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|f| {
                    let role = f["role"].as_str()?;
                    Some((f["name"].as_str()?.to_string(), ROLES.iter().position(|(k, _)| *k == role).unwrap_or(ROLES.len() - 1)))
                })
                .collect()
        })
        .unwrap_or_default();
    b.name = r["name"].as_str().unwrap_or("Board").to_string();
    if let Some(t) = r["thickness"].as_f64().filter(|t| t.is_finite() && *t > 0.0 && *t <= 20.0) {
        b.thickness = t;
    }
    b.finish = usize::from(r["finish"].as_str() == Some("enig"));
    b.warnings = r["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
}

pub fn rows(ui: &mut egui::Ui, b: &mut Board) {
    let source = std::path::Path::new(&b.source).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| b.source.clone());
    form_row(ui, tr("ui.insert_pcb.files"), |ui| {
        crate::ui_kit::read_only_text(ui, &source);
    });
    form_row(ui, tr("ui.insert_pcb.name"), |ui| {
        crate::ui_kit::field(ui, &mut b.name);
    });
    form_row(ui, tr("ui.insert_pcb.thickness"), |ui| {
        crate::ui_kit::number_field(ui, &mut b.thickness, 0.01..=20.0, 0.1, " mm");
    });
    form_row(ui, tr("ui.insert_pcb.finish"), |ui| {
        combo(ui, "insert_pcb_finish", &[tr("ui.insert_pcb.finish.hasl"), tr("ui.insert_pcb.finish.enig")], &mut b.finish);
    });
    form_row(ui, tr("ui.insert_pcb.origin"), |ui| {
        combo(ui, "insert_pcb_origin", &[tr("ui.insert_pcb.origin.file"), tr("ui.insert_pcb.origin.corner")], &mut b.origin);
    });
    let labels: Vec<&str> = ROLES.iter().map(|(_, id)| tr(id)).collect();
    for (i, (name, role)) in b.files.iter_mut().enumerate() {
        let short = name.rsplit(['/', '\\']).next().unwrap_or(name).to_string();
        form_row(ui, &short, |ui| {
            combo(ui, &format!("insert_pcb_role_{i}"), &labels, role);
        });
    }
    for w in &b.warnings {
        crate::ui_kit::status_line(ui, w, crate::ui_kit::StatusTone::Warning);
    }
}

/// The command OK runs: the set, with every layer as listed (so a change in the list is used).
pub fn commands(b: &Board) -> Result<Vec<(String, Value)>, String> {
    if !(b.thickness.is_finite() && b.thickness > 0.0 && b.thickness <= 20.0) {
        return Err("the thickness must be more than 0 and at most 20 mm".into());
    }
    let named = |key: &str| -> Vec<String> {
        b.files.iter().filter(|(_, r)| ROLES.get(*r).is_some_and(|(k, _)| *k == key)).map(|(n, _)| n.clone()).collect()
    };
    let outlines = named("outline");
    let outline = match outlines.as_slice() {
        [one] => one.clone(),
        [] => return Err("choose the file with the board outline".into()),
        _ => return Err("choose one file for the board outline".into()),
    };
    let mut layers = serde_json::Map::new();
    for (key, _) in ROLES.iter().filter(|(k, _)| !matches!(*k, "outline" | "drill" | "other")) {
        let files = named(key);
        match files.as_slice() {
            [] => {}
            [one] => {
                layers.insert((*key).to_string(), json!(one));
            }
            _ => return Err(format!("choose one file for each layer ({})", key.replace('_', " "))),
        }
    }
    let mut p = json!({
        "set": b.path,
        "outline": outline,
        "drills": named("drill"),
        "layers": layers,
        "thickness": b.thickness,
        "finish": if b.finish == 1 { "enig" } else { "hasl" },
        "origin": if b.origin == 1 { "corner" } else { "file" },
    });
    if !b.name.trim().is_empty() {
        p["name"] = json!(b.name.trim());
    }
    Ok(vec![("file.insert_gerber".into(), p)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    const OUTLINE: &str =
        "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.1*%\nD10*\nX0Y0D02*\nX30000000Y0D01*\nX30000000Y20000000D01*\nX0Y20000000D01*\nX0Y0D01*\nM02*\n";
    const DRILLS: &str = "M48\nMETRIC\nT1C3.0\n%\nT1\nX5.0Y5.0\nX25.0Y5.0\nM30\n";

    /// A board's folder: the outline, a drill file and a mask whose name says nothing.
    fn board_folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-insert-pcb-{}", std::process::id())).join(name);
        let _ = std::fs::create_dir_all(&dir);
        for (file, text) in [("board.GKO", OUTLINE), ("board.DRL", DRILLS), ("layer7.gbr", OUTLINE), ("notes.txt", "hello")] {
            std::fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn board(h: &Harness) -> Board {
        match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
            Some(Kind::InsertPcb(b)) => b,
            other => panic!("no Insert PCB dialog: {other:?} {:?}", h.app.status),
        }
    }

    /// Insert PCB asks for a file, then lists the board's files and the layer each is read as;
    /// a layer changed in the list is used, and OK inserts the board in one undo step.
    #[test]
    fn a_picked_file_lists_its_boards_files_and_ok_inserts_the_board() {
        let mut h = Harness::new();
        let dir = board_folder("Picked");
        let picked = dir.join("board.GKO").to_string_lossy().into_owned();
        h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(Some(picked.clone())))));
        let before = h.app.session.doc.components.len();
        h.app.start("file.insert_gerber");
        h.frames(3);
        let b = board(&h);
        let roles: Vec<(&str, &str)> = b.files.iter().map(|(n, r)| (n.as_str(), ROLES[*r].0)).collect();
        assert_eq!(roles, [("board.DRL", "drill"), ("board.GKO", "outline"), ("layer7.gbr", "other")]);
        assert_eq!((b.name.as_str(), b.thickness), ("Picked", 1.6));
        assert_eq!(h.app.session.doc.components.len(), before, "nothing is inserted before OK");
        // The unnamed layer is the top mask.
        if let Some(Kind::InsertPcb(b)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            b.files[2].1 = ROLES.iter().position(|(k, _)| *k == "top_mask").unwrap();
            b.thickness = 1.0;
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let c = h.app.session.doc.components.iter().find(|c| c.name == "Picked").unwrap_or_else(|| panic!("no board"));
        assert_eq!(c.properties.get("pcb.top_mask").map(String::as_str), Some("layer7.gbr"));
        assert_eq!(c.properties.get("pcb.thickness").map(String::as_str), Some("1"));
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert!(!h.app.session.doc.components.iter().any(|c| c.name == "Picked"), "one undo removes the board");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// OK needs exactly one outline and one file per layer; a folder with no board files says so.
    #[test]
    fn the_list_must_name_one_outline_and_one_file_per_layer() {
        let mut b = Board {
            path: "x".into(),
            source: "x".into(),
            files: vec![("a".into(), 1)],
            name: "B".into(),
            thickness: 1.6,
            finish: 0,
            origin: 0,
            warnings: Vec::new(),
        };
        assert!(commands(&b).unwrap_err().contains("outline"));
        b.files = vec![("a".into(), 0), ("b".into(), 2), ("c".into(), 2)];
        assert!(commands(&b).unwrap_err().contains("one file for each layer"));
        b.files.pop();
        let cmds = commands(&b).unwrap();
        assert_eq!(cmds[0].1["layers"], json!({"top_copper": "b"}));
        let mut h = Harness::new();
        let empty = std::env::temp_dir().join(format!("solidcraft-insert-pcb-empty-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&empty);
        open(&mut h.app, empty.to_string_lossy().into_owned());
        assert!(board(&h).files.is_empty());
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.as_ref().and_then(|d| d.error.clone()).is_some_and(|e| e.contains("outline")), "OK says what is missing");
        let _ = std::fs::remove_dir_all(empty);
    }
}
