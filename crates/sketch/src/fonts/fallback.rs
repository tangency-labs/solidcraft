//! The installed fonts tried for a script the chosen font lacks: one list per script, used by the
//! interface and by drawings. Each row is a family and the files it is known to be in, in the order
//! they are tried, so a lookup rarely has to read every font on the machine.

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
