//! The theme system's glue on the token contract (`solidcraft_theme`): a theme file applied for its base with its icon
//! palette, its per-tab surfaces resolved, faded and published each frame, the blur it offers, and the 3D view's
//! colours preference. Theme files themselves are `theme_files`, their options `theme_options`.

use std::collections::BTreeMap;

use egui::Color32;
use serde::{Deserialize, Serialize};
use solidcraft_theme::{
    Custom, Environment, Fade, PublishedTab, Surfaces, TAB_NAMES, TabAccent, Tokens, is_dark, set_custom, set_published, viewport_dark, vision,
};

#[derive(Clone)]
struct PreparedTheme {
    file: crate::theme_files::ThemeFile,
    raw: Option<BTreeMap<String, serde_json::Value>>,
    values: BTreeMap<String, crate::theme_files::OptionValue>,
    base: FilePalette,
    enabled: bool,
    fade_ms: f32,
}

#[derive(Clone, Default)]
struct TabThemeState {
    blur: BlurIntent,
    prepared: Option<PreparedTheme>,
    environment: Option<Environment>,
    targets: BTreeMap<String, TabTarget>,
    tab: String,
    shown: Option<Surfaces>,
    fade: Option<Fade>,
    /// Ownership of the two egui fills survives target/theme invalidation until restored.
    styled_surfaces: bool,
    #[cfg(test)]
    resolutions: usize,
}

#[derive(Clone, Copy)]
struct TabTarget {
    tokens: Tokens,
    view: Tokens,
    accent: TabAccent,
}

const TAB_STATE: &str = "sc_tab_theme_state";

/// Only authored/effective intent is cached. Runtime OS/device availability is never stored
/// here. Reuse prepare_tabs' borrowed full-content/raw comparison, including followed reloads.
#[derive(Clone, Copy, Default)]
pub(crate) struct BlurIntent {
    pub offered: bool,
    pub radius: f32,
    pub revision: u64,
}

pub(crate) fn blur_intent(ctx: &egui::Context) -> BlurIntent {
    ctx.data_mut(|data| data.get_temp_mut_or_default::<TabThemeState>(egui::Id::unique(TAB_STATE)).blur)
}

/// Cache the authored theme and raw saved values before any effective-value normalization.
/// Followed-file reloads use full content equality, including edits that keep the same id/base.
/// The caller supplies its existing normalization contract only on a cache miss.
pub(crate) fn prepare_tabs(
    ctx: &egui::Context,
    file: Option<&crate::theme_files::ThemeFile>,
    raw: Option<&BTreeMap<String, serde_json::Value>>,
    values: impl FnOnce() -> BTreeMap<String, crate::theme_files::OptionValue>,
) -> Option<FilePalette> {
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<TabThemeState>(egui::Id::unique(TAB_STATE));
        let same = match (&state.prepared, file) {
            (Some(old), Some(file)) => old.file == *file && old.raw.as_ref() == raw,
            (None, None) => true,
            _ => false,
        };
        if !same {
            state.blur = BlurIntent { revision: state.blur.revision.wrapping_add(1), ..Default::default() };
            state.prepared = file.map(|file| {
                let values = values();
                state.blur.offered = crate::theme_options::declares_blur(file);
                if state.blur.offered {
                    state.blur.radius = crate::theme_options::blur_radius(file, &values, crate::theme_options::Availability::Available);
                }
                let (enabled, fade_ms) = crate::theme_options::tab_settings(file, &values);
                let base = FilePalette {
                    dark: file.base.is_dark(),
                    tokens: crate::theme_files::resolve_with(file, &values),
                    icons: crate::theme_files::resolve_icon_palette(file, &values),
                };
                PreparedTheme { file: file.clone(), raw: raw.cloned(), values, base, enabled, fade_ms }
            });
            state.environment = None;
            state.targets.clear();
            // A theme/option change applies immediately; only tab changes animate.
            state.shown = None;
            state.fade = None;
            set_published(None);
        }
        state.prepared.as_ref().map(|p| p.base)
    })
}

