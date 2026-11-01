//! The installed fonts tried for a script the chosen font lacks: one list per script, used by the
//! interface and by drawings. Each row is a family and the files it is known to be in, in the order
//! they are tried, so a lookup rarely has to read every font on the machine.
//!
//! The search for a character: the known files of its script's list that lie in the font folders;
//! then, in sorted order, the first face in the folders with outlines and the character, read from
//! each file's character map alone and bounded by `MAX_FONT_FILES`. Its answers, "none has it"
//! too, are kept for the process. [`answers`] runs it where it's called, or on a thread of its own.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::{FaceTable, MAX_FONT_BYTES, face_names, face_tables, font_files, normalize};

/// Installed fonts to try, in order: `(family name, known files)`. A family may come back in a
/// later row, when another font is to be tried before that file.
pub type Candidates = &'static [(&'static str, &'static [&'static str])];

/// The known files of `candidates`, in order.
pub fn candidate_files(candidates: Candidates) -> impl Iterator<Item = &'static str> {
    candidates.iter().flat_map(|(_, files)| files.iter().copied())
}

/// Han and kana, for Japanese text and when no language asks otherwise.
pub const JAPANESE: Candidates = &[
    // Windows
    ("Meiryo", &["C:\\Windows\\Fonts\\meiryo.ttc"]),
    ("Yu Gothic", &["C:\\Windows\\Fonts\\YuGothM.ttc", "C:\\Windows\\Fonts\\YuGothR.ttc"]),
    ("MS Gothic", &["C:\\Windows\\Fonts\\msgothic.ttc"]),
    ("BIZ UDGothic", &["C:\\Windows\\Fonts\\BIZ-UDGothicR.ttc"]),
    ("BIZ UDMincho", &["C:\\Windows\\Fonts\\BIZ-UDMinchoM.ttc"]),
    ("Yu Mincho", &["C:\\Windows\\Fonts\\yumin.ttf"]),
    // macOS
    (
        "Hiragino Sans",
        &[
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/Hiragino Sans W3.ttc",
            "/System/Library/Fonts/Hiragino Sans W4.ttc",
            "/System/Library/Fonts/Hiragino Sans W6.ttc",
            "/System/Library/Fonts/Supplemental/Hiragino Sans W3.ttc",
        ],
    ),
    ("Arial Unicode MS", &["/System/Library/Fonts/Supplemental/Arial Unicode.ttf"]),
    ("Songti SC", &["/System/Library/Fonts/Supplemental/Songti.ttc"]),
    ("PingFang SC", &["/System/Library/Fonts/PingFang.ttc"]),
    ("Hiragino Sans GB", &["/System/Library/Fonts/Hiragino Sans GB.ttc"]),
    ("Apple SD Gothic Neo", &["/System/Library/Fonts/AppleSDGothicNeo.ttc"]),
    ("Arial Unicode MS", &["/Library/Fonts/Arial Unicode.ttf"]),
    ("Hiragino Sans", &["/Library/Fonts/Hiragino Sans W3.ttc"]),
    // Linux
    (
        "Noto Sans CJK JP",
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJKjp-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
        ],
    ),
    ("IPAGothic", &["/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf", "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf"]),
    ("TakaoPGothic", &["/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf"]),
    ("TakaoGothic", &["/usr/share/fonts/truetype/takao-gothic/TakaoGothic.ttf"]),
    ("VL PGothic", &["/usr/share/fonts/truetype/vlgothic/VL-PGothic-Regular.ttf"]),
    ("IPAGothic", &["/usr/share/fonts/truetype/ipafont/ipag.ttf"]),
    ("IPAexGothic", &["/usr/share/fonts/truetype/ipaexfont/ipaexg.ttf"]),
    ("IPAGothic", &["/usr/share/fonts/opentype/ipafont/ipag.ttf"]),
    ("IPAexGothic", &["/usr/share/fonts/opentype/ipaexfont/ipaexg.ttf"]),
    ("Droid Sans Fallback", &["/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"]),
    ("AR PL UMing CN", &["/usr/share/fonts/cjkuni-uming/uming.ttc"]),
];

