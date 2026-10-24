//! Filesystem actions for saved designs; they do not edit or replace the current design.
use super::CommandSpec;
use crate::{
    EngineError, Result, Session,
    params::{bad, str_},
};
use serde_json::{Value, json};
use std::path::Path;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("file.copy", "Copy Design File", copy)
        .icon("copy")
        .noundo()
        .doc_only()
        .params("path, destination; copies the saved design without replacing an existing file; rebases linked designs"),
    CommandSpec::new("file.move", "Move Design File", move_file)
        .icon("move")
        .noundo()
        .doc_only()
        .params("path, destination; also renames; refuses the current design or its linked files; never replaces an existing file"),
    CommandSpec::new("file.trash", "Move Design File to Trash", trash)
        .icon("delete")
        .noundo()
        .doc_only()
        .params("path; refuses the current design or its linked files; system Trash only, never permanent deletion"),
];

fn arg<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    str_(p, key)
        .filter(|s| !s.trim().is_empty() && s.len() < 4096 && !s.contains('\0'))
        .ok_or_else(|| bad("file", format!("A valid {key} is required")))
}

pub(crate) fn same_file(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let (Ok(a), Ok(b)) = (Path::new(a).canonicalize(), Path::new(b).canonicalize()) {
        return a == b;
    }
    false
}

/// Refuse removal of an open design or a file referenced by its linked components.
pub(crate) fn uses_file(s: &Session, path: &str) -> bool {
    s.path.as_deref().is_some_and(|own| same_file(own, path))
        || s.doc.links.values().any(|link| {
            let link_path = Path::new(&link.path);
            let resolved = if link_path.is_relative() {
                s.path.as_deref().and_then(|p| Path::new(p).parent()).unwrap_or_else(|| Path::new(".")).join(link_path)
            } else {
                link_path.to_path_buf()
            };
            resolved.to_str().is_some_and(|p| same_file(p, path))
        })
}

fn guard(s: &Session, path: &str) -> Result<()> {
    if uses_file(s, path) {
        return Err(EngineError::Other("Close designs using this file before renaming, moving, or trashing it".into()));
    }
    Ok(())
}

