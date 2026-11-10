//! Kit callers use shared controls; removed legacy implementations must stay removed.
use std::path::Path;
use syn::visit::{self, Visit};

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
}

/// No caller may bypass the kit for a matching control.
const ALLOWED: &[(&str, usize, &str)] = &[];

#[derive(Default)]
struct Guard {
    found: Vec<String>,
    aliases: Vec<String>,
}

impl<'ast> Visit<'ast> for Guard {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Fn(i) => &i.attrs,
            _ => return visit::visit_item(self, item),
        };
        if !attrs
            .iter()
            .any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
        {
            visit::visit_item(self, item);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if matches!(
            call.method.to_string().as_str(),
            "checkbox"
                | "button"
                | "small_button"
                | "selectable_label"
                | "selectable_value"
                | "radio_value"
                | "toggle_value"
                | "text_edit_singleline"
                | "text_edit_multiline"
        ) {
            self.found.push(format!(".{}", call.method));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
            // Loading an editor's cursor/focus state does not create a raw widget.
            let state_access = names.last().is_some_and(|n| matches!(n.as_str(), "load_state" | "store_state"));
            let kit_options = names.iter().any(|n| n == "ui_kit");
            if !state_access
                && !kit_options
                && names.iter().any(|n| {
                    matches!(n.as_str(), "ComboBox" | "Button" | "Slider" | "Checkbox" | "RadioButton" | "TextEdit" | "DragValue" | "ProgressBar")
                        || self.aliases.contains(n)
                })
            {
                self.found.push(names.join("::"));
            }
        }
        visit::visit_expr_call(self, call);
    }
}

fn violations(source: &str) -> Vec<String> {
    fn aliases(tree: &syn::UseTree, out: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => aliases(&p.tree, out),
            syn::UseTree::Group(g) => {
                for tree in &g.items {
                    aliases(tree, out);
                }
            }
            syn::UseTree::Rename(r)
                if matches!(
                    r.ident.to_string().as_str(),
                    "ComboBox" | "Button" | "Slider" | "Checkbox" | "RadioButton" | "TextEdit" | "DragValue" | "ProgressBar"
                ) =>
            {
                out.push(r.rename.to_string())
            }
            _ => {}
        }
    }
    let ast = syn::parse_file(source).unwrap();
    let mut guard = Guard::default();
    for item in &ast.items {
        if let syn::Item::Use(u) = item {
            aliases(&u.tree, &mut guard.aliases);
        }
    }
    guard.visit_file(&ast);
    guard.found
}

#[test]
fn all_ui_and_apps_use_ui_kit_controls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut failures = Vec::new();
    fn scan(dir: &Path, prefix: &str, failures: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            let relative = format!("{prefix}{name}");
            if path.is_dir() {
                if name != "ui_kit" {
                    scan(&path, &format!("{relative}/"), failures);
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || name.ends_with("_tests.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            // Scan every feature module, including ones that have not imported the kit yet.
            let found = violations(&source);
            let allowed = ALLOWED.iter().find(|(file, _, _)| *file == relative).map_or(0, |(_, count, _)| *count);
            if found.len() > allowed {
                failures.push(format!("{relative}: {} raw widgets, at most {allowed} allowed: {found:?}", found.len()));
            }
        }
    }
    scan(&root, "", &mut failures);
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps"), "apps/", &mut failures);
    failures.sort();
    assert!(failures.is_empty(), "Use crate::ui_kit controls:\n{}", failures.join("\n"));
}

#[test]
fn guard_detects_multiline_and_aliased_widget_calls_but_skips_tests() {
    assert_eq!(
        violations(
            r#"
        use egui::ComboBox as Combo;
        fn show(ui: &mut Ui) {
            ui.checkbox(
                value, "Label"
            );
            ui.button("Go");
            ui.selectable_label(false, "Item");
            ui.add(egui::Slider::new(value, range));
            ui.add(egui::Button::new("Go"));
            egui::ComboBox::from_id_salt("id");
            Combo::from_id_salt("alias");
        }
        #[cfg(test)] mod tests { fn sample() { ui.button("ignored"); } }
        fn after() { ui.small_button("Remove"); }
    "#
        )
        .len(),
        8
    );
}

// Declaration checks catch reintroduced implementations even before a caller reaches them.
fn legacy_declarations(source: &str, removed: &[&str]) -> Vec<String> {
    struct Declarations<'a> {
        removed: &'a [&'a str],
        found: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Declarations<'_> {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let name = item.sig.ident.to_string();
            if self.removed.contains(&name.as_str()) {
                self.found.push(name);
            }
            visit::visit_item_fn(self, item);
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            let name = item.sig.ident.to_string();
            if self.removed.contains(&name.as_str()) {
                self.found.push(name);
            }
            visit::visit_impl_item_fn(self, item);
        }
    }
    let mut guard = Declarations { removed, found: Vec::new() };
    guard.visit_file(&syn::parse_file(source).unwrap());
    guard.found
}

#[test]
fn removed_legacy_helpers_stay_removed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files: &[(&str, &[&str])] = &[
        ("ui_kit/container.rs", &["report"]),
        ("icons.rs", &["paint"]),
        ("preview.rs", &["draw_active"]),
        ("sketch_tools.rs", &["click_glyph"]),
        ("edit_clipboard.rs", &["can_copy"]),
        ("tool_modifiers.rs", &["ctrl_name", "alt_name", "matches_exact"]),
        ("fonts.rs", &["install_cjk_font"]),
        ("frame.rs", &["docked", "docked_on", "heading", "add_outlined", "content_width", "remember_width", "right_aligned"]),
        ("dialogs.rs", &["form_row", "publish_row", "is_wide"]),
        ("dialogs_appearance.rs", &["swatch", "swatch_rows"]),
        (
            "ui_kit/actions.rs",
            &["secondary_button", "small_button", "enabled_button", "footer_impl", "footer_with_status", "footer_selected", "icon_button"],
        ),
        ("ui_kit/choice.rs", &["segments", "segmented_states", "segmented_compact"]),
        ("ui_kit/fields.rs", &["selection_field_state", "value_pair_named", "formatted_slider", "timeline_slider"]),
        ("ui_kit/layout.rs", &["hint", "note", "text_block", "inline_text", "contains"]),
        ("ui_kit/table.rs", &["cell_text"]),
    ];
    for (file, removed) in files {
        let source = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(legacy_declarations(&source, removed).is_empty(), "{file}: removed helper reintroduced");
    }
    assert!(ALLOWED.is_empty(), "raw-control allowance table must stay empty");
}

#[test]
fn guard_detects_editor_constructors_aliases_and_legacy_declarations() {
    assert_eq!(
        violations(
            r#"
        use egui::TextEdit as Edit;
        fn draw(ui: &mut Ui) {
            ui.add(Edit::singleline(value));
            ui.add(egui::TextEdit::multiline(value));
            ui.add(egui::DragValue::new(value));
            ui.add(egui::ProgressBar::new(0.5));
            ui.text_edit_singleline(value);
            egui::TextEdit::load_state(ctx, id);
        }
    "#
        )
        .len(),
        5
    );
    assert_eq!(legacy_declarations("fn docked_on() {}", &["docked_on"]), ["docked_on"]);
    assert_eq!(legacy_declarations("impl Host { fn report() {} }", &["report"]), ["report"]);
}

