//! Mandatory source-level boundary for the universal UI container. Runs in fast CI.
use std::path::Path;
use syn::visit::{self, Visit};

/// Every production UI host, including nested modules and platform apps, goes through the
/// universal container. Parse Rust rather than searching strings: aliases cannot bypass this.
#[derive(Default)]
struct ContainerGuard {
    modules: Vec<String>,
    aliases: Vec<String>,
    frames: Vec<String>,
    found: Vec<String>,
}

impl ContainerGuard {
    fn host(name: &str) -> bool {
        matches!(name, "Area" | "Window" | "Modal" | "Panel" | "SidePanel" | "TopBottomPanel" | "CentralPanel" | "Popup" | "SubMenu")
    }

    fn raw_path(&self, names: &[String]) -> bool {
        names.iter().any(|n| self.aliases.contains(n) || self.frames.contains(n))
            || (names.iter().any(|n| n == "egui" || self.modules.contains(n)) && names.iter().any(|n| Self::host(n) || n == "Frame"))
    }

    fn host_path(&self, names: &[String]) -> bool {
        self.raw_path(names) && names.iter().any(|n| Self::host(n) || self.aliases.contains(n))
    }

    // Default-deny host APIs, so an egui update cannot silently add another constructor.
    // Only operations that inspect/control existing popup state are allowed outside the kit.
    fn state_operation(name: &str) -> bool {
        matches!(
            name,
            "is_any_open"
                | "is_id_open"
                | "default_response_id"
                | "id_from_widget_id"
                | "open_id"
                | "toggle_id"
                | "close_all"
                | "close_id"
                | "position_of_id"
        )
    }

    fn content_frame(&self, names: &[String]) -> bool {
        names.iter().any(|n| n == "Frame" || self.frames.contains(n)) && names.last().is_some_and(|n| matches!(n.as_str(), "new" | "default"))
    }

    fn constructor(name: &str) -> bool {
        matches!(name, "new" | "default" | "default_area" | "left" | "right" | "top" | "bottom" | "menu" | "context_menu" | "popup" | "window")
    }

    // syn intentionally leaves macro bodies opaque. Inspect tokens too, including macro_rules
    // definitions, so moving a constructor into a macro does not hide it from the boundary.
    fn macro_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            if let proc_macro2::TokenTree::Group(group) = token {
                self.macro_tokens(group.stream());
            }
            let proc_macro2::TokenTree::Ident(ident) = token else { continue };
            let mut names = vec![ident.to_string()];
            let mut rest = tokens.get(index + 1..).unwrap_or_default();
            while let [proc_macro2::TokenTree::Punct(a), proc_macro2::TokenTree::Punct(b), proc_macro2::TokenTree::Ident(name), tail @ ..] = rest {
                if a.as_char() != ':' || b.as_char() != ':' {
                    break;
                }
                names.push(name.to_string());
                rest = tail;
            }
            if self.raw_path(&names)
                && !self.content_frame(&names)
                && names.last().is_some_and(|n| if self.host_path(&names) { !Self::state_operation(n) } else { Self::constructor(n) })
            {
                self.found.push(format!("raw host in macro: {}", names.join("::")));
            }
            if tokens.get(index.wrapping_sub(1)).is_some_and(|t| matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == '.'))
                && matches!(ident.to_string().as_str(), "menu_button" | "menu_image_button" | "context_menu" | "shadow")
            {
                self.found.push(format!("raw popup/frame shorthand in macro: {ident}"));
            }
        }
    }

    fn imported(&mut self, tree: &syn::UseTree, egui: bool) {
        match tree {
            syn::UseTree::Path(p) => self.imported(
                &p.tree,
                p.ident == "egui"
                    || self.modules.contains(&p.ident.to_string())
                    || (egui && matches!(p.ident.to_string().as_str(), "containers" | "panel" | "window" | "area" | "popup" | "modal" | "frame")),
            ),
            syn::UseTree::Group(g) => {
                for item in &g.items {
                    self.imported(item, egui);
                }
            }
            syn::UseTree::Name(n)
                if egui && matches!(n.ident.to_string().as_str(), "containers" | "panel" | "window" | "area" | "popup" | "modal" | "frame") =>
            {
                self.modules.push(n.ident.to_string())
            }
            syn::UseTree::Name(n) if egui && n.ident == "Frame" => self.frames.push(n.ident.to_string()),
            syn::UseTree::Rename(r) if egui && r.ident == "Frame" => self.frames.push(r.rename.to_string()),
            syn::UseTree::Name(n) if egui && Self::host(&n.ident.to_string()) => self.aliases.push(n.ident.to_string()),
            syn::UseTree::Rename(r) if egui && Self::host(&r.ident.to_string()) => self.aliases.push(r.rename.to_string()),
            syn::UseTree::Rename(r)
                if r.ident == "egui"
                    || self.modules.contains(&r.ident.to_string())
                    || (egui
                        && matches!(
                            r.ident.to_string().as_str(),
                            "self" | "containers" | "panel" | "window" | "area" | "popup" | "modal" | "frame"
                        )) =>
            {
                self.modules.push(r.rename.to_string())
            }
            syn::UseTree::Glob(_) if egui => self.found.push("egui glob import hides container constructors".into()),
            _ => {}
        }
    }
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
}