impl TabThemeState {
    fn publish(&mut self, tab: &str, now: f64) -> (PublishedTab, TabAccent, bool) {
        let environment = Environment::current();
        if self.environment != Some(environment) {
            self.environment = Some(environment);
            self.targets.clear();
            self.shown = None;
            self.fade = None;
        }
        // Bound the cache even if an internal caller supplies an unknown tab name.
        let tab = if TAB_NAMES.contains(&tab) { tab } else { "MODEL" };
        // Borrow the key on resting frames; own a name only when caching a new target.
        let target = if let Some(target) = self.targets.get(tab) {
            *target
        } else {
            #[cfg(test)]
            {
                self.resolutions += 1;
            }
            let window = if let Some(prepared) = &self.prepared {
                environment.window(crate::theme_files::resolve_for_tab(&prepared.file, &prepared.values, Some(tab)))
            } else {
                Tokens::themed(environment.dark, environment.vision)
            };
            let accent = TabAccent::for_tab(&window, tab);
            let window = if viewport_dark() == is_dark() { window } else { window.with_viewport(Tokens::themed(viewport_dark(), vision())) };
            let target = TabTarget {
                tokens: window.with_background(environment.background),
                view: Tokens::themed(viewport_dark(), vision()).with_background(environment.background),
                accent,
            };
            self.targets.insert(tab.to_owned(), target);
            target
        };
        let destination = Surfaces::of(target.tokens);
        // Sample the old fade before retargeting, even when two tab changes happen in one frame.
        if let Some(fade) = self.fade {
            let (shown, running) = fade.sample(now);
            let mut current = target.tokens;
            shown.apply(&mut current);
            if running {
                current.protect_tab_panel();
            }
            self.shown = Some(Surfaces::of(current));
            if !running {
                self.fade = None;
            }
        }
        if self.tab != tab || self.shown.is_none() {
            let duration = self.prepared.as_ref().filter(|p| p.enabled).map_or(0.0, |p| p.fade_ms);
            self.fade = self.shown.filter(|from| *from != destination && duration > 0.0 && now.is_finite()).map(|from| Fade {
                from,
                to: destination,
                start: now,
                seconds: f64::from(duration) / 1000.0,
            });
            tab.clone_into(&mut self.tab);
            if self.fade.is_none() {
                self.shown = Some(destination);
            }
        }
        let mut tokens = target.tokens;
        self.shown.unwrap_or(destination).apply(&mut tokens);
        // Protect interpolated alpha too; opacity's readability floor applies throughout a fade.
        if self.fade.is_some() {
            tokens.protect_tab_panel();
        }
        self.shown = Some(Surfaces::of(tokens));
        let mut view = if viewport_dark() == is_dark() { tokens } else { target.view };
        view.viewport_top = tokens.viewport_top;
        view.viewport_bottom = tokens.viewport_bottom;
        let tab_surfaces = self.prepared.as_ref().is_some_and(|p| p.enabled && !p.file.tabs.is_empty());
        (PublishedTab { environment, tokens, view, tab_surfaces }, target.accent, self.fade.is_some())
    }
}

/// A theme file as the theme system applies it: its palette for its base (dark or light), and the icon palette it
/// gives the icon renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilePalette {
    pub dark: bool,
    pub tokens: Tokens,
    pub icons: Option<crate::icon_svg::Palette>,
}

/// Apply a theme file's palettes ([`solidcraft_theme::set_custom`] and the icon renderer's), or the built-in ones (`None`). Returns
/// whether that changed anything (the visuals follow on the next [`solidcraft_theme::apply`]). The app applies a theme
/// through this only: `set_custom` alone would keep the previous theme's icon colours.
pub fn set_file_palette(file: Option<FilePalette>) -> bool {
    let tokens = set_custom(file.map(|f| Custom { dark: f.dark, tokens: f.tokens }));
    let icons = crate::icon_svg::set_theme_palette(file.and_then(|f| f.icons.map(|icons| (f.dark, icons))));
    tokens | icons
}

