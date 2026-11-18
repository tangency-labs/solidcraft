//! A board's fabrication files as one set (#12797): a zip, a folder, or a file in a folder, as
//! a board maker gets them. Each file's layer is worked out from what it says about itself (a
//! Gerber X2 `TF.FileFunction`, an Excellon header), then from the job file (`.gbrjob`), then
//! from the usual file names and extensions (`.GTL`, `-F_Cu.gbr`, `TopLayer`...). Files that are
//! neither Gerber nor Excellon are left out.

use std::io::Read as _;

use serde_json::Value;

use crate::sketch2d::MAX_DRAWING_BYTES;
use crate::vfs;

/// Most files a set is read from (a folder may hold other files too; they are counted).
pub const MAX_SET_FILES: usize = 256;
/// Most bytes all the set's files hold together.
pub const MAX_SET_BYTES: usize = 512 << 20;

/// What a file in the set is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetRole {
    Outline,
    Drill,
    TopCopper,
    BottomCopper,
    TopMask,
    BottomMask,
    TopSilk,
    BottomSilk,
    /// A Gerber or Excellon file the import doesn't use (paste, inner copper, drawings...).
    Other,
}

impl SetRole {
    /// The layer's key in `file.insert_gerber`'s `layers`, for the six image layers.
    pub fn layer_key(self) -> Option<&'static str> {
        Some(match self {
            SetRole::TopCopper => "top_copper",
            SetRole::BottomCopper => "bottom_copper",
            SetRole::TopMask => "top_mask",
            SetRole::BottomMask => "bottom_mask",
            SetRole::TopSilk => "top_silk",
            SetRole::BottomSilk => "bottom_silk",
            _ => return None,
        })
    }

    /// The role's name in replies: `outline`, `drill`, a layer key, or `other`.
    pub fn key(self) -> &'static str {
        match self {
            SetRole::Outline => "outline",
            SetRole::Drill => "drill",
            SetRole::Other => "other",
            r => r.layer_key().unwrap_or("other"),
        }
    }
}

/// One Gerber or Excellon file of the set.
#[derive(Clone, Debug)]
pub struct SetFile {
    /// Its name in the set: the file name in a folder, the path inside a zip.
    pub name: String,
    pub role: SetRole,
    pub text: String,
}

/// What the set's job file (`.gbrjob`) says about the board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardJob {
    /// mm.
    pub thickness: Option<f64>,
    /// The surface finish as written (`ENIG`, `HAL SnPb`, `None`...).
    pub finish: Option<String>,
}

/// A board's files.
#[derive(Clone, Debug, Default)]
pub struct GerberSet {
    /// What was picked: the zip, or the folder.
    pub source: String,
    /// Each layer is given to at most one file; any more of the same layer are `Other`, with a
    /// warning. Drills may be many.
    pub files: Vec<SetFile>,
    pub job: Option<BoardJob>,
    pub warnings: Vec<String>,
}

impl GerberSet {
    /// The file with this name.
    pub fn file(&self, name: &str) -> Option<&SetFile> {
        self.files.iter().find(|f| f.name == name)
    }

    /// The (first) file with this role.
    pub fn with_role(&self, role: SetRole) -> Option<&SetFile> {
        self.files.iter().find(|f| f.role == role)
    }
}

/// The extensions a board's files are picked by: a zip, the Gerber layers by their names
/// (Protel-style and Eagle's), Gerber's own, the drill files' and the job file's. Any file in the
/// board's folder will do, whatever the picker shows.
pub const BOARD_EXTENSIONS: [&str; 26] = [
    "zip", "gbr", "ger", "pho", "art", "gtl", "gbl", "gts", "gbs", "gto", "gbo", "gko", "gm1", "gml", "cmp", "sol", "stc", "sts", "plc", "pls",
    "dim", "drl", "xln", "exc", "txt", "gbrjob",
];

