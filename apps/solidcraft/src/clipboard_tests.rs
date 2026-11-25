//! The desktop build keeps the system clipboard.
//!
//! eframe 0.37 builds egui-winit without its clipboard unless asked, and the workspace turns
//! eframe's defaults off: every Copy (the error text, Copy diagnostic information, Ctrl+C in a
//! field) then reaches the platform as a command that nothing carries out. The UI tests see the
//! copied text in egui's output and still pass, so this is checked where the build is described.

#[test]
fn the_desktop_build_enables_eframes_clipboard() {
    let manifest = include_str!("../Cargo.toml");
    let eframe = manifest.lines().find(|line| line.trim_start().starts_with("eframe =")).expect("the eframe dependency");
    assert!(eframe.contains("\"clipboard\""), "eframe without its clipboard feature drops every copy on the desktop: {eframe}");
}
