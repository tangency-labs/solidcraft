//! The host part of the interface type: the bundled IBM Plex faces, user overrides, and
//! language-specific system fallbacks (CJK, Hangul, Thai, Hebrew), installed under the names the
//! kit's type API gives (see [`crate::ui_kit::type_face`]).
//! The same definitions are used on desktop and web; the web shell adds Japanese and Chinese
//! faces on demand.

use std::sync::Arc;

use egui::{FontDefinitions, FontFamily};

use crate::ui_kit::type_face::{
    BOLD_FAMILY, CHINESE_FALLBACK, CJK_FALLBACK, HANGUL_FALLBACK, MEDIUM_FAMILY, SEMIBOLD_FAMILY, SYMBOL_FALLBACK, UI_FONT, fallback_order,
    needs_cjk_font, sans_families, semibold,
};
// The Thai, Hebrew and Arabic faces come only from the desktop's system font search.
#[cfg(not(target_arch = "wasm32"))]
use crate::ui_kit::type_face::{ARABIC_FALLBACK, HEBREW_FALLBACK, THAI_FALLBACK};

const PLEX_SANS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf"));
const PLEX_MONO: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"));
const WEIGHTS: [(&str, f32); 4] = [("plex-sans", 400.0), (MEDIUM_FAMILY, 500.0), (SEMIBOLD_FAMILY, 600.0), (BOLD_FAMILY, 700.0)];

fn all_families() -> impl Iterator<Item = FontFamily> {
    sans_families().into_iter().chain([FontFamily::Monospace])
}

/// IBM Plex Sans at 400/500/600/700 and Plex Mono Regular, followed by egui's symbol/emoji
/// fallbacks. Each variable instance shares the embedded bytes and selects a real weight.
pub fn base_font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for ((name, weight), family) in WEIGHTS.into_iter().zip(sans_families()) {
        let mut data = egui::FontData::from_static(PLEX_SANS);
        data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
        fonts.families.insert(family, std::iter::once(name.to_owned()).chain(fallbacks.iter().cloned()).collect());
    }
    fonts.font_data.insert("plex-mono".to_owned(), Arc::new(egui::FontData::from_static(PLEX_MONO)));
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex-mono".to_owned());
    with_symbol_fallback(fonts)
}

// Keep pending definitions too: set_fonts takes effect on the next frame, while the web
// fetch and preference update can both finish before that frame starts.
fn installed_fonts_id() -> egui::Id {
    egui::Id::unique("solidcraft_interface_fonts")
}

fn install(ctx: &egui::Context, fonts: FontDefinitions) {
    let fonts = with_symbol_fallback(fonts);
    ctx.data_mut(|d| d.insert_temp(installed_fonts_id(), fonts.clone()));
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        if let Some(heading) = style.text_styles.get_mut(&egui::TextStyle::Heading) {
            heading.family = semibold(heading.size).family;
        }
    });
}

/// Install only the bundled definitions (also used by the offscreen scenario shell).
pub fn setup_bundled_fonts(ctx: &egui::Context) {
    install(ctx, base_font_definitions());
}

/// The interface font a preference names (#1290): an installed family, a font file, or the
/// sketch text's built-in family. `None` for the default (empty) or a font that can't be read.
pub fn ui_font(name: &str) -> Option<egui::FontData> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let data = solidcraft_engine::sketch::font_data(Some(name)).ok()?;
    ttf_parser::Face::parse(data.bytes(), data.index).ok()?;
    let mut font = egui::FontData::from_owned(data.bytes().to_vec());
    font.index = data.index;
    Some(font)
}

/// Put a chosen interface face first in every sans family; keep Plex and language fallbacks.
/// A user-selected static face keeps its own weight (we never synthesize a bold face).
pub fn with_ui_font(mut fonts: FontDefinitions, font: egui::FontData) -> FontDefinitions {
    fonts.font_data.insert(UI_FONT.to_owned(), Arc::new(font));
    for family in sans_families() {
        let list = fonts.families.entry(family).or_default();
        list.retain(|entry| entry != UI_FONT);
        list.insert(0, UI_FONT.to_owned());
    }
    fonts
}

/// `fonts` with egui's built-in Hack last in every sans family. Ubuntu-Light, NotoEmoji and
/// emoji-icon-font lack common symbols such as ▲ ▼ ↑ ↓ → ⁻, which Hack has, so they no longer
/// show as empty boxes where no system font fills in (#2955). Text the other faces draw keeps them.
pub fn with_symbol_fallback(mut fonts: egui::FontDefinitions) -> egui::FontDefinitions {
    if fonts.font_data.contains_key(SYMBOL_FALLBACK) {
        for family in sans_families() {
            let list = fonts.families.entry(family).or_default();
            list.retain(|name| name != SYMBOL_FALLBACK);
            list.push(SYMBOL_FALLBACK.to_owned());
        }
    }
    fonts
}

/// `fonts` with `bytes` appended as the last fallback of every interface
/// family, after the Latin UI font. `None` when the bytes are not a font.
pub fn with_cjk_fallback(fonts: egui::FontDefinitions, bytes: Vec<u8>) -> Option<egui::FontDefinitions> {
    with_fallback(fonts, CJK_FALLBACK, bytes)
}

/// `fonts` with `bytes` appended under `name` as the last fallback of every interface
/// family. `None` when the bytes are not a font.
fn with_fallback(mut fonts: egui::FontDefinitions, name: &str, bytes: Vec<u8>) -> Option<egui::FontDefinitions> {
    ttf_parser::Face::parse(&bytes, 0).ok()?;
    fonts.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in all_families() {
        let list = fonts.families.entry(family).or_default();
        list.retain(|entry| entry != name);
        list.push(name.to_owned());
    }
    Some(fonts)
}

/// Use one primary CJK face for mixed-script labels in every sans weight family.
/// An explicit interface font stays first; Plex and the other fallbacks remain available.
fn promote_font_to_front(fonts: &mut FontDefinitions, name: &str) {
    for family in sans_families() {
        if let Some(list) = fonts.families.get_mut(&family)
            && let Some(pos) = list.iter().position(|entry| entry == name)
        {
            let item = list.remove(pos);
            let first = usize::from(list.first().is_some_and(|entry| entry == UI_FONT));
            list.insert(first, item);
        }
    }
}

/// Whether the UI language needs the Simplified or Traditional Chinese web font (#8894).
pub fn needs_chinese_font(language: &str) -> bool {
    matches!(language, "zh" | "zh-TW")
}

/// Which on-demand web face a CJK UI language loads: `"zh"`, `"zh-TW"`, or `"ja"` (also for
/// Korean until a Hangul face ships). `None` when the language uses only the bundled Latin faces.
pub fn web_script_font_kind(language: &str) -> Option<&'static str> {
    match language {
        "zh" => Some("zh"),
        "zh-TW" => Some("zh-TW"),
        "ja" | "ko" => Some("ja"),
        _ => None,
    }
}

/// Install the downloaded Japanese font with the CJK UI's shared mixed-script baseline.
pub fn install_cjk_font_for(ctx: &egui::Context, bytes: Vec<u8>, language: &str) -> bool {
    install_web_script_fonts(ctx, Some(bytes), None, language)
}