/// Whether a path names a zip, by its extension.
pub fn is_zip_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Read a board's files from `path`: a zip, a folder, or any file in the board's folder (the
/// folder is read).
pub fn read_gerber_set(path: &str) -> std::io::Result<GerberSet> {
    if is_zip_path(path) {
        return Ok(classify_set(path.to_string(), zip_files(path)?));
    }
    let dir = if vfs::is_file(path) {
        std::path::Path::new(path).parent().map(|d| d.to_string_lossy().into_owned()).filter(|d| !d.is_empty()).unwrap_or_else(|| ".".into())
    } else {
        path.to_string()
    };
    let files = folder_files(&dir)?;
    Ok(classify_set(dir, files))
}

/// Work out each file's role: `files` are (name, bytes).
pub fn classify_set(source: String, files: Vec<(String, Vec<u8>)>) -> GerberSet {
    let mut set = GerberSet { source, ..GerberSet::default() };
    // The job file first: it names the other files' layers.
    let mut job_roles: Vec<(String, SetRole)> = Vec::new();
    for (name, bytes) in &files {
        if ext_of(name) == "gbrjob"
            && let Ok(v) = serde_json::from_slice::<Value>(bytes)
        {
            let (job, roles) = read_job(&v);
            if set.job.is_none() {
                set.job = Some(job);
            }
            job_roles.extend(roles);
        }
    }
    for (name, bytes) in files {
        if ext_of(&name) == "gbrjob" || bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let Some(kind) = content_kind(&text) else { continue };
        let base = base_name(&name);
        let role = match kind {
            Content::Excellon => SetRole::Drill,
            Content::Gerber(Some(role)) => role,
            Content::Gerber(None) => job_roles.iter().find(|(p, _)| base_name(p) == base).map(|(_, r)| *r).unwrap_or_else(|| name_role(&base)),
        };
        set.files.push(SetFile { name, role, text });
    }
    // One file per layer: an outline drawn on a layer of its own wins over a mechanical layer.
    for role in
        [SetRole::Outline, SetRole::TopCopper, SetRole::BottomCopper, SetRole::TopMask, SetRole::BottomMask, SetRole::TopSilk, SetRole::BottomSilk]
    {
        let mut seen: Option<String> = None;
        for f in set.files.iter_mut().filter(|f| f.role == role) {
            match &seen {
                None => seen = Some(f.name.clone()),
                Some(first) => {
                    set.warnings.push(format!(
                        "{} and {} both look like the {} layer; {} is used",
                        first,
                        f.name,
                        role.key().replace('_', " "),
                        first
                    ));
                    f.role = SetRole::Other;
                }
            }
        }
    }
    set
}

/// What a file's text is.
enum Content {
    Excellon,
    /// A Gerber file, with the layer its `TF.FileFunction` gives, when it gives one.
    Gerber(Option<SetRole>),
}

fn content_kind(text: &str) -> Option<Content> {
    let head: String = text.chars().take(8192).collect();
    // First: an Excellon header carries X2 attributes as comments too (`; #@! TF.FileFunction,Plated,1,2,PTH`).
    if head.lines().any(|l| l.trim() == "M48") {
        return Some(Content::Excellon);
    }
    if let Some(f) = head.lines().find_map(|l| l.split_once("TF.FileFunction,").map(|(_, r)| r)) {
        return Some(Content::Gerber(Some(function_role(f.trim_end_matches(['*', '%'])))));
    }
    if head.contains("%FS") || head.contains("%MO") || head.contains("%ADD") {
        return Some(Content::Gerber(None));
    }
    None
}

/// The role a Gerber X2 file function gives (`Copper,L1,Top`, `SolderMask,Bot`, `Profile,NP`).
fn function_role(f: &str) -> SetRole {
    let parts: Vec<&str> = f.split(',').map(str::trim).collect();
    let side = |i: usize| parts.get(i).copied();
    match parts.first().copied() {
        Some("Profile") => SetRole::Outline,
        Some("Copper") => match side(2) {
            Some("Top") => SetRole::TopCopper,
            Some("Bot") => SetRole::BottomCopper,
            _ => SetRole::Other,
        },
        Some("Soldermask" | "SolderMask") => match side(1) {
            Some("Top") => SetRole::TopMask,
            Some("Bot") => SetRole::BottomMask,
            _ => SetRole::Other,
        },
        Some("Legend") => match side(1) {
            Some("Top") => SetRole::TopSilk,
            Some("Bot") => SetRole::BottomSilk,
            _ => SetRole::Other,
        },
        _ => SetRole::Other,
    }
}

/// The board's thickness and finish, and the layers the job file names, by file.
fn read_job(v: &Value) -> (BoardJob, Vec<(String, SetRole)>) {
    let specs = v.get("GeneralSpecs");
    let thickness = specs.and_then(|s| s.get("BoardThickness")).and_then(Value::as_f64).filter(|t| t.is_finite() && *t > 0.0 && *t <= 20.0);
    let finish = specs.and_then(|s| s.get("Finish")).and_then(Value::as_str).map(|f| f.chars().take(64).collect());
    let roles = v
        .get("FilesAttributes")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .take(MAX_SET_FILES)
                .filter_map(|f| Some((f.get("Path")?.as_str()?.to_string(), function_role(f.get("FileFunction")?.as_str()?))))
                .collect()
        })
        .unwrap_or_default();
    (BoardJob { thickness, finish }, roles)
}

