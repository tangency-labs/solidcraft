//! The Theme panel: pick any built-in theme (the theme crate's Light and Dark and the look's theme
//! files), edit its main tokens live, reset them, and copy the result as a `solidcraft-theme/1`
//! theme file, the format SolidCraft imports (Preferences › Appearance › Import theme file).
//!
//! The edited tokens reach every example through `solidcraft_theme::scoped_tokens` (the gallery's
//! frame), the same override the kit's own tests draw other palettes with.

use egui::{Color32, Ui};
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as kit;
use solidcraft_ui_kit::theme_files::{self, Base};

/// A theme the panel offers: its name, base and resolved tokens and icon palette.
pub struct Theme {
    pub id: String,
    pub name: String,
    pub base: Base,
    pub tokens: Tokens,
    pub icons: Option<kit::icons::Palette>,
}

/// The theme being shown and the edits on top of it.
pub struct ThemeEditor {
    pub themes: Vec<Theme>,
    pub choice: usize,
    pub dark: bool,
    /// The tokens every example draws with: the chosen theme's, as edited.
    pub tokens: Tokens,
    /// What was last applied to the context's visuals and icon palette.
    applied: Option<(usize, bool, Tokens)>,
    copied: bool,
}

impl Default for ThemeEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeEditor {
    /// Every built-in theme, starting on Dark (SolidCraft's default).
    pub fn new() -> Self {
        let mut themes = vec![
            Theme { id: "light".into(), name: "Light".into(), base: Base::Light, tokens: Tokens::light(), icons: None },
            Theme { id: "dark".into(), name: "Dark".into(), base: Base::Dark, tokens: Tokens::dark(), icons: None },
        ];
        let none = std::collections::BTreeMap::new();
        themes.extend(theme_files::builtin().iter().map(|file| Theme {
            id: file.id.clone(),
            name: file.name.clone(),
            base: file.base,
            tokens: theme_files::resolve(file),
            icons: theme_files::resolve_icon_palette(file, &none),
        }));
        Self { themes, choice: 1, dark: true, tokens: Tokens::dark(), applied: None, copied: false }
    }

    /// Show theme `index` as it ships, dropping any edits.
    pub fn choose(&mut self, index: usize) {
        if let Some(theme) = self.themes.get(index) {
            self.choice = index;
            self.dark = theme.base.is_dark();
            self.tokens = theme.tokens;
            self.copied = false;
        }
    }

    /// The top bar's Light/Dark switch: the plain Light or Dark theme, unless the theme shown
    /// already has that base.
    pub fn choose_tone(&mut self, dark: bool) {
        if self.dark != dark {
            self.choose(usize::from(dark));
        }
    }

    /// The tokens as the chosen theme ships them.
    pub fn shipped(&self) -> Tokens {
        self.themes.get(self.choice).map_or_else(|| if self.dark { Tokens::dark() } else { Tokens::light() }, |t| t.tokens)
    }

    pub fn edited(&self) -> bool {
        self.tokens != self.shipped()
    }

    /// Bring the context's visuals and the icon palette up to the theme shown. Call it inside the
    /// frame's `scoped_tokens`, after `set_dark`.
    pub fn apply(&mut self, ctx: &egui::Context) {
        let now = (self.choice, self.dark, self.tokens);
        if self.applied == Some(now) {
            return;
        }
        if self.applied.is_none_or(|(choice, dark, _)| choice != self.choice || dark != self.dark) {
            let icons = self.themes.get(self.choice).and_then(|t| t.icons);
            kit::icons::set_theme_palette(icons.map(|p| (self.dark, p)));
            kit::icons::clear_texture_cache(ctx);
        }
        solidcraft_theme::apply(ctx);
        self.applied = Some(now);
    }