/// Install the web shell's on-demand Japanese and/or Chinese faces, ordered for `language`.
/// Returns false only when at least one byte blob was supplied and none of them parsed as a font.
pub fn install_web_script_fonts(ctx: &egui::Context, cjk: Option<Vec<u8>>, chinese: Option<Vec<u8>>, language: &str) -> bool {
    if cjk.is_none() && chinese.is_none() {
        setup_fonts_for(ctx, language, "");
        return true;
    }
    let mut fonts = ctx.data(|d| d.get_temp::<FontDefinitions>(installed_fonts_id())).unwrap_or_else(base_font_definitions);
    // Drop prior script faces so a language change can replace them without stacking duplicates.
    for name in [CJK_FALLBACK, CHINESE_FALLBACK] {
        fonts.font_data.remove(name);
        for family in all_families() {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.retain(|entry| entry != name);
            }
        }
    }
    let mut any = false;
    for (name, bytes) in [(CJK_FALLBACK, cjk), (CHINESE_FALLBACK, chinese)] {
        let Some(bytes) = bytes else { continue };
        any = true;
        if let Some(next) = with_fallback(fonts.clone(), name, bytes) {
            fonts = next;
        }
    }
    if any && !fonts.font_data.contains_key(CJK_FALLBACK) && !fonts.font_data.contains_key(CHINESE_FALLBACK) {
        return false;
    }
    if needs_cjk_font(language)
        && let Some(primary) = fallback_order(language)
            .into_iter()
            .find(|name| [CJK_FALLBACK, CHINESE_FALLBACK, HANGUL_FALLBACK].contains(name) && fonts.font_data.contains_key(*name))
    {
        promote_font_to_front(&mut fonts, primary);
    }
    install(ctx, fonts);
    true
}

/// Lower-case file-name fragments that identify CJK fonts. Deliberately specific: generic
/// words such as "noto" or "gothic" also match Latin-only fonts (NotoSans-Regular, Century Gothic).
pub const CJK_FONT_KEYWORDS: &[&str] = &[
    "cjk",
    "notosansjp",
    "notoserifjp",
    "bizud",
    "biz-ud",
    "yugoth",
    "meiryo",
    "msgothic",
    "hiragino",
    "pingfang",
    "songti",
    "arial unicode",
    "ipag",
    "ipaexg",
    "takao",
    "vlgothic",
    "droidsansfallback",
];

/// Whether a font file name looks like a CJK font.
pub fn is_cjk_font_name(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    CJK_FONT_KEYWORDS.iter().any(|k| name.contains(k))
}

/// Lower-case file-name fragments that identify fonts covering simplified Chinese. Deliberately
/// separate from [`CJK_FONT_KEYWORDS`]: many Japanese fonts (Hiragino Gothic, Meiryo, Yu Gothic)
/// match those but lack simplified characters, so a Chinese fallback searches these instead.
pub const CHINESE_FONT_KEYWORDS: &[&str] = &[
    "pingfang",
    "hiragino sans gb",
    "stheiti",
    "songti",
    "arial unicode",
    "msyh",
    "yahei",
    "simhei",
    "simsun",
    "wqy",
    "wenquanyi",
    "notosanscjk",
    "notosanssc",
    "notoserifcjk",
    "notoserifsc",
    "droidsansfallback",
    "sourcehansans",
    "sourcehanserif",
    "siyuan",
];

/// Whether a font file name looks like a font covering simplified Chinese.
pub fn is_chinese_font_name(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    CHINESE_FONT_KEYWORDS.iter().any(|k| name.contains(k))
}

/// Candidate font paths to look for on different operating systems.
#[cfg(not(target_arch = "wasm32"))]
const FONT_CANDIDATES: &[&str] = &[
    // Windows
    "C:\\Windows\\Fonts\\meiryo.ttc",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\YuGothR.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "C:\\Windows\\Fonts\\BIZ-UDGothicR.ttc",
    "C:\\Windows\\Fonts\\BIZ-UDMinchoM.ttc",
    "C:\\Windows\\Fonts\\yumin.ttf",
    // macOS
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans W3.ttc",
    "/System/Library/Fonts/Hiragino Sans W4.ttc",
    "/System/Library/Fonts/Hiragino Sans W6.ttc",
    "/System/Library/Fonts/Supplemental/Hiragino Sans W3.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
    "/Library/Fonts/Hiragino Sans W3.ttc",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJKjp-Regular.otf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    "/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf",
    "/usr/share/fonts/truetype/takao-gothic/TakaoGothic.ttf",
    "/usr/share/fonts/truetype/vlgothic/VL-PGothic-Regular.ttf",
    "/usr/share/fonts/truetype/ipafont/ipag.ttf",
    "/usr/share/fonts/truetype/ipaexfont/ipaexg.ttf",
    "/usr/share/fonts/opentype/ipafont/ipag.ttf",
    "/usr/share/fonts/opentype/ipaexfont/ipaexg.ttf",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/cjkuni-uming/uming.ttc",
];

/// Hangul fonts, appended after the CJK fallback so Korean renders even when the CJK font found
/// is a Japanese one without Hangul (Meiryo, Hiragino).
#[cfg(not(target_arch = "wasm32"))]
const HANGUL_FONT_CANDIDATES: &[&str] = &[
    // Windows: Malgun Gothic
    "C:\\Windows\\Fonts\\malgun.ttf",
    // macOS: Apple SD Gothic Neo
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
    // Linux: Nanum Gothic (Noto Sans CJK already covers Hangul)
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/usr/share/fonts/nanum/NanumGothic.ttf",
];

/// Thai fonts, appended last: none of the Latin or CJK faces draw Thai, so without one Thai text
/// shows as empty boxes (#1902).
#[cfg(not(target_arch = "wasm32"))]
const THAI_FONT_CANDIDATES: &[&str] = &[
    // Windows: Leelawadee UI (Windows 8 and later), Leelawadee, Tahoma
    "C:\\Windows\\Fonts\\LeelawUI.ttf",
    "C:\\Windows\\Fonts\\leelawad.ttf",
    "C:\\Windows\\Fonts\\tahoma.ttf",
    // macOS: Thonburi, Ayuthaya
    "/System/Library/Fonts/Thonburi.ttc",
    "/System/Library/Fonts/Supplemental/Thonburi.ttc",
    "/System/Library/Fonts/Supplemental/Ayuthaya.ttf",
    "/Library/Fonts/Thonburi.ttf",
    // Linux: Noto Sans Thai, TLWG Loma and Garuda
    "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSansThai-Regular.ttf",
    "/usr/share/fonts/google-noto-vf/NotoSansThai[wght].ttf",
    "/usr/share/fonts/opentype/noto/NotoSansThai-Regular.ttf",
    "/usr/share/fonts/truetype/tlwg/Loma.ttf",
    "/usr/share/fonts/opentype/tlwg/Loma.otf",
    "/usr/share/fonts/truetype/tlwg/Garuda.ttf",
    "/usr/share/fonts/opentype/tlwg/Garuda.otf",
    "/usr/share/fonts/tlwg/Loma.otf",
];

/// Hebrew fonts, appended last: egui's built-in faces have no Hebrew, so without one the Hebrew
/// interface and Hebrew names show as empty boxes (#3457).
#[cfg(not(target_arch = "wasm32"))]
const HEBREW_FONT_CANDIDATES: &[&str] = &[
    // Windows: Segoe UI, Arial
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    // macOS: Arial Hebrew, SF Hebrew
    "/System/Library/Fonts/ArialHB.ttc",
    "/System/Library/Fonts/SFHebrew.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    // Linux: Noto Sans Hebrew, DejaVu Sans
    "/usr/share/fonts/noto/NotoSansHebrew-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSansHebrew-Regular.ttf",
    "/usr/share/fonts/google-noto-vf/NotoSansHebrew[wght].ttf",
    "/usr/share/fonts/opentype/noto/NotoSansHebrew-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
];

