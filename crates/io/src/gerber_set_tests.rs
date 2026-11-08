use super::*;

const GERBER: &str = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.1*%\nD10*\nX0Y0D02*\nX1000000Y0D01*\nM02*\n";
const DRILL: &str = "M48\nMETRIC\nT1C0.8\n%\nT1\nX1.0Y1.0\nM30\n";

fn set(files: &[(&str, &str)]) -> GerberSet {
    classify_set("board".into(), files.iter().map(|(n, t)| (n.to_string(), t.as_bytes().to_vec())).collect())
}

fn roles(s: &GerberSet) -> Vec<(&str, SetRole)> {
    s.files.iter().map(|f| (f.name.as_str(), f.role)).collect()
}

/// KiCad, EasyEDA, Protel-style and Eagle names each give their layer.
#[test]
fn file_names_give_the_layers() {
    for (name, role) in [
        ("board-F_Cu.gbr", SetRole::TopCopper),
        ("board-B_Cu.gbr", SetRole::BottomCopper),
        ("board-F_Mask.gbr", SetRole::TopMask),
        ("board-B_Silkscreen.gbr", SetRole::BottomSilk),
        ("board-Edge_Cuts.gbr", SetRole::Outline),
        ("board-F_Paste.gbr", SetRole::Other),
        ("board-In1_Cu.gbr", SetRole::Other),
        ("Gerber_TopSolderMaskLayer.GTS", SetRole::TopMask),
        ("Gerber_BoardOutlineLayer.GKO", SetRole::Outline),
        ("Gerber_BottomSilkscreenLayer.GBO", SetRole::BottomSilk),
        ("board.GTL", SetRole::TopCopper),
        ("board.GM1", SetRole::Outline),
        ("board.cmp", SetRole::TopCopper),
        ("board.sts", SetRole::BottomMask),
        ("notes.gbr", SetRole::Other),
    ] {
        assert_eq!(roles(&set(&[(name, GERBER)])), [(name, role)], "{name}");
    }
}

/// A file's own X2 file function wins over its name; Excellon is a drill whatever its name.
#[test]
fn what_a_file_says_wins_over_its_name() {
    let top = format!("%TF.FileFunction,Copper,L1,Top*%\n{GERBER}");
    let profile = format!("G04 #@! TF.FileFunction,Profile,NP*\n{GERBER}");
    let s = set(&[("a-B_Cu.gbr", &top), ("odd.gbr", &profile), ("holes.txt", DRILL), ("readme.txt", "hello"), ("x.pdf", "%PDF\0")]);
    assert_eq!(roles(&s), [("a-B_Cu.gbr", SetRole::TopCopper), ("odd.gbr", SetRole::Outline), ("holes.txt", SetRole::Drill)]);
    // KiCad's drill files carry X2 attributes in their Excellon header: still drills.
    let kicad = format!(
        "M48
; #@! TF.FileFunction,Plated,1,2,PTH
{}",
        &DRILL[4..]
    );
    assert_eq!(roles(&set(&[("board-PTH.drl", &kicad)])), [("board-PTH.drl", SetRole::Drill)]);
}

/// A board's files picked among many others (a Downloads folder) are still found: pictures and
/// documents are left unread, and a folder with more files than a set has is not refused.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_boards_files_among_many_others_are_found() {
    let dir = std::env::temp_dir().join(format!("solidcraft-gerber-set-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for i in 0..MAX_SET_FILES + 20 {
        std::fs::write(dir.join(format!("photo{i:03}.jpg")), "jpeg").unwrap();
    }
    std::fs::write(dir.join("board.GKO"), GERBER).unwrap();
    std::fs::write(dir.join("board.DRL"), DRILL).unwrap();
    let s = read_gerber_set(&dir.join("board.GKO").to_string_lossy()).unwrap();
    assert_eq!(roles(&s), [("board.DRL", SetRole::Drill), ("board.GKO", SetRole::Outline)]);
    let _ = std::fs::remove_dir_all(dir);
}

/// The job file names the layers of files that don't say, and gives thickness and finish.
#[test]
fn the_job_file_names_layers_and_gives_thickness_and_finish() {
    let job = r#"{"GeneralSpecs": {"BoardThickness": 0.8, "Finish": "ENIG"},
        "FilesAttributes": [{"Path": "x1.gbr", "FileFunction": "SolderMask,Bot"}, {"Path": "x2.gbr", "FileFunction": "Profile"}]}"#;
    let s = set(&[("board-job.gbrjob", job), ("x1.gbr", GERBER), ("x2.gbr", GERBER)]);
    assert_eq!(roles(&s), [("x1.gbr", SetRole::BottomMask), ("x2.gbr", SetRole::Outline)]);
    assert_eq!(s.job, Some(BoardJob { thickness: Some(0.8), finish: Some("ENIG".into()) }));
}

/// Two files for one layer: the first is used and the other is left out, with a warning.
#[test]
fn a_layer_is_one_file() {
    let s = set(&[("a.GTL", GERBER), ("b-F_Cu.gbr", GERBER), ("c.drl", DRILL), ("d.drl", DRILL)]);
    assert_eq!(roles(&s), [("a.GTL", SetRole::TopCopper), ("b-F_Cu.gbr", SetRole::Other), ("c.drl", SetRole::Drill), ("d.drl", SetRole::Drill)]);
    assert!(s.warnings.iter().any(|w| w.contains("a.GTL and b-F_Cu.gbr")), "{:?}", s.warnings);
}

/// A zip's files are read by their path inside it; Mac metadata copies and folders are not.
#[test]
fn a_zip_is_read_by_its_entries() {
    let mut bytes = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [("gerber/board.GKO", GERBER), ("gerber/board.DRL", DRILL), ("__MACOSX/gerber/._board.GKO", GERBER)] {
            assert!(z.start_file(name, opts).is_ok());
            assert!(std::io::Write::write_all(&mut z, text.as_bytes()).is_ok());
        }
        assert!(z.add_directory("gerber/", opts).is_ok());
        assert!(z.finish().is_ok());
    }
    let entries = zip_entries(&bytes).unwrap_or_default();
    let s = classify_set("b.zip".into(), entries);
    assert_eq!(roles(&s), [("gerber/board.GKO", SetRole::Outline), ("gerber/board.DRL", SetRole::Drill)]);
}

/// Bytes that are not a zip, a bad job file or binary files are refused or left out, never a panic.
#[test]
fn hostile_sets_never_panic() {
    assert!(zip_entries(b"PK\x03\x04garbage").is_err());
    assert!(zip_entries(&[]).is_err());
    let s = set(&[("a.gbrjob", "{\"GeneralSpecs\": {\"BoardThickness\": -1, \"Finish\": 7}, \"FilesAttributes\": 3}"), ("b.gbr", "%FS\0")]);
    assert!(s.files.is_empty() && s.job == Some(BoardJob::default()));
}
