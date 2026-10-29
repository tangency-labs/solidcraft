//! Fonts for sketch text: the embedded default (Liberation Sans), a TrueType/OpenType file
//! named by its path, or an installed font named by its family or full name, looked up in the
//! system's and the user's font folders (plus `SOLIDCRAFT_FONT_DIRS`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::model::{Result, SketchError};

static DEFAULT_FONT: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/liberation-sans/LiberationSans-Regular.ttf"));

/// Family name of the built-in font (also what an empty or absent font name means).
pub const DEFAULT_FONT_FAMILY: &str = "Liberation Sans";
/// Largest font file read.
const MAX_FONT_BYTES: u64 = 64 << 20;
/// Most font files looked at when searching the font folders.
const MAX_FONT_FILES: usize = 20_000;
/// Deepest folder level searched below a font folder.
const MAX_DIR_DEPTH: usize = 6;
/// Font files kept in memory once read, and the most bytes they may add up to.
const MAX_CACHED_FONTS: usize = 16;
const MAX_CACHED_BYTES: usize = 128 << 20;
/// Longest font name or path accepted.
const MAX_FONT_NAME: usize = 1024;

/// Asked before a font file named by its path is read. A design names such files, and an
/// agent's folders (`vfs::PathScope`) live above this crate; the first check set stays.
static FILE_CHECK: OnceLock<fn(&Path) -> std::result::Result<(), String>> = OnceLock::new();

/// Set the check [`font_data`] runs on a font file's path before reading it.
pub fn set_font_file_check(check: fn(&Path) -> std::result::Result<(), String>) {
    let _ = FILE_CHECK.set(check);
}

/// The bytes of a font file and the face to use in it (collections hold several faces).
#[derive(Clone)]
pub struct FontData {
    bytes: FontBytes,
    pub index: u32,
    /// The weight a variable face is set to (its `wght` axis), when not its default.
    pub weight: Option<f32>,
}

#[derive(Clone)]
enum FontBytes {
    Builtin,
    File(Arc<Vec<u8>>),
}

impl FontData {
    pub fn bytes(&self) -> &[u8] {
        match &self.bytes {
            FontBytes::Builtin => DEFAULT_FONT,
            FontBytes::File(b) => b.as_slice(),
        }
    }
}

/// The font a text names: `None`, an empty name or the built-in family give the built-in
/// font; a name with a path separator or a font file extension is a file (`~` and `%VAR%` are
/// expanded); anything else is the family (or full) name of an installed font.
pub fn font_data(name: Option<&str>) -> Result<FontData> {
    let name = name.map(str::trim).unwrap_or("");
    if name.is_empty() || normalize(name) == normalize(DEFAULT_FONT_FAMILY) {
        return Ok(FontData { bytes: FontBytes::Builtin, index: 0, weight: None });
    }
    if name.len() > MAX_FONT_NAME {
        return Err(SketchError::Invalid("the font name is too long".into()));
    }
    if is_path(name) {
        let path = expand(name);
        // Before the cache too: a file read once for the user is not the agent's to read.
        if let Some(check) = FILE_CHECK.get() {
            check(&path).map_err(|e| SketchError::Invalid(format!("font file `{name}`: {e}")))?;
        }
        let bytes = read_font(&path).map_err(|e| SketchError::Invalid(format!("font file `{name}`: {e}")))?;
        let index = 0;
        ttf_parser::Face::parse(&bytes, index)
            .map_err(|e| SketchError::Invalid(format!("font file `{name}` is not a TrueType/OpenType font: {e}")))?;
        return Ok(FontData { bytes: FontBytes::File(bytes), index, weight: None });
    }
    find_family_in(&font_dirs(), name)?
        .ok_or_else(|| SketchError::Invalid(format!("unknown font `{name}`: not an installed font family, and not a .ttf/.otf file path")))
}