    /// The edited theme as a `solidcraft-theme/1` file: every token, ready to import or to start
    /// a theme file from.
    pub fn export(&self) -> String {
        let (name, base, icons) = match self.themes.get(self.choice) {
            Some(t) => (t.name.as_str(), t.base, t.icons),
            None => ("Theme", if self.dark { Base::Dark } else { Base::Light }, None),
        };
        let name = if self.edited() { format!("{name} (edited)") } else { name.to_owned() };
        serde_json::to_string_pretty(&theme_files::export(&name, base, &self.tokens, icons)).unwrap_or_default()
    }

    /// `tokens` with every colour token that is `was` (at any opacity) changed to `now`, keeping
    /// each one's opacity: tokens a theme derives from one colour (the accent's primary buttons,
    /// focus rings and tints) follow it. Done through the theme file, which names every token.
    pub fn recolour(&self, tokens: Tokens, was: Color32, now: Color32) -> Tokens {
        let base = self.themes.get(self.choice).map_or(if self.dark { Base::Dark } else { Base::Light }, |t| t.base);
        let hex = |c: Color32| {
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            format!("#{r:02x}{g:02x}{b:02x}")
        };
        let (from, to) = (hex(was), hex(now));
        let mut file = theme_files::export("edit", base, &tokens, None);
        fn swap(value: &mut serde_json::Value, from: &str, to: &str) {
            match value {
                serde_json::Value::String(s) if s.len() >= 7 && s[..7].eq_ignore_ascii_case(from) => s.replace_range(..7, to),
                serde_json::Value::Array(items) => items.iter_mut().for_each(|v| swap(v, from, to)),
                serde_json::Value::Object(map) => map.values_mut().for_each(|v| swap(v, from, to)),
                _ => {}
            }
        }
        if let Some(colours) = file.get_mut("colors") {
            swap(colours, &from, &to);
        }
        match theme_files::from_value(&file) {
            Ok(theme) => {
                // A theme file holds colours, radii and borders; the sizes edited here stay.
                let mut recoloured = theme_files::resolve(&theme);
                recoloured.control_height = tokens.control_height;
                for (to, from) in [
                    (&mut recoloured.space_2, tokens.space_2),
                    (&mut recoloured.space_3, tokens.space_3),
                    (&mut recoloured.space_4, tokens.space_4),
                    (&mut recoloured.space_6, tokens.space_6),
                    (&mut recoloured.space_8, tokens.space_8),
                    (&mut recoloured.space_10, tokens.space_10),
                    (&mut recoloured.space_12, tokens.space_12),
                    (&mut recoloured.space_14, tokens.space_14),
                    (&mut recoloured.space_16, tokens.space_16),
                    (&mut recoloured.space_24, tokens.space_24),
                    (&mut recoloured.font_small, tokens.font_small),
                    (&mut recoloured.font_caption, tokens.font_caption),
                    (&mut recoloured.font_body, tokens.font_body),
                    (&mut recoloured.font_label, tokens.font_label),
                    (&mut recoloured.font_button, tokens.font_button),
                    (&mut recoloured.font_heading, tokens.font_heading),
                ] {
                    *to = from;
                }
                recoloured
            }
            Err(e) => {
                log::warn!("recolouring the theme: {e}");
                tokens
            }
        }
    }