/// Arabic fonts with the joining forms the shaper uses. egui's built-in faces have no Arabic.
#[cfg(not(target_arch = "wasm32"))]
const ARABIC_FONT_CANDIDATES: &[&str] = &[
    // Windows: Segoe UI, Tahoma, Arial
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\tahoma.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    // macOS: SF Arabic (11 and later), Geeza Pro, Damascus, Tahoma
    "/System/Library/Fonts/SFArabic.ttf",
    "/System/Library/Fonts/GeezaPro.ttc",
    "/System/Library/Fonts/Supplemental/GeezaPro.ttc",
    "/System/Library/Fonts/Supplemental/Damascus.ttc",
    "/System/Library/Fonts/Supplemental/Tahoma.ttf",
    // Linux: Noto Sans Arabic, Noto Naskh Arabic, KACST One, DejaVu Sans
    "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/google-noto-vf/NotoSansArabic[wght].ttf",
    "/usr/share/fonts/opentype/noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf",
    "/usr/share/fonts/noto/NotoNaskhArabic-Regular.ttf",
    "/usr/share/fonts/truetype/kacst-one/KacstOne.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
];

/// Fonts covering simplified Chinese, appended after the CJK fallback so Chinese renders even
/// when the CJK font found is a Japanese one without simplified characters (Hiragino Gothic,
/// Meiryo, Yu Gothic).
#[cfg(not(target_arch = "wasm32"))]
const CHINESE_FONT_CANDIDATES: &[&str] = &[
    // Windows: Microsoft YaHei
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\msyh.ttf",
    // macOS: PingFang SC, Hiragino Sans GB, Songti SC, STHeiti, Arial Unicode MS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    // Linux: Noto Sans CJK SC
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
];

/// Whether the first face in `bytes` draws `c`: some candidates (an older Arial, a trimmed
/// DejaVu) may lack the script they are listed for.
#[cfg(not(target_arch = "wasm32"))]
fn covers(bytes: &[u8], c: char) -> bool {
    ttf_parser::Face::parse(bytes, 0).is_ok_and(|face| face.glyph_index(c).is_some())
}

#[cfg(not(target_arch = "wasm32"))]
fn try_read_font(path: &std::path::Path) -> Option<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path)
        && ttf_parser::Face::parse(&bytes, 0).is_ok()
    {
        Some(bytes)
    } else {
        None
    }
}

/// Recursively search for matching font files up to a bounded depth, testing names with `matches`.
#[cfg(not(target_arch = "wasm32"))]
fn scan_dir_for_font(dir: &std::path::Path, depth_remaining: usize, matches: fn(&str) -> bool) -> Option<Vec<u8>> {
    scan_dir_for_font_where(dir, depth_remaining, matches, |_| true)
}

/// [`scan_dir_for_font`] that also skips a font whose bytes `accept` refuses.
#[cfg(not(target_arch = "wasm32"))]
fn scan_dir_for_font_where(dir: &std::path::Path, depth_remaining: usize, matches: fn(&str) -> bool, accept: fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    if depth_remaining == 0 {
        return None;
    }
    let rd = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();

    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_file() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let is_font = name.ends_with(".ttc") || name.ends_with(".ttf") || name.ends_with(".otf");
            if is_font
                && matches(&name)
                && let Some(bytes) = try_read_font(&p)
                && accept(&bytes)
            {
                return Some(bytes);
            }
        } else if p.is_dir() {
            subdirs.push(p);
        }
    }

    for sub in subdirs {
        if let Some(bytes) = scan_dir_for_font_where(&sub, depth_remaining.saturating_sub(1), matches, accept) {
            return Some(bytes);
        }
    }

    None
}

/// The font folders named by `SOLIDCRAFT_FONT_DIRS` (a path list), or else by the single folder
/// in `CRAFT_FONTS_DIR` (the craft-fonts pack).
#[cfg(not(target_arch = "wasm32"))]
fn font_override_dirs(dirs: Option<std::ffi::OsString>, pack: Option<std::ffi::OsString>) -> Vec<std::path::PathBuf> {
    match (dirs, pack) {
        (Some(dirs), _) => std::env::split_paths(&dirs).collect(),
        (None, Some(pack)) => vec![std::path::PathBuf::from(pack)],
        (None, None) => Vec::new(),
    }
}

/// Find a CJK fallback font on the local system or in SOLIDCRAFT_FONT_DIRS / CRAFT_FONTS_DIR.
#[cfg(not(target_arch = "wasm32"))]
fn find_system_cjk_font() -> Option<Vec<u8>> {
    // 1. Check the font folders the user named.
    for dir in font_override_dirs(std::env::var_os("SOLIDCRAFT_FONT_DIRS"), std::env::var_os("CRAFT_FONTS_DIR")) {
        let names = ["BIZUDPGothic-Regular.ttf", "BIZ-UDGothicR.ttc", "NotoSansCJK-Regular.ttc", "NotoSansJP-Regular.ttf"];
        for name in names {
            if let Some(bytes) = try_read_font(&dir.join(name)) {
                return Some(bytes);
            }
        }
    }

    // 2. Check well-known candidate paths.
    for path in FONT_CANDIDATES {
        if let Some(bytes) = try_read_font(std::path::Path::new(path)) {
            return Some(bytes);
        }
    }

    // 3. Platform-specific dynamic directory search.
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR").or_else(|_| std::env::var("SystemRoot")) {
            let fonts_dir = std::path::PathBuf::from(windir).join("Fonts");
            let names = ["meiryo.ttc", "YuGothM.ttc", "YuGothR.ttc", "msgothic.ttc", "BIZ-UDGothicR.ttc"];
            for name in names {
                if let Some(bytes) = try_read_font(&fonts_dir.join(name)) {
                    return Some(bytes);
                }
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let user_fonts = std::path::PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts");
            if let Some(bytes) = scan_dir_for_font(&user_fonts, 2, is_cjk_font_name) {
                return Some(bytes);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut dirs = vec![
            std::path::PathBuf::from("/System/Library/Fonts"),
            std::path::PathBuf::from("/System/Library/Fonts/Supplemental"),
            std::path::PathBuf::from("/Library/Fonts"),
        ];
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join("Library/Fonts"));
        }
        for d in &dirs {
            if let Some(bytes) = scan_dir_for_font(d, 2, is_cjk_font_name) {
                return Some(bytes);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let search_dirs = [
            std::path::PathBuf::from("/usr/share/fonts"),
            std::path::PathBuf::from("/usr/local/share/fonts"),
            std::path::PathBuf::from("/run/host/fonts"),
            std::path::PathBuf::from("/run/host/user-fonts"),
        ];
        for dir in &search_dirs {
            if let Some(bytes) = scan_dir_for_font(dir, 3, is_cjk_font_name) {
                return Some(bytes);
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let user_fonts = std::path::PathBuf::from(home).join(".local/share/fonts");
            if let Some(bytes) = scan_dir_for_font(&user_fonts, 2, is_cjk_font_name) {
                return Some(bytes);
            }
        }
    }

    None
}

/// Configure font definitions with system CJK, Chinese, Hangul, Thai and Hebrew fallbacks where available,
/// ordered for the UI language (see [`fallback_order`]), after the interface font `ui_font`
/// when one is chosen (see [`ui_font`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn setup_fonts_for(ctx: &egui::Context, language: &str, ui_font: &str) {
    install_fonts(ctx, language, ui_font, Some(system_fallbacks()));
}

/// The system fallback faces, searched for once and kept: reading them takes long on a system
/// with large CJK fonts, so the app starts that search on a thread and draws its first frame
/// with the bundled faces (see [`load_system_fonts_in_background`]).
#[cfg(not(target_arch = "wasm32"))]
struct SystemFonts {
    cjk: Option<Vec<u8>>,
    chinese: Option<Vec<u8>>,
    hangul: Option<Vec<u8>>,
    thai: Option<Vec<u8>>,
    hebrew: Option<Vec<u8>>,
    arabic: Option<Vec<u8>>,
}

#[cfg(not(target_arch = "wasm32"))]
static SYSTEM_FONTS: std::sync::OnceLock<SystemFonts> = std::sync::OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
fn system_fallbacks() -> &'static SystemFonts {
    SYSTEM_FONTS.get_or_init(|| {
        let cjk = find_system_cjk_font();
        let chinese = find_system_chinese_font().filter(|bytes| cjk.as_ref() != Some(bytes));
        let hangul = HANGUL_FONT_CANDIDATES.iter().find_map(|path| try_read_font(std::path::Path::new(path)));
        let thai = THAI_FONT_CANDIDATES.iter().find_map(|path| try_read_font(std::path::Path::new(path)));
        let hebrew = HEBREW_FONT_CANDIDATES.iter().find_map(|path| try_read_font(std::path::Path::new(path)).filter(|bytes| covers(bytes, 'א')));
        let arabic = arabic_font();
        SystemFonts { cjk, chinese, hangul, thai, hebrew, arabic }
    })
}

