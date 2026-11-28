//! Optional folder reads share the import's budgets and never expose ambient paths or writes.
use super::*;

pub(super) fn open(host: &impl PluginHost, manifest: &Manifest) -> Result<BTreeMap<String, Folder>> {
    let mut folders = BTreeMap::new();
    if !manifest.granted_capabilities().iter().any(|c| c.starts_with("folders@")) {
        return Ok(folders);
    }
    let grants = host.granted_input_folders();
    if grants.len() > 32 {
        return Err(limit("at most 32 input folders"));
    }
    for grant in grants {
        if !grant.read || grant.write || folders.contains_key(&grant.name) {
            return Err(HostError::PermissionDenied { permission: "input folders must be distinct read-only grants".into() });
        }
        let dir = Dir::open_ambient_dir(&grant.folder, cap_std::ambient_authority())
            .map_err(|e| HostError::PermissionDenied { permission: format!("folders:{}: {e}", grant.name) })?;
        folders.insert(grant.name, Folder { dir, path: grant.folder, read: true, write: false });
    }
    Ok(folders)
}

fn plain_name(name: &str) -> bool {
    if name.starts_with('.') {
        return false;
    }
    if !cfg!(windows) {
        return true;
    }
    let upper = name.split('.').next().unwrap_or_default().trim_end_matches(' ').to_ascii_uppercase();
    let device = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(*prefix).is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
        });
    !device && !name.contains('~')
}

impl<H: PluginHost> State<H> {
    fn folder_path(&mut self, grant: &str, path: &str) -> std::result::Result<PathBuf, PluginError> {
        self.check()?;
        self.admit_folder_path(grant, path).map_err(|error| self.fail(error))
    }
    /// The checks of `folder_path`; a refusal is returned without ending the run.
    fn admit_folder_path(&mut self, grant: &str, path: &str) -> std::result::Result<PathBuf, HostError> {
        let Some(folder) = self.input_folders.get(grant) else {
            return Err(HostError::PermissionDenied { permission: format!("folders:{grant}: no approved directory for this entry") });
        };
        let target: PathBuf = Path::new(path).components().collect();
        if path.len() > 4096
            || path.contains(['\\', ':'])
            || path.chars().any(char::is_control)
            || target.components().count() > 64
            || target.components().any(|c| !matches!(c, PathComponent::Normal(name) if plain_name(&name.to_string_lossy())))
        {
            return Err(HostError::PermissionDenied {
                permission: format!("folders:{grant}: path must stay inside the directory with no hidden names"),
            });
        }
        if let Err(error) = refuse_links(&folder.dir, &target) {
            return Err(HostError::PermissionDenied { permission: format!("folders:{grant}: {error}") });
        }
        self.host.authorize_input_file_path(&folder.path.join(&target))?;
        let key = (grant.to_owned(), target.clone());
        let source_files = self.import.as_ref().map_or(0, |i| i.files.len());
        if self.folder_files.len().saturating_add(source_files) >= MAX_IMPORT_FILES && !self.folder_files.contains(&key) {
            return Err(limit("folders: more than 4096 files"));
        }
        self.folder_files.insert(key);
        Ok(target)
    }
}

impl<H: PluginHost> folders::Host for State<H> {
    fn read(&mut self, grant: String, path: String, offset: u64, len: u32) -> std::result::Result<Vec<u8>, PluginError> {
        let target = self.folder_path(&grant, &path)?;
        if len == 0 || len as usize > MAX_IMPORT_CHUNK_BYTES {
            return Err(self.fail(limit("folders.read chunk (1 byte to 1 MiB)")));
        }
        let read = (|| -> std::io::Result<Vec<u8>> {
            use std::io::{Seek, SeekFrom};
            let folder = self.input_folders.get(&grant).ok_or_else(|| std::io::Error::other("folder disappeared"))?;
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            options.custom_flags(O_NONBLOCK);
            let mut file = folder.dir.open_with(&target, &options)?;
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("only regular files are read"));
            }
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = Vec::with_capacity(len as usize);
            file.take(u64::from(len)).read_to_end(&mut bytes)?;
            Ok(bytes)
        })();
        let bytes = read.map_err(|e| self.fail(HostError::PermissionDenied { permission: format!("folders:{grant}: {e}") }))?;
        self.folder_bytes = self.folder_bytes.saturating_add(bytes.len() as u64);
        if self.folder_bytes.saturating_add(self.import.as_ref().map_or(0, |i| i.read_bytes)) > MAX_IMPORT_INPUT_BYTES {
            return Err(self.fail(limit("source and folders: more than 1 GiB read in one run")));
        }
        Ok(bytes)
    }

    fn size(&mut self, grant: String, path: String) -> std::result::Result<Option<u64>, PluginError> {
        let target = self.folder_path(&grant, &path)?;
        let folder = self.input_folders.get(&grant).ok_or_else(|| PluginError::Trap("folder disappeared".into()))?;
        match folder.dir.metadata(&target) {
            Ok(metadata) => Ok(metadata.is_file().then_some(metadata.len())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(self.fail(HostError::PermissionDenied { permission: format!("folders:{grant}: {error}") })),
        }
    }

    fn resolve(&mut self, grant: String, path: String) -> std::result::Result<Option<String>, PluginError> {
        let target = self.folder_path(&grant, &path)?;
        if path.is_empty() {
            return Ok(Some(String::new()));
        }
        let folder = self.input_folders.get(&grant).ok_or_else(|| PluginError::Trap("folder disappeared".into()))?;
        // A disk that ignores case opens a file under any spelling, so the listing decides how the
        // folder spells it. The path as typed is the answer only when the listing cannot tell.
        let typed = folder.dir.metadata(&target).is_ok_and(|m| m.is_file()).then(|| target.to_string_lossy().replace('\\', "/"));
        let (Some(parent), Some(typed_name)) = (target.parent(), target.file_name().map(|n| n.to_string_lossy().into_owned())) else {
            return Ok(typed);
        };
        let wanted = typed_name.to_lowercase();
        let listing = if parent.as_os_str().is_empty() { folder.dir.entries() } else { folder.dir.read_dir(parent) };
        let Ok(listing) = listing else { return Ok(typed) };
        let mut found = None;
        let mut matches = 0;
        for (scanned, entry) in listing.flatten().enumerate() {
            if scanned >= MAX_IMPORT_FILES {
                return Ok(typed);
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.to_lowercase() != wanted || !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if name == typed_name {
                return Ok(Some(target.to_string_lossy().replace('\\', "/")));
            }
            matches += 1;
            found = Some(parent.join(name));
        }
        let Some(found) = found.filter(|_| matches == 1) else { return Ok(typed) };
        // Recheck the name actually returned, including links and the shared file budget. A typed
        // path that opens the file stays the answer when the folder's spelling is not admitted.
        let found = found.to_string_lossy().replace('\\', "/");
        match self.admit_folder_path(&grant, &found) {
            Ok(_) => Ok(Some(found)),
            Err(_) if typed.is_some() => Ok(typed),
            Err(error) => Err(self.fail(error)),
        }
    }
}