    /// The panel's contents.
    pub fn panel(&mut self, ui: &mut Ui) {
        let t = Tokens::get();
        kit::section_heading(ui, "Theme");
        let names: Vec<&str> = self.themes.iter().map(|t| t.name.as_str()).collect();
        let mut choice = self.choice;
        kit::row(ui, "Built-in", |ui| kit::dropdown(ui, "gallery_theme", &mut choice, &names));
        if choice != self.choice {
            self.choose(choice);
        }
        kit::text(ui, "Every example on every page draws with these tokens.", kit::Text::Note("theme_note"));
        let mut tokens = self.tokens;
        let mut recoloured = None;
        kit::section(ui, "theme_colours", "Colours", true, |ui| {
            kit::text(
                ui,
                "A colour carries the tokens that share it: the accent recolours primary buttons, focus and selection too.",
                kit::Text::Note("theme_colours_note"),
            );
            for (label, colour) in [
                ("Accent", &mut tokens.accent),
                ("Accent soft", &mut tokens.accent_soft),
                ("On accent", &mut tokens.on_accent),
                ("Panel", &mut tokens.panel),
                ("Header", &mut tokens.panel_header),
                ("Dialog", &mut tokens.dialog_bg),
                ("App bar", &mut tokens.app_bar),
                ("Control", &mut tokens.control_fill),
                ("Field", &mut tokens.field),
                ("Border", &mut tokens.border),
                ("Ink", &mut tokens.text),
                ("Dim ink", &mut tokens.text_dim),
                ("Error", &mut tokens.error),
                ("Warning", &mut tokens.warning),
            ] {
                let was = *colour;
                colour_row(ui, label, colour);
                if *colour != was {
                    recoloured = Some((was, *colour));
                }
            }
        });
        if let Some((was, now)) = recoloured {
            tokens = self.recolour(tokens, was, now);
        }
        kit::section(ui, "theme_shape", "Shape", true, |ui| {
            radius_row(ui, "Control radius", &mut tokens.control_radius);
            radius_row(ui, "Radius small", &mut tokens.radius_sm);
            radius_row(ui, "Radius medium", &mut tokens.radius_md);
            radius_row(ui, "Radius large", &mut tokens.radius_lg);
            size_row(ui, "Control height", &mut tokens.control_height, 16.0..=48.0);
        });
        kit::section(ui, "theme_space", "Spacing", false, |ui| {
            for (label, space) in [
                ("space_2", &mut tokens.space_2),
                ("space_3", &mut tokens.space_3),
                ("space_4", &mut tokens.space_4),
                ("space_6", &mut tokens.space_6),
                ("space_8", &mut tokens.space_8),
                ("space_10", &mut tokens.space_10),
                ("space_12", &mut tokens.space_12),
                ("space_14", &mut tokens.space_14),
                ("space_16", &mut tokens.space_16),
                ("space_24", &mut tokens.space_24),
            ] {
                size_row(ui, label, space, 0.0..=40.0);
            }
        });
        kit::section(ui, "theme_type", "Type sizes", false, |ui| {
            for (label, size) in [
                ("Small", &mut tokens.font_small),
                ("Caption", &mut tokens.font_caption),
                ("Body", &mut tokens.font_body),
                ("Label", &mut tokens.font_label),
                ("Button", &mut tokens.font_button),
                ("Heading", &mut tokens.font_heading),
            ] {
                size_row(ui, label, size, 8.0..=24.0);
            }
        });
        if tokens != self.tokens {
            self.tokens = tokens;
            self.copied = false;
        }
        ui.add_space(t.space_8);
        ui.horizontal(|ui| {
            if kit::button(ui, "Reset", kit::Button::Enabled(self.edited())).clicked() {
                self.tokens = self.shipped();
            }
            if kit::button(ui, "Copy theme file", kit::Button::Primary).clicked() {
                ui.ctx().copy_text(self.export());
                self.copied = true;
            }
        });
        if self.copied {
            kit::status_line(ui, "Copied. Import it in SolidCraft under Preferences › Appearance › Import theme file.", kit::StatusTone::Dim);
        }
        ui.add_space(t.space_8);
        kit::section(ui, "theme_file", "Theme file", false, |ui| {
            kit::read_only_text(ui, &self.export());
        });
    }
}

fn colour_row(ui: &mut Ui, label: &str, colour: &mut Color32) {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    let mut rgb = [r, g, b];
    kit::row(ui, label, |ui| kit::color_field(ui, &mut rgb));
    if rgb != [r, g, b] {
        *colour = Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a);
    }
}

fn radius_row(ui: &mut Ui, label: &str, radius: &mut u8) {
    kit::row(ui, label, |ui| kit::slider(ui, radius, 0..=16, kit::Slider { integer: true, ..Default::default() }));
}

fn size_row(ui: &mut Ui, label: &str, size: &mut f32, range: std::ops::RangeInclusive<f32>) {
    kit::row(ui, label, |ui| kit::slider(ui, size, range, kit::Slider { step: Some(0.5), ..Default::default() }));
}