/// Han in traditional Chinese.
pub const TRADITIONAL_CHINESE: Candidates = &[
    // Windows
    ("Microsoft JhengHei", &["C:\\Windows\\Fonts\\msjh.ttc", "C:\\Windows\\Fonts\\msjh.ttf"]),
    // macOS
    ("PingFang TC", &["/System/Library/Fonts/PingFang.ttc"]),
    ("Heiti TC", &["/System/Library/Fonts/STHeiti Light.ttc", "/System/Library/Fonts/STHeiti Medium.ttc"]),
    ("Songti TC", &["/System/Library/Fonts/Supplemental/Songti.ttc"]),
    // Linux
    (
        "Noto Sans CJK TC",
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        ],
    ),
    ("AR PL UMing TW", &["/usr/share/fonts/cjkuni-uming/uming.ttc"]),
];

/// Han in simplified Chinese: many Japanese fonts (Hiragino Gothic, Meiryo, Yu Gothic) lack it.
pub const SIMPLIFIED_CHINESE: Candidates = &[
    // Windows
    ("Microsoft YaHei", &["C:\\Windows\\Fonts\\msyh.ttc", "C:\\Windows\\Fonts\\msyh.ttf"]),
    // macOS
    ("PingFang SC", &["/System/Library/Fonts/PingFang.ttc"]),
    ("Hiragino Sans GB", &["/System/Library/Fonts/Hiragino Sans GB.ttc"]),
    ("Heiti SC", &["/System/Library/Fonts/STHeiti Light.ttc", "/System/Library/Fonts/STHeiti Medium.ttc"]),
    ("Songti SC", &["/System/Library/Fonts/Supplemental/Songti.ttc"]),
    ("Arial Unicode MS", &["/System/Library/Fonts/Supplemental/Arial Unicode.ttf", "/Library/Fonts/Arial Unicode.ttf"]),
    // Linux
    (
        "Noto Sans CJK SC",
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        ],
    ),
    ("Droid Sans Fallback", &["/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"]),
];

/// Hangul: a Japanese CJK font (Meiryo, Hiragino) has none. Noto Sans CJK covers it on Linux.
pub const HANGUL: Candidates = &[
    // Windows
    ("Malgun Gothic", &["C:\\Windows\\Fonts\\malgun.ttf"]),
    // macOS
    ("Apple SD Gothic Neo", &["/System/Library/Fonts/AppleSDGothicNeo.ttc"]),
    ("AppleGothic", &["/System/Library/Fonts/Supplemental/AppleGothic.ttf"]),
    // Linux
    ("NanumGothic", &["/usr/share/fonts/truetype/nanum/NanumGothic.ttf", "/usr/share/fonts/nanum/NanumGothic.ttf"]),
];

/// Thai: without one it shows as empty boxes in the interface (#1902).
pub const THAI: Candidates = &[
    // Windows
    ("Leelawadee UI", &["C:\\Windows\\Fonts\\LeelawUI.ttf"]),
    ("Leelawadee", &["C:\\Windows\\Fonts\\leelawad.ttf"]),
    ("Tahoma", &["C:\\Windows\\Fonts\\tahoma.ttf"]),
    // macOS
    ("Thonburi", &["/System/Library/Fonts/Thonburi.ttc", "/System/Library/Fonts/Supplemental/Thonburi.ttc"]),
    ("Ayuthaya", &["/System/Library/Fonts/Supplemental/Ayuthaya.ttf"]),
    ("Thonburi", &["/Library/Fonts/Thonburi.ttf"]),
    // Linux
    (
        "Noto Sans Thai",
        &[
            "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansThai-Regular.ttf",
            "/usr/share/fonts/google-noto-vf/NotoSansThai[wght].ttf",
            "/usr/share/fonts/opentype/noto/NotoSansThai-Regular.ttf",
        ],
    ),
    ("Loma", &["/usr/share/fonts/truetype/tlwg/Loma.ttf", "/usr/share/fonts/opentype/tlwg/Loma.otf"]),
    ("Garuda", &["/usr/share/fonts/truetype/tlwg/Garuda.ttf", "/usr/share/fonts/opentype/tlwg/Garuda.otf"]),
    ("Loma", &["/usr/share/fonts/tlwg/Loma.otf"]),
];

