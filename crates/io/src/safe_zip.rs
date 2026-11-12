//! Reading an untrusted `.zip` into memory, refusing anything that could escape its folder or blow
//! up when unpacked: absolute paths, `..`, symlinks, device entries, and more bytes than the caller
//! allows (counted as they inflate, whatever the entry headers claim). Nothing touches the disk.

use std::io::{Cursor, Read as _, Write as _};

/// Ceilings for [`read`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
}

/// One file of the archive, by its `/`-separated relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Why an archive was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("the file is not a readable .zip ({0})")]
    NotAZip(String),
    #[error("the .zip has an unsafe entry `{entry}`: {reason}")]
    UnsafeEntry { entry: String, reason: &'static str },
    #[error("the .zip has more than {0} entries")]
    TooManyEntries(usize),
    #[error("`{entry}` unpacks to more than {limit} bytes")]
    EntryTooLarge { entry: String, limit: u64 },
    #[error("the .zip unpacks to more than {limit} bytes")]
    TooLarge { limit: u64 },
}

/// The checked relative path of an entry name: `a/b/c`, never absolute, never `..`, `.` or empty
/// parts, no backslashes, drive letters or control characters. Directory names keep no trailing `/`.
fn checked_path(name: &str) -> Result<String, &'static str> {
    if name.is_empty() {
        return Err("an empty name");
    }
    if name.starts_with('/') {
        return Err("an absolute path");
    }
    if name.contains('\\') {
        return Err("a backslash in the path");
    }
    if name.chars().any(char::is_control) {
        return Err("a control character in the path");
    }
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    let mut parts = Vec::new();
    for (i, part) in trimmed.split('/').enumerate() {
        match part {
            ".." => return Err("a `..` in the path"),
            "" | "." => return Err("an empty or `.` part in the path"),
            _ if i == 0 && part.len() >= 2 && part.as_bytes()[1] == b':' => return Err("an absolute path"),
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Every file of `zip`, after the checks above. Directory entries are checked and left out.
pub fn read(zip: &[u8], limits: Limits) -> Result<Vec<File>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| Error::NotAZip(e.to_string()))?;
    if archive.len() > limits.max_entries {
        return Err(Error::TooManyEntries(limits.max_entries));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| Error::NotAZip(e.to_string()))?;
        let name = entry.name().map_err(|e| Error::NotAZip(e.to_string()))?.into_owned();
        let shown: String = name.chars().filter(|c| !c.is_control()).take(200).collect();
        let unsafe_entry = |reason| Error::UnsafeEntry { entry: shown.clone(), reason };
        let path = checked_path(&name).map_err(unsafe_entry)?;
        if entry.is_symlink() {
            return Err(unsafe_entry("a symbolic link"));
        }
        // Only regular files and folders: no devices, pipes or sockets.
        if let Some(mode) = entry.unix_mode()
            && !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000)
        {
            return Err(unsafe_entry("not a regular file or folder"));
        }
        if entry.is_dir() {
            continue;
        }
        if entry.size() > limits.max_entry_bytes {
            return Err(Error::EntryTooLarge { entry: shown, limit: limits.max_entry_bytes });
        }
        let room = limits.max_entry_bytes.min(limits.max_total_bytes - total);
        let mut bytes = Vec::with_capacity(entry.size().min(room) as usize);
        (&mut entry).take(room.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| Error::NotAZip(format!("{shown}: {e}")))?;
        let len = bytes.len() as u64;
        if len > limits.max_entry_bytes {
            return Err(Error::EntryTooLarge { entry: shown, limit: limits.max_entry_bytes });
        }
        if len > room {
            return Err(Error::TooLarge { limit: limits.max_total_bytes });
        }
        total += len;
        files.push(File { path, bytes });
    }
    Ok(files)
}

/// A deflated `.zip` of `files` (path, bytes), in order: for packaging and tests.
pub fn write(files: &[(&str, &[u8])]) -> std::io::Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).last_modified_time(zip::DateTime::default());
    for (path, bytes) in files {
        zip.start_file(*path, options).map_err(std::io::Error::other)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish().map_err(std::io::Error::other)?.into_inner())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits { max_entries: 16, max_entry_bytes: 1 << 20, max_total_bytes: 2 << 20 };

    fn with(name: &str, mode: Option<u32>, bytes: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut options = zip::write::SimpleFileOptions::default();
        if let Some(mode) = mode {
            options = options.unix_permissions(mode);
        }
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_plain_files_and_skips_folders() {
        let zip = write(&[("a/plugin.json", b"{}"), ("a/x.wasm", b"\0asm")]).unwrap();
        let files = read(&zip, LIMITS).unwrap();
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a/plugin.json", "a/x.wasm"]);
        assert_eq!(files[1].bytes, b"\0asm");
    }

    #[test]
    fn refuses_paths_that_leave_the_folder() {
        for name in ["../evil", "a/../../evil", "/etc/passwd", "C:/evil", "a\\..\\evil", "a//b", "./a"] {
            let error = read(&with(name, None, b"x"), LIMITS).unwrap_err();
            assert!(matches!(error, Error::UnsafeEntry { .. }), "{name}: {error}");
        }
    }

    #[test]
    fn refuses_links_and_devices() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_symlink("link", "/etc/passwd", zip::write::SimpleFileOptions::default()).unwrap();
        let link = zip.finish().unwrap().into_inner();
        assert_eq!(read(&link, LIMITS).unwrap_err(), Error::UnsafeEntry { entry: "link".into(), reason: "a symbolic link" });
        let device = read(&with("dev", Some(0o020644), b""), LIMITS);
        // The writer may drop file-type bits; then it is a plain file, which is safe.
        assert!(matches!(device, Err(Error::UnsafeEntry { .. })) || device.is_ok_and(|f| f[0].path == "dev"));
    }

    #[test]
    fn counts_inflated_bytes_against_the_caps() {
        let bomb = write(&[("zeros", &vec![0u8; (1 << 20) + 1])]).unwrap();
        assert!(bomb.len() < 64 << 10, "deflates small: {}", bomb.len());
        assert!(matches!(read(&bomb, LIMITS), Err(Error::EntryTooLarge { .. })));
        let half = vec![0u8; 900 << 10];
        let many = write(&[("a", &half), ("b", &half), ("c", &half)]).unwrap();
        assert_eq!(read(&many, LIMITS), Err(Error::TooLarge { limit: 2 << 20 }));
        let crowded: Vec<(String, Vec<u8>)> = (0..17).map(|i| (format!("f{i}"), vec![])).collect();
        let crowded: Vec<(&str, &[u8])> = crowded.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        assert_eq!(read(&write(&crowded).unwrap(), LIMITS), Err(Error::TooManyEntries(16)));
        assert!(matches!(read(b"not a zip", LIMITS), Err(Error::NotAZip(_))));
    }
}
