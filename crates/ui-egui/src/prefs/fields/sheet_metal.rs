//! Stored preferences on the Sheet Metal page (`PAGE_SHEET_METAL`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it.

macro_rules! sheet_metal_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// The sheet-metal rule that new designs get; the Sheet Metal page picks it from the
            /// design's and the personal rules. `Steel (mm)` until the user picks another.
            sheet_rule: solidcraft_engine::doc::sheet::SheetRule = solidcraft_engine::doc::sheet::SheetRule::steel(), PAGE_SHEET_METAL;
            /// The units the flat-pattern DXF is exported in: `mm` (the default) or `inches`.
            sheet_dxf_units: String = "mm".into(), PAGE_SHEET_METAL;
            /// Whether the exported flat-pattern DXF keeps the bend lines (BEND_UP, BEND_DOWN).
            sheet_export_bend_lines: bool = true, PAGE_SHEET_METAL => check_id_help("prefs.sheet_export_bend_lines", "prefs.sheet_export_bend_lines_help");
            /// Whether the exported flat-pattern DXF keeps the markings (the letters on TEXT/ETCH
            /// and the bend notes).
            sheet_export_markings: bool = true, PAGE_SHEET_METAL => check_id_help("prefs.sheet_export_markings", "prefs.sheet_export_markings_help");
        }
    };
}
