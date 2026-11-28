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
fn numeric_functions_describe_the_numeric_requirement() {
    let path = Path::new("numeric_functions.esc");
    let mut project = Project::default();
    project.replace(path, "esc 1\npart\n\nx = param(abs(\"x\"))\n".to_owned());
    let checked = check(&mut project, path).unwrap();
    let diagnostic = checked.diagnostics.iter().find(|d| d.code == Code::WrongKind).unwrap();
    assert_eq!(diagnostic.message, "`abs` takes a number or quantity, found text");
}

#[test]
fn instances_require_an_import_reference() {
    let path = Path::new("instance_argument.esc");
    for argument in ["42", "\"wheel\"", "[42]"] {
        let mut project = Project::default();
        project.replace(path, format!("esc 1\nassembly\n\ncopy = instance({argument})\n"));
        let checked = check(&mut project, path).unwrap();
        assert!(checked.diagnostics.iter().any(|d| d.code == Code::WrongKind && d.message == "`instance` takes the name of an import"));
    }
}

#[test]
fn instances_follow_dotted_imports_and_check_their_members() {
    let path = tests_dir().join("corpus/dotted_instance.esc");
    for (argument, member, valid) in [("parts.wheel", "hub", true), ("((parts.wheel))", "hub", true), ("parts.wheel", "missing_connector", false)] {
        let mut project = Project::default();
        project.replace(
            &path,
            format!("esc 1\nassembly\n\nparts = import(\"robot\")\n\ncopy = instance({argument})\n\njoint = rigid(copy.{member}, copy.hub)\n"),
        );
        let checked = check(&mut project, &path).unwrap();
        if valid {
            assert_eq!(checked.diagnostics, []);
        } else {
            assert_eq!(checked.diagnostics.len(), 1, "{:?}", checked.diagnostics);
            assert_eq!(checked.diagnostics[0].code, Code::NoMember);
            assert_eq!(checked.diagnostics[0].message, "`copy` has no entry `missing_connector`");
        }
    }
}

#[test]
fn parenthesized_import_paths_are_checked() {
    for (operation, file) in [("import", "missing"), ("import_step", "missing.step")] {
        let path = tests_dir().join("parenthesized_import.esc");
        let mut project = Project::default();
        project.replace(&path, format!("esc 1\npart\n\nexternal = {operation}(((\"{file}\")))\n"));
        let checked = check(&mut project, &path).unwrap();
        assert!(checked.diagnostics.iter().any(|d| d.code == Code::FileNotFound), "{:?}", checked.diagnostics);
    }
}

#[test]
fn import_chains_are_checked_without_recursive_calls() {
    let dir = std::env::temp_dir().join(format!("solidcraft-esc-import-chain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut project = Project::default();
    let count = 600;
    for index in 0..count {
        let text = if index + 1 == count {
            "esc 1\npart\n\nwidth = param(1 mm)\n".to_owned()
        } else {
            format!("esc 1\npart\n\nnext = import(\"{}\")\n", index + 1)
        };
        std::fs::write(dir.join(format!("{index}.esc")), text).unwrap();
    }
    let checked = check(&mut project, &dir.join("0.esc")).unwrap();
    assert_eq!(checked.diagnostics, []);
    let mut project = Project::default();
    for index in (0..count).rev() {
        let checked = check(&mut project, &dir.join(format!("{index}.esc"))).unwrap();
        assert_eq!(checked.diagnostics, []);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cycle_diagnostics_cover_each_cyclic_import_and_refresh_after_edits() {
    let dir = std::env::temp_dir().join(format!("solidcraft-esc-components-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, imports) in [("a", vec!["b", "c"]), ("b", vec!["a"]), ("c", vec!["b", "leaf"]), ("leaf", vec![]), ("root", vec!["a", "leaf"])] {
        let mut text = "esc 1\nassembly\n".to_owned();
        for imported in imports {
            text.push_str(&format!("\n{imported} = import(\"{imported}\")\n"));
        }
        std::fs::write(dir.join(format!("{name}.esc")), text).unwrap();
    }
    let mut project = Project::default();
    for (name, cycles) in [("a", 2), ("b", 1), ("c", 1), ("leaf", 0), ("root", 0)] {
        let checked = check(&mut project, &dir.join(format!("{name}.esc"))).unwrap();
        assert_eq!(checked.diagnostics.iter().filter(|d| d.code == Code::ImportCycle).count(), cycles, "{name}: {:?}", checked.diagnostics);
    }
    project.replace(&dir.join("c.esc"), "esc 1\nassembly\n\nleaf = import(\"leaf\")\n".to_owned());
    let checked = check(&mut project, &dir.join("a.esc")).unwrap();
    assert_eq!(checked.diagnostics.iter().filter(|d| d.code == Code::ImportCycle).count(), 1);
    assert_eq!(check(&mut project, &dir.join("c.esc")).unwrap().diagnostics, []);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn the_loader_rejects_a_fifo_without_waiting_for_a_writer() {
    let dir = std::env::temp_dir().join(format!("solidcraft-esc-fifo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("pipe.esc");
    rustix::fs::mkfifoat(rustix::fs::CWD, &fifo, rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR).unwrap();
    let error = Project::default().load(&fifo).unwrap_err();
    assert!(error.contains("isn't a regular file"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn import_cycles_through_symlinked_directories_are_reported() {
    let dir = std::env::temp_dir().join(format!("solidcraft-esc-symlink-cycle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(".", dir.join("nested")).unwrap();
    let path = dir.join("source.esc");
    std::fs::write(&path, "esc 1\npart\n\nother = import(\"nested/source\")\n").unwrap();
    let checked = check(&mut Project::default(), &path).unwrap();
    assert!(checked.diagnostics.iter().any(|d| d.code == Code::ImportCycle));
    std::fs::remove_dir_all(dir).unwrap();
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