/// Broader than the dense-panel guard: sliders and numeric fields have no caller exceptions,
/// including app entry points and files which do not already import the kit.
#[derive(Default)]
struct SliderGuard {
    aliases: Vec<String>,
    slider_aliases: Vec<String>,
    found: Vec<String>,
    constructors: usize,
    slider_constructors: usize,
    paints: bool,
    track: bool,
    thumb: bool,
}

impl<'ast> Visit<'ast> for SliderGuard {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if item.attrs.iter().any(|a| a.path().is_ident("test")) {
            return;
        }
        let mut function = Self { aliases: self.aliases.clone(), slider_aliases: self.slider_aliases.clone(), ..Self::default() };
        function.visit_block(&item.block);
        let name = item.sig.ident.to_string();
        if function.paints && (name.contains("slider") || (function.track && function.thumb)) {
            function.found.push(format!("{name}: slider track/thumb painting outside the kit"));
        }
        self.found.extend(function.found);
        self.constructors += function.constructors;
        self.slider_constructors += function.slider_constructors;
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !item.attrs.iter().any(|a| a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))) {
            visit::visit_item_mod(self, item);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if !names.iter().any(|n| n == "ui_kit") && names.iter().any(|n| matches!(n.as_str(), "Slider" | "DragValue") || self.aliases.contains(n))
            {
                self.constructors += 1;
                if names.iter().any(|n| n == "Slider" || self.slider_aliases.contains(n)) {
                    self.slider_constructors += 1;
                }
                self.found.push(names.join("::"));
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if matches!(method.as_str(), "rect" | "rect_filled" | "circle" | "circle_filled" | "line_segment" | "add" | "extend") {
            self.paints = true;
            // Only painting arguments identify a track/thumb. A panel's resize_rail call,
            // camera track_hold call or publish_handle call is not slider painting.
            #[derive(Default)]
            struct PaintNames {
                track: bool,
                thumb: bool,
            }
            impl<'ast> Visit<'ast> for PaintNames {
                fn visit_ident(&mut self, ident: &'ast syn::Ident) {
                    let name = ident.to_string().to_ascii_lowercase();
                    let words: Vec<_> = name.split('_').collect();
                    self.track |= words.iter().any(|w| matches!(*w, "track" | "rail"));
                    self.thumb |= words.iter().any(|w| matches!(*w, "thumb" | "knob" | "handle"));
                }
            }
            let mut names = PaintNames::default();
            for arg in &call.args {
                names.visit_expr(arg);
            }
            self.track |= names.track;
            self.thumb |= names.thumb || matches!(method.as_str(), "circle" | "circle_filled");
        }
        visit::visit_expr_method_call(self, call);
    }
}

fn slider_guard(source: &str) -> SliderGuard {
    let ast = syn::parse_file(source).unwrap();
    #[derive(Default)]
    struct Imports {
        widgets: Vec<String>,
        sliders: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Imports {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attrs = match item {
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Fn(i) => &i.attrs,
                _ => return visit::visit_item(self, item),
            };
            if !test_only(attrs) {
                visit::visit_item(self, item);
            }
        }
        fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
            if let syn::UseTree::Rename(rename) = tree
                && matches!(rename.ident.to_string().as_str(), "Slider" | "DragValue")
            {
                self.widgets.push(rename.rename.to_string());
                if rename.ident == "Slider" {
                    self.sliders.push(rename.rename.to_string());
                }
            }
            visit::visit_use_tree(self, tree);
        }
    }
    let mut imports = Imports::default();
    imports.visit_file(&ast);
    let mut guard = SliderGuard { aliases: imports.widgets, slider_aliases: imports.sliders, ..SliderGuard::default() };
    guard.visit_file(&ast);
    guard
}

#[test]
fn all_ui_and_apps_use_the_one_kit_slider_and_numeric_fields() {
    fn scan(dir: &Path, kit: &Path, failures: &mut Vec<String>, constructors: &mut usize) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, kit, failures, constructors);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                let guard = slider_guard(&source);
                if path.starts_with(kit) {
                    // One native slider constructor inside the kit, too: another kit helper must
                    // not quietly introduce a different appearance. Numeric field constructors
                    // are legitimate here and are counted separately below.
                    *constructors += guard.slider_constructors;
                } else if !guard.found.is_empty() {
                    failures.push(format!("{}: {:?}", path.display(), guard.found));
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let kit = root.join("src/ui_kit");
    let mut failures = Vec::new();
    let mut constructors = 0;
    scan(&root.join("src"), &kit, &mut failures, &mut constructors);
    scan(&root.join("../../apps"), &kit, &mut failures, &mut constructors);
    failures.sort();
    assert!(failures.is_empty(), "Sliders only via ui_kit::slider; numeric fields via the kit:\n{}", failures.join("\n"));
    assert_eq!(constructors, 1, "The kit must define exactly one native slider");
}

#[test]
fn slider_guard_rejects_raw_aliases_and_second_painters_without_rejecting_callers() {
    let guard = slider_guard(
        r#"
        fn caller(ui: &mut Ui) {
            crate::ui_kit::slider(ui, value, range, Default::default());
        }
        fn raw(ui: &mut Ui) {
            use egui::{Slider as Scrubber, DragValue as Number};
            ui.add(Scrubber::new(value, range));
            ui.add(Number::new(value));
            ui.add(egui::Slider::new(value, range));
        }
        fn other_slider(ui: &mut Ui) { ui.painter().add(shape); }
        fn scrub(ui: &mut Ui) {
            ui.painter().rect_filled(track, radius, color);
            ui.painter().circle_filled(at, radius, color);
        }
        fn after_tests(ui: &mut Ui) { ui.add(egui::DragValue::new(value)); }
        #[cfg(test)] mod tests { fn ignored() { ui.add(egui::Slider::new(value, range)); } }
        "#,
    );
    assert_eq!(guard.found.len(), 6, "{:?}", guard.found);
    assert_eq!(guard.constructors, 4);
}

/// Checkbox/radio policy applies to every UI/app file, even one without a kit import.
#[derive(Default)]
struct CheckGuard {
    aliases: Vec<String>,
    found: Vec<String>,
    paints: bool,
    mark: bool,
    bottom: bool,
    right_top: bool,
}

