//! Local document file operations. Destinations are never replaced; Trash has no permanent-delete fallback.
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
pub fn transfer(source: &str, destination: &str, original: &[u8], contents: &[u8], moving: bool) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Write};
    crate::vfs::check(source)?;
    crate::vfs::check(destination)?;
    let source = Path::new(source);
    let destination = Path::new(destination);
    if !std::fs::symlink_metadata(source)?.file_type().is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "Choose a regular design file, not a symbolic link or folder"));
    }
    let parent = destination.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().set_permissions(std::fs::metadata(source)?.permissions())?;
    temporary.as_file().sync_all()?;
    // persist_noclobber keeps an existing file (including a dangling symlink) intact.
    temporary.persist_noclobber(destination).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    if moving {
        // If another program saved while we copied, keep both files and report it.
        let current = crate::vfs::read(&source.to_string_lossy(), crate::MAX_DESIGN_BYTES)?;
        if current != original {
            return Err(Error::other("The original changed while copying. Both files were kept; refresh and retry"));
        }
        std::fs::remove_file(source).map_err(|e| Error::new(e.kind(), format!("The copy is saved, but the original could not be removed: {e}")))?;
    }
    #[cfg(unix)]
    for directory in [source.parent(), destination.parent()].into_iter().flatten() {
        if let Ok(file) = std::fs::File::open(directory) {
            let _ = file.sync_all();
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn transfer(_: &str, _: &str, _: &[u8], _: &[u8], _: bool) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Local file management requires the desktop app"))
}

pub fn trash(path: &str) -> std::io::Result<()> {
    crate::vfs::check(path)?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !std::fs::symlink_metadata(Path::new(path))?.file_type().is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Choose a regular design file"));
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return trash::delete(path)
            .map_err(|e| std::io::Error::other(format!("Could not move to the system Trash: {e}. The file was not permanently deleted")));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            // GIO uses the desktop's trash backend, and refuses unsupported volumes.
            // No shell and no --force: a failure must never become permanent deletion.
            let result = std::process::Command::new("gio")
                .args(["trash", "--", path])
                .output()
                .map_err(|e| std::io::Error::other(format!("System Trash requires the GIO trash command: {e}")))?;
            if result.status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(format!("Could not move to Trash: {}", String::from_utf8_lossy(&result.stderr).trim())))
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "System Trash requires the desktop app"))
}
