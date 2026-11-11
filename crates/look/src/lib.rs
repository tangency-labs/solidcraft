//! SolidCraft's look, ready to install into the UI kit (`solidcraft_ui_kit`): its icon masters and
//! colour roles ([`icons`]), its code-drawn glyphs ([`glyphs`]), its IBM Plex interface faces
//! ([`fonts`]) and its built-in themes ([`themes`]). The kit ships none of these; a host that wants
//! SolidCraft's look installs them from here, so there is one copy of each.
//!
//! [`install`] installs all of it. A host that wants only some, or SolidCraft's icons with a few
//! masters of its own ([`icons::DATA`] in a `solidcraft_ui_kit::icons::IconSet::extended`),
//! installs the parts instead. SolidCraft's workspace tabs are the app's, not the look's.

pub mod compass;
pub mod fonts;
pub mod glyphs;
pub mod icons;
pub mod pointer;
pub mod themes;

/// Install SolidCraft's look: [`install_tables`] and its fonts into `ctx`. Call it before the
/// first frame and in every test context.
pub fn install(ctx: &egui::Context) {
    install_tables();
    fonts::install(ctx);
}

/// Install the look's process-wide tables into the kit: its icon set, glyphs and built-in themes,
/// once per process (later calls find them installed), before preferences are read or the first
/// frame. A host that composes its own fonts on [`fonts::definitions`] calls this and installs
/// those.
pub fn install_tables() {
    icons::install();
    glyphs::install();
    themes::install();
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