/// The face of a font for a style: the bold and/or italic face of an installed family (or the
/// file or built-in font itself). True with it: the font has no italic face, so the text is to
/// be slanted instead. A variable face without a bold face of its own is set to Bold on its
/// weight axis. A bold style the font does not have is an error (bold is not made up).
pub fn font_data_styled(name: Option<&str>, bold: bool, italic: bool) -> Result<(FontData, bool)> {
    let trimmed = name.map(str::trim).unwrap_or("");
    let installed = !(trimmed.is_empty() || normalize(trimmed) == normalize(DEFAULT_FONT_FAMILY) || is_path(trimmed));
    if !installed || !(bold || italic) {
        let mut data = font_data(name)?;
        let (face_bold, face_italic) = ttf_parser::Face::parse(data.bytes(), data.index)
            .map(|face| (face.is_bold(), face.is_italic() || face.is_oblique()))
            .map_err(|e| SketchError::Invalid(format!("font: {e}")))?;
        if bold && !face_bold {
            if let Some(weight) = variable_bold(&data) {
                data.weight = Some(weight);
                return Ok((data, italic && !face_italic));
            }
            let which = if trimmed.is_empty() || normalize(trimmed) == normalize(DEFAULT_FONT_FAMILY) {
                "the built-in font".to_string()
            } else {
                format!("the font file `{trimmed}`")
            };
            return Err(SketchError::Invalid(format!("{which} has no bold face: choose an installed font, or its bold file")));
        }
        return Ok((data, italic && !face_italic));
    }
    if trimmed.len() > MAX_FONT_NAME {
        return Err(SketchError::Invalid("the font name is too long".into()));
    }
    let Some((mut data, face_bold, face_italic)) = find_style_in(&font_dirs(), trimmed, bold, italic)? else {
        return Err(SketchError::Invalid(format!("unknown font `{trimmed}`: not an installed font family, and not a .ttf/.otf file path")));
    };
    if bold && !face_bold {
        data.weight = Some(variable_bold(&data).ok_or_else(|| SketchError::Invalid(format!("the font `{trimmed}` has no bold face")))?);
    }
    Ok((data, italic && !face_italic))
}