impl<'ast> Visit<'ast> for CheckGuard {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !item.attrs.iter().any(|a| a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))) {
            visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if item.attrs.iter().any(|a| a.path().is_ident("test")) {
            return;
        }
        let mut function = Self { aliases: self.aliases.clone(), ..Default::default() };
        function.visit_block(&item.block);
        let name = item.sig.ident.to_string().to_lowercase();
        if function.paints
            && (function.mark
                || (function.bottom && function.right_top)
                || ["checkbox", "checkmark", "check_mark", "check_glyph", "paint_tick"].iter().any(|s| name.contains(s)))
        {
            function.found.push(format!("{name}: check glyph painting outside ui_kit"));
        }
        self.found.extend(function.found);
    }

    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        let name = ident.to_string().to_lowercase();
        self.mark |= ["checkmark", "check_mark", "check_glyph", "tick_path", "tick_points"].iter().any(|s| name.contains(s));
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if matches!(method.as_str(), "checkbox" | "radio" | "radio_value" | "selectable_label" | "selectable_value" | "toggle_value") {
            self.found.push(format!(".{method}"));
        }
        self.paints |= matches!(method.as_str(), "line_segment" | "add" | "extend" | "text" | "galley");
        self.bottom |= method == "center_bottom";
        self.right_top |= method == "right_top";
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if names.iter().any(|n| matches!(n.as_str(), "Checkbox" | "RadioButton") || self.aliases.contains(n)) {
                self.found.push(names.join("::"));
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        // A type alias must not hide a native constructor from the call-path check.
        if path.path.segments.iter().any(|s| matches!(s.ident.to_string().as_str(), "Checkbox" | "RadioButton")) {
            self.found.push("native check/radio type outside ui_kit".into());
        }
        visit::visit_type_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // syn leaves vec! arguments opaque; the former duplicate painters put their
        // three landmarks inside that macro rather than ordinary call arguments.
        let tokens = mac.tokens.to_string();
        self.bottom |= tokens.contains("center_bottom");
        self.right_top |= tokens.contains("right_top");
        visit::visit_macro(self, mac);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if literal.value().chars().any(|c| matches!(c, '✓' | '✔' | '☑' | '✅' | '🗸' | '🗹')) {
            self.found.push("font check glyph outside ui_kit".into());
        }
    }
}

fn check_guard(source: &str) -> Vec<String> {
    #[derive(Default)]
    struct Imports(Vec<String>);
    impl<'ast> Visit<'ast> for Imports {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attrs = match item {
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Fn(i) => &i.attrs,
                _ => return visit::visit_item(self, item),
            };
            if !test_only(attrs) {
                visit::visit_item(self, item);
            }
        }
        fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
            if let syn::UseTree::Rename(rename) = tree
                && matches!(rename.ident.to_string().as_str(), "Checkbox" | "RadioButton")
            {
                self.0.push(rename.rename.to_string());
            }
            visit::visit_use_tree(self, tree);
        }
    }
    let ast = syn::parse_file(source).unwrap();
    let mut imports = Imports::default();
    imports.visit_file(&ast);
    let mut guard = CheckGuard { aliases: imports.0, ..Default::default() };
    guard.visit_file(&ast);
    guard.found
}