/// Isolated tests draw with a theme file's palettes, restored even when an assertion fails.
#[cfg(test)]
pub(crate) fn scoped_file(file: &crate::theme_files::ThemeFile) -> impl Drop {
    struct Restore(Option<Custom>, Option<(bool, crate::icon_svg::Palette)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_custom(self.0);
            crate::icon_svg::set_theme_palette(self.1);
        }
    }
    let restore = Restore(solidcraft_theme::custom(), crate::icon_svg::theme_palette_state());
    let values = BTreeMap::new();
    let tokens = crate::theme_files::resolve_with(file, &values);
    set_file_palette(Some(FilePalette { dark: is_dark(), tokens, icons: crate::theme_files::resolve_icon_palette(file, &values) }));
    restore
}

/// Publish the tab palettes and the accent of `tab` for this frame. `sketch_open` is whether a sketch is being
/// edited, `tool_active` whether a tool or dialog is open; both only steer focus painting.
pub fn publish_tab(tab: &str, sketch_open: bool, tool_active: bool, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let (published, accent, fading, style_surfaces) = ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<TabThemeState>(egui::Id::unique(TAB_STATE));
        let (published, accent, fading) = state.publish(tab, now);
        let style_surfaces = published.tab_surfaces || state.styled_surfaces;
        state.styled_surfaces = published.tab_surfaces;
        (published, accent, fading, style_surfaces)
    });
    set_published(Some(published));
    if style_surfaces {
        let fills = {
            let style = ctx.global_style();
            (style.visuals.panel_fill, style.visuals.window_fill)
        };
        if fills != (published.tokens.panel, published.tokens.panel) {
            ctx.global_style_mut(|style| {
                style.visuals.panel_fill = published.tokens.panel;
                style.visuals.window_fill = published.tokens.panel;
            });
        }
    }
    if fading {
        ctx.request_repaint();
    }
    accent.set_current(tab, sketch_open, tool_active, ctx);
}

/// The common conversion boundary for compiled icon palette data.
pub(crate) const fn icon_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// The 3D view's colours (#4146): the theme's own, or the built-in Light or Dark theme's whatever
/// the window uses, so a dark window can show the model on a light view and the other way round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportTheme {
    #[default]
    Theme,
    Light,
    Dark,
}

impl ViewportTheme {
    pub const ALL: [ViewportTheme; 3] = [ViewportTheme::Theme, ViewportTheme::Light, ViewportTheme::Dark];

    /// The saved value (`ui.prefs` `viewport_theme`).
    pub fn id(self) -> &'static str {
        match self {
            ViewportTheme::Theme => "theme",
            ViewportTheme::Light => "light",
            ViewportTheme::Dark => "dark",
        }
    }

    pub fn from_id(id: &str) -> Option<ViewportTheme> {
        ViewportTheme::ALL.into_iter().find(|m| m.id() == id)
    }

    /// Its name in the Preferences (a message id).
    pub fn label_id(self) -> &'static str {
        match self {
            ViewportTheme::Theme => "ui.viewport_theme_follow",
            ViewportTheme::Light => "ui.light",
            ViewportTheme::Dark => "ui.dark",
        }
    }

    /// The view's own dark or light, or `None` to draw it as the window ([`solidcraft_theme::set_viewport_dark`]).
    pub fn dark(self) -> Option<bool> {
        match self {
            ViewportTheme::Theme => None,
            ViewportTheme::Light => Some(false),
            ViewportTheme::Dark => Some(true),
        }
    }
}

/// From saved preferences: an unknown value is Theme, as [`solidcraft_theme::ColorVision`]'s is Off.
impl<'de> Deserialize<'de> for ViewportTheme {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str().and_then(ViewportTheme::from_id).unwrap_or_default())
    }
}