fn transfer(s: &mut Session, p: &Value, moving: bool) -> Result<Value> {
    let path = arg(p, "path")?;
    let destination = arg(p, "destination")?;
    if moving {
        guard(s, path)?;
    }
    if !solidcraft_io::is_design_path(path) || !solidcraft_io::is_design_path(destination) {
        return Err(bad("file", "Keep a supported design extension (.solidcraft or .json)"));
    }
    let original = solidcraft_io::vfs::read(path, solidcraft_io::MAX_DESIGN_BYTES).map_err(|e| EngineError::Other(e.to_string()))?;
    // Validate before a filesystem mutation, and keep external component links pointing
    // to the same originals when moving a saved assembly to another folder.
    let mut doc = solidcraft_io::read_design(&original)?;
    let links = super::component_link::rebased(&doc, Some(path), destination);
    let contents = if links != doc.links {
        doc.links = links;
        if destination.to_lowercase().ends_with(".json") { solidcraft_io::write_design(&doc) } else { solidcraft_io::write_design_container(&doc)? }
    } else {
        original.clone()
    };
    solidcraft_io::library_files::transfer(path, destination, &original, &contents, moving)
        .map_err(|e| EngineError::Other(format!("File operation failed: {e}")))?;
    Ok(json!({"path":path, "destination":destination}))
}
fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    transfer(s, p, false)
}
fn move_file(s: &mut Session, p: &Value) -> Result<Value> {
    transfer(s, p, true)
}
fn trash(s: &mut Session, p: &Value) -> Result<Value> {
    let path = arg(p, "path")?;
    guard(s, path)?;
    if !solidcraft_io::is_design_path(path) {
        return Err(bad("file.trash", "Choose a design file"));
    }
    solidcraft_io::library_files::trash(path).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(json!({"path":path,"trashed":true}))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    static NEXT_FIXTURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            // Clock resolution alone cannot distinguish concurrent test fixtures.
            let serial = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("solidcraft-library-{}-{nanos}-{serial}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn design(&self, name: &str) -> std::path::PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, solidcraft_io::write_design(&solidcraft_doc::Document::new(name))).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn session() -> Session {
        Session::new(solidcraft_doc::Document::new("Unsaved"))
    }

    #[test]
    fn library_copy_move_and_rename_preserve_current_design_and_refuse_overwrites() {
        let f = Fixture::new();
        let original = f.design("Original.solidcraft");
        let copy = f.0.join("Copy.solidcraft");
        let renamed = f.0.join("Renamed.solidcraft");
        let bytes = std::fs::read(&original).unwrap();
        let mut s = session();
        s.execute("solid.box", &json!({"length":10,"width":12,"height":14})).unwrap();
        let before = s.doc.to_json();
        let undo = s.undo.len();
        s.execute("file.copy", &json!({"path":original,"destination":copy})).unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        assert_eq!(std::fs::read(&copy).unwrap(), bytes);
        assert!(s.execute("file.move", &json!({"path":original,"destination":copy})).is_err());
        assert!(original.exists());
        s.execute("file.move", &json!({"path":copy,"destination":renamed})).unwrap();
        assert!(!copy.exists());
        assert_eq!(std::fs::read(&renamed).unwrap(), bytes);
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
        assert!(s.is_dirty());
    }

    #[test]
    fn library_guards_open_and_linked_files_and_rebases_assemblies() {
        let f = Fixture::new();
        let original = f.design("Part.solidcraft");
        let assembly = f.0.join("Assembly.solidcraft");
        let mut doc = solidcraft_doc::Document::new("Assembly");
        doc.links.insert(1, solidcraft_doc::DesignLink { path: "Part.solidcraft".into(), feature: 1 });
        std::fs::write(&assembly, solidcraft_io::write_design(&doc)).unwrap();
        let mut s = session();
        s.path = Some(original.to_string_lossy().into_owned());
        assert!(s.execute("file.move", &json!({"path":original,"destination":f.0.join("Other.solidcraft")})).is_err());
        assert!(s.execute("file.trash", &json!({"path":original})).is_err());
        s.path = Some(assembly.to_string_lossy().into_owned());
        s.doc_mut().links = doc.links.clone();
        assert!(uses_file(&s, original.to_str().unwrap()));
        assert!(s.execute("file.trash", &json!({"path":original})).is_err());
        let destination = f.0.join("Project");
        std::fs::create_dir(&destination).unwrap();
        let moved = destination.join("Assembly.solidcraft");
        let mut other = session();
        other.execute("file.move", &json!({"path":assembly,"destination":moved})).unwrap();
        assert!(!assembly.exists());
        let relocated = solidcraft_io::read_design(&std::fs::read(&moved).unwrap()).unwrap();
        let link = relocated.links.get(&1).unwrap();
        assert_eq!(destination.join(&link.path).canonicalize().unwrap(), original.canonicalize().unwrap());
    }

    #[test]
    fn library_failed_copy_leaves_sources_and_existing_symlinks_intact() {
        let f = Fixture::new();
        let original = f.design("Part.solidcraft");
        let mut s = session();
        let before = std::fs::read(&original).unwrap();
        assert!(s.execute("file.copy", &json!({"path":original,"destination":f.0.join("missing/Part.solidcraft")})).is_err());
        let invalid = f.0.join("Invalid.solidcraft");
        std::fs::write(&invalid, b"broken").unwrap();
        assert!(s.execute("file.move", &json!({"path":invalid,"destination":f.0.join("Other.solidcraft")})).is_err());
        #[cfg(unix)]
        {
            let symlink = f.0.join("Link.solidcraft");
            std::os::unix::fs::symlink("missing.solidcraft", &symlink).unwrap();
            assert!(s.execute("file.copy", &json!({"path":original,"destination":symlink})).is_err());
            assert!(std::fs::symlink_metadata(&symlink).unwrap().file_type().is_symlink());
        }
        assert_eq!(std::fs::read(&original).unwrap(), before);
        assert_eq!(std::fs::read(&invalid).unwrap(), b"broken");
    }

    #[test]
    fn library_io_detects_changed_source_without_removing_either_version() {
        let f = Fixture::new();
        let source = f.design("Changed.solidcraft");
        let destination = f.0.join("Copied.solidcraft");
        let original = std::fs::read(&source).unwrap();
        std::fs::write(&source, b"new version").unwrap();
        assert!(solidcraft_io::library_files::transfer(source.to_str().unwrap(), destination.to_str().unwrap(), &original, &original, true).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"new version");
        assert_eq!(std::fs::read(destination).unwrap(), original);
    }
    #[test]
    #[ignore = "run with isolated XDG_DATA_HOME and SOLIDCRAFT_TEST_TRASH=1 on Linux"]
    fn library_system_trash_preserves_recoverable_contents() {
        assert_eq!(std::env::var("SOLIDCRAFT_TEST_TRASH").as_deref(), Ok("1"));
        let data = std::path::PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap());
        assert!(data.starts_with(std::env::temp_dir()));
        let f = Fixture::new();
        let path = f.design("DisposableTrashTest.solidcraft");
        let bytes = std::fs::read(&path).unwrap();
        session().execute("file.trash", &json!({"path":path})).unwrap();
        assert!(!path.exists());
        let trashed = std::fs::read_dir(data.join("Trash/files"))
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with("DisposableTrashTest"))
            .unwrap()
            .path();
        assert_eq!(std::fs::read(&trashed).unwrap(), bytes);
        let info = data.join("Trash/info").join(format!("{}.trashinfo", trashed.file_name().unwrap().to_string_lossy()));
        let metadata = std::fs::read_to_string(&info).unwrap();
        assert!(metadata.contains("Path="));
        // Restore the disposable original from the trash payload, proving recoverability.
        std::fs::rename(&trashed, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(info).unwrap();
    }
}