#[test]
fn all_ui_and_apps_use_kit_checkboxes_and_the_single_check_glyph() {
    fn scan(dir: &Path, kit: &Path, failures: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path == kit {
                continue;
            }
            if path.is_dir() {
                scan(&path, kit, failures);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let found = check_guard(&std::fs::read_to_string(&path).unwrap());
                if !found.is_empty() {
                    failures.push(format!("{}: {found:?}", path.display()));
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let kit = root.join("src/ui_kit");
    let mut failures = Vec::new();
    scan(&root.join("src"), &kit, &mut failures);
    scan(&root.join("../../apps"), &kit, &mut failures);
    failures.sort();
    assert!(failures.is_empty(), "Checkboxes only via ui_kit::checkbox; no caller allowances:\n{}", failures.join("\n"));
}

#[test]
fn check_guard_rejects_raw_controls_aliases_unicode_and_duplicate_tick_painters() {
    let found = check_guard(
        r#"
        fn raw(ui: &mut Ui) {
            use egui::{Checkbox as Check, RadioButton as Radio};
            ui.checkbox(value, "Name");
            ui.radio(true, "Name");
            ui.radio_value(value, 1, "Name");
            ui.selectable_label(on, "Name");
            ui.selectable_value(value, 1, "Name");
            ui.toggle_value(value, "Name");
            ui.add(Check::new(value, "Name"));
            ui.add(Radio::new(true, "Name"));
            ui.add(egui::Checkbox::new(value, "Name"));
            ui.label("\u{2713}");
        }
        fn paint_checkmark(ui: &mut Ui) { ui.painter().line_segment(points, stroke); }
        fn anonymous(ui: &mut Ui) {
            ui.painter().add(Shape::line(vec![r.left_center(), r.center_bottom(), r.right_top()], stroke));
        }
        fn named(ui: &mut Ui) { ui.painter().add(tick_path); }
        fn caller(ui: &mut Ui) { crate::ui_kit::checkbox(ui, "Name", value); }
        #[cfg(test)] mod tests { fn sample(ui: &mut Ui) { ui.checkbox(value, "✓"); } }
        fn after_tests(ui: &mut Ui) { ui.radio(true, "Name"); }
    "#,
    );
    assert_eq!(found.len(), 14, "{found:?}");
}

#[test]
fn check_guard_rejects_native_type_aliases() {
    assert_eq!(check_guard("type Check<'a> = egui::Checkbox<'a>; fn draw() { Check::new(value, name); }").len(), 1);
    assert_eq!(check_guard("type Radio = egui::RadioButton<'static>; fn draw() { Radio::new(true, name); }").len(), 1);
}

/// No parallel disclosure painter, font glyph, native header or raw disclosure icon.
/// Dropdowns and submenu navigation have separate semantics and remain kit/native controls.
#[derive(Default)]
struct DisclosureGuard {
    found: Vec<String>,
    aliases: Vec<String>,
    paints: bool,
    triangle: bool,
    state: bool,
    definitions: usize,
}

impl<'ast> Visit<'ast> for DisclosureGuard {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !item.attrs.iter().any(|a| a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))) {
            visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if item.attrs.iter().any(|a| a.path().is_ident("test")) {
            return;
        }
        let mut function = Self { aliases: self.aliases.clone(), ..Default::default() };
        function.visit_block(&item.block);
        let name = item.sig.ident.to_string();
        if name == "disclosure" {
            self.definitions += 1;
        }
        if function.paints && (name.contains("disclosure") || name.contains("chevron") || (function.triangle && function.state)) {
            function.found.push(format!("{name}: disclosure painting outside ui_kit"));
        }
        self.found.extend(function.found);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        // State values, not unrelated geometry names such as Shape::closed_line.
        self.state |= path.path.is_ident("open") || path.path.is_ident("closed") || path.path.is_ident("collapsed") || path.path.is_ident("expanded");
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_if(&mut self, expr: &'ast syn::ExprIf) {
        fn glyph(block: &syn::Block) -> bool {
            matches!(block.stmts.last(), Some(syn::Stmt::Expr(syn::Expr::Lit(lit), None))
                if matches!(&lit.lit, syn::Lit::Str(text) if matches!(text.value().as_str(), "«" | "»" | ">" | "<" | "v" | "^")))
        }
        #[derive(Default)]
        struct State(bool);
        impl<'ast> Visit<'ast> for State {
            fn visit_ident(&mut self, ident: &'ast syn::Ident) {
                self.0 |=
                    ident.to_string().split('_').any(|part| matches!(part, "open" | "closed" | "collapsed" | "expanded" | "folded" | "minimized"));
            }
        }
        let mut state = State::default();
        state.visit_expr(&expr.cond);
        if state.0
            && (glyph(&expr.then_branch)
                || expr.else_branch.as_ref().is_some_and(|(_, other)| matches!(other.as_ref(), syn::Expr::Block(block) if glyph(&block.block))))
        {
            self.found.push("conditional font disclosure glyph outside ui_kit".into());
        }
        visit::visit_expr_if(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        if matches!(name.as_str(), "collapsing" | "show_header" | "show_toggle_button")
            || (name == "collapsible"
                && !matches!(call.args.first(), Some(syn::Expr::Lit(lit)) if matches!(lit.lit, syn::Lit::Bool(ref b) if !b.value)))
        {
            self.found.push(format!(".{name}: native disclosure"));
        }
        self.paints |= matches!(name.as_str(), "line_segment" | "add" | "extend" | "text" | "galley" | "image");
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if names.iter().any(|n| n == "CollapsingHeader" || self.aliases.contains(n)) {
                self.found.push("native disclosure header".into());
            }
            if names.last().is_some_and(|n| n == "paint_default_icon") {
                self.found.push("native disclosure painter outside ui_kit".into());
            }
            self.triangle |= names.iter().any(|n| n == "convex_polygon");
            if names.last().is_some_and(|n| n.starts_with("paint") || n.starts_with("icon_button")) {
                #[derive(Default)]
                struct Icon(bool);
                impl<'ast> Visit<'ast> for Icon {
                    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
                        self.0 |= matches!(lit.value().as_str(), "chevron" | "chevron_right" | "disclosure_open" | "disclosure_closed");
                    }
                }
                let mut icon = Icon::default();
                for arg in &call.args {
                    icon.visit_expr(arg);
                }
                if icon.0 {
                    self.found.push("raw disclosure icon outside ui_kit".into());
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
        // Single glyphs, or a glyph-only pair used as state alternatives. Text such as
        // "▶ Play" is a playback label, not a font-based disclosure.
        if !lit.value().is_empty()
            && lit.value().chars().all(|c| matches!(c, '▾' | '▸' | '▶' | '▼' | '◀' | '▲' | '⌄' | '⌃' | '❯' | '❮' | '›' | '‹' | '∨' | '∧'))
        {
            self.found.push("font disclosure glyph outside ui_kit".into());
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // vec!/format! arguments are otherwise opaque to syn's expression visitor.
        if let Ok(args) = mac.parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated) {
            for arg in args {
                self.visit_expr(&arg);
            }
        }
    }
}

fn disclosure_guard(source: &str) -> DisclosureGuard {
    #[derive(Default)]
    struct Imports(Vec<String>);
    impl<'ast> Visit<'ast> for Imports {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attrs = match item {
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Fn(i) => &i.attrs,
                _ => return visit::visit_item(self, item),
            };
            if !test_only(attrs) {
                visit::visit_item(self, item);
            }
        }
        fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
            if let syn::UseTree::Rename(rename) = tree
                && rename.ident == "CollapsingHeader"
            {
                self.0.push(rename.rename.to_string());
            }
            visit::visit_use_tree(self, tree);
        }
    }
    let ast = syn::parse_file(source).unwrap();
    let mut imports = Imports::default();
    imports.visit_file(&ast);
    let mut guard = DisclosureGuard { aliases: imports.0, ..Default::default() };
    guard.visit_file(&ast);
    guard
}

#[test]
fn all_disclosures_use_the_one_kit_chevron_without_allowances() {
    fn scan(dir: &Path, kit: &Path, failures: &mut Vec<String>, definitions: &mut usize) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, kit, failures, definitions);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let guard = disclosure_guard(&std::fs::read_to_string(&path).unwrap());
                *definitions += guard.definitions;
                if !path.starts_with(kit) && !guard.found.is_empty() {
                    failures.push(format!("{}: {:?}", path.display(), guard.found));
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let kit = root.join("src/ui_kit");
    let mut failures = Vec::new();
    let mut definitions = 0;
    scan(&root.join("src"), &kit, &mut failures, &mut definitions);
    scan(&root.join("../../apps"), &kit, &mut failures, &mut definitions);
    scan(&root.join("../../assets/icons/ui/source"), &kit, &mut failures, &mut definitions);
    assert!(failures.is_empty(), "Disclosures only through ui_kit::disclosure:\n{}", failures.join("\n"));
    assert_eq!(definitions, 1, "Exactly one disclosure painter, including inside ui_kit");
}

#[test]
fn disclosure_guard_rejects_duplicate_icons_native_headers_font_glyphs_and_painting() {
    let found = disclosure_guard(
        r#"
        use egui::CollapsingHeader as Fold;
        fn raw(ui: &mut Ui) {
            Fold::new("Title");
            ui.collapsing("Title", |_| {});
            paint_ink(p, r, if open { "chevron" } else { "chevron_right" }, ink);
            paint_icon(p, r, "disclosure_open", 1.0);
            ui.label(if open { "\u{25bc}" } else { "\u{25b6}" });
        }
        fn paint_disclosure(ui: &mut Ui) { ui.painter().line_segment(points, stroke); }
        fn raw_triangle(ui: &mut Ui, open: bool) {
            let arrow = if open { down } else { right };
            ui.painter().add(Shape::convex_polygon(arrow, ink, Stroke::NONE));
        }
        #[cfg(test)] mod tests { fn ignored(ui: &mut Ui) { ui.collapsing("Title", |_| {}); } }
        fn after(ui: &mut Ui) { ui.collapsing("Title", |_| {}); }
        fn caller(ui: &mut Ui) { crate::ui_kit::disclosure(ui.painter(), center, open, ink); }
        fn dropdown(ui: &mut Ui) { crate::ui_kit::dropdown_indicator(ui.painter(), r, ink); }
    "#,
    )
    .found;
    assert_eq!(found.len(), 9, "{found:?}");
}

#[test]
fn disclosure_guard_distinguishes_overflow_and_playback_from_conditional_folds() {
    assert!(disclosure_guard(r#"fn show(ui: &mut Ui) { ui.menu_button("»", |_| {}); ui.label("▶ Play"); }"#).found.is_empty());
    assert_eq!(disclosure_guard(r#"fn show(ui: &mut Ui) { ui.label(if collapsed { "«" } else { "»" }); }"#).found.len(), 1);
    assert_eq!(disclosure_guard("fn show(ui: &mut Ui) { egui::collapsing_header::paint_default_icon(ui, open, response); }").found.len(), 1);
}

/// System windows and confirms may compose content, but never define another card frame.
fn window_frame_violations(source: &str) -> Vec<String> {
    parsed_frame_violations(&syn::parse_file(source).unwrap())
}

fn parsed_frame_violations(ast: &syn::File) -> Vec<String> {
    #[derive(Default)]
    struct Frames {
        found: Vec<String>,
        aliases: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Frames {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attrs = match item {
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Fn(i) => &i.attrs,
                _ => return visit::visit_item(self, item),
            };
            if !attrs
                .iter()
                .any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
            {
                visit::visit_item(self, item);
            }
        }
        fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
            if let syn::UseTree::Rename(rename) = tree
                && matches!(rename.ident.to_string().as_str(), "Frame" | "Window" | "Modal")
            {
                self.aliases.push(rename.rename.to_string());
            }
            visit::visit_use_tree(self, tree);
        }
        fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
            let names: Vec<_> = expr.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if !names.iter().any(|s| s == "ui_kit")
                && names.iter().enumerate().any(|(index, s)| {
                    matches!(s.as_str(), "shell_card" | "dock_frame")
                        || (index + 1 < names.len() && (matches!(s.as_str(), "Frame" | "Window" | "Modal") || self.aliases.contains(s)))
                })
            {
                self.found.push(names.join("::"));
            }
            visit::visit_expr_path(self, expr);
        }
    }
    let mut frames = Frames::default();
    // Collect aliases first, including imports below a caller.
    for item in &ast.items {
        if let syn::Item::Use(u) = item {
            frames.visit_use_tree(&u.tree);
        }
    }
    frames.visit_file(ast);
    frames.found
}

#[test]
fn system_windows_and_prompts_only_use_kit_frames_without_allowances() {
    fn scan(dir: &Path, failures: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_none_or(|n| n != "ui_kit") {
                    scan(&path, failures);
                }
            } else if path.extension().is_some_and(|n| n == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                if source.contains("ui_kit::Window::") || source.contains("ui_kit::Confirm::") {
                    let found = window_frame_violations(&source);
                    if !found.is_empty() {
                        failures.push(format!("{}: {found:?}", path.display()));
                    }
                }
                if path.file_name().is_none_or(|n| n != "ui_kit_guard_tests.rs") {
                    assert!(!source.contains("ui_kit::Prompt"), "{}: use Window or Confirm", path.display());
                }
            }
        }
    }
    let mut failures = Vec::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut failures);
    assert!(failures.is_empty(), "No caller-defined window frames:\n{}", failures.join("\n"));
    assert!(ALLOWED.is_empty());
}

#[test]
fn window_guard_rejects_frames_aliases_and_native_windows_but_accepts_kit_composition() {
    let found = window_frame_violations(
        r#"
        use egui::{Frame as Card, Window as Popup, Modal as Alert};
        fn raw(ui: &mut Ui) {
            let _stage = super::Stage::Frame;
            Card::NONE.show(ui, |_| {});
            egui::Frame::group(ui.style());
            Popup::new("Title"); Alert::new(id);
            crate::frame::shell_card(fill); crate::frame::dock_frame(ctx, side);
        }
        fn kit() { crate::ui_kit::Window::new(id, icon, title); crate::ui_kit::Confirm::new(id, icon, title); }
        #[cfg(test)] mod tests { fn ignored() { egui::Frame::NONE; } }
    "#,
    );
    assert_eq!(found.len(), 6, "{found:?}");
}

#[test]
fn context_and_history_menus_only_use_kit_surfaces_without_allowances() {
    for file in ["context_menu.rs", "timeline.rs", "timeline_left.rs", "toolbar.rs"] {
        let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)).unwrap();
        let mut ast = syn::parse_file(&source).unwrap();
        ast.items.retain(|item| match item {
            syn::Item::Use(_) => true,
            syn::Item::Fn(function) => {
                let name = function.sig.ident.to_string();
                file == "context_menu.rs" || name.contains("menu") || name == "dropdown_row" || name == "section_frame"
            }
            _ => false,
        });
        let found = parsed_frame_violations(&ast);
        assert!(found.is_empty(), "{file}: caller-defined menu frame: {found:?}");
        #[derive(Default)]
        struct NativeMenus(Vec<String>);
        impl<'ast> Visit<'ast> for NativeMenus {
            fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
                if !item.attrs.iter().any(|a| a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))) {
                    visit::visit_item_mod(self, item);
                }
            }
            fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
                if matches!(call.method.to_string().as_str(), "context_menu" | "menu_button") {
                    self.0.push(call.method.to_string());
                }
                visit::visit_expr_method_call(self, call);
            }
            fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
                if expr.path.segments.iter().any(|s| s.ident == "Frame")
                    && expr.path.segments.last().is_some_and(|s| s.ident == "popup" || s.ident == "menu")
                {
                    self.0.push("raw popup frame".into());
                }
                visit::visit_expr_path(self, expr);
            }
        }
        let mut guard = NativeMenus::default();
        guard.visit_file(&syn::parse_file(&source).unwrap());
        assert!(guard.0.is_empty(), "{file}: native menu bypasses: {:?}", guard.0);
    }
    assert!(ALLOWED.is_empty());
}

