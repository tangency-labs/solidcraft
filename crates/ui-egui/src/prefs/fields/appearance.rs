//! Stored preferences on the Appearance page (`PAGE_APPEARANCE`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! appearance_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// A picture behind the model in place of the view's background; empty for none.
            background_image: String = String::new(), PAGE_APPEARANCE;
            /// Colour vision mode (#778): `off`, `protan`, `deutan` or `tritan`.
            color_vision: solidcraft_theme::ColorVision = solidcraft_theme::ColorVision::Off, PAGE_APPEARANCE;
            /// The application bar's logo in the theme's accent instead of the theme's logo colours.
            logo_accent: bool = false, PAGE_APPEARANCE;
            /// `dark`, `light` or `system` (follow the operating system), or a theme file's id: a
            /// built-in one (`gruvbox-dark`, …) or one of `custom_themes` (#2255).
            theme: String = "dark".into(), PAGE_APPEARANCE;
            /// Theme used when the operating system is light; unknown ids resolve to Light.
            theme_day: String = "light".into(), PAGE_APPEARANCE;
            /// Desktop: a `solidcraft-theme/1` file another tool writes, followed as the theme and
            /// reloaded when it changes (#7573). Empty (the default): no file is followed.
            theme_follow: String = String::new(), PAGE_APPEARANCE;
            /// Theme used when the operating system is dark; unknown ids resolve to Dark.
            theme_night: String = "dark".into(), PAGE_APPEARANCE;
            /// Per-theme overrides. Raw JSON retains entries from newer or hand-edited settings.
            theme_options: BTreeMap<String, BTreeMap<String, serde_json::Value>> = BTreeMap::new(), PAGE_APPEARANCE;
            /// Interface font: an installed font family (or a font file), or empty for the built-in one
            /// (#1290). Menus, dialogs and labels use it; glyphs it lacks fall back as before.
            ui_font: String = String::new(), PAGE_APPEARANCE;
            /// Interface scale (1 = 100 %).
            ui_scale: f32 = 1.0, PAGE_APPEARANCE, accept(0.5..=2.5);
            /// The 3D view's background, one colour for the whole application (#7688), or `None` (the
            /// default) for the theme's gradient.
            viewport_background: Option<[u8; 3]> = None, PAGE_APPEARANCE;
            /// The 3D view's colours: the theme's own, or the Light or Dark theme's whatever the window
            /// uses (#4146).
            viewport_theme: crate::theme::ViewportTheme = crate::theme::ViewportTheme::Theme, PAGE_APPEARANCE;
        }
    };
}
