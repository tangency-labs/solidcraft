use crate::ui_kit::*;
use egui::Ui;

/// A pressed letter as the platform delivers it: `Event::Text` carries the typed glyph, which is
/// what makes the accelerator layout-independent (#9841).
fn letter(ch: &str) -> Vec<egui::Event> {
    vec![egui::Event::Text(ch.to_owned())]
}

fn prompt_frame(ctx: &egui::Context, events: Vec<egui::Event>, valid: bool, labels: (&str, &str, Option<&str>)) -> Action {
    let mut action = Action::None;
    let mut draw = |ui: &mut Ui| {
        let (ok, cancel, left) = labels;
        action = Confirm::new(ui.id().with("prompt"), "status_warning", "Save changes?")
            .tone(Tone::Warning)
            .actions(ok, cancel, left)
            .secondary_left()
            .show(ui.ctx(), valid, |ui| {
                text(ui, "Your changes will be lost.", Text::Report("message", false));
            })
            .1;
    };
    // An egui Area spends its first frame on an invisible sizing pass, so the modal only
    // answers keys from its second frame on — the same warm-up the panel tests step through.
    crate::test_support::frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
            focused: true,
            events: vec![],
            ..Default::default()
        },
        &mut draw,
    );
    crate::test_support::frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
            focused: true,
            events,
            ..Default::default()
        },
        &mut draw,
    );
    action
}

/// Each button answers its own first letter, in either case; an unmatched letter answers nothing.
#[test]
fn confirm_answers_the_first_letter_of_each_button_label() {
    let ctx = crate::fonts::test_context();
    let labels = ("Save", "Cancel", Some("Don't Save"));
    assert_eq!(prompt_frame(&ctx, letter("s"), true, labels), Action::Ok, "Save");
    assert_eq!(prompt_frame(&ctx, letter("c"), true, labels), Action::Cancel, "Cancel");
    assert_eq!(prompt_frame(&ctx, letter("d"), true, labels), Action::Apply, "Don't Save");
    assert_eq!(prompt_frame(&ctx, letter("S"), true, labels), Action::Ok, "the letter matches whatever its case");
    assert_eq!(prompt_frame(&ctx, letter("x"), true, labels), Action::None, "no label starts with x");
}

/// The accelerator follows the translated label: German shows „Speichern/Abbrechen/Nicht
/// speichern", so the keys are S, A and N — and the English D answers nothing.
#[test]
fn confirm_first_letter_follows_the_translated_label() {
    let _language = crate::i18n::scope("de");
    let ctx = crate::fonts::test_context();
    let labels = (crate::i18n::tr("ui.save"), crate::i18n::tr("ui.cancel"), Some(crate::i18n::tr("ui.don_t_save")));
    assert_eq!(labels, ("Speichern", "Abbrechen", Some("Nicht speichern")), "the catalog answers in German");
    assert_eq!(prompt_frame(&ctx, letter("s"), true, labels), Action::Ok, "Speichern");
    assert_eq!(prompt_frame(&ctx, letter("a"), true, labels), Action::Cancel, "Abbrechen");
    assert_eq!(prompt_frame(&ctx, letter("n"), true, labels), Action::Apply, "Nicht speichern");
    assert_eq!(prompt_frame(&ctx, letter("d"), true, labels), Action::None, "no German label starts with d");
}

/// Two visible buttons sharing a first letter make the key ambiguous: it answers nothing, while
/// a letter that still distinguishes one button keeps working.
#[test]
fn ambiguous_first_letters_do_nothing() {
    let ctx = crate::fonts::test_context();
    let labels = ("Delete", "Discard", Some("Keep"));
    assert_eq!(prompt_frame(&ctx, letter("d"), true, labels), Action::None, "Delete and Discard both start with d");
    assert_eq!(prompt_frame(&ctx, letter("k"), true, labels), Action::Apply, "Keep is still unambiguous");
}

/// The matched keystroke is consumed: the next frame sees no event and does not answer twice,
/// and Enter and Escape keep their existing meanings even alongside a letter.
#[test]
fn first_letter_is_consumed_and_never_overrides_enter_or_escape() {
    let ctx = crate::fonts::test_context();
    let labels = ("Save", "Cancel", Some("Don't Save"));
    assert_eq!(prompt_frame(&ctx, letter("s"), true, labels), Action::Ok);
    assert_eq!(prompt_frame(&ctx, vec![], true, labels), Action::None, "the consumed letter does not answer again");
    let mut events = crate::test_support::key(egui::Key::Enter);
    events.extend(letter("c"));
    assert_eq!(prompt_frame(&ctx, events, true, labels), Action::Ok, "Enter still picks the selected button");
    let mut events = crate::test_support::key(egui::Key::Escape);
    events.extend(letter("s"));
    assert_eq!(prompt_frame(&ctx, events, true, labels), Action::Cancel, "Escape still cancels");
}

/// A text field inside the dialog (preset and rule name prompts) owns the keystroke while focused.
#[test]
fn first_letter_yields_to_a_text_field_inside_the_dialog() {
    let ctx = crate::fonts::test_context();
    let mut name = String::new();
    let mut action = Action::None;
    let mut draw = |ui: &mut Ui| {
        action = Confirm::new(ui.id().with("name"), "save", "Save preset?")
            .actions("Save", "Cancel", None)
            .show(ui.ctx(), true, |ui| {
                ui.text_edit_singleline(&mut name).request_focus();
            })
            .1;
    };
    crate::test_support::frame(
        &ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
            focused: true,
            events: vec![],
            ..Default::default()
        },
        &mut draw,
    );
    crate::test_support::frame(
        &ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
            focused: true,
            events: letter("s"),
            ..Default::default()
        },
        &mut draw,
    );
    assert_eq!(action, Action::None, "the focused text field keeps the keystroke");
}

/// A disabled button swallows its letter instead of letting it reach the app behind the dialog.
#[test]
fn first_letter_respects_disabled_buttons() {
    let ctx = crate::fonts::test_context();
    let labels = ("Save", "Cancel", Some("Don't Save"));
    assert_eq!(prompt_frame(&ctx, letter("s"), false, labels), Action::None, "Save is disabled while inputs are invalid");
    assert_eq!(prompt_frame(&ctx, vec![], false, labels), Action::None, "the swallowed letter did not linger");
    assert_eq!(prompt_frame(&ctx, letter("c"), false, labels), Action::Cancel, "Cancel is always live");
}
