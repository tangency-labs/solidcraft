//! SolidCraft's built-in themes: the theme files in `themes/`, embedded at build time (`build.rs`),
//! for the kit's theme files (`solidcraft_ui_kit::theme_files`). Their per-tab surfaces (`tabs`) are
//! keyed by SolidCraft's tab names; under a host whose tab table lacks one, that tab's surfaces are
//! left out and the theme loads.

// `BUILTIN`, `(id, JSON text)` sorted by id.
include!(concat!(env!("OUT_DIR"), "/builtin_themes.rs"));

/// Install [`BUILTIN`] as the kit's built-in themes, once per process, before preferences are read
/// or the first frame; later calls find them installed.
pub fn install() {
    solidcraft_ui_kit::theme_files::install_builtin(BUILTIN);
}