/// Whether the system fallback faces have been searched for.
#[cfg(not(target_arch = "wasm32"))]
pub fn system_fonts_ready() -> bool {
    SYSTEM_FONTS.get().is_some()
}

/// Search for the system fallback faces on a thread and repaint when they are found.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_system_fonts_in_background(ctx: &egui::Context) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if SYSTEM_FONTS.get().is_some() {
        return;
    }
    let mut first = false;
    STARTED.call_once(|| first = true);
    if !first {
        return;
    }
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("system-fonts".into()).spawn(move || {
        system_fallbacks();
        ctx.request_repaint();
    });
    // Without a thread the first frame waits for them, as it always did.
    if spawned.is_err() {
        system_fallbacks();
    }
}

/// [`setup_fonts_for`] with the system fallbacks that are there already, never waiting for the search.
#[cfg(not(target_arch = "wasm32"))]
pub fn setup_fonts_without_waiting(ctx: &egui::Context, language: &str, ui_font: &str) {
    install_fonts(ctx, language, ui_font, SYSTEM_FONTS.get());
}

#[cfg(not(target_arch = "wasm32"))]
fn install_fonts(ctx: &egui::Context, language: &str, ui_font: &str, system: Option<&SystemFonts>) {
    let (cjk, chinese, hangul, thai, hebrew, arabic) = match system {
        Some(f) => (f.cjk.clone(), f.chinese.clone(), f.hangul.clone(), f.thai.clone(), f.hebrew.clone(), f.arabic.clone()),
        None => (None, None, None, None, None, None),
    };
    let mut fonts = base_font_definitions();
    if let Some(font) = self::ui_font(ui_font) {
        fonts = with_ui_font(fonts, font);
    }
    let order = fallback_order(language);
    for name in order {
        let bytes = match name {
            CJK_FALLBACK => cjk.clone(),
            CHINESE_FALLBACK => chinese.clone(),
            HANGUL_FALLBACK => hangul.clone(),
            THAI_FALLBACK => thai.clone(),
            HEBREW_FALLBACK => hebrew.clone(),
            _ => arabic.clone(),
        };
        if let Some(bytes) = bytes
            && let Some(with_fallback) = with_fallback(fonts.clone(), name, bytes)
        {
            fonts = with_fallback;
        }
    }
    if needs_cjk_font(language) {
        // Select the first installed face in language order (a duplicate Chinese face may
        // already be installed as CJK_FALLBACK). Keep explicit interface choices ahead of it.
        if let Some(primary) =
            order.into_iter().find(|name| ![THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK].contains(name) && fonts.font_data.contains_key(*name))
        {
            promote_font_to_front(&mut fonts, primary);
        }
    }
    install(ctx, fonts);
}

/// Configure font definitions with system fallbacks for an English UI (all fallbacks are
/// installed whatever the language, so CJK text in a design still renders; only their order
/// follows the language, see [`setup_fonts_for`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn setup_fonts(ctx: &egui::Context) {
    setup_fonts_for(ctx, "en", "");
}

/// Find a font covering simplified Chinese in the known candidate paths or by scanning the
/// system font directories for [`CHINESE_FONT_KEYWORDS`] names.
#[cfg(not(target_arch = "wasm32"))]
fn find_system_chinese_font() -> Option<Vec<u8>> {
    for path in CHINESE_FONT_CANDIDATES {
        if let Some(bytes) = try_read_font(std::path::Path::new(path)) {
            return Some(bytes);
        }
    }
    find_chinese_font_in(&chinese_font_dirs())
}

/// A character only fonts covering simplified Chinese draw: 设 (as in 设置, "settings"), boxed in
/// the report of #3376. Japanese fonts (Meiryo, Yu Gothic, BIZ UDPGothic, IPA) lack it.
#[cfg(not(target_arch = "wasm32"))]
const SIMPLIFIED_CHINESE_PROBE: char = '设';

/// Whether the font's first face draws simplified Chinese (see [`SIMPLIFIED_CHINESE_PROBE`]).
#[cfg(not(target_arch = "wasm32"))]
fn draws_simplified_chinese(bytes: &[u8]) -> bool {
    ttf_parser::Face::parse(bytes, 0).is_ok_and(|face| face.glyph_index(SIMPLIFIED_CHINESE_PROBE).is_some())
}

