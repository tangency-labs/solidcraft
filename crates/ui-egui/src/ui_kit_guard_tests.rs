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
                && names.iter().any(|s| matches!(s.as_str(), "Frame" | "Window" | "Modal" | "shell_card" | "dock_frame") || self.aliases.contains(s))
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
