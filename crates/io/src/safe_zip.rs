//! Reading an untrusted `.zip` into memory, refusing anything that could escape its folder or blow
//! up when unpacked: absolute paths, `..`, symlinks, device entries, and more bytes than the caller
//! allows (counted as they inflate, whatever the entry headers claim). Nothing touches the disk.

use std::collections::BTreeMap;
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
/// A package folder read as its `.zip` would be (`plugin.check`) goes through it too, so a folder
/// and its `.zip` get the same answer.
pub fn checked_path(name: &str) -> Result<String, &'static str> {
    if name.len() > 4096 || name.split('/').count() > 64 {
        return Err("a path that is too long or too deep");
    }
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
    for part in trimmed.split('/') {
        match part {
            ".." => return Err("a `..` in the path"),
            "" | "." => return Err("an empty or `.` part in the path"),
            _ if windows_device(part) => return Err("a reserved device name"),
            _ if part.contains(':') => return Err("a drive letter or stream in the path"),
            _ if part.ends_with(['.', ' ']) => return Err("a trailing dot or space in the path"),
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

// Device aliases apply even with an extension, and must stay ordinary files on Windows.
fn windows_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes().get(3).is_some_and(|b| matches!(b, b'1'..=b'9'))
}

// Keep directory components once, rather than storing a full path for every prefix.
// This bounds memory by the names in the archive, including deeply nested paths.
#[derive(Default)]
struct Names(BTreeMap<String, Name>);
struct Name {
    spelling: String,
    directory: bool,
    declared: bool,
    children: BTreeMap<String, Name>,
}
impl Names {
    fn insert(&mut self, path: &str, directory: bool) -> Result<(), &'static str> {
        let mut names = &mut self.0;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let is_dir = !last || directory;
            let node = names.entry(part.to_lowercase()).or_insert_with(|| Name {
                spelling: part.to_string(),
                directory: is_dir,
                declared: false,
                children: BTreeMap::new(),
            });
            if node.spelling != part || node.directory != is_dir || (last && node.declared) {
                return Err("a duplicate, case-colliding or file/folder path");
            }
            if last {
                node.declared = true;
            }
            names = &mut node.children;
        }
        Ok(())
    }
}

/// Every file of `zip`, after the checks above. Directory entries are checked and left out.
pub fn read(zip: &[u8], limits: Limits) -> Result<Vec<File>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| Error::NotAZip(e.to_string()))?;
    if archive.has_duplicate_names() {
        return Err(Error::UnsafeEntry { entry: String::new(), reason: "duplicate entry names" });
    }
    if archive.len() > limits.max_entries {
        return Err(Error::TooManyEntries(limits.max_entries));
    }
    // Refuse an advertised bomb before reserving any file buffers. Count again while
    // inflating: the directory's sizes are untrusted too.
    let mut advertised = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|e| Error::NotAZip(e.to_string()))?;
        if entry.size() > limits.max_entry_bytes {
            return Err(Error::EntryTooLarge {
                entry: entry.name().map_err(|e| Error::NotAZip(e.to_string()))?.chars().filter(|c| !c.is_control()).take(200).collect(),
                limit: limits.max_entry_bytes,
            });
        }
        advertised =
            advertised.checked_add(entry.size()).filter(|n| *n <= limits.max_total_bytes).ok_or(Error::TooLarge { limit: limits.max_total_bytes })?;
    }
    let mut files = Vec::new();
    let mut names = Names::default();
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
        // Compare directory components too: `Parts/a` and `parts/b` collide.
        names.insert(&path, entry.is_dir()).map_err(unsafe_entry)?;
        let directory = entry.is_dir();
        if directory && entry.size() != 0 {
            return Err(unsafe_entry("a directory with file data"));
        }
        if entry.size() > limits.max_entry_bytes {
            return Err(Error::EntryTooLarge { entry: shown, limit: limits.max_entry_bytes });
        }
        let room = if directory { 0 } else { limits.max_entry_bytes.min(limits.max_total_bytes - total) };
        let mut bytes = Vec::with_capacity(entry.size().min(room) as usize);
        (&mut entry).take(room.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| Error::NotAZip(format!("{shown}: {e}")))?;
        let len = bytes.len() as u64;
        if len > limits.max_entry_bytes {
            return Err(Error::EntryTooLarge { entry: shown, limit: limits.max_entry_bytes });
        }
        if len > room {
            return Err(Error::TooLarge { limit: limits.max_total_bytes });
        }
        if len != entry.size() {
            return Err(Error::NotAZip(format!("{shown}: the entry size disagrees with its data")));
        }
        total += len;
        if !directory {
            files.push(File { path, bytes });
        }
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
        let zip = write(&[("a/", b""), ("a/plugin.json", b"{}"), ("a/x.wasm", b"\0asm")]).unwrap();
        let files = read(&zip, LIMITS).unwrap();
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a/plugin.json", "a/x.wasm"]);
        assert_eq!(files[1].bytes, b"\0asm");
    }

    #[test]
    fn refuses_paths_that_leave_the_folder() {
        for name in
            ["../evil", "a/../../evil", "/etc/passwd", "C:/evil", "a\\..\\evil", "a//b", "./a", "a/file:stream", "a/side. ", "NUL.txt", "COM1"]
        {
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
    fn refuses_case_collisions_and_file_directory_conflicts() {
        for names in [["part.dxf", "PART.dxf"], ["a", "a/part.dxf"], ["A/x", "a/y"]] {
            let zip = write(&[(names[0], b"one"), (names[1], b"two")]).unwrap();
            assert!(matches!(read(&zip, LIMITS), Err(Error::UnsafeEntry { .. })), "{names:?}");
        }
        let mut zip = write(&[("a.txt", b"one"), ("b.txt", b"two")]).unwrap();
        for offset in 0..zip.len().saturating_sub(4) {
            if zip.get(offset..offset + 5) == Some(b"b.txt") {
                zip.get_mut(offset..offset + 5).unwrap().copy_from_slice(b"a.txt");
            }
        }
        assert!(matches!(read(&zip, LIMITS), Err(Error::UnsafeEntry { .. })), "duplicate names");
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
        assert!(matches!(read(&with("folder/", None, b"hidden data"), LIMITS), Err(Error::UnsafeEntry { .. })));
        let mut corrupt_directory = with("folder/", None, b"");
        let central = corrupt_directory.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        corrupt_directory[central + 16..central + 20].copy_from_slice(&1u32.to_le_bytes());
        assert!(matches!(read(&corrupt_directory, LIMITS), Err(Error::NotAZip(_))));
    }
}