/// Hebrew: egui's built-in faces have none (#3457).
pub const HEBREW: Candidates = &[
    // Windows
    ("Segoe UI", &["C:\\Windows\\Fonts\\segoeui.ttf"]),
    ("Arial", &["C:\\Windows\\Fonts\\arial.ttf"]),
    // macOS
    ("Arial Hebrew", &["/System/Library/Fonts/ArialHB.ttc"]),
    ("SF Hebrew", &["/System/Library/Fonts/SFHebrew.ttf"]),
    ("Arial", &["/System/Library/Fonts/Supplemental/Arial.ttf"]),
    // Linux
    (
        "Noto Sans Hebrew",
        &[
            "/usr/share/fonts/noto/NotoSansHebrew-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansHebrew-Regular.ttf",
            "/usr/share/fonts/google-noto-vf/NotoSansHebrew[wght].ttf",
            "/usr/share/fonts/opentype/noto/NotoSansHebrew-Regular.ttf",
        ],
    ),
    (
        "DejaVu Sans",
        &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf"],
    ),
];

/// Arabic, with the joining forms a shaper uses. egui's built-in faces have none.
pub const ARABIC: Candidates = &[
    // Windows
    ("Segoe UI", &["C:\\Windows\\Fonts\\segoeui.ttf"]),
    ("Tahoma", &["C:\\Windows\\Fonts\\tahoma.ttf"]),
    ("Arial", &["C:\\Windows\\Fonts\\arial.ttf"]),
    // macOS
    ("SF Arabic", &["/System/Library/Fonts/SFArabic.ttf"]),
    ("Geeza Pro", &["/System/Library/Fonts/GeezaPro.ttc", "/System/Library/Fonts/Supplemental/GeezaPro.ttc"]),
    ("Damascus", &["/System/Library/Fonts/Supplemental/Damascus.ttc"]),
    ("Tahoma", &["/System/Library/Fonts/Supplemental/Tahoma.ttf"]),
    // Linux
    (
        "Noto Sans Arabic",
        &[
            "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/google-noto-vf/NotoSansArabic[wght].ttf",
            "/usr/share/fonts/opentype/noto/NotoSansArabic-Regular.ttf",
        ],
    ),
    ("Noto Naskh Arabic", &["/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf", "/usr/share/fonts/noto/NotoNaskhArabic-Regular.ttf"]),
    ("KacstOne", &["/usr/share/fonts/truetype/kacst-one/KacstOne.ttf"]),
    (
        "DejaVu Sans",
        &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf"],
    ),
];

/// Largest character map read: a full CJK font's is about a megabyte.
const MAX_CMAP: u32 = 8 << 20;
/// Answers kept; past it the table starts again.
const MAX_ANSWERS: usize = 1 << 16;

/// Whose Han a text wants: its forms differ by language, and no one font has them all.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Han {
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
    Korean,
}

impl Han {
    /// The drawing's language when it writes Han (a region or script subtag decides Chinese); else
    /// Japanese when the text has kana, Korean when it has Hangul, else simplified Chinese.
    pub fn of(language: &str, text: &str) -> Han {
        let language = language.to_ascii_lowercase();
        let mut tags = language.split(['-', '_']);
        match tags.next() {
            Some("ja") => Han::Japanese,
            Some("ko") => Han::Korean,
            Some("zh") if tags.any(|tag| matches!(tag, "hant" | "tw" | "hk" | "mo")) => Han::TraditionalChinese,
            Some("zh") => Han::SimplifiedChinese,
            _ if text.chars().any(|c| script(c) == Some(Script::Kana)) => Han::Japanese,
            _ if text.chars().any(|c| script(c) == Some(Script::Hangul)) => Han::Korean,
            _ => Han::SimplifiedChinese,
        }
    }