/// The role a file's name gives, lower case: its extension (`.gtl`, Eagle's `.cmp`), else the
/// layer names KiCad, EasyEDA and others put in it.
fn name_role(base: &str) -> SetRole {
    let role = match ext_of(base).as_str() {
        "gtl" | "cmp" => SetRole::TopCopper,
        "gbl" | "sol" => SetRole::BottomCopper,
        "gts" | "stc" => SetRole::TopMask,
        "gbs" | "sts" => SetRole::BottomMask,
        "gto" | "plc" => SetRole::TopSilk,
        "gbo" | "pls" => SetRole::BottomSilk,
        "gko" | "gm1" | "gml" | "dim" => SetRole::Outline,
        _ => SetRole::Other,
    };
    if role != SetRole::Other {
        return role;
    }
    let n: String = base.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["paste", "inner", "in1cu", "in2cu", "fab", "courtyard", "drawing", "comment", "assembly"]) {
        SetRole::Other
    } else if has(&["edgecuts", "boardoutline", "outline", "profile"]) {
        SetRole::Outline
    } else if has(&["fmask", "topsoldermask", "topmask", "soldermasktop", "masktop"]) {
        SetRole::TopMask
    } else if has(&["bmask", "bottomsoldermask", "bottommask", "soldermaskbottom", "maskbottom"]) {
        SetRole::BottomMask
    } else if has(&["fsilk", "topsilk", "silktop", "topoverlay", "legendtop", "toplegend"]) {
        SetRole::TopSilk
    } else if has(&["bsilk", "bottomsilk", "silkbottom", "bottomoverlay", "legendbottom", "bottomlegend"]) {
        SetRole::BottomSilk
    } else if has(&["fcu", "toplayer", "topcopper", "coppertop"]) {
        SetRole::TopCopper
    } else if has(&["bcu", "bottomlayer", "bottomcopper", "copperbottom"]) {
        SetRole::BottomCopper
    } else {
        SetRole::Other
    }
}

/// The file name alone, lower case.
fn base_name(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or(name).to_lowercase()
}

fn ext_of(name: &str) -> String {
    base_name(name).rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default()
}

/// Most files a folder may hold for its board's set to be looked for in it.
const MAX_FOLDER_FILES: usize = 10_000;

/// Extensions that are never a board's fabrication file, left unread in a folder (a board's
/// files picked in Downloads sit among all sorts).
const NOT_FABRICATION: [&str; 31] = [
    "7z", "avi", "bmp", "dll", "doc", "docx", "exe", "gif", "gz", "heic", "html", "iso", "jpeg", "jpg", "mkv", "mov", "mp3", "mp4", "msi", "pdf",
    "png", "ppt", "pptx", "rar", "step", "stl", "stp", "webp", "xls", "xlsx", "zip",
];

/// Whether a file read from a folder is one [`classify_set`] keeps: the job file, or text that
/// reads as a Gerber or drill file.
fn is_board_file(name: &str, bytes: &[u8]) -> bool {
    if ext_of(name) == "gbrjob" {
        return true;
    }
    // `content_kind` looks at the first 8192 characters, at most four bytes each.
    let head = bytes.get(..32_768).unwrap_or(bytes);
    !bytes.contains(&0) && content_kind(&String::from_utf8_lossy(head)).is_some()
}

/// The board files in a folder, each at most [`MAX_DRAWING_BYTES`] (larger ones are left out):
/// at most [`MAX_SET_FILES`] of them and [`MAX_SET_BYTES`] in all. Files that are not a board's
/// do not count, so that a board's files among many others are still found.
fn folder_files(dir: &str) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut total = 0usize;
    for path in vfs::files_in(dir, MAX_FOLDER_FILES)? {
        if out.len() >= MAX_SET_FILES {
            break;
        }
        if NOT_FABRICATION.contains(&ext_of(&path).as_str()) {
            continue;
        }
        let Ok(bytes) = vfs::read(&path, MAX_DRAWING_BYTES) else { continue };
        if !is_board_file(&path, &bytes) {
            continue;
        }
        if total.saturating_add(bytes.len()) > MAX_SET_BYTES {
            continue;
        }
        total = total.saturating_add(bytes.len());
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(path);
        out.push((name, bytes));
    }
    Ok(out)
}

/// The files in a zip, each at most [`MAX_DRAWING_BYTES`] whatever the zip says it holds.
fn zip_files(path: &str) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let bytes = vfs::read(path, MAX_SET_BYTES)?;
    zip_entries(&bytes)
}

/// The files in a zip's bytes.
pub fn zip_entries(bytes: &[u8]) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let bad = |e: zip::result::ZipError| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("not a zip that can be read: {e}"));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(bad)?;
    if zip.len() > MAX_SET_FILES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("the zip holds more than {MAX_SET_FILES} files")));
    }
    let mut out = Vec::new();
    let mut total = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(bad)?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().map_err(bad)?.into_owned();
        // Mac zips carry a copy of each file's metadata; it is not a layer.
        if name.starts_with("__MACOSX/") || base_name(&name).starts_with("._") {
            continue;
        }
        let mut buf = Vec::new();
        (&mut entry).take(MAX_DRAWING_BYTES as u64 + 1).read_to_end(&mut buf)?;
        if buf.len() > MAX_DRAWING_BYTES {
            continue;
        }
        total = total.saturating_add(buf.len());
        if total > MAX_SET_BYTES {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the zip's files are too large"));
        }
        out.push((name, buf));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "gerber_set_tests.rs"]
mod tests;