/// SolidCraft's dialogs, Preferences and menus draw the kit's checkbox exactly as the kit does.
mod checkbox_in_use {
    use crate::ui_kit::{self, checkbox, test_checkbox_shapes as checkbox_shapes, test_raster as raster, test_textures_update as textures_update};
    use egui::{Rect, Shape, Ui, vec2};
    use serde_json::json;
    use solidcraft_theme::Tokens;
    use std::collections::HashMap;

    fn surface(
        ui: &mut Ui,
        name: &str,
        app: &mut crate::SolveApp,
        editor: &mut crate::dialogs_motion::Mo,
        appearance: &mut crate::dialogs_appearance::Ap,
        inputs: &mut [crate::selection::SelInput],
    ) {
        ui.set_width(if name == "appearance" { 340.0 } else { 760.0 });
        match name {
            "config" => ui_kit::scope(ui, |ui| {
                let id = ui.make_persistent_id("rule_editor");
                ui.ctx().data_mut(|d| d.insert_persisted(id, true));
                crate::dialogs_motion::rows(app, ui, editor, &mut []);
            }),
            "appearance" => ui_kit::scope(ui, |ui| {
                crate::dialogs_appearance::rows(app, ui, appearance, inputs);
            }),
            "preferences" => crate::prefs::show(app, ui.ctx()),
            "menu" => {
                crate::ui_kit::menu::menu_frame(ui, |ui| crate::toolbar::selection_filter(app, ui));
            }
            _ => panic!("unknown surface {name}"),
        }
    }