/// The system and per-user font folders searched for a Chinese font when no candidate path has
/// one, on every desktop system (#3376: Linux and Windows searched only the fixed paths, so a
/// Japanese CJK font drew the Chinese UI alone and its simplified characters showed as boxes).
#[cfg(not(target_arch = "wasm32"))]
fn chinese_font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR").or_else(|_| std::env::var("SystemRoot")) {
            dirs.push(std::path::PathBuf::from(windir).join("Fonts"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(std::path::PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        for d in ["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"] {
            dirs.push(std::path::PathBuf::from(d));
        }
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        dirs.push(std::path::PathBuf::from("/usr/share/fonts"));
        dirs.push(std::path::PathBuf::from("/usr/local/share/fonts"));
        dirs.push(std::path::PathBuf::from("/run/host/fonts"));
        dirs.push(std::path::PathBuf::from("/run/host/user-fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(&home).join(".local/share/fonts"));
            dirs.push(std::path::PathBuf::from(home).join(".fonts"));
        }
    }
    dirs
}

/// The first font in `dirs` (searched three folders deep, which reaches
/// `/usr/share/fonts/truetype/wqy/`) with a Chinese font's name that draws simplified Chinese.
/// The drawing check keeps out a Japanese-only file whose name matches, such as a
/// `SourceHanSansJP` subset, and Windows' SimSun extension fonts.
#[cfg(not(target_arch = "wasm32"))]
fn find_chinese_font_in(dirs: &[std::path::PathBuf]) -> Option<Vec<u8>> {
    dirs.iter().find_map(|dir| scan_dir_for_font_where(dir, 3, is_chinese_font_name, draws_simplified_chinese))
}

/// The first Arabic font in `CRAFT_FONTS_DIR` (`NotoSansArabic-Regular.ttf`) or the candidate
/// paths that draws Arabic.
#[cfg(not(target_arch = "wasm32"))]
fn arabic_font() -> Option<Vec<u8>> {
    let pack = std::env::var("CRAFT_FONTS_DIR").ok().map(|dir| std::path::PathBuf::from(dir).join("NotoSansArabic-Regular.ttf"));
    pack.into_iter()
        .chain(ARABIC_FONT_CANDIDATES.iter().map(std::path::PathBuf::from))
        .find_map(|path| try_read_font(&path).filter(|bytes| covers(bytes, 'ب')))
}

/// Web installs the bundled faces synchronously; Japanese and Chinese are fetched by the shell
/// on demand. Preserve those downloaded faces when preferences change later in the session.
#[cfg(target_arch = "wasm32")]
pub fn load_system_fonts_in_background(_ctx: &egui::Context) {}

#[cfg(target_arch = "wasm32")]
pub fn system_fonts_ready() -> bool {
    true
}

#[cfg(target_arch = "wasm32")]
pub fn setup_fonts_without_waiting(ctx: &egui::Context, language: &str, ui_font: &str) {
    setup_fonts_for(ctx, language, ui_font);
}

#[cfg(target_arch = "wasm32")]
pub fn setup_fonts_for(ctx: &egui::Context, language: &str, ui_font: &str) {
    let previous = ctx.data(|d| d.get_temp::<FontDefinitions>(installed_fonts_id()));
    let mut fonts = web_font_definitions(ui_font, previous);
    if needs_cjk_font(language)
        && let Some(primary) = fallback_order(language)
            .into_iter()
            .find(|name| [CJK_FALLBACK, CHINESE_FALLBACK, HANGUL_FALLBACK].contains(name) && fonts.font_data.contains_key(*name))
    {
        promote_font_to_front(&mut fonts, primary);
    }
    install(ctx, fonts);
}

#[cfg(any(test, target_arch = "wasm32"))]
fn web_font_definitions(ui_font: &str, previous: Option<FontDefinitions>) -> FontDefinitions {
    let mut fonts = base_font_definitions();
    if let Some(font) = self::ui_font(ui_font) {
        fonts = with_ui_font(fonts, font);
    }
    if let Some(prev) = previous {
        for name in [CJK_FALLBACK, CHINESE_FALLBACK] {
            if let Some(data) = prev.font_data.get(name).cloned() {
                fonts.font_data.insert(name.to_owned(), data);
                for family in all_families() {
                    fonts.families.entry(family).or_default().push(name.to_owned());
                }
            }
        }
    }
    with_symbol_fallback(fonts)
}

/// Web starts with the bundled faces and symbol fallback; script fonts load on demand.
#[cfg(target_arch = "wasm32")]
pub fn setup_fonts(ctx: &egui::Context) {
    setup_fonts_for(ctx, "en", "");
}

/// A context with the production bundled faces, for isolated widget tests.
#[cfg(test)]
pub(crate) fn test_context() -> egui::Context {
    crate::i18n::install();
    let ctx = egui::Context::default();
    setup_bundled_fonts(&ctx);
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_kit::type_face::{RichTextExt as _, bold, medium};
    use egui::{FontId, RichText};

    /// `SOLIDCRAFT_FONT_DIRS` wins and may list several folders; `CRAFT_FONTS_DIR` is the fallback.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cjk_font_folders_from_the_environment() -> Result<(), std::env::JoinPathsError> {
        use std::path::PathBuf;
        let list = std::env::join_paths(["/a", "/b"])?;
        assert_eq!(font_override_dirs(Some(list), Some("/pack".into())), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(font_override_dirs(None, Some("/pack".into())), vec![PathBuf::from("/pack")]);
        assert!(font_override_dirs(None, None).is_empty());
        #[cfg(target_os = "linux")]
        for dir in ["/run/host/fonts", "/run/host/user-fonts"] {
            assert!(chinese_font_dirs().contains(&PathBuf::from(dir)), "missing {dir}");
        }
        Ok(())
    }

    /// The system faces are searched for on a thread; installing fonts before they are found does
    /// not wait for the search and still gives the bundled faces.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn system_fonts_load_in_the_background_and_installing_does_not_wait() {
        let ctx = egui::Context::default();
        setup_fonts_without_waiting(&ctx, "en", "");
        assert!(ctx.data(|d| d.get_temp::<FontDefinitions>(installed_fonts_id())).is_some());
        load_system_fonts_in_background(&ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !system_fonts_ready() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(system_fonts_ready());
    }

    /// Whether a face of the Proportional family has `c`. Checked in the font data:
    /// `Fonts::has_glyph` misses a glyph in the face that also gives the replacement glyph.
    fn proportional_draws(fonts: &egui::FontDefinitions, c: char) -> bool {
        fonts.families[&egui::FontFamily::Proportional]
            .iter()
            .filter_map(|name| fonts.font_data.get(name))
            .any(|data| ttf_parser::Face::parse(data.bytes(), data.index).unwrap().glyph_index(c).is_some())
    }

    /// The symbols the built-in Latin faces lack draw through the built-in Hack (#2955), which
    /// comes last and once, after the system fallbacks, and goes in with the desktop's fonts.
    #[test]
    fn symbols_fall_back_to_the_built_in_hack() {
        let fonts = with_symbol_fallback(base_font_definitions());
        for family in sans_families() {
            let list = &fonts.families[&family];
            assert_eq!(list.last().map(String::as_str), Some(SYMBOL_FALLBACK));
            assert_eq!(list.iter().filter(|name| *name == SYMBOL_FALLBACK).count(), 1);
        }
        for c in "▲▼↑↓→⁻".chars() {
            assert!(proportional_draws(&fonts, c), "{c:?} (U+{:04X}) has a face", c as u32);
        }
        let mut with_cjk = egui::FontDefinitions::default();
        with_cjk.families.entry(egui::FontFamily::Proportional).or_default().push(CJK_FALLBACK.to_owned());
        let fonts = with_symbol_fallback(with_symbol_fallback(with_cjk));
        let list = &fonts.families[&egui::FontFamily::Proportional];
        assert_eq!(list.last().map(String::as_str), Some(SYMBOL_FALLBACK), "{list:?}");
        assert_eq!(list.iter().filter(|name| *name == SYMBOL_FALLBACK).count(), 1, "{list:?}");
        assert_ne!(list.first().map(String::as_str), Some(SYMBOL_FALLBACK), "{list:?}");
        let ctx = egui::Context::default();
        setup_fonts_for(&ctx, "en", "");
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let installed = ctx.fonts(|f| f.definitions().families[&egui::FontFamily::Proportional].clone());
        assert_eq!(installed.last().map(String::as_str), Some(SYMBOL_FALLBACK), "{installed:?}");
    }

    #[test]
    fn the_interface_font_comes_first_and_keeps_the_fallbacks() {
        // #1290: the chosen font leads the Proportional family; the default face stays after it.
        assert!(ui_font("").is_none());
        assert!(ui_font("No Such Font 1290").is_none());
        let font = ui_font(solidcraft_engine::sketch::DEFAULT_FONT_FAMILY).expect("the built-in sketch font reads");
        let defaults = base_font_definitions();
        let fonts = with_ui_font(defaults.clone(), font.clone());
        let again = with_ui_font(fonts, font);
        for family in sans_families() {
            let list = &again.families[&family];
            assert_eq!(list[0], UI_FONT);
            assert_eq!(&list[1..], defaults.families[&family]);
            assert_eq!(list.iter().filter(|n| *n == UI_FONT).count(), 1);
        }
        assert_eq!(again.families[&FontFamily::Monospace], defaults.families[&FontFamily::Monospace]);
    }

    #[test]
    fn bundled_faces_lead_every_family_and_keep_egui_fallbacks() {
        let defaults = with_symbol_fallback(FontDefinitions::default());
        let fonts = base_font_definitions();
        for ((name, weight), family) in WEIGHTS.into_iter().zip(sans_families()) {
            let list = &fonts.families[&family];
            assert_eq!(list[0], name);
            assert_eq!(&list[1..], defaults.families[&FontFamily::Proportional]);
            let data = &fonts.font_data[name];
            assert_eq!(data.bytes(), PLEX_SANS);
            assert_eq!(data.tweak.coords.as_ref()[0].1, weight);
            assert!(data.variation_axes().iter().any(|a| a.tag == *b"wght"));
        }
        let mono = &fonts.families[&FontFamily::Monospace];
        assert_eq!(mono[0], "plex-mono");
        assert_eq!(&mono[1..], defaults.families[&FontFamily::Monospace]);
        assert_eq!(fonts.font_data["plex-mono"].bytes(), PLEX_MONO);
    }

    #[test]
    fn semibold_has_distinct_metrics_and_rich_text_preserves_size_and_colour() {
        let h = crate::scenario::Harness::new();
        let ink = solidcraft_theme::Tokens::get().accent;
        let (regular, heavier) = h.ctx.fonts_mut(|f| {
            let regular = f.layout_no_wrap("Chamfer Revolve New Sketch".into(), FontId::proportional(16.0), ink);
            let heavier = f.layout_no_wrap("Chamfer Revolve New Sketch".into(), semibold(16.0), ink);
            (regular.size().x, heavier.size().x)
        });
        assert!(heavier > regular + 0.1, "real semibold metrics differ: {regular} -> {heavier}");
        let mut output = h.ctx.run_ui(egui::RawInput::default(), |ui| {
            for (text, expected) in [
                (RichText::new("Medium").color(ink).size(16.0).medium(), medium(16.0)),
                (RichText::new("Semibold").color(ink).size(16.0).semibold(), semibold(16.0)),
                (RichText::new("Bold").color(ink).size(16.0).bold_face(), bold(16.0)),
            ] {
                let galley = egui::WidgetText::from(text).into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body);
                assert_eq!(galley.job.sections[0].format.font_id, expected);
                assert_eq!(galley.job.sections[0].format.color, ink);
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn web_download_and_preference_changes_compose_without_losing_plex_or_japanese() {
        let ctx = test_context();
        let chosen = solidcraft_engine::sketch::DEFAULT_FONT_FAMILY;
        install(&ctx, web_font_definitions(chosen, None));
        // Both installations happen before the first frame, like a fast cached web fetch.
        assert!(install_cjk_font_for(
            &ctx,
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf")).to_vec(),
            "ja",
        ));
        let installed = ctx.data(|d| d.get_temp::<FontDefinitions>(installed_fonts_id())).unwrap();
        for family in sans_families() {
            assert_eq!(installed.families[&family][0], UI_FONT);
            assert_eq!(installed.families[&family].get(1).map(String::as_str), Some(CJK_FALLBACK));
        }
        let reset = web_font_definitions("", Some(installed));
        for ((name, _), family) in WEIGHTS.into_iter().zip(sans_families()) {
            assert_eq!(reset.families[&family][0], name);
            let list = &reset.families[&family];
            assert_eq!(list.get(list.len() - 2).map(String::as_str), Some(CJK_FALLBACK));
            assert_eq!(list.last().map(String::as_str), Some(SYMBOL_FALLBACK));
        }
        install(&ctx, reset);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&semibold(14.0), "日本語テスト")));
    }

    #[test]
    fn japanese_and_chinese_fallbacks_cover_semibold_text() {
        let japanese = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf"));
        let chinese = find_system_chinese_font();
        for language in ["ja", "zh"] {
            let mut fonts = base_font_definitions();
            for name in fallback_order(language) {
                let bytes = match name {
                    CJK_FALLBACK => Some(japanese.to_vec()),
                    CHINESE_FALLBACK => chinese.clone(),
                    _ => None,
                };
                if let Some(bytes) = bytes {
                    fonts = with_fallback(fonts, name, bytes).unwrap();
                }
            }
            let ctx = test_context();
            install(&ctx, fonts);
            let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
            output.textures_delta.clear();
            assert!(ctx.fonts_mut(|f| f.has_glyphs(&semibold(14.0), "日本語テスト")));
            if chinese.is_some() {
                assert!(ctx.fonts_mut(|f| f.has_glyphs(&semibold(14.0), "简体中文")));
            }
        }
    }

    fn latin_font_bytes() -> Vec<u8> {
        let defaults = base_font_definitions();
        let name = &defaults.families[&egui::FontFamily::Proportional][0];
        defaults.font_data[name].bytes().to_vec()
    }

    #[test]
    fn default_ui_language_follows_the_system() {
        // A first start follows the system (#10138); English, the fallback it lands on when
        // nothing matches, needs no CJK face.
        assert_eq!(crate::prefs::Prefs::default().language, solidcraft_ui_text::SYSTEM);
        assert!(!needs_cjk_font("en"));
    }

    #[test]
    fn cjk_font_is_a_fallback_after_the_latin_ui_font() {
        let defaults = base_font_definitions();
        let fonts = with_cjk_fallback(defaults.clone(), latin_font_bytes()).unwrap();
        for family in all_families() {
            let list = &fonts.families[&family];
            assert_eq!(list.first(), defaults.families[&family].first(), "the Latin UI font stays first");
            assert_eq!(list.last().map(String::as_str), Some(CJK_FALLBACK), "CJK is the last fallback");
        }
        assert!(with_cjk_fallback(defaults, b"not a font".to_vec()).is_none());
    }

    #[test]
    fn a_chinese_ui_puts_the_chinese_face_before_the_japanese_one() {
        assert_eq!(fallback_order("zh"), [CHINESE_FALLBACK, CJK_FALLBACK, HANGUL_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK]);
        assert_eq!(fallback_order("zh-TW"), fallback_order("zh"));
        assert_eq!(fallback_order("ko"), [HANGUL_FALLBACK, CJK_FALLBACK, CHINESE_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK]);
        assert_eq!(fallback_order("ar")[0], ARABIC_FALLBACK);
        for lang in ["en", "ja", "de", "he"] {
            assert_eq!(
                fallback_order(lang),
                [CJK_FALLBACK, CHINESE_FALLBACK, HANGUL_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK],
                "{lang} keeps the Japanese font first"
            );
        }
        let defaults = base_font_definitions();
        let mut fonts = defaults.clone();
        for name in fallback_order("zh") {
            fonts = with_fallback(fonts, name, latin_font_bytes()).unwrap();
        }
        for family in all_families() {
            let list = &fonts.families[&family];
            assert_eq!(list.first(), defaults.families[&family].first(), "the Latin UI font stays first");
            assert_eq!(&list[list.len() - 6..], [CHINESE_FALLBACK, CJK_FALLBACK, HANGUL_FALLBACK, THAI_FALLBACK, HEBREW_FALLBACK, ARABIC_FALLBACK]);
        }
    }

    /// Thai typed into the agent chat drew as empty boxes (#1902): every Thai candidate this
    /// machine has must parse and cover Thai, and the fonts set up must then draw it.
    #[test]
    fn a_thai_fallback_draws_thai() {
        let found: Vec<Vec<u8>> = THAI_FONT_CANDIDATES.iter().filter_map(|path| try_read_font(std::path::Path::new(path))).collect();
        for bytes in &found {
            let face = ttf_parser::Face::parse(bytes, 0).unwrap();
            for c in "สวัสดี".chars() {
                assert!(face.glyph_index(c).is_some(), "{c:?} is in the Thai font");
            }
        }
        let thai = |fonts: egui::FontDefinitions| {
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts);
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(14.0), "สวัสดี"))
        };
        assert!(!thai(egui::FontDefinitions::default()), "the built-in faces have no Thai");
        let Some(bytes) = found.into_iter().next() else { return };
        assert!(thai(with_fallback(egui::FontDefinitions::default(), THAI_FALLBACK, bytes).unwrap()), "Thai draws with the fallback");
    }

    /// The Hebrew interface (#3457): every Hebrew candidate this machine has that passes the
    /// coverage check draws Hebrew, and the fonts set up must then draw it.
    #[test]
    fn a_hebrew_fallback_draws_hebrew() {
        let found: Vec<Vec<u8>> =
            HEBREW_FONT_CANDIDATES.iter().filter_map(|path| try_read_font(std::path::Path::new(path))).filter(|bytes| covers(bytes, 'א')).collect();
        for bytes in &found {
            let face = ttf_parser::Face::parse(bytes, 0).unwrap();
            for c in "שלום עולם".chars().filter(|c| !c.is_whitespace()) {
                assert!(face.glyph_index(c).is_some(), "{c:?} is in the Hebrew font");
            }
        }
        let hebrew = |fonts: egui::FontDefinitions| {
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts);
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(14.0), "שלום"))
        };
        assert!(!hebrew(base_font_definitions()), "the bundled faces have no Hebrew");
        let Some(bytes) = found.into_iter().next() else { return };
        assert!(hebrew(with_fallback(base_font_definitions(), HEBREW_FALLBACK, bytes).unwrap()), "Hebrew draws with the fallback");
    }

    /// What a Hebrew interface shows depends on egui's own shaping as much as on `solidcraft_ui_text::bidi::visual`:
    /// lay messages out with a real Hebrew font and read the glyphs left to right, as they land on
    /// screen. They must come out in the order the line has on screen (#3457).
    #[test]
    fn hebrew_messages_land_on_screen_in_display_order() {
        let Some(bytes) = HEBREW_FONT_CANDIDATES.iter().filter_map(|path| try_read_font(std::path::Path::new(path))).find(|bytes| covers(bytes, 'א'))
        else {
            return;
        };
        let ctx = egui::Context::default();
        ctx.set_fonts(with_symbol_fallback(with_fallback(base_font_definitions(), HEBREW_FALLBACK, bytes).unwrap()));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let on_screen = |text: &str| {
            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), egui::FontId::proportional(14.0), egui::Color32::WHITE));
            let mut glyphs: Vec<(f32, char)> =
                galley.rows.iter().flat_map(|row| row.row.glyphs.iter()).filter(|g| g.advance_width > 0.0).map(|g| (g.pos.x, g.chr)).collect();
            glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
            glyphs.into_iter().map(|(_, c)| c).collect::<String>()
        };
        for text in ["עץ התכן", "שמירה בשם", "יחידות: mm", "ייצוא STEP", "פרמטרים (5)", "עומק 20.5 mm", "לשמור את השינויים ב־Bracket?"]
        {
            assert_eq!(on_screen(&solidcraft_ui_text::bidi::visual(text)), solidcraft_ui_text::bidi::screen_order(text), "{text}");
        }
        let he = solidcraft_ui_text::Locale::for_language("he");
        for id in ["ui.preferences", "ui.new_design", "ui.feature_history", "ui.needs_design"] {
            let logical = he.text(id).to_owned();
            assert_eq!(
                on_screen(&logical).chars().filter(|c| !c.is_whitespace()).count(),
                logical.chars().filter(|c| !c.is_whitespace()).count(),
                "{id}"
            );
        }
    }

    /// The Arabic interface: lay messages out with a real Arabic font and read the glyphs left to
    /// right, as they land on screen. The words come out in display order and each word is shaped
    /// whole (joined forms, so fewer visible glyphs is fine, but never a stray letter order).
    #[test]
    fn arabic_messages_land_on_screen_in_display_order() {
        let Some(bytes) = arabic_font() else { return };
        let ctx = egui::Context::default();
        ctx.set_fonts(with_symbol_fallback(with_fallback(egui::FontDefinitions::default(), ARABIC_FALLBACK, bytes).unwrap()));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        // Each space-separated word as the set of its characters (the shaper orders and joins a
        // word's glyphs, and a glyph's character comes from its cluster), placed by its leftmost
        // glyph, left to right.
        let words_left_to_right = |drawn: &str| {
            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(drawn.to_owned(), egui::FontId::proportional(14.0), egui::Color32::WHITE));
            let mut words: Vec<(f32, Vec<char>)> = vec![(f32::MAX, Vec::new())];
            for glyph in galley.rows.iter().flat_map(|row| row.row.glyphs.iter()) {
                if glyph.chr == ' ' {
                    words.push((f32::MAX, Vec::new()));
                } else if let Some((x, word)) = words.last_mut() {
                    *x = x.min(glyph.pos.x);
                    word.push(glyph.chr);
                }
            }
            words.retain(|(_, word)| !word.is_empty());
            words.sort_by(|a, b| a.0.total_cmp(&b.0));
            words.into_iter().map(|(_, word)| char_set(word)).collect::<Vec<_>>()
        };
        fn char_set(mut chars: Vec<char>) -> Vec<char> {
            chars.sort_unstable();
            chars.dedup();
            chars
        }
        // Read right to left, the screen shows the sentence's words in reading order.
        for text in ["حفظ باسم", "تحليل خطوط المعامِل", "إعادة تعيين الكل", "تصدير STEP"] {
            let mut read = words_left_to_right(&solidcraft_ui_text::bidi::visual(text));
            read.reverse();
            let expected: Vec<Vec<char>> = text.split(' ').map(|word| char_set(word.chars().collect())).collect();
            assert_eq!(read, expected, "{text}");
        }
    }

    #[test]
    fn cjk_ui_promotes_primary_cjk_font_to_front_for_consistent_baseline() {
        let defaults = base_font_definitions();
        let mut fonts = with_cjk_fallback(defaults.clone(), latin_font_bytes()).unwrap();
        promote_font_to_front(&mut fonts, CJK_FALLBACK);
        for family in sans_families() {
            let list = &fonts.families[&family];
            assert_eq!(list.first().map(String::as_str), Some(CJK_FALLBACK));
            assert!(list.contains(&defaults.families[&family][0]), "the bundled weight remains available");
        }
        assert_eq!(fonts.families[&FontFamily::Monospace][0], "plex-mono");
        let chosen = egui::FontData::from_static(PLEX_SANS);
        let mut fonts = with_ui_font(fonts, chosen);
        promote_font_to_front(&mut fonts, CJK_FALLBACK);
        for family in sans_families() {
            assert_eq!(fonts.families[&family][0], UI_FONT);
            assert_eq!(fonts.families[&family][1], CJK_FALLBACK);
        }
    }

    /// A Ukrainian UI drew Cyrillic wide and loose: the bundled CJK face (BIZ UDPGothic) sets every
    /// Cyrillic glyph in a full em, so it may only draw a non-CJK UI's text when nothing else has
    /// the glyph. A CJK UI still leads with it, so its digits share the CJK baseline.
    #[test]
    fn a_non_cjk_ui_keeps_the_cjk_face_as_a_fallback() {
        let installed = |language: &str| {
            let ctx = egui::Context::default();
            assert!(install_cjk_font_for(&ctx, latin_font_bytes(), language), "{language}: the font installs");
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            ctx.fonts(|f| f.definitions().families[&egui::FontFamily::Proportional].clone())
        };
        let latin = base_font_definitions().families[&egui::FontFamily::Proportional][0].clone();
        for language in ["uk", "en", "de"] {
            let list = installed(language);
            assert_eq!(list.first(), Some(&latin), "{language} keeps the Latin face first: {list:?}");
            assert_eq!(list.last().map(String::as_str), Some(SYMBOL_FALLBACK), "{language} ends with the symbol fallback: {list:?}");
            assert!(list.iter().position(|name| name == CJK_FALLBACK).is_some_and(|at| at > 0), "{language} keeps CJK behind Latin: {list:?}");
        }
        for language in ["ja", "zh", "ko"] {
            let list = installed(language);
            assert_eq!(list.first().map(String::as_str), Some(CJK_FALLBACK), "{language} leads with the CJK face: {list:?}");
        }
    }

    #[test]
    fn hangul_font_falls_back_after_the_cjk_font() {
        let defaults = base_font_definitions();
        let cjk = with_cjk_fallback(defaults.clone(), latin_font_bytes()).unwrap();
        let fonts = with_fallback(cjk, HANGUL_FALLBACK, latin_font_bytes()).unwrap();
        for family in all_families() {
            let list = &fonts.families[&family];
            assert_eq!(list.first(), defaults.families[&family].first(), "the Latin UI font stays first");
            assert_eq!(&list[list.len() - 2..], [CJK_FALLBACK, HANGUL_FALLBACK], "Hangul follows the CJK fallback");
        }
    }

    #[test]
    fn directory_scan_matches_cjk_fonts_only() {
        for cjk in [
            "NotoSansCJK-Regular.ttc",
            "NotoSansJP-Regular.ttf",
            "BIZUDPGothic-Regular.ttf",
            "YuGothM.ttc",
            "meiryo.ttc",
            "msgothic.ttc",
            "Hiragino Sans W3.ttc",
            "ipag.ttf",
            "TakaoPGothic.ttf",
            "DroidSansFallbackFull.ttf",
        ] {
            assert!(is_cjk_font_name(cjk), "{cjk} is a CJK font");
        }
        for latin in ["NotoSans-Regular.ttf", "NotoSerif-Bold.ttf", "NotoMono-Regular.ttf", "GOTHIC.TTF", "DejaVuSans.ttf", "DroidSans.ttf"] {
            assert!(!is_cjk_font_name(latin), "{latin} is not a CJK font");
        }
    }

    #[test]
    fn chinese_font_names_match_the_files_distributions_install() {
        // #3376: Source Han Sans ships as SourceHanSans*.ttc/otf, which "sourcehanss" never matched.
        for chinese in [
            "msyh.ttc",
            "simsun.ttc",
            "wqy-microhei.ttc",
            "wqy-zenhei.ttc",
            "NotoSansCJK-Regular.ttc",
            "NotoSansCJK-VF.ttc",
            "NotoSansSC-Regular.otf",
            "NotoSerifCJK-Regular.ttc",
            "SourceHanSans.ttc",
            "SourceHanSansSC-Regular.otf",
            "SourceHanSerifSC-Regular.otf",
            "DroidSansFallbackFull.ttf",
        ] {
            assert!(is_chinese_font_name(chinese), "{chinese} is a Chinese font");
        }
        for other in ["meiryo.ttc", "YuGothM.ttc", "msgothic.ttc", "NotoSansJP-Regular.ttf", "BIZUDPGothic-Regular.ttf", "ipag.ttf", "DejaVuSans.ttf"]
        {
            assert!(!is_chinese_font_name(other), "{other} is not a Chinese font");
        }
    }

    /// A temporary font folder, removed when dropped.
    struct FontFolder(std::path::PathBuf);

    impl FontFolder {
        fn new(name: &str) -> FontFolder {
            let dir = std::env::temp_dir().join(format!("solidcraft-ui-font-tests-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            FontFolder(dir)
        }
    }

    impl Drop for FontFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn japanese_font_bytes() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf")).unwrap()
    }

    #[test]
    fn a_japanese_font_does_not_count_as_the_chinese_fallback() {
        // #3376: characters Japanese shares with Chinese drew, simplified-only ones were boxes.
        let japanese = japanese_font_bytes();
        let face = ttf_parser::Face::parse(&japanese, 0).unwrap();
        for shared in "文件原点".chars() {
            assert!(face.glyph_index(shared).is_some(), "BIZ UDPGothic draws {shared}");
        }
        assert!(face.glyph_index(SIMPLIFIED_CHINESE_PROBE).is_none(), "BIZ UDPGothic lacks simplified 设");
        assert!(!draws_simplified_chinese(&japanese));
        assert!(!draws_simplified_chinese(&latin_font_bytes()));
        assert!(!draws_simplified_chinese(b"not a font"));
    }

    #[test]
    fn the_chinese_font_search_scans_font_folders_and_checks_what_the_font_draws() {
        // #3376: a Chinese font in a font folder but at none of the fixed paths, such as
        // /usr/share/fonts/truetype/wqy/wqy-microhei.ttc, is found by the folder scan, three
        // folders deep, on every desktop system.
        let folder = FontFolder::new("chinese-scan");
        let wqy = folder.0.join("truetype").join("wqy");
        std::fs::create_dir_all(&wqy).unwrap();
        std::fs::write(folder.0.join("notes.txt"), "not a font").unwrap();
        std::fs::write(wqy.join("wqy-broken.ttc"), "not a font either").unwrap();
        // The Japanese font under a Chinese font's name: the scan reaches it…
        let japanese = japanese_font_bytes();
        std::fs::write(wqy.join("wqy-microhei.ttc"), &japanese).unwrap();
        assert_eq!(scan_dir_for_font_where(&folder.0, 3, is_chinese_font_name, |_| true), Some(japanese.clone()));
        assert_eq!(scan_dir_for_font_where(&folder.0, 2, is_chinese_font_name, |_| true), None, "three folders deep, no deeper");
        // …but it doesn't draw simplified Chinese, so it isn't taken as the Chinese fallback.
        assert_eq!(find_chinese_font_in(std::slice::from_ref(&folder.0)), None);
        assert_eq!(find_chinese_font_in(&[folder.0.join("missing")]), None);
    }

    #[test]
    fn web_chinese_subset_draws_simplified_settings() {
        let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/craft-ui-sc/CraftUI-SC.otf")).to_vec();
        let ctx = test_context();
        assert!(install_web_script_fonts(&ctx, None, Some(bytes), "zh"));
        let installed = ctx.data(|d| d.get_temp::<FontDefinitions>(installed_fonts_id())).unwrap();
        for family in sans_families() {
            assert_eq!(installed.families[&family].first().map(String::as_str), Some(CHINESE_FALLBACK));
        }
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&semibold(14.0), "设置")));
    }

    /// The web shell fetches the Chinese face for a Chinese UI and the Japanese one for Japanese
    /// and Korean (#8894).
    #[test]
    fn the_web_shell_fetches_the_face_each_cjk_language_needs() {
        assert_eq!(web_script_font_kind("zh"), Some("zh"));
        assert_eq!(web_script_font_kind("zh-TW"), Some("zh-TW"));
        assert_eq!(web_script_font_kind("ja"), Some("ja"));
        assert!(needs_chinese_font("zh"));
        assert!(!needs_chinese_font("ja"));
    }
}