/// Bold on a variable face's weight axis: 700 (the weight fonts name Bold), within the axis, when
/// the axis reaches past semibold from a lighter default. None for a face with no such axis.
fn variable_bold(data: &FontData) -> Option<f32> {
    let face = ttf_parser::Face::parse(data.bytes(), data.index).ok()?;
    let axis = face.variation_axes().into_iter().find(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wght"))?;
    (axis.def_value < 600.0 && axis.max_value >= 600.0).then(|| 700_f32.clamp(axis.min_value, axis.max_value))
}

/// An installed face whose family, full or PostScript name is `name`, searched in `dirs`.
pub fn find_family_in(dirs: &[PathBuf], name: &str) -> Result<Option<FontData>> {
    Ok(find_style_in(dirs, name, false, false)?.map(|(data, _, _)| data))
}

/// The installed face of `name` (a family, full or PostScript name) nearest to the style, in
/// `dirs`, with whether it is bold and italic.
pub fn find_style_in(dirs: &[PathBuf], name: &str, bold: bool, italic: bool) -> Result<Option<(FontData, bool, bool)>> {
    let want = normalize(name);
    if want.is_empty() {
        return Ok(None);
    }
    let files = font_files(dirs);
    // Files whose name resembles the family come first (arial.ttf for Arial, times.ttf for
    // Times New Roman): most lookups end there without opening every installed font.
    let (likely, rest): (Vec<&PathBuf>, Vec<&PathBuf>) = files.iter().partition(|p| {
        let stem = p.file_stem().map(|s| normalize(&s.to_string_lossy())).unwrap_or_default();
        stem.len() >= 3 && (stem.contains(&want) || want.contains(&stem))
    });
    for group in [likely, rest] {
        let mut best: Option<(u32, PathBuf, u32, bool, bool)> = None;
        for path in group {
            for face in face_names(path).iter() {
                let score = face.score(&want, bold, italic);
                if score > 0 && best.as_ref().is_none_or(|b| score > b.0) {
                    best = Some((score, path.clone(), face.index, face.bold, face.italic));
                }
            }
            if best.as_ref().is_some_and(|b| b.0 >= FULL_NAME_SCORE) {
                break;
            }
        }
        if let Some((_, path, index, face_bold, face_italic)) = best {
            let bytes = read_font(&path).map_err(|e| SketchError::Invalid(format!("font file `{}`: {e}", path.display())))?;
            return Ok(Some((FontData { bytes: FontBytes::File(bytes), index, weight: None }, face_bold, face_italic)));
        }
    }
    Ok(None)
}

/// The family names of the installed fonts, for choosing one by name: sorted, without repeats
/// and without the built-in family. Only each file's name table is read.
pub fn installed_families() -> Vec<String> {
    families_in(&font_dirs())
}

/// The family names of the fonts under `dirs` (see [`installed_families`]).
pub fn families_in(dirs: &[PathBuf]) -> Vec<String> {
    let mut seen = std::collections::HashSet::from([normalize(DEFAULT_FONT_FAMILY)]);
    let mut out = Vec::new();
    for path in font_files(dirs) {
        for family in file_families(&path) {
            let key = normalize(&family);
            // Names starting with a dot are a system's private fonts.
            if !key.is_empty() && !family.starts_with('.') && seen.insert(key) {
                out.push(family);
            }
        }
    }
    out.sort_by_cached_key(|f| f.to_lowercase());
    out
}

/// Largest name table read when listing families.
const MAX_NAME_TABLE: u32 = 1 << 20;

/// The family names of each face of a font file, from its name tables alone: the file's table
/// directory says where they are, so a large font costs a few small reads, not its whole size.
fn file_families(path: &Path) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return Vec::new() };
    let mut read_at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = vec![0; len];
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let be16 = |b: &[u8], at: usize| b.get(at..at + 2).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes);
    let be32 = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes);
    let Some(head) = read_at(0, 12) else { return Vec::new() };
    let faces: Vec<u32> = if head.starts_with(b"ttcf") {
        let count = be32(&head, 8).unwrap_or(0).min(64) as usize;
        read_at(12, count * 4).map(|b| (0..count).filter_map(|i| be32(&b, i * 4)).collect()).unwrap_or_default()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for face in faces {
        let Some(dir) = read_at(u64::from(face), 12) else { continue };
        let tables = usize::from(be16(&dir, 4).unwrap_or(0).min(512));
        let Some(records) = read_at(u64::from(face) + 12, tables * 16) else { continue };
        let Some(record) = (0..tables).map(|i| i * 16).find(|&r| records.get(r..r + 4) == Some(b"name")) else { continue };
        let (Some(offset), Some(len)) = (be32(&records, record + 8), be32(&records, record + 12)) else { continue };
        if len == 0 || len > MAX_NAME_TABLE {
            continue;
        }
        let Some(data) = read_at(u64::from(offset), len as usize) else { continue };
        if let Some(family) = ttf_parser::name::Table::parse(&data).and_then(|table| family_name(&table)) {
            out.push(family);
        }
    }
    out
}

/// A face's family name as people know it: the English one when there is one.
fn family_name(table: &ttf_parser::name::Table) -> Option<String> {
    use ttf_parser::name_id;
    let mut best: Option<(u8, String)> = None;
    for name in table.names {
        let rank = match name.name_id {
            name_id::FAMILY => 2,
            name_id::TYPOGRAPHIC_FAMILY => 0,
            _ => continue,
        } + u8::from(name.language() == ttf_parser::Language::English_UnitedStates);
        let Some(text) = name.to_string() else { continue };
        let text = text.trim().to_string();
        if !text.is_empty() && best.as_ref().is_none_or(|(b, _)| rank > *b) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, name)| name)
}