    fn candidates(self) -> Candidates {
        match self {
            Han::Japanese => JAPANESE,
            Han::SimplifiedChinese => SIMPLIFIED_CHINESE,
            Han::TraditionalChinese => TRADITIONAL_CHINESE,
            Han::Korean => HANGUL,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Han::Japanese => "Japanese",
            Han::SimplifiedChinese => "simplified Chinese",
            Han::TraditionalChinese => "traditional Chinese",
            Han::Korean => "Korean",
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Script {
    Arabic,
    Hebrew,
    Thai,
    Hangul,
    Kana,
    Han,
}

/// The script with a list above that `c` is written in.
fn script(c: char) -> Option<Script> {
    Some(match u32::from(c) {
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x0E00..=0x0E7F => Script::Thai,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Script::Hangul,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Script::Kana,
        0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFF65 | 0x20000..=0x3FFFF => Script::Han,
        _ => return None,
    })
}

/// How a drawing's status names `c`'s script, and the language picked for it when it is Han.
pub(crate) fn script_name(c: char, han: Han) -> (&'static str, Option<&'static str>) {
    match script(c) {
        Some(Script::Arabic) => ("Arabic", None),
        Some(Script::Hebrew) => ("Hebrew", None),
        Some(Script::Thai) => ("Thai", None),
        Some(Script::Hangul) => ("Hangul", None),
        Some(Script::Kana) => ("Kana", None),
        Some(Script::Han) => ("Han", Some(han.name())),
        None => ("Other text", None),
    }
}

/// One question for the search: which face sets `ch`, in the fonts of `dirs`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Query {
    dirs: Vec<PathBuf>,
    han: Han,
    bold: bool,
    italic: bool,
    ch: char,
}

impl Query {
    pub(crate) fn new(dirs: &[PathBuf], han: Han, bold: bool, italic: bool, ch: char) -> Query {
        Query { dirs: dirs.to_vec(), han, bold, italic, ch }
    }
}

/// A face: its file and its index there.
type Answer = Option<(PathBuf, u32)>;

#[derive(Default)]
struct Search {
    answers: HashMap<Query, Answer>,
    asked: HashSet<Query>,
    running: bool,
}

fn state() -> std::sync::MutexGuard<'static, Search> {
    static STATE: OnceLock<Mutex<Search>> = OnceLock::new();
    STATE.get_or_init(Default::default).lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The answer the search has given to `query`, without searching.
pub(crate) fn answered(query: &Query) -> Answer {
    state().answers.get(query).cloned().flatten()
}

/// The answers to `queries`, in order. With `wait` the search runs here; else the ones not known
/// yet go to a thread, and this is None until they are in (a system with no threads waits).
pub(crate) fn answers(queries: &[Query], wait: bool) -> Option<Vec<Answer>> {
    let unknown: Vec<Query> = {
        let mut search = state();
        let unknown: Vec<Query> = queries.iter().filter(|query| !search.answers.contains_key(*query)).cloned().collect();
        if !wait && !unknown.is_empty() {
            search.asked.extend(unknown);
            if !search.running {
                search.running = true;
                drop(search);
                if std::thread::Builder::new().name("font-search".into()).spawn(run_asked).is_err() {
                    run_asked();
                    return known(queries);
                }
            }
            return None;
        }
        unknown
    };
    if !unknown.is_empty() {
        store(search(&unknown));
    }
    known(queries)
}

fn known(queries: &[Query]) -> Option<Vec<Answer>> {
    let search = state();
    queries.iter().map(|query| search.answers.get(query).cloned()).collect()
}

fn store(found: Vec<(Query, Answer)>) {
    let mut search = state();
    if search.answers.len() + found.len() > MAX_ANSWERS {
        search.answers.clear();
    }
    search.answers.extend(found);
}

/// The thread's work: what has been asked, until nothing is left.
fn run_asked() {
    loop {
        let asked: Vec<Query> = {
            let mut search = state();
            if search.asked.is_empty() {
                search.running = false;
                return;
            }
            search.asked.drain().collect()
        };
        let found = search(&asked);
        // Admitted here, so the window doesn't read and hash them when it asks again.
        for (path, index) in found.iter().filter_map(|(_, answer)| answer.as_ref()) {
            crate::paper_text::warm(path, *index);
        }
        store(found);
    }
}

fn search(queries: &[Query]) -> Vec<(Query, Answer)> {
    let mut out = Vec::with_capacity(queries.len());
    let mut rest: HashMap<&[PathBuf], Vec<&Query>> = HashMap::new();
    for query in queries {
        match known_face(query) {
            Some(face) => out.push((query.clone(), Some(face))),
            None => rest.entry(query.dirs.as_slice()).or_default().push(query),
        }
    }
    for (dirs, left) in rest {
        out.extend(last_resort(dirs, left));
    }
    out
}

/// `candidates` for a character: its script's list.
fn candidates(ch: char, han: Han) -> Candidates {
    match script(ch) {
        Some(Script::Arabic) => ARABIC,
        Some(Script::Hebrew) => HEBREW,
        Some(Script::Thai) => THAI,
        Some(Script::Hangul) => HANGUL,
        Some(Script::Kana) => JAPANESE,
        Some(Script::Han) => han.candidates(),
        None => &[],
    }
}

/// The first of the list's known files, lying in the query's folders, with the character: the
/// face of the family in the style asked for, or else the file's first.
fn known_face(query: &Query) -> Option<(PathBuf, u32)> {
    for (family, files) in candidates(query.ch, query.han) {
        let want = normalize(family);
        for path in files.iter().filter_map(|file| located(&query.dirs, file)) {
            let faces = face_names(&path);
            let Some(face) = faces.iter().rev().max_by_key(|face| face.score(&want, query.bold, query.italic)) else { continue };
            let tables = face_tables(&path, b"cmap", MAX_CMAP);
            if tables.iter().any(|table| table.index == face.index && sets(table, query.ch)) {
                return Some((path, face.index));
            }
        }
    }
    None
}

/// A known file under one of `dirs`: at its own path, or by its name directly in a folder.
fn located(dirs: &[PathBuf], file: &str) -> Option<PathBuf> {
    let known = Path::new(file);
    if dirs.iter().any(|dir| known.starts_with(dir)) && known.is_file() {
        return Some(known.to_path_buf());
    }
    let name = file.rsplit(['/', '\\']).next()?;
    dirs.iter().map(|dir| dir.join(name)).find(|path| path.is_file())
}

/// The first face in the folders, in sorted order, with outlines and each query's character.
fn last_resort(dirs: &[PathBuf], mut left: Vec<&Query>) -> Vec<(Query, Answer)> {
    let mut out = Vec::with_capacity(left.len());
    for path in font_files(dirs) {
        if left.is_empty() {
            break;
        }
        if std::fs::metadata(&path).map_or(true, |meta| meta.len() > MAX_FONT_BYTES) {
            continue;
        }
        for face in face_tables(&path, b"cmap", MAX_CMAP) {
            left.retain(|query| {
                let has = sets(&face, query.ch);
                if has {
                    out.push(((*query).clone(), Some((path.clone(), face.index))));
                }
                !has
            });
        }
    }
    out.extend(left.into_iter().map(|query| (query.clone(), None)));
    out
}

/// Whether a face has outlines and a glyph for `ch`, by its character map.
fn sets(face: &FaceTable, ch: char) -> bool {
    let Some(cmap) = face.table.as_deref().filter(|_| face.outlines).and_then(ttf_parser::cmap::Table::parse) else { return false };
    cmap.subtables.into_iter().filter(|table| table.is_unicode()).any(|table| table.glyph_index(u32::from(ch)).is_some_and(|glyph| glyph.0 != 0))
}
