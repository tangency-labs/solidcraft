//! With no catalogs installed (this test binary installs none), every id reads as itself.

use solidcraft_ui_text as text;

#[test]
fn with_nothing_installed_every_id_reads_as_itself() {
    assert!(text::locales().is_empty());
    assert_eq!(text::tr("ui.cancel"), "ui.cancel");
    assert_eq!(text::tr_label("Profile"), "Profile");
    assert_eq!(text::format("dialog.selected_count", &[("count", "2")]), "dialog.selected_count");
    assert_eq!(text::Locale::for_language("de").text("ui.cancel"), "ui.cancel");
    assert_eq!(text::language(), "en");
    assert_eq!(text::effective_language(text::SYSTEM), "en");
    assert!(!text::is_supported("en"));
    let _de = text::scope("de");
    assert_eq!(text::language(), "en", "no catalog for de: English");
}