impl<'ast> Visit<'ast> for ContainerGuard {
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

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        if item.ident == "egui" {
            if item.rename.is_some() {
                self.found.push("raw egui crate alias bypasses the shared factory".into());
            }
            self.modules.push(item.rename.as_ref().map_or_else(|| item.ident.to_string(), |(_, name)| name.to_string()));
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let before = (self.aliases.len(), self.modules.len(), self.frames.len());
        self.imported(&item.tree, false);
        // Private imports can leak into child modules too. Reject them at the boundary,
        // rather than depending on how another source file spells the imported name.
        if self.aliases.len() > before.0 || self.modules.len() > before.1 {
            self.found.push(format!(
                "raw host/module imports bypass the shared factory: {:?} {:?}",
                self.aliases.get(before.0..).unwrap_or_default(),
                self.modules.get(before.1..).unwrap_or_default()
            ));
        } else if !matches!(item.vis, syn::Visibility::Inherited) && self.frames.len() > before.2 {
            self.found.push("raw frame re-export bypasses the shared API".into());
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let syn::Pat::Type(pat) = &local.pat
            && let syn::Type::Path(path) = pat.ty.as_ref()
        {
            let names = path.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>();
            if self.raw_path(&names) && names.iter().any(|n| Self::host(n) || self.aliases.contains(n)) {
                self.found.push("raw host local type bypasses the shared factory".into());
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if let syn::Type::Path(path) = item.ty.as_ref() {
            let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if names.iter().any(|n| Self::host(n) || self.aliases.contains(n))
                && names.iter().any(|n| n == "egui" || self.modules.contains(n) || self.aliases.contains(n))
            {
                self.found.push(format!("raw host type alias {}", item.ident));
            }
        }
        visit::visit_item_type(self, item);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
        let constructor = names.last().is_some_and(|n| Self::constructor(n));
        let qualified = path.qself.as_ref().is_some_and(|q| {
            if let syn::Type::Path(p) = q.ty.as_ref() {
                self.raw_path(&p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>())
            } else {
                false
            }
        });
        let raw = self.raw_path(&names) || qualified;
        let blocked = names.last().is_some_and(|n| if self.host_path(&names) || qualified { !Self::state_operation(n) } else { constructor });
        if blocked && raw && !self.content_frame(&names) {
            self.found.push(names.join("::"));
        }
        if names.iter().any(|n| matches!(n.as_str(), "shell_card" | "dock_frame")) {
            self.found.push(names.join("::"));
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.macro_tokens(mac.tokens.clone());
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        let names = expr.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>();
        if self.raw_path(&names) && names.last().is_some_and(|n| Self::host(n) || self.aliases.contains(n)) {
            self.found.push(format!("raw host struct literal {}", names.join("::")));
        }
        visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        fn container_root(expr: &syn::Expr) -> bool {
            match expr {
                syn::Expr::MethodCall(call) => container_root(&call.receiver),
                syn::Expr::Call(call) => matches!(call.func.as_ref(), syn::Expr::Path(p) if p.path.segments.iter().any(|s| s.ident == "Container")),
                _ => false,
            }
        }
        if call.method == "frame" && container_root(&call.receiver) {
            struct RawFrame<'a> {
                aliases: &'a [String],
                found: bool,
            }
            impl<'ast> Visit<'ast> for RawFrame<'_> {
                fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
                    self.found |= expr.path.segments.iter().any(|s| s.ident == "Frame" || self.aliases.contains(&s.ident.to_string()));
                }
            }
            let mut frame = RawFrame { aliases: &self.frames, found: false };
            for arg in &call.args {
                frame.visit_expr(arg);
            }
            if frame.found {
                self.found.push("caller-defined frame on shared outer host".into());
            }
        }
        if matches!(call.method.to_string().as_str(), "context_menu" | "menu_button" | "menu_image_button") {
            let shared = if let syn::Expr::Call(receiver) = call.receiver.as_ref() {
                if let syn::Expr::Path(path) = receiver.func.as_ref() { path.path.segments.iter().any(|s| s.ident == "Container") } else { false }
            } else {
                false
            };
            if !shared {
                self.found.push(format!("raw popup shorthand .{}", call.method));
            }
        }
        if call.method == "shadow" {
            self.found.push("caller-defined outer shadow".into());
        }
        visit::visit_expr_method_call(self, call);
    }
}

fn violations(source: &str) -> Result<Vec<String>, String> {
    let ast = syn::parse_file(source).map_err(|e| format!("cannot parse Rust source: {e}"))?;
    // Collect aliases before visiting calls, also when a use appears after its consumer.
    struct Imports(ContainerGuard);
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
        fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
            self.0.visit_item_extern_crate(item);
        }
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.0.imported(&item.tree, false);
        }
    }
    let mut imports = Imports(ContainerGuard::default());
    loop {
        let before = (imports.0.aliases.len(), imports.0.modules.len(), imports.0.frames.len());
        imports.visit_file(&ast);
        imports.0.aliases.sort();
        imports.0.aliases.dedup();
        imports.0.modules.sort();
        imports.0.modules.dedup();
        imports.0.frames.sort();
        imports.0.frames.dedup();
        if before == (imports.0.aliases.len(), imports.0.modules.len(), imports.0.frames.len()) {
            break;
        }
    }
    let mut guard = imports.0;
    guard.visit_file(&ast);
    guard.found.sort();
    guard.found.dedup();
    Ok(guard.found)
}