    #[test]
    fn actual_config_appearance_preferences_and_menu_boxes_and_ticks_match_at_1x_2x() {
        let previous = solidcraft_theme::is_dark();
        let evidence = std::env::var_os("SOLIDCRAFT_CHECKBOX_EVIDENCE").map(std::path::PathBuf::from);
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for ppp in [1.0, 2.0] {
                let reference_ctx = crate::fonts::test_context();
                reference_ctx.set_pixels_per_point(ppp);
                let mut references = Vec::new();
                for on in [false, true] {
                    for disabled in [false, true] {
                        let mut value = on;
                        let mut output = reference_ctx.run_ui(Default::default(), |ui| {
                            ui.add_enabled_ui(!disabled, |ui| {
                                checkbox(ui, "Reference", &mut value);
                            });
                        });
                        let (square, shapes) = checkbox_shapes(&output).remove(0);
                        references.push((square, shapes.clone(), raster(&reference_ctx, shapes, ppp, square, &HashMap::new())));
                        output.textures_delta.clear();
                    }
                }
                for name in ["config", "appearance", "preferences", "menu"] {
                    let ctx = crate::fonts::test_context();
                    ctx.set_pixels_per_point(ppp);
                    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                    let mut app = crate::SolveApp::new(Default::default(), Default::default());
                    app.home.open = false;
                    app.ui.dark = dark;
                    app.run("parameters.add", json!({"name":"width", "expression":"10 mm"})).unwrap();
                    app.run("config.configure", json!({"params":["width"]})).unwrap();
                    app.run("config.rule", json!({"rule":{"id":"width_rule","name":"Width rule", "enabled":true, "condition":{"type":"always"},"effect":{"type":"drive_parameter", "parameter":"width","expression":"12 mm"}}})).unwrap();
                    let mut editor =
                        crate::dialogs_motion::Mo::Configurations { new_row: String::new(), error: None, rule_editor: Default::default() };
                    let Some((crate::dialogs::Kind::Appearance(mut appearance), mut inputs)) =
                        crate::dialogs_appearance::start(&app, "appearance.assign")
                    else {
                        panic!("appearance fixture");
                    };
                    appearance.pbr = Some(Default::default());
                    appearance.local.overwrite = true;
                    app.ui.pick_bodies = true;
                    if name == "preferences" {
                        // Find the redesigned Layout checkbox by its current visible label.
                        crate::prefs::open_at(&mut app, crate::prefs::PAGE_LAYOUT);
                        app.prefs_window.shell.filter = app.tr("prefs.layout_page.active_commands").to_string();
                    }
                    let mut textures = HashMap::new();
                    let mut final_output = None;
                    for _ in 0..3 {
                        crate::scenario::clear_handles();
                        let mut output = ctx.run_ui(
                            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 1300.0))), ..Default::default() },
                            |ui| surface(ui, name, &mut app, &mut editor, &mut appearance, &mut inputs),
                        );
                        textures_update(&mut textures, &output.textures_delta);
                        output.textures_delta.clear();
                        final_output = Some(output);
                    }
                    let output = final_output.unwrap();
                    let boxes = checkbox_shapes(&output);
                    assert!(boxes.len() >= if matches!(name, "config" | "appearance") { 2 } else { 1 }, "{name}: missing actual checkbox controls");
                    let mut ticks = 0;
                    for (square, shapes) in boxes {
                        let checked = shapes.len() > 1;
                        ticks += usize::from(checked);
                        let Shape::Rect(box_shape) = &shapes[0].shape else {
                            panic!("checkbox box");
                        };
                        let opaque_fill = if checked { Tokens::get().form_primary } else { Tokens::get().control_fill };
                        let disabled = box_shape.fill.a() < opaque_fill.a();
                        let (reference_square, reference_shapes, reference_pixels) = &references[usize::from(checked) * 2 + usize::from(disabled)];
                        // Compare the actual path, independent of a form/menu's placement.
                        if checked {
                            let (Shape::Path(actual), Shape::Path(reference)) = (&shapes[1].shape, &reference_shapes[1].shape) else {
                                panic!("tick path");
                            };
                            let normalized = |points: &[egui::Pos2], min| points.iter().map(|p| *p - min).collect::<Vec<_>>();
                            assert_eq!(
                                normalized(&actual.points, square.min),
                                normalized(&reference.points, reference_square.min),
                                "{name}: tick geometry"
                            );
                            assert_eq!(actual.stroke, reference.stroke, "{name}: tick stroke");
                        }
                        let pixels = raster(&ctx, shapes, ppp, square, &HashMap::new());
                        let max_difference = pixels.as_raw().iter().zip(reference_pixels.as_raw()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                        assert!(max_difference <= 1, "{name}: box/check pixel difference {max_difference}, dark={dark}, ppp={ppp}");
                    }
                    assert!(ticks > 0, "{name}: must exercise checked rows");
                    if let Some(dir) = &evidence {
                        std::fs::create_dir_all(dir).unwrap();
                        let mut image = raster(&ctx, output.shapes, ppp, Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 1300.0)), &textures);
                        let background = Tokens::get().dialog_body.to_array();
                        for pixel in image.pixels_mut() {
                            let remaining = 1.0 - f32::from(pixel.0[3]) / 255.0;
                            for (dest, back) in pixel.0.iter_mut().zip(background) {
                                *dest = (f32::from(*dest) + f32::from(back) * remaining).round() as u8;
                            }
                        }
                        image.save(dir.join(format!("{name}-{}-{ppp}x.png", if dark { "dark" } else { "light" }))).unwrap();
                    }
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }
}

/// The kit's disclosures draw SolidCraft's chevron artwork (the host's icon set) at token size.
mod disclosure_artwork {
    use crate::{icon_svg, scenario, ui_kit::*};
    use egui::{ColorImage, Pos2, Rect, vec2};
    use solidcraft_theme::Tokens;

    fn assert_glyph(
        ctx: &egui::Context,
        mut output: egui::FullOutput,
        textures: &std::collections::HashMap<egui::TextureId, ColorImage>,
        at: Pos2,
        open: bool,
    ) {
        let t = Tokens::get();
        let size = t.form_section_icon_size;
        let px = size.round() as u32;
        // Compare the pixels actually uploaded by the widget against the specified SVG id,
        // not a helper's return value or its state. Also check the mesh uses those pixels
        // at the control's position without the former vertical flip.
        let id = if open { "chevron" } else { "chevron_right" };
        let master = icon_svg::resolve(id).master;
        let image = icon_svg::render(
            icon_svg::doc_for(master, px).unwrap(),
            icon_svg::Look { dark: true, accent: None, vision: solidcraft_theme::vision(), palette: None },
            px,
        )
        .unwrap();
        let mut expected = ColorImage::from_rgba_premultiplied([px as usize, px as usize], image.data());
        for pixel in &mut expected.pixels {
            *pixel = t.form_dim.gamma_multiply(f32::from(pixel.a()) / 255.0);
        }
        let texture = textures
            .iter()
            .find_map(|(id, image)| (image == &expected).then_some(*id))
            .expect("widget must upload the specified disclosure SVG pixels at token size and ink");
        let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let bounds = Rect::from_center_size(at, vec2(size + 2.0, size + 2.0));
        assert!(
            meshes.iter().any(|clipped| {
                let egui::epaint::Primitive::Mesh(mesh) = &clipped.primitive else { return false };
                mesh.texture_id == texture
                    && bounds.contains_rect(mesh.calc_bounds())
                    && mesh.vertices.iter().all(|v| (v.pos.y < at.y) == (v.uv.y < 0.5))
            }),
            "specified glyph must actually be drawn, without flipping, at {at:?}"
        );
        output.textures_delta.clear();
    }