/// The names of one face of a font file (normalised), for matching.
struct FaceNames {
    index: u32,
    bold: bool,
    italic: bool,
    names: Vec<(u16, String)>,
}

/// A face named by its full or PostScript name: that face, whatever the style asked for.
const FULL_NAME_SCORE: u32 = 40;

impl FaceNames {
    /// How well the face answers to a (normalised) name in a style: its full or PostScript
    /// name in that style, else its family's face of that style, else (lower) another face of
    /// the family, bold matching first, else its full name in another style (a regular face's
    /// full name is often the family's: "Arial" is not the bold Arial); 0 no match.
    fn score(&self, want: &str, bold: bool, italic: bool) -> u32 {
        use ttf_parser::name_id;
        let style =
            if self.bold == bold && self.italic == italic { 30 } else { 20 + 2 * u32::from(self.bold == bold) + u32::from(self.italic == italic) };
        self.names
            .iter()
            .filter(|(_, n)| n == want)
            .map(|(id, _)| match *id {
                name_id::FULL_NAME | name_id::POST_SCRIPT_NAME if self.bold == bold && self.italic == italic => FULL_NAME_SCORE,
                name_id::FULL_NAME | name_id::POST_SCRIPT_NAME => 10,
                name_id::FAMILY | name_id::TYPOGRAPHIC_FAMILY => style,
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }
}

/// The faces' names of a font file, read once per process (a lookup that finds nothing reads
/// every installed font once, not on every try).
fn face_names(path: &Path) -> Arc<Vec<FaceNames>> {
    static NAMES: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<FaceNames>>>>> = OnceLock::new();
    let cache = NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(c) = cache.lock()
        && let Some(n) = c.get(path)
    {
        return n.clone();
    }
    let mut faces = Vec::new();
    if let Ok(bytes) = read_file(path) {
        let count = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1).min(64);
        for index in 0..count {
            let Ok(face) = ttf_parser::Face::parse(&bytes, index) else { continue };
            let names = face
                .names()
                .into_iter()
                .filter_map(|n| {
                    let text = match n.to_string() {
                        Some(t) => t,
                        // Macintosh Roman names: their ASCII part.
                        None if n.platform_id == ttf_parser::PlatformId::Macintosh => {
                            n.name.iter().filter(|b| b.is_ascii()).map(|b| *b as char).collect()
                        }
                        None => return None,
                    };
                    Some((n.name_id, normalize(&text)))
                })
                .collect();
            faces.push(FaceNames { index, bold: face.is_bold(), italic: face.is_italic() || face.is_oblique(), names });
        }
    }
    let faces = Arc::new(faces);
    if let Ok(mut c) = cache.lock()
        && c.len() < MAX_FONT_FILES
    {
        c.insert(path.to_path_buf(), faces.clone());
    }
    faces
}

/// Lower-case letters and digits only: "DejaVu_Sans", "Dejavu Sans" and "dejavu-sans" are one name.
fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn is_path(name: &str) -> bool {
    name.contains('/') || name.contains('\\') || has_font_extension(name)
}

fn has_font_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".ttf", ".otf", ".ttc", ".otc"].iter().any(|e| lower.ends_with(e))
}

