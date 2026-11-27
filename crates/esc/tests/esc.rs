//! The corpus round-trips and checks clean, `fmt` is idempotent, each diagnostic code has one
//! case (`tests/diagnostics/<code>/main.esc` and the output `check` gives for it in
//! `expected.txt`), and each edit case (`tests/edits/<case>.json`, an intent on a corpus file)
//! gives the files in `<case>.txt`. `UPDATE_EXPECTED=1` rewrites the expected files, to review in
//! the diff.

use std::path::{Path, PathBuf};

use solidcraft_esc::check::check;
use solidcraft_esc::edit::{Intent, apply};
use solidcraft_esc::fmt::format;
use solidcraft_esc::parse::parse;
use solidcraft_esc::project::{Project, list};
use solidcraft_esc::{Code, Located};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn compare(expected: &Path, out: &str) {
    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        std::fs::write(expected, out).unwrap();
    }
    assert_eq!(out, std::fs::read_to_string(expected).unwrap_or_default(), "{}", expected.display());
}

fn rendered(path: &Path) -> String {
    let mut project = Project::default();
    let checked = check(&mut project, path).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    checked.diagnostics.iter().map(|d| Located::new(&name, &text, d).render(&text)).collect()
}

#[test]
fn every_file_round_trips_through_the_parser_and_formats_idempotently() {
    for path in list(&tests_dir()).unwrap() {
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(parse(&text).syntax().to_string(), text, "{}", path.display());
        if let Ok(once) = format(&text) {
            assert_eq!(format(&once), Ok(once.clone()), "{}", path.display());
        }
    }
}

#[test]
fn the_corpus_checks_clean() {
    for path in list(&tests_dir().join("corpus")).unwrap() {
        assert_eq!(rendered(&path), "", "{}", path.display());
    }
}

#[test]
fn each_diagnostic_code_gives_its_expected_output() {
    for code in Code::ALL {
        let dir = tests_dir().join("diagnostics").join(code.as_str());
        let out = rendered(&dir.join("main.esc"));
        assert!(out.contains(&format!("[{}]", code.as_str())), "{}: no {} in\n{out}", dir.display(), code.as_str());
        compare(&dir.join("expected.txt"), &out);
    }
}

#[derive(serde::Deserialize)]
struct EditCase {
    file: String,
    intent: Intent,
}

#[test]
fn each_edit_gives_its_expected_files_and_leaves_them_clean() {
    let corpus = tests_dir().join("corpus");
    let files = list(&corpus).unwrap();
    for case in std::fs::read_dir(tests_dir().join("edits")).unwrap() {
        let case = case.unwrap().path();
        if case.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let EditCase { file, intent } = serde_json::from_str(&std::fs::read_to_string(&case).unwrap()).unwrap();
        let mut project = Project::default();
        let out = match apply(&mut project, &files, &corpus.join(&file), &intent) {
            Ok(changed) => {
                let mut out = String::new();
                for (path, text) in &changed {
                    out.push_str(&format!("== {}\n{text}", path.strip_prefix(&corpus).unwrap().display()));
                    project.replace(path, text.clone());
                }
                for (path, _) in &changed {
                    assert_eq!(check(&mut project, path).unwrap().diagnostics, [], "{}: {}", case.display(), path.display());
                }
                out
            }
            Err(message) => format!("error: {message}\n"),
        };
        compare(&case.with_extension("txt"), &out);
    }
}