    #[test]
    fn section_card_and_floating_window_folds_render_down_when_open_right_when_closed() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for open in [false, true] {
                for surface in ["section", "card", "window"] {
                    let ctx = crate::fonts::test_context();
                    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                    let viewport = Rect::from_min_size(Pos2::ZERO, vec2(700.0, 500.0));
                    let mut textures = std::collections::HashMap::new();
                    // Windows need measuring passes before their first visible frame.
                    for pass in 0..3 {
                        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ui| {
                            ui.set_width(Tokens::get().form_card_width);
                            if surface == "section" {
                                section(ui, "section", "Advanced", open, |_| {});
                            } else if surface == "card" {
                                collapsible_header(ui, "fillet", "Fillet", &mut !open, || {});
                            } else {
                                Panel::new(
                                    "Fillet",
                                    egui::Id::unique("window"),
                                    egui::pos2(20.0, 20.0),
                                    viewport,
                                    crate::frame::Side::Left,
                                    egui::Frame::NONE,
                                )
                                .window(&ctx)
                                .show(ui.ctx(), |ui| collapsible_header(ui, "fillet", "Fillet", &mut !open, || {}));
                            }
                        });
                        for (id, deltas) in &output.textures_delta.set {
                            for delta in deltas {
                                let egui::ImageData::Color(image) = &delta.image;
                                if delta.pos.is_none() {
                                    textures.insert(*id, image.as_ref().clone());
                                }
                            }
                        }
                        output.textures_delta.clear();
                        if pass != 2 {
                            continue;
                        }
                        let at = if surface == "section" {
                            let rect = scenario::rect_of("dialog:section:Advanced").unwrap();
                            egui::pos2(rect.left() + Tokens::get().form_section_icon_size / 2.0, rect.center().y)
                        } else {
                            scenario::handle_at("dialog:collapse").unwrap()
                        };
                        assert_glyph(&ctx, output, &textures, at, open);
                    }
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn strip_fold_and_restore_share_the_chevron_texture_with_opposite_orientation() {
        for expanded in [true, false] {
            let ctx = crate::fonts::test_context();
            let mut rect = Rect::NOTHING;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                rect = strip_toggle(ui, expanded, "Shots").rect;
            });
            let icon =
                Rect::from_min_size(rect.min, vec2(Tokens::get().control_height, Tokens::get().control_height)).shrink(Tokens::get().form_icon_inset);
            let size = icon.width().round() as u32;
            let expected = icon_svg::render(
                icon_svg::doc_for(icon_svg::resolve("chevron").master, size).unwrap(),
                icon_svg::Look { dark: true, accent: None, vision: solidcraft_theme::vision(), palette: None },
                size,
            )
            .unwrap();
            let mut expected = ColorImage::from_rgba_premultiplied([size as usize, size as usize], expected.data());
            for pixel in &mut expected.pixels {
                *pixel = Tokens::get().form_dim.gamma_multiply(f32::from(pixel.a()) / 255.0);
            }
            let mut textures = std::collections::HashMap::new();
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            let texture = textures.iter().find_map(|(id, image)| (image == &expected).then_some(*id)).expect("shared chevron artwork");
            let primitives = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
            assert!(
                primitives.iter().any(|primitive| {
                    let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else { return false };
                    mesh.texture_id == texture
                        && mesh.vertices.iter().all(|vertex| ((vertex.pos.y < icon.center().y) == (vertex.uv.y < 0.5)) == expanded)
                }),
                "hide points down and restore points up"
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn panel_disclosure_blends_only_ink_and_reduced_motion_finishes_immediately() {
        for scale in [1.0, 2.0] {
            let ctx = crate::fonts::test_context();
            ctx.set_pixels_per_point(scale);
            ctx.enable_accesskit();
            ctx.global_style_mut(|style| style.animation_time = 0.14);
            let id = egui::Id::unique("disclosure-transition");
            let bounds = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(28.0, 28.0));
            let frame = |time, collapsed, reduced| {
                let output = crate::test_support::frame(&ctx, egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                    let response = crate::ui_kit::panel_host_disclosure(ui, id, bounds, "Toggle panel", true, collapsed, reduced);
                    assert_eq!(response.rect, bounds);
                });
                let node = &output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|(node_id, _)| *node_id == id.accesskit_id())
                    .unwrap()
                    .1;
                assert_eq!(node.is_expanded(), Some(!collapsed), "semantics commit before the paint transition");
                assert!(output.shapes.iter().all(|shape| shape.clip_rect.contains_rect(bounds)), "paint keeps the inherited clip");
                output.shapes.iter().filter(|shape| matches!(shape.shape, egui::Shape::Mesh(_))).count()
            };
            assert_eq!(frame(0.0, true, false), 1);
            frame(1.0 / 60.0, false, false);
            assert_eq!(frame(2.0 / 60.0, false, false), 2, "only the existing down/up glyphs crossfade");
            for tick in 3..14 {
                frame(f64::from(tick) / 60.0, false, false);
            }
            assert_eq!(frame(14.0 / 60.0, false, false), 1);
            assert_eq!(frame(15.0 / 60.0, true, false), 2, "collapse also transitions without delaying expanded state");
            for tick in 16..28 {
                frame(f64::from(tick) / 60.0, true, false);
            }
            assert_eq!(frame(28.0 / 60.0, true, false), 1);
            assert_eq!(frame(29.0 / 60.0, false, true), 1, "reduced motion paints the committed endpoint immediately");
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            assert_eq!(frame(30.0 / 60.0, true, false), 1, "disabled animation also paints the endpoint");
        }
    }
}

/// String literals in a file's non-test code that are one of the host's `names`, whole or as a
/// dotted/colon-separated part of a longer literal (`menu:view.render_dock`).
struct HostNames<'n> {
    names: &'n std::collections::BTreeSet<String>,
    found: Vec<String>,
}