/// `~` at the start and `%VAR%` replaced from the environment.
fn expand(name: &str) -> PathBuf {
    let mut s = name.to_string();
    if let Some(rest) = s.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
        && let Some(home) = home()
    {
        s = format!("{}{rest}", home.display());
    }
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some((head, tail)) = rest.split_once('%') {
        out.push_str(head);
        match tail.split_once('%') {
            // A variable that is not set stays as written.
            Some((var, after)) if !var.is_empty() && !var.contains(['=', '\0']) => {
                match std::env::var(var) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = after;
            }
            _ => {
                out.push('%');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    PathBuf::from(out)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// The folders searched for installed fonts, in order.
pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("SOLIDCRAFT_FONT_DIRS").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        dirs.push(env("WINDIR").or_else(|| env("SystemRoot")).unwrap_or_else(|| PathBuf::from("C:\\Windows")).join("Fonts"));
        if let Some(local) = env("LOCALAPPDATA") {
            dirs.push(local.join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(h) = home() {
            dirs.push(h.join("Library").join("Fonts"));
        }
    } else {
        if let Some(d) = env("XDG_DATA_HOME") {
            dirs.push(d.join("fonts"));
        }
        if let Some(h) = home() {
            dirs.push(h.join(".local").join("share").join("fonts"));
            dirs.push(h.join(".fonts"));
        }
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        dirs.push(PathBuf::from("/usr/share/fonts"));
        #[cfg(target_os = "linux")]
        {
            dirs.push(PathBuf::from("/run/host/fonts"));
            dirs.push(PathBuf::from("/run/host/user-fonts"));
        }
    }
    dirs
}

/// TrueType/OpenType files under `dirs` (bounded in depth and count).
fn font_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo: Vec<(PathBuf, usize)> = dirs.iter().rev().map(|d| (d.clone(), 0)).collect();
    while let Some((dir, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut sub = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if depth < MAX_DIR_DEPTH {
                    sub.push((path, depth + 1));
                }
            } else if has_font_extension(&path.to_string_lossy()) {
                out.push(path);
                if out.len() >= MAX_FONT_FILES {
                    return out;
                }
            }
        }
        sub.sort();
        todo.extend(sub.into_iter().rev());
    }
    out
}

/// A font file's bytes (bounded in size).
fn read_file(path: &Path) -> std::result::Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_FONT_BYTES {
        return Err("the file is too large for a font".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

/// A font file's bytes, from the cache when it was read before.
fn read_font(path: &Path) -> std::result::Result<Arc<Vec<u8>>, String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(c) = cache.lock()
        && let Some(b) = c.get(path)
    {
        return Ok(b.clone());
    }
    let bytes = Arc::new(read_file(path)?);
    if let Ok(mut c) = cache.lock() {
        if c.len() >= MAX_CACHED_FONTS || c.values().map(|b| b.len()).sum::<usize>() + bytes.len() > MAX_CACHED_BYTES {
            c.clear();
        }
        c.insert(path.to_path_buf(), bytes.clone());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttf_parser::name_id;

    #[cfg(target_os = "linux")]
    #[test]
    fn font_dirs_include_flatpak_host_fonts() {
        let dirs = font_dirs();
        for dir in ["/run/host/fonts", "/run/host/user-fonts"] {
            assert!(dirs.contains(&PathBuf::from(dir)), "missing {dir}");
        }
    }

    /// A face of family "Arial" whose full name is `full`.
    fn face(full: &str, bold: bool, italic: bool) -> FaceNames {
        FaceNames { index: 0, bold, italic, names: vec![(name_id::FAMILY, normalize("Arial")), (name_id::FULL_NAME, normalize(full))] }
    }

    #[test]
    fn a_style_picks_its_face_even_when_the_regular_face_is_named_like_the_family() {
        // As installed: the regular face's full name is the family's own.
        let faces =
            [face("Arial", false, false), face("Arial Bold", true, false), face("Arial Italic", false, true), face("Arial Bold Italic", true, true)];
        let best = |bold: bool, italic: bool| {
            let scores: Vec<u32> = faces.iter().map(|f| f.score("arial", bold, italic)).collect();
            let top = scores.iter().copied().max().unwrap_or(0);
            faces.iter().zip(&scores).find(|(_, s)| **s == top).map(|(f, _)| (f.bold, f.italic))
        };
        for style in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(best(style.0, style.1), Some(style), "{style:?}");
        }
        // A full name still finds its face, whatever style is asked (it is the only one by that name).
        assert!(face("Arial Bold", true, false).score("arialbold", false, false) > 0);
        assert_eq!(face("Arial", false, false).score("times", false, false), 0);
    }
}