/// An sRGB colour carried by model data, including file colours and point-cloud shading.
pub(crate) const fn model_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use solidcraft_theme::{ColorVision, apply, composite, contrast_ratio, set_dark, set_viewport_background, set_viewport_dark, set_vision};

    fn tabs_state(base: &str, overrides: BTreeMap<String, crate::theme_files::OptionValue>) -> TabThemeState {
        let file = crate::theme_files::tab_fixture(base);
        let values = crate::theme_files::effective_values(&file, &overrides).0;
        let (enabled, fade_ms) = crate::theme_options::tab_settings(&file, &values);
        let base = FilePalette {
            dark: file.base.is_dark(),
            tokens: crate::theme_files::resolve_with(&file, &values),
            icons: crate::theme_files::resolve_icon_palette(&file, &values),
        };
        set_dark(base.dark);
        set_file_palette(Some(base));
        set_vision(ColorVision::Off);
        set_viewport_dark(None);
        set_viewport_background(None);
        TabThemeState { prepared: Some(PreparedTheme { file, values, raw: None, base, enabled, fade_ms }), ..Default::default() }
    }

    #[test]
    fn tabs_fade_retargets_from_displayed_surfaces_and_stops_resolving_at_rest() {
        for opacity in [40.0, 100.0] {
            let mut state = tabs_state("dark", BTreeMap::from([("opacity".into(), crate::theme_files::OptionValue::Num(opacity))]));
            let (model, _, repaint) = state.publish("MODEL", 0.0);
            assert!(!repaint);
            let (start, _, repaint) = state.publish("SKETCH", 1.0);
            assert!(repaint);
            assert_eq!(Surfaces::of(start.tokens), Surfaces::of(model.tokens));
            let (middle, _, _) = state.publish("SKETCH", 1.12);
            let (retarget, _, repaint) = state.publish("RENDER", 1.12);
            assert!(repaint);
            assert_eq!(Surfaces::of(retarget.tokens), Surfaces::of(middle.tokens));
            assert_eq!(retarget.view.viewport_top, retarget.tokens.viewport_top);
            assert_eq!(retarget.view, retarget.tokens);
            let (end, _, repaint) = state.publish("RENDER", 1.36);
            assert!(!repaint);
            assert_eq!(state.resolutions, 3);
            for frame in 0..20 {
                let (resting, _, repaint) = state.publish("RENDER", 2.0 + f64::from(frame));
                assert!(!repaint);
                assert_eq!(resting.tokens, end.tokens);
            }
            assert_eq!(state.resolutions, 3, "resting frames must hit the resolved-token cache");
            state.publish("MODEL", 30.0);
            assert_eq!(state.resolutions, 3, "returning to a tab reuses its target");
        }
    }

    #[test]
    fn tabs_panel_text_contrast_holds_at_five_fade_steps_with_opacity() {
        use crate::theme_files::OptionValue;
        for base in ["dark", "light"] {
            for percent in [40.0, 75.0, 100.0] {
                for mode in ColorVision::ALL {
                    for destination in ["SKETCH", "RENDER", "MODEL"] {
                        let mut state = tabs_state(base, BTreeMap::from([("opacity".into(), OptionValue::Num(percent))]));
                        set_vision(mode);
                        state.publish(if destination == "MODEL" { "RENDER" } else { "MODEL" }, 0.0);
                        state.publish(destination, 1.0);
                        for step in [0.0, 0.25, 0.5, 0.75, 1.0] {
                            let (frame, _, _) = state.publish(destination, 1.0 + step * 0.24);
                            let t = frame.tokens;
                            for behind in [Color32::BLACK, Color32::WHITE] {
                                let ground = composite(t.panel, behind);
                                for ink in [t.text, t.text_dim, t.form_ink, t.dialog_label] {
                                    assert!(
                                        contrast_ratio(composite(ink, ground), ground) >= 4.5,
                                        "{base} {mode:?} opacity {percent} tab {destination} step {step}"
                                    );
                                }
                            }
                            assert_eq!((frame.view.viewport_top, frame.view.viewport_bottom), (t.viewport_top, t.viewport_bottom));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn blur_intent_cache_resolves_only_when_theme_or_raw_values_change() {
        let ctx = egui::Context::default();
        let source: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/theme-blur.json")).unwrap();
        let mut file = crate::theme_files::from_value(&source).unwrap();
        let resolutions = Cell::new(0);
        let prepare = |file: Option<&crate::theme_files::ThemeFile>, raw: Option<&BTreeMap<String, serde_json::Value>>| {
            prepare_tabs(&ctx, file, raw, || {
                resolutions.set(resolutions.get() + 1);
                let saved = raw.into_iter().flatten().map(|(k, v)| (k.clone(), serde_json::from_value(v.clone()).unwrap())).collect();
                file.map_or_else(BTreeMap::new, |file| crate::theme_files::effective_values(file, &saved).0)
            });
        };
        for _ in 0..100 {
            prepare(None, None);
        }
        assert_eq!(resolutions.get(), 0, "builtins do not normalize option tables");
        prepare(Some(&file), None);
        let zero = blur_intent(&ctx);
        assert!(zero.offered);
        assert_eq!(zero.radius, 0.0);
        for _ in 0..100 {
            prepare(Some(&file), None);
        }
        assert_eq!(resolutions.get(), 1, "offered zero is a borrowed cache hit");
        assert_eq!(blur_intent(&ctx).revision, zero.revision);
        let raw = BTreeMap::from([("blur".into(), serde_json::json!(18.25))]);
        prepare(Some(&file), Some(&raw));
        assert_eq!(blur_intent(&ctx).radius, 18.25);
        assert_ne!(blur_intent(&ctx).revision, zero.revision);
        for _ in 0..100 {
            prepare(Some(&file), Some(&raw));
        }
        assert_eq!(resolutions.get(), 2);
        // Followed reload keeps id/base, but content changes: it must invalidate.
        file.options.retain(|o| o.id != "blur");
        prepare(Some(&file), Some(&raw));
        assert!(!blur_intent(&ctx).offered);
        assert_eq!(blur_intent(&ctx).radius, 0.0);
        for _ in 0..100 {
            prepare(Some(&file), Some(&raw));
        }
        assert_eq!(resolutions.get(), 3, "undeclared themes also reuse the borrowed identity");
    }

    #[test]
    fn tabs_cache_invalidates_for_theme_values_vision_and_viewport_overrides() {
        let ctx = egui::Context::default();
        let mut file = crate::theme_files::tab_fixture("dark");
        let normalizations = Cell::new(0);
        let prepare = |file: &crate::theme_files::ThemeFile, raw: Option<&BTreeMap<String, serde_json::Value>>| {
            let custom = prepare_tabs(&ctx, Some(file), raw, || {
                normalizations.set(normalizations.get() + 1);
                let saved = raw.into_iter().flatten().map(|(k, v)| (k.clone(), serde_json::from_value(v.clone()).unwrap())).collect();
                crate::theme_files::effective_values(file, &saved).0
            });
            set_dark(file.base.is_dark());
            set_file_palette(custom);
        };
        let publish = || ctx.data_mut(|d| d.get_temp_mut_or_default::<TabThemeState>(egui::Id::unique(TAB_STATE)).publish("SKETCH", 1.0));
        set_vision(ColorVision::Off);
        set_viewport_dark(None);
        set_viewport_background(None);
        prepare(&file, None);
        let before = publish().0.tokens;
        prepare(&file, None);
        assert_eq!(publish().0.tokens, before);
        assert_eq!(normalizations.get(), 1);
        // The content changes while identity/base remain the same, as with a followed reload.
        file.tabs.get_mut("SKETCH").unwrap().insert("viewport_top".into(), serde_json::json!("#1a4232"));
        prepare(&file, None);
        assert_ne!(publish().0.tokens.viewport_top, before.viewport_top);
        assert_eq!(normalizations.get(), 2);
        set_vision(ColorVision::Tritan);
        assert_eq!(publish().0.tokens.axis_x, before.with_vision(true, ColorVision::Tritan).axis_x);
        set_viewport_dark(Some(false));
        assert_eq!(publish().0.tokens.viewport_top, Tokens::light().viewport_top);
        set_viewport_background(Some([3, 7, 11]));
        assert_eq!(publish().0.tokens.viewport_top, Color32::from_rgb(3, 7, 11));
        set_viewport_background(None);
        set_viewport_dark(None);
        let raw = BTreeMap::from([("tab_colours".into(), serde_json::json!(false))]);
        prepare(&file, Some(&raw));
        assert_eq!(publish().0.tokens, Tokens::base());
        assert_eq!(normalizations.get(), 3);
    }

    #[test]
    fn tabs_publish_requests_repaints_only_during_fade_and_zero_is_instant() {
        use crate::theme_files::OptionValue;
        let ctx = crate::fonts::test_context();
        let mut state = tabs_state("dark", BTreeMap::new());
        state.publish("MODEL", 0.0);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(TAB_STATE), state));
        let frame = |time| {
            let mut out = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                publish_tab("SKETCH", false, false, ui.ctx());
                crate::gpu::paint_background(ui.painter(), ui.max_rect(), &Tokens::get());
            });
            let colors: Vec<_> = out
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => Some(mesh.vertices.iter().map(|vertex| vertex.color).collect()),
                    _ => None,
                })
                .unwrap();
            let tokens = Tokens::get();
            assert_eq!(colors, [tokens.viewport_top, tokens.viewport_top, tokens.viewport_bottom, tokens.viewport_bottom]);
            out.textures_delta.clear();
            out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        assert!(frame(1.0).is_zero());
        assert!(frame(1.12).is_zero());
        // Drain the previously requested pass, then verify the endpoint leaves the loop asleep.
        for time in [1.24, 1.3, 1.4] {
            frame(time);
        }
        assert_eq!(frame(2.0), std::time::Duration::MAX);
        let (end, _, _) = ctx.data_mut(|d| d.get_temp_mut_or_default::<TabThemeState>(egui::Id::unique(TAB_STATE)).publish("SKETCH", 2.0));
        assert_eq!(Tokens::get(), end.tokens);
        let mut instant = tabs_state("dark", BTreeMap::from([("tab_fade_ms".into(), OptionValue::Num(0.0))]));
        instant.publish("MODEL", 0.0);
        let (frame, _, repaint) = instant.publish("SKETCH", 1.0);
        assert!(!repaint);
        assert_eq!(
            frame.tokens,
            crate::theme_files::resolve_for_tab(&instant.prepared.as_ref().unwrap().file, &instant.prepared.as_ref().unwrap().values, Some("SKETCH"))
        );
    }

    #[test]
    fn built_in_selected_tab_labels_hold_wcag_small_text_contrast() {
        let mut themes = vec![("Dark", super::Tokens::dark()), ("Light", super::Tokens::light())];
        themes.extend(crate::theme_files::builtin().iter().map(|theme| (theme.name.as_str(), crate::theme_files::resolve(theme))));
        let mut failures = Vec::new();
        for (name, tokens) in themes {
            for tab in super::TAB_NAMES.iter().copied() {
                let accent = super::TabAccent::for_tab(&tokens, tab);
                let ratio = contrast_ratio(accent.ink, accent.soft);
                if ratio < 4.5 {
                    failures.push(format!("{name} {tab}: {ratio:.4}:1 ({:?} on {:?})", accent.ink, accent.soft));
                }
            }
        }
        assert!(failures.is_empty(), "selected tab contrast below 4.5:1:\n{}", failures.join("\n"));
    }

    #[test]
    fn tab_accent_context_is_per_app_and_does_not_change_widget_visuals() {
        let first = crate::fonts::test_context();
        let second = crate::fonts::test_context();
        let before = first.style_of(egui::Theme::Dark).visuals.clone();
        publish_tab("SURFACE", false, false, &first);
        assert_eq!(super::TabAccent::current(&first).color, super::Tokens::get().surface_fill);
        assert_eq!(super::TabAccent::current(&second).color, super::Tokens::get().accent);
        assert_eq!(super::TabAccent::current_icon_accent(&first), Some("surface"));
        assert_eq!(super::TabAccent::current_icon_accent(&second), None);
        assert_eq!(first.style_of(egui::Theme::Dark).visuals, before, "dialog OK, checkboxes and status styles must not inherit the tab");
        publish_tab("MODEL", false, false, &first);
        assert_eq!(super::TabAccent::current(&first).soft, super::Tokens::get().accent_soft);
        assert_eq!(super::TabAccent::current_icon_accent(&first), None);
    }

    #[test]
    fn tabs_surface_publication_restores_base_fills_without_restyling_widgets() {
        use crate::theme_files::{OptionValue, ThemeFile};
        for base in ["dark", "light"] {
            let ctx = crate::fonts::test_context();
            let file = crate::theme_files::tab_fixture(base);
            set_dark(file.base.is_dark());
            set_vision(ColorVision::Off);
            set_viewport_dark(None);
            set_viewport_background(None);
            set_file_palette(None);
            let prepare = |file: &ThemeFile, enabled: Option<bool>| {
                let raw = enabled.map(|enabled| BTreeMap::from([("tab_colours".into(), serde_json::json!(enabled))]));
                let custom = prepare_tabs(&ctx, Some(file), raw.as_ref(), || {
                    let saved = enabled.map(|enabled| BTreeMap::from([("tab_colours".into(), OptionValue::Bool(enabled))])).unwrap_or_default();
                    crate::theme_files::effective_values(file, &saved).0
                });
                set_file_palette(custom)
            };
            let publish = |tab: &str, time| {
                ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| publish_tab(tab, false, false, ui.ctx()))
                    .textures_delta
                    .clear();
            };
            prepare(&file, Some(true));
            apply(&ctx);
            let before = ctx.global_style().visuals.clone();
            let base_custom = solidcraft_theme::custom();
            publish("MODEL", 0.0);
            assert_eq!(ctx.global_style().visuals, before);
            publish("SKETCH", 1.0);
            publish("SKETCH", 1.12);
            let panel = Tokens::get().panel;
            assert_ne!(panel, before.panel_fill, "exercise an actual animated panel fill");
            let mut animated = before.clone();
            animated.panel_fill = panel;
            animated.window_fill = panel;
            assert_eq!(ctx.global_style().visuals, animated, "all non-surface widget/selection/status visuals stay unchanged");
            // Repair either owned fill independently, even when the panel already matches.
            ctx.global_style_mut(|style| style.visuals.window_fill = before.window_fill);
            publish("SKETCH", 1.12);
            assert_eq!(ctx.global_style().visuals, animated);

            assert!(!prepare(&file, Some(false)), "toggling surfaces does not change the base Custom tokens");
            assert_eq!(solidcraft_theme::custom(), base_custom);
            // Deliberately omit apply(): a full restyle would conceal stale animated fills.
            publish("SKETCH", 1.13);
            assert_eq!(ctx.global_style().visuals, before, "turning surfaces off must restore both base fills");

            assert!(!prepare(&file, Some(true)));
            publish("SKETCH", 2.0);
            assert_ne!(ctx.global_style().visuals.panel_fill, before.panel_fill);
            let mut plain = file.clone();
            plain.tabs.clear();
            plain.options.clear();
            assert!(!prepare(&plain, None), "a no-option theme can retain the exact same base tokens");
            assert_eq!(solidcraft_theme::custom(), base_custom);
            publish("SKETCH", 2.1);
            assert_eq!(ctx.global_style().visuals, before, "restoring a no-option theme must release the surface fills");
        }
    }
}