/// Scan current and future crates/apps, including nested source and examples.
/// The implementation module is the sole exception; feature-specific allowances are forbidden.
pub fn run(root: &Path) -> Result<(), String> {
    fn scan(root: &Path, dir: &Path, failures: &mut Vec<String>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                if !matches!(entry.file_name().to_str(), Some("target" | ".git")) {
                    scan(root, &path, failures)?;
                }
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.components().any(|c| c.as_os_str() == "src" || c.as_os_str() == "examples")
                && path != root.join("crates/ui-egui/src/ui_kit/container.rs")
            {
                let source = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let found = violations(&source).map_err(|e| format!("{}: {e}", path.display()))?;
                if !found.is_empty() {
                    failures.push(format!("{}: {}", path.strip_prefix(root).unwrap_or(&path).display(), found.join(", ")));
                }
            }
        }
        Ok(())
    }
    let mut failures = Vec::new();
    for dir in ["crates", "apps"] {
        scan(root, &root.join(dir), &mut failures)?;
    }
    failures.sort();
    if !failures.is_empty() {
        return Err(format!("Use ui_kit::Container for outer hosts:\n{}", failures.join("\n")));
    }
    println!("ui-containers: all outer hosts use the shared API");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn container_violations(source: &str) -> Vec<String> {
        violations(source).unwrap()
    }
    #[test]
    fn container_guard_rejects_aliases_nested_modules_and_preserves_state_queries() {
        let bad = container_violations(
            r#"
        use egui::{Area as Floating, Frame as Card};
        mod nested {
            use egui::containers::Window as Dialog;
            fn show() { Dialog::new("Title"); }
        }
        fn show() {
            Floating::new(id); egui::Panel::left("side");
            egui::Modal::new(id); Card::popup(style);
            egui::CentralPanel::default(); frame.shadow(shadow);
        }
    "#,
        );
        assert_eq!(bad.len(), 9, "{bad:?}");
        assert!(
            container_violations(
                r#"
        fn show() {
            ui_kit::Container::new(ctx, ui_kit::Surface::Panel).area(id);
            ui_kit::Window::new(id, icon, title);
            egui::Popup::is_any_open(ctx);
            egui::containers::panel::PanelState::load(ctx, id);
        }
        #[cfg(test)] mod tests { fn fixture() { egui::Window::new("test"); } }
    "#
            )
            .is_empty()
        );
    }

    #[test]
    fn container_guard_catches_module_and_type_aliases_and_popup_shorthand() {
        let bad = container_violations(
            r#"
        use egui as gui;
        use eframe::egui as toolkit;
        use gui::Area as Other;
        type Raw = gui::Window<'static>;
        fn show(ui: &mut Ui) {
            gui::Area::new(id); toolkit::Panel::right(id); Other::new(id);
            response.context_menu(|ui| {}); ui.menu_button("More", |ui| {});
        }
    "#,
        );
        assert_eq!(bad.len(), 9, "{bad:?}");
    }

    #[test]
    fn container_guard_rejects_replacing_the_shared_outer_frame_but_allows_content_groups() {
        assert_eq!(
            container_violations(
                r#"
        fn show() {
            ui_kit::Container::new(ctx, ui_kit::Surface::Panel).edge(id, edge).frame(egui::Frame::NONE.fill(fill));
        }
    "#
            )
            .len(),
            1
        );
        assert!(
            container_violations(
                r#"
        fn show(ui: &mut Ui) { egui::Frame::NONE.inner_margin(8).show(ui, |ui| {}); }
    "#
            )
            .is_empty()
        );
    }

    #[test]
    fn container_guard_rejects_raw_host_reexports() {
        assert_eq!(container_violations("pub(crate) use egui::Window as OtherHost;").len(), 1);
    }

    #[test]
    fn constructors_cannot_hide_in_function_values_ufcs_macros_or_struct_literals() {
        for source in [
            "fn show() { let make = egui::Window::new; make(title); }",
            "fn show() { egui::Popup::from_response(response); }",
            "fn show() { egui::Window::from_viewport(id, viewport); }",
            "fn show() { egui::Window::future_constructor(title); }",
            "use egui::Area as Alias; mod child;",
            "use egui as gui; mod child;",
            "fn show() { let panel: egui::CentralPanel = Default::default(); }",
            "extern crate egui as gui; fn show() { gui::Area::new(id); }",
            "use egui::containers; fn show() { containers::Window::new(title); }",
            "pub use egui::Frame as Unchecked;",
            "fn show() { <egui::CentralPanel as Default>::default(); }",
            "macro_rules! panel { () => { egui::Area::new(id) }; }",
            "fn show() { wrapper!({ egui::Window::new(title); }); }",
            "fn show() { wrapper!(ui.menu_button(title, content)); }",
            "fn show() { egui::Popup { anchor: x }; }",
            "use egui::containers::window as windows; fn show() { windows::Window::new(title); }",
            "use a::Area as B; use b as a; use c as b; use egui as c; fn show() { B::new(id); }",
        ] {
            assert!(!container_violations(source).is_empty(), "bypass: {source}");
        }
        assert!(container_violations("fn show() { wrapper!(egui::Frame::new().show(ui, body)); }").is_empty());
        assert!(violations("invalid Rust {{").is_err(), "unparseable source fails closed");
    }

    #[test]
    fn new_packages_examples_and_nested_modules_cannot_escape_the_gate() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("solidcraft-container-guard-{}-{unique}", std::process::id()));
        let source = root.join("crates/future-ui/src/nested/panel.rs");
        let example = root.join("apps/future-platform/examples/viewer.rs");
        let implementation = root.join("crates/ui-egui/src/ui_kit/container.rs");
        for path in [&source, &example, &implementation] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&source, "fn show() { egui::Area::new(id); }").unwrap();
        std::fs::write(&example, "fn main() { egui::Window::new(title); }").unwrap();
        std::fs::write(&implementation, "fn factory() { egui::Window::new(title); }").unwrap();
        let rejected = run(&root).unwrap_err();
        assert!(rejected.contains("future-ui/src/nested/panel.rs"));
        assert!(rejected.contains("future-platform/examples/viewer.rs"));
        assert!(!rejected.contains("ui_kit/container.rs"), "only the implementation is exempt");
        for path in [&source, &example] {
            std::fs::write(path, "fn show() { Container::new(ctx, Surface::Panel).area(id); }").unwrap();
        }
        run(&root).unwrap();
        std::fs::write(&source, "invalid Rust {{").unwrap();
        assert!(run(&root).unwrap_err().contains("cannot parse Rust source"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_workspace_hosts_use_the_container() {
        run(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }
}