impl<'ast> Visit<'ast> for HostNames<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            _ => return visit::visit_item(self, item),
        };
        if !test_only(attrs) {
            visit::visit_item(self, item);
        }
    }

    /// Macro bodies (`format!`, a table macro's rows) are tokens, not expressions: read their
    /// string literals too.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        fn literals(tokens: proc_macro2::TokenStream, out: &mut Vec<syn::LitStr>) {
            for token in tokens {
                match token {
                    proc_macro2::TokenTree::Group(group) => literals(group.stream(), out),
                    proc_macro2::TokenTree::Literal(literal) => {
                        if let Ok(literal) = syn::parse2::<syn::LitStr>(proc_macro2::TokenTree::Literal(literal).into()) {
                            out.push(literal);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        literals(mac.tokens.clone(), &mut found);
        for literal in &found {
            self.visit_lit_str(literal);
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        let value = literal.value();
        let parts = value.split(|c: char| c.is_whitespace() || matches!(c, ':' | '/' | ',' | '(' | ')' | '"'));
        if let Some(name) = std::iter::once(value.as_str()).chain(parts).find(|part| self.names.contains(*part)) {
            self.found.push(name.to_owned());
        }
    }
}

fn host_names_in(source: &str, names: &std::collections::BTreeSet<String>) -> Vec<String> {
    let mut guard = HostNames { names, found: Vec::new() };
    guard.visit_file(&syn::parse_file(source).unwrap());
    guard.found
}

/// SolidCraft's tab names and every command id in the engine's registry: what a shared crate
/// (the theme) and the kit and its components must take from the host as data.
fn host_names() -> std::collections::BTreeSet<String> {
    let mut names: std::collections::BTreeSet<String> = crate::workspace::TAB_ACCENTS.names().map(str::to_owned).collect();
    names.extend(solidcraft_engine::command_specs().into_iter().map(|spec| spec.id.to_owned()));
    names
}

/// Host names still in a component, until they move to the host as data. Only shrinks: the
/// test fails on a listed name the file no longer holds.
const PENDING_HOST_NAMES: &[(&str, &[&str])] = &[];

#[test]
fn shared_code_holds_no_host_tab_names_or_command_ids() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let names = host_names();
    let mut failures = Vec::new();
    fn scan(path: &Path, relative: &str, names: &std::collections::BTreeSet<String>, failures: &mut Vec<String>) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                scan(&path, &format!("{relative}/{name}"), names, failures);
            }
            return;
        }
        // Test files (the kit's gallery among them) may use any sample data.
        if path.extension().is_none_or(|e| e != "rs") || relative.ends_with("_tests.rs") {
            return;
        }
        let mut found = host_names_in(&std::fs::read_to_string(path).unwrap(), names);
        let pending = PENDING_HOST_NAMES.iter().find(|(file, _)| *file == relative).map_or(&[][..], |(_, names)| *names);
        found.retain(|name| !pending.contains(&name.as_str()));
        if !found.is_empty() {
            failures.push(format!("{relative}: {found:?}"));
        }
    }
    let src = manifest.join("src");
    scan(&manifest.join("../theme/src"), "crates/theme/src", &names, &mut failures);
    scan(&src.join("ui_kit"), "ui_kit", &names, &mut failures);
    for component in [
        "app_bar.rs",
        "blur.rs",
        "blur_native.rs",
        "frame.rs",
        "history_bar.rs",
        "navigation_bar.rs",
        "prefs_window.rs",
        "ribbon.rs",
        "start_page.rs",
        "theme_files.rs",
        "theme_options.rs",
        "view_compass.rs",
        "window_chrome.rs",
        "window_chrome",
    ] {
        scan(&src.join(component), component, &names, &mut failures);
    }
    for (file, pending) in PENDING_HOST_NAMES {
        let held = host_names_in(&std::fs::read_to_string(src.join(file)).unwrap(), &names);
        for name in *pending {
            if !held.iter().any(|found| found == name) {
                failures.push(format!("{file}: {name} is gone; drop it from PENDING_HOST_NAMES"));
            }
        }
    }
    failures.sort();
    assert!(failures.is_empty(), "shared code names the host's tabs or commands; take them from the host as data:\n{}", failures.join("\n"));
}

#[test]
fn host_name_guard_finds_tab_names_and_command_ids_but_skips_tests() {
    let names = host_names();
    assert!(names.contains("SKETCH") && names.contains("solid.extrude"));
    let source = r#"
        fn tab() -> &'static str { "SKETCH" }
        fn item() -> String { format!("{}", "menu:solid.extrude") }
        fn neutral() -> &'static str { "sample.action_a" }
        #[cfg(test)]
        mod tests { fn sample() -> &'static str { "MODEL" } }
        #[test]
        fn sample() { let _ = "solid.fillet"; }
    "#;
    assert_eq!(host_names_in(source, &names), ["SKETCH", "solid.extrude"]);
}

/// Message-id prefixes of the kit's own vocabulary: the generic words its widgets say
/// themselves (OK, Undo, the History controls), the colour picker's, the search field's and the
/// navigation bar customiser's. Every other id in the host's catalogs names the host's screens.
const KIT_MESSAGE_PREFIXES: &[&str] = &["color.", "navbar.custom.", "search.", "ui."];

/// The message ids (`area.thing`) of SolidCraft's English catalog outside the kit's vocabulary:
/// labels the kit must take from the host as data.
fn host_message_ids() -> std::collections::BTreeSet<String> {
    let english = crate::i18n::LOCALES.first().expect("the English catalog");
    english
        .messages()
        .keys()
        .filter(|key| {
            let mut parts = key.split('.');
            let dotted = key.contains('.')
                && parts.all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
            dotted && !KIT_MESSAGE_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
        })
        .map(|key| key.to_string())
        .collect()
}

#[test]
fn kit_code_names_no_host_message_ids() {
    let names = host_message_ids();
    let mut failures = Vec::new();
    fn scan(path: &Path, relative: &str, names: &std::collections::BTreeSet<String>, failures: &mut Vec<String>) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                scan(&path, &format!("{relative}/{name}"), names, failures);
            }
            return;
        }
        if path.extension().is_none_or(|e| e != "rs") || relative.ends_with("_tests.rs") {
            return;
        }
        let found = host_names_in(&std::fs::read_to_string(path).unwrap(), names);
        if !found.is_empty() {
            failures.push(format!("{relative}: {found:?}"));
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    scan(&src.join("ui_kit"), "ui_kit", &names, &mut failures);
    // The components that move with the kit: the theme options and the navigation bar.
    for component in ["navigation_bar.rs", "theme_files.rs", "theme_options.rs"] {
        scan(&src.join(component), component, &names, &mut failures);
    }
    failures.sort();
    assert!(failures.is_empty(), "kit code names the host's messages; take the text from the host as data:\n{}", failures.join("\n"));
}

#[test]
fn message_id_guard_finds_host_ids_but_skips_kit_vocabulary_and_tests() {
    let names = host_message_ids();
    assert!(names.contains("prefs.blur_off") && !names.contains("ui.undo") && !names.contains("Invert"));
    let source = r#"
        fn host() -> &'static str { solidcraft_ui_text::tr("prefs.blur_off") }
        fn kit() -> &'static str { solidcraft_ui_text::tr("ui.undo") }
        fn handle() -> String { format!("prefs:blur_cost:{}", "ai.prefs.pointer_preview_hidden") }
        #[cfg(test)]
        mod tests { fn sample() -> &'static str { "sketch_text.bold" } }
    "#;
    assert_eq!(host_names_in(source, &names), ["prefs.blur_off", "ai.prefs.pointer_preview_hidden"]);
}
