//! Sandbox save destinations; shared with host-side tests without linking UIKit.
use std::fs;
use std::path::{Component, Path};

pub fn save_path(documents: &Path, name: &str) -> Option<String> {
    // A suggested name is a basename, never permission to leave Documents.
    let mut components = Path::new(name).components();
    let Component::Normal(name) = components.next()? else { return None };
    if components.next().is_some() {
        return None;
    }
    fs::create_dir_all(documents).ok()?;
    let requested = documents.join(name);
    if !requested.exists() {
        return requested.into_os_string().into_string().ok();
    }
    // Save As has no overwrite-confirmation sheet on this host. Preserve the
    // previous design/export instead; ordinary Save uses its established path.
    let name = Path::new(name);
    let stem = name.file_stem()?.to_str()?;
    let suffix = name.extension().and_then(|ext| ext.to_str()).map(|ext| format!(".{ext}")).unwrap_or_default();
    for number in 2..=128 {
        let candidate = documents.join(format!("{stem} ({number}){suffix}"));
        if !candidate.exists() {
            return candidate.into_os_string().into_string().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("solidcraft-ios-path-test-{}-{nanos}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(path)
    }

    #[test]
    fn saves_only_sandbox_basenames() -> Result<(), Box<dyn std::error::Error>> {
        let documents = temporary()?;
        for bad in ["", ".", "..", "../outside.solidcraft", "/tmp/outside.solidcraft", "nested/part.step"] {
            assert_eq!(save_path(&documents, bad), None, "{bad}");
        }
        let path = save_path(&documents, "part.solidcraft").ok_or("No save destination")?;
        assert_eq!(Path::new(&path), documents.join("part.solidcraft"));
        fs::remove_dir_all(documents)?;
        Ok(())
    }

    #[test]
    fn save_as_never_replaces_an_existing_design() -> Result<(), Box<dyn std::error::Error>> {
        let documents = temporary()?;
        let original = documents.join("π.plate.solidcraft");
        fs::write(&original, b"original design")?;
        fs::write(documents.join("π.plate (2).solidcraft"), b"another design")?;
        let path = save_path(&documents, "π.plate.solidcraft").ok_or("No save destination")?;
        assert_eq!(Path::new(&path), documents.join("π.plate (3).solidcraft"));
        assert_eq!(fs::read(original)?, b"original design");
        fs::remove_dir_all(documents)?;
        Ok(())
    }

    #[test]
    fn collision_search_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let documents = temporary()?;
        fs::write(documents.join("part.step"), b"existing")?;
        for number in 2..=128 {
            fs::write(documents.join(format!("part ({number}).step")), b"existing")?;
        }
        assert_eq!(save_path(&documents, "part.step"), None);
        fs::remove_dir_all(documents)?;
        Ok(())
    }
}
